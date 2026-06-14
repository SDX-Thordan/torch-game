//! Factions and reputation (§4, §10) — the three-way cold war as standings.
//!
//! Reputation gates tech catalogs, contracts, station access, and prices (§10).
//! Here we model the standings themselves and how player actions move them — the
//! place the deferred §7b ripple lands: cutting a faction's shipping angers them
//! and quietly pleases their rival. Integer/deterministic (§27).

/// The powers of the inner system (§4). Pirates are handled separately (§13).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faction {
    Earth,
    Mars,
    Belt,
    Independents,
}

impl Faction {
    pub const ALL: [Faction; 4] = [
        Faction::Earth,
        Faction::Mars,
        Faction::Belt,
        Faction::Independents,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Faction::Earth => "Earth",
            Faction::Mars => "Mars",
            Faction::Belt => "Belt",
            Faction::Independents => "Independents",
        }
    }

    /// The faction that takes quiet satisfaction when this one is harmed (§4 cold
    /// war): Earth and Mars are peers; the Belt resents the inners.
    pub fn rival(self) -> Option<Faction> {
        match self {
            Faction::Earth => Some(Faction::Mars),
            Faction::Mars => Some(Faction::Earth),
            Faction::Belt => Some(Faction::Earth),
            Faction::Independents => None,
        }
    }

    fn index(self) -> usize {
        match self {
            Faction::Earth => 0,
            Faction::Mars => 1,
            Faction::Belt => 2,
            Faction::Independents => 3,
        }
    }
}

/// Reputation tiers gate what a faction offers (§10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepTier {
    Hostile,
    Cold,
    Neutral,
    Cordial,
    Allied,
}

/// How far a single interdiction angers the victim faction.
const INTERDICT_PENALTY: i64 = 50;
/// …and how much it pleases their rival.
const RIVAL_BONUS: i64 = 20;
/// Standing is clamped to this magnitude.
const STANDING_CAP: i64 = 1_000;

/// The player's standing with every faction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relations {
    standing: [i64; 4],
}

impl Default for Relations {
    fn default() -> Self {
        Self::new()
    }
}

impl Relations {
    /// Everyone starts neutral (the player is independent by default, §3).
    pub fn new() -> Self {
        Self { standing: [0; 4] }
    }

    pub fn standing(&self, f: Faction) -> i64 {
        self.standing[f.index()]
    }

    /// Reputation tier for a faction, from its current standing.
    pub fn tier(&self, f: Faction) -> RepTier {
        match self.standing(f) {
            s if s <= -600 => RepTier::Hostile,
            s if s <= -200 => RepTier::Cold,
            s if s < 200 => RepTier::Neutral,
            s if s < 600 => RepTier::Cordial,
            _ => RepTier::Allied,
        }
    }

    /// Move a faction's standing, clamped to `[-CAP, CAP]`.
    pub fn adjust(&mut self, f: Faction, delta: i64) {
        let s = &mut self.standing[f.index()];
        *s = (*s + delta).clamp(-STANDING_CAP, STANDING_CAP);
    }

    /// The player cut `victim`'s shipping (§7b): they resent it, their rival
    /// approves.
    pub fn on_player_interdict(&mut self, victim: Faction) {
        self.adjust(victim, -INTERDICT_PENALTY);
        if let Some(rival) = victim.rival() {
            self.adjust(rival, RIVAL_BONUS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everyone_starts_neutral() {
        let r = Relations::new();
        for f in Faction::ALL {
            assert_eq!(r.standing(f), 0);
            assert_eq!(r.tier(f), RepTier::Neutral);
        }
    }

    #[test]
    fn adjust_clamps_to_the_cap() {
        let mut r = Relations::new();
        r.adjust(Faction::Earth, 5_000);
        assert_eq!(r.standing(Faction::Earth), STANDING_CAP);
        r.adjust(Faction::Earth, -5_000);
        assert_eq!(r.standing(Faction::Earth), -STANDING_CAP);
    }

    #[test]
    fn tiers_track_thresholds() {
        let mut r = Relations::new();
        r.adjust(Faction::Mars, 600);
        assert_eq!(r.tier(Faction::Mars), RepTier::Allied);
        r.adjust(Faction::Mars, -800); // now -200
        assert_eq!(r.tier(Faction::Mars), RepTier::Cold);
    }

    #[test]
    fn interdiction_angers_the_victim_and_pleases_the_rival() {
        let mut r = Relations::new();
        r.on_player_interdict(Faction::Earth);
        assert_eq!(r.standing(Faction::Earth), -INTERDICT_PENALTY);
        assert_eq!(r.standing(Faction::Mars), RIVAL_BONUS); // Earth's rival approves
        assert_eq!(r.standing(Faction::Belt), 0); // bystander unmoved
    }

    #[test]
    fn independents_have_no_rival() {
        assert_eq!(Faction::Independents.rival(), None);
        let mut r = Relations::new();
        r.on_player_interdict(Faction::Independents);
        assert_eq!(r.standing(Faction::Independents), -INTERDICT_PENALTY);
    }
}
