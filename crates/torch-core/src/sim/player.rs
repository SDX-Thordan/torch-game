//! The player-entity model — **every actor in the sim is a `Player`** (§0 re-aim).
//!
//! The human is just one player among equals. Ships, facilities, and settlements all carry
//! an `owner: PlayerId` back-reference; counts are derived from those, so a player never
//! holds dangling indices. The large nations (Earth/Mars/OPA) gain extra features in a later
//! iteration; for now every player is the same shape with a (stubbed) utility-AI `Agenda`.

use serde::{Deserialize, Serialize};

/// Stable id into `Sim.players` (and the value stored on owned entities). `players[i].id == i`.
pub type PlayerId = u16;

/// What kind of actor a player is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerKind {
    Human,
    Earth,
    Mars,
    Opa,
    /// One of several independent companies / colony operators.
    Company,
    /// A generic background actor (civilian shipping, minor industry).
    PrivateSector,
    Pirates,
}

impl PlayerKind {
    /// The three large nations — they get extra features (diplomacy, fleets) later.
    pub fn is_nation(self) -> bool {
        matches!(self, PlayerKind::Earth | PlayerKind::Mars | PlayerKind::Opa)
    }
}

/// The utility-AI agenda tag that drives `ai::think` (no-op this iteration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agenda {
    /// The human player, and a placeholder for actors with no active behaviour yet.
    Idle,
    Industrial,
    Trade,
    Expansion,
    Military,
    /// Pirate predation.
    Predation,
}

/// A player entity: its identity, treasury, and good stockpiles.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    pub id: PlayerId,
    pub name: String,
    pub kind: PlayerKind,
    pub agenda: Agenda,
    pub credits: i64,
    /// Per-good holdings, sized by `commodity::commodity_count()` (extensible — never a fixed
    /// array).
    pub stockpiles: Vec<i64>,
}

impl Player {
    pub fn new(id: PlayerId, name: &str, kind: PlayerKind, agenda: Agenda, credits: i64) -> Self {
        Self {
            id,
            name: name.to_string(),
            kind,
            agenda,
            credits,
            stockpiles: vec![0; super::commodity::commodity_count()],
        }
    }

    pub fn is_human(&self) -> bool {
        self.kind == PlayerKind::Human
    }

    /// Add `qty` of good `c` to the stockpile (clamped at 0).
    pub fn add_stock(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.stockpiles.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }

    pub fn stock(&self, c: usize) -> i64 {
        self.stockpiles.get(c).copied().unwrap_or(0)
    }
}

/// The default roster (`players[0]` is always the Human).
pub fn default_players() -> Vec<Player> {
    use Agenda::*;
    use PlayerKind::*;
    let mut next = 0u16;
    let mut mk = |name: &str, kind, agenda, credits| {
        let p = Player::new(next, name, kind, agenda, credits);
        next += 1;
        p
    };
    vec![
        mk("Independent Operator", Human, Idle, 50_000),
        mk("United Nations (Earth)", Earth, Industrial, 500_000),
        mk("Martian Congressional Republic", Mars, Industrial, 500_000),
        mk("Outer Planets Alliance", Opa, Expansion, 200_000),
        mk("Pallas Combine", Company, Trade, 120_000),
        mk("Tycho Industries", Company, Industrial, 120_000),
        mk("Private Sector", PrivateSector, Trade, 80_000),
        mk("The Free Navy", Pirates, Predation, 40_000),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_is_player_zero_and_ids_are_dense() {
        let ps = default_players();
        assert!(ps[0].is_human());
        for (i, p) in ps.iter().enumerate() {
            assert_eq!(p.id as usize, i, "ids are the index");
        }
        assert!(ps.iter().any(|p| p.kind == PlayerKind::Pirates));
        assert!(ps.iter().any(|p| p.kind == PlayerKind::PrivateSector));
    }
}
