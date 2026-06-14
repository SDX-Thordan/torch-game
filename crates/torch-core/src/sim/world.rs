//! The authoritative world and the sim↔view contract (§27, §28, §29).
//!
//! [`Sim`] owns the truth and advances on a fixed tick. The view never reads
//! `Sim` directly: it consumes a [`Snapshot`] (current state to render) plus the
//! [`Event`] stream returned by [`Sim::step`] (what changed). This is the seam
//! the Godot shell binds to and that keeps the core headless and testable.

use super::event::Event;
use super::orbit::{default_system, Body};
use super::rng::Pcg32;

/// A renderable view of one body at a single tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyState {
    pub name: &'static str,
    pub x: i64,
    pub y: i64,
}

/// An immutable snapshot of the world for rendering (§29).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub tick: u64,
    pub bodies: Vec<BodyState>,
}

/// The authoritative deterministic simulation.
pub struct Sim {
    tick: u64,
    bodies: Vec<Body>,
    rng: Pcg32,
    events: Vec<Event>,
}

impl Sim {
    /// Create a sim seeded for determinism (§27). Same seed ⇒ same run.
    pub fn new(seed: u64) -> Self {
        Self {
            tick: 0,
            bodies: default_system(),
            rng: Pcg32::new(seed),
            events: Vec::new(),
        }
    }

    /// The current tick.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The bodies under simulation.
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// The shared deterministic RNG every system draws from (§27).
    pub fn rng_mut(&mut self) -> &mut Pcg32 {
        &mut self.rng
    }

    /// Advance exactly one fixed sim tick (§28) and return the events produced.
    /// The returned slice is valid until the next call to `step`.
    pub fn step(&mut self) -> &[Event] {
        self.tick += 1;
        self.events.clear();
        self.events.push(Event::Tick { tick: self.tick });
        &self.events
    }

    /// Build a render snapshot of the world at the current tick (§29).
    pub fn snapshot(&self) -> Snapshot {
        let bodies = self
            .bodies
            .iter()
            .map(|b| {
                let (x, y) = b.position(self.tick);
                BodyState { name: b.name, x, y }
            })
            .collect();
        Snapshot {
            tick: self.tick,
            bodies,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_advances_tick_and_emits_event() {
        let mut sim = Sim::new(1);
        assert_eq!(sim.tick(), 0);
        let events = sim.step();
        assert_eq!(events, &[Event::Tick { tick: 1 }]);
        assert_eq!(sim.tick(), 1);
    }

    #[test]
    fn snapshot_reflects_current_tick() {
        let mut sim = Sim::new(1);
        for _ in 0..50 {
            sim.step();
        }
        let snap = sim.snapshot();
        assert_eq!(snap.tick, 50);
        assert_eq!(snap.bodies.len(), default_system().len());
        // Sol stays at the origin; an orbiting body has moved off it.
        assert_eq!((snap.bodies[0].x, snap.bodies[0].y), (0, 0));
        assert_ne!((snap.bodies[1].x, snap.bodies[1].y), (0, 0));
    }

    #[test]
    fn same_seed_yields_identical_runs() {
        let mut a = Sim::new(42);
        let mut b = Sim::new(42);
        for _ in 0..500 {
            assert_eq!(a.step(), b.step());
            assert_eq!(a.snapshot(), b.snapshot());
        }
    }
}
