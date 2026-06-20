//! Stockpile-simulation economy (§7a) with anti-death-spiral stabilizers (§7c).
//!
//! Markets hold inventory that fills/drains from NPC production/consumption; price tracks
//! stock. Everything is integer/fixed-point (§27) so a run is bit-identical. The pricing
//! engine is carried verbatim from the validated prototype; only the goods set + the market
//! layout (now owned by a [`PlayerId`]) changed in the multi-player rebuild.

use super::player::PlayerId;
use super::rng::Pcg32;

const BP: i64 = 10_000;
const PRICE_DAMP_BP: i64 = 2_000;
const STABILIZE_BP: i64 = 400;
const WALL_MARGIN_DEN: i64 = 10;

/// The pricing parameters of one good (the "numbers as data" of §31), one entry per catalog
/// good in [`super::commodity::commodities`] order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriceDef {
    pub name: &'static str,
    pub base_price: i64,
    pub floor: i64,
    pub ceiling: i64,
    pub target_stock: i64,
    pub max_stock: i64,
    pub demand_jitter: i64,
}

/// Pricing params per catalog good (same order/length as `commodity::commodities()`).
pub fn price_defs() -> Vec<PriceDef> {
    // name              base  floor ceil  target  max   jitter
    let row = |name, base_price, floor, ceiling, target_stock, max_stock, demand_jitter| PriceDef {
        name,
        base_price,
        floor,
        ceiling,
        target_stock,
        max_stock,
        demand_jitter,
    };
    vec![
        row("Ice", 40, 20, 120, 800, 2_000, 4),
        row("Ore", 50, 25, 150, 800, 2_000, 4),
        row("Rare Materials", 120, 60, 360, 500, 1_500, 3),
        row("Alloys", 150, 80, 400, 600, 1_600, 4),
        row("Fusion Fuel", 110, 60, 300, 600, 1_600, 3),
        row("Electronics", 300, 160, 800, 400, 1_200, 2),
        row("Food", 70, 35, 200, 700, 1_800, 3),
    ]
}

/// Piecewise price target for a stock level (§7a) — monotonically non-increasing in stock.
pub fn target_price(def: &PriceDef, stock: i64) -> i64 {
    if stock <= 0 {
        return def.ceiling;
    }
    if stock >= def.max_stock {
        return def.floor;
    }
    if stock < def.target_stock {
        def.ceiling + (def.base_price - def.ceiling) * stock / def.target_stock
    } else {
        let span = def.max_stock - def.target_stock;
        def.base_price + (def.floor - def.base_price) * (stock - def.target_stock) / span
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stock {
    pub stock: i64,
    pub price: i64,
}

/// A market at a body, owned by a player; self-stabilizes each good toward a setpoint with
/// damped prices.
#[derive(Clone, Debug)]
pub struct Market {
    name: &'static str,
    body: usize,
    owner: PlayerId,
    defs: Vec<PriceDef>,
    setpoints: Vec<i64>,
    stocks: Vec<Stock>,
}

impl Market {
    pub fn with_setpoints(
        name: &'static str,
        body: usize,
        owner: PlayerId,
        defs: Vec<PriceDef>,
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
            owner,
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
    pub fn owner(&self) -> PlayerId {
        self.owner
    }
    pub fn defs(&self) -> &[PriceDef] {
        &self.defs
    }
    pub fn stocks(&self) -> &[Stock] {
        &self.stocks
    }
    pub fn price(&self, c: usize) -> i64 {
        self.stocks[c].price
    }
    pub fn stock(&self, c: usize) -> i64 {
        self.stocks[c].stock
    }
    pub fn wall_low(&self, c: usize) -> i64 {
        (self.defs[c].target_stock / WALL_MARGIN_DEN).max(1)
    }
    pub fn wall_high(&self, c: usize) -> i64 {
        self.defs[c].max_stock - self.wall_low(c)
    }
    pub fn add_stock(&mut self, c: usize, qty: i64) {
        let (lo, hi) = (self.wall_low(c), self.wall_high(c));
        self.stocks[c].stock = (self.stocks[c].stock + qty).clamp(lo, hi);
        self.reprice(c);
    }
    pub fn remove_stock(&mut self, c: usize, qty: i64) {
        let (lo, hi) = (self.wall_low(c), self.wall_high(c));
        self.stocks[c].stock = (self.stocks[c].stock - qty).clamp(lo, hi);
        self.reprice(c);
    }
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
            step = delta.signum();
        }
        s.price = (s.price + step).clamp(def.floor, def.ceiling);
    }
    /// Advance one tick: NPC stabilizers move stock toward setpoint against demand noise, then
    /// prices damp toward target. The only RNG touch in the sim's tick.
    pub fn step(&mut self, rng: &mut Pcg32) {
        for c in 0..self.defs.len() {
            let (lo, hi) = (self.wall_low(c), self.wall_high(c));
            let err = self.setpoints[c] - self.stocks[c].stock;
            let stabilize = err * STABILIZE_BP / BP;
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

/// The default market layout: the three inner trading hubs, each owned by its power.
/// Player ids follow `player::default_players()` order: 1 Earth, 2 Mars, 3 OPA.
pub fn default_markets() -> Vec<Market> {
    let defs = price_defs();
    let neutral: Vec<i64> = defs.iter().map(|d| d.target_stock).collect();
    vec![
        Market::with_setpoints("Earth Hub", 3, 1, defs.clone(), neutral.clone()),
        Market::with_setpoints("Mars Colony", 4, 2, defs.clone(), neutral.clone()),
        Market::with_setpoints("Ceres Yards", 5, 3, defs.clone(), neutral.clone()),
    ]
}

// ---- infinite demand sinks (§ rework) ------------------------------------------------

/// An **infinite demand sink**: a market point that buys any quantity of `commodity` at
/// `price`, giving production (theoretically infinite) an outlet so stockpiles never saturate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sink {
    pub body: usize,
    pub commodity: usize,
    pub price: i64,
}

/// The sink catalog — **Alloys at Ceres/Earth/Mars**, plus a sink for every other good, so
/// nothing is a dead end. Extend by appending rows (geography is recorded for when ship
/// movement lands; absorption is owner-stockpile based for now).
pub fn sinks() -> Vec<Sink> {
    use super::commodity::*;
    let (ceres, earth, mars) = (5usize, 3usize, 4usize);
    vec![
        // Alloys — the headline triple sink.
        Sink {
            body: ceres,
            commodity: ALLOYS,
            price: 140,
        },
        Sink {
            body: earth,
            commodity: ALLOYS,
            price: 150,
        },
        Sink {
            body: mars,
            commodity: ALLOYS,
            price: 145,
        },
        // One sink per remaining good so production always has an outlet.
        Sink {
            body: ceres,
            commodity: ICE,
            price: 35,
        },
        Sink {
            body: earth,
            commodity: ORE,
            price: 45,
        },
        Sink {
            body: mars,
            commodity: RARE,
            price: 110,
        },
        Sink {
            body: earth,
            commodity: FUSION_FUEL,
            price: 100,
        },
        Sink {
            body: earth,
            commodity: ELECTRONICS,
            price: 280,
        },
        Sink {
            body: mars,
            commodity: FOOD,
            price: 65,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::super::commodity::commodity_count;
    use super::*;

    #[test]
    fn price_defs_cover_every_good() {
        assert_eq!(price_defs().len(), commodity_count());
    }

    #[test]
    fn target_price_anchors_at_base_and_is_monotonic() {
        for d in price_defs() {
            assert_eq!(target_price(&d, d.target_stock), d.base_price);
            assert_eq!(target_price(&d, 0), d.ceiling);
            assert_eq!(target_price(&d, d.max_stock), d.floor);
            // Monotonically non-increasing in stock.
            let mut prev = i64::MAX;
            for s in (0..=d.max_stock).step_by((d.max_stock / 20).max(1) as usize) {
                let p = target_price(&d, s);
                assert!(p <= prev);
                prev = p;
            }
        }
    }

    #[test]
    fn no_death_spiral_on_any_seed() {
        // Markets left to run never pin price to a rail (the §7c guarantee), on many seeds.
        let mut bad = false;
        for seed in 0..64u64 {
            let mut rng = Pcg32::new(seed);
            let mut markets = default_markets();
            for _ in 0..2_000 {
                for m in &mut markets {
                    m.step(&mut rng);
                }
            }
            for m in &markets {
                for c in 0..price_defs().len() {
                    let d = &price_defs()[c];
                    if m.price(c) <= d.floor || m.price(c) >= d.ceiling {
                        bad = true;
                    }
                }
            }
        }
        assert!(!bad, "no market pins price to a rail");
    }

    #[test]
    fn every_good_has_a_sink() {
        let goods: std::collections::HashSet<usize> = sinks().iter().map(|s| s.commodity).collect();
        for c in 0..commodity_count() {
            assert!(
                goods.contains(&c),
                "good {c} has no sink — production would dead-end"
            );
        }
    }
}
