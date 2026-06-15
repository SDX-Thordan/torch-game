//! Interception geometry and odds (§7b) — the verb on top of the traffic engine.
//!
//! Cutting a hauler is no longer a guaranteed delete: an interceptor must have
//! the *legs* to reach the hauler somewhere on its remaining path (a real, if
//! simplified, pursuit solution), and then *win the roll* — a hit chance scaled
//! by its speed margin and crew quality. The same resolution serves the player's
//! frigate (the best interdiction platform, §8b) and NPC pirates (§13).

use super::rng::Pcg32;
use super::traffic::Hauler;

/// Basis-point denominator.
const BP: i64 = 10_000;
/// How many points along the remaining path we test for a firing solution.
const SAMPLES: u64 = 12;
/// Base hit chance once a firing solution exists, in basis points.
const BASE_HIT_BP: i64 = 4_500;
/// Hit-chance ceiling, so nothing is ever a certainty.
const MAX_HIT_BP: i64 = 9_500;
/// Fraction of the speed-margin (in bp) that converts into extra hit chance.
const MARGIN_GAIN_DEN: i64 = 4;

/// A unit attempting an interception: where it starts, how fast it burns, and a
/// crew-quality bonus to the hit roll (basis points).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Interceptor {
    pub pos: (i64, i64),
    pub speed: i64,
    pub skill_bp: i64,
}

/// The outcome of an interception attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interdiction {
    /// The hauler was cut; its delivery is denied (a shortage at the destination).
    Interdicted,
    /// A firing solution existed but the roll failed — the hauler runs the gap.
    Escaped,
    /// The interceptor lacked the legs to reach the hauler at all.
    NoSolution,
}

/// Minimum interceptor speed that can reach the hauler somewhere on its
/// remaining path (always ≥ 1), or `None` if it has effectively arrived.
pub fn required_speed(h: &Hauler, from: (i64, i64), now: u64) -> Option<i64> {
    if now >= h.arrival_tick {
        return None;
    }
    let span = h.arrival_tick - now;
    let steps = SAMPLES.min(span);
    let mut best: Option<i64> = None;
    for i in 1..=steps {
        let t = now + span * i / steps;
        let (hx, hy) = h.position(t);
        let (fx, fy) = from;
        let dist = ((hx - fx) * (hx - fx) + (hy - fy) * (hy - fy)).isqrt();
        let dt = (t - now) as i64;
        let req = ((dist + dt - 1) / dt).max(1); // ceil, never zero
        best = Some(best.map_or(req, |b| b.min(req)));
    }
    best
}

/// Resolve an interception attempt deterministically (geometry, then a roll).
pub fn resolve(h: &Hauler, intc: &Interceptor, now: u64, rng: &mut Pcg32) -> Interdiction {
    let Some(req) = required_speed(h, intc.pos, now) else {
        return Interdiction::NoSolution;
    };
    if intc.speed < req {
        return Interdiction::NoSolution;
    }
    let margin_bp = (intc.speed - req) * BP / req;
    let chance = (BASE_HIT_BP + intc.skill_bp + margin_bp / MARGIN_GAIN_DEN).clamp(0, MAX_HIT_BP);
    if rng.chance_bp(chance as u32) {
        Interdiction::Interdicted
    } else {
        Interdiction::Escaped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hauler() -> Hauler {
        Hauler {
            id: 1,
            commodity: 0,
            origin: 0,
            dest: 1,
            qty: 100,
            depart_tick: 0,
            arrival_tick: 100,
            origin_pos: (0, 0),
            dest_pos: (100_000, 0),
        }
    }

    #[test]
    fn no_solution_when_too_slow_or_too_far() {
        let h = hauler();
        let crawler = Interceptor {
            pos: (5_000_000, 5_000_000),
            speed: 1,
            skill_bp: 0,
        };
        let mut rng = Pcg32::new(1);
        assert_eq!(resolve(&h, &crawler, 0, &mut rng), Interdiction::NoSolution);
    }

    #[test]
    fn no_solution_after_arrival() {
        let h = hauler();
        assert_eq!(required_speed(&h, (0, 0), 100), None);
    }

    #[test]
    fn a_fast_interceptor_on_the_path_gets_a_solution() {
        // Sitting on the route ahead of the hauler with ample speed: never a
        // NoSolution, and across seeds it lands the cut a healthy share of time.
        let h = hauler();
        let chaser = Interceptor {
            pos: (50_000, 0),
            speed: 50_000,
            skill_bp: 0,
        };
        let mut hits = 0;
        for seed in 0..200u64 {
            let mut rng = Pcg32::new(seed);
            match resolve(&h, &chaser, 0, &mut rng) {
                Interdiction::NoSolution => panic!("a solution should exist"),
                Interdiction::Interdicted => hits += 1,
                Interdiction::Escaped => {}
            }
        }
        assert!(
            hits > 100,
            "expected many hits with a big speed margin, got {hits}"
        );
    }

    #[test]
    fn required_speed_drops_with_more_lead_time() {
        // The same chase is easier (lower required speed) earlier in the flight.
        let h = hauler();
        let early = required_speed(&h, (0, 0), 0).unwrap();
        let late = required_speed(&h, (0, 0), 90).unwrap();
        assert!(
            early < late,
            "early {early} should need less speed than late {late}"
        );
    }
}
