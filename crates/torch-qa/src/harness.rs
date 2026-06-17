//! The playthrough harness — drives a [`Strategy`] through the sim and records
//! everything a gameplay review needs.
//!
//! It owns the loop the Godot shell owns at runtime (act → `step` → observe),
//! but headless and instrumented: every tick it lets the persona act, advances
//! the sim, tallies the event stream (§29), and periodically samples the world
//! state into a [`Transcript`]. The transcript is the raw material the
//! [`crate::review`] engine critiques.

use crate::strategy::Strategy;
use torch_core::sim::{Campaign, Event, Faction, Sim, Tier};

/// A periodic reading of the world during a playthrough — one row of the run's
/// telemetry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    pub tick: u64,
    pub credits: i64,
    pub trained_crew: i64,
    pub fleet: usize,
    pub freighters: i64,
    pub tier: &'static str,
    /// Gate approach in basis points (the always-visible far goal, §0.1).
    pub gate_bp: i64,
    pub now_progress: i64,
    pub now_target: i64,
    pub research_unlocked: usize,
    pub ceo_level: i64,
    pub blueprints_known: usize,
    pub haulers_in_flight: usize,
    pub surfaced_alerts: usize,
    pub act_now_open: usize,
    /// Standings in [`Faction::ALL`] order: Earth, Mars, Belt, Independents.
    pub standings: [i64; 4],
}

impl Sample {
    /// Read the current world state into a sample.
    fn capture(sim: &Sim) -> Self {
        let campaign: &Campaign = sim.campaign();
        let (_, now_progress, now_target) = campaign.now_goal();
        let prog = sim.progression();
        let rel = sim.relations();
        let standings = [
            rel.standing(Faction::Earth),
            rel.standing(Faction::Mars),
            rel.standing(Faction::Belt),
            rel.standing(Faction::Independents),
        ];
        let surfaced = sim.feed().surfaced();
        let act_now_open = surfaced.iter().filter(|a| a.is_act_now()).count();
        Self {
            tick: sim.tick(),
            credits: sim.corp().credits(),
            trained_crew: sim.corp().trained_crew(),
            fleet: sim.corp().fleet().len(),
            freighters: sim.corp().freighters(),
            tier: campaign.tier().name(),
            gate_bp: campaign.gate_progress_bp(),
            now_progress,
            now_target,
            research_unlocked: prog.research.unlocked_count(),
            ceo_level: prog.ceo.level(),
            blueprints_known: prog.blueprints.known_count(),
            haulers_in_flight: sim.haulers().len(),
            surfaced_alerts: surfaced.len(),
            act_now_open,
            standings,
        }
    }
}

/// The full record of one persona's playthrough.
#[derive(Clone, Debug)]
pub struct Transcript {
    pub persona: &'static str,
    pub intent: &'static str,
    pub seed: u64,
    pub ticks: u64,
    pub samples: Vec<Sample>,

    /// Discrete player actions issued across the run.
    pub actions: u64,
    /// Ticks on which at least one action was issued (decision density).
    pub action_ticks: u64,
    /// Act-now shortage alerts the persona answered with a verb.
    pub alerts_responded: u64,

    // Event-stream tallies (§29).
    pub haulers_departed: u64,
    pub haulers_arrived: u64,
    pub haulers_interdicted: u64,
    pub scarcities: u64,
    /// Act-now (shortage) alerts raised — every scarcity the *feed* saw (§19).
    pub act_now_raised: u64,
    /// Ticks with an open act-now alert pending — attention is demanded (§19).
    pub busy_ticks: u64,
    /// Longest run of ticks with nothing pending and no action — the dead time a
    /// player fast-forwards (time-compression + auto-pause-on-exception, §28).
    pub longest_idle_run: u64,
    /// `(tick, new tier name)` for each ascent, observed from campaign state so
    /// it's robust to player-verb events being dropped (see below).
    pub ascents: Vec<(u64, &'static str)>,
    pub first_tier_up: Option<u64>,
    pub gate_reached: Option<u64>,
    /// `TierAscended` events seen on the returned stream. Player-driven ascents
    /// happen in `act()` before `step()`, whose `events.clear()` wipes them — so
    /// a climb with `ascents` but zero events here exposes that dropped-event gap.
    pub tier_ascended_events: u64,
    /// Fleet engagements fought (§9), and how many the player won.
    pub battles_fought: u64,
    pub battles_won: u64,
    /// Incoming-threat forecasts seen on the stream (§13): the telegraph that
    /// keeps raids from arriving unforeseeable. A healthy world warns before it
    /// bites — `forecasts >= haulers_interdicted` means every strike was foreseen.
    pub forecasts: u64,
    /// Derelicts the world turned up, and how many the persona stripped (§15).
    pub wrecks_sighted: u64,
    pub wrecks_salvaged: u64,
    /// Player ships lost across all engagements (the felt cost of combat, §13).
    pub battle_losses: u64,
    /// Distinct event *kinds* the run produced — a breadth-of-systems proxy.
    pub distinct_event_kinds: u32,
    /// Highest pressure gauge reached at any sample (§13 tension peak, 0..=100).
    pub peak_pressure: i32,

    // Economy extremes.
    pub start_credits: i64,
    pub end_credits: i64,
    pub min_credits: i64,
    pub max_credits: i64,
    /// Sampled commodity readings sitting at a price wall (instability, §7c).
    pub wall_hits: u64,
}

impl Transcript {
    fn new(persona: &'static str, intent: &'static str, seed: u64, ticks: u64) -> Self {
        Self {
            persona,
            intent,
            seed,
            ticks,
            samples: Vec::new(),
            actions: 0,
            action_ticks: 0,
            alerts_responded: 0,
            haulers_departed: 0,
            haulers_arrived: 0,
            haulers_interdicted: 0,
            scarcities: 0,
            act_now_raised: 0,
            busy_ticks: 0,
            longest_idle_run: 0,
            ascents: Vec::new(),
            first_tier_up: None,
            gate_reached: None,
            tier_ascended_events: 0,
            battles_fought: 0,
            forecasts: 0,
            wrecks_sighted: 0,
            wrecks_salvaged: 0,
            battle_losses: 0,
            distinct_event_kinds: 0,
            peak_pressure: 0,
            battles_won: 0,
            start_credits: 0,
            end_credits: 0,
            min_credits: 0,
            max_credits: 0,
            wall_hits: 0,
        }
    }

    /// The tier the run finished at.
    pub fn tier_reached(&self) -> &'static str {
        self.samples
            .last()
            .map(|s| s.tier)
            .unwrap_or_else(|| Tier::Station.name())
    }

    /// Final standings sample (Earth, Mars, Belt, Independents), or all-neutral.
    pub fn final_standings(&self) -> [i64; 4] {
        self.samples.last().map(|s| s.standings).unwrap_or([0; 4])
    }

    /// Fraction of ticks (percent) on which the persona acted.
    pub fn action_density_pct(&self) -> u64 {
        if self.ticks == 0 {
            0
        } else {
            self.action_ticks * 100 / self.ticks
        }
    }

    /// Net treasury change across the run.
    pub fn net_gain(&self) -> i64 {
        self.end_credits - self.start_credits
    }

    /// Integer growth multiple of the treasury (end ÷ start, floored at 0).
    pub fn growth_multiple(&self) -> i64 {
        self.end_credits / self.start_credits.max(1)
    }

    /// The deepest peak-to-trough dip in the sampled treasury, as a percent of
    /// the running peak (0 = monotonic climb). A measure of felt setback — a line
    /// that only goes up has no tension.
    pub fn max_drawdown_pct(&self) -> u64 {
        let (mut peak, mut worst) = (self.start_credits.max(1), 0i64);
        for s in &self.samples {
            peak = peak.max(s.credits);
            let dip = (peak - s.credits) * 100 / peak;
            worst = worst.max(dip);
        }
        worst.max(0) as u64
    }

    /// Tiers experienced over the run (1 = stayed at the Station … 4 = the Gate).
    pub fn tiers_experienced(&self) -> u32 {
        self.ascents.len() as u32 + 1
    }
}

/// Record a tier change (the campaign only ever climbs) into the transcript,
/// observed from state so it survives even when a player-verb's `TierAscended`
/// event is dropped. Updates `last_tier` in place.
fn note_ascent(t: &mut Transcript, sim: &Sim, last_tier: &mut Tier) {
    let tier = sim.campaign().tier();
    if tier != *last_tier {
        let at = sim.tick();
        t.first_tier_up.get_or_insert(at);
        if tier == Tier::Gate {
            t.gate_reached.get_or_insert(at);
        }
        t.ascents.push((at, tier.name()));
        *last_tier = tier;
    }
}

/// Count sampled commodity readings sitting at (or past) a price wall — the
/// §7c gate normally keeps prices strictly between floor and ceiling, so any
/// hit is an instability signal.
fn walls_at(sim: &Sim) -> u64 {
    let mut hits = 0;
    for m in sim.markets() {
        for (c, def) in m.defs().iter().enumerate() {
            let p = m.price(c);
            if p <= def.floor || p >= def.ceiling {
                hits += 1;
            }
        }
    }
    hits
}

/// Play `strat` for `ticks` ticks from `seed`, sampling the world every
/// `sample_every` ticks, and return the transcript.
pub fn run(seed: u64, ticks: u64, sample_every: u64, mut strat: Box<dyn Strategy>) -> Transcript {
    let sample_every = sample_every.max(1);
    let mut sim = Sim::new(seed);
    let mut t = Transcript::new(strat.name(), strat.intent(), seed, ticks);

    // Baseline the tier *before* setup, so any ascent setup triggers (e.g. a
    // Warlord commissioning a squadron, which climbs the spine) is captured.
    let mut last_tier = sim.campaign().tier();
    strat.setup(&mut sim);
    note_ascent(&mut t, &sim, &mut last_tier);

    t.start_credits = sim.corp().credits();
    t.min_credits = t.start_credits;
    t.max_credits = t.start_credits;

    let mut last_events: Vec<Event> = Vec::new();
    let mut idle_run = 0u64;
    let mut seen_kinds = 0u32;
    for _ in 0..ticks {
        let actions = strat.act(&mut sim, &last_events);
        t.actions += actions as u64;
        if actions > 0 {
            t.action_ticks += 1;
        }

        last_events = sim.step().to_vec();

        // Is anything demanding attention this tick? A pending act-now alert means
        // a player would be stopped here; otherwise it's fast-forwardable dead time.
        let pending = sim.feed().surfaced().iter().any(|a| a.is_act_now());
        if pending {
            t.busy_ticks += 1;
        }
        if actions == 0 && !pending {
            idle_run += 1;
            t.longest_idle_run = t.longest_idle_run.max(idle_run);
        } else {
            idle_run = 0;
        }

        for e in &last_events {
            seen_kinds |= event_kind_bit(e);
            match e {
                Event::HaulerDeparted { .. } => t.haulers_departed += 1,
                Event::HaulerArrived { .. } => t.haulers_arrived += 1,
                Event::HaulerInterdicted { .. } => t.haulers_interdicted += 1,
                Event::Scarcity { .. } => {
                    t.scarcities += 1;
                    t.act_now_raised += 1;
                }
                Event::TierAscended { .. } => t.tier_ascended_events += 1,
                Event::BattleResolved { won, losses } => {
                    t.battles_fought += 1;
                    t.battle_losses += *losses as u64;
                    if *won {
                        t.battles_won += 1;
                    }
                }
                Event::ThreatForecast { .. } => t.forecasts += 1,
                Event::WreckSighted { .. } => t.wrecks_sighted += 1,
                Event::WreckSalvaged { .. } => t.wrecks_salvaged += 1,
                // The endgame transit + bridgehead + incursion beats (§17) — personas
                // don't reach them, so just fold them into the ascent tally.
                Event::GateTransited
                | Event::BridgeheadFounded
                | Event::BridgeheadUpgraded { .. }
                | Event::IncursionStruck { .. }
                | Event::BridgeheadDamaged { .. }
                | Event::BridgeheadFell
                | Event::EndgameWon
                | Event::EndgameLost
                | Event::ColonyAcquired { .. } => t.tier_ascended_events += 1,
                Event::Tick { .. } => {}
            }
        }
        t.peak_pressure = t.peak_pressure.max(sim.pressure().peak_level());

        // Observe ascents from campaign *state* (the tier only ever climbs), so
        // we capture player-driven ascents even though their events are wiped.
        note_ascent(&mut t, &sim, &mut last_tier);

        let credits = sim.corp().credits();
        t.min_credits = t.min_credits.min(credits);
        t.max_credits = t.max_credits.max(credits);

        if sim.tick().is_multiple_of(sample_every) || sim.tick() == ticks {
            t.samples.push(Sample::capture(&sim));
            t.wall_hits += walls_at(&sim);
        }
    }

    t.distinct_event_kinds = seen_kinds.count_ones();
    t.end_credits = sim.corp().credits();
    t.alerts_responded = strat.responses();
    t
}

/// A one-bit discriminant per event *kind* (excluding the always-present `Tick`),
/// so OR-ing them across a run counts the distinct kinds the world produced — a
/// breadth-of-systems signal for the engagement assessment.
fn event_kind_bit(e: &Event) -> u32 {
    match e {
        Event::Tick { .. } => 0,
        Event::HaulerDeparted { .. } => 1 << 0,
        Event::HaulerArrived { .. } => 1 << 1,
        Event::HaulerInterdicted { .. } => 1 << 2,
        Event::Scarcity { .. } => 1 << 3,
        Event::TierAscended { .. } => 1 << 4,
        Event::BattleResolved { .. } => 1 << 5,
        Event::ThreatForecast { .. } => 1 << 6,
        Event::WreckSighted { .. } => 1 << 7,
        Event::WreckSalvaged { .. } => 1 << 8,
        // The endgame transit + bridgehead + incursion beats are the supreme ascents —
        // fold them into the ascent bit so the variety denominator is unchanged (no
        // persona reaches them anyway).
        Event::GateTransited
        | Event::BridgeheadFounded
        | Event::BridgeheadUpgraded { .. }
        | Event::IncursionStruck { .. }
        | Event::BridgeheadDamaged { .. }
        | Event::BridgeheadFell
        | Event::EndgameWon
        | Event::EndgameLost
        | Event::ColonyAcquired { .. } => 1 << 4,
    }
}

/// The number of distinct event kinds the engagement variety facet scores
/// against (the bits `event_kind_bit` can set).
pub const EVENT_KIND_COUNT: u32 = 9;
