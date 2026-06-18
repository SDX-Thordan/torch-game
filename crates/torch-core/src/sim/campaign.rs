//! The retention spine (§0) — the destination pull made tangible.
//!
//! TORCH is "a journey toward a destination" (§0.1): a foreshadowed ring-gate
//! pulls the player up through **tiers of scale** (§0.3), and at any moment the
//! player can name a *now* goal, a *tier* goal, and the *far* goal (the gate) —
//! the three-horizon stack (§0.4). This module holds that progression as
//! deterministic sim state (§27); player operations advance it, tier ascents are
//! milestones, and the gate's approach is always visible.

/// The tiers of play (§0.3) — each a different *kind* of game, not just bigger.
/// `Beyond` is the post-gate endgame (§17), reached by *transiting* the open ring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Station,
    Region,
    Sol,
    Gate,
    Beyond,
}

impl Tier {
    pub fn name(self) -> &'static str {
        match self {
            Tier::Station => "The Station",
            Tier::Region => "The Region",
            Tier::Sol => "Sol & the Cold War",
            Tier::Gate => "The Gate",
            Tier::Beyond => "Beyond the Gate",
        }
    }

    fn from_index(i: usize) -> Tier {
        match i {
            0 => Tier::Station,
            1 => Tier::Region,
            2 => Tier::Sol,
            3 => Tier::Gate,
            _ => Tier::Beyond,
        }
    }

    /// Player operations needed to climb to the next tier, or `None` at a summit.
    /// The Gate is **not** auto-advanced — crossing it is the deliberate
    /// [`transit`](Campaign::transit) verb, the payoff of the whole climb (§0.1).
    fn ops_to_advance(self) -> Option<i64> {
        match self {
            Tier::Station => Some(3),
            Tier::Region => Some(10),
            Tier::Sol => Some(25),
            Tier::Gate | Tier::Beyond => None,
        }
    }

    /// The current objective shown for this tier (the authored thread, §16).
    pub fn objective(self) -> &'static str {
        match self {
            Tier::Station => "Disrupt the lanes and prove the operation.",
            Tier::Region => "Extend your network across the Belt.",
            Tier::Sol => "Turn the great powers against each other.",
            Tier::Gate => "The ring is open. When you are ready — transit the gate.",
            Tier::Beyond => "You are through. Hold the bridgehead on the far side.",
        }
    }

    /// The "this is now a different *kind* of game" briefing voiced on arrival
    /// (§0.3): names the new signature activity so the player can reframe.
    pub fn briefing(self) -> &'static str {
        match self {
            Tier::Station => {
                "Tier 1 — The Station. One operation to keep alive and profitable. Learn the verbs."
            }
            Tier::Region => {
                "Tier 2 — The Region. Build out a logistics network across the Belt — and meet your first predators."
            }
            Tier::Sol => {
                "Tier 3 — Sol & the Cold War. The whole system and its politics. Play the powers against each other and earn dominance."
            }
            Tier::Gate => {
                "Tier 4 — The Gate. The ring is open and counting. Everything you built was to reach this. Transit when you are ready — there is no coming back the same."
            }
            Tier::Beyond => {
                "Tier 5 — Beyond the Gate. A new sky, a new economy, and whatever was counting on the far side. The larger game begins here."
            }
        }
    }

    /// How many production stations the player may run at this tier (§0.3:
    /// infrastructure *grows* as you climb — Tier 1 stays the baseline, higher
    /// tiers unlock a wider network). Monotonically non-decreasing.
    pub fn station_cap(self) -> usize {
        match self {
            Tier::Station => 4,
            Tier::Region => 6,
            Tier::Sol => 8,
            Tier::Gate => 12,
            Tier::Beyond => 16,
        }
    }

    /// How many standing trade routes the player may run at this tier (the
    /// master-table widens with scale, §4/§0.3). Monotonically non-decreasing.
    pub fn route_cap(self) -> usize {
        match self {
            Tier::Station => 4,
            Tier::Region => 6,
            Tier::Sol | Tier::Gate => 8,
            Tier::Beyond => 10,
        }
    }
}

/// How the far-side endgame resolves (§17, G5) — the culminating win/loss the §0
/// destination pull finally *completes*. `Undecided` until the bridgehead is either
/// established and held (`Triumph`) or overrun (`Fallen`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum EndgameOutcome {
    #[default]
    Undecided,
    /// The bridgehead was grown and held through the incursions — you own the far
    /// side. The journey's end (§0.1).
    Triumph,
    /// The bridgehead was overrun — the far side is lost. A genuine ending, not a
    /// treadmill.
    Fallen,
}

/// Basis-point denominator.
const BP: i64 = 10_000;
/// Tiers above the start, used to scale gate progress to `[0, BP]`.
const ASCENTS: i64 = 3;

/// The player's position on the climb (§0). Cheap, legible, always-visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
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
    /// foreshadowed from minute one (§0.1). Caps at 100% (the Gate); transiting it
    /// (`Beyond`) is past the bar, not more of it.
    pub fn gate_progress_bp(&self) -> i64 {
        (self.tier_index as i64 * BP / ASCENTS).min(BP)
    }

    /// Whether the gate has opened (the summit of the MVP climb — Gate or Beyond).
    pub fn gate_open(&self) -> bool {
        self.tier_index >= 3
    }

    /// Whether the player has **transited** the gate into the endgame (§17).
    pub fn transited(&self) -> bool {
        self.tier() == Tier::Beyond
    }

    /// Transit the open ring-gate (§0.1/§17) — the climactic, deliberate act that
    /// crosses from the Gate into `Beyond`. Only possible standing at the open gate;
    /// returns the new tier's name on success (the arrival fanfare), else `None`.
    pub fn transit(&mut self) -> Option<&'static str> {
        if self.tier() == Tier::Gate {
            self.tier_index += 1; // → Beyond
            self.ops_in_tier = 0;
            Some(self.tier().name())
        } else {
            None
        }
    }

    /// The current tier's briefing (the "different kind of game" framing, §0.3).
    pub fn briefing(&self) -> &'static str {
        self.tier().briefing()
    }

    /// How many stations / routes the player may run at the current tier — scope
    /// that widens as the company climbs (§0.3).
    pub fn station_cap(&self) -> usize {
        self.tier().station_cap()
    }

    pub fn route_cap(&self) -> usize {
        self.tier().route_cap()
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
    fn infrastructure_scope_widens_as_you_climb() {
        // Tier 1 keeps the baseline (4 stations / 4 routes); each ascent unlocks
        // a wider network — "Region = extended infrastructure" made mechanical.
        let mut c = Campaign::new();
        assert_eq!((c.station_cap(), c.route_cap()), (4, 4));
        let caps = |c: &Campaign| (c.station_cap(), c.route_cap());
        let mut prev = caps(&c);
        for _ in 0..(3 + 10 + 25) {
            c.record_op();
            let now = caps(&c);
            assert!(now.0 >= prev.0 && now.1 >= prev.1, "caps must never shrink");
            prev = now;
        }
        // At the gate the network is at its widest *of the MVP climb*.
        assert_eq!(c.tier(), Tier::Gate);
        assert_eq!((c.station_cap(), c.route_cap()), (12, 8));
    }

    #[test]
    fn transiting_the_open_gate_reaches_the_endgame() {
        // §0.1/§17: the climb summits at the open Gate; *transiting* it is the
        // deliberate climactic act into Beyond. It's only possible at the gate.
        let mut c = Campaign::new();
        assert!(
            c.transit().is_none(),
            "can't transit before reaching the gate"
        );
        for _ in 0..(3 + 10 + 25) {
            c.record_op();
        }
        assert_eq!(c.tier(), Tier::Gate);
        assert_eq!(c.gate_progress_bp(), BP, "the gate is fully open at 100%");
        assert!(!c.transited());
        // Ops never auto-cross the gate — transit is a deliberate verb.
        for _ in 0..50 {
            assert_eq!(c.record_op(), None, "the gate does not auto-advance");
        }
        assert_eq!(c.tier(), Tier::Gate);
        // The deliberate transit crosses into the endgame.
        assert_eq!(c.transit(), Some("Beyond the Gate"));
        assert_eq!(c.tier(), Tier::Beyond);
        assert!(c.transited());
        assert!(c.gate_open());
        // Gate progress stays capped at 100% (Beyond is past the bar, not more of it).
        assert_eq!(c.gate_progress_bp(), BP);
        // No double transit.
        assert!(c.transit().is_none());
    }

    #[test]
    fn every_tier_has_a_distinct_briefing() {
        let briefings: Vec<&str> = [Tier::Station, Tier::Region, Tier::Sol, Tier::Gate]
            .iter()
            .map(|t| t.briefing())
            .collect();
        // All four are present, non-empty, and distinct (a real per-tier reframe).
        assert!(briefings.iter().all(|b| !b.is_empty()));
        for i in 0..briefings.len() {
            for j in (i + 1)..briefings.len() {
                assert_ne!(briefings[i], briefings[j]);
            }
        }
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
