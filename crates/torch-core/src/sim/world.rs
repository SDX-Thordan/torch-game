//! The authoritative world and the sim↔view contract (§27, §28, §29).
//!
//! [`Sim`] owns the truth and advances on a fixed tick. The view never reads
//! `Sim` directly: it consumes a [`Snapshot`] (current state to render) plus the
//! [`Event`] stream returned by [`Sim::step`] (what changed). This is the seam
//! the Godot shell binds to and that keeps the core headless and testable.

use super::alerts::{AlertFeed, Priority};
use super::automation::AutomationPolicy;
use super::campaign::Campaign;
use super::corp::{Corp, OwnedShip};
use super::economy::{default_markets, Market};
use super::event::Event;
use super::faction::Relations;
use super::interdiction::{resolve, Interceptor, Interdiction};
use super::orbit::{default_system, Body};
use super::progression::Progression;
use super::rng::Pcg32;
use super::ships::{self, ShipClass};
use super::traffic::Hauler;

/// Credits charged per unit of a commissioned hull's dry mass (§5 sink).
const SHIP_PRICE_PER_MASS: i64 = 5;
/// CEO experience earned per completed player operation (§10 earned through play).
const OP_XP: i64 = 200;
/// Research points earned per completed player operation.
const OP_RESEARCH_POINTS: i64 = 40;

/// Why a market order could not be filled (§5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TradeError {
    InsufficientCredits,
    InsufficientStock,
    InsufficientCargo,
}

/// Why a ship could not be commissioned (§5/§8c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommissionError {
    CantAfford,
    NotEnoughCrew,
}

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
/// Ticks between NPC pirate raid attempts (§13 ambient predation).
const PIRATE_INTERVAL: u64 = 72;
/// Ticks between automated interdiction sorties (§12 patrol cadence).
const AUTOMATION_INTERVAL: u64 = 12;

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
    pirate: Interceptor,
    feed: AlertFeed,
    relations: Relations,
    progression: Progression,
    policy: AutomationPolicy,
    campaign: Campaign,
    corp: Corp,
    rng: Pcg32,
    events: Vec<Event>,
}

impl Sim {
    /// Create a sim seeded for determinism (§27). Same seed ⇒ same run.
    pub fn new(seed: u64) -> Self {
        let markets = default_markets();
        let market_names = markets.iter().map(|m| m.name().to_string()).collect();
        let commodity_names = markets[0]
            .defs()
            .iter()
            .map(|d| d.name.to_string())
            .collect();
        let commodity_count = markets[0].defs().len();
        Self {
            tick: 0,
            bodies: default_system(),
            haulers: Vec::new(),
            next_hauler_id: 0,
            // A raider lurking on the inner lanes (§13): quick but lightly crewed,
            // so it lands some strikes and muffs others.
            pirate: Interceptor {
                pos: (-600_000, -300_000),
                speed: 24_000,
                skill_bp: 400,
            },
            feed: AlertFeed::new(seed, market_names, commodity_names),
            relations: Relations::new(),
            progression: Progression::new(),
            policy: AutomationPolicy::default(),
            campaign: Campaign::new(),
            corp: Corp::new(commodity_count),
            markets,
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

    /// The alert feed (§19) — the voiced, ranked exception stream.
    pub fn feed(&self) -> &AlertFeed {
        &self.feed
    }

    /// The player's standing with each faction (§10).
    pub fn relations(&self) -> &Relations {
        &self.relations
    }

    /// The retention spine — tier, goals, and the gate's approach (§0).
    pub fn campaign(&self) -> &Campaign {
        &self.campaign
    }

    /// The player corporation — treasury, cargo, fleet, crew (§1/§5).
    pub fn corp(&self) -> &Corp {
        &self.corp
    }

    /// Buy `qty` of commodity `c` at market `m` (§5): debits credits at the
    /// current price, lifts the goods into the warehouse, and nudges the price up.
    pub fn buy(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        let price = self.markets[m].price(c);
        let cost = price * qty;
        if self.markets[m].stock(c) < qty {
            return Err(TradeError::InsufficientStock);
        }
        if self.corp.credits() < cost {
            return Err(TradeError::InsufficientCredits);
        }
        self.markets[m].remove_stock(c, qty);
        self.corp.debit(cost);
        self.corp.store(c, qty);
        Ok(cost)
    }

    /// Sell `qty` of commodity `c` into market `m` (§5): lands warehouse cargo at
    /// the current price for credits, nudging the price down.
    pub fn sell(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        if self.corp.cargo(c) < qty {
            return Err(TradeError::InsufficientCargo);
        }
        let price = self.markets[m].price(c);
        let revenue = price * qty;
        self.corp.unstore(c, qty);
        self.markets[m].add_stock(c, qty);
        self.corp.credit(revenue);
        Ok(revenue)
    }

    /// Commission a warship of `class` into the fleet (§5/§8c): pays its build
    /// cost and draws its crew from the trained-crew pool.
    pub fn commission_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        let hull = ships::hull(class);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        let loadout = ships::reference_loadout(class, &mut self.rng);
        self.corp.debit(price);
        self.corp.assign_crew(hull.crew_required);
        let name = format!("{} {:02}", hull.name, self.corp.fleet().len() + 1);
        self.corp.add_ship(OwnedShip { name, loadout });
        Ok(())
    }

    /// Standings, mutable — for diplomacy/contracts that move reputation (§10).
    pub fn relations_mut(&mut self) -> &mut Relations {
        &mut self.relations
    }

    /// The player's advancement across research / blueprints / CEO skills (§10).
    pub fn progression(&self) -> &Progression {
        &self.progression
    }

    /// Advancement, mutable — for research/CEO progress driven by play.
    pub fn progression_mut(&mut self) -> &mut Progression {
        &mut self.progression
    }

    /// The standing automation policy the managers execute (§12).
    pub fn policy(&self) -> &AutomationPolicy {
        &self.policy
    }

    /// Set the automation policy the managers execute (§12).
    pub fn policy_mut(&mut self) -> &mut AutomationPolicy {
        &mut self.policy
    }

    /// Discover blueprint `i`, honoring its reputation gate against the player's
    /// current standings (§10/§25). Returns whether it was learned.
    pub fn discover_blueprint(&mut self, i: usize) -> bool {
        self.progression
            .blueprints
            .discover(i, &self.relations)
            .is_ok()
    }

    /// Set the player-tunable alert surfacing threshold (§19).
    pub fn set_alert_threshold(&mut self, min_priority: Priority) {
        self.feed.set_threshold(min_priority);
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
        self.pirate_raid();
        self.run_automation();
        self.events.push(Event::Tick { tick: self.tick });
        // The alert feed (§19) consumes this tick's events (§29).
        let tick = self.tick;
        for e in &self.events {
            self.feed.ingest(e, tick);
        }
        &self.events
    }

    /// Administratively cut the in-flight hauler with `id` (a guaranteed delete,
    /// for the binding/tests). Returns whether a hauler was actually cut. For the
    /// positioning-and-odds verb, use [`Sim::interdict_with`].
    pub fn interdict(&mut self, id: u64) -> bool {
        if let Some(i) = self.haulers.iter().position(|h| h.id == id) {
            let h = self.cut_hauler(i);
            self.ripple_reputation(&h);
            true
        } else {
            false
        }
    }

    /// Attempt to interdict hauler `id` with `interceptor` (§7b): the cut only
    /// lands if the interceptor has the legs to reach the hauler *and* wins the
    /// roll. Returns the resolved outcome.
    pub fn interdict_with(&mut self, id: u64, interceptor: Interceptor) -> Interdiction {
        let Some(i) = self.haulers.iter().position(|h| h.id == id) else {
            return Interdiction::NoSolution;
        };
        let outcome = resolve(&self.haulers[i], &interceptor, self.tick, &mut self.rng);
        if outcome == Interdiction::Interdicted {
            let h = self.cut_hauler(i);
            self.ripple_reputation(&h);
        }
        outcome
    }

    /// A *player* cut sours relations with the hauler's owner faction (§7b/§10)
    /// and counts as an operation on the climb (§0); pirate raids do neither.
    fn ripple_reputation(&mut self, h: &Hauler) {
        let faction = self.markets[h.origin].faction();
        self.relations.on_player_interdict(faction);
        // Operations build the company's expertise — progression earned through
        // play (§10), not handed out.
        self.progression.ceo.gain_xp(OP_XP);
        self.progression.research.add_points(OP_RESEARCH_POINTS);
        if let Some(tier) = self.campaign.record_op() {
            self.events.push(Event::TierAscended { tier });
        }
    }

    /// Remove the hauler at `index`, denying its delivery and tagging the
    /// resulting shortage at the destination (§7b). Returns the cut hauler.
    fn cut_hauler(&mut self, index: usize) -> Hauler {
        let h = self.haulers.remove(index);
        self.events.push(Event::HaulerInterdicted { id: h.id });
        self.events.push(Event::Scarcity {
            market: h.dest,
            commodity: h.commodity,
        });
        h
    }

    /// Run the standing automation policy this tick (§12 run-by-exception): the
    /// interdiction patrol cuts matching shipping on its cadence, and research is
    /// auto-invested. The player set the policy; the managers do the work.
    fn run_automation(&mut self) {
        let pol = self.policy; // Copy — no borrow held over the mutations below
        if pol.interdiction.enabled && self.tick.is_multiple_of(AUTOMATION_INTERVAL) {
            let target = self
                .haulers
                .iter()
                .enumerate()
                .filter(|(_, h)| h.qty >= pol.interdiction.min_cargo)
                .filter(|(_, h)| match pol.interdiction.target {
                    Some(f) => self.markets[h.origin].faction() == f,
                    None => true,
                })
                .max_by_key(|(_, h)| h.qty)
                .map(|(i, _)| i);
            if let Some(i) = target {
                let outcome = resolve(&self.haulers[i], &pol.patrol, self.tick, &mut self.rng);
                if outcome == Interdiction::Interdicted {
                    let h = self.cut_hauler(i);
                    self.ripple_reputation(&h); // the player's managed asset → their tab
                }
            }
        }
        if pol.auto_research {
            if let Some(i) = self.progression.research.cheapest_researchable() {
                let _ = self.progression.research.research(i);
            }
        }
    }

    /// NPC pirates periodically strike at the fattest cargo in flight (§13).
    fn pirate_raid(&mut self) {
        if !self.tick.is_multiple_of(PIRATE_INTERVAL) || self.haulers.is_empty() {
            return;
        }
        let target = self
            .haulers
            .iter()
            .enumerate()
            .max_by_key(|(_, h)| h.qty)
            .map(|(i, _)| i);
        if let Some(i) = target {
            let outcome = resolve(&self.haulers[i], &self.pirate, self.tick, &mut self.rng);
            if outcome == Interdiction::Interdicted {
                self.cut_hauler(i); // pirates, not the player → no reputation hit
            }
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

    /// Step until a hauler is in flight; return its id.
    fn fly_a_hauler(sim: &mut Sim) -> u64 {
        loop {
            sim.step();
            if let Some(h) = sim.haulers().first() {
                return h.id;
            }
        }
    }

    #[test]
    fn rich_interdiction_requires_a_firing_solution() {
        let mut sim = Sim::new(2);
        let id = fly_a_hauler(&mut sim);
        let before = sim.haulers().len();
        // A crawler far off the lane can't reach it: a miss that leaves it flying.
        let crawler = Interceptor {
            pos: (8_000_000, 8_000_000),
            speed: 1,
            skill_bp: 0,
        };
        assert_eq!(sim.interdict_with(id, crawler), Interdiction::NoSolution);
        assert_eq!(
            sim.haulers().len(),
            before,
            "a miss must not remove the hauler"
        );
        // A fast frigate sitting on the hauler always has a solution (it lands or
        // the hauler escapes — never NoSolution).
        let pos = sim
            .haulers()
            .iter()
            .find(|h| h.id == id)
            .unwrap()
            .position(sim.tick());
        let frigate = Interceptor {
            pos,
            speed: 200_000,
            skill_bp: 0,
        };
        assert_ne!(sim.interdict_with(id, frigate), Interdiction::NoSolution);
    }

    #[test]
    fn the_corp_starts_solvent_and_crewed() {
        let sim = Sim::new(0);
        assert!(sim.corp().credits() > 0);
        assert!(sim.corp().trained_crew() > 0);
        assert!(sim.corp().fleet().is_empty());
    }

    #[test]
    fn arbitrage_round_trip_turns_a_profit() {
        // Buy ReactorFuel cheap at Earth (refined producer) and sell it dear at
        // Ceres (refined consumer): the player works the same spread as the NPC
        // haulers, for real credits (§5).
        let mut sim = Sim::new(0);
        let (earth, ceres, rf) = (1usize, 0usize, 5usize);
        assert!(sim.markets()[earth].price(rf) < sim.markets()[ceres].price(rf));
        let start = sim.corp().credits();
        let cost = sim.buy(earth, rf, 10).unwrap();
        assert_eq!(sim.corp().credits(), start - cost);
        assert_eq!(sim.corp().cargo(rf), 10);
        let revenue = sim.sell(ceres, rf, 10).unwrap();
        assert!(revenue > cost, "selling dear should beat buying cheap");
        assert!(sim.corp().credits() > start, "the round trip should profit");
        assert_eq!(sim.corp().cargo(rf), 0);
    }

    #[test]
    fn trades_are_guarded() {
        let mut sim = Sim::new(0);
        // Nothing in the warehouse to sell.
        assert_eq!(sim.sell(0, 0, 5), Err(TradeError::InsufficientCargo));
        // More than the market holds.
        assert_eq!(sim.buy(0, 0, 1_000_000), Err(TradeError::InsufficientStock));
        // Affordable stock-wise, but beyond the treasury (200 dear ReactorFuel).
        assert_eq!(sim.buy(0, 5, 200), Err(TradeError::InsufficientCredits));
    }

    #[test]
    fn commissioning_spends_credits_and_crew_with_the_pool_as_the_cap() {
        let mut sim = Sim::new(0);
        let (credits0, crew0) = (sim.corp().credits(), sim.corp().trained_crew());
        sim.commission_ship(ShipClass::Frigate).unwrap();
        assert_eq!(sim.corp().fleet().len(), 1);
        assert!(sim.corp().credits() < credits0);
        assert!(sim.corp().trained_crew() < crew0);
        // A battleship needs more crew than the starting pool can field (§8c).
        assert_eq!(
            sim.commission_ship(ShipClass::Battleship),
            Err(CommissionError::NotEnoughCrew)
        );
    }

    #[test]
    fn operations_climb_the_retention_spine() {
        // Each player interdiction is an operation on the climb; three of them
        // ascend past the Station and draw the gate closer (§0.3).
        use crate::sim::campaign::Tier;
        let mut sim = Sim::new(0);
        assert_eq!(sim.campaign().tier(), Tier::Station);
        let mut ops = 0;
        for _ in 0..400 {
            if let Some(h) = sim.haulers().first() {
                let id = h.id;
                if sim.interdict(id) {
                    ops += 1;
                }
            }
            sim.step();
            if sim.campaign().tier() != Tier::Station {
                break;
            }
        }
        assert!(ops >= 3, "should have completed operations, got {ops}");
        assert_ne!(
            sim.campaign().tier(),
            Tier::Station,
            "should climb past the Station"
        );
        assert!(
            sim.campaign().gate_progress_bp() > 0,
            "the gate should draw closer"
        );
    }

    #[test]
    fn progression_advances_through_the_sim() {
        let mut sim = Sim::new(0);
        sim.progression_mut().ceo.gain_xp(3_000);
        assert_eq!(sim.progression().ceo.level(), 4);
        sim.progression_mut().research.add_points(1_000);
        assert!(sim.progression_mut().research.research(0).is_ok());
        assert!(sim.progression().research.is_unlocked(0));
        // Generic blueprint discoverable; the Martian design stays rep-gated
        // until Mars standing is high enough (§10).
        assert!(sim.discover_blueprint(0));
        assert!(!sim.discover_blueprint(2));
        sim.relations_mut()
            .adjust(crate::sim::faction::Faction::Mars, 500);
        assert!(sim.discover_blueprint(2));
    }

    #[test]
    fn automation_interdicts_only_targeted_shipping() {
        // Set a standing order to hunt Earth shipping; the manager runs it for
        // us, souring Earth while leaving off-target factions alone (§12).
        let mut sim = Sim::new(0);
        sim.policy_mut().interdiction.enabled = true;
        sim.policy_mut().interdiction.target = Some(crate::sim::faction::Faction::Earth);
        for _ in 0..1_000 {
            sim.step();
        }
        assert!(
            sim.relations()
                .standing(crate::sim::faction::Faction::Earth)
                < 0,
            "the patrol should have cut Earth shipping"
        );
        assert_eq!(
            sim.relations().standing(crate::sim::faction::Faction::Belt),
            0,
            "Belt shipping was off-target and untouched"
        );
    }

    #[test]
    fn automation_min_cargo_spares_small_fry() {
        let mut sim = Sim::new(0);
        sim.policy_mut().interdiction.enabled = true;
        sim.policy_mut().interdiction.min_cargo = 1_000_000; // nothing is this big
        for _ in 0..1_000 {
            sim.step();
        }
        for m in sim.markets() {
            assert_eq!(sim.relations().standing(m.faction()), 0);
        }
    }

    #[test]
    fn automation_auto_researches_when_funded() {
        let mut sim = Sim::new(0);
        sim.policy_mut().auto_research = true;
        sim.progression_mut().research.add_points(1_000);
        sim.step();
        assert!(sim.progression().research.unlocked_count() > 0);
    }

    #[test]
    fn player_interdiction_sours_relations() {
        // Cutting a faction's hauler lowers the player's standing with them (§7b/§10).
        let mut sim = Sim::new(0);
        let id = fly_a_hauler(&mut sim);
        let origin = sim.haulers().iter().find(|h| h.id == id).unwrap().origin;
        let faction = sim.markets()[origin].faction();
        assert!(sim.interdict(id));
        assert!(
            sim.relations().standing(faction) < 0,
            "the owner should resent it"
        );
    }

    #[test]
    fn pirate_raids_do_not_blame_the_player() {
        // Pirates thin the lanes for thousands of ticks; the player's standings
        // stay neutral (the raids aren't attributed to them).
        let mut sim = Sim::new(0);
        for _ in 0..2_000 {
            sim.step();
        }
        for m in sim.markets() {
            assert_eq!(sim.relations().standing(m.faction()), 0);
        }
    }

    #[test]
    fn the_alert_feed_voices_the_run() {
        // Over a run the feed fills with ranked alerts, including act-now
        // shortages tagged with a verb (§19/§0.4).
        let mut sim = Sim::new(0);
        for _ in 0..2_000 {
            sim.step();
        }
        let surfaced = sim.feed().surfaced();
        assert!(
            !surfaced.is_empty(),
            "the feed should have something to say"
        );
        assert!(
            surfaced.iter().any(|a| a.is_act_now() && a.verb.is_some()),
            "an interdicted run should raise act-now shortages"
        );
    }

    #[test]
    fn pirates_raid_the_lanes() {
        // Over a long run the ambient raider lands strikes, each tagging a
        // destination scarcity (§7b/§13).
        let mut sim = Sim::new(0);
        let (mut cuts, mut scarcities) = (0, 0);
        for _ in 0..4_000 {
            for e in sim.step() {
                match e {
                    Event::HaulerInterdicted { .. } => cuts += 1,
                    Event::Scarcity { .. } => scarcities += 1,
                    _ => {}
                }
            }
        }
        assert!(cuts > 0, "pirates never struck the lanes");
        assert_eq!(cuts, scarcities, "every cut should leave a scarcity");
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
