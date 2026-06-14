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
/// Fraction of the stock error the NPC stabilizers correct each tick. Kept
/// **gentle** so trade and interdiction (§7b) visibly move the average; the hard
/// stock walls (not this spring) are what guarantee no death-spiral (§7c).
const STABILIZE_BP: i64 = 400;
/// Hard stock walls sit this fraction of `target_stock` inside `[0, max_stock]`,
/// keeping price strictly off its rails however hard trade/jitter push (§7c).
const WALL_MARGIN_DEN: i64 = 10;

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

/// A single market at a location (body): NPC industry that self-stabilizes
/// each commodity toward a **setpoint** stock, with prices damped toward the
/// stock-based target. The setpoint is decoupled from the price anchor
/// (`def.target_stock`), so a producer (setpoint in glut ⇒ cheap) and a consumer
/// (setpoint in scarcity ⇒ dear) reach *different* equilibrium prices — the
/// spread that drives arbitrage traffic (§7b).
#[derive(Clone, Debug)]
pub struct Market {
    name: &'static str,
    body: usize,
    defs: Vec<CommodityDef>,
    setpoints: Vec<i64>,
    stocks: Vec<Stock>,
}

impl Market {
    /// A neutral market: stabilizer setpoint == price anchor ⇒ prices at base.
    pub fn new(defs: Vec<CommodityDef>) -> Self {
        let setpoints = defs.iter().map(|d| d.target_stock).collect();
        Self::with_setpoints("Market", 0, defs, setpoints)
    }

    /// A market located at `body` whose per-commodity stabilizer setpoints set
    /// its equilibrium prices. Starts sitting at that equilibrium.
    pub fn with_setpoints(
        name: &'static str,
        body: usize,
        defs: Vec<CommodityDef>,
        setpoints: Vec<i64>,
    ) -> Self {
        let stocks = defs
            .iter()
            .zip(&setpoints)
            .map(|(d, &sp)| Stock {
                stock: sp,
                price: target_price(d, sp).clamp(d.floor, d.ceiling),
            })
            .collect();
        Self {
            name,
            body,
            defs,
            setpoints,
            stocks,
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn body(&self) -> usize {
        self.body
    }

    pub fn defs(&self) -> &[CommodityDef] {
        &self.defs
    }

    pub fn stocks(&self) -> &[Stock] {
        &self.stocks
    }

    /// Current price of commodity `c`.
    pub fn price(&self, c: usize) -> i64 {
        self.stocks[c].price
    }

    /// Current stock of commodity `c`.
    pub fn stock(&self, c: usize) -> i64 {
        self.stocks[c].stock
    }

    /// Low stock wall — kept above 0 so price never reaches its ceiling (§7c).
    pub fn wall_low(&self, c: usize) -> i64 {
        (self.defs[c].target_stock / WALL_MARGIN_DEN).max(1)
    }

    /// High stock wall — kept below `max_stock` so price never reaches its floor.
    pub fn wall_high(&self, c: usize) -> i64 {
        self.defs[c].max_stock - self.wall_low(c)
    }

    /// Land cargo at this market (a hauler delivery), repricing immediately.
    pub fn add_stock(&mut self, c: usize, qty: i64) {
        let (lo, hi) = (self.wall_low(c), self.wall_high(c));
        self.stocks[c].stock = (self.stocks[c].stock + qty).clamp(lo, hi);
        self.reprice(c);
    }

    /// Lift cargo from this market (a hauler loading), repricing immediately.
    pub fn remove_stock(&mut self, c: usize, qty: i64) {
        let (lo, hi) = (self.wall_low(c), self.wall_high(c));
        self.stocks[c].stock = (self.stocks[c].stock - qty).clamp(lo, hi);
        self.reprice(c);
    }

    /// Damp the price of commodity `c` one notch toward its stock-based target.
    fn reprice(&mut self, c: usize) {
        let def = &self.defs[c];
        let s = &mut self.stocks[c];
        let target = target_price(def, s.stock);
        let delta = target - s.price;
        let mut step = delta * PRICE_DAMP_BP / BP;
        if step == 0 && delta != 0 {
            step = delta.signum(); // never stall on integer truncation
        }
        s.price = (s.price + step).clamp(def.floor, def.ceiling);
    }

    /// Advance the market one tick: NPC stabilizers move stock toward its
    /// setpoint against demand noise, then prices damp toward the target.
    pub fn step(&mut self, rng: &mut Pcg32) {
        for c in 0..self.defs.len() {
            let (lo, hi) = (self.wall_low(c), self.wall_high(c));
            // NPC stabilizer: gentle proportional restoring toward the setpoint.
            let err = self.setpoints[c] - self.stocks[c].stock;
            let stabilize = err * STABILIZE_BP / BP;
            // Deterministic demand noise in [-jitter, jitter].
            let jit = self.defs[c].demand_jitter;
            let jitter = if jit > 0 {
                rng.below((2 * jit + 1) as u32) as i64 - jit
            } else {
                0
            };
            self.stocks[c].stock = (self.stocks[c].stock + stabilize - jitter).clamp(lo, hi);
            self.reprice(c);
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

/// Commodity indices that are *raw* (the first tier); the rest are *refined*.
const RAW: [usize; 3] = [0, 1, 2];

/// Two complementary markets (§4): a Belt producer (cheap raw / dear refined) at
/// Ceres and an inner consumer (dear raw / cheap refined) at Earth. The opposed
/// setpoints create a standing two-way price spread for arbitrage traffic (§7b).
pub fn default_markets() -> Vec<Market> {
    // Halfway into glut ⇒ ~(base+floor)/2 (surplus); halfway into scarcity ⇒
    // ~(base+ceiling)/2 (deficit). Both stay well within [floor, ceiling].
    let glut = |d: &CommodityDef| (d.target_stock + d.max_stock) / 2;
    let scarce = |d: &CommodityDef| d.target_stock / 2;
    let setpoints = |raw_cheap: bool, defs: &[CommodityDef]| -> Vec<i64> {
        defs.iter()
            .enumerate()
            .map(|(i, d)| {
                let cheap = RAW.contains(&i) == raw_cheap;
                if cheap {
                    glut(d)
                } else {
                    scarce(d)
                }
            })
            .collect()
    };
    let defs = default_commodities();
    let ceres = setpoints(true, &defs); // cheap raw, dear refined
    let earth = setpoints(false, &defs); // dear raw, cheap refined
    vec![
        Market::with_setpoints("Ceres Yards", 3, defs.clone(), ceres),
        Market::with_setpoints("Earth Hub", 1, defs, earth),
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
