//! Deterministic orbital model (§6, §17, §35 step 3).
//!
//! Bodies travel circular orbits about a **parent** (Sol for planets; the planet
//! for moons). A body's angle is computed *directly* from the tick (not integrated
//! step-by-step), so there is no drift and any tick can be evaluated in isolation
//! — fully deterministic. Real distances are kept in the sim (§21); the renderer
//! compresses and lets the player zoom. The full system runs Mercury → Pluto with
//! the ring-gate beyond, and the gas giants carry moon systems (§17 frontier).

use super::fixed::{cos_q16, sin_q16, Q16_ONE};

/// Distance units per astronomical unit.
pub const AU: i64 = 1_000_000;
/// Ticks per (Earth) year at the sim's "1 tick ≈ 1 hour" cadence.
const YEAR: u64 = 8_766;

/// What a body *is*, for rendering (size/colour) and gameplay (§17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Star,
    Planet,
    GasGiant,
    DwarfPlanet,
    Moon,
    /// The foreshadowed ring-gate beyond Pluto (§0.1) — a fixed landmark.
    Gate,
    /// A body on the **far side** of the gate (§17 endgame) — revealed only after
    /// the player transits. Exists always (determinism), shown only post-transit.
    FarSide,
}

/// A celestial body on a fixed circular orbit about its parent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub name: &'static str,
    /// Index of the body this orbits; **self-index** for the root (Sol).
    pub parent: usize,
    /// Orbital radius about the parent, in distance units; `0` for the star.
    pub orbit_radius: i64,
    /// Orbital period in ticks; `0` never moves.
    pub period_ticks: u64,
    /// Angle at tick 0, in milli-degrees `[0, 360000)`.
    pub phase_mdeg: i64,
    pub kind: BodyKind,
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

    /// Position **relative to the parent** `(x, y)` in distance units at `tick`.
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

/// Absolute position of body `i` at `tick`, resolving the parent chain (a moon's
/// position is its local orbit added to its planet's, added to Sol at the origin).
pub fn position_of(bodies: &[Body], i: usize, tick: u64) -> (i64, i64) {
    let b = &bodies[i];
    let (lx, ly) = b.position(tick);
    if b.parent == i {
        return (lx, ly); // the root (Sol)
    }
    let (px, py) = position_of(bodies, b.parent, tick);
    (px + lx, py + ly)
}

// A planet about Sol (parent 0).
fn planet(name: &'static str, au_milli: i64, years_x100: u64, phase: i64, kind: BodyKind) -> Body {
    Body {
        name,
        parent: 0,
        orbit_radius: AU * au_milli / 1000,
        period_ticks: YEAR * years_x100 / 100,
        phase_mdeg: phase,
        kind,
    }
}

// A moon about `parent`. Radii are *exaggerated for legibility* (so a zoomed-in
// gas giant reads as a busy little system, §17) rather than astronomically exact.
fn moon(name: &'static str, parent: usize, radius: i64, period: u64, phase: i64) -> Body {
    Body {
        name,
        parent,
        orbit_radius: radius,
        period_ticks: period,
        phase_mdeg: phase,
        kind: BodyKind::Moon,
    }
}

/// The full solar system out to Pluto, the ring-gate beyond, and moon systems on
/// the gas giants (§17). Body **indices are load-bearing** — markets reference
/// them (Earth = 3, Mars = 4, Ceres = 5); keep planets first, then the gate, then
/// moons. Radii are real AU for planets; periods are real years (1 tick ≈ 1 hour).
pub fn default_system() -> Vec<Body> {
    use BodyKind::*;
    let mut bodies = vec![
        // 0: the star.
        Body {
            name: "Sol",
            parent: 0,
            orbit_radius: 0,
            period_ticks: 0,
            phase_mdeg: 0,
            kind: Star,
        },
        // 1–10: the planets (real AU radii, real periods).
        planet("Mercury", 387, 24, 30_000, Planet), // 1
        planet("Venus", 723, 62, 210_000, Planet),  // 2
        planet("Earth", 1000, 100, 0, Planet),      // 3
        planet("Mars", 1524, 188, 90_000, Planet),  // 4
        planet("Ceres", 2768, 459, 200_000, DwarfPlanet), // 5  (the Belt)
        planet("Jupiter", 5203, 1186, 300_000, GasGiant), // 6
        planet("Saturn", 9537, 2946, 140_000, GasGiant), // 7
        planet("Uranus", 19191, 8401, 250_000, GasGiant), // 8
        planet("Neptune", 30069, 16479, 60_000, GasGiant), // 9
        planet("Pluto", 39482, 24796, 330_000, DwarfPlanet), // 10
        // 11: the ring-gate, beyond Pluto, fixed (§0.1).
        Body {
            name: "Ring-Gate",
            parent: 0,
            orbit_radius: AU * 52,
            period_ticks: 0,
            phase_mdeg: 24_000,
            kind: Gate,
        },
        // 12+: moons (exaggerated radii for legibility, each on its own orbit).
        moon("Luna", 3, 220_000, 655, 0), // Earth
        moon("Phobos", 4, 110_000, 8, 0), // Mars
        moon("Deimos", 4, 175_000, 30, 180_000),
        moon("Io", 6, 230_000, 42, 0), // Jupiter
        moon("Europa", 6, 330_000, 85, 70_000),
        moon("Ganymede", 6, 460_000, 171, 160_000),
        moon("Callisto", 6, 640_000, 400, 250_000),
        // Saturn — the bustling system (§17): ~20 named moons on distinct orbits.
        moon("Pan", 7, 110_000, 14, 20_000),
        moon("Daphnis", 7, 125_000, 17, 250_000),
        moon("Atlas", 7, 140_000, 20, 130_000),
        moon("Prometheus", 7, 158_000, 24, 300_000),
        moon("Pandora", 7, 175_000, 27, 80_000),
        moon("Mimas", 7, 195_000, 33, 210_000),
        moon("Janus", 7, 212_000, 38, 340_000),
        moon("Epimetheus", 7, 228_000, 42, 160_000),
        moon("Enceladus", 7, 250_000, 49, 60_000),
        moon("Tethys", 7, 290_000, 63, 290_000),
        moon("Telesto", 7, 305_000, 67, 100_000),
        moon("Calypso", 7, 320_000, 71, 240_000),
        moon("Polydeuces", 7, 345_000, 80, 30_000),
        moon("Dione", 7, 365_000, 87, 200_000),
        moon("Helene", 7, 385_000, 92, 320_000),
        moon("Rhea", 7, 430_000, 120, 110_000),
        moon("Titan", 7, 560_000, 230, 0),
        moon("Hyperion", 7, 650_000, 290, 150_000),
        moon("Iapetus", 7, 820_000, 470, 250_000),
        moon("Phoebe", 7, 980_000, 590, 70_000),
        // Uranus / Neptune / Pluto.
        moon("Titania", 8, 260_000, 209, 0),
        moon("Oberon", 8, 380_000, 323, 150_000),
        moon("Triton", 9, 280_000, 141, 0),
        moon("Charon", 10, 110_000, 153, 0),
        // The asteroid belt's major bodies — the OPA heartland (the contested hubs).
        // Appended after the moons so every existing index (planets/gate/moons + the
        // markets/colonies that reference them) is unmoved; far-side bodies (pushed
        // below) resolve by name, so shifting them is safe. Dwarf bodies ⇒ mineable belt.
        planet("Vesta", 2362, 363, 40_000, DwarfPlanet),
        planet("Eros", 2150, 304, 160_000, DwarfPlanet),
        planet("Pallas", 2900, 494, 280_000, DwarfPlanet),
        planet("Tycho", 3100, 546, 330_000, DwarfPlanet),
    ];
    // ---- The far side of the gate (§17 endgame) ----
    // Appended last, so every existing index (planets/gate/moons, and the markets +
    // colonies that reference them) is unmoved. These bodies exist always for
    // determinism, but the shell only reveals them once the player has transited.
    // A cold dead star beyond the ring, with two worlds in its dark.
    let anchor = bodies.len();
    bodies.push(Body {
        name: "Erebus", // the lightless sun on the far side
        parent: 0,
        orbit_radius: AU * 66,
        period_ticks: 0,
        phase_mdeg: 205_000,
        kind: FarSide,
    });
    bodies.push(Body {
        name: "Threshold", // the bridgehead world
        parent: anchor,
        orbit_radius: AU * 2,
        period_ticks: 52_000,
        phase_mdeg: 0,
        kind: FarSide,
    });
    bodies.push(Body {
        name: "The Tally", // where the count is kept
        parent: anchor,
        orbit_radius: AU * 4,
        period_ticks: 128_000,
        phase_mdeg: 120_000,
        kind: FarSide,
    });
    bodies
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(name: &str) -> Body {
        default_system()
            .into_iter()
            .find(|b| b.name == name)
            .unwrap()
    }

    #[test]
    fn central_star_is_fixed_at_origin() {
        let bodies = default_system();
        assert_eq!(position_of(&bodies, 0, 0), (0, 0));
        assert_eq!(position_of(&bodies, 0, 9_999), (0, 0));
    }

    #[test]
    fn planets_stay_on_their_orbit_circle() {
        for body in default_system().iter().filter(|b| b.orbit_radius > 0) {
            let r2 = body.orbit_radius * body.orbit_radius;
            let step = (body.period_ticks / 50).max(1);
            for tick in (0..body.period_ticks).step_by(step as usize) {
                let (x, y) = body.position(tick);
                let d2 = x * x + y * y;
                assert!((d2 - r2).abs() * 100 < r2 * 2, "{} off-circle", body.name);
            }
        }
    }

    #[test]
    fn position_repeats_each_period() {
        let mars = find("Mars");
        assert_eq!(mars.position(0), mars.position(mars.period_ticks));
        assert_eq!(mars.position(5), mars.position(mars.period_ticks + 5));
    }

    #[test]
    fn moons_orbit_their_planet_not_the_sun() {
        // A moon's absolute position stays within its (exaggerated) orbit radius
        // of its parent planet — it tracks the planet across the system.
        let bodies = default_system();
        let titan = bodies.iter().position(|b| b.name == "Titan").unwrap();
        let saturn = bodies[titan].parent;
        assert_eq!(bodies[saturn].name, "Saturn");
        for &tick in &[0u64, 137, 5_000, 250_000] {
            let (mx, my) = position_of(&bodies, titan, tick);
            let (px, py) = position_of(&bodies, saturn, tick);
            let d2 = (mx - px) * (mx - px) + (my - py) * (my - py);
            let r = bodies[titan].orbit_radius;
            assert!(
                (d2 - r * r).abs() * 100 < r * r * 2,
                "Titan off Saturn-orbit"
            );
        }
    }

    #[test]
    fn the_gate_sits_beyond_pluto() {
        let pluto = find("Pluto").orbit_radius;
        let gate = find("Ring-Gate").orbit_radius;
        assert!(gate > pluto, "the gate is the far frontier (§0.1)");
    }

    #[test]
    fn the_far_side_lies_beyond_the_gate() {
        // §17: the far-side cluster is appended after the gate (so inner indices are
        // unmoved) and lies beyond the ring. Its worlds orbit a far-side anchor.
        let bodies = default_system();
        let gate = find("Ring-Gate").orbit_radius;
        let erebus = bodies.iter().position(|b| b.name == "Erebus").unwrap();
        assert!(
            erebus > 11,
            "the far side is appended after the gate (index 11)"
        );
        assert_eq!(bodies[erebus].kind, BodyKind::FarSide);
        assert!(
            bodies[erebus].orbit_radius > gate,
            "Erebus lies past the ring"
        );
        // Threshold + The Tally orbit the anchor and resolve to real positions.
        for name in ["Threshold", "The Tally"] {
            let i = bodies.iter().position(|b| b.name == name).unwrap();
            assert_eq!(
                bodies[i].parent, erebus,
                "{name} orbits the far-side anchor"
            );
            assert_ne!(position_of(&bodies, i, 5_000), (0, 0));
        }
        // The inner system's load-bearing indices are unchanged.
        assert_eq!(bodies[3].name, "Earth");
        assert_eq!(bodies[5].name, "Ceres");
        assert_eq!(bodies[11].name, "Ring-Gate");
    }

    #[test]
    fn the_system_is_deterministic() {
        // Same tick ⇒ same positions for every body, every time.
        let a = default_system();
        let b = default_system();
        for tick in [0u64, 1, 999, 123_456] {
            for i in 0..a.len() {
                assert_eq!(position_of(&a, i, tick), position_of(&b, i, tick));
            }
        }
    }
}
