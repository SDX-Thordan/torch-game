//! TORCH economy QA harness — runs the deterministic core **renderless** and assesses the health
//! of the autonomous market economy, flagging problems.
//!
//! It depends on the `rlib` side of `torch-core` (like `cargo test`), so it never needs a Godot
//! runtime. `assess()` runs the sim across several seeds, samples the economy periodically, and
//! turns the metrics into a written report of findings (✓ good / ⚠ warning / ✗ failure).

use torch_core::sim::commodity;
use torch_core::sim::{ShipClass, Sim};

/// Severity of a finding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Good,
    Warn,
    Fail,
}

impl Severity {
    fn glyph(self) -> &'static str {
        match self {
            Severity::Good => "✓",
            Severity::Warn => "⚠",
            Severity::Fail => "✗",
        }
    }
}

/// One assessed aspect of the economy.
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
}

/// The assessment result.
pub struct Report {
    pub seeds: Vec<u64>,
    pub ticks: u64,
    pub findings: Vec<Finding>,
}

impl Report {
    pub fn worst(&self) -> Severity {
        if self.findings.iter().any(|f| f.severity == Severity::Fail) {
            Severity::Fail
        } else if self.findings.iter().any(|f| f.severity == Severity::Warn) {
            Severity::Warn
        } else {
            Severity::Good
        }
    }

    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("# TORCH — Economy QA Report\n\n");
        s.push_str(&format!(
            "Ran the deterministic core renderless: seeds {:?} × {} ticks each.\n\n",
            self.seeds, self.ticks
        ));
        let (g, w, f) = self
            .findings
            .iter()
            .fold((0, 0, 0), |(g, w, f), x| match x.severity {
                Severity::Good => (g + 1, w, f),
                Severity::Warn => (g, w + 1, f),
                Severity::Fail => (g, w, f + 1),
            });
        let verdict = match self.worst() {
            Severity::Good => "HEALTHY — the autonomous economy is balanced.",
            Severity::Warn => "LIVABLE — balanced overall, with concerns to watch.",
            Severity::Fail => "BROKEN — at least one critical economic failure.",
        };
        s.push_str(&format!(
            "**Verdict: {verdict}** ({g} ✓ · {w} ⚠ · {f} ✗)\n\n"
        ));
        for x in &self.findings {
            s.push_str(&format!(
                "{} **{}**\n   {}\n\n",
                x.severity.glyph(),
                x.title,
                x.detail
            ));
        }
        s
    }
}

// ---- metrics gathered over a run ----------------------------------------------------------

#[derive(Clone)]
struct RunMetrics {
    seed: u64,
    player_names: Vec<String>,
    credits_start_total: i64,
    credits_end_total: i64,
    // per-player minimum credits seen (solvency).
    player_min_credits: Vec<i64>,
    player_end_credits: Vec<i64>,
    // per-good price extremes across all markets/samples.
    good_price_lo: Vec<i64>,
    good_price_hi: Vec<i64>,
    // per-good: samples where some market pinned the price to a rail.
    good_rail_samples: Vec<u64>,
    // per-good: summed inter-market spread (max-min price) over samples, for the average.
    good_spread_sum: Vec<i64>,
    // starvation: settlements at 0 food (max over samples, and at the end).
    starving_max: usize,
    starving_end: usize,
    settlements: usize,
    // hauler idle fraction: summed idle/total over samples.
    idle_haulers_sum: i64,
    hauler_samples: i64,
    haulers_total: i64,
    // facility "fed" fraction: facilities able to produce this sample.
    fed_facilities_sum: i64,
    facility_samples: i64,
    facilities_total: i64,
    // reservation integrity violations (oversubscription / negative availability).
    reservation_violations: u64,
    samples: u64,
}

fn run_one(seed: u64, ticks: u64, sample_every: u64) -> RunMetrics {
    let goods = commodity::commodity_count();
    let mut sim = Sim::new(seed);
    let player_names: Vec<String> = sim.players().iter().map(|p| p.name.clone()).collect();
    let credits_start_total: i64 = sim.players().iter().map(|p| p.credits).sum();
    let mut player_min_credits: Vec<i64> = sim.players().iter().map(|p| p.credits).collect();

    let mut m = RunMetrics {
        seed,
        player_names,
        credits_start_total,
        credits_end_total: 0,
        player_min_credits: player_min_credits.clone(),
        player_end_credits: vec![0; sim.players().len()],
        good_price_lo: vec![i64::MAX; goods],
        good_price_hi: vec![i64::MIN; goods],
        good_rail_samples: vec![0; goods],
        good_spread_sum: vec![0; goods],
        starving_max: 0,
        starving_end: 0,
        settlements: sim.colonies().len() + sim.mining_stations().len(),
        idle_haulers_sum: 0,
        hauler_samples: 0,
        haulers_total: sim
            .ships()
            .iter()
            .filter(|s| s.class == ShipClass::Hauler)
            .count() as i64,
        fed_facilities_sum: 0,
        facility_samples: 0,
        facilities_total: sim.facilities().len() as i64,
        reservation_violations: 0,
        samples: 0,
    };
    let food = commodity::FOOD;

    for t in 1..=ticks {
        sim.step();
        if !t.is_multiple_of(sample_every) {
            continue;
        }
        m.samples += 1;
        // solvency
        for (i, p) in sim.players().iter().enumerate() {
            player_min_credits[i] = player_min_credits[i].min(p.credits);
        }
        // markets: price extremes, rail-pinning, spreads, reservation integrity
        for c in 0..goods {
            let mut lo = i64::MAX;
            let mut hi = i64::MIN;
            let mut railed = false;
            for mk in sim.markets() {
                let p = mk.price(c);
                lo = lo.min(p);
                hi = hi.max(p);
                let d = &mk.defs()[c];
                if p <= d.floor || p >= d.ceiling {
                    railed = true;
                }
                // A real leak (not the benign stabilizer-drains-below-reservation case) shows as
                // availability going negative, or reservations exceeding the market's whole
                // capacity. Reserved > current stock alone is fine — execute caps at real stock.
                if mk.available_to_buy(c) < 0 || mk.reserved_out(c) > mk.wall_high(c) {
                    m.reservation_violations += 1;
                }
            }
            m.good_price_lo[c] = m.good_price_lo[c].min(lo);
            m.good_price_hi[c] = m.good_price_hi[c].max(hi);
            m.good_spread_sum[c] += hi - lo;
            if railed {
                m.good_rail_samples[c] += 1;
            }
        }
        // starvation
        let starving = sim.colonies().iter().filter(|c| c.get(food) == 0).count()
            + sim
                .mining_stations()
                .iter()
                .filter(|s| s.get(food) == 0)
                .count();
        m.starving_max = m.starving_max.max(starving);
        // hauler idle
        let idle = sim
            .ships()
            .iter()
            .filter(|s| s.class == ShipClass::Hauler && !s.in_flight() && s.job.is_none())
            .count() as i64;
        m.idle_haulers_sum += idle;
        m.hauler_samples += 1;
        // facility fed (can produce)
        let fed = sim
            .facilities()
            .iter()
            .filter(|f| {
                let r = f.kind.recipe();
                f.input_of(r.input) >= f.rate * r.ratio
            })
            .count() as i64;
        m.fed_facilities_sum += fed;
        m.facility_samples += 1;
    }

    m.credits_end_total = sim.players().iter().map(|p| p.credits).sum();
    m.player_min_credits = player_min_credits;
    m.player_end_credits = sim.players().iter().map(|p| p.credits).collect();
    m.starving_end = sim.colonies().iter().filter(|c| c.get(food) == 0).count()
        + sim
            .mining_stations()
            .iter()
            .filter(|s| s.get(food) == 0)
            .count();
    m
}

/// Run the full economy assessment across `seeds`, `ticks` each, and turn it into findings.
pub fn assess(seeds: &[u64], ticks: u64) -> Report {
    let sample_every = 100;
    let runs: Vec<RunMetrics> = seeds
        .iter()
        .map(|&s| run_one(s, ticks, sample_every))
        .collect();
    let names = commodity::commodities();
    let mut findings = Vec::new();

    // 1. Determinism (a separate quick check — two runs of seed 0 agree on end credits).
    {
        let a = run_one(seeds[0], ticks.min(2000), sample_every);
        let b = run_one(seeds[0], ticks.min(2000), sample_every);
        let same = a.credits_end_total == b.credits_end_total
            && a.player_end_credits == b.player_end_credits;
        findings.push(Finding {
            severity: if same { Severity::Good } else { Severity::Fail },
            title: "Determinism".into(),
            detail: if same {
                format!(
                    "Same seed ⇒ identical end state (total credits {}).",
                    a.credits_end_total
                )
            } else {
                "Same seed produced DIFFERENT end states — non-determinism!".into()
            },
        });
    }

    // 2. Solvency — no player should go bankrupt.
    {
        let mut worst = i64::MAX;
        let mut worst_who = String::new();
        let mut worst_seed = 0;
        for r in &runs {
            for (i, &c) in r.player_min_credits.iter().enumerate() {
                if c < worst {
                    worst = c;
                    worst_who = r.player_names[i].clone();
                    worst_seed = r.seed;
                }
            }
        }
        let sev = if worst < 0 {
            Severity::Fail
        } else if worst < 20_000 {
            Severity::Warn
        } else {
            Severity::Good
        };
        findings.push(Finding {
            severity: sev,
            title: "Solvency".into(),
            detail: format!(
                "Lowest any player's credits ever fell: {worst} ({worst_who}, seed {worst_seed}). \
                 {}",
                match sev {
                    Severity::Good => "Everyone stayed comfortably solvent.",
                    Severity::Warn => "A player ran uncomfortably low — watch the balance.",
                    Severity::Fail => "A player went BANKRUPT — the economy can't sustain them.",
                }
            ),
        });
    }

    // 3. Inflation / money supply — credits should stay bounded, not explode or collapse.
    {
        // average end/start ratio across seeds (×100 for integer).
        let mut ratio_sum = 0i64;
        for r in &runs {
            ratio_sum += r.credits_end_total * 100 / r.credits_start_total.max(1);
        }
        let ratio = ratio_sum / runs.len() as i64; // percent
        let bounded = (70..=4_000).contains(&ratio);
        let sev = if bounded {
            Severity::Good
        } else {
            Severity::Warn
        };
        findings.push(Finding {
            severity: sev,
            title: "Money supply".into(),
            detail: format!(
                "Total credits grew to ~{ratio}% of the start over {ticks} ticks (avg of {} seeds). \
                 {}",
                runs.len(),
                if bounded {
                    "Bounded — the production-backed money supply grows steadily, not runaway."
                } else if ratio > 4_000 {
                    "Growing fast — consider a stronger money sink (broker fee / fuel cost)."
                } else {
                    "Shrinking — the economy may be deflating."
                }
            ),
        });
    }

    // 4. Market liveness — prices should vary (not static), and not pin to a rail.
    {
        let mut static_goods = Vec::new();
        let mut railed_goods = Vec::new();
        for (c, name) in names.iter().enumerate() {
            let range: i64 = runs
                .iter()
                .map(|r| r.good_price_hi[c] - r.good_price_lo[c])
                .max()
                .unwrap_or(0);
            if range < 8 {
                static_goods.push(name.name);
            }
            let rail_samples: u64 = runs.iter().map(|r| r.good_rail_samples[c]).sum();
            let total_samples: u64 = runs.iter().map(|r| r.samples).sum();
            if total_samples > 0 && rail_samples * 100 / total_samples > 20 {
                railed_goods.push(name.name);
            }
        }
        let sev = if !railed_goods.is_empty() {
            Severity::Fail
        } else if !static_goods.is_empty() {
            Severity::Warn
        } else {
            Severity::Good
        };
        findings.push(Finding {
            severity: sev,
            title: "Market liveness".into(),
            detail: if railed_goods.is_empty() && static_goods.is_empty() {
                "Every good's price varies and never pins to its floor/ceiling — a living market."
                    .into()
            } else {
                let mut d = String::new();
                if !railed_goods.is_empty() {
                    d.push_str(&format!(
                        "Pinned to a rail (dead): {}. ",
                        railed_goods.join(", ")
                    ));
                }
                if !static_goods.is_empty() {
                    d.push_str(&format!(
                        "Barely moves (static): {}.",
                        static_goods.join(", ")
                    ));
                }
                d
            },
        });
    }

    // 5. Trade / spreads — arbitrage should leave moderate, damped spreads (not huge, not zero).
    {
        let mut lines = Vec::new();
        let mut big = false;
        for (c, name) in names.iter().enumerate() {
            let mut spread_sum = 0i64;
            let mut samp = 0i64;
            for r in &runs {
                spread_sum += r.good_spread_sum[c];
                samp += r.samples as i64;
            }
            let avg = if samp > 0 { spread_sum / samp } else { 0 };
            // Normalize against the price anchor: a spread > 60% of base means arbitrage is failing.
            if avg * 100 / base_price(c).max(1) > 60 {
                big = true;
            }
            lines.push(format!("{} ~{avg}", name.name));
        }
        findings.push(Finding {
            severity: if big { Severity::Warn } else { Severity::Good },
            title: "Trade spreads (arbitrage)".into(),
            detail: format!(
                "Avg inter-market price spread per good: {}. {}",
                lines.join(" · "),
                if big {
                    "Some spreads stay wide — arbitrage isn't fully closing them (too few traders?)."
                } else {
                    "Spreads are moderate — the arbitrage haulers are damping them."
                }
            ),
        });
    }

    // 6. Food security — settlements shouldn't starve.
    {
        let max_starve = runs.iter().map(|r| r.starving_max).max().unwrap_or(0);
        let settlements = runs.first().map(|r| r.settlements).unwrap_or(0);
        let pct = if settlements > 0 {
            max_starve * 100 / settlements
        } else {
            0
        };
        let sev = if pct >= 30 {
            Severity::Fail
        } else if pct >= 10 {
            Severity::Warn
        } else {
            Severity::Good
        };
        findings.push(Finding {
            severity: sev,
            title: "Food security".into(),
            detail: format!(
                "Peak {max_starve}/{settlements} settlements out of food ({pct}%). {}",
                match sev {
                    Severity::Good => "Haulers keep the population fed.",
                    Severity::Warn => "Some settlements occasionally run dry — logistics is tight.",
                    Severity::Fail =>
                        "Widespread starvation — Food supply or haulage can't keep up.",
                }
            ),
        });
    }

    // 7. Hauler utilization — too many idle haulers means no profitable work (oversupply).
    {
        let mut idle_sum = 0i64;
        let mut samp = 0i64;
        let mut total = 0i64;
        for r in &runs {
            idle_sum += r.idle_haulers_sum;
            samp += r.hauler_samples;
            total = total.max(r.haulers_total);
        }
        let avg_idle = if samp > 0 { idle_sum / samp } else { 0 };
        let pct = if total > 0 { avg_idle * 100 / total } else { 0 };
        let sev = if pct >= 60 {
            Severity::Warn
        } else {
            Severity::Good
        };
        findings.push(Finding {
            severity: sev,
            title: "Hauler utilization".into(),
            detail: format!(
                "~{avg_idle}/{total} haulers idle on an average tick ({pct}%). {}",
                if pct >= 60 {
                    "Many haulers sit idle — not enough profitable trades (oversupply of ships)."
                } else {
                    "Haulers are busy — the trade network has work for them."
                }
            ),
        });
    }

    // 8. Industry — facilities should be fed enough to keep producing.
    {
        let mut fed_sum = 0i64;
        let mut samp = 0i64;
        let mut total = 0i64;
        for r in &runs {
            fed_sum += r.fed_facilities_sum;
            samp += r.facility_samples;
            total = total.max(r.facilities_total);
        }
        let avg_fed = if samp > 0 { fed_sum / samp } else { 0 };
        let pct = if total > 0 { avg_fed * 100 / total } else { 0 };
        let sev = if pct < 40 {
            Severity::Warn
        } else {
            Severity::Good
        };
        findings.push(Finding {
            severity: sev,
            title: "Industry throughput".into(),
            detail: format!(
                "~{avg_fed}/{total} facilities had input to produce on an average tick ({pct}%). {}",
                if pct < 40 {
                    "Facilities often starve of input — the supply chain is under-served."
                } else {
                    "Facilities are kept fed — the production chain flows."
                }
            ),
        });
    }

    // 9. Reservation integrity — the anti-stampede must never oversubscribe a market.
    {
        let violations: u64 = runs.iter().map(|r| r.reservation_violations).sum();
        findings.push(Finding {
            severity: if violations == 0 {
                Severity::Good
            } else {
                Severity::Fail
            },
            title: "Reservation integrity".into(),
            detail: if violations == 0 {
                "Market reservations never oversubscribed stock or drove availability negative."
                    .into()
            } else {
                format!("{violations} reservation-integrity violations — the anti-stampede leaks.")
            },
        });
    }

    Report {
        seeds: seeds.to_vec(),
        ticks,
        findings,
    }
}

/// The base price anchor of a good (for normalizing spreads) — from the economy price defs.
fn base_price(c: usize) -> i64 {
    torch_core::sim::economy::price_defs()[c].base_price
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_economy_assesses_as_healthy() {
        // A short assessment runs and produces findings with no FAILures (the economy is balanced).
        let report = assess(&[0, 1], 2_000);
        assert!(!report.findings.is_empty());
        assert!(
            report.findings.iter().all(|f| f.severity != Severity::Fail),
            "{}",
            report.render()
        );
    }
}
