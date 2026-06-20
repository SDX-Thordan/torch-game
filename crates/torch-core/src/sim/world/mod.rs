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

// Sim behaviour, split by theme; each file adds an `impl Sim` block.
mod automation;
mod defence;
mod empire;
mod endgame;
mod fleet;
mod industry;
mod mining;
mod outposts;
mod persist;
mod shipyard;
mod tick;
mod trade;

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
/// Base magnitude of a telegraphed raid threat (§13), and the extra magnitude per
/// point of standing piracy pressure — together they size the stakes the player
/// weighs when a `RaidThreat` dilemma surfaces.
const RAID_MAG_BASE: i64 = 1_500;
const RAID_MAG_PER_PIRACY: i64 = 30;

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
/// A mining ship's **class** (§8e) — the dedicated extractor now comes in tiers, each a
/// pricier, crew-heavier, higher-yield asset. The base **Prospector** is the cheap first
/// step (byte-identical to the old single miner); the **Harvester** and **Refinery Barge**
/// are deliberate mid-game investments — every hull is a costly asset that gates expansion.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MinerClass {
    #[default]
    Prospector,
    Harvester,
    RefineryBarge,
}

impl MinerClass {
    /// Output multiplier over the base extraction rate.
    pub fn yield_mult(self) -> i64 {
        match self {
            MinerClass::Prospector => 1,
            MinerClass::Harvester => 3,
            MinerClass::RefineryBarge => 6,
        }
    }
    /// Purchase price — the Prospector keeps the original [`MINER_COST`].
    pub fn cost(self) -> i64 {
        match self {
            MinerClass::Prospector => MINER_COST,
            MinerClass::Harvester => 30_000,
            MinerClass::RefineryBarge => 75_000,
        }
    }
    /// Trained crew the hull ties up — the §8c bottleneck as the real gate (the Prospector
    /// is crewless to keep the early first-move byte-identical; the bigger rigs cost crew).
    pub fn crew(self) -> i64 {
        match self {
            MinerClass::Prospector => 0,
            MinerClass::Harvester => 10,
            MinerClass::RefineryBarge => 24,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            MinerClass::Prospector => "Prospector",
            MinerClass::Harvester => "Harvester",
            MinerClass::RefineryBarge => "Refinery Barge",
        }
    }
    /// 0/1/2 round-trip for the shell.
    pub fn from_index(i: i64) -> MinerClass {
        match i {
            1 => MinerClass::Harvester,
            2 => MinerClass::RefineryBarge,
            _ => MinerClass::Prospector,
        }
    }
}

/// Evocative call-signs for mining rigs, assigned by deployment order (deterministic — no
/// RNG draw, so buying a miner never perturbs the shared market RNG, §27).
const MINER_NAMES: [&str; 12] = [
    "Pallas Pick",
    "Dusty Maru",
    "Ceres Mole",
    "Vesta Digger",
    "Rock Hopper",
    "Deep Seam",
    "Ironside",
    "Coreshaper",
    "Slag Hauler",
    "Gritwork",
    "Lodebreaker",
    "Tailings Joy",
];

/// A **convoy** — a named group the player forms over their civilian ships (miners + haulers,
/// later escorted by warships, Phase 5). Its point now: a miner grouped with a hauler in the
/// same convoy works **more efficiently** — the hauler ferries its ore so it never stops to run
/// it home. Members carry the convoy's stable `id` (so removing a ship just drops it).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Convoy {
    pub id: u32,
    pub name: String,
    /// Warships assigned to escort this convoy (Phase 5) — an actively-screened convoy deters
    /// piracy better. Drawn from the fleet; 0 for old saves / an unescorted convoy.
    #[serde(default)]
    pub escorts: u8,
}

/// A deployed **mining ship** (§3.1) — a dedicated, named, tiered hull stationed at a body,
/// extracting its raw mineral into your warehouse each tick. The early industrial step.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Miner {
    pub body: usize,
    pub commodity: usize,
    /// The rig's class/tier — scales yield, cost, and crew. Defaults to Prospector so old
    /// saves (and the byte-identical first move) read as the original single miner.
    #[serde(default)]
    pub class: MinerClass,
    /// Christened call-sign (e.g. "Pallas Pick"); empty for old saves.
    #[serde(default)]
    pub name: String,
    /// Tick it entered service (§14 service history); 0 for old saves.
    #[serde(default)]
    pub commissioned_tick: u64,
    /// The convoy this rig belongs to (stable id), if any — a miner convoyed with a hauler
    /// gets the Phase 4 synergy. `None` for old saves / a lone rig.
    #[serde(default)]
    pub convoy: Option<u32>,
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
    /// Not enough trained crew for this rig's class (the bigger tiers need crew).
    NoCrew,
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
    /// A **collector hauler** is dedicated to this outpost (§10) — a freighter drawn from your
    /// pool that ferries the local store to the warehouse (the alternative to building a Hangar;
    /// a collecting hauler can't also run a trade route). `false` for old saves / no collector.
    #[serde(default)]
    pub collector: bool,
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
/// Output bonus (basis points) for a miner convoyed with a hauler (Phase 4 synergy) — the
/// hauler ferries its ore so the rig mines continuously instead of stopping to run it home.
const CONVOY_SYNERGY_BP: i64 = 5_000;
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
/// A dedicated **collector hauler** ferries this many units/tick from an outpost store to the
/// warehouse (§10) — the freighter alternative to a Hangar, at the cost of a route-pool slot.
const COLLECTOR_SHIP_PER_TICK: i64 = 6;
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
/// Armed-hauler self-defense weight (PDC-equivalents) that equals one warship escort —
/// a screen of armed merchantmen substitutes for some of the navy (EP3).
const HAULER_DEFENSE_PER_ESCORT: i64 = 4;
/// Credit cost to bolt a PDC / Ramshackle torpedo onto a hauler.
const HAULER_PDC_COST: i64 = 4_000;
const HAULER_TORPEDO_COST: i64 = 6_000;

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
    /// Player-formed convoys (Phase 4) — named groups over the civilian fleet; a miner convoyed
    /// with a hauler works more efficiently. Empty by default (inert / byte-identical).
    convoys: Vec<Convoy>,
    /// Monotonic id for the next convoy (so member refs stay stable across removals).
    next_convoy_id: u32,
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
            convoys: Vec::new(),
            next_convoy_id: 1,
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
mod tests;
