//! TORCH GDExtension entry point.
//!
//! The thin Godot binding (§26). All game logic lives in the engine-agnostic [`sim`] module;
//! this file only exposes the minimal surface the shell needs (the orrery, the top-bar counts,
//! and save/load), keeping the boundary thin so the core stays headless + native-testable.

#![allow(clippy::result_large_err)]

pub mod sim;

use godot::prelude::*;
use sim::ShipClass;

struct TorchExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TorchExtension {}

/// A tiny bridge object that proves the binding is live + carries the version.
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchCore {
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchCore {
    fn init(base: Base<RefCounted>) -> Self {
        Self { _base: base }
    }
}

#[godot_api]
impl TorchCore {
    #[func]
    fn version(&self) -> GString {
        GString::from(sim::VERSION)
    }
    #[func]
    fn fingerprint(&self, seed: i64) -> i64 {
        sim::fingerprint(seed as u64) as i64
    }
}

/// Godot-facing handle to the deterministic [`sim::Sim`].
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchSim {
    sim: sim::Sim,
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchSim {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            sim: sim::Sim::new(0),
            _base: base,
        }
    }
}

#[godot_api]
impl TorchSim {
    /// Reseed and restart the simulation (§27 determinism).
    #[func]
    fn reset(&mut self, seed: i64) {
        self.sim = sim::Sim::new(seed as u64);
    }

    /// Advance one fixed sim tick; returns the new tick.
    #[func]
    fn step(&mut self) -> i64 {
        self.sim.step();
        self.sim.tick() as i64
    }

    #[func]
    fn tick(&self) -> i64 {
        self.sim.tick() as i64
    }

    // ---- the orrery ----

    #[func]
    fn body_count(&self) -> i64 {
        self.sim.bodies().len() as i64
    }

    #[func]
    fn body_name(&self, index: i64) -> GString {
        GString::from(
            self.sim
                .bodies()
                .get(index.max(0) as usize)
                .map(|b| b.name)
                .unwrap_or(""),
        )
    }

    /// Body kind as an int (0 Star · 1 Planet · 2 GasGiant · 3 DwarfPlanet · 4 Moon · 7 Asteroid).
    #[func]
    fn body_kind(&self, index: i64) -> i64 {
        use sim::BodyKind::*;
        match self.sim.bodies().get(index.max(0) as usize).map(|b| b.kind) {
            Some(Star) => 0,
            Some(Planet) => 1,
            Some(GasGiant) => 2,
            Some(DwarfPlanet) => 3,
            Some(Moon) => 4,
            Some(Asteroid) => 7,
            None => -1,
        }
    }

    #[func]
    fn body_inhabitable(&self, index: i64) -> bool {
        self.sim
            .bodies()
            .get(index.max(0) as usize)
            .map(|b| b.inhabitable)
            .unwrap_or(false)
    }

    #[func]
    fn body_x(&self, index: i64) -> i64 {
        self.sim.body_pos(index.max(0) as usize).0
    }

    #[func]
    fn body_y(&self, index: i64) -> i64 {
        self.sim.body_pos(index.max(0) as usize).1
    }

    // ---- the top bar (human player) ----

    #[func]
    fn credits(&self) -> i64 {
        self.sim.human_credits()
    }
    #[func]
    fn count_haulers(&self) -> i64 {
        self.sim.human_ship_count(ShipClass::Hauler) as i64
    }
    #[func]
    fn count_miners(&self) -> i64 {
        self.sim.human_ship_count(ShipClass::Miner) as i64
    }
    #[func]
    fn count_combat(&self) -> i64 {
        self.sim.human_ship_count(ShipClass::Combat) as i64
    }
    #[func]
    fn count_colonies(&self) -> i64 {
        self.sim.human_colony_count() as i64
    }
    #[func]
    fn count_mining_stations(&self) -> i64 {
        self.sim.human_mining_station_count() as i64
    }
    /// Player count (all entities are equal players).
    #[func]
    fn player_count(&self) -> i64 {
        self.sim.players().len() as i64
    }

    // ---- save / load (§30) ----

    /// Write a binary save to `path` (a globalized OS path). Returns "" or an error message.
    #[func]
    fn save_game(&self, path: GString) -> GString {
        match std::fs::write(path.to_string(), self.sim.save_bytes()) {
            Ok(()) => GString::new(),
            Err(e) => GString::from(e.to_string()),
        }
    }

    /// Load a save from `path`, replacing the live sim. Returns "" or an error message.
    #[func]
    fn load_game(&mut self, path: GString) -> GString {
        match std::fs::read(path.to_string()) {
            Ok(bytes) => match sim::Sim::load_bytes(&bytes) {
                Ok(s) => {
                    self.sim = s;
                    GString::new()
                }
                Err(e) => GString::from(e),
            },
            Err(e) => GString::from(e.to_string()),
        }
    }
}
