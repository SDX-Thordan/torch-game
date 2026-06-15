//! The authoritative world and the sim↔view contract (§27, §28, §29).
//!
//! [`Sim`] owns the truth and advances on a fixed tick. The view never reads
//! `Sim` directly: it consumes a [`Snapshot`] (current state to render) plus the
//! [`Event`] stream returned by [`Sim::step`] (what changed). This is the seam
//! the Godot shell binds to and that keeps the core headless and testable.

use super::alerts::{AlertFeed, Priority, Verb};
use super::automation::AutomationPolicy;
use super::campaign::Campaign;
use super::combat::{self, Band, BattleOutcome, Doctrine, Fleet, TargetPriority};
use super::corp::{Corp, OwnedShip};
use super::economy::{default_markets, Market};
use super::event::Event;
use super::faction::Relations;
use super::industry::Station;
use super::interdiction::{resolve, Interceptor, Interdiction};
use super::logistics::TradeRoute;
use super::orbit::{default_system, Body};
use super::progression::Progression;
use super::rng::Pcg32;
use super::ships::{self, Loadout, ShipClass};
use super::traffic::Hauler;

/// Credits charged per unit of a commissioned hull's dry mass (§5 sink).
const SHIP_PRICE_PER_MASS: i64 = 5;
/// CEO experience earned per completed player operation (§10 earned through play).
const OP_XP: i64 = 200;
/// Research points earned per completed player operation.
const OP_RESEARCH_POINTS: i64 = 40;
/// Basis-point denominator for the brokerage fee.
const FEE_DEN: i64 = 10_000;
/// Treasury a company can hold before operating overhead bites (§5 sink): the
/// starting float plus headroom for a capital purchase, so early/mid play is
/// untaxed and only runaway hoarding is throttled.
const UPKEEP_FREE_FLOAT: i64 = 100_000;
/// Per-tick fraction of the *taxable* treasury skimmed as overhead. Together
/// with the free float this gives a wealth-scaled sink (overhead grows with the
/// enterprise you run), so income strategies settle at a sustainable equilibrium
/// instead of compounding without bound (gameplay-QA economy finding).
const UPKEEP_DEN: i64 = 150;
/// Credits to found a production station (§3.1).
const STATION_COST: i64 = 8_000;
/// Cap on player stations (Tier-1 scope).
const MAX_STATIONS: usize = 4;
/// A refinery's per-tick throughput, sell-surplus floor, and production ceiling.
const REFINERY_RATE: i64 = 5;
const REFINERY_SELL_ABOVE: i64 = 80;
const REFINERY_TARGET: i64 = 160;
/// Number of raw commodities (raw `i` refines to refined `i + RAW_COUNT`).
const RAW_COUNT: usize = 3;

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

/// Why a station could not be founded (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundError {
    CantAfford,
    NotARawCommodity,
    TooManyStations,
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
/// How many standing trade routes the player can run at once (§4 master-table).
const MAX_ROUTES: usize = 4;
/// Ticks between reputation-decay ticks (§10): grudges fade slowly toward
/// neutral, so a Hostile standing is recoverable if you stop antagonizing.
const REP_RECOVERY_INTERVAL: u64 = 24;
/// How far each standing drifts toward neutral per recovery tick. Slow enough
/// that an active raider still outruns it.
const REP_RECOVERY_STEP: i64 = 8;

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
    routes: Vec<TradeRoute>,
    stations: Vec<Station>,
    rng: Pcg32,
    events: Vec<Event>,
    /// How many leading `events` the last `step` already returned and fed to the
    /// alert feed. The next `step` drains exactly these, *keeping* anything the
    /// player's between-tick verbs pushed after them — so player-caused events
    /// (a cut's `Scarcity`, an ascent's `TierAscended`) survive to be voiced.
    returned: usize,
}

impl Sim {
    /// Brokerage fee on instant market orders, in basis points (§5 sink). Tuned
    /// so hand-trading thin spreads loses money — only a fat spread clears it,
    /// which makes the trade a decision and keeps the transit-paying standing
    /// route competitive.
    pub const TRADE_FEE_BP: i64 = 300;

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
            routes: Vec::new(),
            stations: Vec::new(),
            markets,
            rng: Pcg32::new(seed),
            events: Vec::new(),
            returned: 0,
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

    /// Skim operating overhead off the treasury each tick (§5 sink). Overhead is
    /// a fraction of holdings *above* a free float, so it bites only runaway
    /// hoarding — the wealth-scaled sink that turns every income strategy into a
    /// sustainable equilibrium rather than an unbounded faucet.
    fn charge_upkeep(&mut self) {
        let taxable = self.corp.credits() - UPKEEP_FREE_FLOAT;
        if taxable > 0 {
            let upkeep = taxable / UPKEEP_DEN;
            if upkeep > 0 {
                self.corp.debit(upkeep);
            }
        }
    }

    /// Brokerage fee on a `value`-credit instant market order (§5). This is the
    /// cost of *immediate* liquidity the standing route (which pays transit
    /// instead) doesn't incur — so hand-trading is a real decision against the
    /// fee, not a riskless faucet (gameplay-QA economy finding).
    fn trade_fee(value: i64) -> i64 {
        value * Self::TRADE_FEE_BP / FEE_DEN
    }

    /// Buy `qty` of commodity `c` at market `m` (§5): debits the goods cost plus
    /// the brokerage fee, lifts the goods into the warehouse, and nudges the
    /// price up. Returns the total credits spent (cost + fee).
    pub fn buy(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        let price = self.markets[m].price(c);
        let cost = price * qty;
        let total = cost + Self::trade_fee(cost);
        if self.markets[m].stock(c) < qty {
            return Err(TradeError::InsufficientStock);
        }
        if self.corp.credits() < total {
            return Err(TradeError::InsufficientCredits);
        }
        self.markets[m].remove_stock(c, qty);
        self.corp.debit(total);
        self.corp.store(c, qty);
        Ok(total)
    }

    /// Sell `qty` of commodity `c` into market `m` (§5): lands warehouse cargo at
    /// the current price less the brokerage fee, nudging the price down. Returns
    /// the net credits received (revenue − fee).
    pub fn sell(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        if self.corp.cargo(c) < qty {
            return Err(TradeError::InsufficientCargo);
        }
        let price = self.markets[m].price(c);
        let revenue = price * qty;
        let net = revenue - Self::trade_fee(revenue);
        self.corp.unstore(c, qty);
        self.markets[m].add_stock(c, qty);
        self.corp.credit(net);
        Ok(net)
    }

    /// Answer an act-now shortage in one move (§0.4 / §3.3 speculate): source
    /// `qty` of `commodity` at the cheapest *other* market and sell it into the
    /// short `market` for the premium — no pre-held cargo needed. Resolves the
    /// matching alert and returns the net profit (revenue − cost).
    pub fn exploit_shortage(
        &mut self,
        market: usize,
        commodity: usize,
        qty: i64,
    ) -> Result<i64, TradeError> {
        if market >= self.markets.len() {
            return Err(TradeError::InsufficientStock);
        }
        let source = (0..self.markets.len())
            .filter(|&m| m != market)
            .min_by_key(|&m| self.markets[m].price(commodity))
            .ok_or(TradeError::InsufficientStock)?;
        let cost = self.buy(source, commodity, qty)?;
        let revenue = self.sell(market, commodity, qty)?;
        self.feed.resolve_shortage(market, commodity);
        Ok(revenue - cost)
    }

    /// One-press answer to the loudest open act-now shortage (the alert→verb
    /// path the influence model wants). Returns whether one was answered.
    pub fn answer_top_shortage(&mut self, qty: i64) -> bool {
        let target = self.feed.surfaced().iter().find_map(|a| {
            a.verb
                .map(|Verb::ExploitShortage { market, commodity }| (market, commodity))
        });
        match target {
            Some((m, c)) => self.exploit_shortage(m, c, qty).is_ok(),
            None => false,
        }
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
        self.complete_op(); // building the fleet is progress on the climb (§0)
        Ok(())
    }

    /// Commission a civilian freighter to run trade-route standing orders (§4).
    pub fn commission_freighter(&mut self) -> Result<(), CommissionError> {
        let hull = ships::hull(ShipClass::Freighter);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        self.corp.debit(price);
        self.corp.assign_crew(hull.crew_required);
        self.corp.add_freighter();
        self.complete_op(); // standing up logistics is progress on the climb (§0)
        Ok(())
    }

    /// The player's table of standing trade routes (§4).
    pub fn routes(&self) -> &[TradeRoute] {
        &self.routes
    }

    /// The first standing route, if any — a convenience for the single-route
    /// status view in the shell (§4).
    pub fn route(&self) -> Option<TradeRoute> {
        self.routes.first().copied()
    }

    /// Add a parameterized Trade Route standing order to the table — buy
    /// `commodity` at `origin`, sell at `dest`, `qty` per trip, only while the
    /// spread clears `min_margin` (§4). Many routes run concurrently against the
    /// shared freighter pool; exceptions go idle. Capped at [`MAX_ROUTES`].
    pub fn set_trade_route(
        &mut self,
        commodity: usize,
        origin: usize,
        dest: usize,
        qty: i64,
        min_margin: i64,
    ) {
        if self.routes.len() < MAX_ROUTES {
            self.routes
                .push(TradeRoute::new(commodity, origin, dest, qty, min_margin));
        }
    }

    /// Clear the whole route table.
    pub fn clear_trade_route(&mut self) {
        self.routes.clear();
    }

    /// Travel time in ticks between two markets at the current orrery geometry.
    fn travel_ticks(&self, origin: usize, dest: usize) -> u64 {
        let o = self.bodies[self.markets[origin].body()].position(self.tick);
        let d = self.bodies[self.markets[dest].body()].position(self.tick);
        let (dx, dy) = (d.0 - o.0, d.1 - o.1);
        let dist = (dx * dx + dy * dy).isqrt();
        ((dist / CRUISE_SPEED) as u64).max(MIN_TRAVEL)
    }

    /// Run the whole route table this tick (§4): land every arriving trip, then
    /// dispatch idle routes against the **shared freighter pool** (a route can
    /// only set out if a freighter is free). The routes run themselves; the
    /// player only set the parameters, and exceptions (below margin, no free
    /// freighter) simply stay idle.
    fn run_logistics(&mut self) {
        if self.routes.is_empty() {
            return;
        }
        let freighters = self.corp.freighters();
        // Move the table out so the per-route mutations don't fight the
        // `markets`/`corp` borrows (same pattern as the single-route version).
        let mut routes = std::mem::take(&mut self.routes);

        // Deliveries first, freeing up freighters for this tick's dispatch.
        for rt in routes.iter_mut() {
            if rt.in_transit && self.tick >= rt.arrival {
                let revenue = self.markets[rt.dest].price(rt.commodity) * rt.carrying;
                self.markets[rt.dest].add_stock(rt.commodity, rt.carrying);
                self.corp.credit(revenue);
                rt.in_transit = false;
                rt.carrying = 0;
                self.complete_op(); // a delivered standing order is an op (§0/§4)
            }
        }

        // Dispatch idle routes while freighters remain in the pool.
        let mut in_flight = routes.iter().filter(|r| r.in_transit).count() as i64;
        for rt in routes.iter_mut() {
            if in_flight >= freighters {
                break;
            }
            if rt.in_transit || !rt.active {
                continue;
            }
            let buy = self.markets[rt.origin].price(rt.commodity);
            let spread = self.markets[rt.dest].price(rt.commodity) - buy;
            let cost = buy * rt.qty;
            let stocked = self.markets[rt.origin].stock(rt.commodity) > rt.qty;
            if spread >= rt.min_margin && stocked && self.corp.credits() >= cost {
                self.markets[rt.origin].remove_stock(rt.commodity, rt.qty);
                self.corp.debit(cost);
                rt.in_transit = true;
                rt.carrying = rt.qty;
                rt.arrival = self.tick + self.travel_ticks(rt.origin, rt.dest);
                in_flight += 1;
            }
        }

        self.routes = routes;
    }

    /// The player's production stations (§3.1).
    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    /// Found a refinery that turns a raw commodity into its refined product:
    /// source `raw` at `buy_market`, refine, and auto-sell the surplus at
    /// `sell_market` (§3.1 Produce + sell-surplus). Costs capital.
    pub fn found_refinery(
        &mut self,
        raw: usize,
        buy_market: usize,
        sell_market: usize,
    ) -> Result<(), FoundError> {
        if raw >= RAW_COUNT {
            return Err(FoundError::NotARawCommodity);
        }
        if self.stations.len() >= MAX_STATIONS {
            return Err(FoundError::TooManyStations);
        }
        if self.corp.credits() < STATION_COST {
            return Err(FoundError::CantAfford);
        }
        self.corp.debit(STATION_COST);
        self.stations.push(Station {
            body: self.markets[buy_market].body(),
            input: raw,
            output: raw + RAW_COUNT,
            rate: REFINERY_RATE,
            buy_market,
            sell_market,
            sell_above: REFINERY_SELL_ABOVE,
            output_target: REFINERY_TARGET,
        });
        self.complete_op(); // founding industry is progress on the climb (§0)
        Ok(())
    }

    /// Run every station's Produce standing order this tick (§3.1/§4): source
    /// input from a market, transform raw → refined, and dump the surplus output
    /// for credits. Hands-off; the player only set the recipe.
    fn run_industry(&mut self) {
        for i in 0..self.stations.len() {
            let st = self.stations[i];
            let producing = self.corp.cargo(st.output) < st.output_target;
            // Source the input recipe from its market when short.
            if producing && self.corp.cargo(st.input) < st.rate {
                let price = self.markets[st.buy_market].price(st.input);
                let cost = price * st.rate;
                if self.markets[st.buy_market].stock(st.input) > st.rate
                    && self.corp.credits() >= cost
                {
                    self.markets[st.buy_market].remove_stock(st.input, st.rate);
                    self.corp.debit(cost);
                    self.corp.store(st.input, st.rate);
                }
            }
            // Transform input → output (the value-add).
            if producing && self.corp.cargo(st.input) >= st.rate {
                self.corp.unstore(st.input, st.rate);
                self.corp.store(st.output, st.rate);
            }
            // Sell-surplus rule: dump output held above the threshold.
            let surplus = self.corp.cargo(st.output) - st.sell_above;
            if surplus > 0 {
                let price = self.markets[st.sell_market].price(st.output);
                self.corp.unstore(st.output, surplus);
                self.markets[st.sell_market].add_stock(st.output, surplus);
                self.corp.credit(price * surplus);
            }
        }
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
        // Drop only the events the previous `step` already surfaced, retaining
        // any a player verb pushed since (so player-caused events aren't lost to
        // a blanket clear — the §0.3 fanfare and §0.4 shortage fire for the
        // player too, not just for pirate/automation cuts).
        self.events.drain(0..self.returned);
        self.tick += 1;
        for m in self.markets.iter_mut() {
            m.step(&mut self.rng);
        }
        self.deliver_arrivals();
        self.spawn_traffic();
        self.pirate_raid();
        self.run_automation();
        self.run_logistics();
        self.run_industry();
        self.charge_upkeep();
        if self.tick.is_multiple_of(REP_RECOVERY_INTERVAL) {
            self.relations.decay_toward_neutral(REP_RECOVERY_STEP);
        }
        self.events.push(Event::Tick { tick: self.tick });
        // The alert feed (§19) consumes everything surfacing this tick (§29):
        // the carried-over player events plus this tick's own.
        let tick = self.tick;
        for e in &self.events {
            self.feed.ingest(e, tick);
        }
        self.returned = self.events.len();
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

    /// Send the player fleet against a raider pack at `band` and resolve the
    /// battle (§9). This is the missing trigger the gameplay-QA review flagged:
    /// `sim::combat` had no verb on `Sim`, so commissioned warships never fought.
    /// The raider pack is sized to the fleet for a real contest; losses are
    /// applied to the corp, a win counts as an operation on the climb (§0), and a
    /// `BattleResolved` event is emitted for the feed (§19) and diorama (§22).
    /// Returns the outcome, or `None` if the player has no warships to send.
    pub fn engage_raiders(&mut self, band: Band) -> Option<BattleOutcome> {
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return None;
        }
        // A matched pack of raider frigates — quantity the player must answer
        // with quality and doctrine (§8a/§9 saturation tension).
        let pack: Vec<Loadout> = (0..player_ships.len())
            .map(|_| ships::reference_loadout(ShipClass::Frigate, &mut self.rng))
            .collect();
        let doctrine = Doctrine {
            band,
            salvo_reload: 6,
            target: TargetPriority::Biggest,
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine,
            },
            &Fleet {
                ships: &pack,
                doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        self.corp.lose_ships_to(survivors);
        let won = outcome.winner == Some(0);
        if won {
            self.complete_op(); // holding the field is progress on the climb (§0)
        }
        self.events.push(Event::BattleResolved { won, losses });
        Some(outcome)
    }

    /// A *player* cut sours relations with the hauler's owner faction (§7b/§10)
    /// and counts as an operation on the climb (§0); pirate raids do neither.
    fn ripple_reputation(&mut self, h: &Hauler) {
        let faction = self.markets[h.origin].faction();
        self.relations.on_player_interdict(faction);
        self.complete_op();
    }

    /// Record a completed player **operation** — the unit of progress on the §0
    /// climb. Interdiction was the *only* verb that called this, so the retention
    /// spine ignored the whole build/trade/route side of the influence model
    /// (the gameplay-QA review's #1 finding). Now every substantive player act —
    /// a cut, a commissioned ship/freighter, a founded station, a completed
    /// route delivery — advances the campaign and earns the CEO/research
    /// progress operations grant (§10, earned through play). Emits the ascent
    /// fanfare on a tier crossing (§0.3).
    fn complete_op(&mut self) {
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

    #[test]
    fn player_verb_events_survive_to_the_next_step() {
        // A player cut between ticks pushes HaulerInterdicted + Scarcity; the
        // next `step` must *surface* them (not wipe them) so the feed voices the
        // player's own cut — previously `events.clear()` dropped them.
        let mut sim = Sim::new(1);
        let id = fly_a_hauler(&mut sim);
        let feed_before = sim.feed().surfaced().len();
        assert!(sim.interdict(id));
        let events = sim.step().to_vec();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::HaulerInterdicted { .. })),
            "the player's cut should reach the returned stream"
        );
        assert!(events.iter().any(|e| matches!(e, Event::Scarcity { .. })));
        assert!(
            sim.feed().surfaced().len() > feed_before,
            "the player's cut should reach the feed"
        );
        // And the carried-over events are not re-surfaced a second time.
        let next = sim.step().to_vec();
        assert!(!next
            .iter()
            .any(|e| matches!(e, Event::HaulerInterdicted { .. })));
    }

    #[test]
    fn a_player_ascent_is_voiced() {
        // The §0.3 fanfare must fire for the *player's* climb, not just for
        // sim-internal ops: a player interdiction's TierAscended now reaches the
        // returned stream.
        use crate::sim::campaign::Tier;
        let mut sim = Sim::new(0);
        let mut saw_ascent = false;
        for _ in 0..400 {
            if let Some(h) = sim.haulers().first() {
                let id = h.id;
                sim.interdict(id);
            }
            for e in sim.step() {
                if matches!(e, Event::TierAscended { .. }) {
                    saw_ascent = true;
                }
            }
            if sim.campaign().tier() != Tier::Station {
                break;
            }
        }
        assert!(
            saw_ascent,
            "the player's ascent should emit a TierAscended event"
        );
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
    fn exploiting_a_shortage_is_a_one_press_profit() {
        // ReactorFuel is dear at Ceres (the short market): exploiting sources it
        // from the cheaper Earth and sells into Ceres, no pre-held cargo (§0.4).
        let mut sim = Sim::new(0);
        let (ceres, rf) = (0usize, 5usize);
        assert_eq!(sim.corp().cargo(rf), 0);
        let start = sim.corp().credits();
        let profit = sim.exploit_shortage(ceres, rf, 20).unwrap();
        assert!(profit > 0, "exploiting a real shortage should profit");
        assert!(sim.corp().credits() > start);
        assert_eq!(
            sim.corp().cargo(rf),
            0,
            "the cargo round-trips through the warehouse"
        );
    }

    #[test]
    fn the_top_shortage_is_answerable_in_one_press() {
        // Run until a shortage is surfaced, then answer it from the feed.
        let mut sim = Sim::new(0);
        let mut answered = false;
        for _ in 0..2_000 {
            sim.step();
            if sim.feed().surfaced().iter().any(|a| a.is_act_now()) {
                answered = sim.answer_top_shortage(20);
                break;
            }
        }
        assert!(
            answered,
            "an open act-now shortage should be answerable in one press"
        );
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
    fn instant_trades_pay_a_brokerage_fee() {
        // Buying and selling the same lot at one market (no spread) must lose
        // money to the fee — instant liquidity is not free (§5 sink). The fee is
        // what makes hand-trading a decision instead of a riskless skim.
        let mut sim = Sim::new(0);
        let (m, c) = (0usize, 5usize);
        let start = sim.corp().credits();
        let spent = sim.buy(m, c, 10).unwrap();
        let got = sim.sell(m, c, 10).unwrap();
        assert!(
            got < spent,
            "a flat round-trip should lose the fee, got {got} vs {spent}"
        );
        assert!(sim.corp().credits() < start, "the fee leaves the treasury");
    }

    #[test]
    fn overhead_caps_runaway_hoarding() {
        // Operating overhead is a wealth-scaled sink: a treasury far above the
        // free float is skimmed each tick, so hoards can't compound without
        // bound. A small float below the threshold is left untouched.
        let mut sim = Sim::new(0);
        sim.corp.credit(900_000); // well above the free float (private field, test-only)
        let rich = sim.corp().credits();
        sim.step();
        assert!(
            sim.corp().credits() < rich,
            "overhead should skim a large treasury"
        );
        // A company at the float is not taxed (early/mid play stays clean).
        let mut lean = Sim::new(0);
        let base = lean.corp().credits();
        lean.step();
        assert_eq!(
            lean.corp().credits(),
            base,
            "a treasury at the free float pays no overhead"
        );
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
    fn building_and_routing_advance_the_spine_too() {
        // The retention spine used to count only interdictions; now the build and
        // logistics side of the influence model climbs it as well (§0). A few
        // commissions plus a self-running route should advance past the Station
        // with no raiding at all.
        use crate::sim::campaign::Tier;
        let mut sim = Sim::new(0);
        assert_eq!(sim.campaign().tier(), Tier::Station);
        // Two commissions are two operations on their own.
        sim.commission_freighter().unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        // A standing route then delivers itself toward the next rung.
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        for _ in 0..3_000 {
            sim.step();
            if sim.campaign().tier() != Tier::Station {
                break;
            }
        }
        assert_ne!(
            sim.campaign().tier(),
            Tier::Station,
            "build + route operations should climb the spine without interdiction"
        );
        // ...and none of it touched reputation (no cuts were made).
        for m in sim.markets() {
            assert!(sim.relations().standing(m.faction()) >= 0);
        }
    }

    #[test]
    fn the_fleet_can_actually_fight() {
        // The combat resolver was unreachable from the live loop; now a player
        // with warships can engage a raider pack and the battle resolves into a
        // BattleResolved event (§9). With no fleet, there's nothing to send.
        use crate::sim::combat::Band;
        let mut sim = Sim::new(0);
        assert!(
            sim.engage_raiders(Band::Medium).is_none(),
            "no warships ⇒ no engagement"
        );
        sim.commission_ship(ShipClass::Frigate).unwrap();
        sim.commission_ship(ShipClass::Frigate).unwrap();
        let fleet_before = sim.corp().fleet().len();
        let outcome = sim.engage_raiders(Band::Medium).expect("a fleet can fight");
        assert!(outcome.winner.is_some() || outcome.ticks > 0);
        // The battle resolves into an event the feed can voice (surviving the
        // step's player-event plumbing).
        let events = sim.step().to_vec();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::BattleResolved { .. })),
            "the engagement should emit a BattleResolved event"
        );
        // Losses are applied to the fleet (it can only shrink).
        assert!(sim.corp().fleet().len() <= fleet_before);
    }

    #[test]
    fn a_refinery_runs_the_value_add_chain_for_profit() {
        // Found a refinery (Ore → Metals): it sources cheap raw, refines it into a
        // dearer good, and auto-sells the surplus — hands-off (§3.1, Example A).
        let mut sim = Sim::new(0);
        let before = sim.corp().credits();
        sim.found_refinery(1, 0, 0).unwrap(); // Ore, buy+sell at Ceres
        assert_eq!(sim.stations().len(), 1);
        assert!(sim.corp().credits() < before, "founding costs capital");
        let after_found = sim.corp().credits();
        for _ in 0..1_500 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > after_found,
            "the refinery should net profit"
        );
    }

    #[test]
    fn refineries_are_guarded() {
        let mut sim = Sim::new(0);
        // A refined commodity is not a valid input recipe.
        assert_eq!(
            sim.found_refinery(5, 0, 0),
            Err(FoundError::NotARawCommodity)
        );
        // The Tier-1 station cap.
        for raw in [0, 1, 2, 0] {
            sim.found_refinery(raw, 0, 0).unwrap();
        }
        assert_eq!(sim.stations().len(), 4);
        assert_eq!(
            sim.found_refinery(1, 0, 0),
            Err(FoundError::TooManyStations)
        );
    }

    #[test]
    fn a_trade_route_runs_itself_for_profit() {
        // The standing-order heart (§4): set the params + own a freighter, and the
        // sim flies the loop, banking the spread with no further input.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        let start = sim.corp().credits();
        for _ in 0..2_000 {
            sim.step();
        }
        assert!(
            sim.corp().credits() > start,
            "the route should bank profit hands-off"
        );
    }

    #[test]
    fn a_route_needs_a_freighter_and_respects_its_margin() {
        // No freighter ⇒ no trips.
        let mut sim = Sim::new(0);
        sim.set_trade_route(5, 1, 0, 20, 1);
        let start = sim.corp().credits();
        for _ in 0..500 {
            sim.step();
        }
        assert_eq!(
            sim.corp().credits(),
            start,
            "no freighter ⇒ the route can't run"
        );
        // With a freighter but an unreachable margin, the route stays idle.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 100_000);
        let start = sim.corp().credits();
        for _ in 0..500 {
            sim.step();
        }
        assert_eq!(
            sim.corp().credits(),
            start,
            "spread below margin ⇒ idle (an exception)"
        );
    }

    #[test]
    fn the_route_table_runs_many_routes_on_a_shared_freighter_pool() {
        // The §4 master-table: several standing routes run concurrently, bounded
        // by how many freighters are in the pool. Two freighters + three routes
        // ⇒ at most two trips in flight at once, and the table still banks profit.
        let mut sim = Sim::new(0);
        sim.commission_freighter().unwrap();
        sim.commission_freighter().unwrap();
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        sim.set_trade_route(4, 0, 1, 20, 1); // Metals, Ceres → Earth
        sim.set_trade_route(1, 0, 1, 20, 1); // Ore, Ceres → Earth
        assert_eq!(sim.routes().len(), 3, "three routes sit in the table");
        let start = sim.corp().credits();
        let mut max_in_flight = 0;
        for _ in 0..2_000 {
            sim.step();
            let flying = sim.routes().iter().filter(|r| r.in_transit).count();
            max_in_flight = max_in_flight.max(flying);
        }
        assert!(
            max_in_flight <= 2,
            "two freighters cap concurrent trips at 2, saw {max_in_flight}"
        );
        assert!(max_in_flight >= 2, "both freighters should get used");
        assert!(sim.corp().credits() > start, "the table should bank profit");
    }

    #[test]
    fn the_route_table_is_capped() {
        let mut sim = Sim::new(0);
        for _ in 0..10 {
            sim.set_trade_route(5, 1, 0, 20, 1);
        }
        assert_eq!(sim.routes().len(), 4, "the table is capped at MAX_ROUTES");
        sim.clear_trade_route();
        assert!(sim.routes().is_empty(), "clearing empties the whole table");
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
    fn hostility_recovers_once_the_raiding_stops() {
        // Drive Earth to Hostile, then stop: standing must drift back toward
        // neutral over time (§10) — the cliff is now a dial.
        use crate::sim::faction::Faction;
        let mut sim = Sim::new(0);
        sim.relations_mut().adjust(Faction::Earth, -1_000);
        assert_eq!(sim.relations().standing(Faction::Earth), -1_000);
        for _ in 0..2_000 {
            sim.step();
        }
        let healed = sim.relations().standing(Faction::Earth);
        assert!(
            healed > -1_000,
            "Earth should be recovering, still at {healed}"
        );
        assert!(
            healed < 0,
            "but a deep grudge shouldn't fully heal that fast"
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
