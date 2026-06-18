# TORCH

> ⚠️ **This is a private experiment.** TORCH is a personal test to see how far a
> full game can be taken **purely with vibe coding** — driving an AI coding
> agent end-to-end, with the human in a creative-director seat rather than
> writing the code by hand. It is not a commercial product, not accepting
> contributions, and makes no promises about completeness, support, or
> stability. The point is the experiment: *how close to a real, buildable game
> can vibe coding get?*

---

TORCH is a hard sci-fi industrial sandbox: real-time-with-pause, offline,
logistics-first, with a foreshadowed ring-gate destination that pulls the player
up through tiers of scale. It's a "spreadsheet sim in space" in the lineage of
Aurora 4X and EVE — the depth of decision is the fun, driven by **parameterized
standing orders** (set a behavior preset + tunable params → the sim executes →
exceptions surface to an alert feed) over a **map + master-tables** control
surface.

The authoritative design lives in
[`TORCH_Unified_Design_Document2.md`](TORCH_Unified_Design_Document2.md).

## Architecture

| Concern | Choice |
| --- | --- |
| Sim core | **Rust**, deterministic, engine-agnostic (`crates/torch-core`) |
| Determinism | Integer / fixed-point math; PCG32 RNG; no floats in probability paths |
| Engine / shell | **Godot 4.6** (`godot/`), loads the Rust core via gdext |
| Persistence | serde JSON snapshot save/load; seed + tick rebuild content |
| Testing | Native `cargo test` for sim acceptance; QA autoplayer harness |
| Platform | **Android-first**; APK via GitHub Actions |

All game logic lives in the pure `sim` modules (no engine imports), so the core
is headless and native-testable; the Godot layer is only the renderer/shell.

## Layout

```
crates/torch-core/   Rust deterministic core (sim + gdext binding)
crates/torch-qa/     automated gameplay-review harness (autoplayer personas)
godot/               Godot 4.6 project (3D orrery + command-deck shell)
docs/                design reviews + influence model
.github/workflows/   CI (fmt/clippy/test) + Android APK pipeline
```

## Commands

```bash
cargo test --all                              # native sim acceptance tests
cargo fmt --all --check                       # formatting (CI gate)
cargo clippy --all-targets -- -D warnings     # lints (CI gate)
cargo build --release                         # builds the GDExtension
cargo run -p torch-qa                          # prints a Markdown gameplay review
# Godot: open godot/ in Godot 4.6
```

## License

See [LICENSE](LICENSE).
