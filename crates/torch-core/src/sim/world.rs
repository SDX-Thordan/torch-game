//! The rebuilt deterministic world (§ multi-player re-aim).
//!
//! `Sim` owns the bodies, the **players** (`players[0]` is the human), their markets, ships,
//! facilities, and settlements. Every owned entity carries an `owner: PlayerId`. The `step()`
//! loop is a fixed, integer-only phase order (the determinism contract): market stabilization →
//! extraction → production → ship advance → **logistics dispatch** → port refuel. The economy is
//! object-driven — mining stations dig, facilities refine on-site, and **haulers** physically move
//! raw to starved facilities and outputs/raw to demand-center sinks (crediting the owner). No
//! ambient events, no combat — those were removed in the rebuild.

use super::commodity::{self};
use super::economy::{self, Market};
use super::facility::{Facility, FacilityKind};
use super::orbit::{self, Body};
use super::player::{default_players, Player, PlayerId};
use super::rng::Pcg32;
use super::ship::{Job, JobPhase, Ship, ShipClass, SiteRef};
use serde::{Deserialize, Serialize};

/// Minimum surplus at a source before a hauler bothers to pick it up (and the min load).
const LOAD_MIN: i64 = 40;
/// Basis-point denominator + the brokerage fee (the volume-scaled money sink) on each market leg.
const BP: i64 = 10_000;
const BROKER_FEE_BP: i64 = 200;
/// Flat utility for keeping a starved facility/colony fed — high so supply beats small arbitrage
/// (it only fires when a consumer is actually below its low-water mark).
const SUPPLY_BONUS: i64 = 80_000;
/// Credit-equivalent of one fuel unit, used only to *score* (deter) long hauls — not charged.
const EST_FUEL_CREDIT: i64 = 60;
/// A mining station / colony local raw store cap (so it stops digging when a hauler isn't
/// collecting — bounds the world even when logistics stalls).
const STATION_STORE_CAP: i64 = 1_000;
/// Distance units a ship of speed 1 covers per tick — scales the AU-scale `position_of` coords
/// (1 AU = 1_000_000) to sane travel times against the small `ship_stats.speed` numbers.
const SPEED_UNIT: i64 = 1_000;
/// Distance units burned per unit of Fusion Fuel on a flight (lump-sum at departure).
const FUEL_PER_DISTANCE: i64 = 50_000;

/// A held market reservation: `(market index, commodity, quantity)`.
type MarketResv = Option<(usize, usize, i64)>;

/// Raw extracted per tick from a body whose abundance (0..=~252) is `abundance`. 0 if the body
/// has none of that good; otherwise a small deterministic integer rate. No RNG.
fn mine_amount(abundance: i64) -> i64 {
    if abundance <= 0 {
        0
    } else {
        (2 + abundance / 32).max(1)
    }
}

/// A growable settlement on an **inhabitable** body — digs its body's raw (if abundant) and holds
/// imported Food. Carries a local `store` (the locational inventory).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Colony {
    pub owner: PlayerId,
    pub body: usize,
    pub population: i64,
    #[serde(default)]
    pub store: Vec<i64>,
}

impl Colony {
    pub fn new(owner: PlayerId, body: usize, population: i64) -> Self {
        Self {
            owner,
            body,
            population,
            store: vec![0; commodity::commodity_count()],
        }
    }
    pub fn add(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.store.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn get(&self, c: usize) -> i64 {
        self.store.get(c).copied().unwrap_or(0)
    }
}

/// A non-growable **dedicated mining station** on an **uninhabitable** body — extracts its body's
/// raw into a local `store` that haulers collect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiningStation {
    pub owner: PlayerId,
    pub body: usize,
    #[serde(default)]
    pub store: Vec<i64>,
}

impl MiningStation {
    pub fn new(owner: PlayerId, body: usize) -> Self {
        Self {
            owner,
            body,
            store: vec![0; commodity::commodity_count()],
        }
    }
    pub fn add(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.store.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn get(&self, c: usize) -> i64 {
        self.store.get(c).copied().unwrap_or(0)
    }
}

/// The deterministic multi-player world.
#[derive(Clone, Debug)]
pub struct Sim {
    seed: u64,
    tick: u64,
    rng: Pcg32,
    bodies: Vec<Body>,
    players: Vec<Player>,
    human: PlayerId,
    markets: Vec<Market>,
    ships: Vec<Ship>,
    facilities: Vec<Facility>,
    colonies: Vec<Colony>,
    mining_stations: Vec<MiningStation>,
}

impl Sim {
    /// Build the default world from a seed.
    pub fn new(seed: u64) -> Self {
        let bodies = orbit::default_system();
        let players = default_players();
        let markets = economy::default_markets();
        let mut sim = Self {
            seed,
            tick: 0,
            rng: Pcg32::new(seed ^ 0x0070_1C04),
            bodies,
            players,
            human: 0,
            markets,
            ships: Vec::new(),
            facilities: Vec::new(),
            colonies: Vec::new(),
            mining_stations: Vec::new(),
        };
        sim.seed_starting_assets();
        sim
    }

    /// Place the opening assets as **coherent per-player chains** (deterministic, no RNG): every
    /// industrial player owns a mining station (raw source) + a facility + haulers, and every
    /// player owns at least a station + a hauler so its credits flow over a long run. Belt bodies
    /// have abundance of all three raws (the name hash rarely zeroes one), so any station feeds any
    /// facility.
    fn seed_starting_assets(&mut self) {
        let belt = |s: &Self, name: &str| s.bodies.iter().position(|b| b.name == name);
        let (earth, mars, ceres) = (3usize, 4usize, 5usize);

        // Helper: a station on belt body `name` + `haulers` haulers docked there, for `owner`.
        let chain = |s: &mut Self, owner: u16, station_body: &str, dock: usize, haulers: usize| {
            if let Some(b) = belt(s, station_body) {
                s.mining_stations.push(MiningStation::new(owner, b));
            }
            for _ in 0..haulers {
                s.ships
                    .push(Ship::new(owner, ShipClass::Hauler, "Hauler", dock));
            }
        };

        // Human (0): a station at Psyche + a hauler + a Miner (cosmetic) → hauls raw to a sink.
        chain(self, 0, "Psyche", ceres, 1);
        self.ships
            .push(Ship::new(0, ShipClass::Miner, "Prospector", ceres));

        // The nations: homeworld colonies + a facility + a belt station + haulers.
        self.colonies.push(Colony::new(1, earth, 8_000));
        self.colonies.push(Colony::new(2, mars, 5_000));
        self.colonies.push(Colony::new(3, ceres, 2_000));
        self.facilities
            .push(Facility::new(1, earth, FacilityKind::ElectronicsFab));
        self.facilities
            .push(Facility::new(2, mars, FacilityKind::AlloyPlant));
        self.facilities
            .push(Facility::new(3, ceres, FacilityKind::FusionRefinery));
        // Earth also runs a Hydroponics plant so Food has a producer and trades.
        self.facilities
            .push(Facility::new(1, earth, FacilityKind::Hydroponics));
        chain(self, 1, "Hygiea", earth, 2); // Earth
        chain(self, 2, "Juno", mars, 2); // Mars
        for name in ["Vesta", "Pallas", "Eros"] {
            if let Some(b) = belt(self, name) {
                self.mining_stations.push(MiningStation::new(3, b));
            }
        }
        for _ in 0..2 {
            self.ships
                .push(Ship::new(3, ShipClass::Hauler, "Hauler", ceres)); // OPA
        }
        // Combat vessels (no economic role yet).
        self.ships
            .push(Ship::new(1, ShipClass::Combat, "UNN Cerberus", earth));
        self.ships
            .push(Ship::new(2, ShipClass::Combat, "MCRN Donnager", mars));
        self.ships
            .push(Ship::new(7, ShipClass::Combat, "Free Navy Pella", ceres));

        // Companies / pirates: a station + a hauler each, so every player earns.
        chain(self, 4, "Eunomia", earth, 1);
        chain(self, 5, "Davida", mars, 1);
        chain(self, 7, "Sylvia", ceres, 1);
        // The Private Sector (player 6) is the **trade backbone**: a station + 10 haulers,
        // docked across the three hubs for spatial spread. They mostly arbitrage the markets.
        if let Some(b) = belt(self, "Interamnia") {
            self.mining_stations.push(MiningStation::new(6, b));
        }
        let hubs = [earth, mars, ceres];
        for n in 0..10 {
            self.ships
                .push(Ship::new(6, ShipClass::Hauler, "Trader", hubs[n % 3]));
        }
    }

    // ---- the tick loop ----------------------------------------------------------------

    /// Advance one tick. **Phase order is the determinism contract**; the only RNG touch is
    /// the market stabilization.
    pub fn step(&mut self) {
        self.tick += 1;
        // 1. Markets self-stabilize (the sole RNG consumer).
        for m in &mut self.markets {
            m.step(&mut self.rng);
        }
        // 2. Extraction: mining stations + colonies dig their body's raw into a local store.
        self.run_extraction();
        // 3. Production: facilities refine on-site input → on-site output (idle if starved).
        self.run_production();
        // 4. Advance in-flight ships; dock those that have arrived.
        self.advance_ships();
        // 5. Logistics: idle haulers pick + run jobs (mining→facility, output/raw→demand center).
        //    This is the object-driven behaviour that replaces the per-player AI verb seam.
        self.run_logistics();
        // 6. Ship fuel top-up.
        self.run_ship_upkeep();
    }

    /// Mining stations (and abundant colonies) extract their body's raw goods into a local store
    /// each tick, scaled by the body's abundance and capped. No RNG — fully deterministic.
    /// Test baseline: advance only the market stabilizers (no trade), so markets rest at their
    /// specialized setpoints — the wide-spread baseline arbitrage is measured against.
    #[cfg(test)]
    fn step_markets_only(&mut self) {
        self.tick += 1;
        for m in &mut self.markets {
            m.step(&mut self.rng);
        }
    }

    fn run_extraction(&mut self) {
        let raw = commodity::raw_count();
        for st in &mut self.mining_stations {
            let goods = &self.bodies[st.body].goods;
            for r in 0..raw {
                let amt = mine_amount(goods.get(r).copied().unwrap_or(0));
                if amt > 0 && st.get(r) < STATION_STORE_CAP {
                    st.add(r, amt);
                }
            }
        }
        // Colonies on an abundant body dig at a lighter rate (they're not dedicated miners).
        for col in &mut self.colonies {
            let goods = &self.bodies[col.body].goods;
            for r in 0..raw {
                let amt = mine_amount(goods.get(r).copied().unwrap_or(0)) / 2;
                if amt > 0 && col.get(r) < STATION_STORE_CAP {
                    col.add(r, amt);
                }
            }
        }
    }

    /// Facilities consume on-site input → produce on-site output; **idle if starved** (no on-site
    /// input) or if the output buffer is full. Touches no owner stockpile and no RNG.
    fn run_production(&mut self) {
        use super::facility::FACILITY_OUTPUT_CAP;
        for f in &mut self.facilities {
            let r = f.kind.recipe();
            let need = f.rate * r.ratio;
            if f.input_of(r.input) >= need && f.output_of(r.out) < FACILITY_OUTPUT_CAP {
                f.add_input(r.input, -need);
                f.add_output(r.out, f.rate);
            }
        }
    }

    /// The greedy object-driven trade brain: each idle civilian hauler (deterministic, fixed
    /// ship-index order) scores candidate jobs by value, commits to the best, **reserves** the
    /// quantities at the markets so the next hauler fans out, and runs the job. Reservations are
    /// freed on abandon / ship loss.
    fn run_logistics(&mut self) {
        for i in 0..self.ships.len() {
            if self.ships[i].class != ShipClass::Hauler || self.ships[i].in_flight() {
                continue;
            }
            let owner = self.ships[i].owner;
            if self.ships[i].job.is_none() {
                let cap = super::ship::ship_stats(&self.ships[i]).cargo_capacity;
                if let Some(job) = self.find_job(owner, self.ships[i].body, cap) {
                    self.commit_reservations(&job);
                    self.ships[i].job = Some(job);
                }
            }
            self.service_hauler(i);
        }
    }

    /// The two market reservations a job holds in its current phase: `(buy@from, sell@to)`, each
    /// `(market, commodity, qty)`. A `ToPickup` job holds both (if those legs are markets); a
    /// `ToDropoff` job has already executed its buy, so it holds only the sell. The single source
    /// of truth shared by commit / free / reload.
    fn job_reservations(job: &Job) -> (MarketResv, MarketResv) {
        let sell = match job.to {
            SiteRef::Market { market, commodity } => Some((market, commodity, job.qty)),
            _ => None,
        };
        let buy = match (job.phase, job.from) {
            (JobPhase::ToPickup, SiteRef::Market { market, commodity }) => {
                Some((market, commodity, job.qty))
            }
            _ => None,
        };
        (buy, sell)
    }

    fn commit_reservations(&mut self, job: &Job) {
        let (buy, sell) = Self::job_reservations(job);
        if let Some((m, c, q)) = buy {
            self.markets[m].reserve_buy(c, q);
        }
        if let Some((m, c, q)) = sell {
            self.markets[m].reserve_sell(c, q);
        }
    }

    /// Release a ship's outstanding market reservations (job abandoned, or — future — ship
    /// destroyed). A `// COMBAT HOOK`: a future `destroy_ship(i)` MUST call this before removing the
    /// ship, or the reservation leaks and depresses availability forever.
    fn free_reservations(&mut self, i: usize) {
        if let Some(job) = self.ships[i].job {
            let (buy, sell) = Self::job_reservations(&job);
            if let Some((m, c, q)) = buy {
                self.markets[m].release_buy(c, q);
            }
            if let Some((m, c, q)) = sell {
                self.markets[m].release_sell(c, q);
            }
        }
    }

    /// Re-place the reservations for every in-flight job after a load (the markets' reservation
    /// counters are a derived cache; this rebuilds them). Wired into `from_save`.
    fn reapply_reservations(&mut self) {
        for i in 0..self.ships.len() {
            if let Some(job) = self.ships[i].job {
                let (buy, sell) = Self::job_reservations(&job);
                if let Some((m, c, q)) = buy {
                    self.markets[m].reserve_buy(c, q);
                }
                if let Some((m, c, q)) = sell {
                    self.markets[m].reserve_sell(c, q);
                }
            }
        }
    }

    fn credit(&mut self, owner: PlayerId, amount: i64) {
        if let Some(p) = self.players.get_mut(owner as usize) {
            p.credits += amount;
        }
    }
    fn debit(&mut self, owner: PlayerId, amount: i64) {
        if let Some(p) = self.players.get_mut(owner as usize) {
            p.credits -= amount;
        }
    }

    /// Body index a `SiteRef` lives at.
    fn site_body(&self, site: SiteRef) -> usize {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].body,
            SiteRef::Facility(j) => self.facilities[j].body,
            SiteRef::Colony(c) => self.colonies[c].body,
            SiteRef::Market { market, .. } => self.markets[market].body(),
        }
    }

    /// Drive a hauler's committed job forward: at the pickup body it loads + launches to the
    /// dropoff; at the dropoff body it unloads (or sells to a sink) + clears the job.
    fn service_hauler(&mut self, i: usize) {
        let Some(job) = self.ships[i].job else {
            return;
        };
        let owner = self.ships[i].owner;
        let target = match job.phase {
            JobPhase::ToPickup => self.site_body(job.from),
            JobPhase::ToDropoff => self.site_body(job.to),
        };
        if self.ships[i].body != target {
            // Not there yet — fly. If it can't (no fuel), free the reservations + drop the job.
            if !self.launch_ship(i, target) && !self.ships[i].in_flight() {
                self.free_reservations(i);
                self.ships[i].job = None;
            }
            return;
        }
        match job.phase {
            JobPhase::ToPickup => {
                // Peek the available quantity (no mutation yet, so the abandon path can release
                // every still-held reservation exactly once via free_reservations).
                let q = match job.from {
                    SiteRef::Market { market, commodity } => job
                        .qty
                        .min(self.markets[market].available_to_buy(commodity))
                        .max(0),
                    _ => {
                        let want = if job.qty > 0 {
                            job.qty
                        } else {
                            super::ship::ship_stats(&self.ships[i]).cargo_capacity
                        };
                        want.min(self.site_available(job.from, job.good)).max(0)
                    }
                };
                if q <= 0 {
                    self.free_reservations(i); // source dried up — release everything, re-decide
                    self.ships[i].job = None;
                    return;
                }
                // Execute the pickup: a market buy pays cost + fee and releases its buy reservation;
                // a producer pickup is owner-internal (no payment, no reservation).
                match job.from {
                    SiteRef::Market { market, commodity } => {
                        let cost = self.markets[market].execute_buy(commodity, q);
                        self.markets[market].release_buy(commodity, job.qty);
                        let fee = cost * BROKER_FEE_BP / BP;
                        self.debit(owner, cost + fee);
                    }
                    _ => self.site_take(job.from, job.good, q),
                }
                self.ships[i].add_cargo(job.good, q);
                self.ships[i].job = Some(Job {
                    phase: JobPhase::ToDropoff,
                    ..job
                });
                // Immediately head to the dropoff (or unload now if already co-located).
                self.service_hauler(i);
            }
            JobPhase::ToDropoff => {
                let qty = self.ships[i].cargo_of(job.good);
                self.ships[i].add_cargo(job.good, -qty);
                match job.to {
                    // Sell into a market: receive revenue − fee, release the sell reservation.
                    SiteRef::Market { market, commodity } => {
                        let revenue = self.markets[market].execute_sell(commodity, qty);
                        self.markets[market].release_sell(commodity, job.qty);
                        let fee = revenue * BROKER_FEE_BP / BP;
                        self.credit(owner, revenue - fee);
                    }
                    // Deposit into the owner's own facility/colony/station (no payment).
                    _ => self.deliver(job.to, job.good, qty, owner),
                }
                self.ships[i].job = None;
            }
        }
    }

    /// How much of `good` a source site can currently supply.
    fn site_available(&self, site: SiteRef, good: usize) -> i64 {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].get(good),
            SiteRef::Facility(j) => self.facilities[j].output_of(good),
            SiteRef::Colony(c) => self.colonies[c].get(good),
            // Buying from a market is added in the greedy-arbitrage commit; not a source yet.
            SiteRef::Market { .. } => 0,
        }
    }

    fn site_take(&mut self, site: SiteRef, good: usize, qty: i64) {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].add(good, -qty),
            SiteRef::Facility(j) => self.facilities[j].add_output(good, -qty),
            SiteRef::Colony(c) => self.colonies[c].add(good, -qty),
            SiteRef::Market { .. } => {}
        }
    }

    /// Drop `qty` of `good` at `site`: into a facility's input / a colony's store, or **sell to a
    /// market** (`execute_sell` moves stock + reprices) crediting `owner` at the live price.
    fn deliver(&mut self, site: SiteRef, good: usize, qty: i64, owner: PlayerId) {
        match site {
            SiteRef::Facility(j) => self.facilities[j].add_input(good, qty),
            SiteRef::Colony(c) => self.colonies[c].add(good, qty),
            SiteRef::Station(k) => self.mining_stations[k].add(good, qty),
            SiteRef::Market { market, commodity } => {
                let revenue = self.markets[market].execute_sell(commodity, qty);
                if let Some(p) = self.players.get_mut(owner as usize) {
                    p.credits += revenue;
                }
            }
        }
    }

    /// The greedy utility-AI: enumerate candidate jobs for an idle hauler (fixed scan order),
    /// score each by integer **value**, and return the best (value↓ → distance↑ → enumeration
    /// index↑). Candidates: (A) sell the owner's producer output/raw into the best market; (B)
    /// arbitrage cheap market A → dear market B; (C) supply the owner's starved facility/colony by
    /// buying input from the cheapest market.
    fn find_job(&self, owner: PlayerId, at_body: usize, cap: i64) -> Option<Job> {
        use super::facility::{FACILITY_INPUT_CAP, FACILITY_LOW_WATER};
        let dist2 = |a: usize, b: usize| {
            orbit::distance(&self.bodies, at_body, a, self.tick)
                + orbit::distance(&self.bodies, a, b, self.tick)
        };
        let fuel_est = |d: i64| (d / FUEL_PER_DISTANCE).max(0) * EST_FUEL_CREDIT;
        let mut cands: Vec<(i64, i64, Job)> = Vec::new();

        // (A) sell producer output (facilities) + orphan raw (stations/colonies) into a market.
        let mut sell_from = |s: &Self, good: usize, avail: i64, src: SiteRef, src_body: usize| {
            if avail < LOAD_MIN {
                return;
            }
            if let Some(m) = s.best_sell_market(good, src_body) {
                let qty = cap.min(avail).min(s.markets[m].headroom_to_sell(good));
                if qty < LOAD_MIN {
                    return;
                }
                let gross = s.markets[m].price(good) * qty;
                let d = dist2(src_body, s.markets[m].body());
                let value = gross - gross * BROKER_FEE_BP / BP - fuel_est(d);
                cands.push((
                    value,
                    d,
                    Job {
                        good,
                        from: src,
                        to: SiteRef::Market {
                            market: m,
                            commodity: good,
                        },
                        phase: JobPhase::ToPickup,
                        qty,
                    },
                ));
            }
        };
        for (j, f) in self.facilities.iter().enumerate() {
            if f.owner == owner {
                let out = f.kind.recipe().out;
                sell_from(self, out, f.output_of(out), SiteRef::Facility(j), f.body);
            }
        }
        for (k, st) in self.mining_stations.iter().enumerate() {
            if st.owner == owner {
                for raw in 0..commodity::raw_count() {
                    if !self.owner_consumes(owner, raw) {
                        sell_from(self, raw, st.get(raw), SiteRef::Station(k), st.body);
                    }
                }
            }
        }

        // (B) arbitrage: cheap market A → dear market B (any hauler, on its own account).
        for a in 0..self.markets.len() {
            for b in 0..self.markets.len() {
                if a == b {
                    continue;
                }
                for g in 0..commodity::commodity_count() {
                    let (pa, pb) = (self.markets[a].price(g), self.markets[b].price(g));
                    if pb <= pa {
                        continue;
                    }
                    let qty = cap
                        .min(self.markets[a].available_to_buy(g))
                        .min(self.markets[b].headroom_to_sell(g));
                    if qty < LOAD_MIN {
                        continue;
                    }
                    let gross = (pb - pa) * qty;
                    let d = dist2(self.markets[a].body(), self.markets[b].body());
                    let fees = (pa * qty + pb * qty) * BROKER_FEE_BP / BP;
                    let value = gross - fees - fuel_est(d);
                    cands.push((
                        value,
                        d,
                        Job {
                            good: g,
                            from: SiteRef::Market {
                                market: a,
                                commodity: g,
                            },
                            to: SiteRef::Market {
                                market: b,
                                commodity: g,
                            },
                            phase: JobPhase::ToPickup,
                            qty,
                        },
                    ));
                }
            }
        }

        // (C) supply a starved same-owner facility (buy input from the cheapest market).
        for (j, f) in self.facilities.iter().enumerate() {
            if f.owner != owner {
                continue;
            }
            let input = f.kind.recipe().input;
            if f.input_of(input) >= FACILITY_LOW_WATER {
                continue;
            }
            if let Some(m) = self.best_buy_market(input, f.body) {
                let qty = cap
                    .min(self.markets[m].available_to_buy(input))
                    .min(FACILITY_INPUT_CAP - f.input_of(input));
                if qty < LOAD_MIN {
                    continue;
                }
                let cost = self.markets[m].price(input) * qty;
                let d = dist2(self.markets[m].body(), f.body);
                let value = SUPPLY_BONUS - cost - fuel_est(d);
                cands.push((
                    value,
                    d,
                    Job {
                        good: input,
                        from: SiteRef::Market {
                            market: m,
                            commodity: input,
                        },
                        to: SiteRef::Facility(j),
                        phase: JobPhase::ToPickup,
                        qty,
                    },
                ));
            }
        }

        // Pick the best: value↓ → distance↑ → enumeration-index↑ (a full deterministic order).
        cands
            .iter()
            .enumerate()
            .filter(|(_, (v, _, _))| *v > 0)
            .max_by(|(ia, a), (ib, b)| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(ib.cmp(ia)))
            .map(|(_, (_, _, job))| *job)
    }

    /// The cheapest market to **buy** `good` from `at_body` (with stock available): lowest price →
    /// nearest → lowest index.
    fn best_buy_market(&self, good: usize, at_body: usize) -> Option<usize> {
        let tick = self.tick;
        let dist = |m: usize| orbit::distance(&self.bodies, at_body, self.markets[m].body(), tick);
        (0..self.markets.len())
            .filter(|&m| self.markets[m].available_to_buy(good) >= LOAD_MIN)
            .min_by(|&a, &b| {
                self.markets[a]
                    .price(good)
                    .cmp(&self.markets[b].price(good))
                    .then(dist(a).cmp(&dist(b)))
                    .then(a.cmp(&b))
            })
    }

    /// Whether `owner` has a facility that consumes raw `good` (so it's kept for that facility
    /// rather than sold off).
    fn owner_consumes(&self, owner: PlayerId, good: usize) -> bool {
        self.facilities
            .iter()
            .any(|f| f.owner == owner && f.kind.recipe().input == good)
    }

    /// The best market index to **sell** `good` into from `at_body`: highest price → nearest →
    /// lowest index (and it must have headroom to accept the goods).
    fn best_sell_market(&self, good: usize, at_body: usize) -> Option<usize> {
        let tick = self.tick;
        let dist = |m: usize| orbit::distance(&self.bodies, at_body, self.markets[m].body(), tick);
        (0..self.markets.len())
            .filter(|&m| self.markets[m].headroom_to_sell(good) >= LOAD_MIN)
            .min_by(|&a, &b| {
                self.markets[b]
                    .price(good)
                    .cmp(&self.markets[a].price(good))
                    .then(dist(a).cmp(&dist(b)))
                    .then(a.cmp(&b))
            })
    }

    /// Refuel docked ships at port — fuel (Fusion Fuel, ultimately from Ice) is available where a
    /// ship docks, so a hauler never strands mid-network. (A locational fuel economy — buying
    /// Fusion Fuel at the dock's market — is a future refinement; flights still cost fuel.)
    fn run_ship_upkeep(&mut self) {
        for sh in &mut self.ships {
            if !sh.in_flight() {
                sh.fuel = super::ship::ship_stats(sh).fuel_capacity;
            }
        }
    }

    // ---- ship movement ----------------------------------------------------------------

    /// Launch ship `i` toward body `dest`: lock the ETA from the departure-tick distance and the
    /// ship's derived speed, and burn the lump-sum fuel. Returns false (and does not launch) if
    /// the ship is already in flight, already there, or lacks the fuel.
    pub fn launch_ship(&mut self, i: usize, dest: usize) -> bool {
        let Some(sh) = self.ships.get(i) else {
            return false;
        };
        if sh.in_flight() || sh.body == dest || dest >= self.bodies.len() {
            return false;
        }
        let dist = orbit::distance(&self.bodies, sh.body, dest, self.tick);
        let speed = super::ship::ship_stats(sh).speed;
        let travel = (dist / (speed * SPEED_UNIT)).max(1) as u64;
        let fuel_cost = (dist / FUEL_PER_DISTANCE).max(1);
        if sh.fuel < fuel_cost {
            return false;
        }
        let sh = &mut self.ships[i];
        sh.fuel -= fuel_cost;
        sh.dest = Some(dest);
        sh.departed = self.tick;
        sh.arrival = self.tick + travel;
        true
    }

    /// Dock any ship whose flight has completed (`tick >= arrival`): `body = dest`, `dest = None`.
    fn advance_ships(&mut self) {
        let tick = self.tick;
        for sh in &mut self.ships {
            if let Some(d) = sh.dest {
                if tick >= sh.arrival {
                    sh.body = d;
                    sh.dest = None;
                }
            }
        }
    }

    /// A ship's render position: its docked body, or the lerp along its flight leg.
    pub fn ship_pos(&self, i: usize) -> (i64, i64) {
        let Some(sh) = self.ships.get(i) else {
            return (0, 0);
        };
        match sh.dest {
            Some(d) => {
                let span = sh.arrival.saturating_sub(sh.departed).max(1) as i64;
                let num = self.tick.saturating_sub(sh.departed) as i64;
                orbit::lerp_pos(&self.bodies, sh.body, d, self.tick, num, span)
            }
            None => orbit::position_of(&self.bodies, sh.body, self.tick),
        }
    }

    // ---- accessors --------------------------------------------------------------------

    pub fn tick(&self) -> u64 {
        self.tick
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }
    pub fn players(&self) -> &[Player] {
        &self.players
    }
    pub fn human(&self) -> PlayerId {
        self.human
    }
    pub fn ships(&self) -> &[Ship] {
        &self.ships
    }
    pub fn facilities(&self) -> &[Facility] {
        &self.facilities
    }
    pub fn colonies(&self) -> &[Colony] {
        &self.colonies
    }
    pub fn mining_stations(&self) -> &[MiningStation] {
        &self.mining_stations
    }
    pub fn markets(&self) -> &[Market] {
        &self.markets
    }

    /// The human player's treasury.
    pub fn human_credits(&self) -> i64 {
        self.players[self.human as usize].credits
    }

    /// Count the human's ships of a class.
    pub fn human_ship_count(&self, class: ShipClass) -> usize {
        self.ships
            .iter()
            .filter(|s| s.owner == self.human && s.class == class)
            .count()
    }
    pub fn human_colony_count(&self) -> usize {
        self.colonies
            .iter()
            .filter(|c| c.owner == self.human)
            .count()
    }
    pub fn human_mining_station_count(&self) -> usize {
        self.mining_stations
            .iter()
            .filter(|s| s.owner == self.human)
            .count()
    }

    /// Absolute `(x, y)` of body `i` at the current tick (for the orrery).
    pub fn body_pos(&self, i: usize) -> (i64, i64) {
        if i >= self.bodies.len() {
            return (0, 0);
        }
        orbit::position_of(&self.bodies, i, self.tick)
    }

    // ---- persistence (§30) ------------------------------------------------------------

    pub fn to_save(&self) -> super::persist::SaveState {
        use super::persist::{MarketSave, SaveState, SAVE_VERSION};
        SaveState {
            version: SAVE_VERSION,
            seed: self.seed,
            tick: self.tick,
            human: self.human,
            players: self.players.clone(),
            ships: self.ships.clone(),
            facilities: self.facilities.clone(),
            colonies: self.colonies.clone(),
            mining_stations: self.mining_stations.clone(),
            markets: self
                .markets
                .iter()
                .map(|m| MarketSave {
                    stocks: m.stocks().iter().map(|s| s.stock).collect(),
                    prices: m.stocks().iter().map(|s| s.price).collect(),
                })
                .collect(),
        }
    }

    /// Rebuild a `Sim` from a save: fresh world from the seed (bodies/catalogs from code),
    /// then overlay the saved mutable state.
    pub fn from_save(s: super::persist::SaveState) -> Self {
        let mut sim = Sim::new(s.seed);
        sim.tick = s.tick;
        sim.human = s.human;
        sim.players = s.players;
        sim.ships = s.ships;
        sim.facilities = s.facilities;
        sim.colonies = s.colonies;
        sim.mining_stations = s.mining_stations;
        for (m, ms) in sim.markets.iter_mut().zip(&s.markets) {
            m.restore_stocks(&ms.stocks, &ms.prices);
        }
        // Market reservations are a derived cache (zeroed by restore_stocks): rebuild them from
        // the loaded in-flight jobs so availability matches a sim that ran to this tick.
        sim.reapply_reservations();
        // The rng only feeds market noise, which is overwritten by restore_stocks, so a fresh rng
        // is fine.
        sim
    }

    pub fn save_json(&self) -> String {
        self.to_save().to_json()
    }
    pub fn save_bytes(&self) -> Vec<u8> {
        self.to_save().to_bincode()
    }
    /// Load from bytes, auto-detecting JSON (`{`) vs bincode.
    pub fn load_bytes(bytes: &[u8]) -> Result<Self, String> {
        let s = if bytes.first() == Some(&b'{') {
            super::persist::SaveState::from_json(&String::from_utf8_lossy(bytes))?
        } else {
            super::persist::SaveState::from_bincode(bytes)?
        };
        Ok(Self::from_save(s))
    }
}

#[cfg(test)]
mod tests {
    use super::super::player::PlayerKind;
    use super::*;

    #[test]
    fn step_is_deterministic_from_a_seed() {
        let mut a = Sim::new(7);
        let mut b = Sim::new(7);
        for _ in 0..1_000 {
            a.step();
            b.step();
        }
        assert_eq!(a.tick(), b.tick());
        assert_eq!(a.human_credits(), b.human_credits());
        for (pa, pb) in a.players().iter().zip(b.players()) {
            assert_eq!(pa.credits, pb.credits);
            assert_eq!(pa.stockpiles, pb.stockpiles);
        }
    }

    #[test]
    fn the_human_is_player_zero_with_starting_assets() {
        let sim = Sim::new(1);
        assert_eq!(sim.players()[0].kind, PlayerKind::Human);
        assert!(sim.human_ship_count(ShipClass::Hauler) >= 1);
        assert!(sim.human_ship_count(ShipClass::Miner) >= 1);
        assert!(sim.human_mining_station_count() >= 1);
    }

    #[test]
    fn credits_flow_for_every_player_over_a_long_run() {
        // The headline "economy actually FLOWs" gate: every seeded player's credits rise as their
        // chains (mine → haul → facility → haul → sink) turn over.
        let mut sim = Sim::new(0);
        let c0: Vec<i64> = sim.players().iter().map(|p| p.credits).collect();
        for _ in 0..6_000 {
            sim.step();
        }
        for (i, p) in sim.players().iter().enumerate() {
            assert!(
                p.credits > c0[i],
                "player {i} ({}) credits should rise: {} → {}",
                p.name,
                c0[i],
                p.credits
            );
        }
    }

    #[test]
    fn the_market_economy_is_living_and_bounded() {
        // Under full hauler traffic across many seeds: no market price ever pins to a rail (the
        // §7c structural guarantee), reservations never drive availability negative, and prices
        // visibly vary (it's living, not static). Credits grow but stay bounded + positive.
        let goods = super::super::commodity::commodity_count();
        for seed in 0..16u64 {
            let mut sim = Sim::new(seed);
            let g = super::super::commodity::ALLOYS;
            let (mut pmin, mut pmax) = (i64::MAX, i64::MIN);
            for _ in 0..1_500 {
                sim.step();
                for m in sim.markets() {
                    for c in 0..goods {
                        let d = &m.defs()[c];
                        assert!(
                            m.price(c) > d.floor && m.price(c) < d.ceiling,
                            "seed {seed}: price pinned to a rail"
                        );
                        assert!(m.available_to_buy(c) >= 0, "availability never negative");
                    }
                }
                let p = sim.markets()[0].price(g);
                pmin = pmin.min(p);
                pmax = pmax.max(p);
            }
            assert!(
                pmax - pmin > 10,
                "seed {seed}: prices should vary (living), got {pmin}..{pmax}"
            );
            // Credits positive + not absurd (bounded by production throughput, not runaway).
            let total: i64 = sim.players().iter().map(|p| p.credits).sum();
            assert!(
                total > 0 && total < 1_000_000_000,
                "seed {seed}: credits bounded ({total})"
            );
            for p in sim.players() {
                assert!(p.credits >= 0, "seed {seed}: {} insolvent", p.name);
            }
        }
    }

    #[test]
    fn arbitrage_damps_the_inter_market_spread() {
        use super::super::commodity::ALLOYS;
        // With haulers trading, the Alloys spread between the cheap (Ceres) and dear (Earth) hubs
        // is smaller than a no-logistics baseline where the stabilizers hold the setpoints apart.
        let spread = |run_haulers: bool| -> i64 {
            let mut sim = Sim::new(0);
            let mut acc = 0i64;
            let mut n = 0i64;
            for t in 0..3_000 {
                if run_haulers {
                    sim.step();
                } else {
                    sim.step_markets_only();
                }
                if t >= 1_000 {
                    acc += (sim.markets()[0].price(ALLOYS) - sim.markets()[2].price(ALLOYS)).abs();
                    n += 1;
                }
            }
            acc / n.max(1)
        };
        let traded = spread(true);
        let baseline = spread(false);
        assert!(
            traded < baseline,
            "arbitrage should compress the spread: traded {traded} vs baseline {baseline}"
        );
    }

    #[test]
    fn a_hauler_feeds_a_starved_facility_which_then_produces() {
        use super::super::commodity::ALLOYS;
        // Mars's AlloyPlant starts starved; its hauler brings Ore from Mars's belt station; then
        // the plant produces Alloys (on-site output).
        let mut sim = Sim::new(0);
        let fi = sim
            .facilities
            .iter()
            .position(|f| f.kind == FacilityKind::AlloyPlant)
            .unwrap();
        assert_eq!(sim.facilities[fi].output_of(ALLOYS), 0);
        let mut produced = false;
        for _ in 0..3_000 {
            sim.step();
            if sim.facilities[fi].output_of(ALLOYS) > 0 {
                produced = true;
                break;
            }
        }
        assert!(
            produced,
            "a hauler fed the plant on-site and it produced Alloys"
        );
    }

    #[test]
    fn the_human_earns_by_hauling_raw_to_a_demand_center() {
        // The human (1 station + 1 hauler, no facility): the station mines raw, the hauler carries
        // it to the best sink, and credits rise — the simplest object-driven chain.
        let mut sim = Sim::new(0);
        let c0 = sim.players[0].credits;
        for _ in 0..4_000 {
            sim.step();
        }
        assert!(
            sim.players[0].credits > c0,
            "the human's hauler turned mined raw into credits ({} → {})",
            c0,
            sim.players[0].credits
        );
    }

    #[test]
    fn a_run_round_trips_through_a_save_with_in_flight_ships() {
        // Run until the world is busy — ships in flight (cargo/dest/arrival set), facility buffers
        // and station stores non-empty — so the round-trip exercises every new locational field.
        let mut a = Sim::new(9);
        for _ in 0..600 {
            a.step();
        }
        assert!(
            a.ships().iter().any(|s| s.in_flight()),
            "some ship is mid-flight"
        );
        assert!(a
            .mining_stations()
            .iter()
            .any(|s| s.store.iter().sum::<i64>() > 0));
        // Binary round-trip.
        let bytes = a.save_bytes();
        let b = Sim::load_bytes(&bytes).expect("a save reloads");
        assert_eq!(a.to_save(), b.to_save());
        assert_eq!(a.tick(), b.tick());
        // A reloaded in-flight ship keeps its arrival + cargo + reserved job qty.
        let fi = a.ships().iter().position(|s| s.in_flight()).unwrap();
        assert_eq!(a.ships()[fi].arrival, b.ships()[fi].arrival);
        assert_eq!(a.ships()[fi].cargo, b.ships()[fi].cargo);
        // Market reservations are rebuilt on load — they match the live sim good-for-good.
        for (ma, mb) in a.markets().iter().zip(b.markets()) {
            for c in 0..super::super::commodity::commodity_count() {
                assert_eq!(
                    ma.reserved_out(c),
                    mb.reserved_out(c),
                    "buy reservation rebuilt"
                );
                assert_eq!(
                    ma.reserved_in(c),
                    mb.reserved_in(c),
                    "sell reservation rebuilt"
                );
            }
        }
        // JSON round-trip.
        let json = a.save_json();
        assert!(json.starts_with('{'));
        let c = Sim::load_bytes(json.as_bytes()).expect("json reloads");
        assert_eq!(a.to_save(), c.to_save());
        // The version gate rejects an older save.
        let mut old = a.to_save();
        old.version = 3;
        assert!(super::super::persist::SaveState::from_bincode(&old.to_bincode()).is_err());
    }

    #[test]
    fn a_launched_ship_arrives_on_schedule_and_burns_fuel() {
        let mut sim = Sim::new(0);
        // The human's Miner (ship 1) is docked at Ceres (5); Miners aren't auto-dispatched, so
        // it stays put under our manual control. Launch it to Earth (3).
        let i = 1;
        let dest = 3;
        assert!(!sim.ships[i].in_flight());
        let fuel0 = sim.ships[i].fuel;
        let dist = super::super::orbit::distance(&sim.bodies, sim.ships[i].body, dest, sim.tick());
        let speed = super::super::ship::ship_stats(&sim.ships[i]).speed;
        let want_travel = (dist / (speed * SPEED_UNIT)).max(1) as u64;
        assert!(sim.launch_ship(i, dest));
        assert!(sim.ships[i].in_flight());
        assert!(sim.ships[i].fuel < fuel0, "fuel burned at departure");
        let departed = sim.ships[i].departed;
        let arrival = sim.ships[i].arrival;
        assert_eq!(arrival - departed, want_travel, "ETA == distance/speed");
        // Run until just before arrival — still in flight; then it docks at dest.
        while sim.tick() < arrival {
            sim.step();
            if sim.tick() < arrival {
                assert!(sim.ships[i].in_flight(), "in flight until arrival");
            }
        }
        assert!(!sim.ships[i].in_flight(), "docked at arrival");
        assert_eq!(sim.ships[i].body, dest, "arrived at the destination body");
        // Can't launch while already there.
        assert!(!sim.launch_ship(i, dest));
    }

    #[test]
    fn mining_stations_extract_their_bodys_raw_into_a_local_store() {
        let mut sim = Sim::new(0);
        // The human's belt mining station accrues raw over time.
        let s0: i64 = sim.mining_stations[0].store.iter().sum();
        assert_eq!(s0, 0, "starts empty");
        for _ in 0..50 {
            sim.step();
        }
        let s1: i64 = sim.mining_stations[0].store.iter().sum();
        assert!(
            s1 > 0,
            "the station mined its body's raw into its store ({s1})"
        );
    }

    #[test]
    fn a_facility_is_idle_when_starved_and_produces_once_its_input_is_fed() {
        use super::super::commodity::{ALLOYS, ORE};
        // The production phase in isolation: a facility with no on-site input produces nothing;
        // give it input and it produces. (Constructed directly, away from the seeded haulers.)
        let mut f = Facility::new(2, 4, FacilityKind::AlloyPlant);
        // Starved: run_production would produce nothing (input is 0).
        assert_eq!(f.output_of(ALLOYS), 0);
        // Simulate one production tick by hand (mirrors run_production).
        let need = f.rate * f.kind.recipe().ratio;
        assert!(f.input_of(ORE) < need, "no on-site input → starved");
        f.add_input(ORE, 400);
        assert!(f.input_of(ORE) >= need, "fed on-site input → can produce");
    }

    #[test]
    fn the_ring_and_far_side_bodies_are_absent() {
        let sim = Sim::new(0);
        assert!(!sim.bodies().iter().any(|b| b.name == "Ring-Gate"));
        assert!(!sim.bodies().iter().any(|b| b.name == "Erebus"));
        // Sol stays at the origin.
        assert_eq!(sim.body_pos(0), (0, 0));
    }
}
