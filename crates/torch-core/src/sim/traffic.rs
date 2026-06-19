//! Physical, interceptable traffic (§7b) — the featured emergent mechanic.
//!
//! NPC haulers fly representative real routes between markets, driven by price
//! arbitrage (cheapest surplus → dearest with room). A delivery lands cargo at
//! the destination and *damps* the spread, so trade is stabilizing. Cutting a
//! hauler in flight (interdiction, §7b) denies that delivery and leaves a local,
//! temporary shortage that visibly moves prices. Spawn/route logic lives in the
//! `Sim`, which holds the markets and orrery; this defines the hauler itself.

/// A hauler in transit between two markets carrying one commodity. Endpoints are
/// sampled at departure (a straight-line stub; real intercept geometry later).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hauler {
    pub id: u64,
    pub commodity: usize,
    pub origin: usize,
    pub dest: usize,
    pub qty: i64,
    pub depart_tick: u64,
    pub arrival_tick: u64,
    pub origin_pos: (i64, i64),
    pub dest_pos: (i64, i64),
}

impl Hauler {
    /// Position at `tick`, clamped to the arrival endpoint, following a **flip-and-burn**
    /// (brachistochrone) profile: accelerate to the midpoint, flip, decelerate to rest — so
    /// the hauler eases out of the origin, peaks at mid-flight, and brakes into the dest,
    /// instead of gliding at a flat speed. Distance fraction of elapsed fraction `f = t/T`:
    /// `2·f²` for the first half, `1 − 2·(1−f)²` for the braking half (0→0.5→1 at f=0,½,1).
    pub fn position(&self, tick: u64) -> (i64, i64) {
        let span = (self.arrival_tick - self.depart_tick).max(1) as i64;
        let t = (tick.clamp(self.depart_tick, self.arrival_tick) - self.depart_tick) as i64;
        let (ox, oy) = self.origin_pos;
        let (dx, dy) = self.dest_pos;
        // Fixed-point distance fraction = num / (span·span), brachistochrone-shaped.
        let den = span * span;
        let num = if 2 * t <= span {
            2 * t * t
        } else {
            den - 2 * (span - t) * (span - t)
        };
        (ox + (dx - ox) * num / den, oy + (dy - oy) * num / den)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> Hauler {
        Hauler {
            id: 1,
            commodity: 0,
            origin: 0,
            dest: 1,
            qty: 100,
            depart_tick: 10,
            arrival_tick: 110,
            origin_pos: (0, 0),
            dest_pos: (1000, 2000),
        }
    }

    #[test]
    fn position_interpolates_endpoints() {
        let h = h();
        assert_eq!(h.position(10), (0, 0)); // at departure
        assert_eq!(h.position(60), (500, 1000)); // halfway
        assert_eq!(h.position(110), (1000, 2000)); // at arrival
    }

    #[test]
    fn position_clamps_outside_flight_window() {
        let h = h();
        assert_eq!(h.position(0), (0, 0)); // before departure
        assert_eq!(h.position(999), (1000, 2000)); // after arrival
    }
}
