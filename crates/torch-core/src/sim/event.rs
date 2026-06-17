//! The typed event stream (§29) — a BattleLog-style record of *what happened*
//! this tick, consumed by the combat diorama (§22) and the alert feed (§19).
//!
//! This starts minimal; economy, traffic, and combat variants are added as
//! those systems come online. Keeping it an explicit enum (not stringly-typed)
//! lets the view and tests match exhaustively.

use super::pressure::PressureKind;

/// One thing that happened during a tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The simulation advanced to `tick`.
    Tick { tick: u64 },
    /// An arbitrage hauler set out from `origin` to `dest` (§7b).
    HaulerDeparted {
        id: u64,
        commodity: usize,
        origin: usize,
        dest: usize,
        qty: i64,
    },
    /// A hauler delivered its cargo, damping the spread.
    HaulerArrived { id: u64 },
    /// A hauler was cut in flight; its delivery is denied (§7b interdiction).
    HaulerInterdicted { id: u64 },
    /// A denied delivery left a market short of a commodity (§7b consequence).
    Scarcity { market: usize, commodity: usize },
    /// The company climbed to a new tier (§0.3 arrival fanfare).
    TierAscended { tier: &'static str },
    /// The player **transited the ring-gate** into the endgame (§0.1/§17) — the
    /// climactic payoff of the whole climb.
    GateTransited,
    /// The player fleet fought a raider pack and the battle resolved (§9). `won`
    /// is whether the player held the field; `losses` is player ships destroyed.
    BattleResolved { won: bool, losses: usize },
    /// An incoming threat is telegraphed `eta` ticks ahead (§13 forecasting), so
    /// nothing arrives unforeseeable — the player can pre-position or divert.
    ThreatForecast { kind: PressureKind, eta: u64 },
    /// A derelict was sighted, ripe to strip (§15 discovery & wonder).
    WreckSighted { id: u64 },
    /// A wreck was stripped for its reward (§15).
    WreckSalvaged { id: u64 },
    /// The player founded their far-side bridgehead (§17 endgame, G3) — the first
    /// foothold beyond the ring.
    BridgeheadFounded,
    /// The player upgraded the far-side bridgehead to `level` (§17, G3).
    BridgeheadUpgraded { level: u32 },
    /// An incursion from beyond the ring has reached the bridgehead (§17, G4) —
    /// act-now: defend it or it takes `severity` damage. The GATE_ANSWER payoff.
    IncursionStruck { severity: i64 },
    /// The far-side bridgehead took incursion damage (§17, G4); `integrity` is what
    /// remains.
    BridgeheadDamaged { integrity: i64 },
    /// The far-side bridgehead was overrun (§17, G5) — the endgame loss beat.
    BridgeheadFell,
    /// The endgame resolved in **triumph** (§17, G5) — the bridgehead was grown and
    /// held; you own the far side. The journey's end.
    EndgameWon,
    /// The endgame resolved in **defeat** (§17, G5) — the far side is lost.
    EndgameLost,
    /// The player took control of a frontier colony (the empire layer) — `colony`
    /// indexes `frontier::default_colonies`.
    ColonyAcquired { colony: usize },
    /// A great-power **coalition** is striking the player's holdings (the empire
    /// layer, E3) — act-now: defend, or lose a holding. `strength` scales the pack.
    CoalitionStrike { strength: i64 },
    /// The coalition seized one of the player's holdings (E3) — `colony` was
    /// liberated back to the Independents (or, if none, reparations were exacted).
    HoldingLost { colony: usize },
    /// Pirates raided the player's trade empire (EP3) — `loss` credits of cargo were
    /// taken because escorts on station were too thin to cover the holdings.
    EmpireRaided { loss: i64 },
    /// A soured great power inspected the player's shipping and levied a `fine` (EP4)
    /// — political enforcement, countered by repairing the relationship.
    Inspected { fine: i64 },
}
