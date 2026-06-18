//! Contested colonies (early game) — the major frontier hubs the great powers
//! openly fight over (the **Ganymede conflict** from *The Expanse* as the model).
//! Each contested colony carries a per-faction **influence** share that drifts under
//! ambient Earth/Mars pressure (voiced as flares), plus the player's own slowly-built
//! standing. The early-game vision: *slowly gather influence over the small
//! jovian/cronian/belt colonies*, then claim one when your hold is strong enough.
//!
//! Deterministic, integer, and **rng-free**, and entirely additive — it touches only
//! its own numbers + the alert feed, never the market RNG — so the measured economy is
//! byte-identical and the §7c gate is untouched.

use super::faction::Faction;
use serde::{Deserialize, Serialize};

/// Total influence pie per contested colony, in basis points (the four powers split it).
pub const CONTEST_TOTAL: i64 = 1000;
/// Player standing (0..=CONTEST_TOTAL) needed to claim a contested colony.
pub const CLAIM_THRESHOLD: i64 = 600;
/// Player standing banked per courting (the slow gather-influence loop).
pub const COURT_GAIN: i64 = 90;
/// Influence (the E4 statecraft resource) spent to court a contested colony once.
pub const COURT_COST: i64 = 50;
/// Ticks between ambient great-power contest flares (Earth ↔ Mars tug-of-war).
pub const FLARE_INTERVAL: u64 = 90;
/// How much each flare shifts influence between the two inners.
pub const FLARE_SHIFT: i64 = 70;

/// One colony the powers fight over — the early-game political weather.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContestedColony {
    /// Index into `Sim::colonies`.
    pub colony: usize,
    /// Per-faction influence (basis points, summing to [`CONTEST_TOTAL`]), by
    /// [`Faction::index`].
    pub influence: [i64; 4],
    /// The player's own accumulated standing over this colony (0..=CONTEST_TOTAL),
    /// built by courting it (spending Influence). Reach [`CLAIM_THRESHOLD`] to claim it.
    pub player_influence: i64,
}

impl ContestedColony {
    /// A fresh contest seeded so the colony's current owner holds a plurality and the
    /// other powers share the rest — a lived-in, already-contested split.
    pub fn seed(colony: usize, owner: Faction) -> Self {
        // Owner leads (520); the contesting powers split the remainder (160 each).
        let mut influence = [160i64; 4];
        let lead = CONTEST_TOTAL - 160 * 3; // 520
        influence[owner.index()] = lead;
        Self {
            colony,
            influence,
            player_influence: 0,
        }
    }

    /// The power currently holding the upper hand (argmax of the four shares).
    pub fn leader(&self) -> Faction {
        let mut best = 0usize;
        for i in 1..4 {
            if self.influence[i] > self.influence[best] {
                best = i;
            }
        }
        Faction::ALL[best]
    }

    /// Whether the player's standing is strong enough to claim the colony.
    pub fn claimable(&self) -> bool {
        self.player_influence >= CLAIM_THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seeded_contest_sums_to_the_full_pie_and_the_owner_leads() {
        let c = ContestedColony::seed(7, Faction::Mars);
        assert_eq!(c.influence.iter().sum::<i64>(), CONTEST_TOTAL);
        assert_eq!(c.leader(), Faction::Mars);
        assert!(!c.claimable());
    }
}
