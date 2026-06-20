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
/// damped prices. Carries a **reservation layer** so concurrent haulers don't oversubscribe a
/// trade: `reserved_out` is stock promised to outbound buyers, `reserved_in` is high-wall headroom
/// promised to inbound sellers. Reservations are a derived cache (rebuilt from in-flight jobs on
/// load), never serialized.
#[derive(Clone, Debug)]
pub struct Market {
    name: &'static str,
    body: usize,
    owner: PlayerId,
    defs: Vec<PriceDef>,
    setpoints: Vec<i64>,
    stocks: Vec<Stock>,
    reserved_out: Vec<i64>,
    reserved_in: Vec<i64>,
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
        let n = defs.len();
        Self {
            name,
            body,
            owner,
            defs,
            setpoints,
            stocks,
            reserved_out: vec![0; n],
            reserved_in: vec![0; n],
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
        // Reservations are a derived cache; a restored market starts with none (the caller
        // rebuilds them from in-flight jobs).
        for r in &mut self.reserved_out {
            *r = 0;
        }
        for r in &mut self.reserved_in {
            *r = 0;
        }
    }

    // ---- the reservation layer + trade API (§ iteration 3) ----------------------------

    /// Stock a buying hauler may still claim — above the low wall (the market keeps a floor
    /// inventory) and after outstanding reservations.
    pub fn available_to_buy(&self, c: usize) -> i64 {
        (self.stocks[c].stock - self.wall_low(c) - self.reserved_out[c]).max(0)
    }
    /// High-wall headroom a selling hauler may still claim (after inbound reservations), so a
    /// producer-hauler doesn't haul goods that would be clamped away on arrival.
    pub fn headroom_to_sell(&self, c: usize) -> i64 {
        (self.wall_high(c) - self.stocks[c].stock - self.reserved_in[c]).max(0)
    }
    pub fn reserved_out(&self, c: usize) -> i64 {
        self.reserved_out[c]
    }
    pub fn reserved_in(&self, c: usize) -> i64 {
        self.reserved_in[c]
    }
    pub fn reserve_buy(&mut self, c: usize, qty: i64) {
        self.reserved_out[c] += qty.max(0);
    }
    pub fn release_buy(&mut self, c: usize, qty: i64) {
        self.reserved_out[c] = (self.reserved_out[c] - qty.max(0)).max(0);
    }
    pub fn reserve_sell(&mut self, c: usize, qty: i64) {
        self.reserved_in[c] += qty.max(0);
    }
    pub fn release_sell(&mut self, c: usize, qty: i64) {
        self.reserved_in[c] = (self.reserved_in[c] - qty.max(0)).max(0);
    }

    /// Move `qty` of `c` out of the market (a buy): returns the cost (price sampled on the
    /// pre-trade stock) and removes the stock (reprices). The caller releases the reservation.
    pub fn execute_buy(&mut self, c: usize, qty: i64) -> i64 {
        let q = qty.max(0).min(self.stocks[c].stock);
        let cost = q * self.stocks[c].price;
        self.remove_stock(c, q);
        cost
    }
    /// Move `qty` of `c` into the market (a sell): returns the revenue (pre-trade price) and adds
    /// the stock (reprices). The caller releases the reservation.
    pub fn execute_sell(&mut self, c: usize, qty: i64) -> i64 {
        let q = qty.max(0);
        let revenue = q * self.stocks[c].price;
        self.add_stock(c, q);
        revenue
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

/// Per-good market role, setting the NPC-stabilizer setpoint (decoupled from the price anchor) so
/// each hub rests at a deliberately cheap or dear price — the standing spreads arbitrage feeds on.
#[derive(Clone, Copy)]
enum Role {
    /// Glut setpoint ⇒ stock deep above target ⇒ price near floor (cheap to buy here).
    Producer,
    /// Neutral setpoint ⇒ price at base.
    Mid,
    /// Scarcity setpoint (still strictly inside the low wall) ⇒ price near ceiling (dear).
    Consumer,
}

fn setpoint_for(d: &PriceDef, role: Role) -> i64 {
    match role {
        Role::Producer => d.target_stock + (d.max_stock - d.target_stock) * 6 / 10,
        Role::Mid => d.target_stock,
        // Scarce, but kept above wall_low (= target/10) so the setpoint is reachable in-band.
        Role::Consumer => (d.target_stock * 4 / 10).max(d.target_stock / 10 + 1),
    }
}

/// The default market layout: the three inner trading hubs, each specialized so standing spreads
/// exist for arbitrage (Ceres = raw/industrial producer, Earth = consumer, Mars = mixed). Player
/// ids follow `player::default_players()`: 1 Earth, 2 Mars, 3 OPA.
pub fn default_markets() -> Vec<Market> {
    use super::commodity::*;
    use Role::*;
    let defs = price_defs();
    // Build a setpoint vector from a per-good role table (index == commodity index).
    let setpoints = |roles: [Role; 7]| -> Vec<i64> {
        defs.iter()
            .enumerate()
            .map(|(c, d)| setpoint_for(d, roles[c]))
            .collect()
    };
    // [Ice, Ore, Rare, Alloys, FusionFuel, Electronics, Food]
    let _ = (ICE, ORE, RARE, ALLOYS, FUSION_FUEL, ELECTRONICS, FOOD);
    let earth = setpoints([Mid, Consumer, Mid, Consumer, Mid, Consumer, Consumer]);
    let mars = setpoints([Mid, Mid, Consumer, Producer, Mid, Mid, Consumer]);
    let ceres = setpoints([
        Producer, Producer, Producer, Producer, Producer, Consumer, Consumer,
    ]);
    vec![
        Market::with_setpoints("Earth Hub", 3, 1, defs.clone(), earth),
        Market::with_setpoints("Mars Colony", 4, 2, defs.clone(), mars),
        Market::with_setpoints("Ceres Yards", 5, 3, defs.clone(), ceres),
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
    fn specialized_markets_open_with_standing_spreads() {
        use super::super::commodity::ALLOYS;
        let m = default_markets();
        // m[0]=Earth (consumer), m[1]=Mars (Alloys producer), m[2]=Ceres (producer).
        let (earth, ceres) = (m[0].price(ALLOYS), m[2].price(ALLOYS));
        assert!(
            earth > ceres,
            "Alloys dear at Earth ({earth}) vs cheap at Ceres ({ceres}) — a standing spread"
        );
        // The spread persists when the markets run dry (stabilizers hold the setpoints).
        let mut markets = default_markets();
        let mut rng = Pcg32::new(5);
        for _ in 0..2_000 {
            for mk in &mut markets {
                mk.step(&mut rng);
            }
        }
        assert!(
            markets[0].price(ALLOYS) > markets[2].price(ALLOYS),
            "the spread survives the stabilizers"
        );
    }

    #[test]
    fn reservations_reduce_availability_and_execute_moves_stock() {
        let defs = price_defs();
        let sp: Vec<i64> = defs.iter().map(|d| d.target_stock).collect();
        let mut m = Market::with_setpoints("T", 0, 0, defs, sp);
        let c = 1; // Ore
        let stock0 = m.stock(c);
        let avail0 = m.available_to_buy(c);
        assert_eq!(
            avail0,
            stock0 - m.wall_low(c),
            "available is above the low wall"
        );
        // Reserve 100 to buy: availability drops, stock untouched.
        m.reserve_buy(c, 100);
        assert_eq!(m.available_to_buy(c), avail0 - 100);
        assert_eq!(m.stock(c), stock0);
        assert!(
            m.reserved_out(c) <= m.stock(c),
            "invariant: reserved ≤ stock"
        );
        // Execute the buy (cost = pre-trade price × qty), then the caller releases the reservation.
        let price = m.price(c);
        let cost = m.execute_buy(c, 100);
        m.release_buy(c, 100);
        assert_eq!(cost, 100 * price);
        assert_eq!(m.reserved_out(c), 0, "reservation released by the caller");
        assert!(m.stock(c) < stock0, "buy removed stock");
        assert_eq!(m.available_to_buy(c), m.stock(c) - m.wall_low(c));
        // Sell side: reserve_sell reduces headroom; execute_sell adds stock + revenue.
        let head0 = m.headroom_to_sell(c);
        m.reserve_sell(c, 50);
        assert_eq!(m.headroom_to_sell(c), head0 - 50);
        let s_before = m.stock(c);
        let rev = m.execute_sell(c, 50);
        m.release_sell(c, 50);
        assert!(rev > 0 && m.stock(c) > s_before);
        assert_eq!(m.reserved_in(c), 0);
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
}
