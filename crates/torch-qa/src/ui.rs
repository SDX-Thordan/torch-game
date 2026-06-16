//! UI usability audit — the third lens on the build.
//!
//! [`crate::review`] asks *does the sim work*, [`crate::engagement`] asks *is it
//! engaging*. This asks *can the player actually see and reach it all?* — a
//! **usability** lens on the shell.
//!
//! The Godot shell is GDScript, outside the Rust workspace and the `cargo test`
//! gate. But the shell can only touch the sim through the **gdext binding** (the
//! `#[func]` surface in `torch-core`'s `lib.rs`), and it wires that binding to
//! input and display in `godot/main.gd` + `godot/ui/*.gd`. That contract is
//! committed source, so we can audit it *statically and deterministically* —
//! without a running engine — for the affordance gaps that quietly hurt
//! usability:
//!
//! - **Phantom calls** — the shell calling a binding that doesn't exist (a
//!   runtime break GDScript's dynamic typing won't catch until that code path
//!   runs).
//! - **Unreached capability** — verbs/state exposed by the binding the shell
//!   never wires (the player literally can't get to them).
//! - **Exceptions without a press** — the §0.4 "exceptions are verbs" promise:
//!   an act-now shortage must have a one-press answer wired.
//! - **Status visibility** (Nielsen #1) — the load-bearing state (treasury,
//!   tier, the gate, the alert feed) must be on screen.
//! - **Recognition over recall** (Nielsen #6) — a controls legend, not a keymap
//!   to memorise.
//! - **Platform fit** — TORCH is **Android-first** (§33), so a large
//!   keyboard-only control surface is a real usability risk on touch.
//!
//! This complements (doesn't replace) the GUT view tests and the manual
//! render-and-look pass — it's the cheap, deterministic affordance check.

use crate::review::{Finding, Severity};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// A static model of the UI contract, parsed from committed source.
#[derive(Clone, Debug, Default)]
pub struct UiModel {
    /// `#[func]` names the binding exposes to the shell (the UI API).
    pub bindings: BTreeSet<String>,
    /// `sim.<name>(` calls the shell actually makes (what it wires).
    pub shell_calls: BTreeSet<String>,
    /// Distinct keyboard bindings in the shell's keymap (`KEY_*` match arms).
    pub keymap_arms: usize,
    /// Pointer/button affordances (mouse, drag, `Button`, `.pressed.connect`).
    pub pointer_hits: usize,
    /// Explicit touch handling (`InputEventScreenTouch`/`Drag`) — native to the
    /// Android target, not just mouse that Godot emulates from touch.
    pub touch_native: bool,
    /// An on-screen controls legend is present (recognition over recall).
    pub help_legend: bool,
    /// Whether the binding/shell sources were found and parsed at all.
    pub sources_found: bool,
}

impl UiModel {
    /// Bindings the shell never references — exposed but unreached capability.
    pub fn unreached(&self) -> Vec<&str> {
        self.bindings
            .iter()
            .filter(|b| !self.shell_calls.contains(*b))
            .map(String::as_str)
            .collect()
    }

    /// Shell calls with no matching binding — would break at runtime.
    pub fn phantom_calls(&self) -> Vec<&str> {
        self.shell_calls
            .iter()
            .filter(|c| !self.bindings.contains(*c))
            .map(String::as_str)
            .collect()
    }

    /// Does the shell reference *any* of these binding names (substring-tolerant)?
    fn wires_any(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.shell_calls.contains(*n))
    }
}

/// Parse the `#[func]` binding names out of the gdext binding source.
pub fn parse_bindings(rust_src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut armed = false;
    for line in rust_src.lines() {
        let t = line.trim_start();
        if t.starts_with("#[func]") {
            armed = true;
            continue;
        }
        if armed {
            if let Some(name) = t.strip_prefix("fn ").and_then(|r| {
                r.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
            }) {
                if !name.is_empty() {
                    out.insert(name.to_string());
                    armed = false;
                }
            }
            // tolerate a visibility/async qualifier line between attr and fn
        }
    }
    out
}

/// Parse the `sim.<name>(` calls the shell makes out of concatenated GDScript.
pub fn parse_shell_calls(gd_src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, _) in gd_src.match_indices("sim.") {
        let rest = &gd_src[i + 4..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        // Only count it as a call if an open-paren follows the identifier.
        if !name.is_empty() && rest[name.len()..].trim_start().starts_with('(') {
            out.insert(name);
        }
    }
    out
}

/// Build a [`UiModel`] from the binding source and the concatenated shell source.
pub fn model_from_sources(rust_src: &str, gd_src: &str) -> UiModel {
    let keymap_arms = gd_src
        .lines()
        .filter(|l| l.trim_start().starts_with("KEY_"))
        .count();
    let pointer_hits = [
        "InputEventMouseButton",
        "_gui_input",
        ".pressed.connect",
        "InputEventScreenDrag",
    ]
    .iter()
    .map(|p| gd_src.matches(p).count())
    .sum();
    let touch_native =
        gd_src.contains("InputEventScreenTouch") || gd_src.contains("InputEventScreenDrag");
    let help_legend = gd_src.contains("_help.text") || gd_src.contains("[Space");

    UiModel {
        bindings: parse_bindings(rust_src),
        shell_calls: parse_shell_calls(gd_src),
        keymap_arms,
        pointer_hits,
        touch_native,
        help_legend,
        sources_found: !rust_src.is_empty() && !gd_src.is_empty(),
    }
}

/// Status the player must be able to *see* (Nielsen #1) — matched against the
/// bindings the shell wires (substring-tolerant where names vary).
const MUST_SHOW: &[(&str, &[&str])] = &[
    ("treasury", &["credits"]),
    (
        "the tier / destination",
        &["tier_name", "gate_progress_pct", "gate_progress"],
    ),
    ("the alert feed", &["alert_count", "alert_message"]),
    ("the now-goal", &["now_goal", "now_goal_progress"]),
];

/// Verbs that answer an act-now exception (§0.4) — at least one must be wired.
const EXCEPTION_VERBS: &[&str] = &[
    "answer_shortage",
    "exploit_shortage",
    "answer_top_shortage",
    "fulfill_ready_contract",
];

/// Run the usability audit over the parsed UI model.
pub fn audit(m: &UiModel) -> Vec<Finding> {
    let mut f = Vec::new();

    if !m.sources_found {
        f.push(Finding {
            severity: Severity::Note,
            area: "UI · sources",
            message: "Could not locate the shell/binding sources to audit (run from the repo). The UI usability audit is static — it reads `crates/torch-core/src/lib.rs` and `godot/*.gd`.".to_string(),
        });
        return f;
    }

    // 1. Phantom calls — a hard correctness/usability break.
    let phantom = m.phantom_calls();
    if phantom.is_empty() {
        f.push(Finding {
            severity: Severity::Good,
            area: "UI · wiring",
            message: format!(
                "Every one of the shell's {} sim calls resolves to a real binding — no phantom calls that would break at runtime (GDScript wouldn't catch them until that path runs).",
                m.shell_calls.len()
            ),
        });
    } else {
        f.push(Finding {
            severity: Severity::Concern,
            area: "UI · wiring",
            message: format!(
                "The shell calls {} binding(s) that don't exist: {phantom:?}. These break at runtime — the gdext boundary is dynamically typed, so `cargo test` can't catch them.",
                phantom.len()
            ),
        });
    }

    // 2. Coverage — exposed-but-unreached capability.
    let unreached = m.unreached();
    let coverage = (m.bindings.len() - unreached.len()) * 100 / m.bindings.len().max(1);
    let sev = if coverage >= 60 {
        Severity::Note
    } else {
        Severity::Concern
    };
    f.push(Finding {
        severity: sev,
        area: "UI · coverage",
        message: format!(
            "The shell wires {coverage}% of the {} exposed bindings; {} are never referenced (e.g. {:?}). Some are deliberately read-only or future, but a verb the shell never calls is a capability the player can't reach.",
            m.bindings.len(),
            unreached.len(),
            unreached.iter().take(6).collect::<Vec<_>>()
        ),
    });

    // 3. Exceptions are verbs (§0.4) — a one-press answer must be wired.
    if m.wires_any(EXCEPTION_VERBS) {
        f.push(Finding {
            severity: Severity::Good,
            area: "UI · exception→verb",
            message: "The act-now exception loop is pressable: the shell wires a one-press answer to shortages/contracts, so an alert resolves into a verb rather than a dead notification (§0.4).".to_string(),
        });
    } else {
        f.push(Finding {
            severity: Severity::Concern,
            area: "UI · exception→verb",
            message: "No one-press answer to act-now exceptions is wired — the feed can raise a shortage the player has no surfaced verb to resolve (§0.4 'exceptions are verbs, not acknowledgments').".to_string(),
        });
    }

    // 4. Visibility of system status (Nielsen #1).
    let missing: Vec<&str> = MUST_SHOW
        .iter()
        .filter(|(_, names)| !m.wires_any(names))
        .map(|(label, _)| *label)
        .collect();
    if missing.is_empty() {
        f.push(Finding {
            severity: Severity::Good,
            area: "UI · status visibility",
            message: "The load-bearing state is on screen — treasury, the tier/destination, the alert feed, and the now-goal are all read by the shell (Nielsen #1, the §0 three-horizon stack).".to_string(),
        });
    } else {
        f.push(Finding {
            severity: Severity::Concern,
            area: "UI · status visibility",
            message: format!("Load-bearing state isn't surfaced: {missing:?} have no binding the shell reads. The player can't see what they can't reach (Nielsen #1)."),
        });
    }

    // 5. Recognition over recall (Nielsen #6).
    if m.help_legend {
        f.push(Finding {
            severity: Severity::Good,
            area: "UI · recognition",
            message: "A controls legend is on screen — the keymap is recognised, not recalled (Nielsen #6).".to_string(),
        });
    } else {
        f.push(Finding {
            severity: Severity::Note,
            area: "UI · recognition",
            message: "No on-screen controls legend found — players must recall the keymap from memory (Nielsen #6).".to_string(),
        });
    }

    // 6. Platform fit — Android-first (§33) vs. a keyboard-heavy control surface.
    let touch = if m.touch_native {
        "native touch handling"
    } else if m.pointer_hits > 0 {
        "pointer/button handling (touch via Godot's mouse emulation)"
    } else {
        "no pointer or touch handling"
    };
    if m.keymap_arms >= 25 && !m.touch_native {
        f.push(Finding {
            severity: Severity::Concern,
            area: "UI · platform fit",
            message: format!(
                "{} keyboard bindings drive the shell, but TORCH is Android-first (§33) and has {touch}. A keyboard-scale control surface doesn't fall to thumbs — the verbs need first-class touch targets (the master-tables/orrery already give a pointer surface to build on).",
                m.keymap_arms
            ),
        });
    } else {
        f.push(Finding {
            severity: Severity::Note,
            area: "UI · platform fit",
            message: format!(
                "{} keyboard bindings, with {touch}. Keep the touch surface first-class for the Android target (§33).",
                m.keymap_arms
            ),
        });
    }

    f
}

/// Locate and read the binding + shell sources from the repo (relative to this
/// crate). Returns `(rust_src, concatenated_gd_src)`; either may be empty if not
/// found (the audit degrades to a single Note).
pub fn load_sources() -> (String, String) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let rust =
        std::fs::read_to_string(root.join("crates/torch-core/src/lib.rs")).unwrap_or_default();
    let mut gd = std::fs::read_to_string(root.join("godot/main.gd")).unwrap_or_default();
    if let Ok(dir) = std::fs::read_dir(root.join("godot/ui")) {
        for entry in dir.flatten() {
            if entry.path().extension().is_some_and(|e| e == "gd") {
                if let Ok(s) = std::fs::read_to_string(entry.path()) {
                    gd.push('\n');
                    gd.push_str(&s);
                }
            }
        }
    }
    (rust, gd)
}

/// Build the model from the on-disk sources and run the audit.
pub fn audit_repo() -> (UiModel, Vec<Finding>) {
    let (rust, gd) = load_sources();
    let model = model_from_sources(&rust, &gd);
    let findings = audit(&model);
    (model, findings)
}

// ---- Markdown rendering ----------------------------------------------------

/// Render the UI usability audit section.
pub fn render_audit(out: &mut String) {
    let (m, findings) = audit_repo();

    let _ = writeln!(out, "## UI usability audit\n");
    let _ = writeln!(
        out,
        "_A **static** affordance audit of the shell's contract with the sim — the gdext binding \
         (`#[func]`) and how `godot/*.gd` wires it. It can't see pixels (that's the GUT view tests \
         and the manual render pass); it catches the affordance gaps that quietly hurt usability: \
         calls that would break, capability the player can't reach, and platform fit._\n"
    );

    if m.sources_found {
        let unreached = m.unreached().len();
        let coverage = (m.bindings.len() - unreached) * 100 / m.bindings.len().max(1);
        let _ = writeln!(out, "| metric | value |");
        let _ = writeln!(out, "| --- | --- |");
        let _ = writeln!(out, "| bindings exposed | {} |", m.bindings.len());
        let _ = writeln!(
            out,
            "| wired by the shell | {} ({coverage}%) |",
            m.shell_calls.len()
        );
        let _ = writeln!(out, "| keyboard bindings | {} |", m.keymap_arms);
        let _ = writeln!(
            out,
            "| pointer/touch | {} pointer hit(s), native touch: {} |",
            m.pointer_hits, m.touch_native
        );
        let _ = writeln!(out, "| controls legend | {} |\n", m.help_legend);
    }

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
