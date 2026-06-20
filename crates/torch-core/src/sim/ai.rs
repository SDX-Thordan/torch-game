//! The utility-AI seam (§ agendas) — **data-model + stubs this iteration**.
//!
//! Every player has an [`Agenda`]; each tick `think` is called per player in stable id order.
//! The decision logic is a no-op for now (the human is `Idle`); the point of this module is
//! the deterministic seam that later iterations fill in without changing `step()`'s shape.

use super::player::Player;
use super::rng::Pcg32;

/// A read-only view of the world an agenda can reason over (kept minimal for now).
pub struct WorldView<'a> {
    pub tick: u64,
    pub body_count: usize,
    pub _marker: core::marker::PhantomData<&'a ()>,
}

/// Advance one player's agenda by one tick. **No-op this iteration for every agenda**, so a sim
/// that calls it is byte-identical to one that doesn't — the determinism the rebuild guarantees.
/// Later iterations branch on `player.agenda` (Industrial / Trade / Expansion / Military /
/// Predation) here without changing `step()`'s shape.
pub fn think(_player: &mut Player, _world: &WorldView, _rng: &mut Pcg32) {}

#[cfg(test)]
mod tests {
    use super::super::player::{default_players, PlayerKind};
    use super::*;

    #[test]
    fn think_is_a_noop_for_every_agenda() {
        let mut rng = Pcg32::new(1);
        let view = WorldView {
            tick: 0,
            body_count: 10,
            _marker: core::marker::PhantomData,
        };
        for mut p in default_players() {
            let before = p.clone();
            think(&mut p, &view, &mut rng);
            assert_eq!(p, before, "think must not mutate state yet");
        }
        // The human is Idle by construction.
        assert_eq!(default_players()[0].kind, PlayerKind::Human);
    }
}
