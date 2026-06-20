//! The rebuilt deterministic world (§ multi-player re-aim).
//!
//! `Sim` owns the bodies, the **players** (`players[0]` is the human), their markets, ships,
//! facilities, and settlements. Every owned entity carries an `owner: PlayerId`. The `step()`
//! loop is a fixed, integer-only phase order (the determinism contract): market stabilization
//! → facility production → infinite-sink absorption → per-player AI `think` (stubbed) → ship
//! fuel/upkeep. No ambient events, no combat — those were removed in the rebuild.

use super::ai;
use super::commodity::{self, FUSION_FUEL, ICE};
use super::economy::{self, Market};
use super::facility::{Facility, FacilityKind};
use super::orbit::{self, Body};
use super::player::{default_players, Player, PlayerId};
use super::rng::Pcg32;
use super::ship::{Ship, ShipClass};
use serde::{Deserialize, Serialize};

/// Stockpile a player keeps of any sinkable good before the infinite sink monetizes the
/// surplus — enough working stock to build with, while bounding theoretically-infinite output.
const SINK_RESERVE: i64 = 1_000;
/// A mining station / colony local raw store cap (so it stops digging when a hauler isn't
/// collecting — bounds the world even when logistics stalls).
const STATION_STORE_CAP: i64 = 1_000;

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

    /// Place the opening assets for the human + the NPC players so the world has content and
    /// the top-bar counts are meaningful. Deterministic (no RNG).
    fn seed_starting_assets(&mut self) {
        // Body indices: Earth=3, Mars=4, Ceres=5 (load-bearing). Pick belt bodies by name.
        let belt = |s: &Self, name: &str| s.bodies.iter().position(|b| b.name == name);
        let (earth, mars, ceres) = (3usize, 4usize, 5usize);

        // Human: a hauler, a miner, and a mining station at a belt body.
        self.ships
            .push(Ship::new(0, ShipClass::Hauler, "Logistics Wing", ceres));
        self.ships
            .push(Ship::new(0, ShipClass::Miner, "Prospector", ceres));
        if let Some(psyche) = belt(self, "Psyche") {
            self.mining_stations.push(MiningStation::new(0, psyche));
        }

        // NPC nations: homeworld colonies + a facility each.
        // players: 1 Earth, 2 Mars, 3 OPA.
        self.colonies.push(Colony::new(1, earth, 8_000));
        self.colonies.push(Colony::new(2, mars, 5_000));
        self.colonies.push(Colony::new(3, ceres, 2_000));
        self.facilities
            .push(Facility::new(1, earth, FacilityKind::ElectronicsFab));
        self.facilities
            .push(Facility::new(2, mars, FacilityKind::AlloyPlant));
        self.facilities
            .push(Facility::new(3, ceres, FacilityKind::FusionRefinery));
        // A few combat vessels for the nations + a pirate raider.
        self.ships
            .push(Ship::new(1, ShipClass::Combat, "UNN Cerberus", earth));
        self.ships
            .push(Ship::new(2, ShipClass::Combat, "MCRN Donnager", mars));
        self.ships
            .push(Ship::new(7, ShipClass::Combat, "Free Navy Pella", ceres));
        // Companies (4,5) run haulers; private sector (6) too.
        for owner in [4u16, 5, 6] {
            self.ships
                .push(Ship::new(owner, ShipClass::Hauler, "Trader", earth));
        }
        // Belt mining stations for the OPA.
        for name in ["Vesta", "Pallas", "Eros"] {
            if let Some(b) = belt(self, name) {
                self.mining_stations.push(MiningStation::new(3, b));
            }
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
        // 4. Sink absorption (legacy global drain — replaced by hauler delivery in a later commit).
        self.run_sinks();
        // 5. Per-player AI think (stubbed — no-op).
        let view = ai::WorldView {
            tick: self.tick,
            body_count: self.bodies.len(),
            _marker: core::marker::PhantomData,
        };
        for p in &mut self.players {
            ai::think(p, &view, &mut self.rng);
        }
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

    fn run_sinks(&mut self) {
        // Each player's surplus of a sinkable good above the reserve is sold at the sink price
        // (an infinite outlet — Alloys at Ceres/Earth/Mars + a sink per other good). Sink
        // *locations* are recorded in `economy::sinks()` for when ship movement makes geography
        // matter; absorption is per-player for now.
        let sink_price = sink_price_table();
        for p in &mut self.players {
            for (c, &price) in sink_price.iter().enumerate() {
                if price <= 0 {
                    continue;
                }
                let surplus = p.stock(c) - SINK_RESERVE;
                if surplus > 0 {
                    p.add_stock(c, -surplus);
                    p.credits += surplus * price;
                }
            }
        }
    }

    fn run_ship_upkeep(&mut self) {
        for sh in &mut self.ships {
            if sh.fuel < sh.class.fuel_capacity() {
                if let Some(p) = self.players.get_mut(sh.owner as usize) {
                    // Refuel from Fusion Fuel stock, else convert Ice on the spot (cheap).
                    if p.stock(FUSION_FUEL) > 0 {
                        p.add_stock(FUSION_FUEL, -1);
                        sh.fuel += 1;
                    } else if p.stock(ICE) > 0 {
                        p.add_stock(ICE, -1);
                        sh.fuel += 1;
                    }
                }
            }
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

/// A per-good lookup of the (best) sink price, indexed by commodity. 0 = no sink.
fn sink_price_table() -> Vec<i64> {
    let mut t = vec![0i64; commodity::commodity_count()];
    for s in economy::sinks() {
        if s.commodity < t.len() && s.price > t[s.commodity] {
            t[s.commodity] = s.price;
        }
    }
    t
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
    fn facility_production_is_drained_by_the_sink_so_credits_grow_not_stockpiles() {
        // An NPC with a facility produces continuously; the sink monetizes the surplus, so the
        // stockpile stays bounded and credits climb — production never dead-ends.
        let mut sim = Sim::new(3);
        // Give Mars (owner 2, has an AlloyPlant) plenty of Ore to refine.
        sim.players[2].add_stock(super::super::commodity::ORE, 100_000);
        let c0 = sim.players[2].credits;
        for _ in 0..3_000 {
            sim.step();
        }
        let alloys = sim.players[2].stock(super::super::commodity::ALLOYS);
        assert!(
            alloys <= SINK_RESERVE + 100,
            "alloy stockpile stays bounded ({alloys})"
        );
        assert!(
            sim.players[2].credits > c0,
            "the sink turns production into credits"
        );
    }

    #[test]
    fn a_run_round_trips_through_a_save() {
        let mut a = Sim::new(9);
        a.players[2].add_stock(super::super::commodity::ORE, 5_000);
        for _ in 0..400 {
            a.step();
        }
        // Binary round-trip.
        let bytes = a.save_bytes();
        let b = Sim::load_bytes(&bytes).expect("a save reloads");
        assert_eq!(a.to_save(), b.to_save());
        assert_eq!(a.tick(), b.tick());
        assert_eq!(a.human_credits(), b.human_credits());
        // JSON round-trip + version gate.
        let json = a.save_json();
        assert!(json.starts_with('{'));
        let c = Sim::load_bytes(json.as_bytes()).expect("json reloads");
        assert_eq!(a.to_save(), c.to_save());
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
    fn a_facility_produces_from_on_site_input_and_idles_when_starved() {
        use super::super::commodity::{ALLOYS, ORE};
        let mut sim = Sim::new(0);
        // Find an AlloyPlant (Mars, owner 2) — it's starved (no on-site Ore) ⇒ no output.
        let fi = sim
            .facilities
            .iter()
            .position(|f| f.kind == FacilityKind::AlloyPlant)
            .unwrap();
        for _ in 0..20 {
            sim.step();
        }
        assert_eq!(sim.facilities[fi].output_of(ALLOYS), 0, "starved ⇒ idle");
        // Deliver Ore to its on-site input → it now produces Alloys.
        sim.facilities[fi].add_input(ORE, 400);
        for _ in 0..20 {
            sim.step();
        }
        assert!(
            sim.facilities[fi].output_of(ALLOYS) > 0,
            "fed on-site input ⇒ produces output"
        );
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
