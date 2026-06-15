# CLAUDE.md — TORCH project working notes

Durable memory for building **TORCH** (see `TORCH_Unified_Design_Document2.md`,
the authoritative GDD). Read at the start of every session; update whenever a
decision is made or a lesson is learned.

**Companion authorities (in `docs/`):**
- `docs/PLAYABLE_STATE_REVIEW.md` — the playable-state review (gaps + sequenced path).
- `docs/TORCH_Player_Influence_and_Interaction_Model.md` — *what* the player can
  influence, *how*, and *where it's pressable*. Identity: a **spreadsheet sim in
  space** (Aurora 4X / EVE); depth of decision is the fun. The heart is
  **parameterized standing orders** (Behavior Preset + tunable params → sim
  executes → exceptions to the feed) and **map + master-tables** hybrid control.
  This drives all UI/agency work.

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
- [x] **2. Lock the §0 spine** — built it in code instead of paper (`sim::campaign`):
  tiers (Station→Region→Sol→Gate), the now/tier/far goal stack, and the
  always-visible ring-gate. Player operations climb it; ascents are voiced.
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
- [x] **6. Ship design & fitting** (`sim::ships`) — data-driven hull/weapon
  catalogs (4 warships + Q-ship + civilians), integer fitting validation (slots,
  power, tankage, crew), derived stats (delta-v proxy, alpha, mobility, the
  railgun escalation axis), and the captain + crew-quality model (§8c).
- [x] **7. Combat resolver** (`sim::combat`) — headless range-band doctrine sim
  consuming §8 fits: railguns rule at range, torpedo salvos *saturate* the PDC
  screen up close (the equalizer), crew quality scales it. Diorama (§22) later.
- [x] **8. Alert-feed system** (`sim::alerts`) — consumes the world event stream
  (§29) into ranked, voiced alerts with a hard FYI/act-now split; act-now alerts
  carry a verb (§0.4), threshold is player-tunable (§19). Crew-attachment depth
  (history/portraits, §11) right-sized later.
- [x] **9. Progression** — four layered tracks (§10).
  - [x] Factions + reputation (`sim::faction`): standings/tiers per faction, the
    §7b ripple wired (a *player* cut angers the owner, pleases their rival;
    pirate raids don't blame the player). Markets are faction-owned.
  - [x] Research tree + blueprint discovery (seed+params, rep-gated) + CEO skill
    track (level + one perk branch of passive buffs) in `sim::progression`.
- [x] **10. Managers & automation** (`sim::automation`) — run-by-exception policy
  layer: a standing interdiction patrol (faction/min-cargo filter) and
  auto-research run autonomously in `step()`; the alert feed (§19) surfaces the
  consequences. Policy set by the player, executed by managers.
- [ ] **11. Procedural assembly tool** (offline) + baking pipeline.
- [~] **12. Tier ascent + gate foreshadowing** — model + always-visible gate done
  (`sim::campaign`, voiced ascents); per-tier content/"new kind of game" later.
- [ ] **13. Pressure systems** + forecasting + pacing governor.
- [x] **QA. Automated gameplay harness** (`crates/torch-qa`) — autoplayer personas
  drive the deterministic core headless and the run is critiqued into a written
  **gameplay review** (pacing/agency/economy/alerts/reputation + cross-cutting
  design findings). The §32 counterpart to `cargo test`: tests assert systems
  *work*, this critiques how the game *plays*. Same seed ⇒ same review.
- [~] **14. Juice & audio pass**, then UX polish. **First playable shell done**
  (`godot/main.gd`): real-time-with-pause loop (§28), drawn 2D orrery (§21),
  live panels + alert feed (§18/§19), verbs on input (interdict/trade/build).
  Juice/audio/3D-orrery/console-chrome still to come.
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

- **2026-06-14 — Ships & fitting (§8) as pure data + integer fitting.** `sim::ships`
  holds hull/weapon catalogs as data (§31) and validates a `Loadout` against slot
  counts, a power budget, tankage, and the crew minimum (`FitError`). Derived
  `ShipStats` use a **simplified integer delta-v proxy** (`efficiency × remass ÷
  mass`, not true Tsiolkovsky — ln needs floats; revisit if it matters). The §8b
  table fell straight out of the mount counts: railgun mounts 0/1/1/2 are the
  escalation axis, capitals out-alpha escorts, escorts out-maneuver (thrust÷mass)
  and out-range (delta-v) capitals — verified live in the shipyard demo. Crew is a
  named captain (deterministic procedural name, §11) + an abstract quality rating
  that scales effective alpha and grows via `gain_experience` (§8c bottleneck).
  Fleet-wide trained-crew *pool* caps and progression deferred to steps 8–10;
  procedural meshes to step 11. Combat (step 7) will consume these stats.

- **2026-06-14 — Combat resolver (§9) — the band decides.** `sim::combat::resolve`
  runs two fleets to the death at one negotiated range **band** (faster fleet sets
  it). Each tick: railgun volleys (best at Long, poor Close), close-band PDC brawl,
  and torpedo **salvos** resolved as saturation — `leakers = salvo − screen×band`,
  applied as focus-fire. **Key tuning:** continuous fire is lethal fast, so the
  opening salvo must land on tick 1 (init reload 0) — otherwise the capital shreds
  the wing before torpedoes ever fly, and saturation never matters. With that, the
  §8a/§8f tension is emergent and verified: 1–4 frigates always lose; **8 saturate
  and win at Close** but **lose at Long** (full screen + railgun reach); 12 win
  Close/Medium; crew quality scales offense+screen so a veteran wins a mirror.
  Numbers are tuning knobs (hp = armor + mass/10, screen = pdc_intercept/5, band
  railgun/intercept curves). Emits a BattleLog `CombatEvent` stream for the §22
  diorama. rng adds ±12% volley jitter; outcomes deterministic per seed.

- **2026-06-14 — Alert feed (§19) — the voiced exception stream.** `sim::alerts`
  consumes the world `Event` stream (§29) into ranked `Alert`s with a hard
  **FYI vs act-now** split; act-now alerts (scarcity) carry a `Verb`
  (`ExploitShortage`) per §0.4, raids are FYI notices. A player-tunable
  `min_priority` threshold decides what `surfaced()` returns (ranked priority then
  recency). Messages are **voiced** by deterministically-named managers with a
  tone (Terse/Wry), the start of §11 attachment. `Sim` owns a feed and ingests
  each tick's events in `step()`; bound via `TorchSim` (alert_count/message/
  is_act_now + set_alert_threshold). Routine traffic (departed/arrived/tick) is
  filtered as non-feed-worthy to avoid notification spam. Bounded ring buffer
  (64). Lesson: an unread `domain` field tripped `clippy::dead_code` under
  `-D warnings` — managers are distinguished by their feed slot, so the field went.

- **2026-06-14 — Factions + reputation (§4/§10) + the deferred §7b ripple.**
  `sim::faction` models the four powers (Earth/Mars/Belt/Independents), per-faction
  standings (clamped ±1000) and tiers (Hostile→Allied). Markets are now
  faction-owned (Ceres=Belt, Earth=Earth). Cutting a hauler now closes the §7b
  loop: a **player** interdiction sours relations with the cargo's owner faction
  and pleases their rival (Earth↔Mars peers; Belt resents Earth) — but **pirate**
  raids don't (the player isn't blamed), so `cut_hauler` returns the hauler and
  only the player paths call `ripple_reputation`. Verified live: interdicting an
  Earth hauler → Earth −50, Mars +20. Research/blueprints/CEO tracks next (9b).

- **2026-06-14 — Progression tracks (§10) — kept light (§0.2).** `sim::progression`
  holds three player-driven (no-RNG) tracks: a **research** tree (cheap prereqs →
  percent stat bonuses, `drive/armor/screen_bonus`), **blueprints** (a design =
  seed + `BlueprintParams`, §25; faction designs gated behind a reputation
  threshold checked against `Relations`), and the **CEO** (level from XP + one
  one-time perk branch whose `buff()` boosts its domain). `Sim` owns a
  `Progression` + exposes read/mut accessors and `discover_blueprint` (which
  passes its own `relations` to honor the gate). Bound to Godot; demo shows a CEO
  hitting level 4/Warlord, a drive tech, and a discovered blueprint. Each pub
  struct field stays reachable through the re-exports, so no dead-code trip.

- **2026-06-14 — Managers & automation (§12) — run by exception.** `sim::automation`
  holds a `Copy` `AutomationPolicy` (an `InterdictionPolicy` with enable/faction/
  min-cargo filter + a standing `patrol` Interceptor, plus `auto_research`). `Sim`
  owns it; `run_automation()` runs each `step()` after pirates: on a 12-tick patrol
  cadence the manager picks the fattest matching in-flight hauler and flies the
  same `interdiction::resolve` the player would, cutting it (player attribution →
  `ripple_reputation`); `auto_research` spends on `cheapest_researchable()`. The
  loop copies `self.policy` first to avoid holding a borrow across the mutations.
  Default policy is all-off, so existing tests (relations stay neutral) are
  unaffected. Demo: a company auto-hunting Earth drove Earth to −900 hands-off.
  Lesson: an all-default `Default` impl trips `clippy::derivable_impls` — derive it.

- **2026-06-15 — Retention spine in code (§0), per the first review.** The review
  flagged the GDD's #1 priority (the destination pull) as entirely absent while the
  engine was over-built. `sim::campaign` fixes that: `Tier`
  (Station→Region→Sol→Gate), a three-horizon `now_goal` (text + progress + target)
  and an always-visible `gate_progress_bp` (the far goal, foreshadowed from minute
  one). Player operations (`ripple_reputation`, i.e. every player/managed
  interdiction) call `record_op`; crossing a tier threshold emits
  `Event::TierAscended`, which the alert feed voices as a **Critical** "The Board"
  milestone (the §0.3 arrival fanfare). Bound to Godot as a DESTINATION panel.
  Ops-per-tier 3/10/25 is a placeholder ladder; richer per-tier objectives + the
  "different kind of game" per tier come later. Lesson: keep the metric that drives
  the spine attributable to the *player* (reuse the player-interdiction path), not
  ambient events, so pirates don't advance your climb.

- **2026-06-15 — Player corporation (§1/§5) — the review's #1 gap closed.** The
  pushed review (`docs/PLAYABLE_STATE_REVIEW.md`, Phase A.1) named player-agent
  state the foundational missing piece: the sim had a convincing NPC world but no
  player economic actor. `sim::corp::Corp` is now that actor — a treasury, a
  per-commodity warehouse, an owned fleet, and the trained-crew pool (§8c). The
  verbs live on `Sim` (it owns the markets + rng): `buy`/`sell` move cargo against
  a `Market` at its live price (and nudge it), `commission_ship` pays a hull's
  build cost and **draws crew from the pool** — so the §8c bottleneck (not the
  treasury) caps capital ships (starting credits 50k afford a battleship, but its
  120 crew exceed the 60-pool). First real agency: a manual arbitrage round-trip
  (buy ReactorFuel cheap at Earth, sell dear at Ceres) profits, the same spread the
  NPC haulers work. Verified live: +3560 cr arbitrage, then a frigate commissioned.

- **2026-06-15 — First playable shell (§18–§21) — the review's Phase B.** `main.gd`
  is no longer a hello-world dump: it's a `Node2D` game controller. `_process`
  drives `sim.step()` on a real clock scaled by a pause/1×/6×/24× `speed_idx`
  (§28); backgrounding/focus-out forces pause (§6). `_draw` renders the orrery
  (orbit rings, bodies, the in-flight haulers you hunt) at compressed scale over
  true sim distances (§21). Labels show the top-bar clock+treasury, the NOW goal +
  two-market price board + your cargo, and the ranked alert feed (§19).
  `_unhandled_input` maps keys to the actual sim verbs — Space/1/2/3 time, Tab
  select, **I interdict**, T trade (arbitrage), B build — so the §7b fun engine is
  finally *pressable*, the GDD's top risk (§36). Verified headless: the loop
  advances the clock without script errors (T+0→T+7 over frames). Interactive
  input + `_draw` only exercise on a device/desktop; CI stays headless. **This is
  the first playable state**: open it, watch the world, control time, press verbs.
  Next: the §17 3D orrery, the diegetic console chrome (§20), and juice/audio (§23).

- **2026-06-15 — Trade-route standing orders (§4 of the influence model).** First
  parameterized standing order, per the new `docs/TORCH_Player_Influence...` model:
  `sim::logistics::TradeRoute` (commodity, origin→dest, qty, min_margin) is set by
  the player; `Sim::run_logistics` flies an owned **freighter** on the loop each
  tick — buy at origin when the spread clears the margin, travel (orrery distance ÷
  cruise), sell at dest, bank the profit, repeat — all hands-off. Below the margin
  or with no freighter it goes **idle** (the exception the shell surfaces). This is
  the spreadsheet-sim's policy→execute→exception loop: the player tunes params, the
  sim runs them. `route` is `Copy`, so `run_logistics` copies it out, mutates, and
  writes it back — no borrow fight with `self.markets`/`self.corp`. Bound: F
  commission freighter, D set route from the trade cursor, G clear. Replaces
  instant teleport-arbitrage with real logistics over time.

- **2026-06-15 — Player stations + Produce standing order (§3.1, Example A).**
  `sim::industry::Station` is a `Copy` Produce preset (input recipe, output, rate,
  buy/sell markets, sell-surplus threshold, production ceiling). `Sim::run_industry`
  runs each station hands-off: source the raw input from a market when short →
  transform raw→refined (output = input + RAW_COUNT) → dump output above the
  sell-surplus floor for credits. `found_refinery(raw, buy, sell)` costs capital
  (8k), capped at 4 stations (Tier-1). The value-add is real: buy Ore cheap (~22),
  refine to Metals, sell dear (~220) — a refinery nets profit with no input, the
  mine→refine→sell chain. Bound: M founds a refinery for the selected raw commodity
  at the selected market. Same Copy-out-of-self pattern as routes so the per-tick
  loop doesn't fight the `markets`/`corp` borrows. The default Sim has no stations,
  so the §7c stability gate is untouched.
- **2026-06-15 — Player stations + Produce standing order (§3.1, Example A).**
  `sim::industry::Station` is a `Copy` Produce preset (input recipe, output, rate,
  buy/sell markets, sell-surplus threshold, production ceiling). `Sim::run_industry`
  runs each station hands-off: source the raw input from a market when short →
  transform raw→refined (output = input + RAW_COUNT) → dump output above the
  sell-surplus floor for credits. `found_refinery(raw, buy, sell)` costs capital
  (8k), capped at 4 stations (Tier-1). The value-add is real: buy Ore cheap (~22),
  refine to Metals, sell dear (~220) — a refinery nets profit with no input, the
  mine→refine→sell chain. Bound: M founds a refinery for the selected raw commodity
  at the selected market. Same Copy-out-of-self pattern as routes so the per-tick
  loop doesn't fight the `markets`/`corp` borrows. The default Sim has no stations,
  so the §7c stability gate is untouched.

- **2026-06-15 — Automated gameplay QA harness (`crates/torch-qa`).** The
  deterministic core is *playable by a program*, so QA can be a bot, not just unit
  tests. New native crate: a `Strategy` trait + five autoplayer **personas**
  (Spectator/Arbitrageur/Logistician/Privateer/Tycoon), a `harness` that drives a
  persona for thousands of ticks and records a `Transcript` (event tallies +
  periodic state samples), and a `review` engine that emits ranked `Finding`s plus
  a cross-cutting `design_review`. `cargo run -p torch-qa` prints a Markdown
  gameplay review (sample committed at `docs/SAMPLE_GAMEPLAY_REVIEW.md`); same seed
  ⇒ same review, so feel-regressions diff. The first run already paid for itself —
  it surfaced real design gaps the unit tests can't see:
  - **The retention spine is fed by a single verb.** Only player *interdiction*
    calls `record_op`, so trading/routing/building/researching never advance a
    tier — the bulk of the influence model doesn't touch the §0 destination pull.
  - **Combat is unreachable in the live loop.** `sim::combat` has no trigger on
    `Sim` (no fleet-engagement verb); ships are commissioned but never fight.
  - **Unbounded arbitrage.** Hand-trading compounded ~100× with no wealth-scaled
    sink, and the *instant* buy/sell verbs strictly dominate the transit-paying
    standing route they're meant to motivate.
  - **Player-verb events are dropped (engine bug).** Verbs called between ticks
    push onto `Sim::events`, but the next `step()` opens with `events.clear()` —
    so a player interdiction's `Scarcity`/`TierAscended` are wiped before the feed
    or the returned stream ever reads them. Player cuts raise *no* act-now alert
    and ascents go unvoiced; only sim-internal cuts (pirates/automation) are heard.
    Worth fixing so the §0.3 fanfare + §0.4 "exploit shortage" fire for the player.
  - **Reputation is a one-way cliff** (raiding → Hostile with no recovery path).
  Harness lesson: don't trust the event stream for player-caused state changes —
  observe *campaign state* directly (poll `tier()` each tick) and keep the event
  tally only to *detect* the dropped-event discrepancy.

- **2026-06-15 — QA finding #1 fixed: player-verb events survive the step (#27).**
  `step()` opened with `self.events.clear()`, wiping anything a between-tick player
  verb (`interdict`/`interdict_with`) had pushed before the feed or the returned
  stream read it. Now `Sim` tracks `returned` (how many leading events the last
  step surfaced) and `step()` drains *only those*, keeping the player tail, then
  ingests + returns it. So a player cut now voices its `Scarcity` (act-now
  "exploit shortage") and a player ascent emits its `TierAscended` (the §0.3
  fanfare) — previously only pirate/automation cuts were heard. The QA
  `design_review`'s "Event plumbing" concern self-resolves (regression detection
  working as intended).

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
