//! Stub deterministic orbital model (§6, §35 step 3).
//!
//! Bodies travel circular orbits; a body's angle is computed *directly* from the
//! tick (not integrated step-by-step), so there is no drift and any tick can be
//! evaluated in isolation — fully deterministic. Real distances are kept in the
//! sim (§21); the renderer compresses them. Patched-conic transfers come later.

use super::fixed::{cos_q16, sin_q16, Q16_ONE};

/// Distance units per astronomical unit.
pub const AU: i64 = 1_000_000;

/// A celestial body on a fixed circular orbit about the system origin (Sol).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub name: &'static str,
    /// Orbital radius in distance units; `0` for the central star.
    pub orbit_radius: i64,
    /// Orbital period in ticks; `0` for the central star (never moves).
    pub period_ticks: u64,
    /// Angle at tick 0, in milli-degrees `[0, 360000)`.
    pub phase_mdeg: i64,
}

impl Body {
    /// Angle (milli-degrees) at the given tick, derived directly from the tick.
    pub fn angle_mdeg(&self, tick: u64) -> i64 {
        if self.period_ticks == 0 {
            return self.phase_mdeg;
        }
        let swept =
            ((tick % self.period_ticks) as u128 * 360_000u128 / self.period_ticks as u128) as i64;
        (self.phase_mdeg + swept) % 360_000
    }

    /// Cartesian position `(x, y)` in distance units at the given tick.
    pub fn position(&self, tick: u64) -> (i64, i64) {
        if self.orbit_radius == 0 {
            return (0, 0);
        }
        let a = self.angle_mdeg(tick);
        let x = self.orbit_radius * cos_q16(a) / Q16_ONE;
        let y = self.orbit_radius * sin_q16(a) / Q16_ONE;
        (x, y)
    }
}

/// The default inner-system slice (§4): Sol plus three illustrative bodies.
/// Radii are real (AU); periods are scaled so 1 tick ≈ 1 hour.
pub fn default_system() -> Vec<Body> {
    vec![
        Body {
            name: "Sol",
            orbit_radius: 0,
            period_ticks: 0,
            phase_mdeg: 0,
        },
        Body {
            name: "Earth",
            orbit_radius: AU,
            period_ticks: 8_766,
            phase_mdeg: 0,
        },
        Body {
            name: "Mars",
            orbit_radius: 1_524_000,
            period_ticks: 16_487,
            phase_mdeg: 90_000,
        },
        Body {
            name: "Ceres",
            orbit_radius: 2_768_000,
            period_ticks: 40_335,
            phase_mdeg: 200_000,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn central_star_is_fixed_at_origin() {
        let sol = &default_system()[0];
        assert_eq!(sol.position(0), (0, 0));
        assert_eq!(sol.position(9_999), (0, 0));
    }

    #[test]
    fn bodies_stay_on_their_orbit_circle() {
        for body in default_system().iter().filter(|b| b.orbit_radius > 0) {
            let r2 = body.orbit_radius * body.orbit_radius;
            for tick in (0..body.period_ticks).step_by(101) {
                let (x, y) = body.position(tick);
                let d2 = x * x + y * y;
                let err = (d2 - r2).abs();
                assert!(
                    err * 100 < r2 * 2,
                    "{} off-circle at tick {tick}",
                    body.name
                ); // < 2%
            }
        }
    }

    #[test]
    fn position_repeats_each_period() {
        let mars = &default_system()[2];
        assert_eq!(mars.position(0), mars.position(mars.period_ticks));
        assert_eq!(mars.position(5), mars.position(mars.period_ticks + 5));
    }
}
