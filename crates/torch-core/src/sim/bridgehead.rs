//! The far-side bridgehead (§17 endgame, G3) — the player's own foothold beyond
//! the ring. Founded only after **transiting** the gate (`Tier::Beyond`), it
//! anchors presence on the far side: a colony the player upgrades to grow, and
//! that the far-side incursions (G4) threaten. Holding it through rising
//! incursions to a victory — or losing it — is the spine of the endgame (G5).
//!
//! Inert by construction: a fresh sim has no bridgehead (`founded == false`), and
//! it can only be founded post-transit, so nothing here touches the pre-transit
//! economy or the §7c gate.

/// Base integrity of a level-1 foothold.
const BASE_INTEGRITY: i64 = 100;
/// Extra max integrity granted per upgrade level.
const INTEGRITY_PER_LEVEL: i64 = 60;

/// The player's far-side foothold (§17). `Copy` so the verbs can lift it out of
/// `self` before mutating the treasury (the borrow-checker pattern used across the
/// sim). Default = unfounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Bridgehead {
    founded: bool,
    level: u32,
    /// Current integrity in `0..=max_integrity()`. Falls under incursion (G4); a
    /// foothold at zero integrity has **fallen** (G5).
    integrity: i64,
}

impl Bridgehead {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_founded(&self) -> bool {
        self.founded
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn integrity(&self) -> i64 {
        self.integrity
    }

    /// Maximum integrity at the current level — grows with each upgrade, so a
    /// stronger bridgehead weathers more incursion (G4).
    pub fn max_integrity(&self) -> i64 {
        if !self.founded {
            return 0;
        }
        BASE_INTEGRITY + INTEGRITY_PER_LEVEL * (self.level.saturating_sub(1)) as i64
    }

    /// Found the foothold at level 1, full integrity. Idempotent — returns `false`
    /// if one already stands.
    pub fn found(&mut self) -> bool {
        if self.founded {
            return false;
        }
        self.founded = true;
        self.level = 1;
        self.integrity = BASE_INTEGRITY;
        true
    }

    /// Upgrade by one level, raising max integrity and topping it back up. Requires
    /// a standing foothold; returns `false` if none is founded.
    pub fn upgrade(&mut self) -> bool {
        if !self.founded {
            return false;
        }
        self.level += 1;
        self.integrity = self.max_integrity();
        true
    }

    /// Take `amount` of incursion damage (G4). Returns `true` if this blow fells
    /// the bridgehead (integrity hits zero). No-op on an unfounded foothold.
    pub fn damage(&mut self, amount: i64) -> bool {
        if !self.founded {
            return false;
        }
        self.integrity = (self.integrity - amount.max(0)).max(0);
        self.integrity == 0
    }

    /// Repair `amount` integrity, capped at the current max. No-op when unfounded.
    pub fn repair(&mut self, amount: i64) {
        if !self.founded {
            return;
        }
        self.integrity = (self.integrity + amount.max(0)).min(self.max_integrity());
    }

    /// Whether the foothold has fallen — founded, but ground down to zero (G5 loss).
    pub fn has_fallen(&self) -> bool {
        self.founded && self.integrity == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unfounded_bridgehead_is_inert() {
        let mut b = Bridgehead::new();
        assert!(!b.is_founded());
        assert_eq!(b.level(), 0);
        assert_eq!(b.max_integrity(), 0);
        // None of the post-founding verbs do anything without a foothold.
        assert!(!b.upgrade());
        assert!(!b.damage(50));
        b.repair(50);
        assert!(!b.has_fallen());
    }

    #[test]
    fn founding_then_upgrading_grows_the_foothold() {
        let mut b = Bridgehead::new();
        assert!(b.found());
        assert_eq!(b.level(), 1);
        assert_eq!(b.integrity(), BASE_INTEGRITY);
        assert!(!b.found(), "no second founding");
        let l1_max = b.max_integrity();
        assert!(b.upgrade());
        assert_eq!(b.level(), 2);
        assert!(b.max_integrity() > l1_max, "upgrade raises the ceiling");
        assert_eq!(b.integrity(), b.max_integrity(), "upgrade tops it up");
    }

    #[test]
    fn damage_can_fell_a_foothold_and_repair_caps_at_max() {
        let mut b = Bridgehead::new();
        b.found();
        assert!(!b.damage(BASE_INTEGRITY - 1));
        assert_eq!(b.integrity(), 1);
        assert!(!b.has_fallen());
        // Repair never exceeds the max.
        b.repair(10_000);
        assert_eq!(b.integrity(), b.max_integrity());
        // A blow past zero fells it.
        assert!(b.damage(10_000));
        assert!(b.has_fallen());
        assert_eq!(b.integrity(), 0);
    }
}
