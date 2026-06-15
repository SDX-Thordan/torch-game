//! Managers & automation (§12) — run the company by exception.
//!
//! The player sets **policy**; managers execute it autonomously each tick. This
//! removes tedium, not agency: the verbs (§0.4) still run, just on standing
//! orders, and their consequences surface through the alert feed (§19). Here we
//! hold the policy; the `Sim` runs it. Deterministic config (§27).

use super::faction::Faction;
use super::interdiction::Interceptor;

/// Standing orders for the automated interdiction patrol (§7b under automation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InterdictionPolicy {
    /// Whether the patrol is hunting at all.
    pub enabled: bool,
    /// Only strike this faction's shipping; `None` means any target.
    pub target: Option<Faction>,
    /// Ignore cargoes smaller than this (don't waste the sortie).
    pub min_cargo: i64,
}

/// The full policy a CEO leaves their managers (§12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomationPolicy {
    pub interdiction: InterdictionPolicy,
    /// Auto-invest research points in the cheapest available tech (§10).
    pub auto_research: bool,
    /// The standing patrol asset the interdiction manager flies.
    pub patrol: Interceptor,
}

impl Default for AutomationPolicy {
    fn default() -> Self {
        Self {
            interdiction: InterdictionPolicy::default(),
            auto_research: false,
            // A fast inner-system picket that can reach most lanes.
            patrol: Interceptor {
                pos: (0, 0),
                speed: 120_000,
                skill_bp: 1_500,
            },
        }
    }
}
