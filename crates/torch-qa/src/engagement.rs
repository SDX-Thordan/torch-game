//! Engagement & "fun" assessment — the second lens on a playthrough.
//!
//! [`crate::review`] asks *does the game work and is it balanced*. This asks the
//! harder, fuzzier question: *is it engaging to play?* It cannot measure
//! subjective fun — no automated harness can. What it **can** do is score
//! well-established **structural proxies** of engagement from a bot's
//! deterministic playthrough, and flag the anti-patterns that reliably kill fun:
//! aimlessness (no goal), dead air (nothing to do), flat stakes (no tension), a
//! starved reward cadence, and a dominant strategy (only one way to play).
//!
//! Six facets, each scored 0–100 with a one-line rationale, drawn from the
//! project's own design identity — "depth of decision is the fun" (§0, the
//! influence model):
//!
//! - **Direction** — is the player always pulled toward the destination? (§0)
//! - **Flow** — moment-to-moment pacing: a steady stream of pressable
//!   exceptions, not dead air. (§19/§28)
//! - **Agency** — do the player's choices visibly change the outcome? (§0.4)
//! - **Reward rhythm** — are milestones delivered at a satisfying cadence? (§0.3)
//! - **Stakes** — meaningful adversity and recovery, neither flatline nor
//!   wipeout. (§13)
//! - **Variety** — does the experience touch many systems and evolve? (§12)
//!
//! Deterministic in, deterministic out (§27): the same seed yields the same
//! engagement profile, so a feel-regression shows up as a moved score.

use crate::harness::{Transcript, EVENT_KIND_COUNT};
use crate::review::{Finding, Severity};
use std::fmt::Write as _;

/// One scored dimension of engagement.
#[derive(Clone, Debug)]
pub struct Facet {
    pub name: &'static str,
    pub score: u8,
    pub rationale: String,
}

/// A persona's full engagement profile: the facets plus a weighted overall.
#[derive(Clone, Debug)]
pub struct EngagementProfile {
    pub persona: &'static str,
    pub facets: Vec<Facet>,
    pub overall: u8,
}

impl EngagementProfile {
    /// The lowest-scoring facet — the run's weakest engagement dimension.
    pub fn weakest(&self) -> &Facet {
        self.facets
            .iter()
            .min_by_key(|f| f.score)
            .expect("profiles always have facets")
    }

    /// Score of the named facet (0 if absent).
    pub fn facet(&self, name: &str) -> u8 {
        self.facets
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.score)
            .unwrap_or(0)
    }
}

/// Facet weights (sum 100). Direction and Flow lead because this game's fun is
/// "a journey toward a destination" played as run-by-exception (§0/§19).
const WEIGHTS: [(&str, i64); 6] = [
    ("Direction", 22),
    ("Flow", 18),
    ("Agency", 18),
    ("Reward", 16),
    ("Stakes", 14),
    ("Variety", 12),
];

fn clamp100(x: i64) -> u8 {
    x.clamp(0, 100) as u8
}

/// A triangular "sweet spot": rises 0→100 over `[0, lo]`, holds 100 across
/// `[lo, hi]`, then falls back toward 0 by `top`. Used where *more is not always
/// better* — moderate adversity is engaging, none is boring, too much frustrates.
fn sweet(x: i64, lo: i64, hi: i64, top: i64) -> i64 {
    if x <= 0 {
        0
    } else if x < lo {
        x * 100 / lo
    } else if x <= hi {
        100
    } else if x < top {
        100 - (x - hi) * 100 / (top - hi)
    } else {
        0
    }
}

/// Score the six engagement facets for one playthrough.
pub fn assess(t: &Transcript) -> EngagementProfile {
    let ticks = t.ticks.max(1) as i64;

    // Direction — how far the destination pull carried the player (§0).
    let gate = t.samples.last().map(|s| s.gate_bp).unwrap_or(0);
    let direction = clamp100(gate / 100);
    let dir_note = format!("reached {} ({}% to the gate)", t.tier_reached(), direction);

    // Flow — pacing: penalize the longest dead stretch, the time a player would
    // fast-forward through (§28). A lively world keeps that short.
    let dead = t.longest_idle_run as i64 * 100 / ticks;
    let busy_cov = t.busy_ticks as i64 * 100 / ticks;
    let flow = clamp100(100 - dead);
    let flow_note = format!(
        "longest dead stretch {} ticks ({dead}% of the run); {busy_cov}% of ticks had a live exception",
        t.longest_idle_run
    );

    // Agency — did the player's involvement change the outcome: ops climbed (even
    // hands-off via standing orders) and exceptions answered (§0.4).
    let advanced = (t.ascents.len() as i64 * 100 / 3).min(100);
    let responded = if t.act_now_raised > 0 {
        (t.alerts_responded * 100 / t.act_now_raised) as i64
    } else {
        0
    };
    let agency = clamp100((advanced * 6 + responded * 4) / 10);
    let agency_note = format!(
        "advanced {}/3 tiers by its own operations; answered {}/{} act-now shortages",
        t.ascents.len(),
        t.alerts_responded,
        t.act_now_raised
    );

    // Reward rhythm — milestones, and whether they were spread across the run
    // rather than bunched up front or absent (§0.3).
    let n = t.ascents.len() as i64;
    let (reward, span) = if n == 0 {
        (0u8, 0i64)
    } else {
        let base = n.min(3) * 100 / 3;
        let first = t.ascents.first().unwrap().0 as i64;
        let last = t.ascents.last().unwrap().0 as i64;
        let span = if n >= 2 {
            (last - first) * 100 / ticks
        } else {
            0
        };
        let factor = if n >= 2 { 50 + (span * 3).min(50) } else { 70 };
        (clamp100(base * factor / 100), span)
    };
    let reward_note = format!("{n} milestone(s), spanning {span}% of the run");

    // Stakes — felt adversity and recovery (§13). Player-side setbacks lead;
    // ambient pressure seasons. Flatline is boring; pure-loss combat frustrates.
    let drawdown = t.max_drawdown_pct() as i64;
    let losses = (t.battle_losses as i64 * 15).min(60);
    let min_standing = t.final_standings().iter().copied().min().unwrap_or(0);
    let rep_cost = ((-min_standing).clamp(0, 1000) / 20).min(50);
    let ambient = (t.peak_pressure as i64 / 3).min(33);
    let adversity = drawdown.min(60) + losses + rep_cost + ambient;
    let mut stakes = sweet(adversity, 40, 90, 170);
    if t.battles_fought >= 5 && t.battles_won == 0 {
        stakes = stakes * 6 / 10; // a fight you always lose is punishment, not drama
    }
    let stakes = clamp100(stakes);
    let stakes_note = format!(
        "treasury dip {drawdown}%, {} ships lost, rep low {min_standing}, pressure peak {}",
        t.battle_losses, t.peak_pressure
    );

    // Variety — breadth of systems the run touched, and how far the scope widened
    // (tiers experienced, §12).
    let ev = t.distinct_event_kinds as i64 * 100 / EVENT_KIND_COUNT as i64;
    let tiers = t.tiers_experienced() as i64 * 100 / 4;
    let variety = clamp100((ev * 6 + tiers * 4) / 10);
    let variety_note = format!(
        "{} of {} event kinds; {} tier(s) of scope",
        t.distinct_event_kinds,
        EVENT_KIND_COUNT,
        t.tiers_experienced()
    );

    let facets = vec![
        Facet {
            name: "Direction",
            score: direction,
            rationale: dir_note,
        },
        Facet {
            name: "Flow",
            score: flow,
            rationale: flow_note,
        },
        Facet {
            name: "Agency",
            score: agency,
            rationale: agency_note,
        },
        Facet {
            name: "Reward",
            score: reward,
            rationale: reward_note,
        },
        Facet {
            name: "Stakes",
            score: stakes,
            rationale: stakes_note,
        },
        Facet {
            name: "Variety",
            score: variety,
            rationale: variety_note,
        },
    ];

    let overall = weighted_overall(&facets);
    EngagementProfile {
        persona: t.persona,
        facets,
        overall,
    }
}

fn weighted_overall(facets: &[Facet]) -> u8 {
    let mut acc = 0i64;
    for (name, w) in WEIGHTS {
        let score = facets
            .iter()
            .find(|f| f.name == name)
            .map(|f| f.score as i64)
            .unwrap_or(0);
        acc += score * w;
    }
    clamp100(acc / 100)
}

/// The cross-cutting "is the game fun?" synthesis — what the *comparison* of play
/// styles says about the experience as a whole.
pub fn assess_fun(runs: &[Transcript]) -> Vec<Finding> {
    let profiles: Vec<EngagementProfile> = runs.iter().map(assess).collect();
    let mut f = Vec::new();
    if profiles.is_empty() {
        return f;
    }

    // 1. Multiple viable, engaging play styles, or a dominant strategy?
    let engaging: Vec<&str> = profiles
        .iter()
        .filter(|p| p.overall >= 50)
        .map(|p| p.persona)
        .collect();
    if engaging.len() >= 3 {
        f.push(Finding {
            severity: Severity::Good,
            area: "Fun · breadth",
            message: format!(
                "Several distinct play styles are engaging ({engaging:?} all score ≥50/100). Depth-of-decision survives the choice of approach — no single dominant strategy starves the others."
            ),
        });
    } else if engaging.len() <= 1 {
        f.push(Finding {
            severity: Severity::Concern,
            area: "Fun · breadth",
            message: format!(
                "Only {engaging:?} clears a 50/100 engagement bar — the other styles fall flat. The fun may be funnelled into a single dominant approach."
            ),
        });
    }

    // 2. The weakest dimension across the board — the game's biggest fun gap.
    let mut worst_facet = ("", i64::MAX);
    for (name, _) in WEIGHTS {
        let avg: i64 =
            profiles.iter().map(|p| p.facet(name) as i64).sum::<i64>() / profiles.len() as i64;
        if avg < worst_facet.1 {
            worst_facet = (name, avg);
        }
    }
    let sev = if worst_facet.1 < 35 {
        Severity::Concern
    } else {
        Severity::Note
    };
    f.push(Finding {
        severity: sev,
        area: "Fun · weakest link",
        message: format!(
            "Across all play styles, **{}** is the weakest engagement dimension (avg {}/100) — the experience's biggest fun gap to invest in next.",
            worst_facet.0, worst_facet.1
        ),
    });

    // 3. The strongest dimension — what the design already nails.
    let mut best_facet = ("", i64::MIN);
    for (name, _) in WEIGHTS {
        let avg: i64 =
            profiles.iter().map(|p| p.facet(name) as i64).sum::<i64>() / profiles.len() as i64;
        if avg > best_facet.1 {
            best_facet = (name, avg);
        }
    }
    f.push(Finding {
        severity: Severity::Good,
        area: "Fun · strength",
        message: format!(
            "**{}** is the strongest dimension (avg {}/100) — the experience leans on it well.",
            best_facet.0, best_facet.1
        ),
    });

    // 4. Is the hands-off world watchable (the §28 promise)?
    if let Some(spec) = profiles.iter().find(|p| p.persona == "Spectator") {
        let watch = (spec.facet("Flow") as i64 + spec.facet("Variety") as i64) / 2;
        let sev = if watch >= 50 {
            Severity::Good
        } else {
            Severity::Note
        };
        f.push(Finding {
            severity: sev,
            area: "Fun · watchability",
            message: format!(
                "Hands fully off, the world scores {watch}/100 on flow+variety — the measure of whether it's worth watching before you act (§28)."
            ),
        });
    }

    f
}

// ---- Markdown rendering ----------------------------------------------------

fn bar(score: u8) -> String {
    let filled = (score as usize).div_ceil(10); // 0..=10 blocks
    let empty = 10 - filled;
    format!("{}{}", "█".repeat(filled), "·".repeat(empty))
}

/// Render the engagement section for the whole roster: the caveat, a scores
/// table, each persona's weakest facet, and the cross-cutting fun findings.
pub fn render_engagement(out: &mut String, runs: &[Transcript]) {
    let profiles: Vec<EngagementProfile> = runs.iter().map(assess).collect();

    let _ = writeln!(out, "## Engagement & fun assessment\n");
    let _ = writeln!(
        out,
        "_These are **structural proxies** for engagement, not a measure of subjective fun — a \
         deterministic bot can flag aimlessness, dead air, flat stakes, a starved reward cadence, \
         and dominant strategies, but it can't feel delight. Read the scores as \"where is fun at \
         risk?\", not \"how fun is it?\"._\n"
    );

    let _ = writeln!(
        out,
        "| persona | overall | Direction | Flow | Agency | Reward | Stakes | Variety |"
    );
    let _ = writeln!(out, "| --- | --- | --- | --- | --- | --- | --- | --- |");
    for p in &profiles {
        let _ = writeln!(
            out,
            "| {} | **{}** | {} | {} | {} | {} | {} | {} |",
            p.persona,
            p.overall,
            p.facet("Direction"),
            p.facet("Flow"),
            p.facet("Agency"),
            p.facet("Reward"),
            p.facet("Stakes"),
            p.facet("Variety"),
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "**Per-style read (overall + weakest link):**\n");
    for p in &profiles {
        let w = p.weakest();
        let _ = writeln!(
            out,
            "- **{}** {} {}/100 — weakest is _{}_ ({}/100): {}",
            p.persona,
            bar(p.overall),
            p.overall,
            w.name,
            w.score,
            w.rationale
        );
    }
    let _ = writeln!(out);

    let _ = writeln!(out, "**What the comparison says about fun:**\n");
    for finding in assess_fun(runs) {
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
