//! The typed event stream (§29) — a BattleLog-style record of *what happened*
//! this tick, consumed by the combat diorama (§22) and the alert feed (§19).
//!
//! This starts minimal; economy, traffic, and combat variants are added as
//! those systems come online. Keeping it an explicit enum (not stringly-typed)
//! lets the view and tests match exhaustively.

/// One thing that happened during a tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The simulation advanced to `tick`.
    Tick { tick: u64 },
}
