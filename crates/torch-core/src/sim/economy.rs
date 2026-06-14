//! Stockpile-simulation economy (§7a) with anti-death-spiral stabilizers (§7c).
//!
//! Markets hold inventory that fills/drains from NPC production/consumption;
//! price tracks stock. Everything is integer/fixed-point (§27) so a run is
//! bit-identical across platforms and the headless stability gate is meaningful.
//!
//! Design notes carried from the validated prototype:
//! - **Piecewise price target** so `stock == target ⇒ base_price`, sliding to the
//!   ceiling under scarcity and the floor under glut (not a band-midpoint map).
//! - **Damped** price (lerp toward the stock-based target, never raw supply÷demand).
//! - **NPC stabilizers**: production/consumption restore stock toward target, so a
//!   self-sufficient market reaches equilibrium with zero player input.

use super::rng::Pcg32;

/// Basis-point denominator (100% = 10000).
const BP: i64 = 10_000;
/// Fraction of the price→target gap closed each tick.
const PRICE_DAMP_BP: i64 = 2_000;
/// Fraction of the stock error the NPC stabilizers correct each tick.
const STABILIZE_BP: i64 = 2_000;

/// Static definition of a tradable commodity (the "numbers as data" of §31; held
/// in Rust for now, trivially movable to RON/JSON later).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommodityDef {
    pub name: &'static str,
    /// Reference price, reached when `stock == target_stock`.
    pub base_price: i64,
    /// Price floor (approached under maximum glut).
    pub floor: i64,
    /// Price ceiling (approached as stock → 0).
    pub ceiling: i64,
    /// Stock at which price equals `base_price`.
    pub target_stock: i64,
    /// Stock at which price reaches `floor` (the glut cap).
    pub max_stock: i64,
    /// Amplitude of deterministic per-tick demand noise.
    pub demand_jitter: i64,
}

/// Piecewise price target for a given stock level (§7a). Monotonically
/// non-increasing in stock: scarce ⇒ high, glut ⇒ low.
pub fn target_price(def: &CommodityDef, stock: i64) -> i64 {
    if stock <= 0 {
        return def.ceiling;
    }
    if stock >= def.max_stock {
        return def.floor;
    }
    if stock < def.target_stock {
        // Scarcity: lerp ceiling (stock 0) → base (stock target).
        def.ceiling + (def.base_price - def.ceiling) * stock / def.target_stock
    } else {
        // Glut: lerp base (stock target) → floor (stock max).
        let span = def.max_stock - def.target_stock;
        def.base_price + (def.floor - def.base_price) * (stock - def.target_stock) / span
    }
}

/// Live state of one commodity in a market.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stock {
    pub stock: i64,
    pub price: i64,
}

/// A single market: NPC industry that self-stabilizes around each commodity's
/// target stock, with prices damped toward the stock-based target.
#[derive(Clone, Debug)]
pub struct Market {
    defs: Vec<CommodityDef>,
    stocks: Vec<Stock>,
}

impl Market {
    /// Build a market sitting at equilibrium (stock at target, price at base).
    pub fn new(defs: Vec<CommodityDef>) -> Self {
        let stocks = defs
            .iter()
            .map(|d| Stock {
                stock: d.target_stock,
                price: d.base_price,
            })
            .collect();
        Self { defs, stocks }
    }

    pub fn defs(&self) -> &[CommodityDef] {
        &self.defs
    }

    pub fn stocks(&self) -> &[Stock] {
        &self.stocks
    }

    /// Advance the market one tick: NPC stabilizers move stock toward target
    /// against demand noise, then prices damp toward the stock-based target.
    pub fn step(&mut self, rng: &mut Pcg32) {
        for (def, s) in self.defs.iter().zip(self.stocks.iter_mut()) {
            // NPC stabilizer: proportional restoring flow toward target stock.
            let err = def.target_stock - s.stock;
            let stabilize = err * STABILIZE_BP / BP;
            // Deterministic demand noise in [-jitter, jitter].
            let jitter = if def.demand_jitter > 0 {
                rng.below((2 * def.demand_jitter + 1) as u32) as i64 - def.demand_jitter
            } else {
                0
            };
            let hard_cap = def.max_stock + def.target_stock; // generous bound
            s.stock = (s.stock + stabilize - jitter).clamp(0, hard_cap);

            // Damped move of price toward the stock-based target.
            let target = target_price(def, s.stock);
            let delta = target - s.price;
            let mut step = delta * PRICE_DAMP_BP / BP;
            if step == 0 && delta != 0 {
                step = delta.signum(); // never stall on integer truncation
            }
            s.price = (s.price + step).clamp(def.floor, def.ceiling);
        }
    }
}

/// Default inner-system commodity slice (§7d): raw → refined, illustrative.
pub fn default_commodities() -> Vec<CommodityDef> {
    // base floor ceil  target  max   jitter
    const fn c(
        name: &'static str,
        base_price: i64,
        floor: i64,
        ceiling: i64,
        target_stock: i64,
        max_stock: i64,
        demand_jitter: i64,
    ) -> CommodityDef {
        CommodityDef {
            name,
            base_price,
            floor,
            ceiling,
            target_stock,
            max_stock,
            demand_jitter,
        }
    }
    vec![
        c("Ice", 20, 8, 60, 1200, 2400, 35),
        c("Ore", 30, 12, 90, 1000, 2000, 30),
        c("Volatiles", 45, 18, 140, 800, 1600, 22),
        c("Remass", 70, 28, 210, 700, 1400, 18),
        c("Metals", 110, 44, 330, 600, 1200, 14),
        c("ReactorFuel", 180, 72, 540, 400, 800, 9),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Vec<CommodityDef> {
        default_commodities()
    }

    #[test]
    fn target_price_anchors_are_correct() {
        for d in defs() {
            assert_eq!(target_price(&d, d.target_stock), d.base_price); // anchor
            assert_eq!(target_price(&d, 0), d.ceiling); // scarcity
            assert_eq!(target_price(&d, d.max_stock), d.floor); // glut
            assert!(target_price(&d, d.target_stock / 2) > d.base_price); // scarcer ⇒ dearer
            let glut = (d.target_stock + d.max_stock) / 2;
            assert!(target_price(&d, glut) < d.base_price); // glut ⇒ cheaper
        }
    }

    #[test]
    fn target_price_is_monotonic_non_increasing() {
        for d in defs() {
            let mut prev = i64::MAX;
            for stock in (0..=d.max_stock).step_by((d.max_stock / 50).max(1) as usize) {
                let p = target_price(&d, stock);
                assert!(p <= prev, "{} not monotonic at stock {stock}", d.name);
                prev = p;
            }
        }
    }

    #[test]
    fn idle_market_starts_and_stays_at_equilibrium_band() {
        let mut m = Market::new(defs());
        let mut rng = Pcg32::new(7);
        for _ in 0..2_000 {
            m.step(&mut rng);
        }
        for (d, s) in m.defs().iter().zip(m.stocks()) {
            assert!(
                s.price > d.floor && s.price < d.ceiling,
                "{} pinned",
                d.name
            );
        }
    }

    /// The §7c acceptance gate: with **zero player input**, no market may
    /// death-spiral across thousands of ticks **on any seed** — stock never
    /// fully depletes or gluts, and price never pins to a rail.
    #[test]
    fn no_death_spiral_on_any_seed() {
        for seed in 0..64u64 {
            let mut m = Market::new(defs());
            let mut rng = Pcg32::new(seed);
            // Invariants accumulated as plain booleans in the hot loop; asserted
            // once at the end (per the prototype's performance learning).
            let mut ok = true;
            for _ in 0..5_000 {
                m.step(&mut rng);
                for (d, s) in m.defs().iter().zip(m.stocks()) {
                    ok &= s.stock > 0 && s.stock < d.max_stock + d.target_stock;
                    ok &= s.price > d.floor && s.price < d.ceiling;
                }
            }
            assert!(ok, "death-spiral detected on seed {seed}");
        }
    }
}
