# CLAUDE.md — TORCH project working notes

Durable memory for building **TORCH** (see `TORCH_Unified_Design_Document2.md`,
the authoritative GDD). Read at the start of every session; update whenever a
decision is made or a lesson is learned.

---

## 1. Goal

Implement the **full Unified Design Document**, ending in a **buildable
Android APK** produced by a GitHub Actions release workflow. TORCH is a hard
sci-fi industrial sandbox: real-time-with-pause, offline, logistics-first, with
a foreshadowed ring-gate destination pulling the player up through tiers of
scale (§0).

## 2. Working process (how we ship)

- **Small, focused PRs.** One concern per PR. Each keeps `main` green.
- **Squash-merge to `main`** for a clean release log. Branches: `feat/...`,
  `chore/...`, `fix/...`, `ci/...`.
- **CI is the gate** (`.github/workflows/ci.yml`): fmt + clippy + cargo test.
  Nothing merges red.
- **Headless-first** (§35): sim logic is pure, deterministic, native-tested
  (`cargo test`) before any Godot view sits on top.
- **Update this file every PR**: tick the roadmap, append to the learnings log.
- **Hygiene:** never put model identifiers or internal tooling names in commits,
  PR text, or code.

## 3. Architecture & tech stack (per GDD Part IV)

| Concern | Choice | GDD |
| --- | --- | --- |
| Sim core | **Rust**, deterministic, engine-agnostic (`crates/torch-core`, builds a `cdylib` GDExtension + `rlib` for tests) | §26, §27 |
| Determinism | Integer / fixed-point math; **PCG32** RNG with integer basis-point probabilities; no floats in probability paths | §27 |
| Engine / shell | **Godot 4.6** (`godot/`), loads the Rust core via **gdext** (`torch.gdextension`) | §26 |
| Sim ↔ view | Snapshot + typed event stream (BattleLog-style) — *to build* | §29 |
| Persistence | serde + bincode (binary), JSON dev export — *to build* | §30 |
| Tuning data | Hot-reloadable JSON/RON; logic in Rust, numbers in data — *to build* | §31 |
| Testing | Native `cargo test` for sim acceptance; GUT for Godot/view later | §32 |
| Platform / build | **Android-first**; de-risk Rust-on-Android early; APK via GitHub Actions | §33, §35.1 |
| Art | Voxel aesthetic, authored designs baked to meshes (post-foundation) | §24, §25 |

**Determinism rules:** no `Math.random`/float RNG in the sim (use
`sim::rng::Pcg32`); no wall-clock in the sim; fixed tick only; content is data.

**Boundary rule:** all game logic lives in `sim` (pure, no `godot` imports);
`lib.rs` is only the thin gdext binding. This keeps the core headless and
native-testable.

## 4. Repo layout

```
crates/torch-core/        Rust deterministic core
  src/lib.rs              gdext binding (thin)
  src/sim/                pure engine-agnostic sim (rng, + economy/orbit/... to come)
godot/                    Godot 4.6 project (shell/renderer)
  project.godot, main.*   hello-world scene calling the Rust core
  torch.gdextension       binds the cdylib per platform
  bin/                    (gitignored) cross-compiled Android libs, staged in CI
Cargo.toml                workspace
.github/workflows/        ci.yml (fmt/clippy/test); apk workflow to come
TORCH_Unified_Design_Document2.md   authoritative GDD
```

## 5. Commands

```bash
cargo test --all        # native sim acceptance tests
cargo fmt --all         # / --check in CI
cargo clippy --all-targets -- -D warnings
cargo build --release   # produces target/release/libtorch_core.so (the GDExtension)
# Godot: open godot/ in Godot 4.6 (the .gdextension points at the target/ lib)
```

## 6. Roadmap (GDD §35 build order → PRs)

Status: [x] done, [~] in progress, [ ] todo.

- [x] **1. De-risk Rust-on-Android** — gdext hello-world + Android export APK.
  - [x] Rust core crate + gdext binding + Godot hello-world scene + native CI.
  - [x] Android APK pipeline (cargo-ndk cross-compile + Godot 4.6 gradle export →
    signed debug APK, green in CI via `android.yml`).
- [ ] **2. Lock the §0 spine on paper** — destination pull, tier transitions,
  three-horizon goal stack (design note in repo).
- [x] **3. Deterministic core sim** — fixed-tick `Sim`, snapshot + typed event
  contract (§29), stub deterministic orbital model + integer fixed-point trig.
- [~] **4. Economy & industry** (data-driven) **+ headless stability test**.
  - [x] Stockpile pricing (§7a): piecewise damped target, NPC stabilizers, the
    §7c no-death-spiral gate (64 seeds × 5000 ticks). Single self-sufficient market.
  - [x] Multi-market (Ceres producer ↔ Earth consumer) with decoupled setpoints
    → standing two-way price spread.
  - [ ] RON/JSON hot-reloadable commodity data (§31).
- [x] **5. Interdiction prototype** (§7b) — price-arbitrage haulers fly the orrery
  between markets and *damp* spreads; cutting one (`Sim::interdict`) denies the
  delivery → local shortage. Stability re-checked with traffic (32 seeds).
  - [x] Richer interdiction: a real **intercept-geometry + odds** verb
    (`interdict_with`), ambient **NPC pirates** preying on the fattest cargo, and
    **scarcity events** tagging each denied delivery. Stability holds with pirates.
- [ ] **6. Ship design & fitting** — classes, slots, weapons, crew model.
- [ ] **7. Combat resolver** — headless range-band doctrine sim, diorama after.
- [ ] **8. Crew & alert-feed system**.
- [ ] **9. Progression** — research / blueprints / reputation / CEO skills.
- [ ] **10. Managers & automation** (exceptions-as-verbs).
- [ ] **11. Procedural assembly tool** (offline) + baking pipeline.
- [ ] **12. Tier-1→2 ascent + gate foreshadowing**.
- [ ] **13. Pressure systems** + forecasting + pacing governor.
- [ ] **14. Juice & audio pass**, then UX polish.
- [ ] **15. (Post-MVP)** Tier 3 geopolitics → outer frontier → gate/empire.

## 7. Learnings & decisions log (append-only)

- **2026-06-14 — Stack pivot to Godot + Rust.** An earlier TypeScript prototype
  (Vite/Canvas + Capacitor) built the deterministic economy (stockpile pricing,
  stabilizers, §7c headless stability gate), the Hohmann/orbit model, and the
  §7b arbitrage-driven interceptable traffic — all green (31 tests). The updated
  GDD (§26) mandates **Godot 4.x + Rust (gdext)** instead. The prototype is
  archived on branch `prototype/ts` as a validated design reference: its logic
  ports directly to the Rust core (damped pricing, NPC stabilizers, price-driven
  haulers that *damp* spreads, the "no death-spiral on any seed" acceptance
  test). `main` was reset to a clean slate for the new foundation.
- **2026-06-14 — gdext version + Godot 4.6.** `godot = "0.2.4"` is the latest
  published gdext; its API features top out at **`api-4-3`** (no api-4-4/5/6). It
  is **forward-compatible**, so the 4.3-API extension runs on a newer engine — we
  ship on **Godot 4.6.3** with `compatibility_minimum = 4.3` and it loads fine
  (CI: `Initialize godot-rust API v4.3 / runtime v4.6.3`). The reverse fails:
  building against a newer API than the runtime panics. Native `cargo test` works
  with `crate-type = ["cdylib", "rlib"]` — the rlib lets pure `sim` modules be
  tested without a Godot runtime. First gdext build is ~1–2 min (cache it in CI).
- **2026-06-14 — Android APK pipeline (hard-won, see `android.yml`).** Runs in the
  `barichello/godot-ci:4.6.3` container (Godot + templates + Android SDK), with
  Rust + `cargo-ndk` added to cross-compile the GDExtension to `arm64-v8a`. Gotchas,
  each of which failed the *headless* export with an **empty** "configuration
  errors:" message (the real reason is suppressed in headless):
  - **ETC2/ASTC is mandatory.** `rendering/textures/vram_compression/import_etc2_astc=true`
    in `project.godot` — `has_valid_project_configuration` flips invalid *with no
    message* without it. This was the final blocker; everything else is upstream of it.
  - **Editor-settings filename is `editor_settings-<MAJOR>.<MINOR>.tres`** (e.g.
    `editor_settings-4.6.tres`). Wrong name ⇒ the Android SDK path is silently dropped.
  - **GDExtension Android needs the gradle build** (`use_gradle_build=true` +
    `--install-android-build-template`) so the native `.so` is packaged.
  - **Build-tools must match `target_sdk`** (set `target_sdk=34`, install `build-tools;34.0.0`).
  - Container `HOME=/github/home` (not `/root`), so export templates must be staged there.
  - The editor needs the **host** `libtorch_core.so` (`cargo build`) to load the
    extension during export, plus the cross-compiled arm64 lib staged at
    `godot/bin/android/arm64/`.
- **2026-06-14 — Clippy + gdext macros.** `#[godot_api]` expands to `Result`s
  carrying Godot's large `CallError`, tripping `clippy::result_large_err` on
  generated code. Fixed with a crate-level `#![allow(clippy::result_large_err)]`
  so CI can keep `-D warnings` for our own code.
- **2026-06-14 — Determinism primitive.** Implemented PCG32 (`sim::rng`) with a
  bias-free `below()` (rejection sampling) and integer basis-point `chance_bp()`
  per §27 — the RNG every future system draws from.
- **2026-06-14 — Pin the Rust toolchain.** `channel = "stable"` let CI use a
  different rustfmt than local, so `cargo fmt --check` failed on formatting *we*
  couldn't reproduce. Pinned `rust-toolchain.toml` to an exact version (`1.94.1`)
  so fmt/clippy are reproducible CI == local. rustfmt output is not stable across
  versions — always pin.
- **2026-06-14 — Sim↔view contract live.** `sim::Sim` advances a fixed tick and
  returns a typed `Event` stream; `snapshot()` is the render view (§29). Stub
  orbits use integer Bhaskara sin/cos (`sim::fixed`) — no floats in the sim, so
  positions are bit-identical everywhere. Bound to Godot via a thin `TorchSim`.
- **2026-06-14 — Economy ported (stockpile pricing + §7c gate).** Re-implemented
  the prototype's damped piecewise pricing + NPC stabilizers in integer Rust. The
  acceptance gate (`no_death_spiral_on_any_seed`) runs 64 seeds × 5000 ticks and
  accumulates invariants as plain booleans in the hot loop (the prototype's perf
  learning). A proportional stock-restoring stabilizer vs. bounded demand jitter
  keeps a self-sufficient market mean-reverting near reference prices.

- **2026-06-14 — §7b traffic + the stabilizer↔trade tension (key tuning).** Two
  complementary markets (Ceres producer / Earth consumer) get standing spreads by
  **decoupling the stabilizer setpoint from the price anchor** (`target_stock`):
  setpoint in glut ⇒ cheap, in scarcity ⇒ dear. Greedy max-spread arbitrage
  haulers fly the orrery between them; deliveries damp the spread. **Hard-won:** a
  *stiff* proportional stabilizer (20%/tick) instantly neutralizes hauler flows,
  so trade — and therefore interdiction — barely moved prices (~3%), defeating
  §7b. Fix per §7c's own toolkit: make the spring **gentle** (4%/tick) so trade
  meaningfully shifts the average, and rely on **hard stock walls** (inside
  `[0, max_stock]`) to guarantee no death-spiral regardless. Now trade visibly
  damps spreads and `interdict()` measurably starves the destination. Interdiction
  test stays clean because market jitter (the only RNG) advances in lockstep
  across a control vs. cut run, isolating the single denied delivery.

- **2026-06-14 — Richer interdiction (geometry + odds + pirates).** Interdiction
  is now a positioning verb, not a guaranteed delete: `interdiction::resolve`
  finds the **minimum interceptor speed** to reach a hauler on its remaining path
  (sampled pursuit solution, integer `isqrt`), returns `NoSolution` if the
  interceptor lacks the legs, else rolls a hit chance scaled by **speed margin +
  crew skill** (`chance_bp`). The same resolver drives the player's frigate and
  ambient **NPC pirates** (`Sim::pirate_raid`, every 72 ticks vs. the fattest
  cargo). Each cut tags an `Event::Scarcity{market, commodity}` at the destination
  (§7b's "scarcity event"). The no-death-spiral gate now runs *with pirates*
  thinning traffic and still holds — the hard stock walls carry it. Faction-
  relations ripple deferred to the reputation track (step 9). Pirate lethality is
  a tuning knob (lair pos + speed + skill); ~85% on the fattest hauler felt brutal,
  dialed to leave escapes/no-solutions for variety.

### Carried-over design learnings from the TS prototype (still authoritative)

- **Economy pricing anchor.** Price target must be piecewise so `stock == target
  ⇒ basePrice`, sliding to ceiling under scarcity / floor under glut — not a
  band-midpoint map. Otherwise settled prices ignore each commodity's reference.
- **Market self-sufficiency vs. emergent trade.** Making every market
  self-sufficient (base production ≈ 1.1× full demand incl. downstream inputs)
  gives healthy near-reference prices and passes the stability sweep, but it
  *suppresses* deficit-driven trade. Drive §7b haulers by **price arbitrage**
  (cheapest surplus market → dearest with room) instead; equilibrium prices
  differ between markets, so trade flows and *damps* the spread (stabilizing).
  Tension to revisit: comparative-advantage specialization would deepen trade
  but needs a fresh stability check.
- **Stability test performance.** Assert invariants via plain boolean
  accumulation in the hot loop, once at the end — not per-tick assert calls.
