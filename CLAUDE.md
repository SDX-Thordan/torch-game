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
| Persistence | serde JSON snapshot save/load (`sim::persist`); seed + tick rebuild content, overlay player/economy state | §30 |
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
- [x] **4. Economy & industry** (data-driven) **+ headless stability test**.
  - [x] Stockpile pricing (§7a): piecewise damped target, NPC stabilizers, the
    §7c no-death-spiral gate (64 seeds × 5000 ticks). Single self-sufficient market.
  - [x] Multi-market (Ceres producer ↔ Earth consumer) with decoupled setpoints
    → standing two-way price spread.
  - [x] JSON hot-reloadable commodity data (§31): `data/commodities.json` tuning
    overlay (numbers in data, set/identity in code), live `reload_commodities`.
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
  carry a verb (§0.4), threshold is player-tunable (§19). Crew-attachment depth:
  ship names + service history (§11/§14) now in (`OwnedShip`, the Rocinante effect);
  portraits/deeper crew arcs right-sized later.
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
- [x] **12. Tier ascent + gate foreshadowing** — model + always-visible gate +
  voiced ascents (`sim::campaign`); per-tier MVP content now in: each tier has a
  distinct **briefing** (the "different kind of game" reframe, §0.3) and **scope
  that widens as you climb** (station/route caps grow Station→Gate). The post-gate
  "bigger game" (Tier-4 procedural frontier) is tracked under #15 (§17, post-MVP).
- [x] **13. Pressure systems** (`sim::pressure`) — three decaying gauges (faction
  war / piracy / scarcity), **forecasting** (raids telegraphed ahead), a **pacing
  governor** (no two spikes dogpile), biting-but-recoverable decay, and an
  independent **intensity** difficulty knob. Voiced via the feed; gauges on the HUD.
- [x] **QA. Automated gameplay harness** (`crates/torch-qa`) — autoplayer personas
  drive the deterministic core headless and the run is critiqued into a written
  **gameplay review** (pacing/agency/economy/alerts/reputation + cross-cutting
  design findings). The §32 counterpart to `cargo test`: tests assert systems
  *work*, this critiques how the game *plays*. Same seed ⇒ same review.
- [~] **14. Juice & audio pass**, then UX polish. **First playable shell done**
  (`godot/main.gd`): real-time-with-pause loop (§28), drawn 2D orrery (§21),
  live panels + alert feed (§18/§19), verbs on input (interdict/trade/build).
  **Save/load (§30) + a UX/legibility pass now in** (F5/F9, panel backdrops, a
  gate-progress ring, paused indicator, selection reticle). Audio deferred
  indefinitely (player choice); 3D-orrery/deeper console-chrome still to come.
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

- **2026-06-15 — QA finding #6 fixed: a table of standing routes (§4).** The
  standing-order layer was a single `Option<TradeRoute>`; the influence model
  wants a master-table. `Sim` now holds `routes: Vec<TradeRoute>` (capped at
  `MAX_ROUTES` = 4). `run_logistics` lands all arrivals, then dispatches idle
  routes against a **shared freighter pool** (a route only sets out if a
  freighter is free, so the pool — not the route count — bounds concurrent
  trips). `set_trade_route` appends; `clear_trade_route` empties the table;
  `routes()`/`route()` (first) accessors; shell binding gains `route_count` and a
  "+N more" status suffix. Core tests
  `the_route_table_runs_many_routes_on_a_shared_freighter_pool`,
  `the_route_table_is_capped`; the QA Logistician now runs a 2-route / 2-freighter
  table and the `design_review` Logistics finding flips Note → Good.

  **All six original gameplay-QA findings are now resolved (the design review is
  all-Good).** One *new* finding the harness surfaced while wiring combat: matched
  fleet engagements are lopsided (the player held the field in 0% of mirror
  fights), flagged as a combat-balance Note for a later pass.

- **2026-06-15 — QA finding #5 fixed: reputation is a dial, not a one-way cliff.**
  Raiding tanked a faction to Hostile with no modeled way back. `Relations::
  decay_toward_neutral(step)` drifts every standing toward 0, called from `step()`
  every `REP_RECOVERY_INTERVAL` (24) ticks by `REP_RECOVERY_STEP` (8). Stop
  antagonizing a faction and the grudge heals slowly (~3000 ticks from −1000);
  keep raiding every tick and you outrun the drift, so the price is still real
  (the existing automation rep tests — which raid continuously — stay green).
  Core test `hostility_recovers_once_the_raiding_stops`; the per-persona
  reputation finding drops Concern → Note (recoverable dial).

- **2026-06-15 — QA finding #4 fixed: combat is reachable from the live loop.**
  `sim::combat` had no trigger on `Sim`, so commissioned warships never fought —
  only the shipyard's `demo_duel` exercised the resolver. New verb
  `Sim::engage_raiders(band)`: clones the corp fleet's loadouts, generates a
  matched raider pack, resolves via `combat::resolve`, applies losses
  (`Corp::lose_ships_to`), counts a win as an operation, and emits a new
  `Event::BattleResolved { won, losses }` the alert feed voices (§9/§19). Bound
  to the shell (`TorchSim::engage`). New QA **Warlord** persona builds a squadron
  and throws it at raiders; the harness tallies `battles_fought/won` and the
  `design_review` combat finding flips Concern → Good. Two follow-on findings the
  harness then surfaced: (a) setup-time ops were climbing the spine *before* the
  baseline tier was sampled (fixed: `note_ascent` now baselines pre-setup), and
  (b) the matched mirror is **lopsided** — the player held the field in 0% of
  engagements, flagged as a combat-balance Note for a later pass.

- **2026-06-15 — QA finding #3 fixed: instant trade has a cost + a wealth sink.**
  Manual buy/sell was instant, riskless, and free, so it was a constant faucet
  that dominated the transit-paying route. Two §5 sinks: (1) a **brokerage fee**
  (`Sim::TRADE_FEE_BP`, 3%/leg) prices the instant verb's liquidity — sub-fee
  spreads now lose money, so hand-trading is a decision (the QA Arbitrageur skips
  them); the standing route avoids the fee (it pays transit instead). (2) a
  **wealth-scaled overhead** (`charge_upkeep`: a fraction of treasury above a
  100k free float, skimmed each tick) caps runaway hoarding — every income
  strategy now settles at a sustainable equilibrium (~245k for the high-income
  styles) instead of compounding. The free float keeps early/mid play and the
  route/refinery profit tests untaxed. Combined with #28 (routing climbs the
  spine), hand-trading and routing are now complementary, not strictly ordered.
  Core tests `instant_trades_pay_a_brokerage_fee`, `overhead_caps_runaway_hoarding`;
  both economy `design_review` findings flip to Good.

- **2026-06-15 — QA finding #2 fixed: the spine listens to more than raiding.**
  `record_op` was only reachable via interdiction, so the whole build/trade/route
  side of the influence model never advanced the §0 climb. Extracted
  `Sim::complete_op` (campaign `record_op` + CEO XP + research points + ascent
  fanfare) and now call it from every substantive player act: a cut, a
  commissioned ship/freighter, a founded station, and each completed standing
  route delivery. A hands-off Logistician now climbs to Sol on routing alone;
  pure manual teleport-trade still doesn't climb (by design — it's the degenerate
  verb, nerfed separately). Core test `building_and_routing_advance_the_spine_too`;
  the QA `design_review` spine finding flips Concern → Good.

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

- **2026-06-15 — Closed the alert→verb loop (QA finding).** The gameplay-QA review
  flagged the same gap in every persona: act-now shortage alerts were *raised* but
  never *answered* — "no one-press path from the alert to the trade," because the
  only path needed the scarce cargo already on hand. Fix: `Sim::exploit_shortage`
  (and `answer_top_shortage`) source the scarce good at the cheapest *other* market
  and sell it into the short market in one call — speculate/exploit (§3.3/§0.4),
  no pre-held cargo. It resolves the matching feed alert (`AlertFeed::resolve_shortage`).
  Wired the Tycoon persona to use it (130/130 answered, the review flips to the
  GOOD "closed the loop" branch) and bound it to the shell (E). Regenerate
  `docs/SAMPLE_GAMEPLAY_REVIEW.md` after gameplay changes — its first line is a
  hand-added "do not hand-edit" header outside `render_report`, so restore it.

- **2026-06-15 — Combat initiative — the resolver needed variance (QA finding).**
  The gameplay QA flagged matched fights as lopsided (0% then 100% wins). Root
  cause: `combat::resolve` was a deterministic **force-ratio curbstomp** — one
  extra ship or a 1-point crew edge flipped it 100%↔0%, and the ±12% volley jitter
  never changed a winner (focus-fire + equal hp ⇒ matched fleets mutually
  annihilate to a *draw*, never a win). The structural bug: frigates have no
  railgun, so at Medium their 2-tube salvos can't beat a PDC screen and there's no
  damage path at all → guaranteed stalemate. Fixes: (1) **initiative** — at battle
  start one side (rng) wins the opening exchange (+60% tick-1 damage); enough to
  decide an even fight, far too little to overturn a real force advantage, so
  matched fleets are now a genuine coin-flip (proven: 10–90% wins over 64 seeds).
  (2) frigate fleets **knife-fight Close** (where the PDC brawl resolves), not
  Medium. (3) The QA lopsided heuristic only judges off `battles >= 12` — combat is
  crew-capped (§8c) and decisive (§13), so a persona fights a *few pivotal*
  battles, not a grind; balance is proven by the unit test, not the small sample.

- **2026-06-15 — Auto-pause-on-exception + the agency reframe (QA finding).** The
  QA flagged low action density ("long stretches with nothing to press"; the
  GDD's §36 top risk). Two parts: (1) the **shell** now fast-forwards dead time but
  auto-pauses the instant a fresh act-now shortage fires (`TorchSim::just_alerted`,
  set by scanning `step()`'s events for `Scarcity`; shell breaks the step loop and
  zeroes the clock; toggle Y). So the player compresses the quiet and is stopped
  only at decisions (§28/§0.4). (2) The **harness/review** now measure `busy_ticks`
  (an act-now alert pending) and `longest_idle_run` (consecutive ticks with nothing
  pending + no action); the agency finding flips Note→Good when the idle run is
  short (≤120t) — dead time is fast-forwardable, not a pacing gap. Nice emergent
  signal: answering shortages keeps the feed clean (Tycoon 130 pending vs ~3929
  for passive styles, since unanswered scarcity alerts linger in the ring buffer).

- **2026-06-15 — Act-now alerts expire (§19 hygiene).** The pacing metric exposed
  ~3900 "ticks pending" — unanswered scarcity alerts lingered in the ring buffer
  forever, the exact "notification anxiety" §19 warns against. Since §7b shortages
  are *temporary*, `AlertFeed::ingest` now prunes act-now alerts older than
  `ACT_NOW_TTL` (72 ticks) each tick (FYI alerts persist, ring-bounded). The feed is
  a live list of current exceptions now, not a backlog. (busy_ticks stays high
  because the world genuinely fires fresh shortages constantly — that's healthy,
  not stale.) Also loosened the QA agency idle threshold to 240 ticks (~10 s at
  24×): a quiet stretch that short is fast-forwardable + the §21 "felt vastness" of
  a burn, not a pacing dead-zone — so the Warlord's 144-tick gap reads Good.

- **2026-06-15 — Faction contracts (§3.3/§16) — structured income + the rep-repair
  path.** `sim::contracts` adds a job board: a faction posts a **delivery**
  contract (bring `qty` of a commodity to its market) for a premium reward
  (`CONTRACT_PREMIUM_BP` = 130% of face value) and a standing bump
  (`CONTRACT_REP` = 60). The player `accept_contract`s (it then no longer lapses)
  and `fulfill_contract`s from the warehouse — consuming the owed cargo, landing
  it, banking the reward, lifting the faction's standing (§10), and counting the
  delivery as an op on the §0 climb. This ties three systems the influence model
  wants joined: the economy (you must *source* the goods), reputation (a
  contract gives +60 vs. an interdiction's −50, so it's the deliberate repair
  path the §10 "recoverable dial" needs a *verb* for), and the spine. **Key
  determinism move:** `ContractBoard` carries its **own** `Pcg32`
  (`seed ^ 0xC011_7AC7`) so generating offers never advances the shared world
  RNG — proven by `the_contract_board_does_not_perturb_the_economy` (a world that
  reads the board every tick stays bit-identical to one that doesn't) and by the
  QA `SAMPLE_GAMEPLAY_REVIEW.md` regenerating unchanged (personas don't touch
  contracts). Board hygiene mirrors the §19 alert lesson: a small capped menu
  (`MAX_CONTRACTS` = 4), unaccepted offers lapse after a `CONTRACT_WINDOW` (168t)
  delivery window, accepted ones persist (you still owe it). Bound to the shell
  (K accept / J fill-from-warehouse + a deck line). `fulfill_ready_contract` is
  the one-press accept-and-deliver for a contract whose cargo is already on hand.

- **2026-06-15 — Hot-reloadable commodity data (§31) — closes the §4 economy
  block.** The last open economy sub-item: numbers in data, logic in Rust. Chose
  a **tuning-overlay**, not a fully data-defined set — the commodity *identity*
  (names as `&'static str`) and *order* are load-bearing (recipe indices: `RAW =
  [0,1,2]`, industry output = input + RAW_COUNT), so they stay code-defined;
  `data/commodities.json` supplies only the six per-commodity numbers, matched by
  name. This sidesteps the `&'static str` ripple (no `Box::leak` on reload) and is
  the realistic dev loop anyway (tweak prices → reload → watch). `economy`:
  `CommodityTuning` (serde), `parse_tuning`/`apply_tuning` (partial overlay,
  unknown-name = error for typo protection), `tuned_commodities`, and
  `Market::retune` (swap defs on a live market, re-clamp stock/setpoints into the
  new walls, reprice — **touches no RNG**, so a mid-run reload stays
  deterministic). `Sim::reload_commodities(json)` parses *before* mutating, so a
  bad file leaves markets untouched. **Sync guarantee:** `DEFAULT_COMMODITY_JSON`
  is `include_str!`'d and `data_file_matches_compiled_defaults` asserts it
  reproduces `default_commodities()` exactly — the file and code can't drift.
  Bound to the shell as `reload_commodity_data(path) -> ""|error`. Default `Sim`
  still uses compiled defaults, so the §7c gate and QA review are untouched
  (review body byte-identical). **Dep note:** picked **JSON** (`serde`/`serde_json`,
  already in the locked tree via gdext) over RON to avoid a new fetch; §31 says
  "JSON/RON", so JSON satisfies it. `itoa` (serde_json dep) wasn't pre-cached, so
  this needs a network-enabled environment for the first build.

- **2026-06-15 — Pressure, tension & pacing (§13) — `sim::pressure`.** The §35
  build-order item #13: turn ambient predation into a *calibrated* pressure layer.
  `PressureSystem` owns three decaying gauges (FactionWar/Piracy/Scarcity), the
  raid schedule, and the two mechanics §13 names as the stress-vs-tension dial:
  (1) **forecasting** — an incoming raid is telegraphed `FORECAST_LEAD` (18t) ahead
  as `Event::ThreatForecast`, so nothing arrives unforeseeable (the feed voices it
  as a Warning/FYI heads-up); (2) a **pacing governor** — a raid never lands within
  `PACING_COOLDOWN` (24t) of another flashpoint (e.g. a fresh scarcity), and a
  due-but-blocked raid is *deferred, not skipped*. Gauges ebb 1/tick
  (biting-but-recoverable) so a quiet stretch heals while a sustained assault
  outruns the drift. An independent **`Intensity`** knob (Calm/Normal/Harsh) scales
  raid cadence + gauge gains — §13's difficulty setting that does *not* rubber-band
  earned power. **Integration:** the old `pirate_raid` hard-coded a 72t interval;
  that's gone — `run_pressure()` now telegraphs + governs the same raider resolve.
  Normal intensity keeps the 72t cadence, so default play and the §7c gate are
  unchanged; `pirates_raid_the_lanes`/`pirate_raids_do_not_blame_the_player` stay
  green (the governor only defers when the *player* causes a scarcity near a raid —
  ambient cuts are 72t apart, well clear of the 24t cooldown). Pure/integer, draws
  no RNG (`the_schedule_is_deterministic`). Bound to the shell: a pressure HUD line
  + **U** to cycle intensity. **QA:** new `forecasts` tally + a `Pressure`
  design-review finding (GOOD: "raids were telegraphed N times"). *Lesson:* the
  harness's `haulers_interdicted` folds in the player's *own* cuts, so a
  forecasts-vs-cuts comparison falsely flagged the Privateer — the finding reports
  the telegraph count, not a ratio. Sample review regenerated (raid timing shifts
  slightly under the governor; all findings still Good).

- **2026-06-15 — Per-tier content (§0.3) — tiers play differently, not just
  bigger.** Closed the open half of #12. Two mechanical per-tier differences on top
  of the existing spine model: (1) `Tier::briefing()` — a distinct "this is now a
  different *kind* of game" reframe voiced on each ascent (Station = survival puzzle
  → Region = logistics network + first predators → Sol = geopolitics/earn dominance
  → Gate = the larger game), shown persistently in the destination panel; (2)
  **scope that widens as you climb** — `Tier::station_cap()`/`route_cap()` grow
  Station(4/4)→Region(6/6)→Sol(8/8)→Gate(12/8), so "Region = extended
  infrastructure" (§0.3) is mechanical, not flavor. **Key call:** caps only ever
  *increase* at higher tiers, so Tier-1 behavior (and the §7c gate) is unchanged —
  no regression. Nice emergent interaction the test surfaced: founding a station is
  itself a spine op, so building infrastructure *climbs* you and unlocks *more*
  infrastructure — `refineries_are_guarded` was rewritten from a fixed cap-of-4
  assertion to a robust invariant (`len <= tier cap`, a guard always eventually
  fires) since founding now ascends mid-loop. Caps read off `self.campaign.tier()`
  in `found_refinery`/`set_trade_route` (the old `MAX_STATIONS`/`MAX_ROUTES` consts
  removed). Bound to the shell (`tier_briefing`/`station_cap`/`route_cap` + HUD
  lines). QA review byte-identical (personas don't reach the old caps). The full
  "each tier a wholly new game" (Tier-4 procedural systems) stays post-MVP (#15).

- **2026-06-15 — Ship identity & the Rocinante effect (§14/§11).** Closed the
  named-crew-attachment depth #8 deferred. `OwnedShip` now carries a **christened
  call-sign** (`ships::christen_ship`, a 16-name evocative pool, deterministic §27)
  + class, and an accruing **service history** — `commissioned_tick` (age),
  `battles`, `battles_won`, `is_veteran()`. The §13 stakes are now *felt*: losing a
  blooded hull is a real, named loss. **Mechanical Rocinante effect:**
  `Corp::resolve_engagement(survivors, won)` sorts the fleet veterans-first (wins →
  battles → seniority) so the **most-storied hulls pull through** and the green
  ships die, then bloods every survivor; it returns the lost hulls' names so the
  feed can mourn them. `Corp::flagship()` is the most-decorated hull for the shell
  to spotlight. Replaced the old count-only `lose_ships_to` (removed). **Self-
  contained** — touches only `ships`/`corp`/`world` (commission + `engage_raiders`),
  no event/alert/QA-harness churn, so it doesn't tangle with the other open PRs.
  Bound: `ship_name/age/battles/battles_won` + `flagship_name` + a fleet-roster HUD
  line. *Note:* sorting the persistent fleet veterans-first realigns combat RNG for
  a persona's *later* engagements, so the QA sample shifted (Warlord 3→2 battles) —
  benign variance (the 64-seed combat-balance test holds, no new CONCERNs);
  regenerated `SAMPLE_GAMEPLAY_REVIEW.md`.

- **2026-06-15 — Wreck-salvage discovery seed (§15) — `sim::salvage`.** The MVP
  "Discovery & Wonder" pillar (§35.1): derelicts drift in, the player strips them
  for **scrap → credits**, **data → research points**, or the prize — a
  **reverse-engineered blueprint** (`Blueprints::reverse_engineer`, *no* rep gate,
  since you recovered it rather than bought it). So discovery feeds both wallet and
  curiosity, and a salvage counts as an op on the §0 climb. `SalvageField` carries
  its **own** `Pcg32` (`seed ^ SALT`, the contract-board pattern), so sighting
  wrecks never advances the world economy RNG — proven by
  `salvage_discovers_wrecks_without_perturbing_the_economy` (a world that strips
  every wreck keeps bit-identical *markets* to a control) and by the QA
  `SAMPLE_GAMEPLAY_REVIEW.md` regenerating **byte-identical** (personas don't
  salvage). Events `WreckSighted`/`WreckSalvaged` are voiced **FYI** (a discovery
  to pursue when you choose, not an act-now demand — so they add no §19
  notification anxiety). Bounded menu (`MAX_WRECKS` = 3, `SPAWN_INTERVAL` = 96t).
  Bound to the shell: a wrecks HUD line + **H** to salvage the nearest. The full
  §15 (boarding, anomalies/lore, outer-frontier excursions) is the post-MVP arc.

- **2026-06-15 — Persistence (§30) — `sim::persist`, a determinism-native save.**
  Save/load that leans on §27/§31: the sim is deterministic from a **seed**, and
  *content* lives in code (catalogs/orbits), so a save stores only **seed + tick +
  mutable run state** — never the static catalogs (which dodge the `&'static str`
  serde wall entirely). `SaveState` (serde→JSON) captures the corp (treasury,
  warehouse, fleet *by class + crew quality + service history*, crew, freighters),
  standings, campaign, progression (dynamic flag-vectors + CEO), standing orders
  (routes/stations), automation policy, difficulty/alert-threshold, and every
  market's stock+price pair. **Load** = `Sim::new(seed)` → re-sim the ambient layer
  to the saved tick (so traffic/pressure/salvage *phase* lines up; player
  automation is off in a fresh sim, so these steps add no player state) → overlay
  the saved state. The fleet's loadout is rebuilt from class via
  `reference_loadout_quality` (content is code). **Key fidelity move:** prices are
  *damped* (not a pure function of stock), so the save stores both stock *and*
  price and `Market::restore_stocks` overwrites both — otherwise a reload would
  snap prices and drift. The round-trip is proven exact by comparing
  `a.to_save() == b.to_save()` over a full save↔load (the `SaveState` *is* the
  contract), plus a version-mismatch/bad-JSON rejection test. Added small
  `restore`/`flags`/`warehouse` accessors to Corp/Research/Blueprints/Ceo/Market
  and serde derives to the plain data types (Campaign/Relations/AutomationPolicy/
  TradeRoute/Station/Intensity/Priority/Branch/ShipClass/Faction/Interceptor) — no
  serde on content types. Bound: `save_game(path)`/`load_game(path)` → ""|error
  (file I/O in the binding, not the core). QA review byte-identical (personas don't
  save). **Headless-verified the shell end-to-end:** `godot --headless` loads the
  gdext lib + runs 90 frames with no script errors (the §35 headless-first gate now
  covers the Godot layer too, not just `cargo test`).

- **2026-06-15 — UX/legibility pass (§18–§20).** Shell polish alongside §30: panel
  **backdrops** (a dim rect + edge behind the left info column so text never fights
  the orrery), an always-visible **ring-gate arc** that fills with
  `gate_progress_pct` (the §0.1 destination, now drawn not just printed), a **PAUSED**
  banner (§28 clarity), a **selection reticle** on the hauler you'd interdict, and
  **F5/F9** save/load with status feedback. Audio is **deferred indefinitely** (the
  player plays without sound) — the only roadmap item we're consciously dropping
  from the §23 "juice & audio" pass; the juice/3D-orrery half remains open.

- **2026-06-15 — Shell interaction + first juice (§21/§23).** Direct manipulation
  on top of the keyboard verbs: **click an in-flight hauler** in the orrery to
  target it for interdiction (`_pick_hauler` reuses the shared `_orrery_pos`
  world→screen map so render + picking can't disagree), and an **act-now flash** —
  a fading red frame (`flash` decays in `_process`, drawn in `_draw`) that pulls the
  eye to a fresh decision whether or not auto-pause is on. Both headless-verified
  (`godot --headless` runs clean). The first visual *juice* (§23 minus audio);
  pointer-driven market/CEO selection and the 3D orrery are the next UX rungs.

- **2026-06-15 — Orrery as control surface + ascent fanfare (§21/§0.3).** Extended
  click-picking: a left-click tries a hauler first, then falls back to selecting a
  **market by its body** (new `market_body(m)` binding → `_pick_market`), so the
  trade cursor is now pointer-driven, not just `←/→`. Added a **gold tier-ascension
  flash** — the shell watches `tier_name()` across frames and fires `ascend_flash`
  (a warm gold frame, ~1 s) the moment you climb, the visual half of the §0.3
  fanfare the feed already voices. Headless-verified. The orrery is becoming the
  map-half of the §-influence-model "map + master-tables" control; the 3D orrery
  and a richer master-table panel are the remaining UX rungs.

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
