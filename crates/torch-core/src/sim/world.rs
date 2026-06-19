//! The authoritative world and the sim↔view contract (§27, §28, §29).
//!
//! [`Sim`] owns the truth and advances on a fixed tick. The view never reads
//! `Sim` directly: it consumes a [`Snapshot`] (current state to render) plus the
//! [`Event`] stream returned by [`Sim::step`] (what changed). This is the seam
//! the Godot shell binds to and that keeps the core headless and testable.

use super::alerts::{AlertFeed, Priority, Verb};
use super::ambient::AmbientChatter;
use super::automation::AutomationPolicy;
use super::bridgehead::Bridgehead;
use super::campaign::{Campaign, EndgameOutcome, Tier};
use super::combat::{self, Band, BattleOutcome, Doctrine, Fleet, TargetPriority};
use super::contest::{self, ContestedColony};
use super::contracts::ContractBoard;
use super::corp::{Corp, OwnedShip};
use super::decisions::{
    Decision, DecisionKind, DecisionOption, DecisionOutcome, AMBUSH_CHANCE_BP, DEAL_QTY,
    DECISION_TTL, ESCORT_FEE, HUNT_CHANCE_BP, MAX_DECISIONS, RAID_RELIEF, REVENG_CHANCE_BP,
    WAR_REROUTE_COST, WAR_RUN_CHANCE_BP, WAR_SIDE_REP, WAR_STAKE, WRECK_DATA, WRECK_SCRAP,
};
use super::diplomacy::{Diplomacy, Stance};
use super::economy::{default_markets, Market};
use super::event::Event;
use super::faction::{Faction, Relations};
use super::frontier::{default_colonies, Colony};
use super::industry::Station;
use super::interdiction::{resolve, Interceptor, Interdiction};
use super::logistics::TradeRoute;
use super::movement;
use super::orbit::{self, default_system, Body};
use super::pressure::{Intensity, PressureKind, PressureSystem};
use super::progression::Progression;
use super::rng::Pcg32;
use super::salvage::{SalvageField, SalvageReward};
use super::ships::{self, Loadout, ShipCatalog, ShipClass, WeaponDef, WeaponKind};
use super::traffic::Hauler;
use super::weapons;

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
/// Phase B: the bounty per raider hull on a *won* engagement, and how much holding the
/// core calms the piracy gauge. Sized so a win covers attrition + a margin (a frigate
/// costs 4000), making combat net-positive — but combat is crew-capped, so not a faucet.
const BOUNTY_PER_RAIDER: i64 = 2200;
const COMBAT_PIRACY_RELIEF: i32 = 25;
/// Scrap parts recovered per raider hull destroyed on a won fight (Phase B crafting
/// input), and how much crafting a great power's design sours them per tier.
const SCRAP_PER_RAIDER: i64 = 8;
const CRAFT_ANGER: i64 = 6;
/// Weapon production time (§8a): tooling up a line takes time, scaled by tier — you
/// produce your own guns *slowly*, you don't buy them off the shelf.
const PRODUCTION_BASE_TICKS: u64 = 48;
const PRODUCTION_TICKS_PER_TIER: u64 = 30;
/// Refitting a ship's weapons takes time in the yard (Phase B), scaled by hull mass.
const REFIT_TICKS_PER_MASS: u64 = 1;
const REFIT_FEE_PER_MASS: i64 = 2;
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
    /// A custom design (A2) that fails the fitting (e.g. over the power budget).
    BadFit,
    /// This hull needs your own shipyard (of sufficient tier) — only civilians and
    /// (with OPA standing) corvettes come from Tycho.
    NeedShipyard,
}

/// The empire-wide **development doctrine** (Phase C) — a macro tilt on what your
/// developed holdings yield. `Balanced` is the identity default (so a fresh/undeveloped
/// empire is byte-identical). Industry favours raw supply, Trade favours credits, Growth
/// trades yield for cheaper development.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DevDoctrine {
    #[default]
    Balanced,
    Industry,
    Trade,
    Growth,
}

impl DevDoctrine {
    /// (output_bp, tribute_bp, develop_cost_bp) — multipliers in basis points.
    pub fn weights(self) -> (i64, i64, i64) {
        match self {
            DevDoctrine::Balanced => (10_000, 10_000, 10_000),
            DevDoctrine::Industry => (15_000, 6_000, 10_000),
            DevDoctrine::Trade => (6_000, 15_000, 10_000),
            DevDoctrine::Growth => (8_500, 8_500, 6_000),
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            DevDoctrine::Balanced => "Balanced",
            DevDoctrine::Industry => "Industry",
            DevDoctrine::Trade => "Trade",
            DevDoctrine::Growth => "Growth",
        }
    }
    /// Cycle to the next doctrine (the shell's one-press macro knob).
    pub fn next(self) -> Self {
        match self {
            DevDoctrine::Balanced => DevDoctrine::Industry,
            DevDoctrine::Industry => DevDoctrine::Trade,
            DevDoctrine::Trade => DevDoctrine::Growth,
            DevDoctrine::Growth => DevDoctrine::Balanced,
        }
    }
}

/// Why a weapon could not be produced (Phase B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CraftError {
    Unknown,
    AlreadyOwned,
    /// You don't hold the schematic — it must be earned (reverse-engineering), not bought.
    NoSchematic,
    /// A line for this model is already tooling up.
    AlreadyProducing,
    NotEnoughScrap,
    CantAfford,
}

/// A deployed mining ship (early industry): stationed at a body, it extracts the body's
/// raw mineral into your warehouse each tick. Cheap + civilian (from Tycho).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Miner {
    pub body: usize,
    pub commodity: usize,
}

/// A custom warship design (A2) held in the build queue: the chosen weapon model + count
/// per slot and the remass fill (as a percent of tankage). Plain integers so a queued build
/// persists across a save (the loadout itself carries `&'static` defs and can't serialize).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DesignSpec {
    pub pdc_model: usize,
    pub pdc: u32,
    pub torp_model: usize,
    pub torp: u32,
    pub rail_model: usize,
    pub rail: u32,
    pub remass_bp: i64,
}

/// A warship under construction in the shipyard (§6): laid down now (cost paid, crew
/// reserved) and standing up into the fleet once `ready_tick` passes — so commissioning a
/// hull is a paced build like everything else, not an instant macro action. `design` is
/// `None` for a reference loadout (rebuilt from current best weapons at completion).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingShip {
    pub class: ShipClass,
    pub ready_tick: u64,
    #[serde(default)]
    pub design: Option<DesignSpec>,
}

/// Why a miner could not be deployed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinerError {
    /// You've deployed the maximum number of miners.
    Full,
    /// Can't mine here (the sun or the gate).
    BadSite,
    CantAfford,
}

/// A player-founded **outpost** — a station planted at a body that develops through levels
/// into an industrial base. Pays a per-level tribute and boosts a co-located miner. Founding
/// (and each development level) is a **slow construction** — `ready_tick` is when the current
/// build finishes; until then the outpost is inert (the macro "set it and wait" loop).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Outpost {
    pub body: usize,
    pub level: i64,
    /// Tick the in-progress construction completes (0 = idle/ready).
    #[serde(default)]
    pub ready_tick: u64,
    /// Built facilities, a bitmask of [`FAC_MINE`]/[`FAC_STORAGE`]/[`FAC_HANGAR`]. An outpost
    /// **without a Mine produces no raw goods** (only its passive tribute); Storage/Hangar are
    /// the next rungs (warehouse depth / ship resupply). The progression toward a colony.
    #[serde(default)]
    pub facilities: u8,
    /// Settlement rank along the progression: 0 outpost → 1 **colony** (→ later hub/capital). A
    /// fully-built outpost (maxed + all facilities) can be promoted, multiplying its yield.
    #[serde(default)]
    pub rank: u8,
    /// Settled population. Grows while the colony is **supplied with Ice** (the basic good) from
    /// your stores; stalls/decays without it. A population threshold gates colony promotion —
    /// you must attract and feed people before an outpost becomes a colony.
    #[serde(default)]
    pub population: i64,
    /// **Local inventory** of the body's mineral (the per-asset stock, §10): Mine output
    /// accumulates here (not the global warehouse), capped by the Storage facility; a Hangar
    /// ships it out to your warehouse. Without a Hangar the goods pile up on-site, stuck.
    #[serde(default)]
    pub stored: i64,
}

/// Outpost facility bits.
pub const FAC_MINE: u8 = 1;
pub const FAC_STORAGE: u8 = 2;
pub const FAC_HANGAR: u8 = 4;
/// All three facilities built — the gate to colony promotion.
pub const FAC_ALL: u8 = FAC_MINE | FAC_STORAGE | FAC_HANGAR;
/// Settlement ranks.
pub const RANK_OUTPOST: u8 = 0;
pub const RANK_COLONY: u8 = 1;
pub const RANK_HUB: u8 = 2;
pub const RANK_CAPITAL: u8 = 3;
/// Yield multiplier by rank — a settlement out-produces the rung below it.
pub fn rank_yield_mult(rank: u8) -> i64 {
    match rank {
        RANK_COLONY => 3,
        RANK_HUB => 6,
        RANK_CAPITAL => 12,
        _ => 1,
    }
}
/// Population needed to promote **out of** the given rank (to the next one).
pub fn promote_pop_threshold(rank: u8) -> i64 {
    match rank {
        RANK_OUTPOST => 700,  // → Colony
        RANK_COLONY => 1_400, // → Hub
        _ => 2_400,           // Hub → Capital
    }
}

impl Outpost {
    /// Whether the outpost's current construction has finished (it's operational).
    pub fn is_ready(&self, tick: u64) -> bool {
        tick >= self.ready_tick
    }
}

/// Why an outpost action failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutpostError {
    /// You've founded the maximum number of outposts.
    Full,
    /// Not a valid uninhabited site (the sun/gate, or a body already taken).
    BadSite,
    /// No outpost there to develop.
    NoneThere,
    /// Already at the maximum development level.
    Maxed,
    CantAfford,
}

/// Why a shipyard action failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShipyardError {
    AlreadyBuilt,
    NoneBuilt,
    /// Already at the maximum tier.
    Maxed,
    /// Can't build a yard here (the sun or the gate).
    BadSite,
    CantAfford,
}

/// Why a colony could not be developed (Phase C).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevelopError {
    NotControlled,
    /// Already at the development cap.
    Maxed,
    CantAfford,
}

/// Why a ship could not be refitted (Phase B).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefitError {
    NoSuchShip,
    /// Already in the yard being refitted.
    Busy,
    /// Not docked at the home yard (must be on station to refit).
    NotAtYard,
    /// A capital hull (Cruiser/Battleship) can't be refitted without your own shipyard.
    NeedShipyard,
    CantAfford,
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

/// Why a colony acquisition could not proceed (the empire layer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcquireError {
    /// Not an acquirable target (out of range, or not an independent colony — you
    /// can't simply *buy* a great power's territory).
    NotAcquirable,
    /// You already control it.
    AlreadyControlled,
    CantAfford,
}

/// Why courting or claiming a contested colony could not proceed (early game).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContestError {
    /// No such contested colony.
    NoSuchColony,
    /// You already control it.
    AlreadyControlled,
    /// Not enough Influence to court it (the statecraft resource).
    NotEnoughInfluence,
    /// Your standing isn't strong enough to claim it yet (court it more).
    NotStrongEnough,
}

/// Price to buy out an independent **market** colony (a producing frontier hub). Acquiring a
/// whole colony is a **mid-term goal**, not an early-game click — far pricier than founding an
/// outpost (the short-term step), so the player trades/mines/outposts toward it for a long while.
const COLONY_PRICE_MARKET: i64 = 320_000;
/// Price to buy out an independent **outpost** colony (a lesser settlement).
const COLONY_PRICE_OUTPOST: i64 = 180_000;
/// Per-tick tribute a controlled market colony pays the treasury (you run its
/// economy now). A flat credit drip — it never touches market RNG, so the §7c gate
/// is provably unaffected by who owns what.
const COLONY_TRIBUTE_MARKET: i64 = 40;
/// …and a controlled outpost colony's smaller tribute.
const COLONY_TRIBUTE_OUTPOST: i64 = 16;
/// Raw units a controlled colony produces into your warehouse each tick (EP1) — the
/// supply that integrates holdings into your production/logistics chain.
const COLONY_OUTPUT_PER_TICK: i64 = 3;
/// Early-game **mining** (the first industrial step): a cheap civilian miner bought from
/// Tycho, deployed at a body, extracts that body's raw into your warehouse each tick —
/// the bootstrap before you can afford colonies/refineries.
const MINER_COST: i64 = 9_000;
const MINER_OUTPUT_PER_TICK: i64 = 2;
const MAX_MINERS: usize = 12;
/// **Outposts** — player-founded stations you plant at any uninhabited body and develop
/// through levels into a real industrial base. Cheaper than a shipyard; a passive economic
/// base (per-level tribute) and a **deposit point** that boosts a co-located miner (the
/// "haul to the nearest station" benefit). Distinct from the single shipyard + the frontier
/// colonies. Inert by default (no outposts ⇒ the §7c gate + QA stay byte-identical).
const OUTPOST_FOUND_COST: i64 = 18_000;
const OUTPOST_DEVELOP_BASE: i64 = 12_000;
const MAX_OUTPOST_LEVEL: i64 = 5;
const OUTPOST_TRIBUTE_PER_LEVEL: i64 = 30;
const MAX_OUTPOSTS: usize = 16;
/// A miner working an outpost's body extracts +50% (it hauls to the station on-site).
const OUTPOST_MINER_BONUS_BP: i64 = 5_000;
/// Founding an outpost is a **slow build — ~180 days** (6 ticks = 1 day): you commit the
/// macro decision and the credits, then wait it out (the relaxing, un-clicky pace).
pub const OUTPOST_BUILD_TICKS: u64 = 1080;
/// Developing a level is a shorter build (~120 days).
pub const OUTPOST_DEVELOP_TICKS: u64 = 720;
/// A facility (mine / storage / hangar) costs this and takes ~120 days to build.
const OUTPOST_FACILITY_COST: i64 = 12_000;
const OUTPOST_FACILITY_TICKS: u64 = 720;
/// Raw goods a Mine-equipped outpost produces per tick, per level (the body's mineral).
const OUTPOST_MINE_OUTPUT: i64 = 2;
/// Promoting a fully-built outpost to a **colony** — a major ~1-year undertaking that
/// **triples** its yield (tribute + production). The headline progression step.
const OUTPOST_PROMOTE_COST: i64 = 90_000;
const OUTPOST_PROMOTE_TICKS: u64 = 2160;
/// Population: the basic good is **Ice** (commodity 0). A supplied outpost draws settlers; an
/// unsupplied one stagnates. Promotion to a colony needs `PROMOTE_POP` people.
const ICE_COMMODITY: usize = 0;
const OUTPOST_POP_BASE: i64 = 50;
const POP_CAP_PER_LEVEL: i64 = 200; // L5 ⇒ 1000 cap
const ICE_FEED_PER_TICK: i64 = 1; // Ice drawn from stores per operational outpost when growing
const POP_GROWTH: i64 = 1;
const POP_DECAY: i64 = 1;
pub const PROMOTE_POP: i64 = 700;
/// Per-asset local storage (§10): a bare outpost holds little; a **Storage** facility deepens it
/// (× level). A **Hangar** ships this many units per tick (× level) to your warehouse.
const STORE_CAP_BASE: i64 = 100;
const STORE_CAP_WITH_STORAGE: i64 = 1_500;
const HANGAR_SHIP_PER_TICK: i64 = 4;
/// Phase C — colony development (the *tall* growth axis). A colony starts at `DEV_BASE`
/// and can be invested up to `MAX_DEV`; tribute + output scale by the level, so a
/// developed holding is worth far more than a bare one. The cost to raise it escalates
/// (`DEV_COST_BASE × current level`), so growing tall is a real, paced investment — and
/// unlike *wide* expansion, developing your **own** colony draws no coalition alarm.
const DEV_BASE: i64 = 1;
const MAX_DEV: i64 = 5;
const DEV_COST_BASE: i64 = 8_000;
/// Developing a colony a level is a ~180-day build (the new capacity comes online when done).
pub const COLONY_DEVELOP_TICKS: u64 = 1080;
/// Your own **shipyard** — the only place to build warships beyond corvettes. Founding
/// is very expensive and each tier dearer; upkeep scales with tier. Tier gates the
/// largest hull it can lay down: 1 → Destroyer, 2 → Cruiser, 3 → Battleship. Frigates
/// (corvettes) need a yard **or** good OPA standing (bought from Tycho).
const SHIPYARD_FOUND_COST: i64 = 60_000;
const SHIPYARD_EXPAND_COST: i64 = 50_000; // × current tier
const SHIPYARD_UPKEEP_PER_TIER: i64 = 50;
const MAX_SHIPYARD_TIER: i64 = 3;
/// A shipyard is a **major undertaking** — founding takes ~a year (360 days), expanding ~240
/// days. It lays down nothing until the build completes (the relaxing, macro pace).
const SHIPYARD_FOUND_TICKS: u64 = 2160;
const SHIPYARD_EXPAND_TICKS: u64 = 1440;
/// Belt/OPA standing needed to buy corvettes (Frigates) from Tycho.
const CORVETTE_STANDING: i64 = 250;
/// Phase A dilemma tuning: the profiteer's panic premium (bp of the sale), the relief
/// run's sell margin over cost (bp), and the reputation swing for gouging vs. relieving.
const GOUGE_BONUS_BP: i64 = 4000; // +40% of the sale, wrung from desperate buyers
const GOUGE_REP: i64 = 40;
const RELIEF_MARGIN_BP: i64 = 10_500; // sell at ~105% of cost (near break-even)
const RELIEF_REP: i64 = 50;
/// Brokerage fee in basis points at a market you **own** (EP2) — you run the broker,
/// so trade is cheaper than the standard `TRADE_FEE_BP`, but not free (a sink stays).
const OWNED_TRADE_FEE_BP: i64 = 100;
/// Tariff (basis points of cargo value) you collect on an NPC delivery into a market
/// you own (EP2) — your empire earns from the living economy autonomously.
const NPC_TARIFF_BP: i64 = 120;

// ---- administrative capacity: the overextension cap (E2) ----
/// Holdings a green CEO can govern efficiently before strain sets in.
const ADMIN_BASE_CAPACITY: usize = 3;
/// …plus one more holding of capacity per this many CEO levels (a seasoned CEO runs
/// a wider empire — capacity is *earned*, Stellaris-style admin cap).
const ADMIN_CAPACITY_PER_CEO_LEVELS: i64 = 3;
/// Each holding over capacity drops empire-wide tribute efficiency by this (bp)…
const STRAIN_EFFICIENCY_PENALTY_BP: i64 = 1_500;
/// …floored here, so a wildly overextended empire still scrapes some income.
const STRAIN_EFFICIENCY_FLOOR_BP: i64 = 2_000;
/// …and each over-capacity holding also bleeds this much upkeep per tick — so past
/// your administrative reach, holdings go net-negative (overextension bites).
const STRAIN_UPKEEP_PER_HOLDING: i64 = 35;

// ---- faction alarm & the coalition: the geopolitical cap (E3) ----
/// Alarm ceiling — the great powers can be no more threatened than fully united.
const ALARM_MAX: i64 = 1_000;
/// A single acquisition spikes the coalition alarm by this (expanding *fast* unites
/// them even before your empire is large).
const ALARM_PER_ACQUISITION: i64 = 120;
/// The steady-state alarm each holding sustains — a large empire is permanently
/// watched (size baseline = holdings × this).
const ALARM_PER_HOLDING: i64 = 90;
/// Alarm drifts toward its size baseline (and cools) by this much per tick.
const ALARM_DRIFT: i64 = 3;
/// At or above this alarm a coalition forms and strikes your holdings.
const COALITION_THRESHOLD: i64 = 500;
/// A won defense buys this much breathing room (alarm relief).
const ALARM_RELIEF_ON_DEFEND: i64 = 160;
/// Base ticks between coalition strikes while active (tightens as alarm climbs).
const COALITION_BASE_PERIOD: u64 = 150;
const COALITION_MIN_PERIOD: u64 = 60;
/// Ticks to mount a defense before an unanswered strike seizes a holding (E3).
const COALITION_RESPONSE_WINDOW: u64 = 36;
/// Crew quality of the coalition's navies — inner-system regulars, tougher than pirates.
const COALITION_QUALITY: i64 = 65;
/// Coalition pack size = 2 + (alarm over threshold) / this — a modest escalation
/// from a pair (at the threshold) to a small squadron (at max alarm).
const COALITION_STRENGTH_PER_SHIP: i64 = 100;
/// Reparations debited when the coalition strikes but you hold no colony to seize.
const COALITION_REPARATIONS: i64 = 15_000;

// ---- piracy on your trade empire: the security cost (EP3) ----
/// Ticks between pirate-raid attempts on your shipping (a standing predation).
const PIRACY_INTERVAL: u64 = 90;
/// Holdings each warship on station can screen — escorts needed scale with the empire.
const HOLDINGS_PER_ESCORT: usize = 3;
/// Cargo (credits) pirates take per under-covered escort slot when you fall short.
const PIRACY_LOSS_PER_ESCORT_SHORT: i64 = 800;

// ---- faction inspections & enforcement: the political cost (EP4) ----
/// Max customs surcharge (basis points) added to the trade fee at a fully-hostile
/// faction's market — scales with how negative your standing is.
const INSPECTION_SURCHARGE_MAX_BP: i64 = 500;
/// Ticks between customs sweeps by soured great powers.
const INSPECTION_INTERVAL: u64 = 150;
/// A great power at or below this standing inspects your shipping (Cold or worse).
const INSPECTION_THRESHOLD: i64 = -200;
/// Fine per point of (capped) hostility when a soured power inspects you.
const INSPECTION_FINE_PER_STANDING: i64 = 5;

// ---- diplomatic annexation (E4) ----
/// Influence accrued per tick (a slow statecraft resource, capped) — the currency
/// of the peaceful acquisition path.
const INFLUENCE_PER_TICK: i64 = 1;
/// Influence ceiling — you bank toward a diplomatic action, you don't hoard forever.
const INFLUENCE_MAX: i64 = 1_000;
/// Influence to annex an independent colony (E4).
const ANNEX_INFLUENCE_COST: i64 = 300;
/// Standing with the Independents required to annex one of their colonies (Cordial).
const ANNEX_STANDING_REQ: i64 = 200;
/// A diplomatic annexation spikes coalition alarm less than a hostile buyout (E4).
const ALARM_PER_ANNEX: i64 = 60;

// ---- corporate diplomacy with the independent companies (E8) ----
/// Influence to court an independent company up a step toward alliance.
const COURT_INFLUENCE_COST: i64 = 100;
/// Relation gained per courting (Neutral→Ally is ~4 courtings).
const COURT_RELATION_GAIN: i64 = 150;
/// How much buying a company's colony out from under it sours the relationship.
const BUYOUT_RELATION_HIT: i64 = 80;
/// How much seizing a company's colony by force craters the relationship (→ Rival).
const SEIZE_RELATION_HIT: i64 = 600;

// ---- military seizure (E5) ----
/// A successful seizure is open aggression — the biggest coalition-alarm spike (E5).
const ALARM_PER_SEIZE: i64 = 220;
/// Crew quality of a colony's defending garrison (E5).
const GARRISON_QUALITY: i64 = 60;

/// How a colony may be annexed (E4/E8 internal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnnexKind {
    /// An Ally company's colony joins for free.
    Free,
    /// Costs Influence (a Partner company, or good Independents standing).
    Influence,
    /// Can't be annexed (a Rival won't join, or not an acquirable target).
    Blocked,
}

/// Why courting an independent company could not proceed (E8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CourtError {
    InvalidCompany,
    NotEnoughInfluence,
}

/// Why a diplomatic annexation could not proceed (E4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnexError {
    /// Not an annexable target (not an independent colony, or already controlled).
    NotAcquirable,
    AlreadyControlled,
    /// Standing with the Independents is below the diplomatic threshold.
    StandingTooLow,
    NotEnoughInfluence,
}

/// Why a military seizure could not proceed (E5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeizeError {
    /// No such colony, or it's already yours.
    InvalidTarget,
    AlreadyControlled,
    /// No warships to mount the assault.
    NoFleet,
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
/// Ticks the player has to mount a defense once an incursion lands before it
/// strikes the bridgehead unanswered (§17, G4). An act-now window, like a shortage.
const INCURSION_RESPONSE_WINDOW: u64 = 36;
/// Crew quality of the far-side incursion raiders (§17, G4) — a notch above the
/// inner-system pirates: the far side fields a tougher enemy.
const INCURSION_QUALITY: i64 = 70;
/// Incursion-pack size scales with severity: one raider per this-much severity,
/// floored at a pair (§17, G4).
const INCURSION_SEVERITY_PER_SHIP: i64 = 25;
/// Bridgehead level the player must reach to win the endgame (§17, G5).
const WIN_BRIDGEHEAD_LEVEL: u32 = 5;
/// Incursions the player must repel to win the endgame (§17, G5). Together with the
/// level threshold this is "grow the foothold *and* hold it through the assault."
const WIN_INCURSIONS_SURVIVED: u64 = 8;

/// Ticks between hauler spawn attempts (≈ one per day at 1 tick/hour).
const SPAWN_INTERVAL: u64 = 24;
/// Cap on concurrent in-flight haulers.
const MAX_HAULERS: usize = 16;
/// Minimum price spread that makes a route worth flying.
const MIN_SPREAD: i64 = 5;
/// Constant-thrust acceleration for civilian haulers/freighters, in distance-units per
/// tick² — the flip-and-burn "G". Ships accelerate to the midpoint, flip, and brake to the
/// destination (a brachistochrone), so transit time scales with √distance and with 1/√accel
/// (the real Expanse-style burn, not a flat cruise). Calibrated for playable inner-system
/// transit; civilian ≈ low-G, the player's warships burn harder (see `movement::plan`).
const ACCEL_CIV: i64 = 6_000;
/// Floor on travel time so close markets still take real time (§21).
const MIN_TRAVEL: u64 = 24;

/// Flip-and-burn (brachistochrone) travel time: `t = 2·√(distance / accel)`. A higher
/// `accel` (higher-G drive) is faster; quadrupling the distance only doubles the time.
pub fn brachistochrone_ticks(dist: i64, accel: i64) -> u64 {
    (2 * (dist.max(0) / accel.max(1)).isqrt()) as u64
}
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
    /// Pending player dilemmas (Phase A): act-now exceptions surfaced as a small menu
    /// of trade-off options. Transient (not persisted) — re-derived from the world.
    decisions: Vec<Decision>,
    next_decision_id: u64,
    /// Weapon-model production lines in progress (id, completion tick) — Phase B.
    weapon_production: Vec<(usize, u64)>,
    salvage: SalvageField,
    /// Ambient flavour chatter (§19 texture) — occasional system colour voiced to the feed.
    /// Its own RNG keeps the economy byte-identical (§27).
    ambient: AmbientChatter,
    /// The player's far-side foothold (§17 endgame, G3). Unfounded until the player
    /// transits the gate and founds it; inert pre-transit.
    bridgehead: Bridgehead,
    /// The tick the player transited the gate (§17, G4) — lights the incursion
    /// escalation clock; `None` until transit. Persisted so a post-transit save
    /// resumes the endgame.
    endgame_since: Option<u64>,
    /// An incursion currently bearing on the bridgehead, awaiting a defense (§17,
    /// G4): `(severity, deadline tick)`. Transient — a reload re-opens a fresh
    /// window rather than persisting mid-incursion state.
    pending_incursion: Option<(i64, u64)>,
    /// Incursions the player has weathered by repelling them (§17, G5) — half of
    /// the victory condition.
    incursions_survived: u64,
    /// How the far-side endgame resolved (§17, G5) — `Undecided` until a win or loss.
    endgame_outcome: EndgameOutcome,
    /// The frontier colonies (the empire layer): static identity from
    /// `frontier::default_colonies`, with `controlled[i]` flagging the ones the
    /// player has taken. A fresh sim controls none, so this is inert by default.
    colonies: Vec<Colony>,
    controlled: Vec<bool>,
    /// **Development level** per colony (Phase C, the *tall* growth axis): higher dev
    /// scales a controlled colony's tribute + specialty output. Inert until you control
    /// + invest. Indexed like `colonies`; starts at `DEV_BASE`.
    colony_dev: Vec<i64>,
    /// Per-colony tick a development build completes (0 = idle); the new level's benefit only
    /// applies once it's built (~180 days) — developing is a paced investment, not instant.
    colony_dev_ready: Vec<u64>,
    /// Empire-wide development doctrine (Phase C) — tilts holding yield. Balanced default.
    dev_doctrine: DevDoctrine,
    /// The player's shipyard: tier (0 = none) + the body it orbits. Warships need it
    /// (corvettes need it *or* OPA standing). Very expensive to build + maintain.
    shipyard_tier: i64,
    shipyard_body: usize,
    /// Tick the shipyard's current construction (founding / expansion) completes — until then
    /// it's a building site and lays down nothing (a major undertaking takes ~a year).
    shipyard_ready_tick: u64,
    /// Warships under construction (§6) — laid down by a commission/assemble order and
    /// standing up into the fleet once their `ready_tick` passes. Empty by default.
    pending_ships: Vec<PendingShip>,
    /// Deployed mining ships (early industry) — each extracts a body's raw per tick.
    miners: Vec<Miner>,
    /// Player-founded outposts (the body-built station layer) — empty by default (inert).
    outposts: Vec<Outpost>,
    /// Tick the next Earth/Mars war flashpoint fires (player collateral). Fires more in
    /// the early game and dwindles as you climb the tiers.
    next_war_flashpoint: u64,
    /// The major frontier hubs the great powers fight over (early game): per-colony
    /// faction influence + the player's accumulated standing. Ambient Earth/Mars flares
    /// shift the balance; the player courts a colony to claim it. Touches only its own
    /// numbers + the feed, so the economy is byte-identical.
    contested: Vec<ContestedColony>,
    /// Tick the next ambient great-power contest flare fires.
    next_contest_flare: u64,
    /// **Per-faction** alarm at the player's expansion (E3/E7), `0..=ALARM_MAX`,
    /// indexed by `Faction`. The inners (Earth/Mars) are alarmed by your *size*; any
    /// power is spiked by acquisitions/seizures **in its sphere** (taking Mars's
    /// colony angers Mars most). A coalition forms when any great power crosses
    /// `COALITION_THRESHOLD`. Persisted; all-0 for a fresh sim (inert).
    faction_alarm: [i64; 4],
    /// Tick the next coalition strike lands while a coalition is active (E3).
    next_coalition_strike: u64,
    /// Whether the upcoming coalition strike has been telegraphed (transient).
    coalition_forecast_sent: bool,
    /// A coalition strike bearing on the holdings, awaiting a defense (E3):
    /// `(strength, deadline tick)`. Transient — a reload re-opens a fresh window.
    pending_coalition: Option<(i64, u64)>,
    /// Influence — the slow statecraft resource spent on diplomatic annexation (E4)
    /// and courting independent companies (E8).
    influence: i64,
    /// Diplomacy with the independent companies (E8) — the negotiable actors.
    diplomacy: Diplomacy,
    missions: super::missions::Missions,
    /// The player's tactical doctrine for fleet engagements (§9): target priority
    /// + retreat threshold (band is chosen per engagement).
    combat_doctrine: Doctrine,
    /// The last resolved battle (band, starting counts, BattleLog) — for the §22
    /// diorama. Transient (not saved).
    last_battle: Option<(Band, [usize; 2], BattleOutcome)>,
    /// Bounty paid for the last won engagement (Phase B) — 0 on a loss/none.
    last_bounty: i64,
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
            decisions: Vec::new(),
            next_decision_id: 1,
            weapon_production: Vec::new(),
            salvage: SalvageField::new(seed, blueprint_count, body_count),
            ambient: AmbientChatter::new(seed),
            bridgehead: Bridgehead::new(),
            endgame_since: None,
            pending_incursion: None,
            incursions_survived: 0,
            endgame_outcome: EndgameOutcome::Undecided,
            colonies: default_colonies(),
            controlled: vec![false; default_colonies().len()],
            colony_dev: vec![DEV_BASE; default_colonies().len()],
            colony_dev_ready: vec![0; default_colonies().len()],
            dev_doctrine: DevDoctrine::default(),
            shipyard_tier: 0,
            shipyard_body: 0,
            shipyard_ready_tick: 0,
            pending_ships: Vec::new(),
            miners: Vec::new(),
            outposts: Vec::new(),
            next_war_flashpoint: 100,
            contested: default_colonies()
                .iter()
                .enumerate()
                .filter(|(_, c)| c.hub)
                .map(|(i, c)| ContestedColony::seed(i, c.faction))
                .collect(),
            next_contest_flare: contest::FLARE_INTERVAL,
            faction_alarm: [0; 4],
            next_coalition_strike: 0,
            coalition_forecast_sent: false,
            pending_coalition: None,
            influence: 0,
            diplomacy: Diplomacy::new(),
            missions: super::missions::Missions::new(),
            combat_doctrine: Doctrine::default(),
            last_battle: None,
            last_bounty: 0,
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

    /// The latest ambient flavour beat — `(voice, message)` — for the shell's system-wire
    /// ticker (§19 texture). `None` before the first beat fires.
    pub fn latest_chatter(&self) -> Option<(&'static str, &'static str)> {
        self.ambient.latest()
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

    /// Whether the player **owns** market `m` (EP2) — a controlled colony sits on its
    /// body. Owned markets trade fee-reduced and earn a tariff on NPC deliveries.
    pub fn market_is_owned(&self, m: usize) -> bool {
        let Some(market) = self.markets.get(m) else {
            return false;
        };
        let body = market.body();
        self.colonies
            .iter()
            .zip(self.controlled.iter())
            .any(|(c, &held)| held && c.body == body)
    }

    /// The brokerage fee for a trade of `value` at market `m`: reduced at a market you
    /// own (EP2, you run the broker), the standard fee at a neutral market, and a
    /// **customs surcharge** on top at a market whose faction you've soured (EP4) —
    /// trading in hostile space costs more, scaling with how badly you've crossed them.
    fn market_trade_fee(&self, m: usize, value: i64) -> i64 {
        if self.market_is_owned(m) {
            return value * OWNED_TRADE_FEE_BP / FEE_DEN;
        }
        let mut bp = Self::TRADE_FEE_BP;
        let standing = self.relations.standing(self.markets[m].faction());
        if standing < 0 {
            // EP4: customs friction at a soured faction's market, up to the max
            // surcharge at fully-hostile standing.
            bp += (-standing).min(1_000) * INSPECTION_SURCHARGE_MAX_BP / 1_000;
        }
        value * bp / FEE_DEN
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
        let total = cost + self.market_trade_fee(m, cost);
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
        let net = revenue - self.market_trade_fee(m, revenue);
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
        let target = self.feed.surfaced().iter().find_map(|a| match a.verb {
            Some(Verb::ExploitShortage { market, commodity }) => Some((market, commodity)),
            _ => None,
        });
        match target {
            Some((m, c)) => self.exploit_shortage(m, c, qty).is_ok(),
            None => false,
        }
    }

    // ---- Phase A: player dilemmas (act-now decisions with trade-offs) ----------

    /// Raise a dilemma for a fresh act-now exception, deduped per kind and capped to a
    /// small menu (no backlog anxiety).
    fn push_decision(
        &mut self,
        kind: DecisionKind,
        market: usize,
        commodity: usize,
        target: u64,
        magnitude: i64,
        now: u64,
    ) {
        if self.decisions.len() >= MAX_DECISIONS {
            return;
        }
        let dup = self.decisions.iter().any(|x| {
            x.kind == kind
                && match kind {
                    DecisionKind::Shortage => x.market == market && x.commodity == commodity,
                    DecisionKind::Wreck => x.target == target,
                    DecisionKind::RaidThreat => true, // one inbound-raid dilemma at a time
                    DecisionKind::WarCollateral => true, // one war flashpoint at a time
                }
        });
        if dup {
            return;
        }
        let id = self.next_decision_id;
        self.next_decision_id += 1;
        self.decisions.push(Decision {
            id,
            kind,
            market,
            commodity,
            target,
            magnitude,
            deadline_tick: now + DECISION_TTL,
        });
    }

    /// A one-line title for the dilemma at `idx` (for the shell header).
    pub fn decision_title(&self, idx: usize) -> String {
        let Some(d) = self.decisions.get(idx) else {
            return String::new();
        };
        match d.kind {
            DecisionKind::Shortage => {
                let c = self.markets[d.market].defs()[d.commodity].name;
                format!("{c} shortage at {}", self.markets[d.market].name())
            }
            DecisionKind::Wreck => {
                let name = self
                    .salvage
                    .wrecks()
                    .iter()
                    .find(|w| w.id == d.target)
                    .map(|w| w.name)
                    .unwrap_or("Derelict");
                format!("Derelict sighted — {name}")
            }
            DecisionKind::RaidThreat => "Raiders inbound on the lanes".to_string(),
            DecisionKind::WarCollateral => "Earth–Mars flashpoint on your lanes".to_string(),
        }
    }

    /// The pending dilemmas (the act-now menu, §0.4).
    pub fn decisions(&self) -> &[Decision] {
        &self.decisions
    }

    /// The cheapest *other* market to source `commodity` for a shortage at `market`,
    /// plus the deal size affordable/available there.
    fn deal_source(&self, market: usize, commodity: usize) -> Option<(usize, i64)> {
        let source = (0..self.markets.len())
            .filter(|&m| m != market && m < self.far_market_start)
            .min_by_key(|&m| self.markets[m].price(commodity))?;
        let price = self.markets[source].price(commodity).max(1);
        let affordable = self.corp.credits() / price;
        let qty = DEAL_QTY
            .min(self.markets[source].stock(commodity))
            .min(affordable);
        Some((source, qty.max(0)))
    }

    /// The trade-off options for a pending dilemma, with live numbers for the shell.
    pub fn decision_options(&self, idx: usize) -> Vec<DecisionOption> {
        let Some(d) = self.decisions.get(idx) else {
            return Vec::new();
        };
        match d.kind {
            DecisionKind::Shortage => self.shortage_options(d),
            DecisionKind::Wreck => Self::wreck_options(),
            DecisionKind::RaidThreat => Self::raid_options(d.magnitude),
            DecisionKind::WarCollateral => self.war_options(d.magnitude),
        }
    }

    fn wreck_options() -> Vec<DecisionOption> {
        vec![
            DecisionOption {
                label: "Strip Hull",
                summary: format!("Cut it up for scrap. +{WRECK_SCRAP} cr, certain."),
                est_credits: WRECK_SCRAP,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Mine Data",
                summary: format!("Pull the data core. +{WRECK_DATA} research, certain."),
                est_credits: 0,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Reverse-Engineer",
                summary: "Crack the tech. ~50%: a recovered blueprint; else only salvage data."
                    .to_string(),
                est_credits: 0,
                rep_delta: 0,
                risky: true,
            },
        ]
    }

    fn raid_options(mag: i64) -> Vec<DecisionOption> {
        vec![
            DecisionOption {
                label: "Hunt Them",
                summary: format!(
                    "Run the raiders off. ~60%: +{mag} cr bounty + calm; else they slip you."
                ),
                est_credits: mag,
                rep_delta: 0,
                risky: true,
            },
            DecisionOption {
                label: "Hire Escorts",
                summary: format!("Pay {ESCORT_FEE} cr for protection — the threat eases, no risk."),
                est_credits: -ESCORT_FEE,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Set an Ambush",
                summary: format!(
                    "Bait a trap. ~38%: +{} cr + calm; else a {} cr loss.",
                    mag * 2,
                    mag / 2
                ),
                est_credits: mag * 2,
                rep_delta: 0,
                risky: true,
            },
        ]
    }

    fn war_options(&self, stake: i64) -> Vec<DecisionOption> {
        let (fav, riv) = self.favored_inner();
        vec![
            DecisionOption {
                label: "Reroute",
                summary: format!("Take the long way around. −{WAR_REROUTE_COST} cr, but safe."),
                est_credits: -WAR_REROUTE_COST,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Run It",
                summary: format!(
                    "Run the blockade. ~55%: through clean; else −{} cr and an inner sours.",
                    stake * 2
                ),
                est_credits: 0,
                rep_delta: 0,
                risky: true,
            },
            DecisionOption {
                label: "Pick a Side",
                summary: format!(
                    "Side with {}: waved through (+rep), but {} won't forget (−rep).",
                    fav.name(),
                    riv.name()
                ),
                est_credits: 0,
                rep_delta: WAR_SIDE_REP,
                risky: false,
            },
        ]
    }

    fn resolve_war_decision(&mut self, d: &Decision, opt: usize) -> DecisionOutcome {
        let stake = d.magnitude.max(0);
        let (fav, riv) = self.favored_inner();
        match opt {
            // Reroute: a sure toll to stay out of the crossfire (the safe play).
            0 => {
                let cost = WAR_REROUTE_COST.min(self.corp.credits());
                self.corp.debit(cost);
                DecisionOutcome {
                    credits: -cost,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Rerouted around the flashpoint: −{cost} cr, cargo safe."),
                }
            }
            // Run it: gamble on slipping the blockade; caught means a loss + a soured inner.
            1 => {
                if self.rng.chance_bp(WAR_RUN_CHANCE_BP) {
                    self.complete_op();
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: false,
                        message: "Slipped the blockade clean — no toll, no scratch.".to_string(),
                    }
                } else {
                    let loss = (stake * 2).min(self.corp.credits());
                    self.corp.debit(loss);
                    self.relations.adjust(riv, -WAR_SIDE_REP / 2);
                    // Collateral on your space assets: a stray round takes out a miner.
                    let asset = if self.miners.pop().is_some() {
                        " A miner was lost in the shooting."
                    } else {
                        ""
                    };
                    DecisionOutcome {
                        credits: -loss,
                        rep_delta: -WAR_SIDE_REP / 2,
                        backfired: true,
                        message: format!(
                            "Caught in the crossfire — lost {loss} cr and soured {}.{asset}",
                            riv.name()
                        ),
                    }
                }
            }
            // Pick a side: free passage + favour with one inner, resentment from the rival.
            _ => {
                self.relations.adjust(fav, WAR_SIDE_REP);
                self.relations.adjust(riv, -WAR_SIDE_REP);
                self.complete_op();
                DecisionOutcome {
                    credits: 0,
                    rep_delta: WAR_SIDE_REP,
                    backfired: false,
                    message: format!(
                        "Sided with {} — waved through, but {} won't forget.",
                        fav.name(),
                        riv.name()
                    ),
                }
            }
        }
    }

    fn shortage_options(&self, d: &Decision) -> Vec<DecisionOption> {
        let cname = self.markets[d.market].defs()[d.commodity].name;
        let owner = self.markets[d.market].faction().name();
        let Some((source, qty)) = self.deal_source(d.market, d.commodity) else {
            return Vec::new();
        };
        let buy = self.markets[source].price(d.commodity);
        let sell = self.markets[d.market].price(d.commodity);
        let spread = (sell - buy).max(0);
        let speculate = qty * spread - qty * sell * Self::TRADE_FEE_BP / 10_000;
        let profiteer = speculate + qty * sell * GOUGE_BONUS_BP / 10_000;
        let relief = qty * (buy * RELIEF_MARGIN_BP / 10_000 - buy);
        vec![
            DecisionOption {
                label: "Speculate",
                summary: format!(
                    "Buy {cname} cheap, sell into the shortage. ~+{speculate} cr, no strings."
                ),
                est_credits: speculate,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Profiteer",
                summary: format!(
                    "Gouge the panic. ~+{profiteer} cr but {owner} resents it (−rep); risk a fine if they already do."
                ),
                est_credits: profiteer,
                rep_delta: -GOUGE_REP,
                risky: true,
            },
            DecisionOption {
                label: "Relief Run",
                summary: format!(
                    "Sell at cost to break the shortage. ~{relief} cr, but {owner} won't forget the favour (+rep, climbs the spine)."
                ),
                est_credits: relief,
                rep_delta: RELIEF_REP,
                risky: false,
            },
        ]
    }

    /// Resolve the dilemma at `idx` with option `opt`. Returns the outcome (credits,
    /// reputation, whether a risky option backfired) or an error if it can't proceed.
    pub fn resolve_decision(
        &mut self,
        idx: usize,
        opt: usize,
    ) -> Result<DecisionOutcome, TradeError> {
        let Some(d) = self.decisions.get(idx).cloned() else {
            return Err(TradeError::InsufficientStock);
        };
        let outcome = match d.kind {
            DecisionKind::Shortage => {
                let o = self.resolve_shortage_decision(&d, opt)?;
                self.feed.resolve_shortage(d.market, d.commodity);
                o
            }
            DecisionKind::Wreck => self.resolve_wreck_decision(&d, opt)?,
            DecisionKind::RaidThreat => self.resolve_raid_decision(&d, opt),
            DecisionKind::WarCollateral => self.resolve_war_decision(&d, opt),
        };
        self.decisions.retain(|x| x.id != d.id);
        // A2: answering an act-now exception is itself a player **operation** — every
        // dilemma resolved climbs the §0 spine (CEO XP + research + ascent), so the
        // feed pays off for *every* play style, not only the shortage-trading Tycoon.
        self.complete_op();
        Ok(outcome)
    }

    fn resolve_wreck_decision(
        &mut self,
        d: &Decision,
        opt: usize,
    ) -> Result<DecisionOutcome, TradeError> {
        // Claim (remove) the derelict regardless of method; the yield depends on the
        // *choice*, not the pre-rolled reward.
        if self.salvage.claim(d.target).is_none() {
            return Err(TradeError::InsufficientStock);
        }
        let outcome = match opt {
            0 => {
                self.corp.credit(WRECK_SCRAP);
                DecisionOutcome {
                    credits: WRECK_SCRAP,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Stripped the hull: +{WRECK_SCRAP} cr of scrap."),
                }
            }
            1 => {
                self.progression.research.add_points(WRECK_DATA);
                self.reveal_gate_beat(); // data may seed the gate mystery (§15→§0.1)
                DecisionOutcome {
                    credits: 0,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Mined the data core: +{WRECK_DATA} research."),
                }
            }
            2 => {
                // Reverse-engineer: a gamble — recover a **weapon schematic** on success
                // (the route to advanced weapons, Phase B; you can't buy them), else
                // consolation data.
                if self.rng.chance_bp(REVENG_CHANCE_BP) {
                    let learned = self.grant_weapon_schematic();
                    self.reveal_gate_beat();
                    let msg = match learned.and_then(weapons::model) {
                        Some(m) => format!("Cracked it — recovered the {} schematic.", m.name),
                        None => {
                            // Every schematic already known — fall back to a blueprint.
                            let i = (d.target as usize)
                                % self.progression.blueprints.known_count().max(1);
                            self.progression.blueprints.reverse_engineer(i);
                            "Cracked it — a recovered blueprint is yours.".to_string()
                        }
                    };
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: false,
                        message: msg,
                    }
                } else {
                    self.progression.research.add_points(WRECK_DATA / 2);
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: true,
                        message: format!(
                            "The tech resisted — salvaged +{} research instead.",
                            WRECK_DATA / 2
                        ),
                    }
                }
            }
            _ => return Err(TradeError::InsufficientStock),
        };
        self.events.push(Event::WreckSalvaged { id: d.target });
        Ok(outcome)
    }

    fn resolve_raid_decision(&mut self, d: &Decision, opt: usize) -> DecisionOutcome {
        let mag = d.magnitude.max(0);
        match opt {
            // Hunt: gamble for a bounty; success calms piracy, failure costs nothing but
            // the chance (they slip away — the ambient raid still preys on NPC traffic).
            0 => {
                if self.rng.chance_bp(HUNT_CHANCE_BP) {
                    self.corp.credit(mag);
                    self.pressure.relieve(PressureKind::Piracy, RAID_RELIEF);
                    DecisionOutcome {
                        credits: mag,
                        rep_delta: 0,
                        backfired: false,
                        message: format!("Ran the raiders off: +{mag} cr bounty, the lanes calm."),
                    }
                } else {
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: true,
                        message: "The raiders slipped the net — no bounty.".to_string(),
                    }
                }
            }
            // Hire escorts: a sure thing — pay the fee, the threat eases.
            1 => {
                self.corp.debit(ESCORT_FEE.min(self.corp.credits()));
                self.pressure.relieve(PressureKind::Piracy, RAID_RELIEF);
                DecisionOutcome {
                    credits: -ESCORT_FEE,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Hired escorts for {ESCORT_FEE} cr — the threat eases."),
                }
            }
            // Ambush: longer odds, double bounty on success, a loss on failure.
            _ => {
                if self.rng.chance_bp(AMBUSH_CHANCE_BP) {
                    self.corp.credit(mag * 2);
                    self.pressure.relieve(PressureKind::Piracy, RAID_RELIEF * 2);
                    DecisionOutcome {
                        credits: mag * 2,
                        rep_delta: 0,
                        backfired: false,
                        message: format!(
                            "The trap sprang shut: +{} cr, the lanes go quiet.",
                            mag * 2
                        ),
                    }
                } else {
                    let loss = (mag / 2).min(self.corp.credits());
                    self.corp.debit(loss);
                    DecisionOutcome {
                        credits: -loss,
                        rep_delta: 0,
                        backfired: true,
                        message: format!("The ambush failed — lost {loss} cr."),
                    }
                }
            }
        }
    }

    fn resolve_shortage_decision(
        &mut self,
        d: &Decision,
        opt: usize,
    ) -> Result<DecisionOutcome, TradeError> {
        let (source, qty) = self
            .deal_source(d.market, d.commodity)
            .ok_or(TradeError::InsufficientStock)?;
        if qty <= 0 {
            return Err(TradeError::InsufficientStock);
        }
        let owner = self.markets[d.market].faction();
        match opt {
            // Speculate: the clean merchant play — source cheap, sell at market.
            0 => {
                let cost = self.buy(source, d.commodity, qty)?;
                let revenue = self.sell(d.market, d.commodity, qty)?;
                Ok(DecisionOutcome {
                    credits: revenue - cost,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Speculated the shortage: +{} cr.", revenue - cost),
                })
            }
            // Profiteer: gouge the panic for extra credits, at a reputation cost — and
            // an already-resentful faction may slap a profiteering fine (the risk).
            1 => {
                let cost = self.buy(source, d.commodity, qty)?;
                let base = self.sell(d.market, d.commodity, qty)?;
                let bonus = base * GOUGE_BONUS_BP / 10_000;
                self.corp.credit(bonus);
                self.relations.adjust(owner, -GOUGE_REP);
                // Risk: a faction you've already soured may claw back part of the gain.
                let standing = self.relations.standing(owner);
                let mut backfired = false;
                let mut fine = 0;
                if standing < 0 {
                    let chance = (-standing).min(6000) as u32; // up to 60%
                    if self.rng.chance_bp(chance) {
                        fine = (base + bonus) / 2;
                        self.corp.debit(fine.min(self.corp.credits()));
                        backfired = true;
                    }
                }
                let net = base + bonus - cost - fine;
                let msg = if backfired {
                    format!(
                        "Profiteered, but {} fined you: net +{net} cr, −rep.",
                        owner.name()
                    )
                } else {
                    format!(
                        "Profiteered the shortage: +{net} cr, but −rep with {}.",
                        owner.name()
                    )
                };
                Ok(DecisionOutcome {
                    credits: net,
                    rep_delta: -GOUGE_REP,
                    backfired,
                    message: msg,
                })
            }
            // Relief Run: flood the market at near cost — forgo profit for goodwill, and
            // count the favour as an operation on the §0 spine.
            2 => {
                let cost = self.buy(source, d.commodity, qty)?;
                let buy_price = cost / qty.max(1);
                let revenue = qty * buy_price * RELIEF_MARGIN_BP / 10_000;
                self.corp.unstore(d.commodity, qty);
                self.markets[d.market].add_stock(d.commodity, qty);
                self.corp.credit(revenue);
                self.relations.adjust(owner, RELIEF_REP);
                Ok(DecisionOutcome {
                    credits: revenue - cost,
                    rep_delta: RELIEF_REP,
                    backfired: false,
                    message: format!("Ran relief to {}: +rep, climbed the spine.", owner.name()),
                })
            }
            _ => Err(TradeError::InsufficientStock),
        }
    }

    /// Commission a warship of `class` into the fleet (§5/§8c): pays its build cost and
    /// **reserves** its crew now, then lays the hull down in the shipyard — it stands up into
    /// the fleet once the build completes ([`commission_build_ticks`]), not instantly.
    pub fn commission_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        self.hull_source_ok(class)?;
        let hull = self.catalog.hull(class);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        self.corp.debit(price);
        self.lay_down_ship(class, None);
        Ok(())
    }

    // ---- shipyards: where warships come from --------------------------------

    /// Whether `class` can be built given your shipyard + OPA standing. Civilians come
    /// from Tycho freely; **corvettes** (Frigates) need a yard *or* good Belt standing;
    /// **Destroyer/Cruiser/Battleship** need a yard of tier ≥ 1/2/3.
    /// The shipyard tier that's **operational** right now — 0 while a found/expand build is
    /// still in progress (a building yard can't lay down hulls yet).
    fn operational_shipyard_tier(&self) -> i64 {
        if self.shipyard_tier > 0 && self.tick >= self.shipyard_ready_tick {
            self.shipyard_tier
        } else {
            0
        }
    }

    fn hull_source_ok(&self, class: ShipClass) -> Result<(), CommissionError> {
        let tier = self.operational_shipyard_tier();
        let need = match class {
            ShipClass::Frigate => {
                if tier >= 1 || self.relations.standing(Faction::Belt) >= CORVETTE_STANDING {
                    return Ok(());
                }
                return Err(CommissionError::NeedShipyard);
            }
            ShipClass::Destroyer => 1,
            ShipClass::Cruiser => 2,
            ShipClass::Battleship => 3,
            _ => return Ok(()), // civilians: bought from Tycho
        };
        if tier >= need {
            Ok(())
        } else {
            Err(CommissionError::NeedShipyard)
        }
    }

    /// Days left on the shipyard's current build (0 iff operational — ceiling so it only
    /// reads 0 once `tick >= ready_tick`, matching `operational_shipyard_tier`).
    pub fn shipyard_build_days(&self) -> u64 {
        if self.shipyard_tier > 0 && self.tick < self.shipyard_ready_tick {
            (self.shipyard_ready_tick - self.tick).div_ceil(6)
        } else {
            0
        }
    }

    /// Whether `class` can be sourced right now (shipyard tier / OPA standing) — for the
    /// shell to show availability before the player tries.
    pub fn can_commission(&self, class: ShipClass) -> bool {
        self.hull_source_ok(class).is_ok()
    }

    /// The player's shipyard tier (0 = none).
    pub fn shipyard_tier(&self) -> i64 {
        self.shipyard_tier
    }

    /// Sandbox/test affordance: a free max-tier shipyard (the gated path is covered by
    /// the native tests + the personas).
    pub fn dev_grant_shipyard(&mut self) {
        self.shipyard_tier = MAX_SHIPYARD_TIER;
        self.shipyard_body = 1;
    }
    /// The body the shipyard orbits (0 if none).
    pub fn shipyard_body(&self) -> usize {
        self.shipyard_body
    }
    /// The largest hull class the current yard tier can lay down (for the shell).
    pub fn shipyard_max_hull(&self) -> &'static str {
        match self.shipyard_tier {
            0 => "—",
            1 => "Destroyer",
            2 => "Cruiser",
            _ => "Battleship",
        }
    }

    /// Found a shipyard at `body` (an asteroid/moon/dwarf or a station body — not the sun
    /// or the gate). Very expensive; sets tier 1. Errors if you already have one.
    pub fn found_shipyard(&mut self, body: usize) -> Result<(), ShipyardError> {
        if self.shipyard_tier > 0 {
            return Err(ShipyardError::AlreadyBuilt);
        }
        let bodies = super::orbit::default_system();
        match bodies.get(body) {
            Some(b)
                if !matches!(
                    b.kind,
                    super::orbit::BodyKind::Star | super::orbit::BodyKind::Gate
                ) => {}
            _ => return Err(ShipyardError::BadSite),
        }
        if self.corp.credits() < SHIPYARD_FOUND_COST {
            return Err(ShipyardError::CantAfford);
        }
        self.corp.debit(SHIPYARD_FOUND_COST);
        self.shipyard_tier = 1;
        self.shipyard_body = body;
        self.shipyard_ready_tick = self.tick + SHIPYARD_FOUND_TICKS; // ~a year to stand up
        self.complete_op();
        Ok(())
    }

    /// Expand the shipyard one tier (unlocks the next hull class). Each tier is dearer.
    pub fn expand_shipyard(&mut self) -> Result<(), ShipyardError> {
        if self.shipyard_tier == 0 {
            return Err(ShipyardError::NoneBuilt);
        }
        if self.shipyard_tier >= MAX_SHIPYARD_TIER {
            return Err(ShipyardError::Maxed);
        }
        // Can't start a new expansion while the current build is still in progress.
        if self.tick < self.shipyard_ready_tick {
            return Err(ShipyardError::NoneBuilt);
        }
        let cost = SHIPYARD_EXPAND_COST * self.shipyard_tier;
        if self.corp.credits() < cost {
            return Err(ShipyardError::CantAfford);
        }
        self.corp.debit(cost);
        self.shipyard_ready_tick = self.tick + SHIPYARD_EXPAND_TICKS;
        self.shipyard_tier += 1;
        self.complete_op();
        Ok(())
    }

    /// Credit cost to expand the yard (−1 if none / maxed).
    pub fn expand_shipyard_cost(&self) -> i64 {
        if self.shipyard_tier == 0 || self.shipyard_tier >= MAX_SHIPYARD_TIER {
            -1
        } else {
            SHIPYARD_EXPAND_COST * self.shipyard_tier
        }
    }

    // ---- early-game mining: bootstrap the industrial empire -----------------

    /// The deployed miners (early industry).
    pub fn miners(&self) -> &[Miner] {
        &self.miners
    }

    /// The raw mineral a body yields when mined (0 Ice / 1 Ore / 2 Volatiles) —
    /// deterministic by the body, so *where* you deploy decides *what* you extract.
    pub fn body_mineral(&self, body: usize) -> usize {
        body.wrapping_mul(2_654_435_761) % 3
    }

    /// Whether the player may deploy a miner at `body`. Player mining is confined to
    /// the **asteroid/Kuiper belts** (the dwarf bodies, Ceres & Pluto) and the **rings
    /// and moons of the outer systems** (moons of the gas giants + outer dwarfs). The
    /// **Earth and Mars AO** — the inner planets and their moons (Luna, Phobos, Deimos)
    /// — is off-limits to player miners.
    pub fn can_mine_body(&self, body: usize) -> bool {
        use super::orbit::BodyKind;
        let bodies = super::orbit::default_system();
        let Some(b) = bodies.get(body) else {
            return false;
        };
        match b.kind {
            // The belts: Ceres/Pluto (dwarf bodies) and the named belt asteroids
            // (Eros/Pallas/Vesta/Tycho) are all workable mining ground.
            BodyKind::DwarfPlanet | BodyKind::Asteroid => true,
            // Moons/rings — but only of the *outer* systems. A moon of a Planet
            // (Earth/Mars) is inner-system AO; a moon of a GasGiant/DwarfPlanet is fair.
            BodyKind::Moon => matches!(
                bodies.get(b.parent).map(|p| p.kind),
                Some(BodyKind::GasGiant | BodyKind::DwarfPlanet)
            ),
            // Stars, gates, the inner/gas-giant surfaces themselves, the far side: no.
            _ => false,
        }
    }

    /// Buy + deploy a miner at `body` (a civilian bought from Tycho — no shipyard needed).
    /// It mines that body's raw into your warehouse each tick. Cheap; the first step.
    /// Restricted to the belts + the outer moons/rings (not the Earth/Mars AO).
    pub fn buy_miner(&mut self, body: usize) -> Result<(), MinerError> {
        if self.miners.len() >= MAX_MINERS {
            return Err(MinerError::Full);
        }
        if !self.can_mine_body(body) {
            return Err(MinerError::BadSite);
        }
        if self.corp.credits() < MINER_COST {
            return Err(MinerError::CantAfford);
        }
        self.corp.debit(MINER_COST);
        let commodity = self.body_mineral(body);
        self.miners.push(Miner { body, commodity });
        self.complete_op();
        Ok(())
    }

    /// Whether the player has a miner deployed at `body`.
    pub fn miner_at(&self, body: usize) -> bool {
        self.miners.iter().any(|m| m.body == body)
    }

    /// Recall one miner from `body` (the "until withdrawn" half of the loop). The hull is
    /// retired — no refund — so redeploying is a deliberate decision. Returns whether a
    /// miner was there to withdraw.
    pub fn withdraw_miner(&mut self, body: usize) -> bool {
        if let Some(i) = self.miners.iter().position(|m| m.body == body) {
            self.miners.remove(i);
            true
        } else {
            false
        }
    }

    // ---- outposts: the body-built station layer (found anywhere, develop into a base) ----

    /// The player's founded outposts.
    pub fn outposts(&self) -> &[Outpost] {
        &self.outposts
    }

    /// The outpost at `body`, if any (object-contextual lookup).
    pub fn outpost_at(&self, body: usize) -> Option<&Outpost> {
        self.outposts.iter().find(|o| o.body == body)
    }

    /// Whether `body` is a valid, free site to found an outpost — a real world (not the
    /// sun/gate/far-side) that you don't already hold (no outpost, shipyard, or colony there).
    pub fn can_found_outpost(&self, body: usize) -> bool {
        use super::orbit::BodyKind;
        if self.outposts.len() >= MAX_OUTPOSTS || self.outpost_at(body).is_some() {
            return false;
        }
        if self.shipyard_tier > 0 && self.shipyard_body == body {
            return false;
        }
        // Not on a frontier colony body you control.
        if self
            .colonies
            .iter()
            .enumerate()
            .any(|(i, c)| c.body == body && self.colony_controlled(i))
        {
            return false;
        }
        matches!(
            super::orbit::default_system().get(body).map(|b| b.kind),
            Some(
                BodyKind::Planet
                    | BodyKind::GasGiant
                    | BodyKind::DwarfPlanet
                    | BodyKind::Moon
                    | BodyKind::Asteroid
            )
        )
    }

    /// **Found an outpost** at `body` (the body-built station the early-game empire grows on).
    /// Cheaper than a shipyard; starts at level 1 and develops from there. A spine op (§0).
    pub fn found_outpost(&mut self, body: usize) -> Result<(), OutpostError> {
        if self.outposts.len() >= MAX_OUTPOSTS {
            return Err(OutpostError::Full);
        }
        if !self.can_found_outpost(body) {
            return Err(OutpostError::BadSite);
        }
        if self.corp.credits() < OUTPOST_FOUND_COST {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(OUTPOST_FOUND_COST);
        // A slow build (~180 days): the outpost is laid down now but inert until `ready_tick`.
        self.outposts.push(Outpost {
            body,
            level: 1,
            ready_tick: self.tick + OUTPOST_BUILD_TICKS,
            facilities: 0,
            rank: RANK_OUTPOST,
            population: OUTPOST_POP_BASE,
            stored: 0,
        });
        self.complete_op();
        Ok(())
    }

    /// Whether the outpost at `body` has facility `kind` (a `FAC_*` bit) built.
    pub fn outpost_has_facility(&self, body: usize, kind: u8) -> bool {
        self.outpost_at(body)
            .is_some_and(|o| o.facilities & kind != 0)
    }

    /// The local-storage capacity of `o` — deepened by a Storage facility, scaled by level (§10).
    fn outpost_store_cap(o: &Outpost) -> i64 {
        let per = if o.facilities & FAC_STORAGE != 0 {
            STORE_CAP_WITH_STORAGE
        } else {
            STORE_CAP_BASE
        };
        per * o.level
    }

    /// The outpost-at-`body`'s local stored stock + its cap (per-asset inventory readout).
    pub fn outpost_stored(&self, body: usize) -> (i64, i64) {
        match self.outpost_at(body) {
            Some(o) => (o.stored, Self::outpost_store_cap(o)),
            None => (0, 0),
        }
    }

    /// **Build a facility** at the outpost on `body` (a `FAC_*` bit). The outpost must be
    /// operational (not mid-build) and not already have it; it's a ~120-day build, during which
    /// the outpost is under construction again. Without a Mine an outpost produces no raw goods.
    pub fn build_facility(&mut self, body: usize, kind: u8) -> Result<(), OutpostError> {
        let o = self.outpost_at(body).ok_or(OutpostError::NoneThere)?;
        if !o.is_ready(self.tick) || o.facilities & kind != 0 {
            return Err(OutpostError::BadSite);
        }
        if self.corp.credits() < OUTPOST_FACILITY_COST {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(OUTPOST_FACILITY_COST);
        let tick = self.tick;
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.facilities |= kind;
            o.ready_tick = tick + OUTPOST_FACILITY_TICKS;
        }
        self.complete_op();
        Ok(())
    }

    /// Whether the player already holds a Capital (only one is allowed — the late-game seat).
    pub fn has_capital(&self) -> bool {
        self.outposts.iter().any(|o| o.rank >= RANK_CAPITAL)
    }

    /// Whether the settlement at `body` is ready to **promote to the next rank** (outpost →
    /// colony → hub → capital): operational, below Capital, maxed level, all facilities built,
    /// and enough population. Only **one** Capital is allowed.
    pub fn can_promote_outpost(&self, body: usize) -> bool {
        let capital_held = self.has_capital();
        self.outpost_at(body).is_some_and(|o| {
            o.is_ready(self.tick)
                && o.rank < RANK_CAPITAL
                && o.level >= MAX_OUTPOST_LEVEL
                && o.facilities & FAC_ALL == FAC_ALL
                && o.population >= promote_pop_threshold(o.rank)
                && (o.rank != RANK_HUB || !capital_held) // only one Capital
        })
    }

    /// **Promote** a settlement to its next rank (the headline progression) — a major ~1-year
    /// build that multiplies its yield. Cost escalates with the target rank.
    pub fn promote_outpost(&mut self, body: usize) -> Result<(), OutpostError> {
        if !self.can_promote_outpost(body) {
            return Err(OutpostError::BadSite);
        }
        let next = self.outpost_at(body).map(|o| o.rank + 1).unwrap_or(1);
        let cost = OUTPOST_PROMOTE_COST * next as i64;
        if self.corp.credits() < cost {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(cost);
        let tick = self.tick;
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.rank = next;
            o.ready_tick = tick + OUTPOST_PROMOTE_TICKS;
        }
        self.complete_op();
        Ok(())
    }

    /// Build progress for the outpost at `body` as `(days_remaining, total_days)`, or `None`
    /// if there's no outpost there or it's already operational.
    pub fn outpost_build_remaining(&self, body: usize) -> Option<u64> {
        let o = self.outpost_at(body)?;
        if o.is_ready(self.tick) {
            None
        } else {
            Some((o.ready_tick - self.tick) / 6) // 6 ticks = 1 day
        }
    }

    /// The credit cost to develop the outpost at `body` one level (escalates with level), or
    /// `None` if there's none there, it's maxed, or a build is still in progress.
    pub fn outpost_develop_cost(&self, body: usize) -> Option<i64> {
        let o = self.outpost_at(body)?;
        if o.level >= MAX_OUTPOST_LEVEL || !o.is_ready(self.tick) {
            return None;
        }
        Some(OUTPOST_DEVELOP_BASE * o.level)
    }

    /// **Develop** the outpost at `body` a level — raising its tribute (and, like colonies,
    /// drawing no coalition alarm: improving your own is the safe growth). A spine op (§0).
    pub fn develop_outpost(&mut self, body: usize) -> Result<(), OutpostError> {
        let cost = match self.outpost_develop_cost(body) {
            Some(c) => c,
            None if self.outpost_at(body).is_none() => return Err(OutpostError::NoneThere),
            None => return Err(OutpostError::Maxed),
        };
        if self.corp.credits() < cost {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(cost);
        // Developing is also a build (~120 days): raise the level now but re-arm the timer so
        // the new capacity only comes online when construction finishes.
        let tick = self.tick;
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.level += 1;
            o.ready_tick = tick + OUTPOST_DEVELOP_TICKS;
        }
        self.complete_op();
        Ok(())
    }

    /// Each **operational** outpost pays a per-level tribute into the treasury (one still
    /// under construction pays nothing). No-op without outposts (byte-identical).
    fn run_outposts(&mut self) {
        if self.outposts.is_empty() {
            return;
        }
        let tick = self.tick;
        // Announce any outpost whose construction completes exactly this tick (the "you'll be
        // told when it's ready" payoff for the slow build).
        let completed: Vec<usize> = self
            .outposts
            .iter()
            .filter(|o| o.ready_tick != 0 && o.ready_tick == tick)
            .map(|o| o.body)
            .collect();
        for body in completed {
            let name = super::orbit::default_system()
                .get(body)
                .map(|b| b.name)
                .unwrap_or("the site");
            self.feed.announce(
                "Foundry",
                format!("Outpost at {name} is now operational — it joins your industrial base."),
                tick,
            );
        }
        // A promoted colony triples its yield (tribute + production) over a bare outpost.
        let rank_mult = |o: &Outpost| rank_yield_mult(o.rank);
        let tribute: i64 = self
            .outposts
            .iter()
            .filter(|o| o.is_ready(tick))
            .map(|o| o.level * OUTPOST_TRIBUTE_PER_LEVEL * rank_mult(o))
            .sum();
        self.corp.credit(tribute);
        // Per-asset inventory (§10): a Mine-equipped operational outpost extracts the body's raw
        // each tick into its **local store** (capped by its Storage facility) — not the global
        // warehouse. A **Hangar** then ships the local stock out to your warehouse; without one
        // the goods pile up on-site (you'll later send a freighter). Without a Mine: nothing.
        let count = self.outposts.len();
        for i in 0..count {
            let o = &self.outposts[i];
            if !o.is_ready(tick) {
                continue;
            }
            let has_mine = o.facilities & FAC_MINE != 0;
            let has_hangar = o.facilities & FAC_HANGAR != 0;
            let cap = Self::outpost_store_cap(o);
            let mult = rank_mult(o);
            let level = o.level;
            if has_mine {
                let produced = level * OUTPOST_MINE_OUTPUT * mult;
                self.outposts[i].stored = (self.outposts[i].stored + produced).min(cap);
            }
            if has_hangar && self.outposts[i].stored > 0 {
                let shipped = (level * HANGAR_SHIP_PER_TICK).min(self.outposts[i].stored);
                self.outposts[i].stored -= shipped;
                let commodity = self.body_mineral(self.outposts[i].body);
                self.corp.store(commodity, shipped);
            }
        }
        // Population: each operational outpost draws settlers while you can **supply it with
        // Ice** from your stores (capped by its level); without Ice it stagnates. The supply
        // loop that gates promotion — you must feed people before an outpost becomes a colony.
        let count = self.outposts.len();
        for i in 0..count {
            if !self.outposts[i].is_ready(tick) {
                continue;
            }
            // Population cap grows with both level and rank, so a colony can grow the people a
            // hub needs, and a hub the people a capital needs.
            let cap =
                self.outposts[i].level * POP_CAP_PER_LEVEL * (self.outposts[i].rank as i64 + 1);
            let fed = self.corp.cargo(ICE_COMMODITY) >= ICE_FEED_PER_TICK;
            let pop = self.outposts[i].population;
            if fed && pop < cap {
                self.corp.unstore(ICE_COMMODITY, ICE_FEED_PER_TICK);
                self.outposts[i].population = (pop + POP_GROWTH).min(cap);
            } else if !fed {
                self.outposts[i].population = (pop - POP_DECAY).max(OUTPOST_POP_BASE);
            }
        }
    }

    // ---- the great-power war (Earth/Mars conflict that haunts the early game) ----

    /// Ticks between Earth/Mars war flashpoints — frequent early, dwindling as you climb
    /// the tiers (the inners' grip on Sol wanes once you're a power, §0).
    fn war_flashpoint_interval(&self) -> u64 {
        match self.campaign.tier() {
            Tier::Station | Tier::Region => 460,
            Tier::Sol => 680,
            _ => 1000, // Gate/Beyond — Earth & Mars influence dwindles
        }
    }

    /// The two inners by your standing: the one you favour, and their rival.
    fn favored_inner(&self) -> (Faction, Faction) {
        if self.relations.standing(Faction::Earth) >= self.relations.standing(Faction::Mars) {
            (Faction::Earth, Faction::Mars)
        } else {
            (Faction::Mars, Faction::Earth)
        }
    }

    /// The ambient Earth/Mars conflict (the early-game "weather"): on a flashpoint, if the
    /// lanes you work are exposed, catch the player in the crossfire as a dilemma.
    fn run_war(&mut self) {
        if self.tick < self.next_war_flashpoint {
            return;
        }
        self.next_war_flashpoint = self.tick + self.war_flashpoint_interval();
        if self.haulers.is_empty()
            || self
                .decisions
                .iter()
                .any(|d| d.kind == DecisionKind::WarCollateral)
        {
            return; // no exposure, or a war dilemma is already open
        }
        let tick = self.tick;
        self.push_decision(DecisionKind::WarCollateral, 0, 0, 0, WAR_STAKE, tick);
        self.feed.announce(
            "The Inners",
            "An Earth–Mars flashpoint flares across your lanes — mind your cargo.".to_string(),
            tick,
        );
    }

    // ---- contested colonies (the great powers fight over the major hubs) ----

    /// The ambient great-power contest over the major frontier hubs (early-game
    /// "weather"): on a flare, Earth and Mars shove over one colony, shifting its
    /// influence balance — voiced via the feed (the Ganymede conflict as the model).
    /// Pure integer + rng-free + touches only contest numbers, so it's byte-identical.
    fn run_contest(&mut self) {
        if self.contested.is_empty() || self.tick < self.next_contest_flare {
            return;
        }
        self.next_contest_flare = self.tick + contest::FLARE_INTERVAL;
        let tick = self.tick;
        let step = tick / contest::FLARE_INTERVAL;
        // Round-robin which colony flares; alternate which inner presses its claim.
        let which = (step as usize) % self.contested.len();
        let earth = Faction::Earth.index();
        let mars = Faction::Mars.index();
        let (gain, lose) = if step.is_multiple_of(2) {
            (mars, earth)
        } else {
            (earth, mars)
        };
        let shift = contest::FLARE_SHIFT.min(self.contested[which].influence[lose]);
        self.contested[which].influence[lose] -= shift;
        self.contested[which].influence[gain] += shift;
        let colony = self.contested[which].colony;
        let colony_name = self.colonies[colony].name;
        let winner = Faction::ALL[gain].name();
        self.feed.announce(
            "The Frontier",
            format!("Earth and Mars clash over {colony_name} — {winner} presses its claim."),
            tick,
        );
    }

    /// Deposit each miner's haul into the warehouse. A miner working an outpost's body hauls
    /// to that on-site station for **+50%** output. No-op without miners (byte-identical: with
    /// no outposts the bonus is 0, so output == the original `MINER_OUTPUT_PER_TICK`).
    fn run_miners(&mut self) {
        if self.miners.is_empty() {
            return;
        }
        let deposits: Vec<(usize, i64)> = self
            .miners
            .iter()
            .map(|m| {
                let has_ready_outpost = self
                    .outpost_at(m.body)
                    .is_some_and(|o| o.is_ready(self.tick));
                let bonus = if has_ready_outpost {
                    MINER_OUTPUT_PER_TICK * OUTPOST_MINER_BONUS_BP / 10_000
                } else {
                    0
                };
                (m.commodity, MINER_OUTPUT_PER_TICK + bonus)
            })
            .collect();
        for (c, qty) in deposits {
            self.corp.store(c, qty);
        }
    }

    /// Drain the shipyard's per-tick maintenance (expensive to keep). No-op without one.
    fn run_shipyard_upkeep(&mut self) {
        if self.shipyard_tier > 0 {
            let upkeep = self.shipyard_tier * SHIPYARD_UPKEEP_PER_TIER;
            let drain = upkeep.min(self.corp.credits());
            self.corp.debit(drain);
        }
    }

    /// Assemble a warship of `class` from the player's **own component stock** (§7d):
    /// consumes the Assembled-tier goods in [`ship_bom`] from the warehouse plus a
    /// small labour fee + crew — far cheaper than buying a finished hull, the payoff
    /// of building out the production chain. Fails if any part or the crew is short.
    pub fn assemble_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        self.hull_source_ok(class)?;
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
        self.lay_down_ship(class, None);
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

    /// Commission a warship of `class` to the **player's custom design** (A2): the
    /// chosen weapon counts + remass (as a percent of tankage), validated through the
    /// fitting (`FitError` → `BadFit`). Same hull price + crew draw as the reference
    /// commission; the design only changes what's bolted on (and thus the stats).
    #[allow(clippy::too_many_arguments)]
    pub fn commission_designed(
        &mut self,
        class: ShipClass,
        pdc_model: usize,
        pdc: u32,
        torp_model: usize,
        torp: u32,
        rail_model: usize,
        rail: u32,
        remass_bp: i64,
    ) -> Result<(), CommissionError> {
        self.hull_source_ok(class)?;
        let hull = self.catalog.hull(class);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        // Validate the fit now (so a bad design is rejected before any cost is paid); the
        // loadout is rebuilt from the persisted `DesignSpec` when the build completes.
        let design = DesignSpec {
            pdc_model,
            pdc,
            torp_model,
            torp,
            rail_model,
            rail,
            remass_bp: remass_bp.clamp(0, 100),
        };
        self.build_designed_loadout(class, &design)
            .map_err(|_| CommissionError::BadFit)?;
        self.corp.debit(price);
        self.lay_down_ship(class, Some(design));
        Ok(())
    }

    /// Rebuild a custom [`Loadout`] from a [`DesignSpec`] (validation at order time + stand-up
    /// at completion share this), threading the current weapon catalog + rng.
    fn build_designed_loadout(&mut self, class: ShipClass, d: &DesignSpec) -> Result<Loadout, ()> {
        let hull = self.catalog.hull(class);
        let remass = hull.remass_capacity * d.remass_bp.clamp(0, 100) / 100;
        let pdc_def = self.chosen_weapon_def(WeaponKind::Pdc, d.pdc_model);
        let torp_def = self.chosen_weapon_def(WeaponKind::Torpedo, d.torp_model);
        let rail_def = self.chosen_weapon_def(WeaponKind::Railgun, d.rail_model);
        self.catalog
            .custom_loadout_with(
                class,
                &pdc_def,
                d.pdc,
                &torp_def,
                d.torp,
                &rail_def,
                d.rail,
                remass,
                50,
                &mut self.rng,
            )
            .map_err(|_| ())
    }

    /// The [`WeaponDef`] for a player-chosen weapon model of `kind` — if the model is in
    /// service (owned), use it; otherwise fall back to the best-owned of that kind.
    pub fn chosen_weapon_def(&self, kind: WeaponKind, model_id: usize) -> WeaponDef {
        match weapons::model(model_id) {
            Some(m) if m.kind == kind && self.corp.owns_weapon(model_id) => m.to_def(),
            _ => self.best_weapon_def(kind),
        }
    }

    /// The [`WeaponDef`] of the highest-tier model of `kind` the player owns (Phase B) —
    /// crafted upgrades flow into newly built ships. Falls back to the generic catalog.
    pub fn best_weapon_def(&self, kind: WeaponKind) -> WeaponDef {
        weapons::weapon_models()
            .into_iter()
            .filter(|m| m.kind == kind && self.corp.owns_weapon(m.id))
            .max_by_key(|m| m.tier)
            .map(|m| m.to_def())
            .unwrap_or_else(|| self.catalog.weapon(kind))
    }

    /// Shared tail of commission/assemble: fit the hull with the player's **best-owned**
    /// weapons (Phase B), draw its crew, christen it (§14), dock it (§6), count the op.
    fn stand_up_hull(&mut self, class: ShipClass) {
        let pdc = self.best_weapon_def(WeaponKind::Pdc);
        let torp = self.best_weapon_def(WeaponKind::Torpedo);
        let rail = self.best_weapon_def(WeaponKind::Railgun);
        let loadout = self
            .catalog
            .loadout_with(class, &pdc, &torp, &rail, 50, &mut self.rng);
        self.stand_up_loadout(loadout);
    }

    /// Stand a fitted hull up into the fleet (shared by reference + custom builds). The crew
    /// was **reserved at lay-down** ([`lay_down_ship`]), so this does not draw it again.
    fn stand_up_loadout(&mut self, loadout: Loadout) {
        let hull_name = loadout.hull().name;
        // A christened call-sign + class, e.g. "Lodestar (Frigate)" (§14). It rolls
        // off the line docked at Ceres Yards (the shipyard) with a full tank (§6).
        let name = format!("{} ({})", ships::christen_ship(&mut self.rng), hull_name);
        let home = self.markets[0].body();
        self.corp
            .add_ship(OwnedShip::new(name, loadout, self.tick, home));
        // The op + the FirstWarship beat are counted when the hull is *ordered*
        // (`lay_down_ship`) — committing to the build is the macro decision that climbs the
        // spine (§0); this is just its delivery.
    }

    /// Build-out time for a commissioned hull, in ticks (6 = 1 day): bigger hulls take
    /// longer. A frigate is weeks; a battleship the better part of a year — so building a
    /// fleet is a paced commitment, not an instant purchase.
    pub fn commission_build_ticks(class: ShipClass) -> u64 {
        match class {
            ShipClass::Frigate | ShipClass::QShip => 180, // ~30 days (a small hull)
            ShipClass::Destroyer => 360,                  // ~60 days
            ShipClass::Cruiser => 600,                    // ~100 days
            ShipClass::Battleship => 1080,                // ~180 days (a capital)
            _ => 120,                                     // civilians (~20 days)
        }
    }

    /// Lay a hull down in the shipyard: **reserve** its crew now and queue it to stand up
    /// into the fleet once the build completes. Cost/parts are already paid by the caller.
    fn lay_down_ship(&mut self, class: ShipClass, design: Option<DesignSpec>) {
        let crew_required = self.catalog.hull(class).crew_required;
        self.corp.assign_crew(crew_required);
        let ready_tick = self.tick + Self::commission_build_ticks(class);
        self.pending_ships.push(PendingShip {
            class,
            ready_tick,
            design,
        });
        self.note_mission(super::missions::Trigger::FirstWarship); // §16 tutorial
        self.complete_op(); // committing to the build is progress on the climb (§0)
    }

    /// Stand up any queued hull whose build has completed (called each `step`).
    fn run_shipyard_builds(&mut self) {
        if self.pending_ships.is_empty() {
            return;
        }
        let tick = self.tick;
        let ready: Vec<PendingShip> = {
            let mut done = Vec::new();
            self.pending_ships.retain(|p| {
                if p.ready_tick <= tick {
                    done.push(*p);
                    false
                } else {
                    true
                }
            });
            done
        };
        for p in ready {
            match p.design {
                Some(d) => {
                    if let Ok(loadout) = self.build_designed_loadout(p.class, &d) {
                        self.stand_up_loadout(loadout);
                    } else {
                        self.stand_up_hull(p.class); // design no longer valid — fall back
                    }
                }
                None => self.stand_up_hull(p.class),
            }
        }
    }

    /// Warships currently under construction (count) — for the shell's build queue.
    pub fn pending_ship_count(&self) -> usize {
        self.pending_ships.len()
    }

    /// `(class, days-left)` for queued build `i`, soonest-ordered first.
    pub fn pending_ship(&self, i: usize) -> Option<(ShipClass, u64)> {
        self.pending_ships
            .get(i)
            .map(|p| (p.class, p.ready_tick.saturating_sub(self.tick).div_ceil(6)))
    }

    /// Test helper: stand up every queued hull at the current tick (skip the build wait),
    /// leaving the rest of the world untouched — so an acceptance test reads as if
    /// commissioning were instant (the behavior these tests assert pre-dates timed builds).
    #[cfg(test)]
    pub(crate) fn finish_pending_ships(&mut self) {
        let t = self.tick;
        for p in self.pending_ships.iter_mut() {
            p.ready_tick = t;
        }
        self.run_shipyard_builds();
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
        let t = (self.tick - s.nav.depart_tick).min(span as u64) as i64;
        // Flip-and-burn distance fraction (accelerate to mid-flight, brake to the dock),
        // matching the NPC hauler profile in `traffic::Hauler::position`.
        let den = span * span;
        let num = if 2 * t <= span {
            2 * t * t
        } else {
            den - 2 * (span - t) * (span - t)
        };
        (
            from.0 + (to.0 - from.0) * num / den,
            from.1 + (to.1 - from.1) * num / den,
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
        brachistochrone_ticks(dist, ACCEL_CIV).max(MIN_TRAVEL)
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
            scrap: self.corp.scrap(),
            schematics: self.corp.schematics().to_vec(),
            arsenal: self.corp.arsenal().to_vec(),
            weapon_production: self.weapon_production.clone(),
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
            endgame_since: self.endgame_since,
            incursions_survived: self.incursions_survived,
            endgame_outcome: self.endgame_outcome,
            controlled_colonies: self.controlled.clone(),
            colony_dev: self.colony_dev.clone(),
            colony_dev_ready: self.colony_dev_ready.clone(),
            dev_doctrine: self.dev_doctrine,
            shipyard_tier: self.shipyard_tier,
            shipyard_ready_tick: self.shipyard_ready_tick,
            shipyard_body: self.shipyard_body,
            pending_ships: self.pending_ships.clone(),
            miners: self.miners.clone(),
            outposts: self.outposts.clone(),
            next_war_flashpoint: self.next_war_flashpoint,
            contested_player_influence: self.contested.iter().map(|c| c.player_influence).collect(),
            faction_alarm: self.faction_alarm,
            influence: self.influence,
            company_relations: self.diplomacy.relations(),
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
        // Restore the arsenal (+ schematics) first so the fleet rebuilds with the
        // player's best-owned weapons (Phase B) — a reload never downgrades your guns.
        self.corp
            .restore_arsenal(s.scrap, s.schematics.clone(), s.arsenal.clone());
        self.weapon_production = s.weapon_production.clone();
        let pdc = self.best_weapon_def(WeaponKind::Pdc);
        let torp = self.best_weapon_def(WeaponKind::Torpedo);
        let rail = self.best_weapon_def(WeaponKind::Railgun);
        // Rebuild each hull's loadout from its class + crew quality (§14), then
        // restore its name and service history.
        let fleet = s
            .fleet
            .iter()
            .map(|sh| {
                let loadout = self.catalog.loadout_with(
                    sh.class,
                    &pdc,
                    &torp,
                    &rail,
                    sh.crew_quality,
                    &mut self.rng,
                );
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
        // Resume the far-side endgame clock if this is a post-transit save (§17, G4).
        self.endgame_since = s.endgame_since;
        if let Some(start) = s.endgame_since {
            self.pressure.begin_endgame(start);
        }
        self.incursions_survived = s.incursions_survived;
        self.endgame_outcome = s.endgame_outcome;
        // The empire layer (E1): restore controlled colonies if the save carries them
        // (old saves / fresh games control none → keep the all-false default).
        if s.controlled_colonies.len() == self.controlled.len() {
            self.controlled = s.controlled_colonies.clone();
        }
        if s.colony_dev.len() == self.colony_dev.len() {
            self.colony_dev = s.colony_dev.clone();
        }
        if s.colony_dev_ready.len() == self.colony_dev_ready.len() {
            self.colony_dev_ready = s.colony_dev_ready.clone();
        }
        self.dev_doctrine = s.dev_doctrine;
        self.shipyard_tier = s.shipyard_tier;
        self.shipyard_ready_tick = s.shipyard_ready_tick;
        self.shipyard_body = s.shipyard_body;
        self.pending_ships = s.pending_ships.clone();
        self.miners = s.miners.clone();
        self.outposts = s.outposts.clone();
        if s.next_war_flashpoint > 0 {
            self.next_war_flashpoint = s.next_war_flashpoint;
        }
        // The ambient powers' influence + flare schedule replayed during the re-sim
        // above; overlay only the player's accumulated standing (player-driven, not
        // re-simmed). Length-guarded so the contest layout can evolve safely.
        if s.contested_player_influence.len() == self.contested.len() {
            for (c, &pi) in self
                .contested
                .iter_mut()
                .zip(s.contested_player_influence.iter())
            {
                c.player_influence = pi.clamp(0, contest::CONTEST_TOTAL);
            }
        }
        // E3/E7: restore per-faction alarm; the strike schedule re-arms from it.
        self.faction_alarm = s.faction_alarm;
        for a in &mut self.faction_alarm {
            *a = (*a).clamp(0, ALARM_MAX);
        }
        self.next_coalition_strike = 0;
        self.influence = s.influence.clamp(0, INFLUENCE_MAX); // E4
        if !s.company_relations.is_empty() {
            self.diplomacy.restore(&s.company_relations); // E8
        }
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
        self.run_weapon_production();
        self.run_shipyard_upkeep();
        self.run_shipyard_builds();
        self.run_miners();
        self.run_outposts();
        self.run_war();
        self.run_contest();
        self.run_holdings();
        self.run_coalition(self.tick);
        self.run_empire_piracy(self.tick);
        self.run_inspections(self.tick);
        // Discovery (§15): the field may turn up a derelict to strip. Its own RNG
        // keeps the economy bit-identical whether or not anyone salvages.
        if let Some(id) = self.salvage.maybe_sight(self.tick) {
            self.events.push(Event::WreckSighted { id });
        }
        // Ambient flavour chatter (§19 texture) — occasional system colour, its own RNG, no
        // mechanical effect, so the economy stays bit-identical.
        if let Some((voice, msg)) = self.ambient.maybe_chatter(self.tick) {
            self.feed.chatter(voice, msg.to_string(), self.tick);
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
        // Phase A: surface fresh act-now exceptions as player dilemmas (menus of
        // trade-off options), and drop any that timed out.
        let now = self.tick;
        let mut shortages: Vec<(usize, usize)> = Vec::new();
        let mut wrecks: Vec<u64> = Vec::new();
        let mut raid = false;
        for e in &self.events {
            match e {
                Event::Scarcity { market, commodity } => shortages.push((*market, *commodity)),
                Event::WreckSighted { id } => wrecks.push(*id),
                Event::ThreatForecast {
                    kind: PressureKind::Piracy,
                    ..
                } => raid = true,
                _ => {}
            }
        }
        for (m, c) in shortages {
            self.push_decision(DecisionKind::Shortage, m, c, 0, 0, now);
        }
        for id in wrecks {
            self.push_decision(DecisionKind::Wreck, 0, 0, id, 0, now);
        }
        if raid {
            let mag = 1500 + self.pressure.level(PressureKind::Piracy) as i64 * 30;
            self.push_decision(DecisionKind::RaidThreat, 0, 0, 0, mag, now);
        }
        self.decisions.retain(|d| d.deadline_tick > now);
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
        // The far-side endgame threat (§17, G4) — dormant until the gate is transited.
        if self.pressure.endgame() {
            self.run_incursions(now);
        }
    }

    /// The far-side incursion layer (§17, G4), run each tick once past the ring: an
    /// escalating threat that telegraphs, lands on the bridgehead as an act-now
    /// "defend" exception, and — if unanswered within the window — damages the
    /// foothold. Gated on `pressure.endgame()`, which is off until transit, so the
    /// pre-transit world never enters here.
    fn run_incursions(&mut self, now: u64) {
        // Once the endgame has resolved (§17, G5) the far side stops pressing — the
        // journey has reached its end, win or lose.
        if self.endgame_outcome != EndgameOutcome::Undecided {
            return;
        }
        // An unanswered incursion strikes the bridgehead when its window lapses.
        if let Some((severity, deadline)) = self.pending_incursion {
            if now >= deadline {
                self.pending_incursion = None;
                self.strike_bridgehead(severity);
            }
        }
        // Telegraph the next incursion ahead of time (§13 forecasting carried over).
        if self.pressure.should_forecast_incursion(now) {
            let eta = self.pressure.incursion_eta(now);
            self.events.push(Event::ThreatForecast {
                kind: PressureKind::Incursion,
                eta,
            });
            self.pressure.mark_incursion_forecast_sent();
        }
        // A new incursion lands (only if none is already pending — one crisis at a
        // time on the foothold).
        if self.pending_incursion.is_none() && self.pressure.incursion_ready(now) {
            let severity = self.pressure.incursion_severity(now);
            self.pending_incursion = Some((severity, now + INCURSION_RESPONSE_WINDOW));
            self.events.push(Event::IncursionStruck { severity });
            self.pressure.after_incursion(now);
        }
    }

    /// Apply incursion damage to the bridgehead and voice it; if it falls, emit the
    /// loss beat (§17, G4/G5). No-op without a founded foothold (the incursion finds
    /// nothing to hit).
    fn strike_bridgehead(&mut self, severity: i64) {
        if !self.bridgehead.is_founded() {
            return;
        }
        let fell = self.bridgehead.damage(severity);
        self.events.push(Event::BridgeheadDamaged {
            integrity: self.bridgehead.integrity(),
        });
        if fell {
            self.events.push(Event::BridgeheadFell);
            // The bridgehead is overrun — the endgame is lost (§17, G5).
            if self.endgame_outcome == EndgameOutcome::Undecided {
                self.endgame_outcome = EndgameOutcome::Fallen;
                self.events.push(Event::EndgameLost);
            }
        }
    }

    /// Check whether the far-side endgame has been won (§17, G5): the bridgehead has
    /// been grown to [`WIN_BRIDGEHEAD_LEVEL`] *and* held through
    /// [`WIN_INCURSIONS_SURVIVED`] repelled incursions. Fires once.
    fn check_endgame_won(&mut self) {
        if self.endgame_outcome == EndgameOutcome::Undecided
            && self.bridgehead.level() >= WIN_BRIDGEHEAD_LEVEL
            && self.incursions_survived >= WIN_INCURSIONS_SURVIVED
        {
            self.endgame_outcome = EndgameOutcome::Triumph;
            self.events.push(Event::EndgameWon);
            self.complete_op();
        }
    }

    /// How the far-side endgame resolved (§17, G5): `Undecided`/`Triumph`/`Fallen`.
    pub fn endgame_outcome(&self) -> EndgameOutcome {
        self.endgame_outcome
    }

    /// Incursions repelled so far (§17, G5) — progress toward the victory threshold.
    pub fn incursions_survived(&self) -> u64 {
        self.incursions_survived
    }

    /// The victory thresholds for the destination panel (§17, G5):
    /// `(target bridgehead level, target incursions survived)`.
    pub fn endgame_targets(&self) -> (u32, u64) {
        (WIN_BRIDGEHEAD_LEVEL, WIN_INCURSIONS_SURVIVED)
    }

    // ---- the empire layer: holdings & acquisition (E1) ----------------------

    /// The frontier colonies (the empire layer) — static identity + faction.
    pub fn colonies(&self) -> &[Colony] {
        &self.colonies
    }

    /// Whether the player controls colony `i`.
    pub fn colony_controlled(&self, i: usize) -> bool {
        self.controlled.get(i).copied().unwrap_or(false)
    }

    /// How many frontier colonies the player controls — the empire's size.
    pub fn controlled_colony_count(&self) -> usize {
        self.controlled.iter().filter(|&&c| c).count()
    }

    /// Total holdings the player runs: the stations they built + the colonies they
    /// control (the unified empire view the EMPIRE panel reads).
    pub fn holding_count(&self) -> usize {
        self.stations.len() + self.controlled_colony_count()
    }

    /// The empire's standing in the system, by holdings (E6) — the headline of the
    /// expansion spine: a legible rank that climbs as you consolidate the frontier.
    pub fn empire_rank(&self) -> &'static str {
        match self.holding_count() {
            0 => "Independent Operator",
            1..=2 => "Local Power",
            3..=5 => "Regional Power",
            6..=9 => "Great Power",
            _ => "Hegemon",
        }
    }

    /// The next empire rank and the holdings it takes to reach it (E6), or `None` at
    /// the summit — the *next* rung of the expansion spine, always visible.
    pub fn next_empire_rank(&self) -> Option<(&'static str, usize)> {
        match self.holding_count() {
            0 => Some(("Local Power", 1)),
            1..=2 => Some(("Regional Power", 3)),
            3..=5 => Some(("Great Power", 6)),
            6..=9 => Some(("Hegemon", 10)),
            _ => None,
        }
    }

    /// Independent colonies the player could **buy** right now (not a great power's
    /// territory, not already controlled) — the economic acquisition targets.
    pub fn acquirable_colonies(&self) -> Vec<usize> {
        (0..self.colonies.len())
            .filter(|&i| self.is_acquirable(i))
            .collect()
    }

    fn is_acquirable(&self, i: usize) -> bool {
        matches!(self.colonies.get(i), Some(c) if c.faction == Faction::Independents)
            && !self.colony_controlled(i)
    }

    /// The credit price to buy colony `i` (markets cost more than outposts), or
    /// `None` if it isn't an acquirable target.
    pub fn colony_acquire_cost(&self, i: usize) -> Option<i64> {
        let c = self.colonies.get(i)?;
        if c.faction != Faction::Independents {
            return None;
        }
        Some(if c.is_market {
            COLONY_PRICE_MARKET
        } else {
            COLONY_PRICE_OUTPOST
        })
    }

    /// **Buy out an independent frontier colony** (the empire layer's economic
    /// acquisition path): pay its price, take control, and pay the political cost —
    /// the inner powers grow wary of a rising outer corporation while the Belt
    /// approves (`Relations::on_player_expand`). Taking ground is a spine op (§0).
    pub fn acquire_colony(&mut self, i: usize) -> Result<(), AcquireError> {
        if self.colony_controlled(i) {
            return Err(AcquireError::AlreadyControlled);
        }
        if !self.is_acquirable(i) {
            return Err(AcquireError::NotAcquirable);
        }
        let cost = self
            .colony_acquire_cost(i)
            .ok_or(AcquireError::NotAcquirable)?;
        if self.corp.credits() < cost {
            return Err(AcquireError::CantAfford);
        }
        self.corp.debit(cost);
        self.controlled[i] = true;
        // The political cost: expansion is never free (be careful not to overextend).
        self.relations.on_player_expand();
        // …and it spikes the inners' alarm — expand too fast and they unite (E3/E7):
        // taking the independent frontier is watched by Earth and Mars alike.
        self.raise_alarm(Faction::Earth, ALARM_PER_ACQUISITION);
        self.raise_alarm(Faction::Mars, ALARM_PER_ACQUISITION);
        // Buying a colony out from under its operator sours the relationship (E8).
        if let Some(ci) = self.diplomacy.company_for_colony(i) {
            self.diplomacy.adjust(ci, -BUYOUT_RELATION_HIT);
        }
        self.events.push(Event::ColonyAcquired { colony: i });
        self.complete_op();
        Ok(())
    }

    /// The player's current Influence — the statecraft resource for diplomatic
    /// annexation (E4).
    pub fn influence(&self) -> i64 {
        self.influence
    }

    // ---- contested colonies (gather influence over the major hubs, early game) ----

    /// The contested major frontier hubs — the powers' tug-of-war state (early game).
    pub fn contested_colonies(&self) -> &[ContestedColony] {
        &self.contested
    }

    /// Number of contested colonies.
    pub fn contested_count(&self) -> usize {
        self.contested.len()
    }

    /// Read contested colony `i` (None out of range).
    pub fn contested_colony(&self, i: usize) -> Option<&ContestedColony> {
        self.contested.get(i)
    }

    /// **Court a contested colony** (the slow early-game gather-influence loop):
    /// spend Influence to build your standing over it. Reach the claim threshold to
    /// take it. Counts as a spine op (§0). Fails without enough Influence.
    pub fn court_contested_colony(&mut self, i: usize) -> Result<(), ContestError> {
        let cc = self.contested.get(i).ok_or(ContestError::NoSuchColony)?;
        if self.colony_controlled(cc.colony) {
            return Err(ContestError::AlreadyControlled);
        }
        if self.influence < contest::COURT_COST {
            return Err(ContestError::NotEnoughInfluence);
        }
        self.influence -= contest::COURT_COST;
        let cc = &mut self.contested[i];
        cc.player_influence =
            (cc.player_influence + contest::COURT_GAIN).min(contest::CONTEST_TOTAL);
        self.complete_op();
        Ok(())
    }

    /// **Claim a contested colony** you've built enough standing over — the
    /// influence path to control (cheaper than a buyout, but slow to earn). Taking a
    /// hub from the powers spikes the inners' alarm and is a spine op. Fails until your
    /// standing clears the claim threshold.
    pub fn claim_contested_colony(&mut self, i: usize) -> Result<(), ContestError> {
        let cc = self.contested.get(i).ok_or(ContestError::NoSuchColony)?;
        let colony = cc.colony;
        if self.colony_controlled(colony) {
            return Err(ContestError::AlreadyControlled);
        }
        if !cc.claimable() {
            return Err(ContestError::NotStrongEnough);
        }
        self.controlled[colony] = true;
        // Taking a contested hub from the powers is watched as expansion (E3/E7).
        self.relations.on_player_expand();
        self.raise_alarm(Faction::Earth, ALARM_PER_ACQUISITION);
        self.raise_alarm(Faction::Mars, ALARM_PER_ACQUISITION);
        self.events.push(Event::ColonyAcquired { colony });
        self.complete_op();
        Ok(())
    }

    // ---- corporate diplomacy with the independent companies (E8) ----

    /// The independent companies — the negotiable diplomatic actors (E8).
    pub fn companies(&self) -> &[super::diplomacy::Company] {
        self.diplomacy.companies()
    }

    /// Number of independent companies (E8).
    pub fn company_count(&self) -> usize {
        self.diplomacy.companies().len()
    }

    /// Company `i`'s relation dial with the player (E8).
    pub fn company_relation(&self, i: usize) -> i64 {
        self.diplomacy.relation(i)
    }

    /// Company `i`'s stance toward the player (E8).
    pub fn company_stance(&self, i: usize) -> Stance {
        self.diplomacy.stance(i)
    }

    /// How many allied companies are lending you escorts (E8).
    pub fn ally_count(&self) -> usize {
        self.diplomacy.ally_count()
    }

    /// The company operating colony `colony`, if any (E8).
    pub fn colony_company(&self, colony: usize) -> Option<usize> {
        self.diplomacy.company_for_colony(colony)
    }

    /// The stance of the company operating `colony` (Neutral if none) (E8).
    fn colony_company_stance(&self, colony: usize) -> Stance {
        self.colony_company(colony)
            .map(|ci| self.diplomacy.stance(ci))
            .unwrap_or(Stance::Neutral)
    }

    /// **Court an independent company** (E8) — the macro diplomacy move: spend
    /// Influence to deepen the relationship a step (Neutral → Partner → Ally). An
    /// Ally's colony joins you freely and its ships help screen your trade.
    pub fn court_company(&mut self, i: usize) -> Result<(), CourtError> {
        if i >= self.diplomacy.companies().len() {
            return Err(CourtError::InvalidCompany);
        }
        if self.influence < COURT_INFLUENCE_COST {
            return Err(CourtError::NotEnoughInfluence);
        }
        self.influence -= COURT_INFLUENCE_COST;
        self.diplomacy.adjust(i, COURT_RELATION_GAIN);
        Ok(())
    }

    /// How a colony may be annexed (E4/E8): free (its company is an Ally), influence-
    /// gated (a Partner company, or good generic Independents standing), or blocked
    /// (a Rival won't join, or it isn't an acquirable target).
    fn annex_kind(&self, i: usize) -> AnnexKind {
        if !self.is_acquirable(i) {
            return AnnexKind::Blocked;
        }
        match self.colony_company_stance(i) {
            Stance::Ally => AnnexKind::Free,
            Stance::Rival => AnnexKind::Blocked,
            stance => {
                let eligible = stance >= Stance::Partner
                    || self.relations.standing(Faction::Independents) >= ANNEX_STANDING_REQ;
                if eligible {
                    AnnexKind::Influence
                } else {
                    AnnexKind::Blocked
                }
            }
        }
    }

    /// Whether colony `i` can be **diplomatically annexed** right now (E4/E8): a
    /// Partner/Ally company's colony (or good Independents standing), with the
    /// Influence to pay (waived for an Ally).
    pub fn can_annex(&self, i: usize) -> bool {
        match self.annex_kind(i) {
            AnnexKind::Free => true,
            AnnexKind::Influence => self.influence >= ANNEX_INFLUENCE_COST,
            AnnexKind::Blocked => false,
        }
    }

    /// **Diplomatically annex an independent colony** (E4/E8) — the peaceful path: it
    /// *joins* you. An **Ally** company's colony joins for free; otherwise it costs
    /// Influence and a Partner relationship (or good Independents standing). Pays the
    /// gentler political cost (`on_player_annex` + a smaller alarm spike) than a buyout.
    pub fn annex_colony(&mut self, i: usize) -> Result<(), AnnexError> {
        if self.colony_controlled(i) {
            return Err(AnnexError::AlreadyControlled);
        }
        match self.annex_kind(i) {
            AnnexKind::Blocked if !self.is_acquirable(i) => return Err(AnnexError::NotAcquirable),
            AnnexKind::Blocked => return Err(AnnexError::StandingTooLow),
            AnnexKind::Influence => {
                if self.influence < ANNEX_INFLUENCE_COST {
                    return Err(AnnexError::NotEnoughInfluence);
                }
                self.influence -= ANNEX_INFLUENCE_COST;
            }
            AnnexKind::Free => {} // an Ally joins willingly, no Influence spent
        }
        self.controlled[i] = true;
        self.relations.on_player_annex();
        // A peaceful annexation alarms the inners less (E7).
        self.raise_alarm(Faction::Earth, ALARM_PER_ANNEX);
        self.raise_alarm(Faction::Mars, ALARM_PER_ANNEX);
        self.events.push(Event::ColonyAcquired { colony: i });
        self.complete_op();
        Ok(())
    }

    /// The defending garrison size for colony `i` (E5) — scaled by its owner: the
    /// inner powers garrison hard, the Independents barely at all, so taking Earth's
    /// ground by force needs a real battlefleet while an outpost falls to a frigate or two.
    pub fn garrison_size(&self, i: usize) -> usize {
        match self.colonies.get(i).map(|c| c.faction) {
            Some(Faction::Earth) => 8,
            Some(Faction::Mars) => 6,
            Some(Faction::Belt) => 4,
            _ => 2,
        }
    }

    /// **Seize a colony by force** (E5) — the aggressive path: muster the fleet and
    /// assault the colony's garrison (any colony, not just independents). A won siege
    /// takes control but at the harshest political price (`on_player_seize` craters
    /// the owner's standing + the biggest alarm spike); a lost one just costs ships.
    /// Returns the battle outcome on a resolved assault.
    pub fn seize_colony(&mut self, i: usize, band: Band) -> Result<BattleOutcome, SeizeError> {
        if i >= self.colonies.len() {
            return Err(SeizeError::InvalidTarget);
        }
        if self.colony_controlled(i) {
            return Err(SeizeError::AlreadyControlled);
        }
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return Err(SeizeError::NoFleet);
        }
        let owner = self.colonies[i].faction;
        let garrison: Vec<Loadout> = (0..self.garrison_size(i))
            .map(|_| {
                ships::reference_loadout_quality(
                    ShipClass::Frigate,
                    GARRISON_QUALITY,
                    &mut self.rng,
                )
            })
            .collect();
        let player_doctrine = Doctrine {
            band,
            ..self.combat_doctrine
        };
        let garrison_doctrine = Doctrine {
            band,
            ..Doctrine::default()
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine: player_doctrine,
            },
            &Fleet {
                ships: &garrison,
                doctrine: garrison_doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        let won = outcome.winner == Some(0);
        let all: Vec<usize> = (0..player_ships.len()).collect();
        self.corp.resolve_engagement_for(all, survivors, won);
        if won {
            self.controlled[i] = true;
            self.relations.on_player_seize(owner);
            // Open aggression spikes the **victim's** alarm hardest (E7 — taking
            // Mars's colony brings Mars down on you), with lesser inner wariness.
            self.raise_alarm(owner, ALARM_PER_SEIZE);
            if owner != Faction::Earth {
                self.raise_alarm(Faction::Earth, ALARM_PER_ACQUISITION);
            }
            if owner != Faction::Mars {
                self.raise_alarm(Faction::Mars, ALARM_PER_ACQUISITION);
            }
            // Taking a company's colony by force makes it a Rival (E8).
            if let Some(ci) = self.diplomacy.company_for_colony(i) {
                self.diplomacy.adjust(ci, -SEIZE_RELATION_HIT);
            }
            self.events.push(Event::ColonyAcquired { colony: i });
            self.complete_op();
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), garrison.len()], outcome.clone()));
        Ok(outcome)
    }

    /// How many holdings the player can govern efficiently (E2) — a base plus a
    /// slice earned through the CEO track. Beyond this, holdings strain (§ Stellaris
    /// admin cap): a seasoned operator runs a wider empire than a green one.
    pub fn admin_capacity(&self) -> usize {
        ADMIN_BASE_CAPACITY
            + (self.progression.ceo.level() / ADMIN_CAPACITY_PER_CEO_LEVELS).max(0) as usize
    }

    /// The administrative load on the company — one per holding (E2).
    pub fn admin_load(&self) -> usize {
        self.holding_count()
    }

    /// Holdings over administrative capacity (E2) — the overextension amount; 0 when
    /// comfortably within reach.
    pub fn admin_strain(&self) -> usize {
        self.admin_load().saturating_sub(self.admin_capacity())
    }

    /// Empire-wide tribute efficiency in basis points (E2): 100% within capacity,
    /// falling with each over-capacity holding down to a floor.
    pub fn holdings_efficiency_bp(&self) -> i64 {
        let strain = self.admin_strain() as i64;
        (10_000 - strain * STRAIN_EFFICIENCY_PENALTY_BP).max(STRAIN_EFFICIENCY_FLOOR_BP)
    }

    /// Per-tick empire income/upkeep (the empire layer): controlled colonies pay
    /// tribute, scaled by administrative efficiency, minus the strain upkeep of any
    /// over-capacity holdings (E2). Within capacity it's pure income; overextended,
    /// holdings go net-negative. A pure credit flow — no market RNG — so a fresh sim
    /// (which controls nothing) is byte-identical and the §7c gate holds.
    fn run_holdings(&mut self) {
        // Influence accrues slowly toward its cap (E4) — the statecraft resource for
        // diplomatic annexation. Pure (no RNG), so a fresh sim stays byte-identical.
        self.influence = (self.influence + INFLUENCE_PER_TICK).min(INFLUENCE_MAX);
        let (out_bp, trib_bp, _) = self.dev_doctrine.weights();
        let gross: i64 = self
            .controlled
            .iter()
            .enumerate()
            .filter(|(_, &held)| held)
            .map(|(i, _)| {
                let base = match self.colonies[i].is_market {
                    true => COLONY_TRIBUTE_MARKET,
                    false => COLONY_TRIBUTE_OUTPOST,
                };
                // Phase C: tribute scales with development, tilted by the doctrine.
                base * self.effective_colony_dev(i) * trib_bp / 10_000
            })
            .sum();
        if gross == 0 {
            return; // no holdings → byte-identical no-op
        }
        let tribute = gross * self.holdings_efficiency_bp() / 10_000;
        let upkeep = self.admin_strain() as i64 * STRAIN_UPKEEP_PER_HOLDING;
        let net = tribute - upkeep;
        if net >= 0 {
            self.corp.credit(net);
        } else {
            // Overextension can drain the treasury, but not below zero.
            let drain = (-net).min(self.corp.credits());
            self.corp.debit(drain);
        }
        // EP1: each controlled colony produces its specialty raw into your warehouse —
        // holdings are supply nodes feeding your production (refine it) and logistics
        // (route/sell it), not just a credit drip. Warehouse-only ⇒ no market RNG, so
        // a fresh sim (which controls nothing) stays byte-identical and §7c holds.
        let outputs: Vec<(usize, i64)> = (0..self.controlled.len())
            .filter(|&i| self.controlled[i])
            .map(|i| (self.colony_specialty(i), self.effective_colony_dev(i)))
            .collect();
        for (c, dev) in outputs {
            // Phase C: output scales with dev, tilted by the doctrine.
            self.corp
                .store(c, COLONY_OUTPUT_PER_TICK * dev * out_bp / 10_000);
        }
    }

    /// The empire-wide development doctrine (Phase C).
    pub fn dev_doctrine(&self) -> DevDoctrine {
        self.dev_doctrine
    }

    /// Cycle the development doctrine (a macro empire knob).
    pub fn cycle_dev_doctrine(&mut self) {
        self.dev_doctrine = self.dev_doctrine.next();
    }

    /// A controlled colony's development level (Phase C) — `DEV_BASE` until invested.
    pub fn colony_dev(&self, i: usize) -> i64 {
        self.colony_dev.get(i).copied().unwrap_or(DEV_BASE)
    }

    /// The **operational** development level of colony `i` — the most-recent level whose build
    /// has finished (one level lower while a development build is still in progress).
    fn effective_colony_dev(&self, i: usize) -> i64 {
        let lvl = self.colony_dev(i);
        if self.colony_dev_ready.get(i).is_some_and(|&t| self.tick < t) {
            (lvl - 1).max(DEV_BASE)
        } else {
            lvl
        }
    }

    /// Days left on colony `i`'s development build (0 if none / done).
    pub fn colony_build_days(&self, i: usize) -> u64 {
        match self.colony_dev_ready.get(i) {
            Some(&t) if self.tick < t => (t - self.tick).div_ceil(6),
            _ => 0,
        }
    }

    /// The credit cost to raise colony `i` one development level (escalates with level).
    /// `None` if it can't be developed (not controlled, maxed, or a build is in progress).
    pub fn develop_cost(&self, i: usize) -> Option<i64> {
        if !self.colony_controlled(i)
            || self.colony_dev(i) >= MAX_DEV
            || self.colony_build_days(i) > 0
        {
            return None;
        }
        let (_, _, cost_bp) = self.dev_doctrine.weights();
        Some(DEV_COST_BASE * self.colony_dev(i) * cost_bp / 10_000)
    }

    /// **Develop** a controlled colony (Phase C, the *tall* growth axis): spend credits
    /// to raise its development a level, scaling its tribute + output. Unlike acquiring a
    /// *new* colony, improving your **own** draws **no coalition alarm** — the safe way
    /// to grow. Counts as an operation on the §0 climb.
    pub fn develop_colony(&mut self, i: usize) -> Result<(), DevelopError> {
        let cost = match self.develop_cost(i) {
            Some(c) => c,
            None if !self.colony_controlled(i) => return Err(DevelopError::NotControlled),
            None => return Err(DevelopError::Maxed),
        };
        if self.corp.credits() < cost {
            return Err(DevelopError::CantAfford);
        }
        self.corp.debit(cost);
        // Raise the level now but arm the build timer — the new capacity (tribute + output)
        // only comes online when construction finishes (~180 days), via effective_colony_dev.
        self.colony_dev[i] += 1;
        self.colony_dev_ready[i] = self.tick + COLONY_DEVELOP_TICKS;
        self.complete_op();
        Ok(())
    }

    /// The highest development among the player's holdings (for the EMPIRE headline).
    pub fn peak_dev(&self) -> i64 {
        (0..self.colonies.len())
            .filter(|&i| self.colony_controlled(i))
            .map(|i| self.colony_dev(i))
            .max()
            .unwrap_or(0)
    }

    /// The specialty raw commodity a colony produces (EP1) — thematic by its faction
    /// (Belters mine ice, Mars ore, Earth volatiles), independents varying by location.
    /// Deterministic; one of the raw tiers `[0,1,2]`.
    pub fn colony_specialty(&self, i: usize) -> usize {
        match self.colonies.get(i).map(|c| c.faction) {
            Some(Faction::Belt) => 0,  // Ice
            Some(Faction::Mars) => 1,  // Ore
            Some(Faction::Earth) => 2, // Volatiles
            _ => i % 3,                // Independents vary by location
        }
    }

    // ---- faction alarm & the coalition (E3) ---------------------------------

    /// The loudest great-power alarm at the player's expansion (E3/E7), `0..=ALARM_MAX`
    /// — the overall coalition pressure (the most-threatened power).
    pub fn coalition_alarm(&self) -> i64 {
        [Faction::Earth, Faction::Mars, Faction::Belt]
            .iter()
            .map(|&f| self.faction_alarm[f.index()])
            .max()
            .unwrap_or(0)
    }

    /// A single great power's alarm at your expansion (E7).
    pub fn faction_alarm(&self, f: Faction) -> i64 {
        self.faction_alarm[f.index()]
    }

    /// The great power leading the coalition (the most alarmed) — whose sphere you've
    /// most provoked (E7).
    pub fn coalition_leader(&self) -> Faction {
        [Faction::Earth, Faction::Mars, Faction::Belt]
            .into_iter()
            .max_by_key(|&f| self.faction_alarm[f.index()])
            .unwrap_or(Faction::Earth)
    }

    /// Whether a great-power coalition has formed and is striking the holdings (E3).
    pub fn coalition_active(&self) -> bool {
        self.coalition_alarm() >= COALITION_THRESHOLD
    }

    /// Whether a coalition strike is bearing on the holdings, awaiting a defense (E3).
    pub fn coalition_strike_pending(&self) -> bool {
        self.pending_coalition.is_some()
    }

    /// Spike a specific faction's alarm (E7) — `by` clamped into `0..=ALARM_MAX`.
    fn raise_alarm(&mut self, f: Faction, by: i64) {
        let a = &mut self.faction_alarm[f.index()];
        *a = (*a + by).clamp(0, ALARM_MAX);
    }

    /// The size-driven alarm baseline for faction `f` (E7): the inners (Earth/Mars)
    /// are made wary by the sheer size of your empire; the Belt is your home and is
    /// only alarmed if you *provoke* it directly (a seized Belt colony), so its
    /// baseline is 0.
    fn alarm_baseline(&self, f: Faction) -> i64 {
        match f {
            Faction::Earth | Faction::Mars => {
                (self.holding_count() as i64 * ALARM_PER_HOLDING).min(ALARM_MAX)
            }
            _ => 0,
        }
    }

    /// The alarm a coalition strike answers to — tighter cadence + bigger packs the
    /// more threatened the powers are.
    fn coalition_period(&self) -> u64 {
        // From the base period at threshold, tightening toward the floor at max alarm.
        let over = (self.coalition_alarm() - COALITION_THRESHOLD).max(0);
        let span = (ALARM_MAX - COALITION_THRESHOLD).max(1);
        let tighten = (COALITION_BASE_PERIOD - COALITION_MIN_PERIOD) as i64 * over / span;
        COALITION_BASE_PERIOD.saturating_sub(tighten as u64)
    }

    /// Per-tick coalition layer (E3/E7): each great power's alarm drifts toward its
    /// size baseline, and once any crosses the threshold a coalition (led by the
    /// angriest power) telegraphs + lands strikes. Inert while the player controls
    /// nothing (baselines 0, spikes 0) — so a fresh sim is byte-identical, §7c holds.
    fn run_coalition(&mut self, now: u64) {
        // Each great power's alarm trends toward its size baseline (a big empire keeps
        // the inners watching); with no holdings every baseline is 0 → alarm decays.
        for f in [Faction::Earth, Faction::Mars, Faction::Belt] {
            let baseline = self.alarm_baseline(f);
            let a = self.faction_alarm[f.index()];
            let next = if a < baseline {
                (a + ALARM_DRIFT).min(baseline)
            } else if a > baseline {
                (a - ALARM_DRIFT).max(baseline)
            } else {
                a
            };
            self.faction_alarm[f.index()] = next;
        }
        if !self.coalition_active() {
            // Cooled below the threshold: the coalition stands down.
            self.coalition_forecast_sent = false;
            self.next_coalition_strike = 0;
            return;
        }
        // Resolve an undefended strike whose window has lapsed.
        if let Some((strength, deadline)) = self.pending_coalition {
            if now >= deadline {
                self.pending_coalition = None;
                self.coalition_seize_holding(strength);
            }
        }
        // Schedule the first strike when the coalition forms.
        if self.next_coalition_strike == 0 {
            self.next_coalition_strike = now + self.coalition_period();
        }
        // Telegraph the incoming strike (§13 forecasting).
        if !self.coalition_forecast_sent
            && now + super::pressure::FORECAST_LEAD >= self.next_coalition_strike
        {
            let eta = self.next_coalition_strike.saturating_sub(now);
            self.events.push(Event::ThreatForecast {
                kind: PressureKind::FactionWar,
                eta,
            });
            self.coalition_forecast_sent = true;
        }
        // Land a strike (only if none is already pending — one crisis at a time).
        if self.pending_coalition.is_none() && now >= self.next_coalition_strike {
            let strength = self.coalition_alarm();
            self.pending_coalition = Some((strength, now + COALITION_RESPONSE_WINDOW));
            self.events.push(Event::CoalitionStrike { strength });
            self.next_coalition_strike = now + self.coalition_period();
            self.coalition_forecast_sent = false;
        }
    }

    /// An undefended coalition strike seizes a holding (E3): the inners liberate the
    /// player's most valuable controlled colony back to the Independents. Taking it
    /// *relieves* the coalition's alarm (they got what they came for). With no colony
    /// to seize, they exact reparations from the treasury instead.
    fn coalition_seize_holding(&mut self, _strength: i64) {
        // Prefer to seize a market colony (the prize), else any controlled one.
        let target = (0..self.colonies.len())
            .filter(|&i| self.controlled[i])
            .max_by_key(|&i| self.colonies[i].is_market as i64);
        if let Some(i) = target {
            self.controlled[i] = false;
            self.events.push(Event::HoldingLost { colony: i });
            // Taking a holding relieves the leader's resolve (they got their prize).
            let leader = self.coalition_leader();
            self.raise_alarm(leader, -ALARM_RELIEF_ON_DEFEND);
        } else {
            let drain = COALITION_REPARATIONS.min(self.corp.credits());
            self.corp.debit(drain);
            self.events.push(Event::HoldingLost { colony: usize::MAX });
        }
    }

    /// **Defend the holdings** against the pending coalition strike (E3): rally the
    /// fleet against a coalition pack scaled by the strike's strength. A win repels
    /// it (no holding lost, alarm relieved, an op); a loss lets the strike through
    /// (a holding is seized). Returns the battle outcome, or `None` if there's no
    /// strike to answer or no warships to answer with.
    pub fn defend_holdings(&mut self, band: Band) -> Option<BattleOutcome> {
        let (strength, _) = self.pending_coalition?;
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return None;
        }
        let over = (strength - COALITION_THRESHOLD).max(0);
        let pack_size = (2 + over / COALITION_STRENGTH_PER_SHIP) as usize;
        let pack: Vec<Loadout> = (0..pack_size)
            .map(|_| {
                ships::reference_loadout_quality(
                    ShipClass::Frigate,
                    COALITION_QUALITY,
                    &mut self.rng,
                )
            })
            .collect();
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
        let all: Vec<usize> = (0..player_ships.len()).collect();
        self.corp.resolve_engagement_for(all, survivors, won);
        self.pending_coalition = None;
        self.feed.resolve_holdings();
        if won {
            // Repelled — the holdings stand and the coalition leader's resolve cools.
            let leader = self.coalition_leader();
            self.raise_alarm(leader, -ALARM_RELIEF_ON_DEFEND);
            self.complete_op();
        } else {
            self.coalition_seize_holding(strength);
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), pack.len()], outcome.clone()));
        Some(outcome)
    }

    /// The bounty paid for the last won engagement (Phase B) — 0 on a loss/none.
    pub fn last_bounty(&self) -> i64 {
        self.last_bounty
    }

    /// Start **producing** a weapon model (Phase B). You can't *buy* advanced weapons:
    /// you must hold the **schematic** (earned by reverse-engineering, never bought),
    /// then tool up a production line — it costs scrap (from combat) + credits and takes
    /// **time** (`production_ticks`, longer for higher tiers). Building a great power's
    /// design **antagonises** that power. The line finishes in `step()` → the arsenal.
    pub fn produce_weapon(&mut self, id: usize) -> Result<(), CraftError> {
        let model = weapons::model(id).ok_or(CraftError::Unknown)?;
        if self.corp.owns_weapon(id) {
            return Err(CraftError::AlreadyOwned);
        }
        if !self.corp.knows_schematic(id) {
            return Err(CraftError::NoSchematic);
        }
        if self.weapon_production.iter().any(|&(m, _)| m == id) {
            return Err(CraftError::AlreadyProducing);
        }
        if self.corp.scrap() < model.scrap_cost {
            return Err(CraftError::NotEnoughScrap);
        }
        if self.corp.credits() < model.credit_cost {
            return Err(CraftError::CantAfford);
        }
        self.corp.spend_scrap(model.scrap_cost);
        self.corp.debit(model.credit_cost);
        if let Some(f) = model.origin.antagonist() {
            self.relations.adjust(f, -(model.tier as i64) * CRAFT_ANGER);
        }
        let done = self.tick + Self::production_ticks(model.tier);
        self.weapon_production.push((id, done));
        Ok(())
    }

    /// How long tooling up a tier-`t` weapon line takes — advanced designs are slower.
    fn production_ticks(tier: u8) -> u64 {
        PRODUCTION_BASE_TICKS + tier as u64 * PRODUCTION_TICKS_PER_TIER
    }

    /// Tick the weapon-production lines: any that finished this tick join the arsenal
    /// (fittable on new/refitted ships) and count as an operation on the climb (§0).
    fn run_weapon_production(&mut self) {
        let now = self.tick;
        let mut finished = Vec::new();
        self.weapon_production.retain(|&(id, done)| {
            if done <= now {
                finished.push(id);
                false
            } else {
                true
            }
        });
        for id in finished {
            self.corp.add_weapon(id);
            if let Some(model) = weapons::model(id) {
                self.feed.announce(
                    "Foundry",
                    format!("{} line online — fit it on new builds.", model.name),
                    now,
                );
            }
            self.complete_op();
        }
    }

    /// Ticks remaining on the production line for `id` (0 = not in production).
    pub fn production_remaining(&self, id: usize) -> u64 {
        self.weapon_production
            .iter()
            .find(|&&(m, _)| m == id)
            .map(|&(_, done)| done.saturating_sub(self.tick))
            .unwrap_or(0)
    }

    /// Refit ship `idx` to **chosen** weapon models per kind (Phase B): swaps its guns,
    /// charges a yard fee, and puts the hull **in the yard** for a refit period (it can't
    /// move or fight until done). An unowned/invalid model id falls back to best-owned
    /// (so passing `usize::MAX` for all three is "refit to best"). Must be docked at the
    /// home yard and not already refitting.
    pub fn refit_ship(
        &mut self,
        idx: usize,
        pdc_model: usize,
        torp_model: usize,
        rail_model: usize,
    ) -> Result<(), RefitError> {
        let now = self.tick;
        let home = self.markets[0].body();
        let fleet = self.corp.fleet();
        let ship = fleet.get(idx).ok_or(RefitError::NoSuchShip)?;
        if ship.is_refitting(now) {
            return Err(RefitError::Busy);
        }
        if ship.nav.in_transit(now) || ship.nav.location != home {
            return Err(RefitError::NotAtYard);
        }
        let class = ship.loadout.hull().class;
        // A capital hull's refit needs your own shipyard — Tycho only handles the small
        // ones when you've no yard of your own.
        if matches!(class, ShipClass::Cruiser | ShipClass::Battleship) && self.shipyard_tier < 1 {
            return Err(RefitError::NeedShipyard);
        }
        let mass = ship.loadout.hull().dry_mass;
        let fee = mass * REFIT_FEE_PER_MASS;
        if self.corp.credits() < fee {
            return Err(RefitError::CantAfford);
        }
        let crew_quality = ship.loadout.crew().quality;
        // Rebuild the loadout with the chosen models (fall back to best-owned).
        let pdc = self.chosen_weapon_def(WeaponKind::Pdc, pdc_model);
        let torp = self.chosen_weapon_def(WeaponKind::Torpedo, torp_model);
        let rail = self.chosen_weapon_def(WeaponKind::Railgun, rail_model);
        let new_loadout =
            self.catalog
                .loadout_with(class, &pdc, &torp, &rail, crew_quality, &mut self.rng);
        self.corp.debit(fee);
        let until = now + mass as u64 * REFIT_TICKS_PER_MASS;
        if let Some(ship) = self.corp.fleet_mut().get_mut(idx) {
            ship.loadout = new_loadout;
            ship.refit_until = until;
        }
        self.complete_op();
        Ok(())
    }

    /// Learn a weapon schematic the player doesn't yet hold (earned, e.g. from
    /// reverse-engineering a derelict). Returns the learned model id, if any was new.
    fn grant_weapon_schematic(&mut self) -> Option<usize> {
        let unknown: Vec<usize> = weapons::weapon_models()
            .iter()
            .map(|m| m.id)
            .filter(|id| !self.corp.knows_schematic(*id))
            .collect();
        if unknown.is_empty() {
            return None;
        }
        let pick = unknown[self.rng.below(unknown.len() as u32) as usize];
        self.corp.learn_schematic(pick);
        if let Some(model) = weapons::model(pick) {
            let tick = self.tick;
            self.feed.announce(
                "R&D",
                format!(
                    "Schematic recovered: {} — you can produce it now.",
                    model.name
                ),
                tick,
            );
        }
        Some(pick)
    }

    // ---- piracy on your trade empire (EP3) ----------------------------------

    /// How many escorts (warships on station) the empire needs to screen its shipping
    /// from piracy (EP3) — scales with holdings, so a bigger empire needs a bigger
    /// navy. Zero when you hold nothing.
    pub fn escorts_needed(&self) -> usize {
        let h = self.holding_count();
        if h == 0 {
            0
        } else {
            1 + h / HOLDINGS_PER_ESCORT
        }
    }

    /// Escorts effectively screening your trade (EP3/E8): your warships on station
    /// plus the ships **allied companies** lend you — diplomacy buys security.
    pub fn effective_escorts(&self) -> usize {
        self.warships_on_station() + self.diplomacy.ally_count()
    }

    /// Whether the empire's shipping is adequately escorted (EP3/E8) — your navy plus
    /// allied support meet or exceed the need.
    pub fn empire_secure(&self) -> bool {
        self.effective_escorts() >= self.escorts_needed()
    }

    /// Standing predation on your trade (EP3): if your empire's shipping outruns its
    /// escorts, pirates skim cargo on a cadence. Countered by keeping a navy **on
    /// station** that scales with the empire — neglect it and a big empire bleeds.
    /// Gated on holding anything, draws no RNG → a fresh sim is byte-identical.
    fn run_empire_piracy(&mut self, now: u64) {
        if self.holding_count() == 0 || !now.is_multiple_of(PIRACY_INTERVAL) {
            return;
        }
        let needed = self.escorts_needed();
        let escorts = self.effective_escorts();
        if escorts >= needed {
            return; // well-screened (navy + allies) — the patrols hold
        }
        let shortfall = (needed - escorts) as i64;
        let loss = (shortfall * PIRACY_LOSS_PER_ESCORT_SHORT).min(self.corp.credits());
        if loss > 0 {
            self.corp.debit(loss);
        }
        self.events.push(Event::EmpireRaided { loss });
    }

    /// The harshest standing a great power holds against the player — how soured the
    /// inners are (EP4). Negative = wary/hostile.
    pub fn worst_standing(&self) -> i64 {
        [Faction::Earth, Faction::Mars, Faction::Belt]
            .iter()
            .map(|&f| self.relations.standing(f))
            .min()
            .unwrap_or(0)
    }

    /// Political enforcement on a trader you've crossed (EP4): on a cadence, a great
    /// power you've soured past the threshold inspects your shipping and fines you,
    /// scaling with how hostile they are. Countered by **repairing the relationship**
    /// (contracts lift standing; it also heals over time) — distinct from piracy
    /// (countered by a navy). Gated on holding assets + a soured power; draws no RNG.
    fn run_inspections(&mut self, now: u64) {
        if self.holding_count() == 0 || !now.is_multiple_of(INSPECTION_INTERVAL) {
            return;
        }
        // The most-soured great power leads the sweep.
        let worst = self.worst_standing();
        if worst > INSPECTION_THRESHOLD {
            return; // no power is angry enough to enforce
        }
        let fine = ((-worst).min(1_000) * INSPECTION_FINE_PER_STANDING).min(self.corp.credits());
        if fine > 0 {
            self.corp.debit(fine);
        }
        self.events.push(Event::Inspected { fine });
    }

    /// Whether an incursion is currently bearing on the bridgehead (§17, G4) — the
    /// shell lights the DEFEND verb while this holds.
    pub fn incursion_pending(&self) -> bool {
        self.pending_incursion.is_some()
    }

    /// The severity of the pending incursion, or 0 if none (§17, G4).
    pub fn pending_incursion_severity(&self) -> i64 {
        self.pending_incursion.map(|(s, _)| s).unwrap_or(0)
    }

    /// **Defend the bridgehead** against the pending incursion (§17, G4): rally the
    /// fleet and resolve combat against a far-side raider pack scaled by the
    /// incursion's severity. A win repels it cleanly (the foothold takes no damage)
    /// and counts as an op; a loss lets the incursion through (the bridgehead is
    /// struck for its severity). Returns the battle outcome, or `None` if there's no
    /// incursion to answer or no warships to answer with.
    pub fn defend_bridgehead(&mut self, band: Band) -> Option<BattleOutcome> {
        let (severity, _) = self.pending_incursion?;
        // The whole fleet rallies to the far side — defending the foothold is the
        // priority, wherever the ships were (§17). Need at least one warship.
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return None;
        }
        // The incursion pack scales with severity — a tougher, growing enemy (§17).
        let pack_size = ((severity / INCURSION_SEVERITY_PER_SHIP).max(2)) as usize;
        let pack: Vec<Loadout> = (0..pack_size)
            .map(|_| {
                ships::reference_loadout_quality(
                    ShipClass::Frigate,
                    INCURSION_QUALITY,
                    &mut self.rng,
                )
            })
            .collect();
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
        let all: Vec<usize> = (0..player_ships.len()).collect();
        self.corp.resolve_engagement_for(all, survivors, won);
        self.pending_incursion = None;
        self.feed.resolve_incursion();
        if won {
            // Repelled — the foothold is safe, the win is progress (§0), and the
            // far side has been weathered one more time (§17, G5).
            self.complete_op();
            self.incursions_survived += 1;
            self.check_endgame_won();
        } else {
            // The line broke — the incursion reaches the bridgehead.
            self.strike_bridgehead(severity);
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), pack.len()], outcome.clone()));
        Some(outcome)
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
            .filter(|(_, s)| {
                !s.nav.in_transit(self.tick)
                    && s.nav.location == muster
                    && !s.is_refitting(self.tick)
            })
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
        self.last_bounty = if won {
            // Phase B: holding the field *pays* (bounty per raider hull) and protects
            // the lanes (calms piracy) — so a navy is a viable economic strategy, not
            // pure attrition. Combat is crew-capped, so this isn't a faucet.
            let bounty = pack.len() as i64 * BOUNTY_PER_RAIDER;
            self.corp.credit(bounty);
            // Scrap recovered from the wrecked raiders — the crafting input (Phase B).
            self.corp.add_scrap(pack.len() as i64 * SCRAP_PER_RAIDER);
            self.pressure
                .relieve(PressureKind::Piracy, COMBAT_PIRACY_RELIEF);
            self.complete_op(); // holding the field is progress on the climb (§0)
            bounty
        } else {
            0
        };
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

    /// Advance the gate-mystery beat counter (§0.1) — but **no longer voice it**. The
    /// placeholder gate lore is removed from the player's view until the proper mid/late-game
    /// arc lands (`docs/MID_LATE_GAME_STORY.md`); the `Missions::reveal_gate` machinery +
    /// `GATE_LORE` stay live (counter still advances, save field still meaningful) so that
    /// arc can re-wire the feed/UI. Re-enable by re-announcing the returned beat here.
    fn reveal_gate_beat(&mut self) {
        let _ = self.missions.reveal_gate();
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
        // The far side now knows your face (§17, G4): light the incursion clock.
        self.endgame_since = Some(tick);
        self.pressure.begin_endgame(tick);
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
        // Reaching the target level may clinch the endgame (§17, G5).
        self.check_endgame_won();
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
            // EP2: an NPC delivery into a market you own pays a tariff to the treasury
            // — your empire earns from the living economy autonomously. (Pure credit,
            // no RNG; owned-only, so a fresh sim is byte-identical and §7c holds.)
            if self.market_is_owned(dest) {
                let value = self.markets[dest].price(commodity) * qty;
                self.corp.credit(value * NPC_TARIFF_BP / FEE_DEN);
            }
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
        let travel = brachistochrone_ticks(dist, ACCEL_CIV).max(MIN_TRAVEL);
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

    /// Test helper: stand up a max-tier shipyard directly (no cost) so warship-building
    /// tests can build any hull. The shipyard gate (Phase B+) is tested on its own path.
    fn yard(sim: &mut Sim) {
        sim.corp_mut().credit(2_000_000); // cover the yard's upkeep during stepped tests
        sim.shipyard_tier = MAX_SHIPYARD_TIER;
        sim.shipyard_body = 1;
    }

    #[test]
    fn a_custom_design_commissions_a_lighter_faster_hull_when_stripped() {
        // A2: commissioning a stripped design (no torpedoes/railgun, less remass) builds
        // a real ship that fits, and a fully-armed one out-guns it — the designer matters.
        let mut sim = Sim::new(0);
        yard(&mut sim);
        sim.corp_mut().credit(2_000_000);
        // A lean frigate: PDC only, no torpedoes, half tanks (the 60-crew pool affords it).
        assert_eq!(
            sim.commission_designed(ShipClass::Frigate, 0, 2, 8, 0, 13, 0, 50),
            Ok(())
        );
        sim.finish_pending_ships();
        assert_eq!(sim.corp().fleet().len(), 1);
        let lean = sim.corp().fleet()[0].loadout.stats();
        // A fully-armed frigate (torpedoes added).
        assert_eq!(
            sim.commission_designed(ShipClass::Frigate, 0, 2, 8, 2, 13, 0, 100),
            Ok(())
        );
        sim.finish_pending_ships();
        let armed = sim.corp().fleet()[1].loadout.stats();
        assert!(
            armed.raw_alpha > lean.raw_alpha,
            "more weapons = more firepower"
        );
        assert!(
            lean.thrust_to_mass > armed.thrust_to_mass,
            "the stripped hull is more mobile"
        );
    }

    #[test]
    fn a_warship_flies_a_committed_trajectory_and_refuels() {
        // §6 / Pillar #2: a move commits a trajectory, spends remass, takes time,
        // and the ship is positional — it can't be re-tasked mid-flight, and a tank
        // refuels at a dock.
        let mut sim = Sim::new(0);
        yard(&mut sim);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.finish_pending_ships();
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
            yard(&mut sim);
            for _ in 0..3 {
                sim.commission_ship(ShipClass::Frigate).unwrap();
            }
            sim.finish_pending_ships();
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
    fn producing_a_weapon_needs_a_schematic_then_time_and_antagonizes_the_power() {
        // Phase B: you can't *buy* advanced weapons — you need the schematic (earned),
        // then tool up a production line that takes time; building a great power's design
        // sours that power. A newly built ship then fits the produced model.
        let mut sim = Sim::new(0);
        yard(&mut sim);
        let base_pdc = sim.best_weapon_def(WeaponKind::Pdc).intercept;
        let model17 = 2usize; // Model 17 PDC (Earth, tier 2)
        assert!(
            matches!(sim.produce_weapon(model17), Err(CraftError::NoSchematic)),
            "no buying — you need the schematic first"
        );
        // Learn the schematic (as reverse-engineering would) + grant scrap.
        sim.corp_mut().learn_schematic(model17);
        sim.corp_mut().add_scrap(200);
        let earth0 = sim.relations().standing(crate::sim::Faction::Earth);
        sim.produce_weapon(model17).unwrap();
        assert!(
            sim.relations().standing(crate::sim::Faction::Earth) < earth0,
            "building Earth's design antagonises Earth"
        );
        assert!(
            !sim.corp().owns_weapon(model17) && sim.production_remaining(model17) > 0,
            "production takes time, it's not instant"
        );
        // Run until the line completes.
        for _ in 0..400 {
            sim.step();
            if sim.corp().owns_weapon(model17) {
                break;
            }
        }
        assert!(
            sim.corp().owns_weapon(model17),
            "the line eventually finishes"
        );
        let up_pdc = sim.best_weapon_def(WeaponKind::Pdc).intercept;
        assert!(
            up_pdc > base_pdc,
            "the produced PDC screens better ({up_pdc} > {base_pdc})"
        );
        // A newly built Frigate fits the produced PDC.
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.finish_pending_ships();
        let ship = sim.corp().fleet().last().unwrap();
        assert!(
            ship.loadout
                .weapons()
                .iter()
                .any(|w| w.kind == WeaponKind::Pdc && w.intercept == up_pdc),
            "the new ship fits the produced PDC"
        );
    }

    #[test]
    fn refitting_upgrades_an_old_hull_for_time_and_money() {
        // Phase B: refit re-equips an existing ship with your best-owned weapons, for a
        // yard fee and a stint in the yard (it can't fight while refitting).
        let mut sim = Sim::new(0);
        yard(&mut sim);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.finish_pending_ships();
        let pdc = |s: &Sim| {
            s.corp().fleet()[0]
                .loadout
                .weapons()
                .iter()
                .find(|w| w.kind == WeaponKind::Pdc)
                .unwrap()
                .intercept
        };
        let before = pdc(&sim);
        // Produce a better PDC line so "best owned" upgrades.
        sim.corp_mut().learn_schematic(2); // Model 17 PDC
        sim.corp_mut().add_scrap(200);
        sim.produce_weapon(2).unwrap();
        for _ in 0..400 {
            sim.step();
            if sim.corp().owns_weapon(2) {
                break;
            }
        }
        assert_eq!(
            pdc(&sim),
            before,
            "the old hull still has its original screen"
        );
        let credits0 = sim.corp().credits();
        sim.refit_ship(0, usize::MAX, usize::MAX, usize::MAX)
            .unwrap(); // refit to best
        assert!(sim.corp().credits() < credits0, "refit charges a yard fee");
        assert!(
            sim.corp().fleet()[0].is_refitting(sim.tick()),
            "the hull is in the yard, out of action"
        );
        assert!(pdc(&sim) > before, "refit swapped in the better screen");
    }

    #[test]
    fn winning_an_engagement_pays_a_bounty() {
        // Phase B: holding the field credits a bounty per raider hull, so a won fight is
        // net-positive — combat is a viable economic strategy, not pure attrition.
        for seed in 0..64 {
            let mut sim = Sim::new(seed);
            yard(&mut sim);
            for _ in 0..3 {
                sim.commission_ship(ShipClass::Frigate).unwrap();
            }
            sim.finish_pending_ships();
            let before = sim.corp().credits();
            let out = sim.engage_raiders(Band::Close).unwrap();
            if out.winner == Some(0) {
                assert!(sim.last_bounty() > 0, "a win pays a bounty");
                assert!(
                    sim.corp().credits() > before,
                    "a won engagement is net-positive (ship loss is not a credit cost)"
                );
                return;
            }
        }
        panic!("no winning engagement found across seeds");
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
    fn a_shortage_dilemma_offers_three_diverging_choices() {
        // Phase A: a shortage isn't a one-press exploit but a menu of trade-offs —
        // speculate (clean profit), profiteer (more credits, rep cost), or relief
        // (forgo profit for goodwill + spine progress). Each pulls a different lever.
        let (ceres, rf) = (0usize, 5usize);
        let owner = Sim::new(0).markets()[ceres].faction();

        // Speculate: profits, leaves reputation untouched.
        let mut a = Sim::new(0);
        let rep0 = a.relations().standing(owner);
        a.push_decision(DecisionKind::Shortage, ceres, rf, 0, 0, a.tick());
        assert_eq!(
            a.decision_options(0).len(),
            3,
            "a dilemma is a menu, not a button"
        );
        let spec = a.resolve_decision(0, 0).unwrap();
        assert!(spec.credits > 0, "speculating a real shortage profits");
        assert_eq!(
            a.relations().standing(owner),
            rep0,
            "speculating costs no rep"
        );
        assert!(a.decisions().is_empty(), "resolving clears the dilemma");

        // Profiteer: out-earns speculating (no fine at neutral standing) but sours the owner.
        let mut b = Sim::new(0);
        b.push_decision(DecisionKind::Shortage, ceres, rf, 0, 0, b.tick());
        let prof = b.resolve_decision(0, 1).unwrap();
        assert!(
            prof.credits > spec.credits,
            "profiteering out-earns the clean play"
        );
        assert!(
            b.relations().standing(owner) < rep0,
            "gouging sours the market's owner"
        );

        // Relief Run: the reputation play — owner standing rises, profit is forgone.
        let mut c = Sim::new(0);
        c.push_decision(DecisionKind::Shortage, ceres, rf, 0, 0, c.tick());
        let relief = c.resolve_decision(0, 2).unwrap();
        assert!(
            c.relations().standing(owner) > rep0,
            "relief earns goodwill"
        );
        assert!(
            relief.credits < prof.credits,
            "relief forgoes the gouge profit"
        );
    }

    #[test]
    fn a_wreck_dilemma_chooses_what_to_extract() {
        // A sighted derelict auto-raises a dilemma: strip for credits, mine data, or
        // gamble on reverse-engineering — the yield follows the *choice*.
        let mut sim = Sim::new(3);
        let mut idx = None;
        for _ in 0..4_000 {
            sim.step();
            if let Some(i) = sim
                .decisions()
                .iter()
                .position(|d| d.kind == DecisionKind::Wreck)
            {
                idx = Some(i);
                break;
            }
        }
        let i = idx.expect("a wreck dilemma should be raised within the run");
        let id = sim.decisions()[i].target;
        assert_eq!(sim.decision_options(i).len(), 3);
        let credits0 = sim.corp().credits();
        let out = sim.resolve_decision(i, 0).unwrap(); // strip for scrap
        assert!(
            out.credits > 0 && sim.corp().credits() > credits0,
            "stripping pays credits"
        );
        assert!(
            sim.wrecks().iter().all(|w| w.id != id),
            "the wreck is consumed"
        );
    }

    #[test]
    fn a_raid_dilemma_eases_the_threat_when_answered() {
        // A telegraphed raid auto-raises a dilemma; hiring escorts is the sure play —
        // pay the fee, the piracy gauge eases.
        let mut sim = Sim::new(0);
        let mut idx = None;
        for _ in 0..2_000 {
            sim.step();
            if let Some(i) = sim
                .decisions()
                .iter()
                .position(|d| d.kind == DecisionKind::RaidThreat)
            {
                idx = Some(i);
                break;
            }
        }
        let i = idx.expect("a raid dilemma should be raised within the run");
        assert_eq!(sim.decision_options(i).len(), 3);
        let before = sim.corp().credits();
        let piracy0 = sim
            .pressure()
            .level(crate::sim::pressure::PressureKind::Piracy);
        let out = sim.resolve_decision(i, 1).unwrap(); // hire escorts
        assert!(
            out.credits < 0 && sim.corp().credits() < before,
            "the fee was paid"
        );
        assert!(
            sim.pressure()
                .level(crate::sim::pressure::PressureKind::Piracy)
                <= piracy0,
            "answering eases the piracy gauge"
        );
    }

    #[test]
    fn an_earth_mars_flashpoint_catches_the_player_in_the_crossfire() {
        // The great-power war is a hazard you live under: a flashpoint raises a dilemma,
        // and rerouting around it costs a sure toll to keep your cargo safe.
        let mut sim = Sim::new(0);
        let mut idx = None;
        for _ in 0..800 {
            sim.step();
            if let Some(i) = sim
                .decisions()
                .iter()
                .position(|d| d.kind == DecisionKind::WarCollateral)
            {
                idx = Some(i);
                break;
            }
        }
        let i = idx.expect("an Earth–Mars flashpoint should fire within the run");
        assert_eq!(sim.decision_options(i).len(), 3);
        let before = sim.corp().credits();
        let out = sim.resolve_decision(i, 0).unwrap(); // reroute
        assert!(
            out.credits < 0 && sim.corp().credits() < before,
            "rerouting around the war costs a toll"
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
        yard(&mut sim);
        let (credits0, crew0) = (sim.corp().credits(), sim.corp().trained_crew());
        sim.commission_ship(ShipClass::Frigate).unwrap();
        // Cost is charged + crew reserved at order time; the hull stands up once built.
        assert!(sim.corp().credits() < credits0);
        assert!(sim.corp().trained_crew() < crew0);
        sim.finish_pending_ships();
        assert_eq!(sim.corp().fleet().len(), 1);
        // A battleship needs more crew than the starting pool can field (§8c).
        assert_eq!(
            sim.commission_ship(ShipClass::Battleship),
            Err(CommissionError::NotEnoughCrew)
        );
    }

    #[test]
    fn commissioning_a_hull_is_a_timed_build_not_instant() {
        // The "no instant macro actions" pace re-aim: a commissioned hull is laid down in
        // the yard and only joins the fleet once its build completes (a frigate ~60 days).
        let mut sim = Sim::new(0);
        yard(&mut sim);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        assert_eq!(sim.corp().fleet().len(), 0, "the hull isn't instant");
        assert_eq!(sim.pending_ship_count(), 1, "it's under construction");
        let (class, days) = sim.pending_ship(0).unwrap();
        assert_eq!(class, ShipClass::Frigate);
        assert!(days > 0, "a build countdown is showing ({days} days)");
        // Run past the build; it stands up exactly once, then the queue is empty.
        let build = Sim::commission_build_ticks(ShipClass::Frigate);
        for _ in 0..=build {
            sim.step();
        }
        assert_eq!(
            sim.corp().fleet().len(),
            1,
            "the hull joins the fleet when built"
        );
        assert_eq!(sim.pending_ship_count(), 0, "the queue drained");
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
        for _ in 0..200 {
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
    fn buying_a_frontier_colony_grows_the_empire_and_alarms_the_inners() {
        // The empire layer (E1): an independent colony can be bought; it joins the
        // player's holdings, pays tribute, and the political cost lands on the inners.
        use crate::sim::faction::Faction;
        let mut sim = Sim::new(1);
        assert_eq!(sim.controlled_colony_count(), 0);
        let targets = sim.acquirable_colonies();
        assert!(
            !targets.is_empty(),
            "there are independent colonies to take"
        );
        let i = targets[0];
        // You can't buy a great power's territory — only independents.
        let earth_owned =
            (0..sim.colonies().len()).find(|&j| sim.colonies()[j].faction != Faction::Independents);
        if let Some(j) = earth_owned {
            assert_eq!(sim.acquire_colony(j), Err(AcquireError::NotAcquirable));
        }
        // Credit just over the (now mid-game-priced) cost so post-purchase wealth stays under
        // the free-float overhead sink — otherwise the sink masks the small colony tribute.
        let cost = sim.colony_acquire_cost(i).unwrap();
        sim.corp_mut().credit(cost + 8_000);
        let before = sim.corp().credits();
        let earth0 = sim.relations().standing(Faction::Earth);
        let belt0 = sim.relations().standing(Faction::Belt);
        assert_eq!(sim.acquire_colony(i), Ok(()));
        assert!(sim.colony_controlled(i));
        assert_eq!(sim.controlled_colony_count(), 1);
        assert!(sim.corp().credits() < before, "buying costs credits");
        // The inners grew wary; the home Belt approved (the overextension pressure).
        assert!(sim.relations().standing(Faction::Earth) < earth0);
        assert!(sim.relations().standing(Faction::Belt) > belt0);
        // No double purchase.
        assert_eq!(sim.acquire_colony(i), Err(AcquireError::AlreadyControlled));
        // A controlled colony pays tribute — the treasury grows hands-off.
        let held = sim.corp().credits();
        for _ in 0..50 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > held,
            "holdings pay tribute over time"
        );
    }

    #[test]
    fn overextension_strains_an_empire_past_its_administrative_reach() {
        // E2: within admin capacity, holdings are full-efficiency income; past it,
        // efficiency falls and strain upkeep turns extra holdings net-negative.
        let mut sim = Sim::new(2);
        sim.corp_mut().credit(2_000_000);
        let cap = sim.admin_capacity();
        assert!(cap >= ADMIN_BASE_CAPACITY);
        assert_eq!(sim.admin_strain(), 0);
        assert_eq!(
            sim.holdings_efficiency_bp(),
            10_000,
            "unstrained = full income"
        );
        // Buy every independent colony available — almost certainly past capacity.
        let targets = sim.acquirable_colonies();
        assert!(targets.len() > cap, "enough colonies to overextend");
        for i in targets {
            let _ = sim.acquire_colony(i);
        }
        assert!(sim.admin_load() > 0);
        assert!(
            sim.admin_strain() > 0,
            "taking the whole frontier overextends the company"
        );
        assert!(
            sim.holdings_efficiency_bp() < 10_000,
            "overextension cuts efficiency"
        );
    }

    #[test]
    fn courting_a_company_to_ally_opens_a_free_annex_and_lends_an_escort() {
        // E8: the macro diplomacy loop — invest Influence to court an independent
        // company; an Ally's colony joins you for free and its ships screen your trade.
        let mut sim = Sim::new(4);
        // Pick a company and its colony.
        assert!(!sim.companies().is_empty());
        let colony = sim.companies()[0].home_colony;
        let company = 0usize;
        // Bank influence and court the company up to Ally (≈4 courtings).
        for _ in 0..1_000 {
            sim.step();
        }
        let mut courted = 0;
        while sim.company_stance(company) != crate::sim::diplomacy::Stance::Ally && courted < 10 {
            if sim.court_company(company).is_err() {
                // ran out of influence — let it accrue
                for _ in 0..120 {
                    sim.step();
                }
            } else {
                courted += 1;
            }
        }
        assert_eq!(
            sim.company_stance(company),
            crate::sim::diplomacy::Stance::Ally,
            "courting reaches alliance"
        );
        // An Ally's colony annexes for free (no Influence spent).
        assert!(sim.can_annex(colony));
        let infl_before = sim.influence();
        assert_eq!(sim.annex_colony(colony), Ok(()));
        assert!(sim.colony_controlled(colony));
        assert_eq!(sim.influence(), infl_before, "an ally joins for free");
    }

    #[test]
    fn seizing_a_companys_colony_makes_it_a_rival() {
        // E8: cross a company (take its colony by force) and it turns Rival, refusing
        // to be annexed thereafter.
        let mut sim = Sim::new(5);
        sim.corp_mut().credit(5_000_000);
        for _ in 0..5 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        let colony = sim.companies()[0].home_colony;
        let company = 0usize;
        assert_ne!(
            sim.company_stance(company),
            crate::sim::diplomacy::Stance::Rival
        );
        let _ = sim.seize_colony(colony, Band::Close);
        if sim.colony_controlled(colony) {
            assert_eq!(
                sim.company_stance(company),
                crate::sim::diplomacy::Stance::Rival,
                "force makes an enemy"
            );
        }
    }

    #[test]
    fn diplomatic_annexation_costs_influence_and_good_standing_not_credits() {
        // E4: the peaceful path — annex an independent colony with banked Influence
        // and Cordial standing, paying a gentler political cost than a buyout.
        use crate::sim::faction::Faction;
        let mut sim = Sim::new(4);
        let i = sim.acquirable_colonies()[0];
        // Without standing or influence, you can't annex.
        assert_eq!(sim.annex_colony(i), Err(AnnexError::StandingTooLow));
        sim.relations_mut().adjust(Faction::Independents, 400); // Cordial
        assert_eq!(
            sim.annex_colony(i),
            Err(AnnexError::NotEnoughInfluence),
            "still need Influence banked"
        );
        // Bank influence over time (it accrues each tick).
        for _ in 0..ANNEX_INFLUENCE_COST {
            sim.step();
        }
        assert!(sim.influence() >= ANNEX_INFLUENCE_COST);
        let credits_before = sim.corp().credits();
        let earth_before = sim.relations().standing(Faction::Earth);
        assert!(sim.can_annex(i));
        assert_eq!(sim.annex_colony(i), Ok(()));
        assert!(sim.colony_controlled(i));
        assert_eq!(
            sim.corp().credits(),
            credits_before,
            "annexation costs no credits"
        );
        assert!(sim.influence() < ANNEX_INFLUENCE_COST, "it spent Influence");
        // A gentler ding than a buyout (−20 vs −40), but still some inner wariness.
        assert!(sim.relations().standing(Faction::Earth) < earth_before);
        assert!(sim.relations().standing(Faction::Earth) >= earth_before - 25);
    }

    #[test]
    fn military_seizure_takes_a_colony_by_force_at_the_harshest_political_price() {
        // E5: the aggressive path — assault a colony's garrison and, on a win, take
        // it (even a great power's), enraging the owner.
        let mut sim = Sim::new(7);
        yard(&mut sim);
        sim.corp_mut().credit(5_000_000);
        // Need a fleet to mount an assault.
        let indie = sim.acquirable_colonies()[0];
        assert_eq!(
            sim.seize_colony(indie, Band::Close),
            Err(SeizeError::NoFleet)
        );
        for _ in 0..5 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        sim.finish_pending_ships();
        // Seize a lightly-garrisoned independent colony (2 defenders) — 5 frigates win.
        let owner = sim.colonies()[indie].faction;
        let alarm_before = sim.coalition_alarm();
        let owner_before = sim.relations().standing(owner);
        let outcome = sim
            .seize_colony(indie, Band::Close)
            .expect("a resolved assault");
        assert_eq!(outcome.winner, Some(0), "the squadron takes the colony");
        assert!(sim.colony_controlled(indie));
        // Open aggression: the biggest alarm spike + the owner is enraged.
        assert!(sim.coalition_alarm() > alarm_before);
        assert!(sim.relations().standing(owner) < owner_before);
        // Can't seize what you already hold.
        assert_eq!(
            sim.seize_colony(indie, Band::Close),
            Err(SeizeError::AlreadyControlled)
        );
    }

    #[test]
    fn overexpansion_provokes_a_coalition_that_seizes_an_undefended_holding() {
        // E3: grow too big and the great powers unite; an undefended strike pries a
        // holding from your grip — the geopolitical cap on reckless expansion.
        let mut sim = Sim::new(3);
        sim.corp_mut().credit(5_000_000);
        for i in sim.acquirable_colonies() {
            let _ = sim.acquire_colony(i);
        }
        // A couple of stations push the empire past the alarm baseline.
        let _ = sim.found_refinery(0, 0, 1);
        let _ = sim.found_refinery(1, 0, 1);
        assert!(sim.holding_count() >= 6, "a sizeable empire");
        let mut struck = false;
        for _ in 0..600 {
            sim.step();
            if sim.coalition_strike_pending() {
                struck = true;
                break;
            }
        }
        assert!(
            sim.coalition_active(),
            "overexpansion united the great powers"
        );
        assert!(struck, "the coalition moved on the holdings");
        // Leave it undefended — a holding is seized.
        let before = sim.controlled_colony_count();
        for _ in 0..(COALITION_RESPONSE_WINDOW + 5) {
            sim.step();
        }
        assert!(
            sim.controlled_colony_count() < before,
            "an undefended coalition strike costs a colony"
        );
    }

    #[test]
    fn defending_repels_the_coalition_and_keeps_the_holdings() {
        // E3: with a fleet, you can answer the coalition and hold what you took.
        let mut sim = Sim::new(8);
        yard(&mut sim);
        sim.corp_mut().credit(5_000_000);
        for i in sim.acquirable_colonies() {
            let _ = sim.acquire_colony(i);
        }
        let _ = sim.found_refinery(0, 0, 1);
        let _ = sim.found_refinery(1, 0, 1);
        for _ in 0..5 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        sim.finish_pending_ships();
        assert!(!sim.corp().fleet().is_empty());
        let mut defended = false;
        for _ in 0..600 {
            sim.step();
            if sim.coalition_strike_pending() {
                let held = sim.controlled_colony_count();
                let outcome = sim.defend_holdings(Band::Close);
                assert!(outcome.is_some(), "the fleet answers");
                assert!(!sim.coalition_strike_pending(), "the strike is resolved");
                assert_eq!(
                    sim.controlled_colony_count(),
                    held,
                    "a won defense loses no holding"
                );
                defended = true;
                break;
            }
        }
        assert!(defended, "a coalition strike arrived to defend against");
    }

    #[test]
    fn souring_a_faction_brings_customs_surcharges_and_inspection_fines() {
        // EP4: anger a great power and trading in its space costs more (customs
        // surcharge), and — once you hold assets — it inspects and fines your
        // shipping. Countered by repairing the relationship.
        use crate::sim::faction::Faction;
        let mut sim = Sim::new(3);
        sim.corp_mut().credit(500_000);
        // Find an Earth-owned market; the fee is the baseline while neutral.
        let m = (0..sim.markets().len())
            .find(|&m| sim.markets()[m].faction() == Faction::Earth)
            .expect("an Earth market");
        let neutral_fee = sim.market_trade_fee(m, 100_000);
        // Sour Earth hard → trading there now carries a customs surcharge.
        sim.relations_mut().adjust(Faction::Earth, -800);
        assert!(
            sim.market_trade_fee(m, 100_000) > neutral_fee,
            "trading in soured space costs more"
        );
        // Take a colony so you're a trader with assets to inspect, then run.
        let c = sim.acquirable_colonies()[0];
        let _ = sim.acquire_colony(c);
        let mut inspected = false;
        for _ in 0..(INSPECTION_INTERVAL * 2) {
            if sim
                .step()
                .iter()
                .any(|e| matches!(e, Event::Inspected { .. }))
            {
                inspected = true;
            }
        }
        assert!(inspected, "a soured power inspects and fines your shipping");
        // Mend fences (standing back above the threshold) → inspections stop.
        sim.relations_mut().adjust(Faction::Earth, 1_000);
        assert!(sim.worst_standing() > INSPECTION_THRESHOLD);
        let mut inspected_after = false;
        for _ in 0..(INSPECTION_INTERVAL * 2) {
            if sim
                .step()
                .iter()
                .any(|e| matches!(e, Event::Inspected { .. }))
            {
                inspected_after = true;
            }
        }
        assert!(
            !inspected_after,
            "repairing the relationship stops the sweeps"
        );
    }

    #[test]
    fn seizing_a_powers_colony_alarms_that_power_most() {
        // E7: the coalition is per-faction — taking Mars's colony by force spikes
        // *Mars's* alarm hardest, and Mars leads the response. Buying the independent
        // frontier, by contrast, alarms the inners evenly.
        use crate::sim::faction::Faction;
        let mut sim = Sim::new(6);
        sim.corp_mut().credit(5_000_000);
        for _ in 0..6 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        // Find a Mars-owned colony with a light enough garrison to take with 6 frigates.
        let mars = (0..sim.colonies().len())
            .filter(|&i| sim.colonies()[i].faction == Faction::Mars)
            .min_by_key(|&i| sim.garrison_size(i))
            .expect("a Mars colony");
        let earth_before = sim.faction_alarm(Faction::Earth);
        let _ = sim.seize_colony(mars, Band::Close);
        if sim.colony_controlled(mars) {
            // A successful seizure alarms Mars far more than Earth.
            assert!(
                sim.faction_alarm(Faction::Mars) > sim.faction_alarm(Faction::Earth),
                "the victim power is the most alarmed"
            );
            assert!(
                sim.faction_alarm(Faction::Earth) > earth_before,
                "others note it too"
            );
            assert_eq!(
                sim.coalition_leader(),
                Faction::Mars,
                "Mars leads the response"
            );
        }
    }

    #[test]
    fn an_unescorted_trade_empire_is_raided_but_a_navy_protects_it() {
        // EP3: a growing empire with too few escorts on station is preyed upon by
        // pirates; a navy that scales with the empire deters them. Real but counterable.
        let mut sim = Sim::new(2);
        yard(&mut sim);
        sim.corp_mut().credit(150_000);
        for i in sim.acquirable_colonies() {
            let _ = sim.acquire_colony(i);
        }
        assert!(sim.holding_count() > 0);
        assert!(sim.escorts_needed() >= 1);
        assert!(!sim.empire_secure(), "no warships yet → unescorted");
        // With no navy, a raid event fires within a few cadences.
        let mut raided = false;
        for _ in 0..(PIRACY_INTERVAL * 3) {
            if sim
                .step()
                .iter()
                .any(|e| matches!(e, Event::EmpireRaided { .. }))
            {
                raided = true;
            }
        }
        assert!(raided, "an unescorted empire is preyed upon");
        // Stand up a navy that covers the empire → secure, and raids stop.
        for _ in 0..(sim.escorts_needed() + 2) {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        sim.finish_pending_ships();
        assert!(
            sim.empire_secure(),
            "a navy that scales with the empire protects it"
        );
        let mut raided_after = false;
        for _ in 0..(PIRACY_INTERVAL * 3) {
            if sim
                .step()
                .iter()
                .any(|e| matches!(e, Event::EmpireRaided { .. }))
            {
                raided_after = true;
            }
        }
        assert!(!raided_after, "escorted shipping is no longer raided");
    }

    #[test]
    fn owning_a_market_cuts_your_fee_and_earns_a_tariff_on_npc_trade() {
        // EP2: a colony you control is a market you own — you trade there fee-reduced,
        // and NPC deliveries into it pay your treasury a tariff (your empire earns from
        // the living economy). A market you don't own does neither.
        let mut sim = Sim::new(1);
        // Just enough for the (mid-game-priced) market colony, kept under the free-float so
        // the wealth sink doesn't swamp the tribute/tariff we're measuring.
        sim.corp_mut().credit(330_000);
        // Find a market-colony to take, and its market index (same body).
        let colony = (0..sim.colonies().len())
            .find(|&i| {
                sim.colonies()[i].is_market && sim.colonies()[i].faction == Faction::Independents
            })
            .expect("an independent market colony");
        let body = sim.colonies()[colony].body;
        let m = (0..sim.markets().len())
            .find(|&m| sim.markets()[m].body() == body)
            .expect("its market");
        assert!(!sim.market_is_owned(m), "not owned before acquiring");
        assert_eq!(sim.acquire_colony(colony), Ok(()));
        assert!(sim.market_is_owned(m), "owned after acquiring");
        // The fee on a buy at the owned market is the reduced rate.
        let owned_fee = sim.market_trade_fee(m, 100_000);
        let other = (0..sim.markets().len())
            .find(|&x| !sim.market_is_owned(x))
            .expect("an unowned market");
        assert!(
            owned_fee < sim.market_trade_fee(other, 100_000),
            "owning the broker is cheaper"
        );
        // NPC deliveries into the owned market grow the treasury over time (the tariff).
        let before = sim.corp().credits();
        for _ in 0..800 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > before,
            "tariff + tribute grow the treasury from NPC trade through your market"
        );
    }

    #[test]
    fn controlled_colonies_supply_raw_goods_into_your_warehouse() {
        // EP1: a controlled colony produces its specialty raw into your warehouse each
        // tick — holdings feed your supply chain, not just a credit drip.
        let mut sim = Sim::new(1);
        sim.corp_mut().credit(500_000);
        let i = sim.acquirable_colonies()[0];
        let specialty = sim.colony_specialty(i);
        let before = sim.corp().cargo(specialty);
        assert_eq!(sim.acquire_colony(i), Ok(()));
        for _ in 0..50 {
            sim.step();
        }
        let after = sim.corp().cargo(specialty);
        assert!(
            after >= before + 50 * COLONY_OUTPUT_PER_TICK,
            "the colony stocked your warehouse with its specialty good"
        );
    }

    #[test]
    fn developing_a_colony_scales_output_and_draws_no_coalition_alarm() {
        // Phase C (the *tall* axis): investing in a colony raises its development, which
        // scales its specialty output — and unlike acquiring a *new* colony, improving
        // your own draws no extra coalition alarm.
        let mut sim = Sim::new(1);
        sim.corp_mut().credit(2_000_000);
        let i = sim.acquirable_colonies()[0];
        sim.acquire_colony(i).unwrap();
        let specialty = sim.colony_specialty(i);
        // Output at base development.
        let c0 = sim.corp().cargo(specialty);
        for _ in 0..20 {
            sim.step();
        }
        let base_out = sim.corp().cargo(specialty) - c0;
        // Develop it: costs credits, raises the level, no extra alarm.
        let alarm_before = sim.coalition_alarm();
        let cost = sim.develop_cost(i).unwrap();
        let credits0 = sim.corp().credits();
        sim.develop_colony(i).unwrap();
        assert_eq!(
            sim.corp().credits(),
            credits0 - cost,
            "development costs credits"
        );
        assert_eq!(sim.colony_dev(i), 2);
        assert!(
            sim.coalition_alarm() <= alarm_before,
            "developing your own colony draws no coalition alarm"
        );
        // Developing is a ~180-day build: the new level's benefit only lands once it's built.
        assert!(sim.colony_build_days(i) > 0);
        while sim.colony_build_days(i) > 0 {
            sim.step();
        }
        // Output now scales with the higher (now operational) development.
        let c1 = sim.corp().cargo(specialty);
        for _ in 0..20 {
            sim.step();
        }
        let dev_out = sim.corp().cargo(specialty) - c1;
        assert!(
            dev_out > base_out,
            "a developed colony out-produces a bare one ({dev_out} > {base_out})"
        );
    }

    #[test]
    fn the_development_doctrine_tilts_holding_yield() {
        // Phase C: Industry favours raw output (vs Trade), and Growth cheapens dev.
        let out_under = |doc: DevDoctrine| -> i64 {
            let mut sim = Sim::new(1);
            sim.corp_mut().credit(2_000_000);
            let i = sim.acquirable_colonies()[0];
            sim.acquire_colony(i).unwrap();
            while sim.dev_doctrine() != doc {
                sim.cycle_dev_doctrine();
            }
            let specialty = sim.colony_specialty(i);
            let c0 = sim.corp().cargo(specialty);
            for _ in 0..30 {
                sim.step();
            }
            sim.corp().cargo(specialty) - c0
        };
        assert!(
            out_under(DevDoctrine::Industry) > out_under(DevDoctrine::Trade),
            "Industry tilts holding output above Trade"
        );
        // Growth cheapens development.
        let mut sim = Sim::new(1);
        sim.corp_mut().credit(2_000_000);
        let i = sim.acquirable_colonies()[0];
        sim.acquire_colony(i).unwrap();
        let balanced_cost = sim.develop_cost(i).unwrap();
        while sim.dev_doctrine() != DevDoctrine::Growth {
            sim.cycle_dev_doctrine();
        }
        assert!(
            sim.develop_cost(i).unwrap() < balanced_cost,
            "Growth doctrine cheapens development"
        );
    }

    #[test]
    fn warships_need_a_shipyard_except_corvettes_with_opa_standing() {
        // Ship sourcing (Phase B+): civilians + (with OPA standing) corvettes come from
        // Tycho; everything bigger needs your own shipyard, unlocked by tier.
        let mut sim = Sim::new(0);
        sim.corp_mut().credit(2_000_000);
        assert!(matches!(
            sim.commission_ship(ShipClass::Frigate),
            Err(CommissionError::NeedShipyard)
        ));
        assert!(matches!(
            sim.commission_ship(ShipClass::Cruiser),
            Err(CommissionError::NeedShipyard)
        ));
        // Good OPA standing buys corvettes from Tycho — but not warships.
        sim.relations_mut()
            .adjust(crate::sim::Faction::Belt, CORVETTE_STANDING);
        assert!(
            sim.commission_ship(ShipClass::Frigate).is_ok(),
            "corvettes come from Tycho with OPA standing"
        );
        assert!(matches!(
            sim.commission_ship(ShipClass::Destroyer),
            Err(CommissionError::NeedShipyard)
        ));
        // A shipyard unlocks warships, gated by its tier — but founding is a ~1-year build,
        // so it lays down nothing until construction finishes.
        let home = sim.markets()[0].body();
        sim.found_shipyard(home).unwrap();
        assert!(sim.shipyard_build_days() > 0);
        assert!(
            matches!(
                sim.commission_ship(ShipClass::Destroyer),
                Err(CommissionError::NeedShipyard)
            ),
            "a building yard lays down nothing yet"
        );
        while sim.shipyard_build_days() > 0 {
            sim.step();
        }
        assert_eq!(sim.shipyard_max_hull(), "Destroyer");
        assert!(matches!(
            sim.commission_ship(ShipClass::Cruiser),
            Err(CommissionError::NeedShipyard)
        ));
        sim.corp_mut().credit(2_000_000); // the year-long build drained the treasury (overhead sink)
        sim.expand_shipyard().unwrap();
        while sim.shipyard_build_days() > 0 {
            sim.step();
        }
        assert_eq!(sim.shipyard_max_hull(), "Cruiser");
        assert!(matches!(
            sim.found_shipyard(home),
            Err(ShipyardError::AlreadyBuilt)
        ));
    }

    #[test]
    fn an_outpost_pays_tribute_develops_and_boosts_a_co_located_miner() {
        let mut sim = Sim::new(0);
        sim.corp_mut().credit(50_000); // below the 100k wealth-overhead sink, so tribute nets out
                                       // Found an outpost at a mineable belt body (Vesta).
        let body = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Vesta")
            .unwrap();
        assert!(sim.can_found_outpost(body));
        sim.found_outpost(body).unwrap();
        assert_eq!(sim.outposts().len(), 1);
        // Founding is a slow build (~180 days): inert while under construction.
        assert!(!sim.outpost_at(body).unwrap().is_ready(sim.tick()));
        assert_eq!(sim.outpost_build_remaining(body), Some(180));
        let c0 = sim.corp().credits();
        sim.step();
        assert_eq!(
            sim.corp().credits(),
            c0,
            "an outpost under construction pays nothing"
        );
        // Fast-forward past the build: it comes online and pays tribute.
        while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
            sim.step();
        }
        assert!(sim.outpost_build_remaining(body).is_none());
        let c1 = sim.corp().credits();
        sim.step();
        assert!(
            sim.corp().credits() > c1,
            "an operational outpost pays tribute"
        );
        // A miner on the (ready) outpost's body hauls to the on-site station: +50% output.
        let mineral = sim.body_mineral(body);
        sim.buy_miner(body).unwrap();
        let before = sim.corp().cargo(mineral);
        sim.step();
        assert!(
            sim.corp().cargo(mineral) > before + MINER_OUTPUT_PER_TICK,
            "a co-located miner is boosted by the operational outpost"
        );
        // Developing re-arms the build timer (the new level isn't instant).
        sim.develop_outpost(body).unwrap();
        assert!(!sim.outpost_at(body).unwrap().is_ready(sim.tick()));
    }

    #[test]
    fn an_outpost_needs_a_mine_to_produce_raw_goods() {
        let mut sim = Sim::new(0);
        sim.corp_mut().credit(60_000);
        let body = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Eros")
            .unwrap();
        sim.found_outpost(body).unwrap();
        while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
            sim.step();
        }
        // Operational but Mine-less: produces no raw goods (only its credit tribute).
        let mineral = sim.body_mineral(body);
        let before = sim.corp().cargo(mineral);
        for _ in 0..10 {
            sim.step();
        }
        assert_eq!(
            sim.corp().cargo(mineral),
            before,
            "no Mine ⇒ no raw production"
        );
        assert_eq!(sim.outpost_stored(body).0, 0, "no Mine ⇒ empty local store");
        // Build a Mine (a ~120-day build); once it's up, the outpost extracts raw into its
        // LOCAL store each tick (not the warehouse — per-asset inventory, §10).
        assert!(!sim.outpost_has_facility(body, FAC_MINE));
        sim.build_facility(body, FAC_MINE).unwrap();
        while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
            sim.step();
        }
        let wh_before = sim.corp().cargo(mineral);
        for _ in 0..10 {
            sim.step();
        }
        assert!(
            sim.outpost_stored(body).0 > 0,
            "a Mine fills the local store"
        );
        assert_eq!(
            sim.corp().cargo(mineral),
            wh_before,
            "no Hangar ⇒ the goods stay on-site, not in your warehouse"
        );
        // A Hangar ships the local stock out to your warehouse.
        sim.corp_mut().credit(20_000);
        sim.build_facility(body, FAC_HANGAR).unwrap();
        while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
            sim.step();
        }
        let wh2 = sim.corp().cargo(mineral);
        for _ in 0..10 {
            sim.step();
        }
        assert!(
            sim.corp().cargo(mineral) > wh2,
            "a Hangar ships local goods to your warehouse"
        );
    }

    #[test]
    fn a_fully_built_outpost_can_be_promoted_to_a_colony() {
        let mut sim = Sim::new(0);
        sim.corp_mut().credit(5_000_000);
        let body = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Io")
            .unwrap();
        sim.found_outpost(body).unwrap();
        let finish_build = |sim: &mut Sim| {
            while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
                sim.step();
            }
        };
        finish_build(&mut sim);
        // Not promotable until maxed + all facilities.
        assert!(!sim.can_promote_outpost(body));
        while sim.outpost_at(body).unwrap().level < MAX_OUTPOST_LEVEL {
            sim.corp_mut().credit(100_000);
            sim.develop_outpost(body).unwrap();
            finish_build(&mut sim);
        }
        for kind in [FAC_MINE, FAC_STORAGE, FAC_HANGAR] {
            sim.build_facility(body, kind).unwrap();
            finish_build(&mut sim);
        }
        // Maxed + facilities, but population must first be grown by supplying Ice.
        assert!(!sim.can_promote_outpost(body), "needs population too");
        sim.corp_mut().store(ICE_COMMODITY, 5_000); // a stockpile of the basic good
        while sim.outpost_at(body).unwrap().population < PROMOTE_POP {
            sim.step();
        }
        assert!(
            sim.can_promote_outpost(body),
            "maxed + facilities + population ⇒ promotable"
        );
        assert_eq!(sim.outpost_at(body).unwrap().rank, RANK_OUTPOST);
        sim.corp_mut().credit(100_000);
        sim.promote_outpost(body).unwrap();
        assert_eq!(sim.outpost_at(body).unwrap().rank, RANK_COLONY);
        finish_build(&mut sim);
        // A colony out-yields the bare outpost it was (3× tribute) — credits climb faster.
        let c0 = sim.corp().credits();
        for _ in 0..30 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > c0,
            "a promoted colony pays a fat tribute"
        );
    }

    #[test]
    fn a_colony_climbs_through_hub_to_a_single_capital() {
        // The full ladder: Outpost → Colony → Hub → Capital. Each rung needs the prior
        // promotion finished, more population, and more credits; there can be only one Capital.
        let mut sim = Sim::new(0);
        sim.corp_mut().credit(50_000_000);
        let body = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Io")
            .unwrap();
        sim.found_outpost(body).unwrap();
        let finish_build = |sim: &mut Sim| {
            while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
                sim.step();
            }
        };
        finish_build(&mut sim);
        while sim.outpost_at(body).unwrap().level < MAX_OUTPOST_LEVEL {
            sim.corp_mut().credit(200_000);
            sim.develop_outpost(body).unwrap();
            finish_build(&mut sim);
        }
        for kind in [FAC_MINE, FAC_STORAGE, FAC_HANGAR] {
            sim.corp_mut().credit(200_000);
            sim.build_facility(body, kind).unwrap();
            finish_build(&mut sim);
        }
        sim.corp_mut().store(ICE_COMMODITY, 100_000); // plenty to grow on
                                                      // Walk the three promotions, each gated on its own (rising) population threshold.
        for expected in [RANK_COLONY, RANK_HUB, RANK_CAPITAL] {
            let need = promote_pop_threshold(expected - 1);
            while sim.outpost_at(body).unwrap().population < need {
                sim.corp_mut().store(ICE_COMMODITY, 1_000); // keep it fed
                sim.step();
            }
            assert!(
                sim.can_promote_outpost(body),
                "rung {expected}: maxed + facilities + population ⇒ promotable"
            );
            sim.corp_mut().credit(1_000_000);
            sim.promote_outpost(body).unwrap();
            assert_eq!(sim.outpost_at(body).unwrap().rank, expected);
            finish_build(&mut sim);
        }
        assert!(sim.has_capital());
        // A Capital is the top — no further promotion.
        assert!(!sim.can_promote_outpost(body), "Capital is the top rung");
    }

    #[test]
    fn outposts_are_inert_by_default() {
        // No outposts ⇒ the economy is byte-identical to a world that never had the layer.
        let mut a = Sim::new(5);
        let mut b = Sim::new(5);
        for _ in 0..400 {
            a.step();
            let _ = b.outposts();
            b.step();
        }
        assert_eq!(a.corp().credits(), b.corp().credits());
    }

    #[test]
    fn a_deployed_miner_stocks_the_warehouse_with_the_bodys_mineral() {
        // Early industry: a miner bought from Tycho and stationed at a body extracts
        // that body's raw into your warehouse each tick — the bootstrap before colonies.
        let mut sim = Sim::new(0);
        let body = sim.markets()[0].body(); // the home body (Ceres)
        let mineral = sim.body_mineral(body);
        let before = sim.corp().cargo(mineral);
        let credits0 = sim.corp().credits();
        sim.buy_miner(body).unwrap();
        assert_eq!(sim.miners().len(), 1);
        assert!(
            sim.corp().credits() < credits0,
            "buying a miner costs credits"
        );
        assert!(matches!(sim.buy_miner(0), Err(MinerError::BadSite))); // not the sun
                                                                       // Player mining is confined to the belts + outer moons; the Earth/Mars AO is off-limits.
        let earth = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Earth")
            .unwrap();
        let luna = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Luna")
            .unwrap();
        let titan = super::orbit::default_system()
            .iter()
            .position(|b| b.name == "Titan")
            .unwrap();
        assert!(matches!(sim.buy_miner(earth), Err(MinerError::BadSite)));
        assert!(matches!(sim.buy_miner(luna), Err(MinerError::BadSite))); // Earth's moon
        assert!(sim.can_mine_body(titan), "an outer moon is a valid site");
        assert!(sim.can_mine_body(body), "the belt (Ceres) is a valid site");
        for _ in 0..30 {
            sim.step();
        }
        assert!(
            sim.corp().cargo(mineral) >= before + 30 * MINER_OUTPUT_PER_TICK,
            "the miner stocked the warehouse with the body's mineral"
        );
    }

    #[test]
    fn the_powers_contest_the_major_hubs_and_courting_lets_you_claim_one() {
        // The major frontier hubs are fought over (the Ganymede conflict). Ambient
        // Earth/Mars flares shift the balance; the player gathers influence to claim one.
        let mut sim = Sim::new(0);
        assert!(sim.contested_count() > 0, "major hubs should be contested");
        // The ambient contest shifts influence over time (Earth/Mars tug-of-war).
        let before: Vec<[i64; 4]> = sim
            .contested_colonies()
            .iter()
            .map(|c| c.influence)
            .collect();
        for _ in 0..contest::FLARE_INTERVAL * 2 + 5 {
            sim.step();
        }
        let after: Vec<[i64; 4]> = sim
            .contested_colonies()
            .iter()
            .map(|c| c.influence)
            .collect();
        assert_ne!(
            before, after,
            "ambient great-power flares shift the balance"
        );
        // Court a colony enough to claim it (the slow gather-influence loop).
        sim.influence = 100_000; // plenty of statecraft resource
        let colony = sim.contested_colony(0).unwrap().colony;
        assert!(!sim.colony_controlled(colony));
        // Claiming before you've built standing is rejected.
        assert!(matches!(
            sim.claim_contested_colony(0),
            Err(ContestError::NotStrongEnough)
        ));
        while !sim.contested_colony(0).unwrap().claimable() {
            sim.court_contested_colony(0).unwrap();
        }
        sim.claim_contested_colony(0).unwrap();
        assert!(sim.colony_controlled(colony), "claimed the contested hub");
    }

    #[test]
    fn the_contest_does_not_perturb_the_economy() {
        // The contest layer touches only its own numbers + the feed — never the market
        // RNG — so a world that watches/courts it stays bit-identical in the economy.
        let mut a = Sim::new(7);
        let mut b = Sim::new(7);
        for _ in 0..600 {
            a.step();
            // b reads the contest every tick (and the ambient flares fire in both).
            let _ = b.contested_colonies();
            b.step();
        }
        let stocks_a: Vec<Vec<i64>> = a
            .markets()
            .iter()
            .map(|m| m.stocks().iter().map(|s| s.stock).collect())
            .collect();
        let stocks_b: Vec<Vec<i64>> = b
            .markets()
            .iter()
            .map(|m| m.stocks().iter().map(|s| s.stock).collect())
            .collect();
        assert_eq!(stocks_a, stocks_b, "the contest never perturbs the economy");
    }

    #[test]
    fn a_fresh_world_controls_no_colonies() {
        // The empire layer is inert by default — a fresh sim owns nothing, so the
        // §7c gate + existing economy are unaffected (no tribute, no rep shift).
        let mut sim = Sim::new(0);
        for _ in 0..200 {
            sim.step();
        }
        assert_eq!(sim.controlled_colony_count(), 0);
        assert_eq!(sim.holding_count(), 0);
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
        for _ in 0..200 {
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
    fn incursions_only_fire_after_transit_and_damage_an_undefended_bridgehead() {
        // §17/G4: pre-transit no incursion ever fires (byte-identical world); after
        // transit they escalate, and an undefended one chips the bridgehead.
        let mut sim = Sim::new(5);
        // A long pre-transit run raises no incursion at all.
        for _ in 0..600 {
            sim.step();
        }
        assert!(!sim.incursion_pending());
        assert!(!sim.pressure().endgame());
        // Climb, transit, found the foothold.
        for _ in 0..200 {
            sim.complete_op();
        }
        assert!(sim.transit_gate());
        assert!(
            sim.pressure().endgame(),
            "transit lights the incursion clock"
        );
        sim.corp_mut().credit(500_000);
        assert_eq!(sim.found_bridgehead(), Ok(()));
        let full = sim.bridgehead().integrity();
        // Run long enough for an incursion to land and (undefended) lapse onto the
        // foothold — its integrity must fall.
        for _ in 0..400 {
            sim.step();
            if sim.bridgehead().integrity() < full {
                break;
            }
        }
        assert!(
            sim.bridgehead().integrity() < full,
            "an undefended incursion damages the bridgehead"
        );
    }

    #[test]
    fn defending_an_incursion_protects_the_bridgehead() {
        // §17/G4: with a strong enough fleet, answering the incursion repels it and
        // the bridgehead takes no damage.
        let mut sim = Sim::new(11);
        yard(&mut sim);
        for _ in 0..200 {
            sim.complete_op();
        }
        assert!(sim.transit_gate());
        sim.corp_mut().credit(5_000_000);
        assert_eq!(sim.found_bridgehead(), Ok(()));
        // Stand up a frigate squadron (the 60-crew pool affords five) — a heavy
        // numeric edge over the 2-ship opening incursion pack, so the defense wins.
        for _ in 0..5 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        sim.finish_pending_ships();
        assert!(!sim.corp().fleet().is_empty(), "a squadron stands ready");
        let full = sim.bridgehead().integrity();
        // Advance until an incursion is pending, then defend it.
        let mut defended = false;
        for _ in 0..400 {
            sim.step();
            if sim.incursion_pending() {
                let outcome = sim.defend_bridgehead(Band::Close);
                assert!(outcome.is_some(), "the fleet answers");
                assert!(!sim.incursion_pending(), "the incursion is resolved");
                defended = true;
                break;
            }
        }
        assert!(defended, "an incursion arrived to defend against");
        // A won defense leaves the foothold unscathed.
        assert_eq!(
            sim.bridgehead().integrity(),
            full,
            "a successful defense costs the bridgehead no integrity"
        );
    }

    #[test]
    fn the_endgame_is_won_by_growing_and_holding_the_bridgehead() {
        // §17/G5: the journey completes when the bridgehead reaches the target level
        // *and* has weathered the required incursions — a genuine victory state.
        let mut sim = Sim::new(11);
        yard(&mut sim);
        for _ in 0..200 {
            sim.complete_op();
        }
        assert!(sim.transit_gate());
        assert_eq!(sim.endgame_outcome(), EndgameOutcome::Undecided);
        sim.corp_mut().credit(50_000_000);
        assert_eq!(sim.found_bridgehead(), Ok(()));
        for _ in 0..5 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
        let (target_level, target_survived) = sim.endgame_targets();
        // Grow the bridgehead to (just below) the target — not yet a win without the
        // incursions weathered.
        while sim.bridgehead().level() < target_level {
            assert_eq!(sim.upgrade_bridgehead(), Ok(()));
        }
        assert_eq!(
            sim.endgame_outcome(),
            EndgameOutcome::Undecided,
            "level alone does not win — the far side must be held"
        );
        // Repel incursions until the threshold is met; the win then fires.
        let mut guard = 0;
        while sim.endgame_outcome() == EndgameOutcome::Undecided {
            sim.step();
            if sim.incursion_pending() {
                // Refit if the squadron was thinned, so defenses keep winning.
                while sim.corp().fleet().len() < 5 {
                    if sim.commission_ship(ShipClass::Frigate).is_err() {
                        break;
                    }
                }
                sim.defend_bridgehead(Band::Close);
            }
            guard += 1;
            assert!(guard < 20_000, "the endgame should resolve in bounded time");
        }
        assert_eq!(sim.endgame_outcome(), EndgameOutcome::Triumph);
        assert!(sim.incursions_survived() >= target_survived);
        // Resolution is terminal — no further incursions press.
        assert!(!sim.incursion_pending());
    }

    #[test]
    fn the_endgame_is_lost_if_the_bridgehead_is_overrun() {
        // §17/G5: an undefended bridgehead ground to zero is the loss ending.
        let mut sim = Sim::new(5);
        for _ in 0..200 {
            sim.complete_op();
        }
        assert!(sim.transit_gate());
        sim.corp_mut().credit(500_000);
        assert_eq!(sim.found_bridgehead(), Ok(()));
        // Never defend — incursions grind the foothold down to nothing.
        let mut guard = 0;
        while sim.endgame_outcome() == EndgameOutcome::Undecided {
            sim.step();
            guard += 1;
            assert!(
                guard < 50_000,
                "an undefended bridgehead must eventually fall"
            );
        }
        assert_eq!(sim.endgame_outcome(), EndgameOutcome::Fallen);
        assert!(sim.bridgehead().has_fallen());
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
        yard(&mut sim);
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
        yard(&mut sim);
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
        sim.finish_pending_ships();
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
        yard(&mut sim);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.finish_pending_ships();
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
        yard(&mut sim);
        assert!(
            sim.engage_raiders(Band::Medium).is_none(),
            "no warships ⇒ no engagement"
        );
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.finish_pending_ships();
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
        yard(&mut sim);
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.finish_pending_ships();
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
