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
/// Cross-owner purchase tax (basis points) paid to the market-owning government — a conserving
/// transfer that tightens trade and funds the nations.
const TAX_BP: i64 = 200;
/// Per-capita national income (basis points of population per tick) minted to a nation from its
/// **capital colony** — the GDP/tax base not modelled in the goods sim. This is the nations'
/// (Earth/Mars/OPA) real money source to fund their fleets, refuelling, and construction; private
/// actors get none (their money is the bottomless free-market / Private-Sector abstraction).
const INCOME_PER_CAPITA_BP: i64 = 400;
/// A small flat per-tick revenue for the light commercial actors — companies' minor off-screen
/// industry and the Free Navy's raiding. Keeps them solvent against fuel + the price-drift losses
/// a ~90-hauler market inflicts on marginal traders (nations use population income; the Private
/// Sector is the bottomless float).
const ENTERPRISE_INCOME: i64 = 340;
const RAIDING_INCOME: i64 = 220;
/// Population is stored at **realistic scale** (Earth ~8e9). The per-tick economic effects
/// (income + food demand) are divided down by these so the per-tick magnitudes stay in the band
/// the markets/fleets are balanced against — a bigger population still means proportionally more
/// income and more food to haul, just bounded.
const POP_SCALE: i64 = 150_000;
/// People fed per one unit of Food demand per tick (so Earth's billions ⇒ tens of Food/tick).
const POP_PER_FOOD: i64 = 400_000_000;
/// Flat utility for routing raw/inputs to a starved **facility** (fires only below its low-water
/// mark). High enough that feeding the chain beats arbitrage — safe now that the price ladder is
/// value-additive (production earns a margin, so buying inputs is recouped on the sale). The
/// `- cost` term still naturally deprioritises overpaying for an already-dear feedstock.
const SUPPLY_BONUS: i64 = 220_000;
/// Utility for feeding a **settlement** (population/crew Food) — sized above the fattest arbitrage
/// leg so essential food delivery always beats speculative trade. Food is cheap, so this never
/// bankrupts the buyer (and nations' population income covers their food bill).
const CONSUMER_BONUS: i64 = 400_000;
/// Credit-equivalent of one fuel unit, used only to *score* (deter) long hauls — not charged.
const EST_FUEL_CREDIT: i64 = 60;
/// A mining station / colony local raw store cap (so it stops digging when a hauler isn't
/// collecting — bounds the world even when logistics stalls).
const STATION_STORE_CAP: i64 = 1_000;
/// Distance units burned per unit of Fusion Fuel on a flight (lump-sum at departure). Tuned so a
/// belt round-trip's fuel is a small fraction of the trade margin — with ~90 haulers competing,
/// margins compress, so a too-heavy fuel bill would bleed the marginal (company) haulers.
const FUEL_PER_DISTANCE: i64 = 1_500_000;

/// A held market reservation: `(market index, commodity, quantity)`.
type MarketResv = Option<(usize, usize, i64)>;

/// How many ticks of food a settlement keeps on hand before a hauler restocks it. Sized above the
/// worst hauler round-trip so even distant belt outposts get fed before they run dry.
const FOOD_BUFFER_TICKS: i64 = 150;

/// A shipyard buys + consumes a batch of Alloys + Electronics once every this many ticks (the
/// build cadence) — the terminal demand sink. Gentle so the importer's bill stays affordable.
const SHIPYARD_INTERVAL: u64 = 12;
const SHIPYARD_BATCH: i64 = 2;
/// A shipyard's owner won't buy materials below this treasury (so a build never bankrupts a
/// government — the sink is bounded by affordability).
const SHIPYARD_MIN_TREASURY: i64 = 700_000;
/// Materials value a shipyard accumulates before a (notional) ship completes.
const SHIP_BUILD_COST: i64 = 90;
/// Ship procurement is **disabled** — a completed build sinks its materials but spawns no `Ship`.
const SHIP_PROCUREMENT_ENABLED: bool = false;
/// A power may run a **bounded deficit** funding its public infrastructure (shipyards/habitats)
/// before it counts as bankrupt — a government isn't insolvent for carrying modest debt.
#[cfg(test)]
const DEFICIT_FLOOR: i64 = -150_000;

/// A settlement's size tier — sets its population/crew and so its **food demand** per tick. From a
/// crewed mining **Outpost** up through a planetary **Capital**.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementTier {
    Outpost,
    Station,
    Colony,
    Hub,
    Capital,
}

impl SettlementTier {
    pub fn name(self) -> &'static str {
        match self {
            SettlementTier::Outpost => "Outpost",
            SettlementTier::Station => "Station",
            SettlementTier::Colony => "Colony",
            SettlementTier::Hub => "Hub",
            SettlementTier::Capital => "Capital",
        }
    }
    /// Food consumed per tick by this tier's population/crew. Kept modest so a settlement's food
    /// bill stays well under its owner's trade income (the QA harness flags it if a player can't
    /// keep up).
    pub fn food_per_tick(self) -> i64 {
        match self {
            SettlementTier::Outpost => 1,
            SettlementTier::Station => 1,
            SettlementTier::Colony => 2,
            SettlementTier::Hub => 3,
            SettlementTier::Capital => 4,
        }
    }
    /// The tier a colony of `population` falls into (realistic scale: Earth ~8e9 = Capital).
    pub fn for_population(population: i64) -> Self {
        match population {
            p if p >= 1_000_000_000 => SettlementTier::Capital,
            p if p >= 100_000_000 => SettlementTier::Hub,
            p if p >= 10_000_000 => SettlementTier::Colony,
            p if p >= 1_000_000 => SettlementTier::Station,
            _ => SettlementTier::Outpost,
        }
    }
}

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
        let mut c = Self {
            owner,
            body,
            population,
            store: vec![0; commodity::commodity_count()],
        };
        // Start with a full food larder so the population isn't starving at tick 0.
        let food = c.food_demand() * FOOD_BUFFER_TICKS;
        c.add(commodity::FOOD, food);
        c
    }
    pub fn add(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.store.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn get(&self, c: usize) -> i64 {
        self.store.get(c).copied().unwrap_or(0)
    }
    /// The colony's size tier (from its population).
    pub fn tier(&self) -> SettlementTier {
        SettlementTier::for_population(self.population)
    }
    /// Food consumed per tick — **population-proportional** (a floor of 1 so any settled body
    /// still demands), so Earth's billions drive tens of Food/tick and a real logistics load.
    pub fn food_demand(&self) -> i64 {
        (self.population / POP_PER_FOOD).max(1)
    }
}

/// A non-growable **dedicated mining station** on an **uninhabitable** body — extracts its body's
/// raw into a local `store` that haulers collect. Its crew is a small **Outpost** that eats food.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiningStation {
    pub owner: PlayerId,
    pub body: usize,
    #[serde(default)]
    pub store: Vec<i64>,
}

impl MiningStation {
    pub fn new(owner: PlayerId, body: usize) -> Self {
        let mut s = Self {
            owner,
            body,
            store: vec![0; commodity::commodity_count()],
        };
        // Start with a full food larder for the crew.
        let food = s.food_demand() * FOOD_BUFFER_TICKS;
        s.add(commodity::FOOD, food);
        s
    }
    pub fn add(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.store.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn get(&self, c: usize) -> i64 {
        self.store.get(c).copied().unwrap_or(0)
    }
    /// A mining station is a crewed Outpost.
    pub fn tier(&self) -> SettlementTier {
        SettlementTier::Outpost
    }
    pub fn food_demand(&self) -> i64 {
        self.tier().food_per_tick()
    }
}

/// Which kind of zero-G orbital structure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZeroGKind {
    /// A deep-space population habitat (big Food demand).
    Habitat,
    /// A shipyard orbiting a capital — consumes Alloys + Electronics (continuous demand sink) and
    /// holds the (disabled) ship-procurement framework.
    Shipyard,
}

/// A **zero-G station** — a deep-space habitat or a shipyard orbiting its capital. Expensive,
/// player-unobtainable structures (no build verb); the set is fixed at world creation. Holds a
/// local `store` (food larder, fuel depot, and — for shipyards — the Alloys/Electronics it consumes).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZeroGStation {
    pub owner: PlayerId,
    pub body: usize,
    pub name: String,
    pub kind: ZeroGKind,
    #[serde(default)]
    pub store: Vec<i64>,
    /// The ship the yard is building toward (procurement framework — `None`/disabled for now).
    #[serde(default)]
    pub order: Option<ShipClass>,
    /// Materials value accumulated toward `order`.
    #[serde(default)]
    pub progress: i64,
}

impl ZeroGStation {
    pub fn new(owner: PlayerId, body: usize, name: &str, kind: ZeroGKind) -> Self {
        let mut s = Self {
            owner,
            body,
            name: name.to_string(),
            kind,
            store: vec![0; commodity::commodity_count()],
            order: None,
            progress: 0,
        };
        // Start with a full food larder for the crew/population.
        let food = s.food_demand() * FOOD_BUFFER_TICKS;
        s.add(commodity::FOOD, food);
        s
    }
    pub fn add(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.store.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn get(&self, c: usize) -> i64 {
        self.store.get(c).copied().unwrap_or(0)
    }
    pub fn is_shipyard(&self) -> bool {
        self.kind == ZeroGKind::Shipyard
    }
    /// A habitat is a Capital-scale population; a shipyard is a Station-scale crew.
    pub fn tier(&self) -> SettlementTier {
        match self.kind {
            ZeroGKind::Habitat => SettlementTier::Colony,
            ZeroGKind::Shipyard => SettlementTier::Station,
        }
    }
    pub fn food_demand(&self) -> i64 {
        self.tier().food_per_tick()
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
    zero_g_stations: Vec<ZeroGStation>,
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
            zero_g_stations: Vec::new(),
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

        // Helpers (each takes `&mut Self`, so they don't alias `belt`'s shared borrow).
        let colony = |s: &mut Self, owner: u16, name: &str, pop: i64| {
            if let Some(b) = belt(s, name) {
                s.colonies.push(Colony::new(owner, b, pop));
            }
        };
        let plant = |s: &mut Self, owner: u16, name: &str, kind: FacilityKind, rate: i64| {
            if let Some(b) = belt(s, name) {
                let mut f = Facility::new(owner, b, kind);
                f.rate = rate;
                s.facilities.push(f);
            }
        };
        let mine = |s: &mut Self, owner: u16, name: &str| {
            if let Some(b) = belt(s, name) {
                s.mining_stations.push(MiningStation::new(owner, b));
            }
        };
        let fleet = |s: &mut Self, owner: u16, label: &str, count: usize, docks: &[usize]| {
            for n in 0..count {
                s.ships.push(Ship::new(
                    owner,
                    ShipClass::Hauler,
                    label,
                    docks[n % docks.len()],
                ));
            }
        };
        let tycho = belt(self, "Tycho").unwrap_or(ceres);

        // Human (0): a **blank-slate start** — no ships or stations, only the opening credit
        // treasury (see `player.rs`). An acquisition path is a deliberate follow-up.

        // ---- Populations (realistic scale; income + food demand scale off these). Capitals plus
        //      OPA belt colonies + an inner Earth colony spread demand across several centres.
        colony(self, 1, "Earth", 8_000_000_000); // UN capital
        colony(self, 1, "Mercury", 250_000_000); // UN inner colony
        colony(self, 2, "Mars", 3_000_000_000); // MCR capital
        colony(self, 3, "Ceres", 400_000_000); // OPA belt metropolis
        colony(self, 3, "Vesta", 60_000_000); // OPA
        colony(self, 3, "Tycho", 45_000_000); // OPA
        colony(self, 3, "Pluto", 25_000_000); // OPA frontier

        // ---- Production chain: a facility per produced good, distributed so every good has a
        //      source. Food-side rates are high (population food demand is large now).
        plant(self, 1, "Earth", FacilityKind::WaferFab, 8); // Earth: tech + agriculture
        plant(self, 1, "Earth", FacilityKind::ElectronicsFab, 6);
        plant(self, 1, "Earth", FacilityKind::Refinery, 4);
        plant(self, 1, "Earth", FacilityKind::Hydroponics, 30);
        plant(self, 2, "Mars", FacilityKind::AlloyPlant, 10); // Mars: heavy industry
        plant(self, 2, "Mars", FacilityKind::MachineShop, 8);
        plant(self, 2, "Mars", FacilityKind::Shipworks, 5);
        plant(self, 2, "Mars", FacilityKind::Hydroponics, 15);
        plant(self, 3, "Ceres", FacilityKind::FusionRefinery, 12); // OPA: refining + food
        plant(self, 3, "Ceres", FacilityKind::AlloyPlant, 8);
        plant(self, 3, "Ceres", FacilityKind::Refinery, 4);
        plant(self, 3, "Ceres", FacilityKind::WaferFab, 6);
        plant(self, 3, "Ceres", FacilityKind::Hydroponics, 20);
        plant(self, 6, "Tycho", FacilityKind::ElectronicsFab, 5); // Private Sector node
        plant(self, 6, "Tycho", FacilityKind::MachineShop, 5);
        plant(self, 6, "Tycho", FacilityKind::Shipworks, 4);
        plant(self, 6, "Tycho", FacilityKind::Hydroponics, 10); // feeds the PS deep-space habitats

        // ---- Belt mining stations (raw supply). More independents (Private Sector + companies).
        mine(self, 1, "Hygiea"); // Earth
        mine(self, 2, "Juno"); // Mars
        for name in ["Vesta", "Pallas", "Eros"] {
            mine(self, 3, name); // OPA heartland
        }
        mine(self, 4, "Eunomia"); // Pallas Combine
        mine(self, 5, "Davida"); // Tycho Industries
        for name in ["Psyche", "Interamnia", "Hektor", "Tycho"] {
            mine(self, 6, name); // Private Sector — the independent miners
        }
        mine(self, 7, "Sylvia"); // Free Navy

        // ---- Fleets. Private Sector is the trade backbone; the nations + companies run their own.
        let hubs = [earth, mars, ceres, tycho];
        fleet(self, 6, "Trader", 40, &hubs); // Private Sector — 40
        fleet(self, 3, "Hauler", 20, &[ceres, mars]); // OPA — 20
        fleet(self, 2, "Hauler", 10, &[mars]); // Mars — 10
        fleet(self, 1, "Hauler", 10, &[earth]); // Earth — 10
        fleet(self, 4, "Hauler", 3, &[earth]); // Pallas Combine
        fleet(self, 5, "Hauler", 3, &[mars]); // Tycho Industries
        fleet(self, 7, "Hauler", 2, &[ceres]); // Free Navy
        self.ships
            .push(Ship::new(6, ShipClass::Miner, "Prospector", ceres));

        // Combat vessels (no economic role yet).
        self.ships
            .push(Ship::new(1, ShipClass::Combat, "UNN Cerberus", earth));
        self.ships
            .push(Ship::new(2, ShipClass::Combat, "MCRN Donnager", mars));
        self.ships
            .push(Ship::new(7, ShipClass::Combat, "Free Navy Pella", ceres));

        // Zero-G stations (player-unobtainable; fixed set). One shipyard per power orbiting its
        // capital, plus the independent private sector's yard + a couple of deep-space habitats.
        let yard = |s: &mut Self, owner: u16, name: &str| {
            if let Some(b) = belt(s, name) {
                s.zero_g_stations
                    .push(ZeroGStation::new(owner, b, name, ZeroGKind::Shipyard));
            }
        };
        yard(self, 1, "Bush Naval Yard"); // Earth
        yard(self, 2, "Kirino Station"); // Mars
        yard(self, 3, "Miller Construction Yard"); // OPA
        yard(self, 6, "Tycho Shipyards"); // independent → private sector
        for name in ["Toth Station", "Medina Drift"] {
            if let Some(b) = belt(self, name) {
                self.zero_g_stations
                    .push(ZeroGStation::new(6, b, name, ZeroGKind::Habitat));
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
        // 3b. Food consumption: settlements (colonies + station crews) eat from their local store.
        self.run_food_consumption();
        // 3b2. National income: nations mint GDP/tax from their capital populations (their money
        //      source to fund fleets, refuelling, construction). The sole nation-side money tap.
        self.run_population_income();
        // 3c. Shipyards consume Alloys + Electronics (the terminal demand sink for those goods).
        self.run_shipyards();
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
            // Remote outpost crews run **closed-loop life support** — they grow their own food
            // (net-zero against the crew's draw in `run_food_consumption`), so a belt mining
            // station never starves waiting on a long-haul food convoy. The real food economy is
            // the population centres (colonies + habitats), which trade for it.
            st.add(commodity::FOOD, st.food_demand());
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
            // Produce only if *every* input is on hand (× the rate) and there's output headroom.
            let fed = r.inputs.iter().all(|(g, n)| f.input_of(*g) >= f.rate * n);
            if fed && f.output_of(r.out) < FACILITY_OUTPUT_CAP {
                for (g, n) in &r.inputs {
                    f.add_input(*g, -(f.rate * n));
                }
                f.add_output(r.out, f.rate);
            }
        }
    }

    /// Settlements consume Food from their local store each tick (population/crew demand). When the
    /// store runs out the population simply goes hungry — a shortage the QA harness flags; haulers
    /// restock from the markets (the Food-supply jobs in `find_job`). No RNG.
    fn run_food_consumption(&mut self) {
        for col in &mut self.colonies {
            let d = col.food_demand();
            col.add(commodity::FOOD, -d);
        }
        for st in &mut self.mining_stations {
            let d = st.food_demand();
            st.add(commodity::FOOD, -d);
        }
        for z in &mut self.zero_g_stations {
            let d = z.food_demand();
            z.add(commodity::FOOD, -d);
        }
    }

    /// National income: each **nation** (Earth/Mars/OPA) mints `population × INCOME_PER_CAPITA_BP/BP`
    /// from every colony it owns (its capital) — the domestic GDP/tax base not modelled in the goods
    /// sim, and the nations' real money source to fund fleets, refuelling, and construction. Private
    /// actors own no colonies (only stations) so they get none — their liquidity is the bottomless
    /// free-market abstraction instead. Integer, no RNG.
    fn run_population_income(&mut self) {
        for ci in 0..self.colonies.len() {
            let owner = self.colonies[ci].owner;
            if self.players[owner as usize].kind.is_nation() {
                let income = self.colonies[ci].population * INCOME_PER_CAPITA_BP / BP / POP_SCALE;
                self.credit(owner, income);
            }
        }
        // The light commercial actors (companies + pirates) have a small off-screen revenue so they
        // stay solvent — the nations earn from population, the Private Sector is bottomless.
        for p in 0..self.players.len() {
            let income = match self.players[p].kind {
                super::player::PlayerKind::Company => ENTERPRISE_INCOME,
                super::player::PlayerKind::Pirates => RAIDING_INCOME,
                _ => 0,
            };
            if income > 0 {
                self.credit(p as u16, income);
            }
        }
    }

    /// Shipyards **buy + consume** Alloys + Electronics from the markets on a build cadence — the
    /// terminal demand sink for those goods (the tightening lever). Modelled as a direct, **treasury-
    /// gated** market purchase (the government won't build itself bankrupt) rather than a hauler job,
    /// so it's a clean bounded sink. Cross-owner buys pay the tax. Accumulates `progress`; on
    /// completion the materials are sunk (a `Ship` is spawned only when `SHIP_PROCUREMENT_ENABLED` —
    /// off — so the world is byte-identical to no-procurement bar the consumption). No RNG.
    fn run_shipyards(&mut self) {
        use super::commodity::{MACHINE_PARTS, SHIP_COMPONENTS};
        if !self.tick.is_multiple_of(SHIPYARD_INTERVAL) {
            return;
        }
        for zi in 0..self.zero_g_stations.len() {
            if !self.zero_g_stations[zi].is_shipyard() {
                continue;
            }
            let owner = self.zero_g_stations[zi].owner;
            if self.players[owner as usize].credits < SHIPYARD_MIN_TREASURY {
                continue; // can't afford to build right now
            }
            let at = self.zero_g_stations[zi].body;
            // The terminal demand sink — pulls the whole new chain (raw → … → Ship Components).
            for good in [SHIP_COMPONENTS, MACHINE_PARTS] {
                if let Some(m) = self.best_buy_market(good, at) {
                    let q = SHIPYARD_BATCH.min(self.markets[m].available_to_buy(good));
                    if q <= 0 {
                        continue;
                    }
                    let mkt_owner = self.markets[m].owner();
                    let cost = self.markets[m].execute_buy(good, q);
                    self.debit(owner, cost);
                    if mkt_owner != owner {
                        let tax = cost * TAX_BP / BP;
                        self.pay(owner, mkt_owner, tax);
                    }
                    self.zero_g_stations[zi].progress += q; // materials consumed (sunk)
                }
            }
            if self.zero_g_stations[zi].progress >= SHIP_BUILD_COST {
                self.zero_g_stations[zi].progress = 0;
                // SHIP_PROCUREMENT_ENABLED is false — no Ship spawned (framework only).
                let _ = SHIP_PROCUREMENT_ENABLED;
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
    /// Transfer `amount` from `payer` to `payee` (conserving — no mint/burn).
    fn pay(&mut self, payer: PlayerId, payee: PlayerId, amount: i64) {
        self.debit(payer, amount);
        self.credit(payee, amount);
    }

    /// Body index a `SiteRef` lives at.
    fn site_body(&self, site: SiteRef) -> usize {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].body,
            SiteRef::Facility(j) => self.facilities[j].body,
            SiteRef::Colony(c) => self.colonies[c].body,
            SiteRef::ZeroG(z) => self.zero_g_stations[z].body,
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
                        let mkt_owner = self.markets[market].owner();
                        let cost = self.markets[market].execute_buy(commodity, q);
                        self.markets[market].release_buy(commodity, job.qty);
                        let fee = cost * BROKER_FEE_BP / BP;
                        self.debit(owner, cost + fee);
                        // Cross-owner purchase tax → the market-owning government (a transfer, so it
                        // tightens trade *and* funds the nations; same-owner buys are untaxed).
                        if mkt_owner != owner {
                            let tax = cost * TAX_BP / BP;
                            self.pay(owner, mkt_owner, tax);
                        }
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
            SiteRef::ZeroG(z) => self.zero_g_stations[z].get(good),
            // Buying from a market is added in the greedy-arbitrage commit; not a source yet.
            SiteRef::Market { .. } => 0,
        }
    }

    fn site_take(&mut self, site: SiteRef, good: usize, qty: i64) {
        match site {
            SiteRef::Station(k) => self.mining_stations[k].add(good, -qty),
            SiteRef::Facility(j) => self.facilities[j].add_output(good, -qty),
            SiteRef::Colony(c) => self.colonies[c].add(good, -qty),
            SiteRef::ZeroG(z) => self.zero_g_stations[z].add(good, -qty),
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
            SiteRef::ZeroG(z) => self.zero_g_stations[z].add(good, qty),
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

        // (C) supply a starved same-owner facility. **Prefer the owner's own raw** (a station/colony
        // with the input on hand) — a direct internal haul, no market round-trip + no tax — falling
        // back to buying from the cheapest market only if the owner mines none of it.
        for (j, f) in self.facilities.iter().enumerate() {
            if f.owner != owner {
                continue;
            }
            // A multi-input facility can be starved on any one feedstock — route each below its
            // low-water mark independently.
            for (input, _ratio) in f.kind.recipe().inputs {
                if f.input_of(input) >= FACILITY_LOW_WATER {
                    continue;
                }
                // Own-raw source first (free internal feed).
                let mut own_src = None;
                for (k, st) in self.mining_stations.iter().enumerate() {
                    if st.owner == owner && st.get(input) >= LOAD_MIN {
                        own_src = Some((SiteRef::Station(k), st.body, st.get(input)));
                        break;
                    }
                }
                if let Some((src, src_body, avail)) = own_src {
                    let qty = cap.min(avail).min(FACILITY_INPUT_CAP - f.input_of(input));
                    if qty >= LOAD_MIN {
                        let d = dist2(src_body, f.body);
                        cands.push((
                            SUPPLY_BONUS - fuel_est(d),
                            d,
                            Job {
                                good: input,
                                from: src,
                                to: SiteRef::Facility(j),
                                phase: JobPhase::ToPickup,
                                qty,
                            },
                        ));
                        continue;
                    }
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
        }

        // (D) supply a same-owner consumer below its buffer (Food → settlements + zero-G stations;
        //     Alloys/Electronics → shipyards) by buying from the cheapest market — the demand that
        //     closes each consumer loop.
        use super::commodity::FOOD;
        let mut supply =
            |s: &Self, good: usize, have: i64, want: i64, dest: SiteRef, dest_body: usize| {
                if want <= 0 || have >= want {
                    return;
                }
                // Prefer a same-owner facility that **produces** this good (a direct internal haul,
                // e.g. Hydroponics → settlement). Cheap consumer goods never sell well into a market,
                // so without this the output piles up (production stalls) while settlements starve.
                for (j, f) in s.facilities.iter().enumerate() {
                    if f.owner == owner && f.output_of(good) >= LOAD_MIN {
                        let qty = cap.min(f.output_of(good)).min(want - have);
                        if qty >= LOAD_MIN {
                            let d = dist2(f.body, dest_body);
                            cands.push((
                                CONSUMER_BONUS - fuel_est(d),
                                d,
                                Job {
                                    good,
                                    from: SiteRef::Facility(j),
                                    to: dest,
                                    phase: JobPhase::ToPickup,
                                    qty,
                                },
                            ));
                            return;
                        }
                    }
                }
                if let Some(m) = s.best_buy_market(good, dest_body) {
                    let qty = cap
                        .min(s.markets[m].available_to_buy(good))
                        .min(want - have);
                    if qty < LOAD_MIN {
                        return;
                    }
                    let cost = s.markets[m].price(good) * qty;
                    let d = dist2(s.markets[m].body(), dest_body);
                    let value = CONSUMER_BONUS - cost - fuel_est(d);
                    cands.push((
                        value,
                        d,
                        Job {
                            good,
                            from: SiteRef::Market {
                                market: m,
                                commodity: good,
                            },
                            to: dest,
                            phase: JobPhase::ToPickup,
                            qty,
                        },
                    ));
                }
            };
        for (c, col) in self.colonies.iter().enumerate() {
            if col.owner == owner {
                let want = col.food_demand() * FOOD_BUFFER_TICKS;
                supply(
                    self,
                    FOOD,
                    col.get(FOOD),
                    want,
                    SiteRef::Colony(c),
                    col.body,
                );
            }
        }
        for (k, st) in self.mining_stations.iter().enumerate() {
            if st.owner == owner {
                let want = st.food_demand() * FOOD_BUFFER_TICKS;
                supply(self, FOOD, st.get(FOOD), want, SiteRef::Station(k), st.body);
            }
        }
        for (z, zg) in self.zero_g_stations.iter().enumerate() {
            if zg.owner == owner {
                // Food for the population/crew (shipyard materials are bought directly in
                // run_shipyards, not via haulers).
                let want = zg.food_demand() * FOOD_BUFFER_TICKS;
                supply(self, FOOD, zg.get(FOOD), want, SiteRef::ZeroG(z), zg.body);
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
            .any(|f| f.owner == owner && f.kind.recipe().inputs.iter().any(|(g, _)| *g == good))
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

    /// Refuel docked ships **by buying Fusion Fuel** from the market network (no longer free) — the
    /// terminal consumer Fusion Fuel lacked, and the fuel expense nations cover from their population
    /// income. A docked ship tops its tank from the best fuel market (cost + broker fee, + the
    /// cross-owner tax to the market owner), removing that stock. If no market has fuel within reach
    /// it tops up free — a fallback so a ship can never be permanently stranded. No RNG.
    fn run_ship_upkeep(&mut self) {
        use super::commodity::FUSION_FUEL;
        for i in 0..self.ships.len() {
            if self.ships[i].in_flight() {
                continue;
            }
            let cap = super::ship::ship_stats(&self.ships[i]).fuel_capacity;
            let deficit = cap - self.ships[i].fuel;
            if deficit <= 0 {
                continue;
            }
            let owner = self.ships[i].owner;
            let body = self.ships[i].body;
            if let Some(m) = self.best_buy_market(FUSION_FUEL, body) {
                let q = deficit.min(self.markets[m].available_to_buy(FUSION_FUEL));
                if q > 0 {
                    let mkt_owner = self.markets[m].owner();
                    let cost = self.markets[m].execute_buy(FUSION_FUEL, q);
                    let fee = cost * BROKER_FEE_BP / BP;
                    self.debit(owner, cost + fee);
                    if mkt_owner != owner {
                        self.pay(owner, mkt_owner, cost * TAX_BP / BP);
                    }
                    self.ships[i].fuel += q;
                    continue;
                }
            }
            // No reachable fuel market with stock — top up free (no permanent strand).
            self.ships[i].fuel = cap;
        }
    }

    // ---- ship movement ----------------------------------------------------------------

    /// Launch ship `i` toward body `dest` on a **light Hohmann transfer**: the ETA is the
    /// radii-derived transfer time (so it can be fixed before the moving target is led), and fuel is
    /// burned lump-sum. Returns false (and does not launch) if already in flight / already there /
    /// out of fuel.
    pub fn launch_ship(&mut self, i: usize, dest: usize) -> bool {
        let Some(sh) = self.ships.get(i) else {
            return false;
        };
        if sh.in_flight() || sh.body == dest || dest >= self.bodies.len() {
            return false;
        }
        let dist = orbit::distance(&self.bodies, sh.body, dest, self.tick);
        let speed = super::ship::ship_stats(sh).speed;
        let travel = orbit::hohmann_ticks(&self.bodies, sh.body, dest, speed);
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

    /// A ship's render position: its docked body, or a **curved transfer arc** from where it
    /// departed to where the destination *will be* on arrival (leading the moving target).
    pub fn ship_pos(&self, i: usize) -> (i64, i64) {
        let Some(sh) = self.ships.get(i) else {
            return (0, 0);
        };
        match sh.dest {
            Some(d) => {
                let span = sh.arrival.saturating_sub(sh.departed).max(1) as i64;
                let num = self.tick.saturating_sub(sh.departed) as i64;
                let from = orbit::position_of(&self.bodies, sh.body, sh.departed);
                let to = orbit::position_of(&self.bodies, d, sh.arrival); // the led arrival point
                orbit::transfer_arc(from, to, num, span)
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
    pub fn zero_g_stations(&self) -> &[ZeroGStation] {
        &self.zero_g_stations
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
            zero_g_stations: self.zero_g_stations.clone(),
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
        sim.zero_g_stations = s.zero_g_stations;
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
    fn a_docked_ship_buys_its_fuel_from_the_market_not_free() {
        use super::super::commodity::FUSION_FUEL;
        let mut sim = Sim::new(0);
        // A human hauler docks at Ceres (markets[2]), a Fusion Fuel producer. Empty its tank, then
        // run the refuel phase: it should *buy* fuel — paying credits and drawing the market's
        // Fusion-Fuel stock down — rather than topping up for free.
        let ceres_body = sim.markets()[2].body();
        let i = sim
            .ships
            .iter()
            .position(|s| s.class == ShipClass::Hauler && s.body == ceres_body)
            .expect("a hauler docked at Ceres");
        let owner = sim.ships[i].owner;
        sim.ships[i].fuel = 0;
        let cred0 = sim.players[owner as usize].credits;
        let stock0 = sim.markets[2].stock(FUSION_FUEL);
        sim.run_ship_upkeep();
        assert!(sim.ships[i].fuel > 0, "the ship refuelled");
        assert!(
            sim.players[owner as usize].credits < cred0,
            "fuel cost the owner credits (not free)"
        );
        assert!(
            sim.markets[2].stock(FUSION_FUEL) < stock0,
            "refuelling drew Fusion Fuel from the market (a terminal consumer)"
        );
    }

    #[test]
    fn nations_earn_population_income_but_private_actors_dont() {
        let mut sim = Sim::new(0);
        // Sum the expected per-tick income directly from the nation-owned colonies.
        let expected: i64 = sim
            .colonies()
            .iter()
            .filter(|c| sim.players()[c.owner as usize].kind.is_nation())
            .map(|c| c.population * INCOME_PER_CAPITA_BP / BP / POP_SCALE)
            .sum();
        assert!(expected > 0, "nations have capital populations");
        // A private player owns no colony, so the income phase pays it nothing. Capture credits of
        // a nation and a private actor, run ONLY the income phase, and compare the deltas.
        let earth = 1usize; // United Nations (Earth) — a nation
        let priv6 = 6usize; // Private Sector — not a nation, owns no colony
                            // Income is minted **per colony** (floored), so sum the per-colony incomes — not the
                            // summed population (which would round differently).
        let earth_income: i64 = sim
            .colonies()
            .iter()
            .filter(|c| c.owner as usize == earth)
            .map(|c| c.population * INCOME_PER_CAPITA_BP / BP / POP_SCALE)
            .sum();
        let (e0, p0) = (sim.players()[earth].credits, sim.players()[priv6].credits);
        sim.run_population_income();
        assert_eq!(
            sim.players()[earth].credits - e0,
            earth_income,
            "Earth earned exactly its population income"
        );
        assert_eq!(
            sim.players()[priv6].credits - p0,
            0,
            "the Private Sector earns no population income"
        );
    }

    #[test]
    fn the_human_is_player_zero_and_starts_empty() {
        // Blank-slate start: player 0 is the human with the opening credit treasury but **no**
        // ships, stations, or colonies (acquisition is a deliberate follow-up).
        let sim = Sim::new(1);
        assert_eq!(sim.players()[0].kind, PlayerKind::Human);
        assert!(sim.human_credits() > 0, "the human starts with a treasury");
        assert_eq!(sim.human_ship_count(ShipClass::Hauler), 0);
        assert_eq!(sim.human_ship_count(ShipClass::Miner), 0);
        assert_eq!(sim.human_mining_station_count(), 0);
        assert_eq!(sim.human_colony_count(), 0);
    }

    #[test]
    fn the_economy_is_active_and_every_player_stays_solvent() {
        // In the tighter (taxed, terminal-demand) economy, not everyone gets richer — a heavy
        // importer can shed credits to taxes/material costs. The honest gate: the economy is
        // **active** (total credits move) and **no player goes bankrupt** (all stay solvent).
        let mut sim = Sim::new(0);
        let total0: i64 = sim.players().iter().map(|p| p.credits).sum();
        for _ in 0..6_000 {
            sim.step();
        }
        let total1: i64 = sim.players().iter().map(|p| p.credits).sum();
        assert_ne!(total0, total1, "the economy is active (credits move)");
        // No catastrophic bankruptcy — a power may run a modest deficit funding infrastructure.
        for p in sim.players() {
            assert!(
                p.credits > DEFICIT_FLOOR,
                "{} not bankrupt: {}",
                p.name,
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
                assert!(
                    p.credits > DEFICIT_FLOOR,
                    "seed {seed}: {} bankrupt",
                    p.name
                );
            }
        }
    }

    #[test]
    fn shipyards_buy_materials_as_a_demand_sink_but_procure_no_ships() {
        use super::super::commodity::{MACHINE_PARTS, SHIP_COMPONENTS};
        let mut sim = Sim::new(0);
        let zi = sim
            .zero_g_stations
            .iter()
            .position(|z| z.is_shipyard())
            .unwrap();
        let owner = sim.zero_g_stations[zi].owner;
        // The shipyard buys Ship Components + Machine Parts from the market (the terminal demand →
        // it pays + market stock drops), and advances its build progress. Procurement disabled.
        let ships0 = sim.ships.len();
        let cred0 = sim.players[owner as usize].credits;
        let stock0: i64 = (0..sim.markets.len())
            .map(|m| sim.markets[m].stock(SHIP_COMPONENTS) + sim.markets[m].stock(MACHINE_PARTS))
            .sum();
        sim.run_shipyards();
        let stock1: i64 = (0..sim.markets.len())
            .map(|m| sim.markets[m].stock(SHIP_COMPONENTS) + sim.markets[m].stock(MACHINE_PARTS))
            .sum();
        assert!(
            stock1 < stock0,
            "shipyards drew Ship Components/Machine Parts from the markets (a demand sink)"
        );
        assert!(
            sim.players[owner as usize].credits < cred0,
            "the shipyard owner paid for materials"
        );
        assert!(
            sim.zero_g_stations[zi].progress > 0,
            "build progress advanced"
        );
        assert_eq!(
            sim.ships.len(),
            ships0,
            "procurement disabled — no ship spawned"
        );
    }

    #[test]
    fn the_named_zero_g_stations_are_seeded_on_station_bodies() {
        let sim = Sim::new(0);
        // The four named shipyards + a couple of habitats exist; the human owns none.
        for name in [
            "Bush Naval Yard",
            "Kirino Station",
            "Miller Construction Yard",
            "Tycho Shipyards",
            "Toth Station",
        ] {
            let z = sim
                .zero_g_stations()
                .iter()
                .find(|z| z.name == name)
                .unwrap_or_else(|| panic!("{name} seeded"));
            assert_eq!(
                sim.bodies()[z.body].kind,
                super::super::orbit::BodyKind::Station
            );
            assert_ne!(z.owner, sim.human(), "{name} not human-owned");
        }
        // A shipyard orbits its capital (its body's parent is the capital), a habitat is deep space.
        let bush = sim
            .zero_g_stations()
            .iter()
            .find(|z| z.name == "Bush Naval Yard")
            .unwrap();
        assert_eq!(
            sim.bodies()[bush.body].parent,
            3,
            "Bush Naval Yard orbits Earth"
        );
        let toth = sim
            .zero_g_stations()
            .iter()
            .find(|z| z.name == "Toth Station")
            .unwrap();
        assert_eq!(
            sim.bodies()[toth.body].parent,
            0,
            "Toth Station is deep space (heliocentric)"
        );
        // Determinism holds with the new bodies/stations.
        let mut a = Sim::new(3);
        let mut b = Sim::new(3);
        for _ in 0..500 {
            a.step();
            b.step();
        }
        assert_eq!(a.to_save(), b.to_save());
    }

    #[test]
    fn zero_g_stations_model_habitats_and_shipyards() {
        use super::super::commodity::FOOD;
        let hab = ZeroGStation::new(6, 5, "Toth Station", ZeroGKind::Habitat);
        let yard = ZeroGStation::new(1, 3, "Bush Naval Yard", ZeroGKind::Shipyard);
        // A habitat is a Capital-scale population; a shipyard a Station-scale crew.
        assert!(hab.food_demand() > yard.food_demand());
        assert!(yard.is_shipyard() && !hab.is_shipyard());
        // Both boot with a food larder and an idle (disabled) procurement order.
        assert!(hab.get(FOOD) > 0 && yard.get(FOOD) > 0);
        assert_eq!(yard.order, None);
        // Round-trips through a save (the new field is carried).
        let mut sim = Sim::new(0);
        sim.zero_g_stations.push(yard.clone());
        let b = Sim::load_bytes(&sim.save_bytes()).unwrap();
        assert_eq!(b.zero_g_stations(), sim.zero_g_stations());
    }

    #[test]
    fn settlements_consume_food_and_are_restocked_by_haulers() {
        use super::super::commodity::FOOD;
        // A tiered demand exists, and over a run haulers keep most settlements fed from the markets
        // (the Food trade loop closes: Hydroponics/markets → haulers → settlements).
        let sim0 = Sim::new(0);
        assert_eq!(
            sim0.colonies()[0].tier(),
            SettlementTier::Capital,
            "Earth pop 8e9 = Capital"
        );
        assert!(sim0.colonies()[0].food_demand() > sim0.mining_stations()[0].food_demand());
        let mut sim = Sim::new(0);
        for _ in 0..4_000 {
            sim.step();
        }
        let settlements = sim.colonies().len() + sim.mining_stations().len();
        let fed = sim.colonies().iter().filter(|c| c.get(FOOD) > 0).count()
            + sim
                .mining_stations()
                .iter()
                .filter(|s| s.get(FOOD) > 0)
                .count();
        assert!(
            fed * 5 >= settlements * 4,
            "haulers keep most settlements fed: {fed}/{settlements}"
        );
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
        // The human starts blank, so grant it the simplest object-driven chain in-test: a belt
        // station (mines raw) + two haulers (one keeps the station fed, one earns under the slower
        // Hohmann travel). Credits should then rise.
        let mut sim = Sim::new(0);
        let psyche = sim.bodies.iter().position(|b| b.name == "Psyche").unwrap();
        let ceres = sim.markets()[2].body();
        sim.mining_stations.push(MiningStation::new(0, psyche));
        for _ in 0..2 {
            sim.ships
                .push(Ship::new(0, ShipClass::Hauler, "Hauler", ceres));
        }
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
    fn a_launched_ship_rides_a_hohmann_transfer_to_the_moving_target() {
        let mut sim = Sim::new(0);
        // Grant the (blank-slate) human a Miner. A Miner isn't auto-dispatched, so it stays under
        // our manual control. Launch it to Earth (3).
        let ceres = sim.markets()[2].body();
        sim.ships
            .push(Ship::new(0, ShipClass::Miner, "Prospector", ceres));
        let i = sim
            .ships
            .iter()
            .position(|s| s.owner == 0 && s.class == ShipClass::Miner)
            .unwrap();
        let dest = 3;
        assert!(!sim.ships[i].in_flight());
        let fuel0 = sim.ships[i].fuel;
        let speed = super::super::ship::ship_stats(&sim.ships[i]).speed;
        let want = super::super::orbit::hohmann_ticks(&sim.bodies, sim.ships[i].body, dest, speed);
        assert!(sim.launch_ship(i, dest));
        assert!(sim.ships[i].in_flight() && sim.ships[i].fuel < fuel0);
        let (departed, arrival) = (sim.ships[i].departed, sim.ships[i].arrival);
        assert_eq!(
            arrival - departed,
            want,
            "ETA == radii-derived Hohmann time"
        );
        while sim.tick() < arrival {
            sim.step();
            if sim.tick() < arrival {
                assert!(sim.ships[i].in_flight(), "in flight until arrival");
            }
        }
        assert!(!sim.ships[i].in_flight(), "docked at arrival");
        assert_eq!(
            sim.ships[i].body, dest,
            "docked at the (moving) destination body"
        );
        assert!(!sim.launch_ship(i, dest));
    }

    #[test]
    fn hohmann_transfers_scale_with_orbital_distance() {
        // An outer-system transfer takes longer than an inner one (radii-derived).
        let sim = Sim::new(0);
        let h = |a: usize, b: usize| super::super::orbit::hohmann_ticks(&sim.bodies, a, b, 180);
        let earth_mars = h(3, 4);
        let earth_jupiter = h(3, 6);
        assert!(
            earth_jupiter > earth_mars,
            "Jupiter transfer ({earth_jupiter}) > Mars ({earth_mars})"
        );
        assert!(earth_mars >= 1);
    }

    #[test]
    fn mining_stations_extract_their_bodys_raw_into_a_local_store() {
        let raw = super::super::commodity::raw_count();
        let mut sim = Sim::new(0);
        // The first belt mining station accrues *raw* over time (food larder excluded).
        let raw0: i64 = (0..raw).map(|c| sim.mining_stations[0].get(c)).sum();
        assert_eq!(raw0, 0, "no raw mined yet");
        for _ in 0..50 {
            sim.step();
        }
        let raw1: i64 = (0..raw).map(|c| sim.mining_stations[0].get(c)).sum();
        assert!(
            raw1 > 0,
            "the station mined its body's raw into its store ({raw1})"
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
        // Simulate one production tick by hand (mirrors run_production: AlloyPlant ← Ore).
        let (ore_in, ratio) = f.kind.recipe().inputs[0];
        assert_eq!(ore_in, ORE);
        let need = f.rate * ratio;
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
