//! Player stations & the Produce standing order (§3.1 / §4 of the interaction
//! model) — the Tier-1 base and the value-add half of the core loop (§5).
//!
//! A `Station` runs a **Produce** preset hands-off: source its input commodity
//! from a market, transform it into a higher-value output (raw → refined), and
//! auto-sell the surplus above a threshold into a market. The player tunes the
//! recipe + thresholds; the deterministic sim runs it; the orchestration lives on
//! `Sim` (which owns the markets and treasury). Integer/deterministic (§27).

/// A player-owned production station running a Produce standing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Station {
    /// Body it sits at (for the orrery).
    pub body: usize,
    /// Commodity consumed (the input recipe).
    pub input: usize,
    /// Commodity produced (the value-add output).
    pub output: usize,
    /// Units transformed per tick.
    pub rate: i64,
    /// Market the input is sourced from.
    pub buy_market: usize,
    /// Market the surplus output is sold into.
    pub sell_market: usize,
    /// Hold this much output before selling the surplus (the sell-surplus rule).
    pub sell_above: i64,
    /// Throttle: stop producing once output stock reaches this (input priority).
    pub output_target: i64,
}
