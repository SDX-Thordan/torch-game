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

use super::faction::Faction;
use super::rng::Pcg32;
use serde::Deserialize;

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
    faction: Faction,
    defs: Vec<CommodityDef>,
    setpoints: Vec<i64>,
    stocks: Vec<Stock>,
}

impl Market {
    /// A neutral independent market: setpoint == price anchor ⇒ prices at base.
    pub fn new(defs: Vec<CommodityDef>) -> Self {
        let setpoints = defs.iter().map(|d| d.target_stock).collect();
        Self::with_setpoints("Market", 0, Faction::Independents, defs, setpoints)
    }

    /// A market located at `body`, owned by `faction`, whose per-commodity
    /// stabilizer setpoints set its equilibrium prices. Starts at equilibrium.
    pub fn with_setpoints(
        name: &'static str,
        body: usize,
        faction: Faction,
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
            faction,
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

    /// The faction that owns this market (§4).
    pub fn faction(&self) -> Faction {
        self.faction
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
    /// Overwrite live stock + price per commodity from a loaded save (§30),
    /// clamped into each commodity's walls. Setpoints/defs are rebuilt from code,
    /// so only the dynamic stock/price pair is restored. Touches no RNG.
    pub fn restore_stocks(&mut self, stocks: &[i64], prices: &[i64]) {
        for c in 0..self.stocks.len().min(stocks.len()).min(prices.len()) {
            let def = &self.defs[c];
            self.stocks[c].stock = stocks[c].clamp(0, def.max_stock);
            self.stocks[c].price = prices[c].clamp(def.floor, def.ceiling);
        }
    }

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

    /// Hot-reload new commodity numbers onto this live market (§31). The set and
    /// order are fixed by code (names must match the current defs one-for-one), so
    /// only the *numbers* change; live stock and setpoints are re-clamped into the
    /// new walls and prices re-damp toward the new targets next reprice. Touches no
    /// RNG, so a reload keeps the run deterministic.
    pub fn retune(&mut self, defs: &[CommodityDef]) -> Result<(), String> {
        if defs.len() != self.defs.len() {
            return Err(format!(
                "commodity count changed ({} → {}); the set is code-defined",
                self.defs.len(),
                defs.len()
            ));
        }
        for (old, new) in self.defs.iter().zip(defs) {
            if old.name != new.name {
                return Err(format!(
                    "commodity name changed ({} → {})",
                    old.name, new.name
                ));
            }
        }
        self.defs = defs.to_vec();
        for c in 0..self.defs.len() {
            let (lo, hi) = (self.wall_low(c), self.wall_high(c));
            self.setpoints[c] = self.setpoints[c].clamp(lo, hi);
            self.stocks[c].stock = self.stocks[c].stock.clamp(lo, hi);
            self.stocks[c].price = self.stocks[c]
                .price
                .clamp(self.defs[c].floor, self.defs[c].ceiling);
            self.reprice(c);
        }
        Ok(())
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

/// One commodity's tunable numbers as loaded from data (§31). Identity (`name`)
/// matches a compiled commodity; the rest overrides its numbers. Deserialized
/// from JSON; unknown fields are ignored so the data file can carry comments.
#[derive(Clone, Debug, Deserialize)]
pub struct CommodityTuning {
    pub name: String,
    pub base_price: i64,
    pub floor: i64,
    pub ceiling: i64,
    pub target_stock: i64,
    pub max_stock: i64,
    pub demand_jitter: i64,
}

/// Top-level shape of `data/commodities.json` (a `commodities` array; any other
/// keys — e.g. `_comment` — are ignored).
#[derive(Debug, Deserialize)]
struct CommodityFile {
    commodities: Vec<CommodityTuning>,
}

/// The numbers shipped in-tree, embedded so a default build needs no filesystem.
/// `data_file_matches_compiled_defaults` proves it reproduces `default_commodities`.
pub const DEFAULT_COMMODITY_JSON: &str = include_str!("../../data/commodities.json");

/// Parse a commodity-tuning document (§31). Returns the listed overrides, or a
/// human-readable error (the shell surfaces it instead of crashing on a typo).
pub fn parse_tuning(json: &str) -> Result<Vec<CommodityTuning>, String> {
    serde_json::from_str::<CommodityFile>(json)
        .map(|f| f.commodities)
        .map_err(|e| format!("invalid commodity data: {e}"))
}

/// Overlay tuning numbers onto a commodity set, matching by name. Partial files
/// are allowed (override only what's listed); an entry naming no compiled
/// commodity is an error (typo protection) — the set itself stays code-defined.
pub fn apply_tuning(defs: &mut [CommodityDef], tunings: &[CommodityTuning]) -> Result<(), String> {
    for t in tunings {
        let def = defs
            .iter_mut()
            .find(|d| d.name == t.name)
            .ok_or_else(|| format!("unknown commodity '{}'", t.name))?;
        def.base_price = t.base_price;
        def.floor = t.floor;
        def.ceiling = t.ceiling;
        def.target_stock = t.target_stock;
        def.max_stock = t.max_stock;
        def.demand_jitter = t.demand_jitter;
    }
    Ok(())
}

/// The compiled commodity set with a JSON tuning overlay applied (§31): code owns
/// identity + recipe order, data owns the numbers.
pub fn tuned_commodities(json: &str) -> Result<Vec<CommodityDef>, String> {
    let mut defs = default_commodities();
    let tunings = parse_tuning(json)?;
    apply_tuning(&mut defs, &tunings)?;
    Ok(defs)
}

/// Commodity indices that are *raw* (the first tier); the rest are *refined*.
const RAW: [usize; 3] = [0, 1, 2];

/// Two complementary markets (§4): a Belt producer (cheap raw / dear refined) at
/// Ceres and an inner consumer (dear raw / cheap refined) at Earth. The opposed
/// setpoints create a standing two-way price spread for arbitrage traffic (§7b).
pub fn default_markets() -> Vec<Market> {
    markets_from_defs(default_commodities())
}

/// The two default markets built over an arbitrary commodity set — the shared
/// construction `default_markets` and the tuned variant both use, so a JSON
/// overlay (§31) flows straight into market setup.
pub fn markets_from_defs(defs: Vec<CommodityDef>) -> Vec<Market> {
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
    let ceres = setpoints(true, &defs); // cheap raw, dear refined
    let earth = setpoints(false, &defs); // dear raw, cheap refined
    vec![
        Market::with_setpoints("Ceres Yards", 3, Faction::Belt, defs.clone(), ceres),
        Market::with_setpoints("Earth Hub", 1, Faction::Earth, defs, earth),
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

    /// The shipped data file (§31) must reproduce the compiled defaults exactly —
    /// this keeps `data/commodities.json` and `default_commodities` in lockstep and
    /// documents the format by example. If you change one, change the other.
    #[test]
    fn data_file_matches_compiled_defaults() {
        let from_data = tuned_commodities(DEFAULT_COMMODITY_JSON)
            .expect("shipped commodity data should parse and overlay cleanly");
        assert_eq!(from_data, default_commodities());
    }

    #[test]
    fn tuning_overlays_only_listed_numbers_and_rejects_typos() {
        // A partial file overrides just one commodity's numbers; the rest default.
        let json = r#"{ "commodities": [
            { "name": "Ore", "base_price": 99, "floor": 40, "ceiling": 400,
              "target_stock": 500, "max_stock": 1000, "demand_jitter": 5 } ] }"#;
        let defs = tuned_commodities(json).unwrap();
        let ore = defs.iter().find(|d| d.name == "Ore").unwrap();
        assert_eq!(ore.base_price, 99);
        assert_eq!(ore.ceiling, 400);
        // Untouched commodity keeps its compiled numbers.
        let ice = defs.iter().find(|d| d.name == "Ice").unwrap();
        assert_eq!(ice, &default_commodities()[0]);

        // A typo'd commodity name is rejected (the set is code-defined).
        let bad = r#"{ "commodities": [
            { "name": "Orre", "base_price": 1, "floor": 1, "ceiling": 2,
              "target_stock": 1, "max_stock": 2, "demand_jitter": 0 } ] }"#;
        assert!(tuned_commodities(bad)
            .unwrap_err()
            .contains("unknown commodity"));
        // Malformed JSON is reported, not panicked.
        assert!(parse_tuning("{ not json").is_err());
    }

    /// Retuning a live market keeps it consistent (prices stay off the rails) and
    /// the new numbers take effect — and the same retune on two markets stepped
    /// with the same seed stays bit-identical (no RNG touched), so hot-reload is
    /// deterministic.
    #[test]
    fn retune_takes_effect_and_stays_deterministic() {
        let dearer = tuned_commodities(
            r#"{ "commodities": [
                { "name": "Ore", "base_price": 250, "floor": 100, "ceiling": 900,
                  "target_stock": 1000, "max_stock": 2000, "demand_jitter": 30 } ] }"#,
        )
        .unwrap();

        let mut a = Market::new(defs());
        let mut b = Market::new(defs());
        a.retune(&dearer).unwrap();
        b.retune(&dearer).unwrap();

        let ore = 1;
        let (mut ra, mut rb) = (Pcg32::new(5), Pcg32::new(5));
        for _ in 0..1_000 {
            a.step(&mut ra);
            b.step(&mut rb);
            assert_eq!(a.stocks(), b.stocks(), "retune must be deterministic");
            assert!(
                a.price(ore) > a.defs()[ore].floor && a.price(ore) < a.defs()[ore].ceiling,
                "retuned market pinned"
            );
        }
        // The dearer floor lifted the equilibrium price above the old base (30).
        assert!(a.price(ore) > 30, "retune did not change the price level");
    }

    #[test]
    fn retune_rejects_a_changed_set() {
        let mut m = Market::new(defs());
        // Drop a commodity → wrong count.
        let mut short = defs();
        short.pop();
        assert!(m.retune(&short).is_err());
        // Rename one → identity mismatch.
        let mut renamed = defs();
        renamed[0].name = "NotIce";
        assert!(m.retune(&renamed).is_err());
    }
}
