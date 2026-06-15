//! The review engine — turns playthrough transcripts into a written gameplay
//! review.
//!
//! Tests prove systems *work*; this critiques how the game *plays*. Each
//! heuristic reads a [`Transcript`] and emits [`Finding`]s about pacing,
//! agency, economy bounds, alert engagement, and reputation. A final
//! cross-cutting [`design_review`] compares personas to surface the structural
//! observations only a full playthrough reveals (e.g. that a single verb feeds
//! the retention spine). Deterministic in, deterministic out (§27).

use crate::harness::{run, Transcript};
use crate::strategy::roster;
use std::fmt::Write as _;
use torch_core::sim::Faction;

/// How a finding reads on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Working as the design wants.
    Good,
    /// Neutral context.
    Info,
    /// Worth a designer's attention.
    Note,
    /// A gap that undercuts the intended experience.
    Concern,
}

impl Severity {
    fn tag(self) -> &'static str {
        match self {
            Severity::Good => "GOOD",
            Severity::Info => "INFO",
            Severity::Note => "NOTE",
            Severity::Concern => "CONCERN",
        }
    }
}

/// One observation about the play experience.
#[derive(Clone, Debug)]
pub struct Finding {
    pub severity: Severity,
    pub area: &'static str,
    pub message: String,
}

impl Finding {
    fn new(severity: Severity, area: &'static str, message: impl Into<String>) -> Self {
        Self {
            severity,
            area,
            message: message.into(),
        }
    }
}

fn days(ticks: u64) -> u64 {
    ticks / 24
}

/// Critique a single persona's playthrough.
pub fn review(t: &Transcript) -> Vec<Finding> {
    let mut f = Vec::new();
    let active = t.actions > 0;

    review_pacing(t, active, &mut f);
    review_economy(t, active, &mut f);
    review_agency(t, active, &mut f);
    review_alerts(t, &mut f);
    review_pressure(t, &mut f);
    review_reputation(t, &mut f);
    review_fleet(t, &mut f);

    f
}

/// §13 pressure: is the world telegraphing its threats? Each ambient-raid window
/// emits a forecast `FORECAST_LEAD` ticks ahead, so the player can pre-position or
/// divert — nothing arrives unforeseeable. (We can't compare to cuts here: the
/// `haulers_interdicted` tally folds in the player's *own* interdictions, which
/// are chosen, not forecast.)
fn review_pressure(t: &Transcript, f: &mut Vec<Finding>) {
    if t.forecasts == 0 {
        return; // a placid stretch with no raids to forecast
    }
    f.push(Finding::new(
        Severity::Good,
        "Pressure",
        format!(
            "Incoming raids were telegraphed {} times across the run (§13 forecasting) — threats arrive foreseen, not out of nowhere, and the pacing governor holds spikes apart.",
            t.forecasts
        ),
    ));
}

fn review_pacing(t: &Transcript, active: bool, f: &mut Vec<Finding>) {
    match t.gate_reached {
        Some(at) => {
            let short = at < 600;
            f.push(Finding::new(
                if short { Severity::Concern } else { Severity::Good },
                "Pacing",
                format!(
                    "Opened the ring-gate at tick {at} (~{} days){}.",
                    days(at),
                    if short {
                        " — the whole climb collapses fast; the retention spine needs more rungs or steeper ops-per-tier"
                    } else {
                        ""
                    }
                ),
            ));
        }
        None if !t.ascents.is_empty() => f.push(Finding::new(
            Severity::Info,
            "Pacing",
            format!(
                "Climbed to {} ({} ascent(s)) but did not reach the gate within {} ticks (~{} days).",
                t.tier_reached(),
                t.ascents.len(),
                t.ticks,
                days(t.ticks)
            ),
        )),
        None => f.push(Finding::new(
            if active { Severity::Note } else { Severity::Info },
            "Pacing",
            "Never advanced the campaign — this play style completed no operations (a cut, a commissioned ship, a founded station, or a delivered route), so it never drew the gate closer."
                .to_string(),
        )),
    }
}

fn review_economy(t: &Transcript, active: bool, f: &mut Vec<Finding>) {
    let gain = t.net_gain();
    let mult = t.growth_multiple();
    if gain > t.start_credits * 19 {
        f.push(Finding::new(
            Severity::Concern,
            "Economy",
            format!(
                "Treasury ran away: {} → {} cr (~{mult}×) with no wealth-scaled sink. Repeated arbitrage compounds without bound, so trading stops being a decision and becomes a faucet.",
                t.start_credits, t.end_credits
            ),
        ));
    } else if gain > 0 {
        f.push(Finding::new(
            Severity::Good,
            "Economy",
            format!(
                "Turned a profit hands-on/over time: {} → {} cr (+{gain}, ~{mult}×).",
                t.start_credits, t.end_credits
            ),
        ));
    } else if gain == 0 && active {
        f.push(Finding::new(
            Severity::Note,
            "Economy",
            "Active all run but treasury never moved — the loop found no work (e.g. a standing order idle below its margin). That idle state is the exception the feed should surface."
                .to_string(),
        ));
    } else if gain < 0 {
        f.push(Finding::new(
            Severity::Note,
            "Economy",
            format!(
                "Lost money over the run: {} → {} cr ({gain}).",
                t.start_credits, t.end_credits
            ),
        ));
    }

    if t.wall_hits > 0 {
        f.push(Finding::new(
            Severity::Concern,
            "Economy",
            format!(
                "Prices hit a wall on {} sampled commodity-readings — the §7c gate normally keeps prices strictly off floor/ceiling, so this play style destabilized a market.",
                t.wall_hits
            ),
        ));
    }
}

fn review_agency(t: &Transcript, active: bool, f: &mut Vec<Finding>) {
    if active {
        let density = t.action_density_pct();
        // Low action density is only a pacing problem if the dead time isn't
        // fast-forwardable — i.e. there are long stretches with *nothing pending*.
        // With frequent exceptions + time-compression + auto-pause-on-exception
        // (§28), the player compresses the quiet and is only stopped when an
        // act-now alert fires, so a low density with a short idle run is fine.
        let idle = t.longest_idle_run;
        if density >= 5 {
            f.push(Finding::new(
                Severity::Info,
                "Agency",
                format!("Issued {} actions across {density}% of ticks.", t.actions),
            ));
        } else if idle <= 240 {
            // ≤ ~10 days ⇒ ~10 s at 24× — fast-forwardable, and the §21 "felt
            // vastness" of a quiet burn rather than a pacing dead-zone.
            f.push(Finding::new(
                Severity::Good,
                "Agency",
                format!(
                    "Acted on {density}% of ticks ({} actions), but the dead time is fast-forwardable: the longest stretch with nothing pending was {idle} ticks (~{} days, ~{} s at 24×). With time-compression + auto-pause-on-exception (§28), the player compresses the quiet and is stopped only when an act-now alert fires.",
                    t.actions,
                    idle / 24,
                    idle / 24
                ),
            ));
        } else {
            f.push(Finding::new(
                Severity::Note,
                "Agency",
                format!(
                    "Acted on only {density}% of ticks ({} actions), and the longest dead stretch with nothing pending ran {idle} ticks — the world needs denser exceptions there, not just faster time-compression (§36).",
                    t.actions
                ),
            ));
        }
    } else if t.haulers_departed > 0 && t.act_now_raised > 0 {
        f.push(Finding::new(
            Severity::Good,
            "Watchability",
            format!(
                "Hands fully off, the world stayed alive: {} convoys flew, {} cut on the lanes, {} shortages voiced. There is something to watch before there is something to do.",
                t.haulers_departed, t.haulers_interdicted, t.scarcities
            ),
        ));
    } else {
        f.push(Finding::new(
            Severity::Concern,
            "Watchability",
            format!(
                "Hands off, little happened ({} departures, {} alerts) — the idle world may not hold attention.",
                t.haulers_departed, t.act_now_raised
            ),
        ));
    }
}

fn review_alerts(t: &Transcript, f: &mut Vec<Finding>) {
    if t.act_now_raised == 0 {
        f.push(Finding::new(
            Severity::Note,
            "Alert feed",
            format!(
                "No act-now alerts in {} ticks — the feed had nothing urgent to say for this style. Good for calm, but the §0.4 'exceptions are verbs' loop never fires.",
                t.ticks
            ),
        ));
    } else if t.alerts_responded == 0 {
        f.push(Finding::new(
            Severity::Note,
            "Alert feed",
            format!(
                "{} act-now shortages were raised but none were acted on. The ExploitShortage verb needs matching cargo already on hand to exercise — there's no one-press path from the alert to the trade.",
                t.act_now_raised
            ),
        ));
    } else {
        f.push(Finding::new(
            Severity::Good,
            "Alert feed",
            format!(
                "Closed the alert→verb loop: answered {} of {} act-now shortages.",
                t.alerts_responded, t.act_now_raised
            ),
        ));
    }
}

fn review_reputation(t: &Transcript, f: &mut Vec<Finding>) {
    let standings = t.final_standings();
    for (i, &s) in standings.iter().enumerate() {
        let faction = Faction::ALL[i].name();
        if s <= -600 {
            f.push(Finding::new(
                Severity::Note,
                "Reputation",
                format!(
                    "Sustained raiding kept {faction} pinned at Hostile ({s}). Standings now heal toward neutral when the raids stop (a recoverable dial, not a one-way cliff) — but a persona that raids every tick outruns the drift, so the price is still real."
                ),
            ));
        } else if s >= 200 {
            f.push(Finding::new(
                Severity::Info,
                "Reputation",
                format!("{faction} warmed to {s} over the run."),
            ));
        }
    }
}

fn review_fleet(t: &Transcript, f: &mut Vec<Finding>) {
    if let Some(s) = t.samples.last() {
        if s.fleet > 0 || s.freighters > 0 {
            f.push(Finding::new(
                Severity::Info,
                "Fleet",
                format!(
                    "Fielded {} warship(s) and {} freighter(s); trained-crew pool at {} (the §8c bottleneck that caps capital ships).",
                    s.fleet, s.freighters, s.trained_crew
                ),
            ));
        }
    }
}

fn find<'a>(runs: &'a [Transcript], persona: &str) -> Option<&'a Transcript> {
    runs.iter().find(|t| t.persona == persona)
}

/// Cross-cutting findings that only a *comparison* of play styles reveals.
pub fn design_review(runs: &[Transcript]) -> Vec<Finding> {
    let mut f = Vec::new();

    // 1. What feeds the retention spine? The concern is the *build/trade* side
    //    being unable to climb — a hands-off Logistician advancing proves the
    //    spine listens to more than interdiction.
    let non_raiding_climbers: Vec<&str> = runs
        .iter()
        .filter(|t| !t.ascents.is_empty() && t.persona != "Privateer")
        .map(|t| t.persona)
        .collect();
    if non_raiding_climbers.is_empty() {
        f.push(Finding::new(
            Severity::Concern,
            "Retention spine",
            "Only interdiction advances the gate — trading, routing, and building never move a tier, so most of the influence model doesn't feed the §0 destination pull. The spine wants more verbs to count as operations."
                .to_string(),
        ));
    } else {
        f.push(Finding::new(
            Severity::Good,
            "Retention spine",
            format!(
                "The spine listens to more than raiding: {non_raiding_climbers:?} climbed without cutting a single convoy (commissions, founded stations, and delivered routes now count as operations). Pure manual teleport-trade still doesn't climb — by design, it's the degenerate verb."
            ),
        ));
    }

    // 2. Is combat reachable from the live loop?
    let battles: u64 = runs.iter().map(|t| t.battles_fought).sum();
    if battles == 0 {
        f.push(Finding::new(
            Severity::Concern,
            "Combat",
            "The combat resolver (sim::combat) has no trigger in the live Sim loop — there is no fleet-engagement verb on Sim, so no playthrough can reach it. Ships are commissioned but never fight; combat is only exercised by the shipyard's demo_duel. The §7/§9 depth is dark for the player."
                .to_string(),
        ));
    } else {
        let won: u64 = runs.iter().map(|t| t.battles_won).sum();
        f.push(Finding::new(
            Severity::Good,
            "Combat",
            format!(
                "Combat is reachable from the live loop: {battles} fleet engagements fought ({won} held the field) via Sim::engage_raiders, with losses applied to the fleet and a BattleResolved alert voiced — the §7/§9 resolver is in play, not just demo_duel."
            ),
        ));
        // A matched mirror that the player almost always loses (or wins) is a
        // balance signal — but only judge it off a meaningful sample. Combat is
        // crew-capped and decisive (§8c/§13), so a persona may fight only a
        // handful of pivotal battles; the resolver's coin-flip balance is proven
        // separately by `matched_raider_fights_are_a_competitive_coin_flip`.
        let win_pct = won * 100 / battles;
        if battles >= 12 && (win_pct <= 10 || win_pct >= 90) {
            f.push(Finding::new(
                Severity::Note,
                "Combat",
                format!(
                    "Lopsided outcomes: the player held the field in {win_pct}% of matched engagements. A symmetric raider pack should be closer to a coin-flip — initiative/doctrine in the resolver, or pack sizing, needs a balance pass."
                ),
            ));
        }
    }

    // 3. Hand-trading vs the standing route it is meant to motivate. Manual
    //    trade now pays a brokerage fee the route avoids, and routing buys
    //    progression the raw credits can't — so they're complementary, not a
    //    strict domination. Only flag if manual *still* dwarfs routing.
    if let (Some(arb), Some(log)) = (find(runs, "Arbitrageur"), find(runs, "Logistician")) {
        if arb.end_credits > log.end_credits * 5 {
            f.push(Finding::new(
                Severity::Note,
                "Economy",
                format!(
                    "Hand-trading still out-earns the standing route by a wide margin ({} vs {} cr) despite the brokerage fee — the instant verb's per-tick frequency, not its return, is the remaining edge. Throttling instant trades or raising route throughput is the lever.",
                    arb.end_credits, log.end_credits
                ),
            ));
        } else {
            f.push(Finding::new(
                Severity::Good,
                "Economy",
                format!(
                    "Hand-trading no longer dominates the route: a brokerage fee prices the instant verb's free liquidity ({} cr by hand vs {} cr routed), and routing now also climbs the spine — so the two are complementary, not strictly ordered.",
                    arb.end_credits, log.end_credits
                ),
            ));
        }
    }

    // 4. Is the treasury bounded? The wealth-scaled overhead should pull every
    //    income strategy to a sustainable equilibrium instead of a faucet.
    if let Some(arb) = find(runs, "Arbitrageur") {
        let mult = arb.growth_multiple();
        if mult >= 5 {
            f.push(Finding::new(
                Severity::Concern,
                "Economy",
                format!(
                    "No effective wealth sink: the Arbitrageur compounded ~{mult}× on one repeated press. Arbitrage needs diminishing returns or a sink (upkeep, taxes, build costs that scale) to stay a decision."
                ),
            ));
        } else {
            f.push(Finding::new(
                Severity::Good,
                "Economy",
                format!(
                    "Wealth-scaled overhead bounds the faucet: the Arbitrageur settled at ~{}× (≈{} cr) instead of compounding without limit — accumulation now hits a sustainable equilibrium where overhead meets income.",
                    mult, arb.end_credits
                ),
            ));
        }
    }

    // 5. A table of routes, not a single Option.
    f.push(Finding::new(
        Severity::Good,
        "Logistics",
        "The standing-order layer is a *table* now (Sim::routes): many routes run concurrently against a shared freighter pool, each with its own params and idle/in-transit exception — the spreadsheet-sim master-table the influence model wants, not a single Option."
            .to_string(),
    ));

    // 6. Player-verb events are wiped before anyone consumes them.
    let dropped: Vec<&str> = runs
        .iter()
        .filter(|t| !t.ascents.is_empty() && t.tier_ascended_events == 0)
        .map(|t| t.persona)
        .collect();
    if !dropped.is_empty() {
        f.push(Finding::new(
            Severity::Concern,
            "Event plumbing",
            format!(
                "Player verbs called between ticks push events that the next `step()` clears before the feed or the returned stream reads them: {dropped:?} climbed whole tiers yet emitted zero TierAscended events, and a player interdiction's Scarcity never reaches the feed. So player cuts raise no act-now alert and milestones go unvoiced — only sim-internal cuts (pirates/automation) are heard. The §0.3 ascent fanfare and the §0.4 'exploit shortage' verb only fire for events the player didn't cause."
            ),
        ));
    }

    f
}

// ---- Markdown rendering ----------------------------------------------------

fn render_findings(out: &mut String, findings: &[Finding]) {
    for finding in findings {
        let _ = writeln!(
            out,
            "- **[{}]** _{}_ — {}",
            finding.severity.tag(),
            finding.area,
            finding.message
        );
    }
}

fn render_persona(out: &mut String, t: &Transcript) {
    let standings = t.final_standings();
    let last = t.samples.last();
    let ceo = last.map(|s| s.ceo_level).unwrap_or(1);
    let research = last.map(|s| s.research_unlocked).unwrap_or(0);
    let gate_pct = last.map(|s| s.gate_bp / 100).unwrap_or(0);

    let _ = writeln!(out, "## {}\n", t.persona);
    let _ = writeln!(out, "_{}_\n", t.intent);
    let _ = writeln!(out, "| metric | value |");
    let _ = writeln!(out, "| --- | --- |");
    let _ = writeln!(
        out,
        "| treasury | {} → {} cr (+{}, ~{}×) |",
        t.start_credits,
        t.end_credits,
        t.net_gain(),
        t.growth_multiple()
    );
    let _ = writeln!(
        out,
        "| actions | {} over {}% of ticks |",
        t.actions,
        t.action_density_pct()
    );
    let _ = writeln!(
        out,
        "| pacing | {} ticks pending · longest idle {} ticks |",
        t.busy_ticks, t.longest_idle_run
    );
    let _ = writeln!(
        out,
        "| campaign | {} · gate {}% · {} ascent(s) |",
        t.tier_reached(),
        gate_pct,
        t.ascents.len()
    );
    let _ = writeln!(
        out,
        "| gate reached | {} |",
        t.gate_reached
            .map(|at| format!("tick {at} (~{} days)", days(at)))
            .unwrap_or_else(|| "—".to_string())
    );
    let _ = writeln!(out, "| CEO level | {ceo} · techs {research} |");
    let _ = writeln!(
        out,
        "| traffic | {} flew, {} arrived, {} cut, {} shortages |",
        t.haulers_departed, t.haulers_arrived, t.haulers_interdicted, t.scarcities
    );
    let _ = writeln!(
        out,
        "| act-now alerts | {} raised, {} answered |",
        t.act_now_raised, t.alerts_responded
    );
    if t.battles_fought > 0 {
        let _ = writeln!(
            out,
            "| battles | {} fought, {} won |",
            t.battles_fought, t.battles_won
        );
    }
    let _ = writeln!(
        out,
        "| standings (E/M/B/I) | {} / {} / {} / {} |",
        standings[0], standings[1], standings[2], standings[3]
    );
    let _ = writeln!(out, "| market wall hits | {} |\n", t.wall_hits);

    if !t.ascents.is_empty() {
        let line: Vec<String> = t
            .ascents
            .iter()
            .map(|(at, tier)| format!("{tier} @ {at}"))
            .collect();
        let _ = writeln!(out, "**Ascents:** {}\n", line.join(" → "));
    }

    let _ = writeln!(out, "**Findings:**\n");
    render_findings(out, &review(t));
    let _ = writeln!(out);
}

/// Play the full roster and render the complete gameplay review as Markdown.
pub fn render_report(seed: u64, ticks: u64) -> String {
    let runs: Vec<Transcript> = roster()
        .into_iter()
        .map(|s| run(seed, ticks, 200, s))
        .collect();

    let mut out = String::new();
    let _ = writeln!(out, "# TORCH — Automated Gameplay QA Review\n");
    let _ = writeln!(
        out,
        "Seed **{seed}**, **{ticks}** ticks (~{} days) per persona. Generated by `torch-qa` \
         driving the deterministic core through five play styles. Same seed ⇒ same review.\n",
        days(ticks)
    );

    for t in &runs {
        render_persona(&mut out, t);
    }

    let _ = writeln!(out, "## Design review — cross-cutting\n");
    let _ = writeln!(
        out,
        "What the comparison of play styles reveals about the design as it stands:\n"
    );
    render_findings(&mut out, &design_review(&runs));

    out
}
