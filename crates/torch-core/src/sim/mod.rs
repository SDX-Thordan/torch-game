//! The deterministic, engine-agnostic simulation core (§27).
//!
//! Everything here is pure Rust with no Godot dependency, so it runs headless
//! and is verified by native `cargo test` (§32). The Godot layer (`lib.rs`) is a
//! thin binding over this.

pub mod alerts;
pub mod combat;
pub mod economy;
pub mod event;
pub mod faction;
pub mod fixed;
pub mod interdiction;
pub mod orbit;
pub mod rng;
pub mod ships;
pub mod traffic;
pub mod world;

pub use alerts::{Alert, AlertFeed, Priority, Urgency, Verb};
pub use combat::{Band, BattleOutcome, CombatEvent, Doctrine, Fleet, TargetPriority};
pub use economy::{CommodityDef, Market, Stock};
pub use event::Event;
pub use faction::{Faction, Relations, RepTier};
pub use interdiction::{Interceptor, Interdiction};
pub use ships::{Crew, FitError, HullDef, Loadout, ShipClass, ShipStats, WeaponDef, WeaponKind};
pub use traffic::Hauler;
pub use world::{BodyState, Sim, Snapshot};

use rng::Pcg32;

/// Core version, surfaced to the shell to prove the binding is live.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Greeting used by the toolchain de-risk hello-world (§35.1).
pub fn greeting() -> String {
    format!("TORCH core v{VERSION} — deterministic sim online")
}

/// A deterministic fingerprint of a seed: sum of the first 1000 PCG32 outputs.
/// Stable across platforms; used to assert determinism end-to-end through the
/// Godot binding.
pub fn fingerprint(seed: u64) -> u64 {
    let mut rng = Pcg32::new(seed);
    let mut acc = 0u64;
    for _ in 0..1000 {
        acc = acc.wrapping_add(rng.next_u32() as u64);
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_deterministic() {
        assert_eq!(fingerprint(42), fingerprint(42));
        assert_ne!(fingerprint(1), fingerprint(2));
    }

    #[test]
    fn version_is_present() {
        assert!(!VERSION.is_empty());
        assert!(greeting().contains(VERSION));
    }
}
