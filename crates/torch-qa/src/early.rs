//! Early-game-loop audit — an explicit look at the *opening*.
//!
//! The other lenses average over a whole playthrough. But a game lives or dies in
//! its first session: does a brand-new player know what to do, get a reward
//! quickly, learn the verbs, and feel the destination pull — without being
//! ambushed before they've found their feet? This drives a **Newcomer** (a
//! reasonable new player following the opening-mission beats) through the first
//! `WINDOW` ticks and audits the onboarding loop, deterministically (§27).
//!
//! It checks the things the opening has to get right:
//! - **A first objective + the carrot** — an explicit opening mission and the
//!   ring-gate foreshadowed from tick 0 (§0.1).
//! - **Time-to-first-reward** — how soon the first profit lands.
//! - **The onboarding chain** — do the opening missions actually complete (teach
//!   the verbs) in a first session (§16)?
//! - **The first industrial step** — is a miner reachable/affordable from the
//!   starting float (the early §3.1 first move)?
//! - **Opening pacing** — dead air vs. a pressable loop.
//! - **A calm runway** — the opening isn't overwhelmed by raids/war collateral
//!   before the player can cope (§13).

use crate::review::{Finding, Severity};
use crate::strategy::best_spread;
use std::fmt::Write as _;
use torch_core::sim::{Campaign, Event, Sim};

/// The opening window the audit covers: the first 720 ticks (~30 in-game days at
/// 1 tick/hour) — long enough to clear the opening-mission chain and the first
/// promotion, short enough to be "the first session".
pub const WINDOW: u64 = 720;

/// What the opening playthrough measured.
#[derive(Clone, Debug, Default)]
pub struct EarlyReport {
    pub window: u64,
    /// The active opening objective + hint shown at tick 0 (§16 onboarding).
    pub opening_objective: String,
    /// The now-goal text at tick 0 (the §0 three-horizon "now").
    pub now_goal: String,
    /// The gate is already foreshadowed at tick 0 (§0.1 carrot).
    pub gate_carrot: bool,
    pub start_credits: i64,

    pub first_action: Option<u64>,
    pub first_profit: Option<u64>,
    pub first_route: Option<u64>,
    pub first_industry: Option<u64>,
    pub first_cut: Option<u64>,
    pub first_ascent: Option<u64>,

    /// Opening missions completed / total, and the tick of each completion.
    pub missions_done: usize,
    pub missions_total: usize,
    pub mission_ticks: Vec<u64>,
    /// The opening mission still open at the end of the window (if any) — what a
    /// new player *couldn't* reach in their first session.
    pub still_open: Option<String>,

    pub longest_idle: u64,
    pub peak_pressure: i32,
    pub early_shortages: u64,
    pub shortages_answered: u64,
}

#[derive(Default)]
struct Newcomer {
    routed: bool,
    industry: bool,
    cut: bool,
}

/// Buy the fattest spread cheap and sell it dear, up to `cap` units. Returns
/// whether a round trip was placed (a new player's first, instant trade).
fn trade_once(sim: &mut Sim, cap: i64) -> bool {
    let Some((c, cheap, dear, spread)) = best_spread(sim) else {
        return false;
    };
    if spread < 2 {
        return false;
    }
    let price = sim.markets()[cheap].price(c).max(1);
    let qty = cap
        .min(sim.corp().credits() / price)
        .min(sim.markets()[cheap].stock(c));
    if qty <= 0 || sim.buy(cheap, c, qty).is_err() {
        return false;
    }
    sim.sell(dear, c, qty).is_ok()
}

/// The first body a miner can work (a belt or outer moon, not the inner AO).
fn mineable_body(sim: &Sim) -> Option<usize> {
    (0..sim.bodies().len()).find(|&b| sim.can_mine_body(b))
}

impl Newcomer {
    /// One tick of a reasonable new player following the opening beats, lightly
    /// paced. Returns a label of the action taken (for first-action / idle).
    fn act(&mut self, sim: &mut Sim, tick: u64) -> Option<&'static str> {
        // Answer an act-now shortage the moment one is pending (the §0.4 loop).
        if sim.feed().surfaced().iter().any(|a| a.is_act_now()) && sim.answer_top_shortage(10) {
            return Some("exploit shortage");
        }
        // A first trade (FirstTrade) — almost immediately.
        if tick >= 4 && self.first_trade_pending(sim) && trade_once(sim, 20) {
            return Some("first trade");
        }
        // Stand up logistics (FirstRoute): a freighter + a standing route.
        if !self.routed && tick >= 24 && sim.commission_freighter().is_ok() {
            if let Some((c, o, d, _)) = best_spread(sim) {
                sim.set_trade_route(c, o, d, 20, 1);
                self.routed = true;
                return Some("set route");
            }
        }
        // The first industrial step — a miner (the early §3.1 first move).
        if !self.industry && tick >= 48 {
            if let Some(b) = mineable_body(sim) {
                if sim.buy_miner(b).is_ok() {
                    self.industry = true;
                    return Some("buy miner");
                }
            }
        }
        // Cut a lane (FirstCut) when a convoy is in reach.
        if !self.cut {
            if let Some(h) = sim.haulers().first() {
                let id = h.id;
                if sim.interdict(id) {
                    self.cut = true;
                    return Some("interdict");
                }
            }
        }
        // Keep the loop alive toward the first promotion (FirstAscent).
        if tick.is_multiple_of(30) && trade_once(sim, 15) {
            return Some("trade");
        }
        None
    }

    /// The opening "first trade" mission is still open (drive it early).
    fn first_trade_pending(&self, sim: &Sim) -> bool {
        sim.missions()
            .active()
            .map(|m| m.title == "First Light")
            .unwrap_or(false)
    }
}

/// Drive a Newcomer through the opening window and record what the first session
/// felt like.
pub fn run_newcomer(seed: u64) -> EarlyReport {
    let mut sim = Sim::new(seed);
    let mut r = EarlyReport {
        window: WINDOW,
        start_credits: sim.corp().credits(),
        gate_carrot: !sim.missions().latest_gate().is_empty(),
        now_goal: sim.campaign().now_goal().0.to_string(),
        missions_total: sim.missions().opening_progress().1,
        peak_pressure: sim.pressure().peak_level(),
        ..Default::default()
    };
    r.opening_objective = sim
        .missions()
        .active()
        .map(|m| format!("{} — {}", m.title, m.hint))
        .unwrap_or_default();

    let mut nc = Newcomer::default();
    let mut idle = 0u64;
    let mut done = 0usize;
    let mut last_tier = sim.campaign().tier();

    for _ in 0..WINDOW {
        let tick = sim.tick();
        let label = nc.act(&mut sim, tick);
        if label.is_some() {
            r.first_action.get_or_insert(tick);
        }
        if label == Some("exploit shortage") {
            r.shortages_answered += 1;
        }

        let events = sim.step().to_vec();
        let now = sim.tick();

        for e in &events {
            if matches!(e, Event::Scarcity { .. }) {
                r.early_shortages += 1;
            }
        }

        if sim.corp().credits() > r.start_credits {
            r.first_profit.get_or_insert(now);
        }
        if nc.routed {
            r.first_route.get_or_insert(now);
        }
        if nc.industry {
            r.first_industry.get_or_insert(now);
        }
        if nc.cut {
            r.first_cut.get_or_insert(now);
        }

        let campaign: &Campaign = sim.campaign();
        if campaign.tier() != last_tier {
            r.first_ascent.get_or_insert(now);
            last_tier = campaign.tier();
        }

        let (md, _) = sim.missions().opening_progress();
        while done < md {
            r.mission_ticks.push(now);
            done += 1;
        }

        let pending = sim.feed().surfaced().iter().any(|a| a.is_act_now());
        if label.is_none() && !pending {
            idle += 1;
            r.longest_idle = r.longest_idle.max(idle);
        } else {
            idle = 0;
        }
        r.peak_pressure = r.peak_pressure.max(sim.pressure().peak_level());
    }
    r.missions_done = done;
    if done < r.missions_total {
        r.still_open = sim
            .missions()
            .active()
            .map(|m| format!("{} — {}", m.title, m.hint));
    }
    r
}

fn at(opt: Option<u64>) -> String {
    opt.map(|t| format!("tick {t}"))
        .unwrap_or_else(|| "—".into())
}

/// Audit the opening loop and emit findings.
pub fn audit(r: &EarlyReport) -> Vec<Finding> {
    let mut f = Vec::new();

    // 1. A first objective and the destination carrot from tick 0.
    if !r.opening_objective.is_empty() && r.gate_carrot {
        f.push(Finding {
            severity: Severity::Good,
            area: "Early · direction",
            message: format!(
                "From tick 0 the player has an explicit first objective (\"{}\") and the now-goal \"{}\", with the ring-gate already foreshadowed — direction from minute one (§0.1/§16).",
                r.opening_objective, r.now_goal
            ),
        });
    } else {
        f.push(Finding {
            severity: Severity::Concern,
            area: "Early · direction",
            message: "The opening lacks an explicit first objective and/or the gate carrot — a new player is dropped in without a 'what now?' (§0.1/§16).".to_string(),
        });
    }

    // 2. Time-to-first-reward.
    match r.first_profit {
        Some(t) if t <= 120 => f.push(Finding {
            severity: Severity::Good,
            area: "Early · first reward",
            message: format!("First profit by {} — the loop pays off quickly, before a new player's patience runs out.", at(Some(t))),
        }),
        Some(t) => f.push(Finding {
            severity: Severity::Note,
            area: "Early · first reward",
            message: format!("First profit only at {} (~{} in-game days in) — the opening reward is slow.", at(Some(t)), t / 24),
        }),
        None => f.push(Finding {
            severity: Severity::Concern,
            area: "Early · first reward",
            message: "No profit at all in the opening window — the first loop never pays off.".to_string(),
        }),
    }

    // 3. The onboarding chain — does it teach the verbs in a first session?
    let sev = match r.missions_done {
        d if d >= r.missions_total.saturating_sub(1) => Severity::Good,
        d if d >= 2 => Severity::Note,
        _ => Severity::Concern,
    };
    let still = r
        .still_open
        .as_ref()
        .map(|m| format!(" The one left open is \"{m}\" — check it's reachable from the opening state, not gated behind later unlocks.", ))
        .unwrap_or_default();
    f.push(Finding {
        severity: sev,
        area: "Early · onboarding",
        message: format!(
            "{}/{} opening missions completed in the first {} ticks (~{} in-game days), at {:?} — the chain {} the core verbs in a first session (§16).{}",
            r.missions_done, r.missions_total, r.window, r.window / 24, r.mission_ticks,
            if r.missions_done >= r.missions_total.saturating_sub(1) { "teaches" } else { "only partly teaches" },
            still
        ),
    });

    // 4. The first industrial step (a miner) — reachable from the starting float?
    if let Some(t) = r.first_industry {
        f.push(Finding {
            severity: Severity::Good,
            area: "Early · first build",
            message: format!("The early industrial step is reachable: a miner was affordable and deployed by {} (the §3.1 first move from the {}cr starting float).", at(Some(t)), r.start_credits),
        });
    } else {
        f.push(Finding {
            severity: Severity::Concern,
            area: "Early · first build",
            message: format!("A new player couldn't take the first industrial step in the opening — no miner was affordable/sited within {} ticks (the §3.1 first move is gated too high, or no mineable body is reachable).", r.window),
        });
    }

    // 5. Opening pacing — dead air vs. a pressable loop.
    let idle_pct = r.longest_idle * 100 / r.window.max(1);
    if r.longest_idle <= 120 {
        f.push(Finding {
            severity: Severity::Good,
            area: "Early · pacing",
            message: format!("The opening stays pressable: the longest dead stretch is {} ticks ({idle_pct}% of the window) — no early lull a new player would quit in.", r.longest_idle),
        });
    } else {
        f.push(Finding {
            severity: Severity::Note,
            area: "Early · pacing",
            message: format!("A {}-tick dead stretch ({idle_pct}% of the opening) — long quiet early can read as 'is this thing on?'.", r.longest_idle),
        });
    }

    // 6. A calm runway — not ambushed before finding their feet.
    if r.peak_pressure <= 40 {
        f.push(Finding {
            severity: Severity::Good,
            area: "Early · calm runway",
            message: format!("The opening is a genuine runway: pressure peaked at only {}/100 and the player answered {}/{} early shortages — onboarding happens before the world bites (§13).", r.peak_pressure, r.shortages_answered, r.early_shortages),
        });
    } else {
        f.push(Finding {
            severity: Severity::Note,
            area: "Early · calm runway",
            message: format!("Pressure already peaked at {}/100 in the opening window — the runway may be short for a brand-new player (§13).", r.peak_pressure),
        });
    }

    // 7. The first promotion — present, but not *instant* (a too-fast first tier
    //    reads as rushing the journey; too slow and the first arc never closes).
    match r.first_ascent {
        Some(t) if t < 72 => f.push(Finding {
            severity: Severity::Note,
            area: "Early · first ascent",
            message: format!("The first promotion comes very fast — {} (~{} in-game day(s)). The opening tier flips almost instantly; the Station should read as a stay, not a checkpoint (raise its ops threshold).", at(Some(t)), (t / 24).max(1)),
        }),
        Some(t) => f.push(Finding {
            severity: Severity::Good,
            area: "Early · first ascent",
            message: format!("The first promotion lands at {} (~{} in-game days) — a deliberate Station tier, and the §0.3 fanfare still fires inside the opening session to close the first arc.", at(Some(t)), t / 24),
        }),
        None => f.push(Finding {
            severity: Severity::Note,
            area: "Early · first ascent",
            message: format!("No tier promotion within the first {} ticks — the first arc doesn't close in the opening session (§0.3).", r.window),
        }),
    }

    f
}

/// Run the opening playthrough and audit it (seed 7, the report's default).
pub fn audit_opening(seed: u64) -> (EarlyReport, Vec<Finding>) {
    let r = run_newcomer(seed);
    let findings = audit(&r);
    (r, findings)
}

/// Render the early-game-loop audit section.
pub fn render_early(out: &mut String, seed: u64) {
    let (r, findings) = audit_opening(seed);

    let _ = writeln!(out, "## Early-game loop audit\n");
    let _ = writeln!(
        out,
        "_An explicit look at the **opening**: a Newcomer (a reasonable new player following the \
         opening-mission beats) driven through the first {} ticks (~{} in-game days). The other \
         lenses average over a whole run; a game lives or dies in its first session._\n",
        r.window,
        r.window / 24
    );

    let _ = writeln!(out, "| opening beat | when |");
    let _ = writeln!(out, "| --- | --- |");
    let _ = writeln!(out, "| first action | {} |", at(r.first_action));
    let _ = writeln!(out, "| first profit | {} |", at(r.first_profit));
    let _ = writeln!(out, "| first standing route | {} |", at(r.first_route));
    let _ = writeln!(
        out,
        "| first industrial step (miner) | {} |",
        at(r.first_industry)
    );
    let _ = writeln!(out, "| first lane cut | {} |", at(r.first_cut));
    let _ = writeln!(out, "| first promotion | {} |", at(r.first_ascent));
    let _ = writeln!(
        out,
        "| opening missions | {}/{} |",
        r.missions_done, r.missions_total
    );
    let _ = writeln!(out, "| peak pressure (window) | {}/100 |", r.peak_pressure);
    let _ = writeln!(out, "| longest dead stretch | {} ticks |\n", r.longest_idle);

    let _ = writeln!(out, "**Findings:**\n");
    for finding in &findings {
        let _ = writeln!(
            out,
            "- **[{}]** _{}_ — {}",
            finding.severity.tag(),
            finding.area,
            finding.message
        );
    }
    let _ = writeln!(out);
}
