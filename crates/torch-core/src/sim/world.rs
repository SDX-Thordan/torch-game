//! The authoritative world and the sim↔view contract (§27, §28, §29).
//!
//! [`Sim`] owns the truth and advances on a fixed tick. The view never reads
//! `Sim` directly: it consumes a [`Snapshot`] (current state to render) plus the
//! [`Event`] stream returned by [`Sim::step`] (what changed). This is the seam
//! the Godot shell binds to and that keeps the core headless and testable.

use super::economy::{default_markets, Market};
use super::event::Event;
use super::orbit::{default_system, Body};
use super::rng::Pcg32;
use super::traffic::Hauler;

/// Ticks between hauler spawn attempts (≈ one per day at 1 tick/hour).
const SPAWN_INTERVAL: u64 = 24;
/// Cap on concurrent in-flight haulers.
const MAX_HAULERS: usize = 8;
/// Minimum price spread that makes a route worth flying.
const MIN_SPREAD: i64 = 5;
/// Hauler cruise speed in distance units per tick.
const CRUISE_SPEED: i64 = 20_000;
/// Floor on travel time so close markets still take real time (§21).
const MIN_TRAVEL: u64 = 24;

/// A renderable view of one body at a single tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyState {
    pub name: &'static str,
    pub x: i64,
    pub y: i64,
}

/// A renderable view of one commodity in a market.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommodityState {
    pub name: &'static str,
    pub stock: i64,
    pub price: i64,
}

/// A renderable view of one market.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketState {
    pub name: &'static str,
    pub commodities: Vec<CommodityState>,
}

/// A renderable view of one hauler in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HaulerState {
    pub id: u64,
    pub commodity: &'static str,
    pub x: i64,
    pub y: i64,
}

/// An immutable snapshot of the world for rendering (§29).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub tick: u64,
    pub bodies: Vec<BodyState>,
    pub markets: Vec<MarketState>,
    pub haulers: Vec<HaulerState>,
}

/// The authoritative deterministic simulation.
pub struct Sim {
    tick: u64,
    bodies: Vec<Body>,
    markets: Vec<Market>,
    haulers: Vec<Hauler>,
    next_hauler_id: u64,
    rng: Pcg32,
    events: Vec<Event>,
}

impl Sim {
    /// Create a sim seeded for determinism (§27). Same seed ⇒ same run.
    pub fn new(seed: u64) -> Self {
        Self {
            tick: 0,
            bodies: default_system(),
            markets: default_markets(),
            haulers: Vec::new(),
            next_hauler_id: 0,
            rng: Pcg32::new(seed),
            events: Vec::new(),
        }
    }

    /// The current tick.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The bodies under simulation.
    pub fn bodies(&self) -> &[Body] {
        &self.bodies
    }

    /// The markets (§7a).
    pub fn markets(&self) -> &[Market] {
        &self.markets
    }

    /// The haulers currently in flight (§7b).
    pub fn haulers(&self) -> &[Hauler] {
        &self.haulers
    }

    /// The shared deterministic RNG every system draws from (§27).
    pub fn rng_mut(&mut self) -> &mut Pcg32 {
        &mut self.rng
    }

    /// Advance exactly one fixed sim tick (§28) and return the events produced.
    /// The returned slice is valid until the next call to `step`.
    pub fn step(&mut self) -> &[Event] {
        self.tick += 1;
        self.events.clear();
        for m in self.markets.iter_mut() {
            m.step(&mut self.rng);
        }
        self.deliver_arrivals();
        self.spawn_traffic();
        self.events.push(Event::Tick { tick: self.tick });
        &self.events
    }

    /// Interdict the in-flight hauler with `id` (§7b): it is removed and its
    /// delivery denied, leaving the destination short. Returns whether a hauler
    /// was actually cut.
    pub fn interdict(&mut self, id: u64) -> bool {
        if let Some(i) = self.haulers.iter().position(|h| h.id == id) {
            self.haulers.remove(i);
            self.events.push(Event::HaulerInterdicted { id });
            true
        } else {
            false
        }
    }

    /// Land cargo for any hauler arriving this tick, damping the spread.
    fn deliver_arrivals(&mut self) {
        let tick = self.tick;
        let mut landed: Vec<(usize, usize, i64, u64)> = Vec::new();
        self.haulers.retain(|h| {
            if h.arrival_tick == tick {
                landed.push((h.dest, h.commodity, h.qty, h.id));
                false
            } else {
                true
            }
        });
        for (dest, commodity, qty, id) in landed {
            self.markets[dest].add_stock(commodity, qty);
            self.events.push(Event::HaulerArrived { id });
        }
    }

    /// Spawn at most one arbitrage hauler on the most profitable open route.
    fn spawn_traffic(&mut self) {
        if !self.tick.is_multiple_of(SPAWN_INTERVAL) || self.haulers.len() >= MAX_HAULERS {
            return;
        }
        let Some((commodity, origin, dest, qty)) = self.best_route() else {
            return;
        };
        // Lift the cargo now (origin sheds surplus); land it on arrival.
        self.markets[origin].remove_stock(commodity, qty);
        let origin_pos = self.bodies[self.markets[origin].body()].position(self.tick);
        let dest_pos = self.bodies[self.markets[dest].body()].position(self.tick);
        let (dx, dy) = (dest_pos.0 - origin_pos.0, dest_pos.1 - origin_pos.1);
        let dist = (dx * dx + dy * dy).isqrt();
        let travel = ((dist / CRUISE_SPEED) as u64).max(MIN_TRAVEL);
        let id = self.next_hauler_id;
        self.next_hauler_id += 1;
        self.events.push(Event::HaulerDeparted {
            id,
            commodity,
            origin,
            dest,
            qty,
        });
        self.haulers.push(Hauler {
            id,
            commodity,
            origin,
            dest,
            qty,
            depart_tick: self.tick,
            arrival_tick: self.tick + travel,
            origin_pos,
            dest_pos,
        });
    }

    /// The (commodity, origin, dest, qty) with the largest profitable spread
    /// where the origin has surplus and the destination has room.
    fn best_route(&self) -> Option<(usize, usize, usize, i64)> {
        let n = self.markets[0].defs().len();
        let mut best: Option<(usize, usize, usize, i64)> = None;
        let mut best_spread = MIN_SPREAD;
        for c in 0..n {
            let qty = (self.markets[0].defs()[c].target_stock / 10).max(1);
            for &(o, d) in &[(0usize, 1usize), (1, 0)] {
                let spread = self.markets[d].price(c) - self.markets[o].price(c);
                let has_surplus = self.markets[o].stock(c) > qty;
                let has_room = self.markets[d].stock(c) + qty < self.markets[d].wall_high(c);
                if spread > best_spread && has_surplus && has_room {
                    best = Some((c, o, d, qty));
                    best_spread = spread;
                }
            }
        }
        best
    }

    /// Build a render snapshot of the world at the current tick (§29).
    pub fn snapshot(&self) -> Snapshot {
        let bodies = self
            .bodies
            .iter()
            .map(|b| {
                let (x, y) = b.position(self.tick);
                BodyState { name: b.name, x, y }
            })
            .collect();
        let markets = self
            .markets
            .iter()
            .map(|m| MarketState {
                name: m.name(),
                commodities: m
                    .defs()
                    .iter()
                    .zip(m.stocks())
                    .map(|(d, s)| CommodityState {
                        name: d.name,
                        stock: s.stock,
                        price: s.price,
                    })
                    .collect(),
            })
            .collect();
        let names = self.markets[0].defs();
        let haulers = self
            .haulers
            .iter()
            .map(|h| {
                let (x, y) = h.position(self.tick);
                HaulerState {
                    id: h.id,
                    commodity: names[h.commodity].name,
                    x,
                    y,
                }
            })
            .collect();
        Snapshot {
            tick: self.tick,
            bodies,
            markets,
            haulers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_advances_tick_and_emits_event() {
        let mut sim = Sim::new(1);
        assert_eq!(sim.tick(), 0);
        let events = sim.step();
        assert!(events.contains(&Event::Tick { tick: 1 }));
        assert_eq!(sim.tick(), 1);
    }

    #[test]
    fn snapshot_has_bodies_and_markets() {
        let mut sim = Sim::new(1);
        for _ in 0..50 {
            sim.step();
        }
        let snap = sim.snapshot();
        assert_eq!(snap.tick, 50);
        assert_eq!(snap.bodies.len(), default_system().len());
        assert_eq!(snap.markets.len(), 2);
        assert_eq!((snap.bodies[0].x, snap.bodies[0].y), (0, 0)); // Sol fixed
    }

    #[test]
    fn same_seed_yields_identical_runs() {
        let mut a = Sim::new(42);
        let mut b = Sim::new(42);
        for _ in 0..600 {
            assert_eq!(a.step(), b.step());
            assert_eq!(a.snapshot(), b.snapshot());
        }
    }

    #[test]
    fn markets_carry_a_standing_arbitrage_spread() {
        // Ceres (producer) is cheaper than Earth (consumer) on raw Ore.
        let sim = Sim::new(0);
        let ore = 1;
        assert!(sim.markets()[0].price(ore) < sim.markets()[1].price(ore));
        // ...and dearer than Earth on refined Metals.
        let metals = 4;
        assert!(sim.markets()[0].price(metals) > sim.markets()[1].price(metals));
    }

    #[test]
    fn haulers_fly_the_routes() {
        let mut sim = Sim::new(3);
        let mut saw_hauler = false;
        for _ in 0..500 {
            sim.step();
            saw_hauler |= !sim.haulers().is_empty();
        }
        assert!(saw_hauler, "arbitrage never spawned a hauler");
    }

    #[test]
    fn trade_damps_the_spread() {
        // ReactorFuel carries the largest spread (Ceres dear, Earth cheap), so it
        // gets the most traffic; with haulers flowing its average spread settles
        // below the no-trade structural value.
        let mut sim = Sim::new(5);
        let rf = 5;
        let spread = |s: &Sim| s.markets()[0].price(rf) - s.markets()[1].price(rf);
        let structural = spread(&sim);
        for _ in 0..2_000 {
            sim.step();
        }
        let (mut sum, mut count) = (0i64, 0i64);
        for _ in 0..400 {
            sim.step();
            sum += spread(&sim);
            count += 1;
        }
        let avg = sum / count;
        assert!(
            avg < structural,
            "avg spread {avg} not damped below {structural}"
        );
        assert!(avg > 0, "the structural spread should persist, just damped");
    }

    #[test]
    fn interdiction_starves_the_destination() {
        // Two identical runs; in one we cut the first hauler. The RNG (market
        // jitter) stays aligned across both, so the only divergence is the
        // denied delivery — leaving the destination dearer (a shortage, §7b).
        let mut control = Sim::new(1);
        let mut cut = Sim::new(1);
        let (id, dest, commodity, arrival) = loop {
            control.step();
            cut.step();
            if let Some(h) = cut.haulers().first() {
                break (h.id, h.dest, h.commodity, h.arrival_tick);
            }
        };
        assert!(cut.interdict(id));
        assert!(!cut.interdict(id), "a cut hauler cannot be cut twice");
        while cut.tick() < arrival {
            control.step();
            cut.step();
        }
        assert!(
            cut.markets()[dest].price(commodity) > control.markets()[dest].price(commodity),
            "interdiction did not raise the destination price"
        );
    }

    /// The §7c gate, re-checked with the §7b traffic layer running: trade must
    /// not destabilize any market on any seed.
    #[test]
    fn no_death_spiral_with_traffic_on_any_seed() {
        for seed in 0..32u64 {
            let mut sim = Sim::new(seed);
            let mut ok = true;
            for _ in 0..5_000 {
                sim.step();
                for m in sim.markets() {
                    for (d, s) in m.defs().iter().zip(m.stocks()) {
                        ok &= s.stock > 0 && s.stock < d.max_stock + d.target_stock;
                        ok &= s.price > d.floor && s.price < d.ceiling;
                    }
                }
            }
            assert!(ok, "death-spiral with traffic on seed {seed}");
        }
    }
}
