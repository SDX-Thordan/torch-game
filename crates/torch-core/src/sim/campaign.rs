//! The retention spine (§0) — the destination pull made tangible.
//!
//! TORCH is "a journey toward a destination" (§0.1): a foreshadowed ring-gate
//! pulls the player up through **tiers of scale** (§0.3), and at any moment the
//! player can name a *now* goal, a *tier* goal, and the *far* goal (the gate) —
//! the three-horizon stack (§0.4). This module holds that progression as
//! deterministic sim state (§27); player operations advance it, tier ascents are
//! milestones, and the gate's approach is always visible.

/// The tiers of play (§0.3) — each a different *kind* of game, not just bigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Station,
    Region,
    Sol,
    Gate,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Station => "The Station",
            Tier::Region => "The Region",
            Tier::Sol => "Sol & the Cold War",
            Tier::Gate => "The Gate",
        }
    }

    fn from_index(i: usize) -> Tier {
        match i {
            0 => Tier::Station,
            1 => Tier::Region,
            2 => Tier::Sol,
            _ => Tier::Gate,
        }
    }

    /// Player operations needed to climb to the next tier, or `None` at the top.
    fn ops_to_advance(self) -> Option<i64> {
        match self {
            Tier::Station => Some(3),
            Tier::Region => Some(10),
            Tier::Sol => Some(25),
            Tier::Gate => None,
        }
    }

    /// The current objective shown for this tier (the authored thread, §16).
    pub fn objective(self) -> &'static str {
        match self {
            Tier::Station => "Disrupt the lanes and prove the operation.",
            Tier::Region => "Extend your network across the Belt.",
            Tier::Sol => "Turn the great powers against each other.",
            Tier::Gate => "The ring-gate is open — seize the frontier.",
        }
    }
}

/// Basis-point denominator.
const BP: i64 = 10_000;
/// Tiers above the start, used to scale gate progress to `[0, BP]`.
const ASCENTS: i64 = 3;

/// The player's position on the climb (§0). Cheap, legible, always-visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Campaign {
    tier_index: usize,
    ops_in_tier: i64,
}

impl Campaign {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tier(&self) -> Tier {
        Tier::from_index(self.tier_index)
    }

    /// The *now* goal: the current objective and progress toward the next rung
    /// (`(text, progress, target)`; target 0 means "summit reached").
    pub fn now_goal(&self) -> (&'static str, i64, i64) {
        let target = self.tier().ops_to_advance().unwrap_or(0);
        (
            self.tier().objective(),
            self.ops_in_tier.min(target.max(0)),
            target,
        )
    }

    /// The *far* goal: how close the ring-gate is to opening, in basis points —
    /// foreshadowed from minute one (§0.1).
    pub fn gate_progress_bp(&self) -> i64 {
        self.tier_index as i64 * BP / ASCENTS
    }

    /// Whether the gate has opened (the summit of the MVP climb).
    pub fn gate_open(&self) -> bool {
        self.tier() == Tier::Gate
    }

    /// Record a completed player operation. Returns the new tier's name if this
    /// op triggered an ascent (the arrival fanfare, §0.3).
    pub fn record_op(&mut self) -> Option<&'static str> {
        self.ops_in_tier += 1;
        if let Some(target) = self.tier().ops_to_advance() {
            if self.ops_in_tier >= target {
                self.tier_index += 1;
                self.ops_in_tier = 0;
                return Some(self.tier().name());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_the_station_with_the_gate_far_off() {
        let c = Campaign::new();
        assert_eq!(c.tier(), Tier::Station);
        assert_eq!(c.gate_progress_bp(), 0);
        assert!(!c.gate_open());
        let (_, progress, target) = c.now_goal();
        assert_eq!((progress, target), (0, 3));
    }

    #[test]
    fn operations_climb_the_tier_and_advance_the_gate() {
        let mut c = Campaign::new();
        assert_eq!(c.record_op(), None);
        assert_eq!(c.record_op(), None);
        // Third op completes Station → ascend to Region (the fanfare).
        assert_eq!(c.record_op(), Some("The Region"));
        assert_eq!(c.tier(), Tier::Region);
        assert!(c.gate_progress_bp() > 0);
        // Progress resets within the new tier.
        assert_eq!(c.now_goal().1, 0);
    }

    #[test]
    fn the_full_climb_opens_the_gate() {
        let mut c = Campaign::new();
        // 3 + 10 + 25 ops to climb Station → Region → Sol → Gate.
        for _ in 0..(3 + 10 + 25) {
            c.record_op();
        }
        assert_eq!(c.tier(), Tier::Gate);
        assert!(c.gate_open());
        assert_eq!(c.gate_progress_bp(), BP);
        assert_eq!(c.record_op(), None); // no rung above the summit
    }
}
