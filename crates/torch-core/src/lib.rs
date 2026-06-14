//! TORCH GDExtension entry point.
//!
//! This is the thin Godot binding (§26 Godot 4.x + Rust gdext). All game logic
//! lives in the engine-agnostic [`sim`] module; this file only exposes it to the
//! Godot shell. Keeping the boundary thin is what lets the core stay headless
//! and native-testable (§27, §32).

// gdext's `#[godot_api]` macro expands to `Result`s carrying Godot's large
// `CallError`; this clippy lint fires on generated code we don't control.
#![allow(clippy::result_large_err)]

pub mod sim;

use godot::prelude::*;

struct TorchExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TorchExtension {}

/// Bridge object the Godot shell instantiates to talk to the Rust core. For the
/// toolchain de-risk (§35.1) it just proves the binding is live; it will grow
/// into the snapshot + event-stream contract (§29).
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
    /// Core crate version.
    #[func]
    fn version(&self) -> GString {
        GString::from(sim::VERSION)
    }

    /// Hello-world greeting from the Rust core.
    #[func]
    fn greeting(&self) -> GString {
        GString::from(sim::greeting())
    }

    /// Deterministic fingerprint of a seed — lets the shell verify the same
    /// seed yields the same result through the binding (§27 determinism).
    #[func]
    fn fingerprint(&self, seed: i64) -> i64 {
        sim::fingerprint(seed as u64) as i64
    }
}

/// Godot-facing handle to the deterministic [`sim::Sim`] (§29). Exposes the
/// fixed-tick advance plus scalar snapshot accessors the shell renders; the
/// real game logic stays in `sim`, this is only the binding.
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

    /// Advance one fixed sim tick (§28); returns the new tick.
    #[func]
    fn step(&mut self) -> i64 {
        self.sim.step();
        self.sim.tick() as i64
    }

    /// Current tick.
    #[func]
    fn tick(&self) -> i64 {
        self.sim.tick() as i64
    }

    /// Number of bodies in the snapshot.
    #[func]
    fn body_count(&self) -> i64 {
        self.sim.bodies().len() as i64
    }

    #[func]
    fn body_name(&self, index: i64) -> GString {
        GString::from(
            self.sim
                .bodies()
                .get(index as usize)
                .map(|b| b.name)
                .unwrap_or(""),
        )
    }

    #[func]
    fn body_x(&self, index: i64) -> i64 {
        self.sim
            .snapshot()
            .bodies
            .get(index as usize)
            .map(|b| b.x)
            .unwrap_or(0)
    }

    #[func]
    fn body_y(&self, index: i64) -> i64 {
        self.sim
            .snapshot()
            .bodies
            .get(index as usize)
            .map(|b| b.y)
            .unwrap_or(0)
    }

    /// Number of markets (§7a).
    #[func]
    fn market_count(&self) -> i64 {
        self.sim.markets().len() as i64
    }

    #[func]
    fn market_name(&self, market: i64) -> GString {
        GString::from(
            self.sim
                .markets()
                .get(market as usize)
                .map(|m| m.name())
                .unwrap_or(""),
        )
    }

    /// Number of commodities (shared across markets).
    #[func]
    fn commodity_count(&self) -> i64 {
        self.sim.markets()[0].defs().len() as i64
    }

    #[func]
    fn commodity_name(&self, index: i64) -> GString {
        GString::from(
            self.sim.markets()[0]
                .defs()
                .get(index as usize)
                .map(|d| d.name)
                .unwrap_or(""),
        )
    }

    /// Price of commodity `c` at market `m`.
    #[func]
    fn price(&self, market: i64, commodity: i64) -> i64 {
        self.sim
            .markets()
            .get(market as usize)
            .map(|m| m.price(commodity as usize))
            .unwrap_or(0)
    }

    /// Stock of commodity `c` at market `m`.
    #[func]
    fn stock(&self, market: i64, commodity: i64) -> i64 {
        self.sim
            .markets()
            .get(market as usize)
            .map(|m| m.stock(commodity as usize))
            .unwrap_or(0)
    }

    /// Number of haulers currently in flight (§7b).
    #[func]
    fn hauler_count(&self) -> i64 {
        self.sim.haulers().len() as i64
    }

    /// Id of the in-flight hauler at `index` (−1 if out of range).
    #[func]
    fn hauler_id(&self, index: i64) -> i64 {
        self.sim
            .haulers()
            .get(index as usize)
            .map(|h| h.id as i64)
            .unwrap_or(-1)
    }

    /// Cut the in-flight hauler with `id`; returns whether one was interdicted.
    #[func]
    fn interdict(&mut self, id: i64) -> bool {
        self.sim.interdict(id as u64)
    }

    /// Attempt to interdict hauler `id` with an interceptor at `(x, y)` of the
    /// given `speed` and crew `skill_bp` (§7b). Returns the outcome:
    /// 0 = no solution, 1 = escaped, 2 = interdicted.
    #[func]
    fn attempt_interdict(&mut self, id: i64, x: i64, y: i64, speed: i64, skill_bp: i64) -> i64 {
        let interceptor = sim::Interceptor {
            pos: (x, y),
            speed,
            skill_bp,
        };
        match self.sim.interdict_with(id as u64, interceptor) {
            sim::Interdiction::NoSolution => 0,
            sim::Interdiction::Escaped => 1,
            sim::Interdiction::Interdicted => 2,
        }
    }
}

/// Godot-facing view of the warship catalog and reference fits (§8). Exposes the
/// derived stats of a sensible reference loadout per class so the shell can show
/// the railgun escalation axis; the fitting logic stays in `sim::ships`.
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchShipyard {
    classes: Vec<(GString, sim::ShipStats)>,
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchShipyard {
    fn init(base: Base<RefCounted>) -> Self {
        use sim::ships::{reference_loadout, ShipClass};
        let mut rng = sim::rng::Pcg32::new(1);
        let classes = [
            ShipClass::Frigate,
            ShipClass::Destroyer,
            ShipClass::Cruiser,
            ShipClass::Battleship,
        ]
        .into_iter()
        .map(|c| {
            let lo = reference_loadout(c, &mut rng);
            (GString::from(lo.hull().name), lo.stats())
        })
        .collect();
        Self {
            classes,
            _base: base,
        }
    }
}

#[godot_api]
impl TorchShipyard {
    #[func]
    fn class_count(&self) -> i64 {
        self.classes.len() as i64
    }

    #[func]
    fn class_name(&self, index: i64) -> GString {
        self.classes
            .get(index as usize)
            .map(|c| c.0.clone())
            .unwrap_or_default()
    }

    #[func]
    fn railguns(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.railguns as i64)
            .unwrap_or(0)
    }

    #[func]
    fn alpha(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.effective_alpha())
            .unwrap_or(0)
    }

    #[func]
    fn delta_v(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.delta_v)
            .unwrap_or(0)
    }

    #[func]
    fn mobility(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.thrust_to_mass)
            .unwrap_or(0)
    }

    /// Resolve a demo duel: `n` torpedo frigates vs one battleship at `band`
    /// (0 = close, 1 = medium, 2 = long). Returns a one-line result (§9).
    #[func]
    fn duel(&self, n: i64, band: i64) -> GString {
        use sim::combat::demo_duel;
        let band = match band {
            0 => sim::Band::Close,
            2 => sim::Band::Long,
            _ => sim::Band::Medium,
        };
        let out = demo_duel(n.max(0) as usize, band, 0);
        let who = match out.winner {
            Some(0) => "frigates win",
            Some(1) => "battleship wins",
            _ => "stalemate",
        };
        GString::from(format!("{who} in {} ticks", out.ticks))
    }
}
