//! Autoplayer personas — each a play style the QA harness drives end-to-end.
//!
//! A [`Strategy`] is a programmatic player: it reads the same world a human
//! would and presses the same verbs on [`Sim`] (buy/sell, commission, set a
//! route, interdict, set policy). The roster spans the spectrum the influence
//! model cares about — pure observation, hand-trading, standing orders, raiding,
//! and a balanced operator — so the review can compare how each *feels* and
//! where the design pushes or starves the player.

use torch_core::sim::{Band, Event, Hauler, Interceptor, Interdiction, ShipClass, Sim};

/// A programmatic player. The harness calls [`Strategy::setup`] once, then
/// [`Strategy::act`] every tick before advancing the sim.
pub trait Strategy {
    /// Short persona name for the report.
    fn name(&self) -> &'static str;

    /// One line on the play style under test (shown in the review).
    fn intent(&self) -> &'static str;

    /// One-time setup at tick 0 — commission ships, set routes, set policy.
    fn setup(&mut self, _sim: &mut Sim) {}

    /// Decide and apply this tick's actions, given the previous tick's events
    /// (the same stream the alert feed consumes). Returns how many discrete
    /// player actions were issued.
    fn act(&mut self, sim: &mut Sim, last_events: &[Event]) -> u32;

    /// Act-now shortage alerts the persona answered with a verb over the run.
    fn responses(&self) -> u64 {
        0
    }
}

// ---- Shared helpers (the verbs the personas press) -------------------------

/// The fattest instant spread across the two markets right now:
/// `(commodity, cheap_market, dear_market, spread)`.
pub fn best_spread(sim: &Sim) -> Option<(usize, usize, usize, i64)> {
    if sim.markets().len() < 2 {
        return None;
    }
    let n = sim.markets()[0].defs().len();
    let mut best: Option<(usize, usize, usize, i64)> = None;
    let mut best_spread = 0;
    for c in 0..n {
        let p0 = sim.markets()[0].price(c);
        let p1 = sim.markets()[1].price(c);
        let (cheap, dear, spread) = if p0 <= p1 {
            (0, 1, p1 - p0)
        } else {
            (1, 0, p0 - p1)
        };
        if spread > best_spread {
            best_spread = spread;
            best = Some((c, cheap, dear, spread));
        }
    }
    best
}

/// One round of by-hand arbitrage on the fattest spread, up to `cap` units:
/// buy cheap, sell dear (the verbs are instant, so this is a teleport trade).
/// Only fires when the spread clears the round-trip brokerage fee — hand-trading
/// is a decision against the fee, not a free skim. Returns whether it traded.
fn arbitrage_once(sim: &mut Sim, cap: i64) -> bool {
    let Some((c, cheap, dear, spread)) = best_spread(sim) else {
        return false;
    };
    let buy_price = sim.markets()[cheap].price(c).max(1);
    let sell_price = sim.markets()[dear].price(c);
    let affordable = sim.corp().credits() / buy_price;
    let available = sim.markets()[cheap].stock(c);
    let qty = cap.min(affordable).min(available);
    if qty <= 0 {
        return false;
    }
    // Net of the fee on both legs; skip anything that wouldn't clear a profit.
    let fee = (buy_price + sell_price) * qty * Sim::TRADE_FEE_BP / 10_000;
    if spread * qty <= fee {
        return false;
    }
    if sim.buy(cheap, c, qty).is_err() {
        return false;
    }
    sim.sell(dear, c, qty).is_ok()
}

/// A fast interceptor sitting right on a hauler — a firing solution the
/// player's frigate can take (mirrors the §7b interdiction geometry).
fn frigate_on(h: &Hauler, tick: u64) -> Interceptor {
    Interceptor {
        pos: h.position(tick),
        speed: 200_000,
        skill_bp: 600,
    }
}

/// Attempt to cut the fattest hauler in flight. Returns whether a cut landed.
fn strike_fattest(sim: &mut Sim) -> bool {
    let tick = sim.tick();
    let target = sim
        .haulers()
        .iter()
        .max_by_key(|h| h.qty)
        .map(|h| (h.id, frigate_on(h, tick)));
    match target {
        Some((id, interceptor)) => {
            matches!(
                sim.interdict_with(id, interceptor),
                Interdiction::Interdicted
            )
        }
        None => false,
    }
}

// ---- The roster ------------------------------------------------------------

/// Touches nothing — measures whether the world is alive and watchable on its
/// own (§28 real-time-with-pause: the sim should be worth watching).
pub struct Spectator;

impl Strategy for Spectator {
    fn name(&self) -> &'static str {
        "Spectator"
    }
    fn intent(&self) -> &'static str {
        "Touches nothing — is the world alive and worth watching with hands off the controls? (§28)"
    }
    fn act(&mut self, _sim: &mut Sim, _last_events: &[Event]) -> u32 {
        0
    }
}

/// Works the price spread by hand every tick — stress-tests the trade loop and
/// whether its returns are bounded (§5/§7a).
pub struct Arbitrageur;

impl Strategy for Arbitrageur {
    fn name(&self) -> &'static str {
        "Arbitrageur"
    }
    fn intent(&self) -> &'static str {
        "Hand-trades the spread every tick — does the economy stay a decision, or become a faucet? (§5/§7a)"
    }
    fn act(&mut self, sim: &mut Sim, _last_events: &[Event]) -> u32 {
        u32::from(arbitrage_once(sim, 40))
    }
}

/// Sets one standing trade route and walks away — tests the parameterized
/// standing-order heart of the influence model (§4).
pub struct Logistician;

impl Strategy for Logistician {
    fn name(&self) -> &'static str {
        "Logistician"
    }
    fn intent(&self) -> &'static str {
        "Fills a small route table, then leaves — does the policy→execute→exception loop pay off hands-off across many routes? (§4)"
    }
    fn setup(&mut self, sim: &mut Sim) {
        // A two-freighter pool running a table of routes (the §4 master-table).
        let _ = sim.commission_freighter();
        let _ = sim.commission_freighter();
        sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
        sim.set_trade_route(4, 0, 1, 20, 1); // Metals, Ceres → Earth
    }
    fn act(&mut self, _sim: &mut Sim, _last_events: &[Event]) -> u32 {
        0
    }
}

/// Hunts the lanes — climbs the campaign spine by interdiction and pays for it
/// in reputation (§7b/§0). The only style that advances the gate.
pub struct Privateer;

impl Strategy for Privateer {
    fn name(&self) -> &'static str {
        "Privateer"
    }
    fn intent(&self) -> &'static str {
        "Cuts every convoy it can — climbs the retention spine and tracks the reputation cost (§7b/§0)."
    }
    fn act(&mut self, sim: &mut Sim, _last_events: &[Event]) -> u32 {
        if sim.haulers().is_empty() {
            0
        } else {
            // An attempt is an action whether or not the cut lands.
            strike_fattest(sim);
            1
        }
    }
}

/// The intended operator: trades for cash, runs a route, climbs by measured
/// raids, auto-researches, and answers act-now shortages (the full loop, §0–§19).
#[derive(Default)]
pub struct Tycoon {
    responses: u64,
}

impl Strategy for Tycoon {
    fn name(&self) -> &'static str {
        "Tycoon"
    }
    fn intent(&self) -> &'static str {
        "The intended full-loop operator: trade, route, raid to climb, auto-research, and answer shortages (§0–§19)."
    }
    fn setup(&mut self, sim: &mut Sim) {
        let _ = sim.commission_freighter();
        sim.set_trade_route(5, 1, 0, 20, 1);
        let _ = sim.commission_ship(ShipClass::Frigate);
        sim.policy_mut().auto_research = true;
    }
    fn act(&mut self, sim: &mut Sim, last_events: &[Event]) -> u32 {
        let mut actions = 0;

        // 1. Answer last tick's act-now shortages in one press: source the scarce
        //    good cheap and sell it into the short market (the ExploitShortage
        //    verb, §0.4) — no pre-held cargo needed.
        for e in last_events {
            if let Event::Scarcity { market, commodity } = e {
                if sim.exploit_shortage(*market, *commodity, 20).is_ok() {
                    self.responses += 1;
                    actions += 1;
                }
            }
        }

        // 2. Hands-on arbitrage for working capital.
        if arbitrage_once(sim, 25) {
            actions += 1;
        }

        // 3. Climb on a measured cadence, so raids don't suppress the very
        //    traffic the economy feeds on.
        if sim.tick().is_multiple_of(48) && !sim.haulers().is_empty() {
            strike_fattest(sim);
            actions += 1;
        }

        actions
    }
    fn responses(&self) -> u64 {
        self.responses
    }
}

/// Builds a war fleet and throws it at raider packs — exercises the combat
/// resolver the live loop never reached, and tracks the attrition (§7/§9).
pub struct Warlord;

impl Strategy for Warlord {
    fn name(&self) -> &'static str {
        "Warlord"
    }
    fn intent(&self) -> &'static str {
        "Stands up warships and fights raider packs — is the combat resolver reachable, and does attrition bite? (§7/§9)"
    }
    fn setup(&mut self, sim: &mut Sim) {
        // Stand up a small initial squadron — keep crew in reserve to rebuild
        // after a costly fight, rather than blowing the whole §8c pool at once.
        for _ in 0..2 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
    }
    fn act(&mut self, sim: &mut Sim, _last_events: &[Event]) -> u32 {
        let mut actions = 0;
        // Reinforce whenever the treasury and the crew pool allow (rebuilding the
        // squadron between battles, until the §8c bottleneck runs dry).
        if sim.tick().is_multiple_of(18) && sim.commission_ship(ShipClass::Frigate).is_ok() {
            actions += 1;
        }
        // Pick a fight on a cadence whenever there's a fleet to send. Frigates
        // carry no railgun — they knife-fight Close, where the PDC brawl resolves
        // (at range a screened salvo-only mirror just stalemates). Combat is
        // decisive and crew-capped, so these are a few pivotal battles, not a grind.
        if sim.tick().is_multiple_of(40) && !sim.corp().fleet().is_empty() {
            sim.engage_raiders(Band::Close);
            actions += 1;
        }
        actions
    }
}

/// Grows a station/colony empire by acquisition, then holds it — the QA lens on the
/// empire layer (E1–E6): does expansion pay, and do overextension + the coalition bite?
#[derive(Default)]
pub struct Expansionist;

impl Strategy for Expansionist {
    fn name(&self) -> &'static str {
        "Expansionist"
    }
    fn intent(&self) -> &'static str {
        "Buys frontier colonies and holds them — does expansion pay off, and do administrative strain + the great-power coalition cap it? (Empire layer E1–E6)"
    }
    fn setup(&mut self, sim: &mut Sim) {
        // A standing squadron to answer the coalition when it comes for the holdings.
        for _ in 0..3 {
            let _ = sim.commission_ship(ShipClass::Frigate);
        }
    }
    fn act(&mut self, sim: &mut Sim, _last_events: &[Event]) -> u32 {
        let mut actions = 0;
        // Answer a coalition strike the moment it lands (defend the empire, E3).
        if sim.coalition_strike_pending() && !sim.corp().fleet().is_empty() {
            sim.defend_holdings(Band::Close);
            actions += 1;
        }
        // Fund expansion by hand-trading — but only while saving up, so this is a war
        // chest, not an unbounded arbitrage faucet.
        if sim.corp().credits() < 150_000
            && sim.tick().is_multiple_of(8)
            && arbitrage_once(sim, 300)
        {
            actions += 1;
        }
        // Reinforce the squadron between defenses, until the §8c crew pool runs dry.
        if sim.tick().is_multiple_of(50) && sim.commission_ship(ShipClass::Frigate).is_ok() {
            actions += 1;
        }
        // Expand: buy the cheapest acquirable colony whenever affordable (E1)…
        if sim.tick().is_multiple_of(90) {
            if let Some(&i) = sim
                .acquirable_colonies()
                .iter()
                .min_by_key(|&&i| sim.colony_acquire_cost(i).unwrap_or(i64::MAX))
            {
                if sim.acquire_colony(i).is_ok() {
                    actions += 1;
                }
            }
        }
        // …and build out industry, deliberately pushing the empire past the coalition
        // threshold so the QA lens exercises the overextension teeth (E2/E3).
        if sim.tick().is_multiple_of(140) && sim.found_refinery(0, 0, 1).is_ok() {
            actions += 1;
        }
        actions
    }
}

/// Answers the act-now **dilemma** feed — resolves every shortage/wreck/raid the world
/// surfaces (Phase A / §0.4). The QA lens on the Agency loop: does engaging the
/// exception menu pay off, and does answering climb the §0 spine (A2)?
#[derive(Default)]
pub struct Responder {
    responses: u64,
}

impl Strategy for Responder {
    fn name(&self) -> &'static str {
        "Responder"
    }
    fn intent(&self) -> &'static str {
        "Answers the act-now feed — speculates shortages, strips wrecks, hunts raiders. Does engaging the exception menu pay off and climb the spine? (Phase A / §0.4)"
    }
    fn act(&mut self, sim: &mut Sim, _last_events: &[Event]) -> u32 {
        let mut actions = 0;
        // Clear the dilemma menu each tick: resolve the top option (Speculate / Strip /
        // Hunt) until it's empty — every answer is also a §0 operation (A2).
        while !sim.decisions().is_empty() && actions < 4 {
            if sim.resolve_decision(0, 0).is_err() {
                break;
            }
            self.responses += 1;
            actions += 1;
        }
        actions
    }
    fn responses(&self) -> u64 {
        self.responses
    }
}

/// The full roster the report runs.
pub fn roster() -> Vec<Box<dyn Strategy>> {
    vec![
        Box::new(Spectator),
        Box::new(Arbitrageur),
        Box::new(Logistician),
        Box::new(Privateer),
        Box::new(Warlord),
        Box::new(Tycoon::default()),
        Box::new(Expansionist),
        Box::new(Responder::default()),
    ]
}
