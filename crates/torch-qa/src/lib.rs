//! TORCH automated gameplay QA (§32, §35 headless-first).
//!
//! The sim core is pure, deterministic, and engine-agnostic, which makes it
//! *playable by a program*. This crate is that program: a set of **autoplayer
//! personas** (each a play style under the [`Strategy`] trait), a [`harness`]
//! that runs a persona for thousands of ticks and records a [`Transcript`], and
//! a [`review`] engine that turns the transcript into a written **gameplay
//! review** — pacing, agency, economy bounds, alert engagement, and the
//! cross-cutting design findings only a full playthrough surfaces.
//!
//! It is the QA counterpart to `cargo test`: tests assert that systems *work*;
//! this asserts (and critiques) how the game *plays*. Same seed ⇒ same review
//! (§27), so a regression in feel shows up as a diff.
//!
//! ```no_run
//! let report = torch_qa::review::render_report(7, 4_000);
//! print!("{report}");
//! ```

pub mod engagement;
pub mod harness;
pub mod review;
pub mod strategy;
pub mod ui;

pub use engagement::{assess, assess_fun, EngagementProfile, Facet};
pub use harness::{run, Sample, Transcript};
pub use review::{design_review, render_report, review, Finding, Severity};
pub use strategy::{roster, Strategy};
pub use ui::{audit, audit_repo, UiModel};

#[cfg(test)]
mod tests {
    use super::*;
    use torch_core::sim::Event;

    /// Every persona plays a long run without panicking and produces samples.
    #[test]
    fn all_personas_play_cleanly() {
        for strat in roster() {
            let name = strat.name();
            let t = run(3, 1_000, 200, strat);
            assert!(!t.samples.is_empty(), "{name} produced no samples");
            assert_eq!(t.ticks, 1_000);
        }
    }

    /// Same seed ⇒ same playthrough (the review is a determinism fingerprint).
    #[test]
    fn a_review_is_deterministic() {
        let a = run(7, 2_000, 200, Box::new(strategy::Tycoon::default()));
        let b = run(7, 2_000, 200, Box::new(strategy::Tycoon::default()));
        assert_eq!(a.end_credits, b.end_credits);
        assert_eq!(a.ascents, b.ascents);
        assert_eq!(a.act_now_raised, b.act_now_raised);
    }

    /// Hands-off, the world is alive: convoys fly and shortages are voiced with
    /// zero player input (the §28 watchability the Spectator measures).
    #[test]
    fn the_idle_world_runs_itself() {
        let t = run(0, 2_000, 200, Box::new(strategy::Spectator));
        assert_eq!(t.actions, 0, "the Spectator must touch nothing");
        assert!(t.haulers_departed > 0, "no convoys ever flew");
    }

    /// Both the raider and the hands-off logistician climb the spine now that
    /// building/routing count as operations; pure manual teleport-trade still
    /// doesn't (the degenerate verb the economy PR also nerfs).
    #[test]
    fn raiding_and_routing_both_climb() {
        let raider = run(0, 4_000, 200, Box::new(strategy::Privateer));
        assert!(
            !raider.ascents.is_empty(),
            "the Privateer should advance a tier"
        );
        let logistician = run(0, 4_000, 200, Box::new(strategy::Logistician));
        assert!(
            !logistician.ascents.is_empty(),
            "routing/building should advance the spine without raiding"
        );
        let trader = run(0, 4_000, 200, Box::new(strategy::Arbitrageur));
        assert!(
            trader.ascents.is_empty(),
            "pure hand-trading is not an operation, so it should not climb"
        );
    }

    /// The review engine always has something to say, and the design pass flags
    /// the cross-cutting structural findings.
    #[test]
    fn the_review_speaks() {
        let runs: Vec<Transcript> = roster()
            .into_iter()
            .map(|s| run(1, 2_000, 200, s))
            .collect();
        for t in &runs {
            assert!(!review(t).is_empty(), "{} got no findings", t.persona);
        }
        assert!(!design_review(&runs).is_empty());
    }

    /// Engagement profiles are well-formed: every facet and the overall sit in
    /// 0..=100, and a goal-driven operator out-engages a do-nothing spectator on
    /// the destination pull.
    #[test]
    fn engagement_scores_are_well_formed() {
        let tycoon = run(0, 4_000, 200, Box::new(strategy::Tycoon::default()));
        let spectator = run(0, 4_000, 200, Box::new(strategy::Spectator));
        let pt = assess(&tycoon);
        let ps = assess(&spectator);
        for p in [&pt, &ps] {
            assert!(p.overall <= 100);
            assert_eq!(p.facets.len(), 6);
            for facet in &p.facets {
                assert!(facet.score <= 100, "{} out of range", facet.name);
            }
        }
        assert!(
            pt.facet("Direction") > ps.facet("Direction"),
            "an operator that climbs should beat a spectator on Direction"
        );
        assert!(
            pt.overall > ps.overall,
            "the operator should out-engage idle"
        );
    }

    /// The fun synthesis is deterministic and always says something.
    #[test]
    fn the_fun_assessment_speaks_deterministically() {
        let runs = |seed| -> Vec<Transcript> {
            roster()
                .into_iter()
                .map(|s| run(seed, 3_000, 200, s))
                .collect()
        };
        let a = assess_fun(&runs(5));
        let b = assess_fun(&runs(5));
        assert!(!a.is_empty(), "the fun assessment should produce findings");
        assert_eq!(a.len(), b.len(), "same seed ⇒ same fun findings");
        assert_eq!(a[0].message, b[0].message);
    }

    /// The UI audit parses the binding/shell contract and flags wiring gaps.
    #[test]
    fn ui_audit_parses_the_contract() {
        // A binding with one func the shell wires, one it doesn't, and a phantom
        // call the shell makes to a non-existent binding.
        let rust = "#[func]\nfn buy(&mut self) {}\n#[func]\nfn lonely(&self) {}\n";
        let gd = "func _x():\n\tsim.buy(0,1,2)\n\tsim.ghost(9)\n\tprint(sim.buy(1))\n";
        let m = ui::model_from_sources(rust, gd);
        assert_eq!(m.bindings.len(), 2);
        assert!(m.bindings.contains("buy") && m.bindings.contains("lonely"));
        assert_eq!(
            m.unreached(),
            vec!["lonely"],
            "lonely is exposed but unwired"
        );
        assert_eq!(m.phantom_calls(), vec!["ghost"], "ghost has no binding");
        let findings = ui::audit(&m);
        assert!(
            findings
                .iter()
                .any(|f| f.area == "UI · wiring" && matches!(f.severity, Severity::Concern)),
            "a phantom call must raise a wiring concern"
        );
    }

    /// The audit runs against the *real* committed shell + binding and produces a
    /// verdict on every usability dimension (deterministic, headless).
    #[test]
    fn ui_audit_reads_the_real_shell() {
        let (model, findings) = ui::audit_repo();
        assert!(
            model.sources_found,
            "the shell/binding sources should be found"
        );
        assert!(
            model.bindings.len() > 50,
            "the binding surface should be rich"
        );
        assert!(!findings.is_empty());
        assert!(
            findings.iter().any(|f| f.area.starts_with("UI ·")),
            "every finding is a UI dimension"
        );
    }

    /// The transcript's event tally matches the raw event stream (the harness
    /// counts what actually happened).
    #[test]
    fn the_tally_matches_the_stream() {
        let mut sim = torch_core::sim::Sim::new(2);
        let mut cuts = 0u64;
        for _ in 0..1_500 {
            for e in sim.step() {
                if matches!(e, Event::HaulerInterdicted { .. }) {
                    cuts += 1;
                }
            }
        }
        let t = run(2, 1_500, 200, Box::new(strategy::Spectator));
        assert_eq!(
            t.haulers_interdicted, cuts,
            "tally diverged from the stream"
        );
    }
}
