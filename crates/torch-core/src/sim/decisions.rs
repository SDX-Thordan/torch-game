//! Player-facing **dilemmas** (Phase A / §0.4): an act-now exception is not a single
//! button but a small set of options that trade off across credits, reputation, and
//! risk — so answering the feed is a *decision*, not a reflex. The world raises the
//! exception (a shortage, later a wreck or a raid); the player chooses how to answer.
//!
//! Decisions are **transient** (like the act-now alerts they shadow): they expire if
//! unanswered and are not persisted — a reload re-derives them from the live world.

/// What kind of exception a decision answers. (Shortage today; wreck/raid to come.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionKind {
    /// A market is short of a commodity (price spiked): speculate / gouge / relieve.
    Shortage,
}

/// A pending dilemma the player can answer until `deadline_tick`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub id: u64,
    pub kind: DecisionKind,
    pub market: usize,
    pub commodity: usize,
    pub deadline_tick: u64,
}

/// One choosable option, with the numbers the shell shows so the player can weigh it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionOption {
    pub label: &'static str,
    /// A one-line benefit/risk summary with live numbers.
    pub summary: String,
    /// Estimated net credits (+gain / −cost) — an estimate; risk may change the result.
    pub est_credits: i64,
    /// Reputation effect on the affected faction (+favour / −resentment).
    pub rep_delta: i64,
    /// Whether the option carries an uncertain (rolled) downside.
    pub risky: bool,
}

/// The outcome of resolving a decision, for shell feedback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionOutcome {
    pub credits: i64,
    pub rep_delta: i64,
    /// A risky option's downside fired (e.g. a profiteering fine).
    pub backfired: bool,
    pub message: String,
}

/// How long a dilemma stays answerable (ticks) — matches the act-now alert TTL.
pub const DECISION_TTL: u64 = 72;
/// The most dilemmas pending at once (a small, focused menu — no backlog anxiety).
pub const MAX_DECISIONS: usize = 3;
/// The standard deal size a dilemma trades.
pub const DEAL_QTY: i64 = 20;
