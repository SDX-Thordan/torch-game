//! The deterministic, engine-agnostic simulation core (§27).
//!
//! Pure Rust with no Godot dependency, verified by native `cargo test` (§32). The Godot layer
//! (`lib.rs`) is a thin binding over this. Rebuilt around the multi-player entity model.

pub mod ai;
pub mod commodity;
pub mod economy;
pub mod facility;
pub mod fixed;
pub mod orbit;
pub mod persist;
pub mod player;
pub mod rng;
pub mod ship;
pub mod world;

pub use commodity::{CommodityDef, GoodTier, Recipe};
pub use economy::{Market, PriceDef, Sink};
pub use facility::{Facility, FacilityKind};
pub use orbit::{Body, BodyKind};
pub use persist::{SaveState, SAVE_VERSION};
pub use player::{Agenda, Player, PlayerId, PlayerKind};
pub use ship::{Section, SectionKind, Ship, ShipClass, SlotKind, Subsystem};
pub use world::{Colony, MiningStation, Sim};

use rng::Pcg32;

/// Core version, surfaced to the shell to prove the binding is live.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A deterministic fingerprint of a seed: sum of the first 1000 PCG32 outputs. Stable across
/// platforms; the end-to-end determinism canary.
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
    }
}
