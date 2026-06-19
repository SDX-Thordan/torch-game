//! Delta-v-governed ship movement (§6, Pillar #2) — the universal constraint made
//! concrete. Every owned ship has a **tracked position** (a body it's docked at, or
//! an in-flight trajectory) and a **remass budget** (reaction mass = fuel). A move
//! *commits a trajectory*: it spends remass and takes time derived from the ship's
//! drive and the chosen burn at the **live orbital distance** — never a flat speed.
//! Run the tank dry and the ship is **stranded** until refuelled. Integer/
//! deterministic (§27).
//!
//! This is the movement layer the combat-only `delta_v` proxy (§8) never fed; it
//! turns the fleet into a positional, logistical asset (and a real interdiction
//! target) instead of an abstract roster.

use super::ships::Loadout;

/// Acceleration scale: `accel (units/tick²) = max_thrust × K / dry_mass` (thrust-to-mass
/// *is* acceleration). A ship flies a flip-and-burn (brachistochrone): it accelerates to the
/// midpoint, flips, and brakes to rest, so `travel = 2·√(distance / accel)`. Tuned so a
/// warship's harder burn out-runs a civilian hauler and ~1 AU is a few dozen ticks.
const ACCEL_K: i64 = 1_200;
/// Floor on acceleration so a heavy hull still moves.
const MIN_ACCEL: i64 = 600;
/// Floor on travel so even a short hop takes real time (§21 felt distance).
const MIN_TRAVEL: u64 = 4;
/// Remass cost numerator/denominator: `cost = distance × NUM / (drive_efficiency ×
/// DEN)`. An efficient drive burns less; tuned so a small hull does several inner
/// hops on a tank but can't reach the outer system without refuelling.
const REMASS_NUM: i64 = 9;
const REMASS_DEN: i64 = 1_000;

/// Per-ship navigation state (§6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Nav {
    /// The body it's docked at (or last departed from). Index into the body list.
    pub location: usize,
    /// Destination body; equals `location` when docked.
    pub dest: usize,
    pub depart_tick: u64,
    pub arrival_tick: u64,
    /// Current reaction mass (fuel) and tankage.
    pub remass: i64,
    pub remass_max: i64,
}

impl Nav {
    /// A ship docked at `location` with a full tank.
    pub fn docked(location: usize, remass_max: i64) -> Self {
        Self {
            location,
            dest: location,
            depart_tick: 0,
            arrival_tick: 0,
            remass: remass_max,
            remass_max: remass_max.max(1),
        }
    }

    /// Whether the ship is mid-trajectory at `tick`.
    pub fn in_transit(&self, tick: u64) -> bool {
        self.dest != self.location && tick < self.arrival_tick
    }

    /// Fuel as basis points of tankage (0..=10000) — for the FLEET gauge (§14).
    pub fn fuel_bp(&self) -> i64 {
        (self.remass * 10_000 / self.remass_max.max(1)).clamp(0, 10_000)
    }

    /// Stranded: docked (not moving) with too little remass to do anything useful.
    pub fn is_stranded(&self, tick: u64) -> bool {
        !self.in_transit(tick) && self.dest == self.location && self.remass <= 0
    }
}

/// The cost of a committed trajectory (§6): time + remass, from the live distance,
/// the ship's drive, and the chosen burn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub travel_ticks: u64,
    pub remass_cost: i64,
}

/// Plan a transfer of `distance` units for `loadout`, at an **economical** or
/// **hard** burn (§6 verb #4): a hard burn halves the time but doubles the remass.
pub fn plan(loadout: &Loadout, distance: i64, hard_burn: bool) -> Plan {
    let h = loadout.hull();
    let accel = (h.max_thrust * ACCEL_K / h.dry_mass.max(1)).max(MIN_ACCEL);
    // Flip-and-burn: t = 2·√(distance / accel). A hard burn pushes ~2× the G, ≈0.7× the time.
    let base_ticks = (2 * (distance.max(0) / accel.max(1)).isqrt()) as u64;
    let base_cost = (distance * REMASS_NUM / (h.drive_efficiency.max(1) * REMASS_DEN)).max(1);
    if hard_burn {
        Plan {
            travel_ticks: (base_ticks * 7 / 10).max(MIN_TRAVEL),
            remass_cost: base_cost * 2,
        }
    } else {
        Plan {
            travel_ticks: base_ticks.max(MIN_TRAVEL),
            remass_cost: base_cost,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::Pcg32;
    use crate::sim::ships::{reference_loadout_quality, ShipClass};

    fn frigate() -> Loadout {
        reference_loadout_quality(ShipClass::Frigate, 50, &mut Pcg32::new(1))
    }

    #[test]
    fn a_hard_burn_is_faster_but_thirstier() {
        let l = frigate();
        let au = 1_000_000;
        let eco = plan(&l, 3 * au, false);
        let hard = plan(&l, 3 * au, true);
        assert!(hard.travel_ticks < eco.travel_ticks, "hard burn is faster");
        assert!(
            hard.remass_cost > eco.remass_cost,
            "hard burn costs more remass"
        );
    }

    #[test]
    fn farther_costs_more_and_takes_longer() {
        let l = frigate();
        let au = 1_000_000;
        let near = plan(&l, au, false);
        let far = plan(&l, 9 * au, false);
        assert!(far.travel_ticks > near.travel_ticks);
        assert!(far.remass_cost > near.remass_cost);
    }

    #[test]
    fn the_outer_system_can_strand_a_small_hull() {
        // A frigate's tank can't reach Saturn (~9 AU) in one economical hop —
        // refuelling is strategic ground (§6).
        let l = frigate();
        let tank = l.hull().remass_capacity;
        let saturn = plan(&l, 9 * 1_000_000, false);
        assert!(saturn.remass_cost > tank, "deep hauls need a refuel stop");
    }

    #[test]
    fn nav_tracks_transit_and_fuel() {
        let mut nav = Nav::docked(3, 600);
        assert!(!nav.in_transit(0));
        assert_eq!(nav.fuel_bp(), 10_000);
        nav.dest = 5;
        nav.depart_tick = 10;
        nav.arrival_tick = 40;
        nav.remass = 300;
        assert!(nav.in_transit(20));
        assert!(!nav.in_transit(40)); // arrived
        assert_eq!(nav.fuel_bp(), 5_000);
    }
}
