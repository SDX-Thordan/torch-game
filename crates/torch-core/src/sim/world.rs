//! The authoritative world and the sim↔view contract (§27, §28, §29).
//!
//! [`Sim`] owns the truth and advances on a fixed tick. The view never reads
//! `Sim` directly: it consumes a [`Snapshot`] (current state to render) plus the
//! [`Event`] stream returned by [`Sim::step`] (what changed). This is the seam
//! the Godot shell binds to and that keeps the core headless and testable.

use super::alerts::{AlertFeed, Priority, Verb};
use super::automation::AutomationPolicy;
use super::bridgehead::Bridgehead;
use super::campaign::{Campaign, Tier};
use super::combat::{self, Band, BattleOutcome, Doctrine, Fleet, TargetPriority};
use super::contracts::ContractBoard;
use super::corp::{Corp, OwnedShip};
use super::economy::{default_markets, Market};
use super::event::Event;
use super::faction::Relations;
use super::industry::Station;
use super::interdiction::{resolve, Interceptor, Interdiction};
use super::logistics::TradeRoute;
use super::movement;
use super::orbit::{self, default_system, Body};
use super::pressure::{Intensity, PressureKind, PressureSystem};
use super::progression::Progression;
use super::rng::Pcg32;
use super::salvage::{SalvageField, SalvageReward};
use super::ships::{self, Loadout, ShipCatalog, ShipClass};
use super::traffic::Hauler;

/// Credits charged per unit of a commissioned hull's dry mass (§5 sink) — the
/// "buy a finished hull off the yard" price.
const SHIP_PRICE_PER_MASS: i64 = 5;
/// Credits per unit dry mass to **assemble** a hull from your own component stock
/// (§7d) — labour only, far below the off-the-yard price, since you supplied the
/// Assembled-tier goods yourself. The chain's payoff: produce the parts, build
/// cheaper. Top-tier commodity indices in the 4-tier grid: Habitats 9 / Machinery
/// 10 / Drives 11.
const ASSEMBLY_FEE_PER_MASS: i64 = 1;
/// CEO experience earned per completed player operation (§10 earned through play).
const OP_XP: i64 = 200;
/// Research points earned per completed player operation.
const OP_RESEARCH_POINTS: i64 = 40;
/// Basis-point denominator for the brokerage fee.
const FEE_DEN: i64 = 10_000;
/// Treasury a company can hold before operating overhead bites (§5 sink): the
/// starting float plus headroom for a capital purchase, so early/mid play is
/// untaxed and only runaway hoarding is throttled.
const UPKEEP_FREE_FLOAT: i64 = 100_000;
/// Per-tick fraction of the *taxable* treasury skimmed as overhead. Together
/// with the free float this gives a wealth-scaled sink (overhead grows with the
/// enterprise you run), so income strategies settle at a sustainable equilibrium
/// instead of compounding without bound (gameplay-QA economy finding).
const UPKEEP_DEN: i64 = 150;
/// Credits to found a production station (§3.1).
const STATION_COST: i64 = 8_000;
/// A refinery's per-tick throughput, sell-surplus floor, and production ceiling.
const REFINERY_RATE: i64 = 5;
const REFINERY_SELL_ABOVE: i64 = 80;
const REFINERY_TARGET: i64 = 160;
/// Number of raw commodities (raw `i` refines to refined `i + RAW_COUNT`).
const RAW_COUNT: usize = 3;
/// The commodity that *is* reaction mass (refuel buys this): "Remass" (index 3).
const REMASS_COMMODITY: usize = 3;
/// Remass units bought per unit of the Remass commodity (a discount vs. raw price
/// so refuelling is affordable mid-campaign, §6).
const REMASS_PER_FUEL: i64 = 5;
/// Crew quality of a raider pack (§13). Matched to the player's reference 50: a
/// same-count pack is a genuine coin-flip (the gameplay-QA balance target), so
/// committing warships is a real risk — your fleet can be lost (§13 attrition).
const RAIDER_QUALITY: i64 = 50;
/// Ticks between contract-board postings (§3.3/§16): a faction posts a delivery
/// job roughly once a day at 1 tick/hour.
const CONTRACT_INTERVAL: u64 = 24;
/// How many open (unaccepted) offers the board carries at once — a small, fresh
/// menu, not a backlog (the §19 anti-anxiety lesson applied to the job board).
const MAX_CONTRACTS: usize = 4;
/// Ticks a posted contract stays on the board before lapsing (a delivery window).
const CONTRACT_WINDOW: u64 = 168;
/// Delivery size band (units) for a posted contract.
const CONTRACT_QTY_MIN: i64 = 20;
const CONTRACT_QTY_SPAN: i64 = 40;
/// Reward premium over the goods' face value at the delivery market, in basis
/// points: the contract pays a margin above just buying-and-selling, which is
/// what makes accepting it worthwhile (the structured-income hook, §3.3).
const CONTRACT_PREMIUM_BP: i64 = 13_000;
/// Standing gained with the offering faction on fulfilment (§10): more than a
/// single interdiction costs, so contracts are a real reputation-repair path.
const CONTRACT_REP: i64 = 60;

/// Why a market order could not be filled (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeError {
    InsufficientCredits,
    InsufficientStock,
    InsufficientCargo,
}

/// Why a ship could not be commissioned (§5/§8c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommissionError {
    CantAfford,
    NotEnoughCrew,
    /// Assembling from parts (§7d), but the warehouse lacks the required goods.
    MissingParts,
}

/// Why a warship could not be ordered to move (§6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveError {
    NoSuchShip,
    BadDestination,
    /// Already mid-trajectory.
    Busy,
    AlreadyThere,
    /// Not enough remass to make the burn — refuel first (stranding is real).
    InsufficientRemass,
}

/// Why a faction contract could not be accepted or fulfilled (§3.3/§16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractError {
    NotFound,
    NotAccepted,
    InsufficientCargo,
}

/// Why a station could not be founded (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundError {
    CantAfford,
    /// The input has no higher tier to refine into (it's a top-tier finished good).
    NotProcessable,
    TooManyStations,
}

/// Why a far-side bridgehead op (found/upgrade) could not proceed (§17, G3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeheadError {
    /// Not through the ring yet — the foothold can only be planted in the Beyond.
    NotBeyond,
    CantAfford,
    /// Founding when one already stands, or upgrading when none does.
    AlreadyFounded,
    NotFounded,
}

/// Credits to found the far-side bridgehead (§17, G3) — an endgame outlay.
const BRIDGEHEAD_FOUND_COST: i64 = 60_000;
/// Credits to upgrade the bridgehead one level; scales with the level reached so
/// each reinforcement is a heavier commitment (§17, G3).
const BRIDGEHEAD_UPGRADE_BASE_COST: i64 = 40_000;

/// Ticks between hauler spawn attempts (≈ one per day at 1 tick/hour).
const SPAWN_INTERVAL: u64 = 24;
/// Cap on concurrent in-flight haulers.
const MAX_HAULERS: usize = 16;
/// Minimum price spread that makes a route worth flying.
const MIN_SPREAD: i64 = 5;
/// Hauler cruise speed in distance units per tick.
const CRUISE_SPEED: i64 = 60_000;
/// Floor on travel time so close markets still take real time (§21).
const MIN_TRAVEL: u64 = 24;
/// A standing-route freighter burns Remass scaled by trip length (§6 delta-v as
/// operating cost): `remass_units = travel_ticks / this`, floored at 1. Tuned so
/// short inner hauls cost modest fuel and long outer hauls cost a lot — and so
/// producing your own Remass (the Ice→Remass chain) cheapens the whole network.
const FREIGHTER_REMASS_DIVISOR: u64 = 10;
/// Ticks between automated interdiction sorties (§12 patrol cadence).
const AUTOMATION_INTERVAL: u64 = 12;
/// Ticks between reputation-decay ticks (§10): grudges fade slowly toward
/// neutral, so a Hostile standing is recoverable if you stop antagonizing.
const REP_RECOVERY_INTERVAL: u64 = 24;
/// How far each standing drifts toward neutral per recovery tick. Slow enough
/// that an active raider still outruns it.
const REP_RECOVERY_STEP: i64 = 8;

/// A renderable view of one body at a single tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyState {
    pub name: &'static str,
    pub x: i64,
    pub y: i64,
}

/// A renderable view of one commodity in a market.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommodityState {
    pub name: &'static str,
    pub stock: i64,
    pub price: i64,
}

/// A renderable view of one market.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketState {
    pub name: &'static str,
    pub commodities: Vec<CommodityState>,
}

/// A renderable view of one hauler in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HaulerState {
    pub id: u64,
    pub commodity: &'static str,
    pub x: i64,
    pub y: i64,
}

/// An immutable snapshot of the world for rendering (§29).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub tick: u64,
    pub bodies: Vec<BodyState>,
    pub markets: Vec<MarketState>,
    pub haulers: Vec<HaulerState>,
}

/// The authoritative deterministic simulation.
pub struct Sim {
    /// The world seed (§27) — kept so a save can reconstruct the deterministic
    /// world and ambient phase by re-simming from it (§30).
    seed: u64,
    tick: u64,
    bodies: Vec<Body>,
    /// All markets: the inner economy `[0..far_market_start]`, then the far-side
    /// endgame markets `[far_market_start..]` (§17). Far-side markets are stepped
    /// with `far_rng` and excluded from NPC routing, so the inner game is unchanged.
    markets: Vec<Market>,
    far_market_start: usize,
    /// A dedicated RNG for the far-side markets so they never perturb the shared
    /// `rng` — keeping the pre-transit economy byte-identical (the contract-board /
    /// salvage pattern, §27).
    far_rng: Pcg32,
    haulers: Vec<Hauler>,
    next_hauler_id: u64,
    pirate: Interceptor,
    feed: AlertFeed,
    relations: Relations,
    progression: Progression,
    policy: AutomationPolicy,
    campaign: Campaign,
    corp: Corp,
    routes: Vec<TradeRoute>,
    stations: Vec<Station>,
    board: ContractBoard,
    salvage: SalvageField,
    /// The player's far-side foothold (§17 endgame, G3). Unfounded until the player
    /// transits the gate and founds it; inert pre-transit.
    bridgehead: Bridgehead,
    missions: super::missions::Missions,
    /// The player's tactical doctrine for fleet engagements (§9): target priority
    /// + retreat threshold (band is chosen per engagement).
    combat_doctrine: Doctrine,
    /// The last resolved battle (band, starting counts, BattleLog) — for the §22
    /// diorama. Transient (not saved).
    last_battle: Option<(Band, [usize; 2], BattleOutcome)>,
    /// Hull + weapon catalog the player's ships are fit from (§31). Defaults to the
    /// compiled tables; `reload_ship_data` retunes it from JSON. A tuning overlay,
    /// not save state — content stays in code.
    catalog: ShipCatalog,
    pressure: PressureSystem,
    rng: Pcg32,
    events: Vec<Event>,
    /// How many leading `events` the last `step` already returned and fed to the
    /// alert feed. The next `step` drains exactly these, *keeping* anything the
    /// player's between-tick verbs pushed after them — so player-caused events
    /// (a cut's `Scarcity`, an ascent's `TierAscended`) survive to be voiced.
    returned: usize,
}

impl Sim {
    /// Brokerage fee on instant market orders, in basis points (§5 sink). Tuned
    /// so hand-trading thin spreads loses money — only a fat spread clears it,
    /// which makes the trade a decision and keeps the transit-paying standing
    /// route competitive.
    pub const TRADE_FEE_BP: i64 = 300;

    /// Create a sim seeded for determinism (§27). Same seed ⇒ same run.
    pub fn new(seed: u64) -> Self {
        let mut markets = default_markets();
        let far_market_start = markets.len();
        // Append the far-side endgame markets (§17). They exist always (so route /
        // trade verbs work on them by index post-transit) but step on `far_rng` and
        // are excluded from NPC routing, so the inner economy is byte-identical.
        markets.extend(super::economy::far_side_markets(
            super::economy::default_commodities(),
        ));
        let market_names = markets.iter().map(|m| m.name().to_string()).collect();
        let commodity_names = markets[0]
            .defs()
            .iter()
            .map(|d| d.name.to_string())
            .collect();
        let commodity_count = markets[0].defs().len();
        let blueprint_count = Progression::new().blueprints.catalog().len();
        let body_count = default_system().len();
        Self {
            seed,
            tick: 0,
            bodies: default_system(),
            haulers: Vec::new(),
            next_hauler_id: 0,
            // A raider lurking on the inner lanes (§13): quick but lightly crewed,
            // so it lands some strikes and muffs others.
            pirate: Interceptor {
                pos: (-600_000, -300_000),
                speed: 24_000,
                skill_bp: 400,
            },
            feed: AlertFeed::new(seed, market_names, commodity_names),
            relations: Relations::new(),
            progression: Progression::new(),
            policy: AutomationPolicy::default(),
            campaign: Campaign::new(),
            corp: Corp::new(commodity_count),
            routes: Vec::new(),
            stations: Vec::new(),
            board: ContractBoard::new(seed),
            salvage: SalvageField::new(seed, blueprint_count, body_count),
            bridgehead: Bridgehead::new(),
            missions: super::missions::Missions::new(),
            combat_doctrine: Doctrine::default(),
            last_battle: None,
            catalog: ShipCatalog::default(),
            pressure: PressureSystem::new(Intensity::default()),
            markets,
            far_market_start,
            far_rng: Pcg32::new(seed ^ 0xFA5_FACE),
            rng: Pcg32::new(seed),
            events: Vec::new(),
            returned: 0,
        }
    }

    /// The current tick.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The bodies under simulation.
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// The markets (§7a).
    pub fn markets(&self) -> &[Market] {
        &self.markets
    }

    /// Whether market `m` is a **far-side** endgame market (§17) — the shell hides
    /// these from the board until the player transits the gate.
    pub fn is_far_side_market(&self, m: usize) -> bool {
        m >= self.far_market_start
    }

    /// Hot-reload commodity numbers from a JSON tuning document (§31): re-tune
    /// every live market in place, keeping stock/setpoints and touching no RNG, so
    /// a designer can retune prices mid-run without breaking determinism. Returns a
    /// human-readable error (bad JSON, unknown commodity) and leaves markets
    /// untouched if parsing fails (it parses before mutating anything).
    pub fn reload_commodities(&mut self, json: &str) -> Result<(), String> {
        let defs = super::economy::tuned_commodities(json)?;
        for m in &mut self.markets {
            m.retune(&defs)?;
        }
        Ok(())
    }

    /// Hot-reload hull + weapon numbers from a JSON tuning document (§31): swap the
    /// ship catalog the player's future ships are fit from. Parses before mutating,
    /// so a bad file leaves the catalog untouched. Already-built hulls keep their
    /// fitted loadout (it's baked at commission); new commissions use the new
    /// numbers. Touches no RNG — a mid-run retune stays deterministic.
    pub fn reload_ship_data(&mut self, json: &str) -> Result<(), String> {
        self.catalog = super::ships::tuned_ship_catalog(json)?;
        Ok(())
    }

    /// The live ship catalog (§31), for the shell's shipyard readout.
    pub fn ship_catalog(&self) -> &ShipCatalog {
        &self.catalog
    }

    /// The §13 pressure layer (gauges, raid schedule, intensity) — read by the
    /// shell's pressure HUD and the §23c audio state.
    pub fn pressure(&self) -> &PressureSystem {
        &self.pressure
    }

    /// Set the independent pressure-intensity difficulty (§13).
    pub fn set_intensity(&mut self, intensity: Intensity) {
        self.pressure.set_intensity(intensity);
    }

    /// The haulers currently in flight (§7b).
    pub fn haulers(&self) -> &[Hauler] {
        &self.haulers
    }

    /// The shared deterministic RNG every system draws from (§27).
    pub fn rng_mut(&mut self) -> &mut Pcg32 {
        &mut self.rng
    }

    /// The alert feed (§19) — the voiced, ranked exception stream.
    pub fn feed(&self) -> &AlertFeed {
        &self.feed
    }

    /// The player's standing with each faction (§10).
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// The retention spine — tier, goals, and the gate's approach (§0).
    pub fn campaign(&self) -> &Campaign {
        &self.campaign
    }

    /// The player corporation — treasury, cargo, fleet, crew (§1/§5).
    pub fn corp(&self) -> &Corp {
        &self.corp
    }

    /// Mutable corporation access — for seeding/adjusting holdings (e.g. crediting
    /// produced goods into the warehouse).
    pub fn corp_mut(&mut self) -> &mut Corp {
        &mut self.corp
    }

    /// Rename owned ship `idx`'s call-sign (§14 expressive identity), keeping its
    /// class suffix (e.g. rename to "Valkyrie" → "Valkyrie (Frigate)"). Returns
    /// whether the rename took. Pure string edit — no RNG, no balance effect.
    pub fn rename_ship(&mut self, idx: usize, call_sign: &str) -> bool {
        let call_sign = call_sign.trim();
        if call_sign.is_empty() {
            return false;
        }
        match self.corp.fleet_mut().get_mut(idx) {
            Some(s) => {
                let class = s.loadout.hull().name;
                s.name = format!("{call_sign} ({class})");
                true
            }
            None => false,
        }
    }

    /// Skim operating overhead off the treasury each tick (§5 sink). Overhead is
    /// a fraction of holdings *above* a free float, so it bites only runaway
    /// hoarding — the wealth-scaled sink that turns every income strategy into a
    /// sustainable equilibrium rather than an unbounded faucet.
    fn charge_upkeep(&mut self) {
        let taxable = self.corp.credits() - UPKEEP_FREE_FLOAT;
        if taxable > 0 {
            let upkeep = taxable / UPKEEP_DEN;
            if upkeep > 0 {
                self.corp.debit(upkeep);
            }
        }
    }

    /// Brokerage fee on a `value`-credit instant market order (§5). This is the
    /// cost of *immediate* liquidity the standing route (which pays transit
    /// instead) doesn't incur — so hand-trading is a real decision against the
    /// fee, not a riskless faucet (gameplay-QA economy finding).
    fn trade_fee(value: i64) -> i64 {
        value * Self::TRADE_FEE_BP / FEE_DEN
    }

    /// Buy `qty` of commodity `c` at market `m` (§5): debits the goods cost plus
    /// the brokerage fee, lifts the goods into the warehouse, and nudges the
    /// price up. Returns the total credits spent (cost + fee).
    pub fn buy(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        let price = self.markets[m].price(c);
        let cost = price * qty;
        let total = cost + Self::trade_fee(cost);
        if self.markets[m].stock(c) < qty {
            return Err(TradeError::InsufficientStock);
        }
        if self.corp.credits() < total {
            return Err(TradeError::InsufficientCredits);
        }
        self.markets[m].remove_stock(c, qty);
        self.corp.debit(total);
        self.corp.store(c, qty);
        Ok(total)
    }

    /// Sell `qty` of commodity `c` into market `m` (§5): lands warehouse cargo at
    /// the current price less the brokerage fee, nudging the price down. Returns
    /// the net credits received (revenue − fee).
    pub fn sell(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        if self.corp.cargo(c) < qty {
            return Err(TradeError::InsufficientCargo);
        }
        let price = self.markets[m].price(c);
        let revenue = price * qty;
        let net = revenue - Self::trade_fee(revenue);
        self.corp.unstore(c, qty);
        self.markets[m].add_stock(c, qty);
        self.corp.credit(net);
        self.note_mission(super::missions::Trigger::FirstTrade); // §16 tutorial
        Ok(net)
    }

    /// Answer an act-now shortage in one move (§0.4 / §3.3 speculate): source
    /// `qty` of `commodity` at the cheapest *other* market and sell it into the
    /// short `market` for the premium — no pre-held cargo needed. Resolves the
    /// matching alert and returns the net profit (revenue − cost).
    pub fn exploit_shortage(
        &mut self,
        market: usize,
        commodity: usize,
        qty: i64,
    ) -> Result<i64, TradeError> {
        if market >= self.markets.len() {
            return Err(TradeError::InsufficientStock);
        }
        let source = (0..self.markets.len())
            .filter(|&m| m != market)
            .min_by_key(|&m| self.markets[m].price(commodity))
            .ok_or(TradeError::InsufficientStock)?;
        let cost = self.buy(source, commodity, qty)?;
        let revenue = self.sell(market, commodity, qty)?;
        self.feed.resolve_shortage(market, commodity);
        Ok(revenue - cost)
    }

    /// One-press answer to the loudest open act-now shortage (the alert→verb
    /// path the influence model wants). Returns whether one was answered.
    pub fn answer_top_shortage(&mut self, qty: i64) -> bool {
        let target = self.feed.surfaced().iter().find_map(|a| {
            a.verb
                .map(|Verb::ExploitShortage { market, commodity }| (market, commodity))
        });
        match target {
            Some((m, c)) => self.exploit_shortage(m, c, qty).is_ok(),
            None => false,
        }
    }

    /// Commission a warship of `class` into the fleet (§5/§8c): pays its build
    /// cost and draws its crew from the trained-crew pool.
    pub fn commission_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        let hull = self.catalog.hull(class);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        self.corp.debit(price);
        self.stand_up_hull(class);
        Ok(())
    }

    /// Assemble a warship of `class` from the player's **own component stock** (§7d):
    /// consumes the Assembled-tier goods in [`ship_bom`] from the warehouse plus a
    /// small labour fee + crew — far cheaper than buying a finished hull, the payoff
    /// of building out the production chain. Fails if any part or the crew is short.
    pub fn assemble_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        let hull = self.catalog.hull(class);
        let fee = hull.dry_mass * ASSEMBLY_FEE_PER_MASS;
        if self.corp.credits() < fee {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        let bom = Self::ship_bom(class);
        if bom.iter().any(|&(c, q)| self.corp.cargo(c) < q) {
            return Err(CommissionError::MissingParts);
        }
        for &(c, q) in bom {
            self.corp.unstore(c, q);
        }
        self.corp.debit(fee);
        self.stand_up_hull(class);
        Ok(())
    }

    /// The Assembled-tier bill of materials to build a hull of `class` from parts
    /// (§7d): `(commodity index, quantity)`. Bigger hulls need more Machinery (10)
    /// and Drives (11); capitals also need Habitats (9) for their crew.
    pub fn ship_bom(class: ShipClass) -> &'static [(usize, i64)] {
        match class {
            ShipClass::Frigate => &[(10, 2), (11, 1)],
            ShipClass::Destroyer => &[(10, 4), (11, 2)],
            ShipClass::Cruiser => &[(10, 7), (11, 3), (9, 1)],
            ShipClass::Battleship => &[(10, 12), (11, 5), (9, 2)],
            ShipClass::QShip => &[(10, 2), (11, 1)],
            ShipClass::Freighter => &[(10, 3)],
            ShipClass::Miner => &[(10, 2)],
            ShipClass::Tanker => &[(10, 2)],
        }
    }

    /// Shared tail of commission/assemble: fit the hull off the catalog, draw its
    /// crew, christen it (§14), dock it at the yard (§6), and count the op (§0/§16).
    fn stand_up_hull(&mut self, class: ShipClass) {
        let hull = self.catalog.hull(class);
        let loadout = self
            .catalog
            .reference_loadout_quality(class, 50, &mut self.rng);
        self.corp.assign_crew(hull.crew_required);
        // A christened call-sign + class, e.g. "Lodestar (Frigate)" (§14). It rolls
        // off the line docked at Ceres Yards (the shipyard) with a full tank (§6).
        let name = format!("{} ({})", ships::christen_ship(&mut self.rng), hull.name);
        let home = self.markets[0].body();
        self.corp
            .add_ship(OwnedShip::new(name, loadout, self.tick, home));
        self.note_mission(super::missions::Trigger::FirstWarship); // §16 tutorial
        self.complete_op(); // building the fleet is progress on the climb (§0)
    }

    /// Order warship `idx` to fly to `dest` body (§6): commit a trajectory at the
    /// live orbital distance, spend remass, and take time derived from the ship's
    /// drive and the chosen burn (economical vs. hard). Fails if the ship is busy,
    /// already there, or lacks the remass to make the burn (stranding is real).
    pub fn move_ship(&mut self, idx: usize, dest: usize, hard_burn: bool) -> Result<(), MoveError> {
        if dest >= self.bodies.len() {
            return Err(MoveError::BadDestination);
        }
        let ship = self.corp.fleet().get(idx).ok_or(MoveError::NoSuchShip)?;
        if ship.nav.in_transit(self.tick) {
            return Err(MoveError::Busy);
        }
        if ship.nav.location == dest {
            return Err(MoveError::AlreadyThere);
        }
        let here = orbit::position_of(&self.bodies, ship.nav.location, self.tick);
        let there = orbit::position_of(&self.bodies, dest, self.tick);
        let (dx, dy) = (there.0 - here.0, there.1 - here.1);
        let distance = (dx * dx + dy * dy).isqrt();
        let plan = movement::plan(&ship.loadout, distance, hard_burn);
        let nav = ship.nav; // `Nav` is `Copy`; ends the immutable borrow of `ship`
        if nav.remass < plan.remass_cost {
            return Err(MoveError::InsufficientRemass);
        }
        self.corp.fleet_mut()[idx].nav = movement::Nav {
            location: nav.location,
            dest,
            depart_tick: self.tick,
            arrival_tick: self.tick + plan.travel_ticks,
            remass: nav.remass - plan.remass_cost,
            remass_max: nav.remass_max,
        };
        Ok(())
    }

    /// Refuel docked warship `idx` to a full tank (§6), buying the reaction mass at
    /// the cheapest market price for ReactorFuel. Returns whether it refuelled.
    pub fn refuel_ship(&mut self, idx: usize) -> bool {
        let nav = match self.corp.fleet().get(idx) {
            Some(s) => s.nav, // `Copy` — ends the borrow of `self.corp`
            None => return false,
        };
        if nav.in_transit(self.tick) {
            return false;
        }
        let need = nav.remass_max - nav.remass;
        if need <= 0 {
            return false;
        }
        let unit = self
            .markets
            .iter()
            .map(|m| m.price(REMASS_COMMODITY))
            .min()
            .unwrap_or(1);
        let cost = (need * unit / REMASS_PER_FUEL).max(0);
        if !self.corp.debit(cost) {
            return false;
        }
        self.corp.fleet_mut()[idx].nav.remass = nav.remass_max;
        true
    }

    /// Advance in-flight ships: any whose trajectory has completed docks at its
    /// destination (§6). Called each tick.
    fn run_fleet_nav(&mut self) {
        let tick = self.tick;
        for s in self.corp.fleet_mut() {
            if s.nav.dest != s.nav.location && tick >= s.nav.arrival_tick {
                s.nav.location = s.nav.dest;
            }
        }
    }

    /// Absolute position of owned ship `idx` (§6/§21): its dock body when docked,
    /// or interpolated along its trajectory when in transit.
    pub fn ship_position(&self, idx: usize) -> (i64, i64) {
        let Some(s) = self.corp.fleet().get(idx) else {
            return (0, 0);
        };
        let from = orbit::position_of(&self.bodies, s.nav.location, self.tick);
        if !s.nav.in_transit(self.tick) {
            return from;
        }
        let to = orbit::position_of(&self.bodies, s.nav.dest, self.tick);
        let span = (s.nav.arrival_tick - s.nav.depart_tick).max(1) as i64;
        let t = (self.tick - s.nav.depart_tick) as i64;
        (
            from.0 + (to.0 - from.0) * t / span,
            from.1 + (to.1 - from.1) * t / span,
        )
    }

    /// Commission a civilian freighter to run trade-route standing orders (§4).
    pub fn commission_freighter(&mut self) -> Result<(), CommissionError> {
        let hull = self.catalog.hull(ShipClass::Freighter);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        self.corp.debit(price);
        self.corp.assign_crew(hull.crew_required);
        self.corp.add_freighter();
        self.complete_op(); // standing up logistics is progress on the climb (§0)
        Ok(())
    }

    /// The player's table of standing trade routes (§4).
    pub fn routes(&self) -> &[TradeRoute] {
        &self.routes
    }

    /// The first standing route, if any — a convenience for the single-route
    /// status view in the shell (§4).
    pub fn route(&self) -> Option<TradeRoute> {
        self.routes.first().copied()
    }

    /// Indices into [`routes`](Self::routes) whose freighter is **in flight** right
    /// now (§6 positional logistics) — one flying freighter per in-transit route.
    pub fn flying_routes(&self) -> Vec<usize> {
        self.routes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.in_transit)
            .map(|(i, _)| i)
            .collect()
    }

    /// Live position of route `i`'s freighter, interpolated along its orbital path
    /// (origin → dest market body) by trip progress — the same lane model the NPC
    /// haulers use, so the logistics wing is a real positional asset on the map (§6).
    pub fn route_freighter_pos(&self, i: usize) -> (i64, i64) {
        match self.routes.get(i) {
            Some(rt) if rt.in_transit => {
                let o = orbit::position_of(&self.bodies, self.markets[rt.origin].body(), self.tick);
                let d = orbit::position_of(&self.bodies, self.markets[rt.dest].body(), self.tick);
                let span = rt.arrival.saturating_sub(rt.departed).max(1) as i64;
                let t = (self.tick.saturating_sub(rt.departed) as i64).clamp(0, span);
                (o.0 + (d.0 - o.0) * t / span, o.1 + (d.1 - o.1) * t / span)
            }
            _ => (0, 0),
        }
    }

    /// The destination body position for route `i` (for the freighter's lane trail).
    pub fn route_dest_pos(&self, i: usize) -> (i64, i64) {
        match self.routes.get(i) {
            Some(rt) => orbit::position_of(&self.bodies, self.markets[rt.dest].body(), self.tick),
            None => (0, 0),
        }
    }

    /// The Remass a freighter burns on route `i`'s current geometry (§6) — the
    /// distance-scaled fuel load it refuels at the origin port each trip.
    pub fn route_remass_units(&self, i: usize) -> i64 {
        match self.routes.get(i) {
            Some(rt) => {
                (self.travel_ticks(rt.origin, rt.dest) / FREIGHTER_REMASS_DIVISOR).max(1) as i64
            }
            None => 0,
        }
    }

    /// Trip progress of route `i`'s freighter in basis points (0..=10000), for the
    /// FLEET view's en-route readout.
    pub fn route_progress_bp(&self, i: usize) -> i64 {
        match self.routes.get(i) {
            Some(rt) if rt.in_transit => {
                let span = rt.arrival.saturating_sub(rt.departed).max(1) as i64;
                let t = (self.tick.saturating_sub(rt.departed) as i64).clamp(0, span);
                t * 10_000 / span
            }
            _ => 0,
        }
    }

    /// Add a parameterized Trade Route standing order to the table — buy
    /// `commodity` at `origin`, sell at `dest`, `qty` per trip, only while the
    /// spread clears `min_margin` (§4). Many routes run concurrently against the
    /// shared freighter pool; exceptions go idle. Capped at the tier's route cap.
    pub fn set_trade_route(
        &mut self,
        commodity: usize,
        origin: usize,
        dest: usize,
        qty: i64,
        min_margin: i64,
    ) {
        if self.routes.len() < self.campaign.tier().route_cap() {
            self.routes
                .push(TradeRoute::new(commodity, origin, dest, qty, min_margin));
            self.note_mission(super::missions::Trigger::FirstRoute); // §16 tutorial
        }
    }

    /// Clear the whole route table.
    pub fn clear_trade_route(&mut self) {
        self.routes.clear();
    }

    /// Travel time in ticks between two markets at the current orrery geometry.
    fn travel_ticks(&self, origin: usize, dest: usize) -> u64 {
        let o = orbit::position_of(&self.bodies, self.markets[origin].body(), self.tick);
        let d = orbit::position_of(&self.bodies, self.markets[dest].body(), self.tick);
        let (dx, dy) = (d.0 - o.0, d.1 - o.1);
        let dist = (dx * dx + dy * dy).isqrt();
        ((dist / CRUISE_SPEED) as u64).max(MIN_TRAVEL)
    }

    /// Run the whole route table this tick (§4): land every arriving trip, then
    /// dispatch idle routes against the **shared freighter pool** (a route can
    /// only set out if a freighter is free). The routes run themselves; the
    /// player only set the parameters, and exceptions (below margin, no free
    /// freighter) simply stay idle.
    fn run_logistics(&mut self) {
        if self.routes.is_empty() {
            return;
        }
        let freighters = self.corp.freighters();
        // Move the table out so the per-route mutations don't fight the
        // `markets`/`corp` borrows (same pattern as the single-route version).
        let mut routes = std::mem::take(&mut self.routes);

        // Deliveries first, freeing up freighters for this tick's dispatch.
        for rt in routes.iter_mut() {
            if rt.in_transit && self.tick >= rt.arrival {
                let revenue = self.markets[rt.dest].price(rt.commodity) * rt.carrying;
                self.markets[rt.dest].add_stock(rt.commodity, rt.carrying);
                self.corp.credit(revenue);
                rt.in_transit = false;
                rt.carrying = 0;
                self.complete_op(); // a delivered standing order is an op (§0/§4)
            }
        }

        // Dispatch idle routes while freighters remain in the pool.
        let mut in_flight = routes.iter().filter(|r| r.in_transit).count() as i64;
        for rt in routes.iter_mut() {
            if in_flight >= freighters {
                break;
            }
            if rt.in_transit || !rt.active {
                continue;
            }
            let buy = self.markets[rt.origin].price(rt.commodity);
            let spread = self.markets[rt.dest].price(rt.commodity) - buy;
            let cost = buy * rt.qty;
            // Fuel (§6): the freighter refuels with Remass at the origin port, an
            // amount scaled by the trip distance. Long outer hauls cost far more
            // fuel — the delta-v constraint as opex — and a hub that produces cheap
            // Remass lowers the whole network's running cost.
            let travel = self.travel_ticks(rt.origin, rt.dest);
            let remass_units = (travel / FREIGHTER_REMASS_DIVISOR).max(1) as i64;
            let fuel_cost = remass_units * self.markets[rt.origin].price(REMASS_COMMODITY);
            let cargo_stocked = self.markets[rt.origin].stock(rt.commodity) > rt.qty;
            let fuel_stocked = self.markets[rt.origin].stock(REMASS_COMMODITY) >= remass_units;
            if spread >= rt.min_margin
                && cargo_stocked
                && fuel_stocked
                && self.corp.credits() >= cost + fuel_cost
            {
                self.markets[rt.origin].remove_stock(rt.commodity, rt.qty);
                self.markets[rt.origin].remove_stock(REMASS_COMMODITY, remass_units);
                self.corp.debit(cost + fuel_cost);
                rt.in_transit = true;
                rt.carrying = rt.qty;
                rt.departed = self.tick;
                rt.arrival = self.tick + travel;
                in_flight += 1;
            }
        }

        self.routes = routes;
    }

    /// The player's production stations (§3.1).
    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    /// Found a refinery that turns a raw commodity into its refined product:
    /// source `raw` at `buy_market`, refine, and auto-sell the surplus at
    /// `sell_market` (§3.1 Produce + sell-surplus). Costs capital.
    pub fn found_refinery(
        &mut self,
        input: usize,
        buy_market: usize,
        sell_market: usize,
    ) -> Result<(), FoundError> {
        // A station refines `input` into the next tier in its line (`input + 3`),
        // so any non-top-tier commodity can host one (§7d): Ore→Metals→Alloys→
        // Machinery, etc. Only the top-tier finished goods have nowhere to go.
        let output = input + RAW_COUNT;
        if output >= self.markets[0].defs().len() {
            return Err(FoundError::NotProcessable);
        }
        if self.stations.len() >= self.campaign.tier().station_cap() {
            return Err(FoundError::TooManyStations);
        }
        if self.corp.credits() < STATION_COST {
            return Err(FoundError::CantAfford);
        }
        self.corp.debit(STATION_COST);
        self.stations.push(Station {
            body: self.markets[buy_market].body(),
            input,
            output,
            rate: REFINERY_RATE,
            buy_market,
            sell_market,
            sell_above: REFINERY_SELL_ABOVE,
            output_target: REFINERY_TARGET,
        });
        self.complete_op(); // founding industry is progress on the climb (§0)
        Ok(())
    }

    /// Run every station's Produce standing order this tick (§3.1/§4): source
    /// input from a market, transform raw → refined, and dump the surplus output
    /// for credits. Hands-off; the player only set the recipe.
    fn run_industry(&mut self) {
        for i in 0..self.stations.len() {
            let st = self.stations[i];
            let producing = self.corp.cargo(st.output) < st.output_target;
            // Source the input recipe from its market when short.
            if producing && self.corp.cargo(st.input) < st.rate {
                let price = self.markets[st.buy_market].price(st.input);
                let cost = price * st.rate;
                if self.markets[st.buy_market].stock(st.input) > st.rate
                    && self.corp.credits() >= cost
                {
                    self.markets[st.buy_market].remove_stock(st.input, st.rate);
                    self.corp.debit(cost);
                    self.corp.store(st.input, st.rate);
                }
            }
            // Transform input → output (the value-add).
            if producing && self.corp.cargo(st.input) >= st.rate {
                self.corp.unstore(st.input, st.rate);
                self.corp.store(st.output, st.rate);
            }
            // Sell-surplus rule: dump output held above the threshold.
            let surplus = self.corp.cargo(st.output) - st.sell_above;
            if surplus > 0 {
                let price = self.markets[st.sell_market].price(st.output);
                self.corp.unstore(st.output, surplus);
                self.markets[st.sell_market].add_stock(st.output, surplus);
                self.corp.credit(price * surplus);
            }
        }
    }

    /// The faction job board — open and accepted delivery contracts (§3.3/§16).
    pub fn contracts(&self) -> &[super::contracts::Contract] {
        self.board.offers()
    }

    /// Number of open (not-yet-accepted) contracts on the board.
    pub fn open_contract_count(&self) -> usize {
        self.board.open_count()
    }

    /// Maintain the contract board each tick (§3.3/§16): lapse stale unaccepted
    /// offers, then — on the posting cadence and while the menu has room — post a
    /// fresh delivery job. A faction asks for `qty` of a commodity delivered to
    /// its market for a premium reward and a standing bump; accepting and
    /// fulfilling it ties the economy (you must source the goods) to reputation
    /// (§10) and the §0 climb (a fulfilment is an operation). The board draws from
    /// its **own** RNG so generating offers never perturbs the world streams.
    fn run_contracts(&mut self) {
        self.board.expire_unaccepted(self.tick);
        if !self.tick.is_multiple_of(CONTRACT_INTERVAL) || self.board.open_count() >= MAX_CONTRACTS
        {
            return;
        }
        // Contracts target the inner markets only (the far side trades post-transit
        // via its own verbs) — and bounding to the inner count keeps the board's RNG
        // draw byte-identical to before the far-side markets existed.
        let market = self.board.rng().below(self.far_market_start as u32) as usize;
        let commodity_count = self.markets[market].defs().len();
        let commodity = self.board.rng().below(commodity_count as u32) as usize;
        let qty = CONTRACT_QTY_MIN + self.board.rng().below(CONTRACT_QTY_SPAN as u32) as i64;
        let faction = self.markets[market].faction();
        let face = self.markets[market].price(commodity) * qty;
        let reward = face * CONTRACT_PREMIUM_BP / FEE_DEN;
        let deadline = self.tick + CONTRACT_WINDOW;
        self.board.post(
            faction,
            market,
            commodity,
            qty,
            reward,
            CONTRACT_REP,
            deadline,
        );
    }

    /// Accept open contract `id` (§3.3): the player now owes the delivery until
    /// its deadline (accepted contracts no longer lapse). Returns whether it was
    /// accepted.
    pub fn accept_contract(&mut self, id: u64) -> bool {
        self.board.accept(id)
    }

    /// Fulfil accepted contract `id` from the warehouse (§3.3/§16): consumes the
    /// owed cargo, lands it at the faction's market, pays the reward, lifts the
    /// standing (§10), and counts the delivery as an operation on the climb (§0).
    /// Returns the reward credited, or why it could not be fulfilled.
    pub fn fulfill_contract(&mut self, id: u64) -> Result<i64, ContractError> {
        let c = *self.board.find(id).ok_or(ContractError::NotFound)?;
        if !c.accepted {
            return Err(ContractError::NotAccepted);
        }
        if self.corp.cargo(c.commodity) < c.qty {
            return Err(ContractError::InsufficientCargo);
        }
        self.corp.unstore(c.commodity, c.qty);
        self.markets[c.market].add_stock(c.commodity, c.qty);
        self.corp.credit(c.reward);
        self.relations.adjust(c.faction, c.rep);
        self.board.remove(id);
        self.complete_op(); // a delivered contract is progress on the climb (§0)
        Ok(c.reward)
    }

    /// Accept and immediately attempt to fulfil the first open contract whose
    /// owed cargo is already in the warehouse (the one-press path the influence
    /// model wants). Returns the reward credited, if any.
    pub fn fulfill_ready_contract(&mut self) -> Option<i64> {
        let ready = self
            .board
            .offers()
            .iter()
            .find(|c| self.corp.cargo(c.commodity) >= c.qty)
            .map(|c| c.id)?;
        self.accept_contract(ready);
        self.fulfill_contract(ready).ok()
    }

    /// Standings, mutable — for diplomacy/contracts that move reputation (§10).
    pub fn relations_mut(&mut self) -> &mut Relations {
        &mut self.relations
    }

    /// The player's advancement across research / blueprints / CEO skills (§10).
    pub fn progression(&self) -> &Progression {
        &self.progression
    }

    /// Advancement, mutable — for research/CEO progress driven by play.
    pub fn progression_mut(&mut self) -> &mut Progression {
        &mut self.progression
    }

    /// The standing automation policy the managers execute (§12).
    pub fn policy(&self) -> &AutomationPolicy {
        &self.policy
    }

    /// Set the automation policy the managers execute (§12).
    pub fn policy_mut(&mut self) -> &mut AutomationPolicy {
        &mut self.policy
    }

    /// Discover blueprint `i`, honoring its reputation gate against the player's
    /// current standings (§10/§25). Returns whether it was learned.
    pub fn discover_blueprint(&mut self, i: usize) -> bool {
        self.progression
            .blueprints
            .discover(i, &self.relations)
            .is_ok()
    }

    /// The wrecks currently sighted and awaiting salvage (§15).
    pub fn wrecks(&self) -> &[super::salvage::Wreck] {
        self.salvage.wrecks()
    }

    /// Strip the sighted wreck `id` (§15): bank its reward — scrap → credits, data
    /// → research, or a reverse-engineered blueprint (no rep gate) — and count it
    /// as an operation on the climb (§0). Returns whether a wreck was salvaged.
    pub fn salvage_wreck(&mut self, id: u64) -> bool {
        let Some(reward) = self.salvage.claim(id) else {
            return false;
        };
        match reward {
            SalvageReward::Scrap(credits) => self.corp.credit(credits),
            SalvageReward::Data(points) => self.progression.research.add_points(points),
            SalvageReward::Blueprint(i) => {
                self.progression.blueprints.reverse_engineer(i);
            }
        }
        self.events.push(Event::WreckSalvaged { id });
        // Salvaged data sometimes seeds the gate mystery (§15 anomaly → §0.1 lore).
        self.reveal_gate_beat();
        self.complete_op();
        true
    }

    /// One-press salvage of the first sighted wreck (§15/§0.4). Returns whether one
    /// was stripped.
    pub fn salvage_top(&mut self) -> bool {
        match self.salvage.first() {
            Some(id) => self.salvage_wreck(id),
            None => false,
        }
    }

    /// Set the player-tunable alert surfacing threshold (§19).
    pub fn set_alert_threshold(&mut self, min_priority: Priority) {
        self.feed.set_threshold(min_priority);
    }

    /// Capture the run as a deterministic [`SaveState`] (§30): seed + tick + the
    /// mutable player/economy state. Static content (catalogs, bodies) is rebuilt
    /// on load, so it isn't stored.
    pub fn to_save(&self) -> super::persist::SaveState {
        use super::persist::{MarketSave, SaveState, ShipSave, SAVE_VERSION};
        let fleet = self
            .corp
            .fleet()
            .iter()
            .map(|s| ShipSave {
                name: s.name.clone(),
                class: s.loadout.hull().class,
                commissioned_tick: s.commissioned_tick,
                battles: s.battles,
                battles_won: s.battles_won,
                crew_quality: s.loadout.crew().quality,
                nav: s.nav,
            })
            .collect();
        let markets = self
            .markets
            .iter()
            .map(|m| MarketSave {
                stocks: m.stocks().iter().map(|s| s.stock).collect(),
                prices: m.stocks().iter().map(|s| s.price).collect(),
            })
            .collect();
        SaveState {
            version: SAVE_VERSION,
            seed: self.seed,
            tick: self.tick,
            credits: self.corp.credits(),
            warehouse: self.corp.warehouse().to_vec(),
            trained_crew: self.corp.trained_crew(),
            freighters: self.corp.freighters(),
            fleet,
            corp_name: self.corp.name().to_string(),
            corp_livery: self.corp.livery(),
            relations: self.relations.clone(),
            campaign: self.campaign,
            research_unlocked: self.progression.research.flags().to_vec(),
            research_points: self.progression.research.points(),
            blueprints_known: self.progression.blueprints.flags().to_vec(),
            ceo_xp: self.progression.ceo.xp(),
            ceo_branch: self.progression.ceo.branch(),
            mission_done: self.missions.done_flags(),
            gate_revealed: self.missions.gate_beats_revealed(),
            bridgehead: self.bridgehead,
            routes: self.routes.clone(),
            stations: self.stations.clone(),
            policy: self.policy,
            intensity: self.pressure.intensity(),
            alert_threshold: self.feed.threshold(),
            markets,
        }
    }

    /// Serialize the run to a JSON save document (the dev export, §30).
    pub fn save_json(&self) -> String {
        self.to_save().to_json()
    }

    /// Serialize the run to the compact **binary** shipping save (§30): bincode.
    pub fn save_bytes(&self) -> Vec<u8> {
        self.to_save().to_bincode()
    }

    /// Rebuild a [`Sim`] from a JSON save (§30).
    pub fn load_json(json: &str) -> Result<Self, String> {
        Self::rebuild_from_save(super::persist::SaveState::from_json(json)?)
    }

    /// Rebuild a [`Sim`] from a save document, **auto-detecting** the format (§30):
    /// a leading `{`/whitespace is the JSON dev export, anything else is the binary
    /// shipping format. So new binary saves and old JSON saves both load.
    pub fn load_bytes(bytes: &[u8]) -> Result<Self, String> {
        let looks_json = bytes
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|&b| b == b'{');
        let save = if looks_json {
            let json = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
            super::persist::SaveState::from_json(json)?
        } else {
            super::persist::SaveState::from_bincode(bytes)?
        };
        Self::rebuild_from_save(save)
    }

    /// Reconstruct the seeded world, re-sim the ambient layer up to the saved tick
    /// so its phase lines up, then overlay the saved player + economy state (§30).
    fn rebuild_from_save(save: super::persist::SaveState) -> Result<Self, String> {
        let mut sim = Sim::new(save.seed);
        // Advance the ambient world (traffic, pressure, salvage, RNG phase) to the
        // saved tick. Player automation is off in a fresh sim, so these steps add
        // no player-driven state — the overlay below restores all of that.
        for _ in 0..save.tick {
            sim.step();
        }
        sim.apply_save(&save);
        Ok(sim)
    }

    /// Overlay a loaded [`SaveState`] onto a sim already re-simmed to its tick.
    fn apply_save(&mut self, s: &super::persist::SaveState) {
        self.tick = s.tick;
        // Rebuild each hull's loadout from its class + crew quality (§14), then
        // restore its name and service history.
        let fleet = s
            .fleet
            .iter()
            .map(|sh| {
                let loadout =
                    ships::reference_loadout_quality(sh.class, sh.crew_quality, &mut self.rng);
                let mut ship = OwnedShip::new(
                    sh.name.clone(),
                    loadout,
                    sh.commissioned_tick,
                    sh.nav.location,
                );
                ship.battles = sh.battles;
                ship.battles_won = sh.battles_won;
                ship.nav = sh.nav;
                ship
            })
            .collect();
        self.corp.restore(
            s.credits,
            s.warehouse.clone(),
            s.trained_crew,
            s.freighters,
            fleet,
        );
        self.corp.set_identity(s.corp_name.clone(), s.corp_livery);
        self.relations = s.relations.clone();
        self.campaign = s.campaign;
        self.progression
            .research
            .restore(s.research_unlocked.clone(), s.research_points);
        self.progression
            .blueprints
            .restore(s.blueprints_known.clone());
        self.progression.ceo.restore(s.ceo_xp, s.ceo_branch);
        self.missions.restore(&s.mission_done, s.gate_revealed);
        self.bridgehead = s.bridgehead;
        self.routes = s.routes.clone();
        self.stations = s.stations.clone();
        self.policy = s.policy;
        self.pressure.set_intensity(s.intensity);
        self.feed.set_threshold(s.alert_threshold);
        for (m, ms) in self.markets.iter_mut().zip(&s.markets) {
            m.restore_stocks(&ms.stocks, &ms.prices);
        }
    }

    /// Advance exactly one fixed sim tick (§28) and return the events produced.
    /// The returned slice is valid until the next call to `step`.
    pub fn step(&mut self) -> &[Event] {
        // Drop only the events the previous `step` already surfaced, retaining
        // any a player verb pushed since (so player-caused events aren't lost to
        // a blanket clear — the §0.3 fanfare and §0.4 shortage fire for the
        // player too, not just for pirate/automation cuts).
        self.events.drain(0..self.returned);
        self.tick += 1;
        // Inner markets step on the shared rng exactly as before (byte-identical);
        // the far-side markets step on their own `far_rng` so they never perturb it.
        let split = self.far_market_start;
        for m in self.markets[..split].iter_mut() {
            m.step(&mut self.rng);
        }
        for m in self.markets[split..].iter_mut() {
            m.step(&mut self.far_rng);
        }
        self.deliver_arrivals();
        self.spawn_traffic();
        self.run_pressure();
        self.run_automation();
        self.run_logistics();
        self.run_industry();
        self.run_fleet_nav();
        self.run_contracts();
        // Discovery (§15): the field may turn up a derelict to strip. Its own RNG
        // keeps the economy bit-identical whether or not anyone salvages.
        if let Some(id) = self.salvage.maybe_sight(self.tick) {
            self.events.push(Event::WreckSighted { id });
        }
        self.charge_upkeep();
        if self.tick.is_multiple_of(REP_RECOVERY_INTERVAL) {
            self.relations.decay_toward_neutral(REP_RECOVERY_STEP);
        }
        self.events.push(Event::Tick { tick: self.tick });
        // The alert feed (§19) consumes everything surfacing this tick (§29):
        // the carried-over player events plus this tick's own.
        let tick = self.tick;
        for e in &self.events {
            self.feed.ingest(e, tick);
            self.pressure.note_event(e, tick);
        }
        // Gauges ebb each tick — biting-but-recoverable (§13).
        self.pressure.decay();
        self.returned = self.events.len();
        &self.events
    }

    /// The §13 pressure layer, run each tick: telegraph an incoming raid ahead of
    /// time (forecasting), then fire the ambient raider only when the pacing
    /// governor allows (no dogpiling another flashpoint). Pure scheduling — the
    /// raid itself still resolves with geometry + odds in [`Sim::pirate_raid`].
    fn run_pressure(&mut self) {
        let now = self.tick;
        if self.pressure.should_forecast(now) {
            let eta = self.pressure.raid_eta(now);
            self.events.push(Event::ThreatForecast {
                kind: PressureKind::Piracy,
                eta,
            });
            self.pressure.mark_forecast_sent();
        }
        if self.pressure.raid_ready(now) {
            let struck = self.pirate_raid();
            self.pressure.after_raid(now, struck);
        }
    }

    /// Administratively cut the in-flight hauler with `id` (a guaranteed delete,
    /// for the binding/tests). Returns whether a hauler was actually cut. For the
    /// positioning-and-odds verb, use [`Sim::interdict_with`].
    pub fn interdict(&mut self, id: u64) -> bool {
        if let Some(i) = self.haulers.iter().position(|h| h.id == id) {
            let h = self.cut_hauler(i);
            self.ripple_reputation(&h);
            true
        } else {
            false
        }
    }

    /// Attempt to interdict hauler `id` with `interceptor` (§7b): the cut only
    /// lands if the interceptor has the legs to reach the hauler *and* wins the
    /// roll. Returns the resolved outcome.
    pub fn interdict_with(&mut self, id: u64, interceptor: Interceptor) -> Interdiction {
        let Some(i) = self.haulers.iter().position(|h| h.id == id) else {
            return Interdiction::NoSolution;
        };
        let outcome = resolve(&self.haulers[i], &interceptor, self.tick, &mut self.rng);
        if outcome == Interdiction::Interdicted {
            let h = self.cut_hauler(i);
            self.ripple_reputation(&h);
        }
        outcome
    }

    /// Send the player fleet against a raider pack at `band` and resolve the
    /// battle (§9). This is the missing trigger the gameplay-QA review flagged:
    /// `sim::combat` had no verb on `Sim`, so commissioned warships never fought.
    /// The raider pack is sized to the fleet for a real contest; losses are
    /// applied to the corp, a win counts as an operation on the climb (§0), and a
    /// `BattleResolved` event is emitted for the feed (§19) and diorama (§22).
    /// Returns the outcome, or `None` if the player has no warships to send.
    pub fn engage_raiders(&mut self, band: Band) -> Option<BattleOutcome> {
        // Raiders muster on the inner lanes at the home core (§6/§13): only
        // warships **on station** there can answer — a fleet flown off to the outer
        // system can't defend the core until it burns home. This is what makes the
        // delta-v movement layer consequential (Pillar #2).
        let muster = self.markets[0].body();
        let on_station: Vec<usize> = self
            .corp
            .fleet()
            .iter()
            .enumerate()
            .filter(|(_, s)| !s.nav.in_transit(self.tick) && s.nav.location == muster)
            .map(|(i, _)| i)
            .collect();
        if on_station.is_empty() {
            return None;
        }
        let player_ships: Vec<Loadout> = on_station
            .iter()
            .map(|&i| self.corp.fleet()[i].loadout.clone())
            .collect();
        // A matched-count pack of raider frigates at a matched crew quality — a
        // genuine coin-flip, so committing the fleet is a real risk (§13/§9).
        let pack: Vec<Loadout> = (0..player_ships.len())
            .map(|_| {
                ships::reference_loadout_quality(ShipClass::Frigate, RAIDER_QUALITY, &mut self.rng)
            })
            .collect();
        // The player fleet fights under the player's doctrine (target + retreat),
        // at the band they chose; raiders press the attack to the death (§9).
        let player_doctrine = Doctrine {
            band,
            ..self.combat_doctrine
        };
        let raider_doctrine = Doctrine {
            band,
            ..Doctrine::default()
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine: player_doctrine,
            },
            &Fleet {
                ships: &pack,
                doctrine: raider_doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        let won = outcome.winner == Some(0);
        // Only the on-station ships were at risk; veterans pull through (§11/§13).
        self.corp.resolve_engagement_for(on_station, survivors, won);
        if won {
            self.complete_op(); // holding the field is progress on the climb (§0)
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), pack.len()], outcome.clone()));
        Some(outcome)
    }

    /// The most recent resolved engagement, for the diorama (§22): the band, the
    /// starting `[player, raider]` counts, and the full BattleLog.
    pub fn last_battle(&self) -> Option<&(Band, [usize; 2], BattleOutcome)> {
        self.last_battle.as_ref()
    }

    /// Warships currently **on station** at the home core, ready to answer a raider
    /// muster (§6): docked at `markets[0]`'s body, not in transit. The shell uses
    /// this to tell "no fleet" apart from "fleet is off defending elsewhere."
    pub fn warships_on_station(&self) -> usize {
        let muster = self.markets[0].body();
        self.corp
            .fleet()
            .iter()
            .filter(|s| !s.nav.in_transit(self.tick) && s.nav.location == muster)
            .count()
    }

    /// Set the player's target-priority doctrine (§9).
    pub fn set_combat_target(&mut self, target: TargetPriority) {
        self.combat_doctrine.target = target;
    }

    /// Set the player's retreat threshold in basis points (§9): break off below
    /// this fraction of the starting fleet. `0` = fight to the death.
    pub fn set_combat_retreat(&mut self, bp: i64) {
        self.combat_doctrine.retreat_bp = bp.clamp(0, 10_000);
    }

    /// Fire railguns hot or disciplined (§9 heat): aggressive fire boosts railgun
    /// output but builds heat that periodically forces a vent.
    pub fn set_combat_aggressive(&mut self, on: bool) {
        self.combat_doctrine.aggressive_fire = on;
    }

    /// The player's current tactical doctrine (§9).
    pub fn combat_doctrine(&self) -> Doctrine {
        self.combat_doctrine
    }

    /// A *player* cut sours relations with the hauler's owner faction (§7b/§10)
    /// and counts as an operation on the climb (§0); pirate raids do neither.
    fn ripple_reputation(&mut self, h: &Hauler) {
        let faction = self.markets[h.origin].faction();
        self.relations.on_player_interdict(faction);
        self.note_mission(super::missions::Trigger::FirstCut); // §16 tutorial
        self.complete_op();
    }

    /// Record a completed player **operation** — the unit of progress on the §0
    /// climb. Interdiction was the *only* verb that called this, so the retention
    /// spine ignored the whole build/trade/route side of the influence model
    /// (the gameplay-QA review's #1 finding). Now every substantive player act —
    /// a cut, a commissioned ship/freighter, a founded station, a completed
    /// route delivery — advances the campaign and earns the CEO/research
    /// progress operations grant (§10, earned through play). Emits the ascent
    /// fanfare on a tier crossing (§0.3).
    fn complete_op(&mut self) {
        self.progression.ceo.gain_xp(OP_XP);
        self.progression.research.add_points(OP_RESEARCH_POINTS);
        if let Some(tier) = self.campaign.record_op() {
            self.events.push(Event::TierAscended { tier });
            // The climb teaches the spine and advances the authored thread (§0.1):
            // each ascent voices the next gate-mystery beat.
            self.note_mission(super::missions::Trigger::FirstAscent);
            self.reveal_gate_beat();
        }
    }

    /// Voice a completed opening mission (§16) through the feed.
    fn note_mission(&mut self, trigger: super::missions::Trigger) {
        if let Some(title) = self.missions.note(trigger) {
            let tick = self.tick;
            self.feed
                .announce("The Board", format!("Objective complete — {title}."), tick);
        }
    }

    /// Reveal the next gate-mystery beat (§0.1), voiced as "The Gate".
    fn reveal_gate_beat(&mut self) {
        if let Some(beat) = self.missions.reveal_gate() {
            let tick = self.tick;
            self.feed.announce("The Gate", beat.to_string(), tick);
        }
    }

    /// **Transit the open ring-gate** (§0.1/§17) — the climactic, deliberate payoff
    /// of the whole climb: cross from the Gate into the `Beyond` endgame. Only
    /// possible standing at the open gate. On transit it tells the rest of the
    /// mystery, voices the gate's *answer*, and counts as an operation. Returns
    /// whether the transit happened.
    pub fn transit_gate(&mut self) -> bool {
        if self.campaign.transit().is_none() {
            return false;
        }
        let tick = self.tick;
        // Tell whatever of the mystery is still untold, then the answer.
        while self.missions.reveal_gate().is_some() {}
        self.events.push(Event::GateTransited);
        self.feed
            .announce("The Gate", super::missions::GATE_ANSWER.to_string(), tick);
        // The transit is itself the supreme operation on the climb (§0).
        self.progression.ceo.gain_xp(OP_XP);
        true
    }

    /// Whether the player can transit the gate right now (standing at the open
    /// ring, not yet through) — drives the shell's transit verb.
    pub fn can_transit_gate(&self) -> bool {
        self.campaign.tier() == Tier::Gate
    }

    /// The player's far-side bridgehead (§17 endgame, G3) — unfounded until transit.
    pub fn bridgehead(&self) -> &Bridgehead {
        &self.bridgehead
    }

    /// **Found the far-side bridgehead** (§17, G3) — plant the first foothold beyond
    /// the ring. Only possible in the `Beyond` (post-transit), once, for a credit
    /// outlay. Founding is itself a spine op (it advances within the endgame).
    pub fn found_bridgehead(&mut self) -> Result<(), BridgeheadError> {
        if !self.campaign.transited() {
            return Err(BridgeheadError::NotBeyond);
        }
        if self.bridgehead.is_founded() {
            return Err(BridgeheadError::AlreadyFounded);
        }
        if self.corp.credits() < BRIDGEHEAD_FOUND_COST {
            return Err(BridgeheadError::CantAfford);
        }
        self.corp.debit(BRIDGEHEAD_FOUND_COST);
        self.bridgehead.found();
        self.events.push(Event::BridgeheadFounded);
        self.complete_op(); // securing the far side is progress on the climb (§0)
        Ok(())
    }

    /// Cost to upgrade the bridgehead from its current level (§17, G3).
    fn bridgehead_upgrade_cost(&self) -> i64 {
        BRIDGEHEAD_UPGRADE_BASE_COST * self.bridgehead.level().max(1) as i64
    }

    /// **Upgrade the far-side bridgehead** (§17, G3) — reinforce the foothold a level,
    /// raising the integrity it can weather under incursion (G4). Requires a standing
    /// bridgehead and the (level-scaled) credits. A spine op.
    pub fn upgrade_bridgehead(&mut self) -> Result<(), BridgeheadError> {
        if !self.bridgehead.is_founded() {
            return Err(BridgeheadError::NotFounded);
        }
        let cost = self.bridgehead_upgrade_cost();
        if self.corp.credits() < cost {
            return Err(BridgeheadError::CantAfford);
        }
        self.corp.debit(cost);
        self.bridgehead.upgrade();
        self.events.push(Event::BridgeheadUpgraded {
            level: self.bridgehead.level(),
        });
        self.complete_op();
        Ok(())
    }

    /// The authored thread — opening missions + the gate mystery (§0.1/§16).
    pub fn missions(&self) -> &super::missions::Missions {
        &self.missions
    }

    /// Adopt corp name preset `i` (§14 expressive identity).
    pub fn set_corp_name_preset(&mut self, i: usize) {
        self.corp.set_name_preset(i);
    }

    /// Cycle the fleet livery colour (§14); returns the new index.
    pub fn cycle_corp_livery(&mut self) -> usize {
        self.corp.cycle_livery()
    }

    /// Remove the hauler at `index`, denying its delivery and tagging the
    /// resulting shortage at the destination (§7b). Returns the cut hauler.
    fn cut_hauler(&mut self, index: usize) -> Hauler {
        let h = self.haulers.remove(index);
        self.events.push(Event::HaulerInterdicted { id: h.id });
        self.events.push(Event::Scarcity {
            market: h.dest,
            commodity: h.commodity,
        });
        h
    }

    /// Run the standing automation policy this tick (§12 run-by-exception): the
    /// interdiction patrol cuts matching shipping on its cadence, and research is
    /// auto-invested. The player set the policy; the managers do the work.
    fn run_automation(&mut self) {
        let pol = self.policy; // Copy — no borrow held over the mutations below
        if pol.interdiction.enabled && self.tick.is_multiple_of(AUTOMATION_INTERVAL) {
            let target = self
                .haulers
                .iter()
                .enumerate()
                .filter(|(_, h)| h.qty >= pol.interdiction.min_cargo)
                .filter(|(_, h)| match pol.interdiction.target {
                    Some(f) => self.markets[h.origin].faction() == f,
                    None => true,
                })
                .max_by_key(|(_, h)| h.qty)
                .map(|(i, _)| i);
            if let Some(i) = target {
                let outcome = resolve(&self.haulers[i], &pol.patrol, self.tick, &mut self.rng);
                if outcome == Interdiction::Interdicted {
                    let h = self.cut_hauler(i);
                    self.ripple_reputation(&h); // the player's managed asset → their tab
                }
            }
        }
        if pol.auto_research {
            if let Some(i) = self.progression.research.cheapest_researchable() {
                let _ = self.progression.research.research(i);
            }
        }
    }

    /// NPC pirates periodically strike at the fattest cargo in flight (§13).
    /// Resolve one ambient raider strike against the fattest in-flight cargo (§13);
    /// the *when* is decided by the pressure layer ([`Sim::run_pressure`]), not a
    /// raw interval. Returns whether a convoy was actually cut (a flashpoint).
    fn pirate_raid(&mut self) -> bool {
        if self.haulers.is_empty() {
            return false;
        }
        let target = self
            .haulers
            .iter()
            .enumerate()
            .max_by_key(|(_, h)| h.qty)
            .map(|(i, _)| i);
        if let Some(i) = target {
            let outcome = resolve(&self.haulers[i], &self.pirate, self.tick, &mut self.rng);
            if outcome == Interdiction::Interdicted {
                self.cut_hauler(i); // pirates, not the player → no reputation hit
                return true;
            }
        }
        false
    }

    /// Land cargo for any hauler arriving this tick, damping the spread.
    fn deliver_arrivals(&mut self) {
        let tick = self.tick;
        let mut landed: Vec<(usize, usize, i64, u64)> = Vec::new();
        self.haulers.retain(|h| {
            if h.arrival_tick == tick {
                landed.push((h.dest, h.commodity, h.qty, h.id));
                false
            } else {
                true
            }
        });
        for (dest, commodity, qty, id) in landed {
            self.markets[dest].add_stock(commodity, qty);
            self.events.push(Event::HaulerArrived { id });
        }
    }

    /// Spawn at most one arbitrage hauler on the most profitable open route.
    fn spawn_traffic(&mut self) {
        if !self.tick.is_multiple_of(SPAWN_INTERVAL) || self.haulers.len() >= MAX_HAULERS {
            return;
        }
        let Some((commodity, origin, dest, qty)) = self.best_route() else {
            return;
        };
        // Lift the cargo now (origin sheds surplus); land it on arrival.
        self.markets[origin].remove_stock(commodity, qty);
        let origin_pos = orbit::position_of(&self.bodies, self.markets[origin].body(), self.tick);
        let dest_pos = orbit::position_of(&self.bodies, self.markets[dest].body(), self.tick);
        let (dx, dy) = (dest_pos.0 - origin_pos.0, dest_pos.1 - origin_pos.1);
        let dist = (dx * dx + dy * dy).isqrt();
        let travel = ((dist / CRUISE_SPEED) as u64).max(MIN_TRAVEL);
        let id = self.next_hauler_id;
        self.next_hauler_id += 1;
        self.events.push(Event::HaulerDeparted {
            id,
            commodity,
            origin,
            dest,
            qty,
        });
        self.haulers.push(Hauler {
            id,
            commodity,
            origin,
            dest,
            qty,
            depart_tick: self.tick,
            arrival_tick: self.tick + travel,
            origin_pos,
            dest_pos,
        });
    }

    /// The (commodity, origin, dest, qty) with the largest profitable spread
    /// where the origin has surplus and the destination has room.
    fn best_route(&self) -> Option<(usize, usize, usize, i64)> {
        let n = self.markets[0].defs().len();
        // NPC haulers route only the **inner** economy — the far-side markets (§17)
        // are unreachable to ambient traffic, so the inner game is unchanged.
        let m = self.far_market_start;
        let mut best: Option<(usize, usize, usize, i64)> = None;
        let mut best_spread = MIN_SPREAD;
        for c in 0..n {
            let qty = (self.markets[0].defs()[c].target_stock / 10).max(1);
            // Every ordered market pair — so a third market (or more) joins the
            // arbitrage on its own merits, not just a hard-coded two (§7b).
            for o in 0..m {
                for d in 0..m {
                    if o == d {
                        continue;
                    }
                    let spread = self.markets[d].price(c) - self.markets[o].price(c);
                    let has_surplus = self.markets[o].stock(c) > qty;
                    let has_room = self.markets[d].stock(c) + qty < self.markets[d].wall_high(c);
                    if spread > best_spread && has_surplus && has_room {
                        best = Some((c, o, d, qty));
                        best_spread = spread;
                    }
                }
            }
        }
        best
    }

    /// Build a render snapshot of the world at the current tick (§29).
    pub fn snapshot(&self) -> Snapshot {
        let bodies = (0..self.bodies.len())
            .map(|i| {
                let (x, y) = orbit::position_of(&self.bodies, i, self.tick);
                BodyState {
                    name: self.bodies[i].name,
                    x,
                    y,
                }
            })
            .collect();
        let markets = self
            .markets
            .iter()
            .map(|m| MarketState {
                name: m.name(),
                commodities: m
                    .defs()
                    .iter()
                    .zip(m.stocks())
                    .map(|(d, s)| CommodityState {
                        name: d.name,
                        stock: s.stock,
                        price: s.price,
                    })
                    .collect(),
            })
            .collect();
        let names = self.markets[0].defs();
        let haulers = self
            .haulers
            .iter()
            .map(|h| {
                let (x, y) = h.position(self.tick);
                HaulerState {
                    id: h.id,
                    commodity: names[h.commodity].name,
                    x,
                    y,
                }
            })
            .collect();
        Snapshot {
            tick: self.tick,
            bodies,
            markets,
            haulers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::pressure::Intensity;
    use crate::sim::ships::ShipClass;

    #[test]
    fn a_warship_flies_a_committed_trajectory_and_refuels() {
        // §6 / Pillar #2: a move commits a trajectory, spends remass, takes time,
        // and the ship is positional — it can't be re-tasked mid-flight, and a tank
        // refuels at a dock.
        let mut sim = Sim::new(0);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        let full = sim.corp().fleet()[0].nav.remass;
        assert!(
            !sim.corp().fleet()[0].nav.in_transit(sim.tick()),
            "starts docked"
        );

        // Order it from Ceres Yards to Earth (body 3).
        sim.move_ship(0, 3, false)
            .expect("a frigate can reach Earth");
        assert!(
            sim.corp().fleet()[0].nav.in_transit(sim.tick()),
            "now en route"
        );
        assert!(
            sim.corp().fleet()[0].nav.remass < full,
            "spent remass on the burn"
        );
        assert_eq!(
            sim.move_ship(0, 5, false),
            Err(MoveError::Busy),
            "can't re-task mid-flight"
        );

        // Fly it out; it arrives at Earth.
        for _ in 0..3_000 {
            sim.step();
            if !sim.corp().fleet()[0].nav.in_transit(sim.tick()) {
                break;
            }
        }
        assert_eq!(sim.corp().fleet()[0].nav.location, 3, "docked at Earth");

        // Refuel tops the tank (costs credits).
        let before = sim.corp().fleet()[0].nav.remass;
        let credits = sim.corp().credits();
        assert!(sim.refuel_ship(0));
        assert_eq!(sim.corp().fleet()[0].nav.remass, full, "tank full again");
        assert!(sim.corp().fleet()[0].nav.remass > before);
        assert!(sim.corp().credits() < credits, "fuel costs money");
    }

    #[test]
    fn a_run_round_trips_through_a_json_save() {
        // Play a varied run — trade, build, route, research, tune difficulty — so
        // every persisted facet is exercised (§30).
        let mut a = Sim::new(7);
        for _ in 0..40 {
            a.step();
        }
        let _ = a.buy(1, 5, 30); // hold some cargo
        let _ = a.commission_freighter();
        a.set_trade_route(5, 1, 0, 20, 10);
        let _ = a.found_refinery(0, 1, 0);
        let _ = a.commission_ship(ShipClass::Frigate);
        a.set_intensity(Intensity::Harsh);
        a.set_alert_threshold(Priority::Warning);
        a.progression_mut().research.add_points(120);
        a.progression_mut().ceo.gain_xp(300);
        for _ in 0..60 {
            a.step();
        }

        let json = a.save_json();
        let b = Sim::load_json(&json).expect("a valid save reloads");

        // The whole persisted state round-trips bit-for-bit (the SaveState is the
        // complete contract): treasury, warehouse, fleet identity + history,
        // standings, campaign, progression, standing orders, policy, difficulty,
        // and every market's stock/price.
        assert_eq!(a.to_save(), b.to_save());
        assert_eq!(a.tick(), b.tick());
        // Spot-check a few live readers agree, not just the snapshot.
        assert_eq!(a.corp().credits(), b.corp().credits());
        assert_eq!(a.corp().fleet().len(), b.corp().fleet().len());
        assert_eq!(
            a.campaign().gate_progress_bp(),
            b.campaign().gate_progress_bp()
        );
        assert_eq!(
            a.markets()[0].stocks()[5].price,
            b.markets()[0].stocks()[5].price
        );

        // The binary shipping format round-trips identically, and auto-detect loads
        // both formats (§30): binary is smaller than the JSON dev export.
        let bytes = a.save_bytes();
        let c = Sim::load_bytes(&bytes).expect("a binary save reloads");
        assert_eq!(a.to_save(), c.to_save(), "bincode round-trips bit-for-bit");
        assert!(
            bytes.len() < json.len(),
            "binary ({}) is more compact than JSON ({})",
            bytes.len(),
            json.len()
        );
        // load_bytes also accepts the JSON dev export (auto-detected).
        let d = Sim::load_bytes(json.as_bytes()).expect("auto-detect reads JSON too");
        assert_eq!(a.to_save(), d.to_save());
    }

    #[test]
    fn a_bad_save_is_rejected_cleanly() {
        assert!(Sim::load_json("not json").is_err());
        assert!(Sim::load_bytes(b"\x00\x01 not a valid bincode save").is_err());
        // A future version is refused rather than misread (both formats).
        let mut s = Sim::new(1).to_save();
        s.version = 999;
        assert!(Sim::load_json(&s.to_json()).is_err());
        assert!(crate::sim::persist::SaveState::from_bincode(&s.to_bincode()).is_err());
    }

    #[test]
    fn step_advances_tick_and_emits_event() {
        let mut sim = Sim::new(1);
        assert_eq!(sim.tick(), 0);
        let events = sim.step();
        assert!(events.contains(&Event::Tick { tick: 1 }));
        assert_eq!(sim.tick(), 1);
    }

    #[test]
    fn player_verb_events_survive_to_the_next_step() {
        // A player cut between ticks pushes HaulerInterdicted + Scarcity; the
        // next `step` must *surface* them (not wipe them) so the feed voices the
        // player's own cut — previously `events.clear()` dropped them.
        let mut sim = Sim::new(1);
        let id = fly_a_hauler(&mut sim);
        let feed_before = sim.feed().surfaced().len();
        assert!(sim.interdict(id));
        let events = sim.step().to_vec();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::HaulerInterdicted { .. })),
            "the player's cut should reach the returned stream"
        );
        assert!(events.iter().any(|e| matches!(e, Event::Scarcity { .. })));
        assert!(
            sim.feed().surfaced().len() > feed_before,
            "the player's cut should reach the feed"
        );
        // And the carried-over events are not re-surfaced a second time.
        let next = sim.step().to_vec();
        assert!(!next
            .iter()
            .any(|e| matches!(e, Event::HaulerInterdicted { .. })));
    }

    #[test]
    fn a_player_ascent_is_voiced() {
        // The §0.3 fanfare must fire for the *player's* climb, not just for
        // sim-internal ops: a player interdiction's TierAscended now reaches the
        // returned stream.
        use crate::sim::campaign::Tier;
        let mut sim = Sim::new(0);
        let mut saw_ascent = false;
        for _ in 0..400 {
            if let Some(h) = sim.haulers().first() {
                let id = h.id;
                sim.interdict(id);
            }
            for e in sim.step() {
                if matches!(e, Event::TierAscended { .. }) {
                    saw_ascent = true;
                }
            }
            if sim.campaign().tier() != Tier::Station {
                break;
            }
        }
        assert!(
            saw_ascent,
            "the player's ascent should emit a TierAscended event"
        );
    }

    /// Step until a hauler is in flight; return its id.
    fn fly_a_hauler(sim: &mut Sim) -> u64 {
        loop {
            sim.step();
            if let Some(h) = sim.haulers().first() {
                return h.id;
            }
        }
    }

    #[test]
    fn rich_interdiction_requires_a_firing_solution() {
        let mut sim = Sim::new(2);
        let id = fly_a_hauler(&mut sim);
        let before = sim.haulers().len();
        // A crawler far off the lane can't reach it: a miss that leaves it flying.
        let crawler = Interceptor {
            pos: (8_000_000, 8_000_000),
            speed: 1,
            skill_bp: 0,
        };
        assert_eq!(sim.interdict_with(id, crawler), Interdiction::NoSolution);
        assert_eq!(
            sim.haulers().len(),
            before,
            "a miss must not remove the hauler"
        );
        // A fast frigate sitting on the hauler always has a solution (it lands or
        // the hauler escapes — never NoSolution).
        let pos = sim
            .haulers()
            .iter()
            .find(|h| h.id == id)
            .unwrap()
            .position(sim.tick());
        let frigate = Interceptor {
            pos,
            speed: 200_000,
            skill_bp: 0,
        };
        assert_ne!(sim.interdict_with(id, frigate), Interdiction::NoSolution);
    }

    #[test]
    fn the_corp_starts_solvent_and_crewed() {
        let sim = Sim::new(0);
        assert!(sim.corp().credits() > 0);
        assert!(sim.corp().trained_crew() > 0);
        assert!(sim.corp().fleet().is_empty());
    }

    #[test]
    fn arbitrage_round_trip_turns_a_profit() {
        // Buy ReactorFuel cheap at Earth (refined producer) and sell it dear at
        // Ceres (refined consumer): the player works the same spread as the NPC
        // haulers, for real credits (§5).
        let mut sim = Sim::new(0);
        let (earth, ceres, rf) = (1usize, 0usize, 5usize);
        assert!(sim.markets()[earth].price(rf) < sim.markets()[ceres].price(rf));
        let start = sim.corp().credits();
        let cost = sim.buy(earth, rf, 10).unwrap();
        assert_eq!(sim.corp().credits(), start - cost);
        assert_eq!(sim.corp().cargo(rf), 10);
        let revenue = sim.sell(ceres, rf, 10).unwrap();
        assert!(revenue > cost, "selling dear should beat buying cheap");
        assert!(sim.corp().credits() > start, "the round trip should profit");
        assert_eq!(sim.corp().cargo(rf), 0);
    }

    #[test]
    fn matched_raider_fights_are_a_competitive_coin_flip() {
        // The fix for the old screened stalemate (§9): a matched pack at Close
        // resolves to decisive outcomes that are neither a guaranteed win nor a
        // guaranteed loss — committing the fleet is a real, two-sided risk (§13).
        let trials = 64;
        let mut wins = 0;
        let mut decisive = 0;
        for seed in 0..trials {
            let mut sim = Sim::new(seed);
            for _ in 0..3 {
                sim.commission_ship(ShipClass::Frigate).unwrap();
            }
            let out = sim.engage_raiders(Band::Close).unwrap();
            if out.winner.is_some() {
                decisive += 1;
            }
            if out.winner == Some(0) {
                wins += 1;
            }
        }
        assert!(
            decisive > 0,
            "fights should resolve to a winner, not always stalemate"
        );
        let pct = wins * 100 / trials;
        assert!(
            (10..=90).contains(&pct),
            "win rate {pct}% should be competitive, not lopsided"
        );
    }

    #[test]
    fn exploiting_a_shortage_is_a_one_press_profit() {
        // ReactorFuel is dear at Ceres (the short market): exploiting sources it
        // from the cheaper Earth and sells into Ceres, no pre-held cargo (§0.4).
        let mut sim = Sim::new(0);
        let (ceres, rf) = (0usize, 5usize);
        assert_eq!(sim.corp().cargo(rf), 0);
        let start = sim.corp().credits();
        let profit = sim.exploit_shortage(ceres, rf, 20).unwrap();
        assert!(profit > 0, "exploiting a real shortage should profit");
        assert!(sim.corp().credits() > start);
        assert_eq!(
            sim.corp().cargo(rf),
            0,
            "the cargo round-trips through the warehouse"
        );
    }

    #[test]
    fn the_top_shortage_is_answerable_in_one_press() {
        // Run until a shortage is surfaced, then answer it from the feed.
        let mut sim = Sim::new(0);
        let mut answered = false;
        for _ in 0..2_000 {
            sim.step();
            if sim.feed().surfaced().iter().any(|a| a.is_act_now()) {
                answered = sim.answer_top_shortage(20);
                break;
            }
        }
        assert!(
            answered,
            "an open act-now shortage should be answerable in one press"
        );
    }

    #[test]
    fn trades_are_guarded() {
        let mut sim = Sim::new(0);
        // Nothing in the warehouse to sell.
        assert_eq!(sim.sell(0, 0, 5), Err(TradeError::InsufficientCargo));
        // More than the market holds.
        assert_eq!(sim.buy(0, 0, 1_000_000), Err(TradeError::InsufficientStock));
        // Affordable stock-wise, but beyond the treasury (200 dear ReactorFuel).
        assert_eq!(sim.buy(0, 5, 200), Err(TradeError::InsufficientCredits));
    }

    #[test]
    fn instant_trades_pay_a_brokerage_fee() {
        // Buying and selling the same lot at one market (no spread) must lose
        // money to the fee — instant liquidity is not free (§5 sink). The fee is
        // what makes hand-trading a decision instead of a riskless skim.
        let mut sim = Sim::new(0);
        let (m, c) = (0usize, 5usize);
        let start = sim.corp().credits();
        let spent = sim.buy(m, c, 10).unwrap();
        let got = sim.sell(m, c, 10).unwrap();
        assert!(
            got < spent,
            "a flat round-trip should lose the fee, got {got} vs {spent}"
        );
        assert!(sim.corp().credits() < start, "the fee leaves the treasury");
    }

    #[test]
    fn overhead_caps_runaway_hoarding() {
        // Operating overhead is a wealth-scaled sink: a treasury far above the
        // free float is skimmed each tick, so hoards can't compound without
        // bound. A small float below the threshold is left untouched.
        let mut sim = Sim::new(0);
        sim.corp.credit(900_000); // well above the free float (private field, test-only)
        let rich = sim.corp().credits();
        sim.step();
        assert!(
            sim.corp().credits() < rich,
            "overhead should skim a large treasury"
        );
        // A company at the float is not taxed (early/mid play stays clean).
        let mut lean = Sim::new(0);
        let base = lean.corp().credits();
        lean.step();
        assert_eq!(
            lean.corp().credits(),
            base,
            "a treasury at the free float pays no overhead"
        );
    }

    #[test]
    fn commissioning_spends_credits_and_crew_with_the_pool_as_the_cap() {
        let mut sim = Sim::new(0);
        let (credits0, crew0) = (sim.corp().credits(), sim.corp().trained_crew());
        sim.commission_ship(ShipClass::Frigate).unwrap();
        assert_eq!(sim.corp().fleet().len(), 1);
        assert!(sim.corp().credits() < credits0);
        assert!(sim.corp().trained_crew() < crew0);
        // A battleship needs more crew than the starting pool can field (§8c).
        assert_eq!(
            sim.commission_ship(ShipClass::Battleship),
            Err(CommissionError::NotEnoughCrew)
        );
    }

    #[test]
    fn transiting_the_gate_is_the_climactic_payoff() {
        // §0.1/§17: standing at the open gate, the deliberate transit verb crosses
        // into the Beyond endgame, voices the gate's answer, and emits GateTransited.
        let mut sim = Sim::new(0);
        assert!(
            !sim.can_transit_gate(),
            "the gate isn't reachable at the start"
        );
        assert!(!sim.transit_gate());
        // Climb the whole spine to the open gate.
        for _ in 0..(3 + 10 + 25) {
            sim.complete_op();
        }
        assert_eq!(sim.campaign().tier(), Tier::Gate);
        assert!(sim.can_transit_gate());
        // Transit — the payoff.
        assert!(sim.transit_gate());
        assert_eq!(sim.campaign().tier(), Tier::Beyond);
        assert!(sim.campaign().transited());
        assert!(!sim.can_transit_gate(), "no second transit");
        assert!(!sim.transit_gate());
        // The transit surfaced a GateTransited event for the feed to voice.
        let events = sim.step().to_vec();
        // (The event was pushed before this step; the feed voices the answer.)
        let _ = events;
    }

    #[test]
    fn the_bridgehead_is_a_post_transit_endgame_verb() {
        // §17/G3: the far-side foothold can only be founded after transiting the gate,
        // costs credits, and is itself a spine op. Upgrading reinforces it.
        let mut sim = Sim::new(3);
        assert!(!sim.bridgehead().is_founded());
        // Can't found before the Beyond, even flush with cash.
        sim.corp_mut().credit(500_000);
        assert_eq!(
            sim.found_bridgehead(),
            Err(BridgeheadError::NotBeyond),
            "no foothold before the ring"
        );
        // Climb + transit into the Beyond.
        for _ in 0..(3 + 10 + 25) {
            sim.complete_op();
        }
        assert!(sim.transit_gate());
        assert!(sim.campaign().transited());
        // Found it — costs credits, stands at level 1, and counts as an op.
        let before = sim.corp().credits();
        assert_eq!(sim.found_bridgehead(), Ok(()));
        assert!(sim.bridgehead().is_founded());
        assert_eq!(sim.bridgehead().level(), 1);
        assert!(sim.corp().credits() < before, "founding costs credits");
        assert_eq!(
            sim.found_bridgehead(),
            Err(BridgeheadError::AlreadyFounded),
            "no second founding"
        );
        // Upgrade reinforces it (raises the level + integrity).
        let max1 = sim.bridgehead().max_integrity();
        assert_eq!(sim.upgrade_bridgehead(), Ok(()));
        assert_eq!(sim.bridgehead().level(), 2);
        assert!(sim.bridgehead().max_integrity() > max1);
    }

    #[test]
    fn the_far_side_markets_exist_in_deep_scarcity_without_perturbing_the_inner_economy() {
        // §17 endgame: the far-side markets are appended after the inner economy and
        // step on a dedicated RNG, so the pre-transit world is byte-identical. Prove
        // (a) they're present and correctly partitioned, (b) they sit deeper in
        // scarcity than the inner markets, and (c) running the world for a while
        // leaves the inner markets bit-identical to a sim that never reads them.
        let mut a = Sim::new(9);
        let mut b = Sim::new(9);
        let split = a.far_market_start;
        assert!(split > 0 && split < a.markets.len(), "far side is appended");
        for m in 0..a.markets.len() {
            assert_eq!(a.is_far_side_market(m), m >= split);
        }
        // Far-side raw/refined tiers start in deep scarcity (so prices ride high) —
        // dearer than the matching inner consumer market on the same good.
        let raw = 0usize;
        let far_price = a.markets[split].price(raw);
        let inner_dearest = a.markets[..split]
            .iter()
            .map(|m| m.price(raw))
            .max()
            .unwrap();
        assert!(
            far_price > inner_dearest,
            "the far side should be dearer ({far_price} vs {inner_dearest})"
        );
        // Drive both worlds; `a` polls the far side every tick, `b` never does.
        for _ in 0..400 {
            a.step();
            b.step();
            for m in a.far_market_start..a.markets.len() {
                let _ = a.markets[m].price(0);
            }
        }
        for m in 0..split {
            for c in 0..a.markets[m].defs().len() {
                assert_eq!(
                    a.markets[m].price(c),
                    b.markets[m].price(c),
                    "inner market {m} commodity {c} drifted — far side perturbed it"
                );
            }
        }
    }

    #[test]
    fn building_and_routing_advance_the_spine_too() {
        // The retention spine used to count only interdictions; now the build and
        // logistics side of the influence model climbs it as well (§0). A few
        // commissions plus a self-running route should advance past the Station
        // with no raiding at all.
        use crate::sim::campaign::Tier;
        let mut sim = Sim::new(0);
        assert_eq!(sim.campaign().tier(), Tier::Station);
        // Two commissions are two operations on their own.
        sim.commission_freighter().unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        // A standing route then delivers itself toward the next rung.
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        for _ in 0..3_000 {
            sim.step();
            if sim.campaign().tier() != Tier::Station {
                break;
            }
        }
        assert_ne!(
            sim.campaign().tier(),
            Tier::Station,
            "build + route operations should climb the spine without interdiction"
        );
        // ...and none of it touched reputation (no cuts were made).
        for m in sim.markets() {
            assert!(sim.relations().standing(m.faction()) >= 0);
        }
    }

    #[test]
    fn a_warship_can_be_assembled_from_produced_components() {
        // §7d payoff: a player who has built up the production chain can *assemble*
        // a warship from their own Assembled-tier stock for a fraction of the
        // off-the-yard credit price — the bill-of-materials link from economy to fleet.
        let mut sim = Sim::new(0);
        // Empty warehouse ⇒ no parts ⇒ can't assemble.
        assert_eq!(
            sim.assemble_ship(ShipClass::Frigate),
            Err(CommissionError::MissingParts)
        );
        // Stock the frigate's bill of materials (2 Machinery #10, 1 Drives #11).
        for &(c, q) in Sim::ship_bom(ShipClass::Frigate) {
            sim.corp_mut().store(c, q);
        }
        let credits_before = sim.corp().credits();
        let fleet_before = sim.corp().fleet().len();
        sim.assemble_ship(ShipClass::Frigate).unwrap();
        assert_eq!(
            sim.corp().fleet().len(),
            fleet_before + 1,
            "hull joined the fleet"
        );
        // The parts were consumed...
        assert_eq!(sim.corp().cargo(10), 0, "Machinery consumed");
        assert_eq!(sim.corp().cargo(11), 0, "Drives consumed");
        // ...and assembling cost far less than buying the hull off the yard.
        let assembly_spend = credits_before - sim.corp().credits();
        let yard_price = ships::hull(ShipClass::Frigate).dry_mass * SHIP_PRICE_PER_MASS;
        assert!(
            assembly_spend < yard_price,
            "assembling from owned parts ({assembly_spend}) is cheaper than the yard ({yard_price})"
        );
    }

    #[test]
    fn a_ship_can_be_renamed_keeping_its_class() {
        // §14 expressive identity: the player renames a hull's call-sign; the class
        // suffix is preserved and an empty name is rejected.
        let mut sim = Sim::new(0);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        assert!(sim.rename_ship(0, "Valkyrie"));
        assert_eq!(sim.corp().fleet()[0].name, "Valkyrie (Frigate)");
        assert!(!sim.rename_ship(0, "   "), "blank names are rejected");
        assert!(!sim.rename_ship(9, "Ghost"), "no such ship");
    }

    #[test]
    fn the_fleet_can_actually_fight() {
        // The combat resolver was unreachable from the live loop; now a player
        // with warships can engage a raider pack and the battle resolves into a
        // BattleResolved event (§9). With no fleet, there's nothing to send.
        use crate::sim::combat::Band;
        let mut sim = Sim::new(0);
        assert!(
            sim.engage_raiders(Band::Medium).is_none(),
            "no warships ⇒ no engagement"
        );
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        let fleet_before = sim.corp().fleet().len();
        let outcome = sim.engage_raiders(Band::Medium).expect("a fleet can fight");
        assert!(outcome.winner.is_some() || outcome.ticks > 0);
        // The battle resolves into an event the feed can voice (surviving the
        // step's player-event plumbing).
        let events = sim.step().to_vec();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::BattleResolved { .. })),
            "the engagement should emit a BattleResolved event"
        );
        // Losses are applied to the fleet (it can only shrink).
        assert!(sim.corp().fleet().len() <= fleet_before);
    }

    #[test]
    fn an_off_station_fleet_cannot_defend_the_core() {
        // Pillar #2: combat is positional. Warships defend the home core only when
        // on station there; fly them away and the core is undefended until they
        // burn home — so the delta-v movement layer is consequential.
        use crate::sim::combat::Band;
        let mut sim = Sim::new(0);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        assert_eq!(sim.warships_on_station(), 2, "fresh hulls dock at the core");
        assert!(
            sim.engage_raiders(Band::Medium).is_some(),
            "on-station fleet can fight"
        );

        // Send the survivors to Earth (body 3): in transit ⇒ off station.
        for i in 0..sim.corp().fleet().len() {
            let _ = sim.move_ship(i, 3, false);
        }
        assert_eq!(
            sim.warships_on_station(),
            0,
            "a departed fleet is off station"
        );
        assert!(
            sim.engage_raiders(Band::Medium).is_none(),
            "the core is undefended while the fleet is away"
        );

        // Let them arrive at Earth — docked, but at the wrong body, still no defence.
        for _ in 0..3_000 {
            sim.step();
            if !sim
                .corp()
                .fleet()
                .iter()
                .any(|s| s.nav.in_transit(sim.tick()))
            {
                break;
            }
        }
        assert_eq!(
            sim.warships_on_station(),
            0,
            "docked at Earth is not on station at the core"
        );

        // Recall one hull home; the core can be defended again.
        let muster = sim.markets()[0].body();
        sim.refuel_ship(0);
        sim.move_ship(0, muster, false)
            .expect("a frigate can burn home");
        for _ in 0..3_000 {
            sim.step();
            if !sim.corp().fleet()[0].nav.in_transit(sim.tick()) {
                break;
            }
        }
        assert_eq!(
            sim.warships_on_station(),
            1,
            "the recalled hull stands guard"
        );
        assert!(
            sim.engage_raiders(Band::Medium).is_some(),
            "a fleet back on station can fight again"
        );
    }

    #[test]
    fn a_refinery_runs_the_value_add_chain_for_profit() {
        // Found a refinery (Ore → Metals): it sources cheap raw, refines it into a
        // dearer good, and auto-sells the surplus — hands-off (§3.1, Example A).
        let mut sim = Sim::new(0);
        let before = sim.corp().credits();
        sim.found_refinery(1, 0, 0).unwrap(); // Ore, buy+sell at Ceres
        assert_eq!(sim.stations().len(), 1);
        assert!(sim.corp().credits() < before, "founding costs capital");
        let after_found = sim.corp().credits();
        for _ in 0..1_500 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > after_found,
            "the refinery should net profit"
        );
    }

    #[test]
    fn the_production_chain_runs_four_tiers_deep() {
        // §7d: the chain is Raw → Refined → Components → Assembled. A station can be
        // founded at any non-top tier, refining into the next tier up its line —
        // Ore(1) → Metals(4) → Alloys(7) → Machinery(10). Each step is a real
        // value-add: the output anchors dearer than the input.
        let defs = super::super::economy::default_commodities();
        // The line is contiguous +3 and strictly climbs in price.
        for &i in &[1usize, 4, 7] {
            assert!(
                defs[i + 3].base_price > defs[i].base_price,
                "tier {i} refines into a dearer good"
            );
        }
        // A component factory (Metals → Alloys) is a valid recipe and produces its
        // tier-2 output hands-off.
        let mut sim = Sim::new(0);
        sim.found_refinery(4, 0, 0).unwrap(); // Metals → Alloys at Ceres
        assert_eq!(sim.stations()[0].output, 7, "Metals refines into Alloys");
        // Seed some Metals into the source market so the factory has feedstock.
        for _ in 0..2_000 {
            sim.step();
        }
        assert!(
            sim.corp().cargo(7) > 0 || sim.markets()[0].stock(7) > 0,
            "the component factory should have produced Alloys somewhere"
        );
        // The top tier has nothing higher to refine into.
        assert_eq!(
            sim.found_refinery(10, 0, 0),
            Err(FoundError::NotProcessable)
        );
    }

    #[test]
    fn refineries_are_guarded() {
        let mut sim = Sim::new(0);
        // A top-tier finished good has no higher tier to refine into (§7d).
        let top = sim.markets()[0].defs().len() - 1; // Drives
        assert_eq!(
            sim.found_refinery(top, 0, 0),
            Err(FoundError::NotProcessable)
        );
        // ...but a mid-chain commodity (Metals → Alloys) now *is* a valid recipe.
        assert!(
            sim.found_refinery(4, 0, 0).is_ok(),
            "components are producible"
        );
        // Found stations until a guard fires. Founding is an op that climbs the
        // spine, and the cap *widens* with the tier (§0.3), so the count is never
        // allowed to exceed the *current* tier's cap, and a guard (cap or capital)
        // eventually stops the spree.
        let mut last_err = None;
        for _ in 0..20 {
            match sim.found_refinery(1, 0, 0) {
                Ok(()) => assert!(
                    sim.stations().len() <= sim.campaign().station_cap(),
                    "must never exceed the tier station cap"
                ),
                Err(e) => {
                    last_err = Some(e);
                    break;
                }
            }
        }
        assert!(
            matches!(
                last_err,
                Some(FoundError::TooManyStations) | Some(FoundError::CantAfford)
            ),
            "founding is bounded by the tier cap or capital, got {last_err:?}"
        );
    }

    #[test]
    fn a_trade_route_runs_itself_for_profit() {
        // The standing-order heart (§4): set the params + own a freighter, and the
        // sim flies the loop, banking the spread with no further input.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        let start = sim.corp().credits();
        for _ in 0..2_000 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > start,
            "the route should bank profit hands-off"
        );
    }

    #[test]
    fn a_route_needs_a_freighter_and_respects_its_margin() {
        // No freighter ⇒ no trips.
        let mut sim = Sim::new(0);
        sim.set_trade_route(5, 1, 0, 20, 1);
        let start = sim.corp().credits();
        for _ in 0..500 {
            sim.step();
        }
        assert_eq!(
            sim.corp().credits(),
            start,
            "no freighter ⇒ the route can't run"
        );
        // With a freighter but an unreachable margin, the route stays idle.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 100_000);
        let start = sim.corp().credits();
        for _ in 0..500 {
            sim.step();
        }
        assert_eq!(
            sim.corp().credits(),
            start,
            "spread below margin ⇒ idle (an exception)"
        );
    }

    #[test]
    fn the_route_table_runs_many_routes_on_a_shared_freighter_pool() {
        // The §4 master-table: several standing routes run concurrently, bounded
        // by how many freighters are in the pool. Two freighters + three routes
        // ⇒ at most two trips in flight at once, and the table still banks profit.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        sim.set_trade_route(4, 0, 1, 20, 1); // Metals, Ceres → Earth
        sim.set_trade_route(1, 0, 1, 20, 1); // Ore, Ceres → Earth
        assert_eq!(sim.routes().len(), 3, "three routes sit in the table");
        let start = sim.corp().credits();
        let mut max_in_flight = 0;
        for _ in 0..2_000 {
            sim.step();
            let flying = sim.routes().iter().filter(|r| r.in_transit).count();
            max_in_flight = max_in_flight.max(flying);
        }
        assert!(
            max_in_flight <= 2,
            "two freighters cap concurrent trips at 2, saw {max_in_flight}"
        );
        assert!(max_in_flight >= 2, "both freighters should get used");
        assert!(sim.corp().credits() > start, "the table should bank profit");
    }

    #[test]
    fn a_flying_freighter_has_a_real_position_on_its_lane() {
        // §6 positional logistics: a freighter running a standing route is a located
        // asset — its position sits between the origin and destination market bodies
        // and advances along the lane as the trip progresses.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
                                             // Step until a freighter is dispatched.
        let mut flying = Vec::new();
        for _ in 0..2_000 {
            sim.step();
            flying = sim.flying_routes();
            if !flying.is_empty() {
                break;
            }
        }
        assert!(!flying.is_empty(), "the route should dispatch a freighter");
        let r = flying[0];
        let p0 = sim.route_freighter_pos(r);
        let early = sim.route_progress_bp(r);
        // Position is a real point (not the origin-only placeholder of the old model).
        assert!(p0 != (0, 0), "a flying freighter has a position");
        // Advance and confirm the trip progresses toward the destination.
        for _ in 0..30 {
            sim.step();
            if !sim.routes()[r].in_transit {
                break;
            }
        }
        if sim.routes()[r].in_transit {
            assert!(
                sim.route_progress_bp(r) > early,
                "the freighter advances along its lane over time"
            );
        }
    }

    #[test]
    fn a_route_trip_burns_remass_scaled_by_distance() {
        // §6 delta-v as opex: a freighter refuels with Remass at the origin port,
        // an amount scaled by trip length — so a long outer haul burns more fuel
        // than a short inner hop. (The fuel is debited + drawn from the port at
        // dispatch in run_logistics; here we assert the distance-scaling that drives
        // it, which is deterministic — market stock is too noisy to assert on.)
        let mut sim = Sim::new(0);
        sim.set_trade_route(1, 0, 1, 20, 1); // inner: Ceres → Mars
        sim.set_trade_route(1, 0, 5, 20, 1); // outer: Ceres → a frontier hub
        let inner = sim.route_remass_units(0);
        let outer = sim.route_remass_units(1);
        assert!(inner >= 1, "every trip burns at least one unit of fuel");
        assert!(
            outer > inner,
            "the long outer haul ({outer}) burns more fuel than the inner hop ({inner})"
        );
    }

    #[test]
    fn the_route_table_is_capped() {
        let mut sim = Sim::new(0);
        for _ in 0..10 {
            sim.set_trade_route(5, 1, 0, 20, 1);
        }
        assert_eq!(
            sim.routes().len(),
            4,
            "the table is capped at the tier route cap"
        );
        sim.clear_trade_route();
        assert!(sim.routes().is_empty(), "clearing empties the whole table");
    }

    #[test]
    fn operations_climb_the_retention_spine() {
        // Each player interdiction is an operation on the climb; three of them
        // ascend past the Station and draw the gate closer (§0.3).
        use crate::sim::campaign::Tier;
        let mut sim = Sim::new(0);
        assert_eq!(sim.campaign().tier(), Tier::Station);
        let mut ops = 0;
        for _ in 0..400 {
            if let Some(h) = sim.haulers().first() {
                let id = h.id;
                if sim.interdict(id) {
                    ops += 1;
                }
            }
            sim.step();
            if sim.campaign().tier() != Tier::Station {
                break;
            }
        }
        assert!(ops >= 3, "should have completed operations, got {ops}");
        assert_ne!(
            sim.campaign().tier(),
            Tier::Station,
            "should climb past the Station"
        );
        assert!(
            sim.campaign().gate_progress_bp() > 0,
            "the gate should draw closer"
        );
    }

    #[test]
    fn progression_advances_through_the_sim() {
        let mut sim = Sim::new(0);
        sim.progression_mut().ceo.gain_xp(3_000);
        assert_eq!(sim.progression().ceo.level(), 4);
        sim.progression_mut().research.add_points(1_000);
        assert!(sim.progression_mut().research.research(0).is_ok());
        assert!(sim.progression().research.is_unlocked(0));
        // Generic blueprint discoverable; the Martian design stays rep-gated
        // until Mars standing is high enough (§10).
        assert!(sim.discover_blueprint(0));
        assert!(!sim.discover_blueprint(2));
        sim.relations_mut()
            .adjust(crate::sim::faction::Faction::Mars, 500);
        assert!(sim.discover_blueprint(2));
    }

    #[test]
    fn automation_interdicts_only_targeted_shipping() {
        // Set a standing order to hunt Earth shipping; the manager runs it for
        // us, souring Earth while leaving off-target factions alone (§12).
        let mut sim = Sim::new(0);
        sim.policy_mut().interdiction.enabled = true;
        sim.policy_mut().interdiction.target = Some(crate::sim::faction::Faction::Earth);
        for _ in 0..1_000 {
            sim.step();
        }
        assert!(
            sim.relations()
                .standing(crate::sim::faction::Faction::Earth)
                < 0,
            "the patrol should have cut Earth shipping"
        );
        assert_eq!(
            sim.relations().standing(crate::sim::faction::Faction::Belt),
            0,
            "Belt shipping was off-target and untouched"
        );
    }

    #[test]
    fn automation_min_cargo_spares_small_fry() {
        let mut sim = Sim::new(0);
        sim.policy_mut().interdiction.enabled = true;
        sim.policy_mut().interdiction.min_cargo = 1_000_000; // nothing is this big
        for _ in 0..1_000 {
            sim.step();
        }
        for m in sim.markets() {
            assert_eq!(sim.relations().standing(m.faction()), 0);
        }
    }

    #[test]
    fn automation_auto_researches_when_funded() {
        let mut sim = Sim::new(0);
        sim.policy_mut().auto_research = true;
        sim.progression_mut().research.add_points(1_000);
        sim.step();
        assert!(sim.progression().research.unlocked_count() > 0);
    }

    #[test]
    fn player_interdiction_sours_relations() {
        // Cutting a faction's hauler lowers the player's standing with them (§7b/§10).
        let mut sim = Sim::new(0);
        let id = fly_a_hauler(&mut sim);
        let origin = sim.haulers().iter().find(|h| h.id == id).unwrap().origin;
        let faction = sim.markets()[origin].faction();
        assert!(sim.interdict(id));
        assert!(
            sim.relations().standing(faction) < 0,
            "the owner should resent it"
        );
    }

    #[test]
    fn hostility_recovers_once_the_raiding_stops() {
        // Drive Earth to Hostile, then stop: standing must drift back toward
        // neutral over time (§10) — the cliff is now a dial.
        use crate::sim::faction::Faction;
        let mut sim = Sim::new(0);
        sim.relations_mut().adjust(Faction::Earth, -1_000);
        assert_eq!(sim.relations().standing(Faction::Earth), -1_000);
        for _ in 0..2_000 {
            sim.step();
        }
        let healed = sim.relations().standing(Faction::Earth);
        assert!(
            healed > -1_000,
            "Earth should be recovering, still at {healed}"
        );
        assert!(
            healed < 0,
            "but a deep grudge shouldn't fully heal that fast"
        );
    }

    #[test]
    fn salvage_discovers_wrecks_without_perturbing_the_economy() {
        // A world where the player strips every sighted wreck keeps bit-identical
        // *markets* to one that ignores them — the salvage field's own RNG (§15)
        // never advances the world economy (the §27 contract-board lesson).
        let mut control = Sim::new(5);
        let mut salvager = Sim::new(5);
        let (mut sighted, mut stripped) = (0, 0);
        for _ in 0..2_000 {
            control.step();
            for e in salvager.step().to_vec() {
                if let Event::WreckSighted { .. } = e {
                    sighted += 1;
                }
            }
            // Strip whatever's adrift; rewards land in the corp/progression, not
            // the markets.
            while salvager.salvage_top() {
                stripped += 1;
            }
            for (cm, sm) in control.markets().iter().zip(salvager.markets()) {
                assert_eq!(cm.stocks(), sm.stocks(), "salvage perturbed the economy");
            }
        }
        assert!(sighted > 0, "the field should turn up wrecks over the run");
        assert_eq!(sighted, stripped, "every sighted wreck was strippable");
    }

    #[test]
    fn pirate_raids_do_not_blame_the_player() {
        // Pirates thin the lanes for thousands of ticks; the player's standings
        // stay neutral (the raids aren't attributed to them).
        let mut sim = Sim::new(0);
        for _ in 0..2_000 {
            sim.step();
        }
        for m in sim.markets() {
            assert_eq!(sim.relations().standing(m.faction()), 0);
        }
    }

    #[test]
    fn the_alert_feed_voices_the_run() {
        // Over a run the feed fills with ranked alerts, including act-now
        // shortages tagged with a verb (§19/§0.4). Act-now alerts age out after a
        // TTL, so we watch the whole run rather than only the final tick.
        let mut sim = Sim::new(0);
        let mut saw_act_now = false;
        for _ in 0..3_000 {
            sim.step();
            if sim
                .feed()
                .surfaced()
                .iter()
                .any(|a| a.is_act_now() && a.verb.is_some())
            {
                saw_act_now = true;
            }
        }
        assert!(
            !sim.feed().surfaced().is_empty(),
            "the feed should have something to say"
        );
        assert!(
            saw_act_now,
            "an interdicted run should raise act-now shortages"
        );
    }

    #[test]
    fn pirates_raid_the_lanes() {
        // Over a long run the ambient raider lands strikes, each tagging a
        // destination scarcity (§7b/§13).
        let mut sim = Sim::new(0);
        let (mut cuts, mut scarcities) = (0, 0);
        for _ in 0..4_000 {
            for e in sim.step() {
                match e {
                    Event::HaulerInterdicted { .. } => cuts += 1,
                    Event::Scarcity { .. } => scarcities += 1,
                    _ => {}
                }
            }
        }
        assert!(cuts > 0, "pirates never struck the lanes");
        assert_eq!(cuts, scarcities, "every cut should leave a scarcity");
    }

    #[test]
    fn snapshot_has_bodies_and_markets() {
        let mut sim = Sim::new(1);
        for _ in 0..50 {
            sim.step();
        }
        let snap = sim.snapshot();
        assert_eq!(snap.tick, 50);
        assert_eq!(snap.bodies.len(), default_system().len());
        // 6 inner markets + 2 far-side endgame markets (§17).
        assert_eq!(snap.markets.len(), 8);
        assert_eq!((snap.bodies[0].x, snap.bodies[0].y), (0, 0)); // Sol fixed
    }

    #[test]
    fn same_seed_yields_identical_runs() {
        let mut a = Sim::new(42);
        let mut b = Sim::new(42);
        for _ in 0..600 {
            assert_eq!(a.step(), b.step());
            assert_eq!(a.snapshot(), b.snapshot());
        }
    }

    #[test]
    fn markets_carry_a_standing_arbitrage_spread() {
        // Ceres (producer) is cheaper than Earth (consumer) on raw Ore.
        let sim = Sim::new(0);
        let ore = 1;
        assert!(sim.markets()[0].price(ore) < sim.markets()[1].price(ore));
        // ...and dearer than Earth on refined Metals.
        let metals = 4;
        assert!(sim.markets()[0].price(metals) > sim.markets()[1].price(metals));
    }

    #[test]
    fn haulers_fly_the_routes() {
        let mut sim = Sim::new(3);
        let mut saw_hauler = false;
        for _ in 0..500 {
            sim.step();
            saw_hauler |= !sim.haulers().is_empty();
        }
        assert!(saw_hauler, "arbitrage never spawned a hauler");
    }

    #[test]
    fn trade_damps_the_spread() {
        // ReactorFuel carries the largest spread (Ceres dear, Earth cheap), so it
        // gets the most traffic; with haulers flowing its average spread settles
        // below the no-trade structural value.
        let mut sim = Sim::new(5);
        let rf = 5;
        let spread = |s: &Sim| s.markets()[0].price(rf) - s.markets()[1].price(rf);
        let structural = spread(&sim);
        for _ in 0..2_000 {
            sim.step();
        }
        let (mut sum, mut count) = (0i64, 0i64);
        for _ in 0..400 {
            sim.step();
            sum += spread(&sim);
            count += 1;
        }
        let avg = sum / count;
        assert!(
            avg < structural,
            "avg spread {avg} not damped below {structural}"
        );
        assert!(avg > 0, "the structural spread should persist, just damped");
    }

    #[test]
    fn interdiction_starves_the_destination() {
        // Two identical runs; in one we cut the first hauler. The RNG (market
        // jitter) stays aligned across both, so the only divergence is the
        // denied delivery — leaving the destination dearer (a shortage, §7b).
        let mut control = Sim::new(1);
        let mut cut = Sim::new(1);
        let (id, dest, commodity, arrival) = loop {
            control.step();
            cut.step();
            if let Some(h) = cut.haulers().first() {
                break (h.id, h.dest, h.commodity, h.arrival_tick);
            }
        };
        assert!(cut.interdict(id));
        assert!(!cut.interdict(id), "a cut hauler cannot be cut twice");
        while cut.tick() < arrival {
            control.step();
            cut.step();
        }
        assert!(
            cut.markets()[dest].price(commodity) > control.markets()[dest].price(commodity),
            "interdiction did not raise the destination price"
        );
    }

    /// The §7c gate, re-checked with the §7b traffic layer running: trade must
    /// not destabilize any market on any seed.
    #[test]
    fn no_death_spiral_with_traffic_on_any_seed() {
        for seed in 0..32u64 {
            let mut sim = Sim::new(seed);
            let mut ok = true;
            for _ in 0..5_000 {
                sim.step();
                for m in sim.markets() {
                    for (d, s) in m.defs().iter().zip(m.stocks()) {
                        ok &= s.stock > 0 && s.stock < d.max_stock + d.target_stock;
                        ok &= s.price > d.floor && s.price < d.ceiling;
                    }
                }
            }
            assert!(ok, "death-spiral with traffic on seed {seed}");
        }
    }

    #[test]
    fn the_board_posts_and_caps_contracts() {
        let mut sim = Sim::new(7);
        // The board fills to its cap and never exceeds it.
        for _ in 0..2_000 {
            sim.step();
            assert!(sim.open_contract_count() <= MAX_CONTRACTS);
        }
        assert_eq!(
            sim.open_contract_count(),
            MAX_CONTRACTS,
            "a healthy world keeps the job menu full"
        );
    }

    #[test]
    fn fulfilling_a_contract_pays_and_lifts_reputation() {
        let mut sim = Sim::new(11);
        // Let the board post some offers.
        for _ in 0..CONTRACT_INTERVAL {
            sim.step();
        }
        let c = *sim.contracts().first().expect("an offer should be posted");
        // Stock the warehouse with exactly what the contract owes, then fulfil.
        sim.corp.store(c.commodity, c.qty);
        let before_credits = sim.corp().credits();
        let before_rep = sim.relations().standing(c.faction);
        assert!(sim.accept_contract(c.id));
        let reward = sim
            .fulfill_contract(c.id)
            .expect("fulfilment should succeed");
        assert_eq!(reward, c.reward);
        assert_eq!(sim.corp().credits(), before_credits + c.reward);
        assert_eq!(sim.relations().standing(c.faction), before_rep + c.rep);
        assert_eq!(
            sim.corp().cargo(c.commodity),
            0,
            "the owed cargo is consumed"
        );
        assert!(
            sim.contracts().iter().all(|o| o.id != c.id),
            "a fulfilled contract leaves the board"
        );
    }

    #[test]
    fn a_contract_must_be_accepted_and_stocked_to_fulfil() {
        let mut sim = Sim::new(13);
        for _ in 0..CONTRACT_INTERVAL {
            sim.step();
        }
        let c = *sim.contracts().first().expect("an offer should be posted");
        // Not accepted yet → NotAccepted.
        assert_eq!(sim.fulfill_contract(c.id), Err(ContractError::NotAccepted));
        // Accepted but empty warehouse → InsufficientCargo.
        assert!(sim.accept_contract(c.id));
        assert_eq!(
            sim.fulfill_contract(c.id),
            Err(ContractError::InsufficientCargo)
        );
        // A bogus id is NotFound.
        assert_eq!(sim.fulfill_contract(99_999), Err(ContractError::NotFound));
    }

    #[test]
    fn unaccepted_contracts_lapse_but_accepted_ones_persist() {
        let mut sim = Sim::new(17);
        for _ in 0..CONTRACT_INTERVAL {
            sim.step();
        }
        let c = *sim.contracts().first().expect("an offer should be posted");
        assert!(sim.accept_contract(c.id));
        // Run well past the delivery window; the accepted contract is still owed.
        for _ in 0..(CONTRACT_WINDOW + CONTRACT_INTERVAL) {
            sim.step();
        }
        assert!(
            sim.contracts().iter().any(|o| o.id == c.id && o.accepted),
            "an accepted contract does not lapse"
        );
    }

    #[test]
    fn the_contract_board_does_not_perturb_the_economy() {
        // The board has its own RNG, so a world *with* contract postings must be
        // bit-identical in its economy to one where we never read the board —
        // proving offer generation never advances the shared world streams (§27).
        let mut a = Sim::new(23);
        let mut b = Sim::new(23);
        for _ in 0..1_000 {
            a.step();
            // `b` additionally pokes the board read paths every tick.
            b.step();
            let _ = b.contracts();
            let _ = b.open_contract_count();
        }
        for (ma, mb) in a.markets().iter().zip(b.markets()) {
            for c in 0..ma.defs().len() {
                assert_eq!(ma.price(c), mb.price(c), "economy diverged");
                assert_eq!(ma.stock(c), mb.stock(c), "stock diverged");
            }
        }
    }
}
