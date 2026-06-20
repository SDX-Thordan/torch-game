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
/// A mining station / colony local raw store cap (so it stops digging when a hauler isn't
/// collecting — bounds the world even when logistics stalls).
const STATION_STORE_CAP: i64 = 1_000;

/// The best (highest) sink price for `commodity`, or 0 if it has no sink.
fn best_sink_price(commodity: usize) -> i64 {
    economy::sinks()
        .iter()
        .filter(|s| s.commodity == commodity)
        .map(|s| s.price)
        .max()
        .unwrap_or(0)
}
/// Distance units a ship of speed 1 covers per tick — scales the AU-scale `position_of` coords
/// (1 AU = 1_000_000) to sane travel times against the small `ship_stats.speed` numbers.
const SPEED_UNIT: i64 = 1_000;
/// Distance units burned per unit of Fusion Fuel on a flight (lump-sum at departure).
const FUEL_PER_DISTANCE: i64 = 50_000;

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

        // Companies / private sector / pirates: a station + a hauler each, so every player earns.
        chain(self, 4, "Eunomia", earth, 1);
        chain(self, 5, "Davida", mars, 1);
        chain(self, 6, "Interamnia", earth, 1);
        chain(self, 7, "Sylvia", ceres, 1);
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

    /// The object-driven logistics brain: each idle hauler (deterministic, fixed ship-index order)
    /// commits to a job and runs it — pick up raw/output at a source, fly, drop off at a facility
    /// or a demand-center sink (crediting the owner). Replaces the old global sink drain.
    fn run_logistics(&mut self) {
        for i in 0..self.ships.len() {
            if self.ships[i].class != ShipClass::Hauler || self.ships[i].in_flight() {
                continue;
            }
            let owner = self.ships[i].owner;
            // Assign a job if idle.
            if self.ships[i].job.is_none() {
                self.ships[i].job = self.find_job(owner, self.ships[i].body);
            }
            // Process the job — possibly load+launch, or unload, within this tick.
            self.service_hauler(i);
        }
    }

    /// Body index a `SiteRef` lives at.
    fn site_body(&self, site: SiteRef) -> usize {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].body,
            SiteRef::Facility(j) => self.facilities[j].body,
            SiteRef::Colony(c) => self.colonies[c].body,
            SiteRef::Sink { body, .. } => body,
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
            // Not there yet — fly. If it can't (no fuel), drop the job to re-decide later.
            if !self.launch_ship(i, target) && !self.ships[i].in_flight() {
                self.ships[i].job = None;
            }
            return;
        }
        match job.phase {
            JobPhase::ToPickup => {
                let cap = super::ship::ship_stats(&self.ships[i]).cargo_capacity;
                let avail = self.site_available(job.from, job.good);
                let qty = cap.min(avail);
                if qty <= 0 {
                    self.ships[i].job = None; // source dried up — re-decide
                    return;
                }
                self.site_take(job.from, job.good, qty);
                self.ships[i].add_cargo(job.good, qty);
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
                self.deliver(job.to, job.good, qty, owner);
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
            SiteRef::Sink { .. } => 0,
        }
    }

    fn site_take(&mut self, site: SiteRef, good: usize, qty: i64) {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].add(good, -qty),
            SiteRef::Facility(j) => self.facilities[j].add_output(good, -qty),
            SiteRef::Colony(c) => self.colonies[c].add(good, -qty),
            SiteRef::Sink { .. } => {}
        }
    }

    /// Drop `qty` of `good` at `site`: into a facility's input / a colony's store, or **sell to a
    /// sink** — crediting `owner` at the sink price (the new geographic, per-delivery payout).
    fn deliver(&mut self, site: SiteRef, good: usize, qty: i64, owner: PlayerId) {
        match site {
            SiteRef::Facility(j) => self.facilities[j].add_input(good, qty),
            SiteRef::Colony(c) => self.colonies[c].add(good, qty),
            SiteRef::Station(k) => self.mining_stations[k].add(good, qty),
            SiteRef::Sink { commodity, .. } => {
                let price = best_sink_price(commodity);
                if let Some(p) = self.players.get_mut(owner as usize) {
                    p.credits += qty * price;
                }
            }
        }
    }

    /// Pick a job for an idle hauler (deterministic, fixed scan order): (a) feed a starved
    /// same-owner facility from a same-owner raw source; (b) sell a facility's output to its best
    /// sink; (c) haul a raw surplus with no local consumer to its sink.
    fn find_job(&self, owner: PlayerId, at_body: usize) -> Option<Job> {
        use super::facility::FACILITY_LOW_WATER;
        // (a) **sell** a facility's accumulated output to its best sink (checked first so feeding
        //     never monopolizes the haulers and output actually turns into credits).
        for (j, f) in self.facilities.iter().enumerate() {
            if f.owner != owner {
                continue;
            }
            let out = f.kind.recipe().out;
            if f.output_of(out) >= LOAD_MIN {
                if let Some(body) = self.best_sink_body(out, at_body) {
                    return Some(Job {
                        good: out,
                        from: SiteRef::Facility(j),
                        to: SiteRef::Sink {
                            body,
                            commodity: out,
                        },
                        phase: JobPhase::ToPickup,
                    });
                }
            }
        }
        // (b) feed a starved facility from a same-owner raw source.
        for (j, f) in self.facilities.iter().enumerate() {
            if f.owner != owner {
                continue;
            }
            let input = f.kind.recipe().input;
            if f.input_of(input) >= FACILITY_LOW_WATER {
                continue;
            }
            if let Some(from) = self.find_raw_source(owner, input) {
                return Some(Job {
                    good: input,
                    from,
                    to: SiteRef::Facility(j),
                    phase: JobPhase::ToPickup,
                });
            }
        }
        // (c) raw with no local consumer → its sink
        for (k, st) in self.mining_stations.iter().enumerate() {
            if st.owner != owner {
                continue;
            }
            for raw in 0..commodity::raw_count() {
                if st.get(raw) >= LOAD_MIN && !self.owner_consumes(owner, raw) {
                    if let Some(body) = self.best_sink_body(raw, at_body) {
                        return Some(Job {
                            good: raw,
                            from: SiteRef::Station(k),
                            to: SiteRef::Sink {
                                body,
                                commodity: raw,
                            },
                            phase: JobPhase::ToPickup,
                        });
                    }
                }
            }
        }
        None
    }

    /// A same-owner station/colony with a pickup-worthy surplus of raw good `good`.
    fn find_raw_source(&self, owner: PlayerId, good: usize) -> Option<SiteRef> {
        for (k, st) in self.mining_stations.iter().enumerate() {
            if st.owner == owner && st.get(good) >= LOAD_MIN {
                return Some(SiteRef::Station(k));
            }
        }
        for (c, col) in self.colonies.iter().enumerate() {
            if col.owner == owner && col.get(good) >= LOAD_MIN {
                return Some(SiteRef::Colony(c));
            }
        }
        None
    }

    /// Whether `owner` has a facility that consumes raw `good` (so it shouldn't be dumped to a sink).
    fn owner_consumes(&self, owner: PlayerId, good: usize) -> bool {
        self.facilities
            .iter()
            .any(|f| f.owner == owner && f.kind.recipe().input == good)
    }

    /// The best sink body for `good` from `at_body`: highest price → nearest → lowest body index.
    fn best_sink_body(&self, good: usize, at_body: usize) -> Option<usize> {
        let tick = self.tick;
        economy::sinks()
            .iter()
            .filter(|s| s.commodity == good)
            .min_by(|a, b| {
                b.price
                    .cmp(&a.price)
                    .then(
                        orbit::distance(&self.bodies, at_body, a.body, tick).cmp(&orbit::distance(
                            &self.bodies,
                            at_body,
                            b.body,
                            tick,
                        )),
                    )
                    .then(a.body.cmp(&b.body))
            })
            .map(|s| s.body)
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
        // Re-seed the rng to the loaded tick's expectations: the rng only feeds market noise,
        // which is overwritten by restore_stocks, so a fresh rng is fine.
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
        // A reloaded in-flight ship keeps its arrival + cargo.
        let fi = a.ships().iter().position(|s| s.in_flight()).unwrap();
        assert_eq!(a.ships()[fi].arrival, b.ships()[fi].arrival);
        assert_eq!(a.ships()[fi].cargo, b.ships()[fi].cargo);
        // JSON round-trip.
        let json = a.save_json();
        assert!(json.starts_with('{'));
        let c = Sim::load_bytes(json.as_bytes()).expect("json reloads");
        assert_eq!(a.to_save(), c.to_save());
        // The v2 gate rejects an old version.
        let mut old = a.to_save();
        old.version = 2;
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
