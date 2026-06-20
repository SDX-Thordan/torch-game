# CLAUDE.md — TORCH project working notes

Durable memory for building **TORCH** (see `TORCH_Unified_Design_Document2.md`,
the authoritative GDD). Read at the start of every session; update whenever a
decision is made or a lesson is learned.

**Companion authorities (in `docs/`):**
- `docs/STATE_OF_THE_GAME.md` — **the single live backlog + status** (consolidated
  2026-06-18). Start here for *what's left*: the prioritized open work (P1 mid/late-game
  arc → P2 diplomacy → P3 art → P4 polish) + the explicit **non-goals**. The other plan
  docs below are completed archives; §7 is the distilled lessons (full history is in git).
- `TORCH_Unified_Design_Document2.md` (root) — the authoritative GDD. **Part VI
  (2026-06-17)** documents the empire-sim re-aim + everything built since.
- `docs/EMPIRE_LAYER_PLAN.md` / `EMPIRE_PHASE2_PLAN.md` / `EMPIRE_DIPLOMACY_PLAN.md` /
  `POST_GATE_PLAN.md` — the sequenced empire/endgame roadmaps (E1–E8, EP1–EP4, G1–G5),
  all ✅ done; the live record of what shipped and why.
- `docs/MID_LATE_GAME_STORY.md` — **design notes (not yet built)** for the mid/late-game
  authored arc: the protomolecule mystery time-gating into the Earth/Mars war + OPA
  uprising (Act II), then the post-gate collapse — population flight, the EMC gate
  blockade, the Free Navy ⟷ EMC war, and the powers waning into the player's opening
  (Act III). Reuses the war/contest/coalition/transit machinery; sequenced M1–M2/L1–L4.
- `docs/SAMPLE_GAMEPLAY_REVIEW.md` — the QA harness's current output (regenerated each
  run; not hand-edited).
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
- **Update the docs every PR:** (1) **`docs/STATE_OF_THE_GAME.md`** — the live backlog:
  tick the item you shipped, add any new open work, keep the priorities honest; (2)
  **this file** — append a learnings entry for any durable lesson, and tick §6 if a
  build-step moved. The backlog is the source of truth for *what's next*; keep it current.
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

## 3.5 Code standards (how the code reads)

The bar for every change: **clean, readable, maintainable, extendable.** These are the
Java/Python-flavoured SRP/DRY/KISS/SOLID principles, mapped to this stack. They're a *default*,
not dogma — favour the idiom already in the file, and prefer reusing an existing helper over
adding one (search first).

- **SRP / small methods.** One concern per function. **Extract a functional-block comment into
  a named method** — the `// ---- Feature:` block label *becomes* the method name (`Sim::step`
  → `advance_economy`/`run_pressure_phase`; `_build_systems_view` → `_build_context_panel`/…).
  No "monster methods": if a fn needs section headers to navigate, split it.
- **No magic numbers / strings.** Name every bare literal as a `const` **next to the existing
  const block** (the files have a strong named-const culture — extend it). Tuning numbers stay
  **named consts in code** (§31 content-in-code), *not* migrated into data.
- **DRY / KISS.** One helper for a repeated pattern (the body→index lookups, the acquisition
  flows). Route player-attributed triggers through **centralized hooks** (`ripple_reputation`,
  `complete_op`) so one path covers manual + managed. Keep agency un-fiddly: **macro > micro**,
  one verb + escalating cost is the depth (§0.2, §7.10). Don't over-engineer for a 2nd consumer
  that doesn't exist yet.
- **Layering / composition (SOLID).** Logic in `sim` (no `godot`); `lib.rs` is the thin binding
  (the boundary rule above). Rust: a monolith carves into themed `impl Sim` child modules under
  `world/<theme>.rs` (a child sees the parent's private fields — a byte-identical *move*, §7.1).
  GDScript: composition, not inheritance — reusable units are **stateless `static func` Kit
  utilities the host calls** (`UiKit`/`PlanetShaders`/`OrreryKit`); extract only *pure* helpers,
  leave host-coupled code as well-named methods in `main.gd` (§7.4).
- **Concise docs, no narration.** A `///`/`#` doc says the **why** in one tight sentence; never
  a comment that just restates the next line. Functional-block comments are a smell — make them
  method names (above).
- **Meaningful tests (AAA).** Tests live in `#[cfg(test)] mod tests` **co-located** with the
  code (they move when a module splits). Arrange-Act-Assert; **table-driven** loops for inputs
  that share a shape (`for (case, expected) in [...]`, no new deps); **name cryptic args** via
  small builders/consts; reuse setup helpers. Cover the **negative/null/edge** cases (bad index,
  empty fleet, zero-qty, threshold boundaries). Test the deciding helper, not framework noise.
- **The gate.** Every refactor ends **green** (`cargo fmt --all --check` + `clippy -D warnings`
  + `cargo test --all` + GUT) **and QA byte-identical** (§7.1) — a pure refactor must not move
  the gameplay review. GDScript changes also keep the distinct `sim.X(` binding set unchanged
  and are render-verified (§7.5).

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

# GUT view/integration tests (§32) — boots the gdext core headless:
cargo build                                   # the debug cdylib the extension loads
godot --headless --path godot --import        # register the gdextension + GUT class_names
cd godot && godot --headless -s addons/gut/gut_cmdln.gd -gdir=res://test -gexit
```

## 6. Roadmap (GDD §35 build order → PRs)

> **The live, deduplicated backlog of *open* work is `docs/STATE_OF_THE_GAME.md`.** This
> section is the historical **build-order** record (steps 1–15, mostly done). For "what's
> next," read the backlog — not the scattered "Next:/Deferred:" notes in §7 (those are
> history, several since shipped).

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
  *work*, this critiques how the game *plays*. Same seed ⇒ same review. Now a
  **four-lens** tool: `review`/`design_review` (works & balanced?) + `engagement`
  (engaging & fun?) + `ui` (a static affordance audit of the binding ⟷ shell
  wiring — can the player see & reach it all?) + `early` (an explicit early-game
  loop audit — does the *opening* land?).
- [~] **14. Juice & audio pass**, then UX polish. **Playable shell + 3D orrery
  done** (`godot/main.gd`): real-time-with-pause loop (§28), a **3D orrery** (§21:
  lit bodies orbit the sun on the ecliptic, haulers run the lanes, an
  always-visible gate ring brightens with approach), live panels + alert feed
  (§18/§19) on a 2D CanvasLayer overlay, verbs on input + click-to-target/select.
  **Save/load (§30)** (F5/F9) and the first juice (act-now + ascension flashes).
  **Combat command + §22 diorama** in (doctrine knobs + engage verb + played-back
  BattleLog). Audio deferred indefinitely (player choice); deeper console-chrome +
  richer juice (a *voxel* diorama, live mid-fight commands) still to come.
- [~] **15. (Post-MVP)** Tier 3 geopolitics → outer frontier → gate/empire.
  **Post-gate sandbox (G1–G5) complete** (`docs/POST_GATE_PLAN.md`): the `Tier::Beyond`
  tier + `transit_gate` + the gate-mystery *answer* (G1/§0.1), the far-side **place**
  (Erebus/Threshold/The Tally bodies, G1), **economy** (the far-side markets, G2),
  **bridgehead** colonization (G3), escalating **incursions** (G4), and the **win/loss**
  resolution (G5) — a full endgame loop, every rung transit-gated so the inner game
  (and the §7c gate + QA review) stays byte-identical. Remaining: the **art track**
  (A1 procedural assembly/baking, A2 voxel diorama) + deeper Tier-3 geopolitics.

## 7. Learnings & decisions log (distilled)

> **Curated, not append-only.** This was a 2000-line chronological PR log; it's been
> condensed into the durable, reusable lessons below. The blow-by-blow history lives in
> **git** (squash commits) and the **backlog** (`docs/STATE_OF_THE_GAME.md`). Add a new
> bullet only for a *durable* lesson; fold it into the right theme rather than narrating a PR.

### 7.1 Determinism & the byte-identical discipline (the core method)

- **Determinism rules:** integer / fixed-point only, `sim::rng::Pcg32`, no floats in
  probability paths, no wall-clock, fixed tick, content is data (§27).
- **The byte-identical trick:** gate a new feature on **player-only state** — an empty
  `Vec` (`run_miners`/`run_war`/`run_contest`/`run_shipyard_upkeep` early-return as no-ops),
  an **identity-default** value (`DEV_BASE=1` ⇒ ×1; `DevDoctrine::Balanced` = 10000bp;
  tier-0 weapon stats == the old generic weapon; `Doctrine.aggressive_fire=false` skips the
  heat RNG branch; `jitter==0` short-circuits the jitter draw), or a **player-only verb**
  personas never call. Then the §7c economy gate **and** the QA *gameplay* body stay
  byte-identical — only the UI-wiring binding count moves. **Verify via the QA diff.**
- **Don't perturb the shared market RNG:** give an ambient system its **own** `Pcg32`
  (contracts `seed^0xC0117AC7`, salvage `seed^SALT`, far-side `far_rng seed^0xFA5FACE`) or
  make it **rng-free** (the contest). Split a market slice (`[..split]` shared rng /
  `[split..]` far_rng) rather than branching per-market inside one loop.
- **Voice flavour without a new `Event` variant:** `feed.announce(voice, msg, tick)` (used
  for Foundry/R&D/The Inners/The Frontier). A *new* `Event` variant forces the **two QA
  exhaustive matches** (`harness.rs` tally + `event_kind_bit`) + the `pressure`/`alerts`
  matches — fold its variety-bit into an existing bit so `EVENT_KIND_COUNT` is unchanged.
- **When a change *legitimately* moves the world, regenerate QA honestly** (don't chase
  byte-identity): a persona that newly exercises the feature (Warlord under the shipyard
  gate, Responder under war-collateral, Expansionist for the empire), or **adding orbital
  bodies** (the salvage RNG reseeds off `body_count()` → a benign shift, the far-side / belt
  precedent). Confirm "no *new* CONCERNs," then regenerate.
- **Regenerating `SAMPLE_GAMEPLAY_REVIEW.md`:** restore its hand-added do-not-edit header
  line; regenerate **after** the shell edits (its UI-wiring facet scans `main.gd` for
  `sim.X()` calls vs the binding list, so any binding-call change shifts it).
- **Splitting a monolithic file is a byte-identical *move*, not a rewrite** — verify with the
  empty QA diff. A giant `impl Sim` carves into `world/<theme>.rs` child modules (each a fresh
  `impl Sim` block + `use super::*`): a **child sees the parent's private fields, consts, and
  even private `use` bindings**, so the move needs no API changes — only cross-module-called
  private helpers widen to `pub(crate)`. Watch the **name clash**: a `mod combat;` collides
  with `use super::combat::{self,…}` (the `self` binds `combat`) — name the child `defence`.
  The shell's UI facet counts **distinct** `sim.X(` names, so dropping one `sim.foo()` call (e.g.
  to a bare `sim.foo` Callable for a DRY helper) is byte-identical as long as `foo` is still
  called with parens elsewhere.

### 7.2 Persistence (§30)

- Save only **seed + tick + mutable player/economy state**, never the static catalogs
  (content is code). **Load** = `Sim::new(seed)` → re-sim the ambient world to the saved
  tick (phase lines up; player automation is off in a fresh sim, so the re-sim adds no
  player state) → **overlay** the saved state. `#[serde(default)]` every new field so old
  saves load.
- **Only persist player state that the re-sim *doesn't* reproduce.** The contest's ambient
  influence + flare schedule replay deterministically on re-sim, so only `player_influence`
  is saved (a plain `Vec<i64>`). Transient things (pending decisions/incursions) aren't saved.
- **The `&'static str` serde wall:** don't derive `Serialize` on content types carrying
  `&'static str` names (companies, contest). Persist the **mutable dials** as a plain
  `Vec<i64>` and rebuild identity from code (the §31 content-in-code split).
- **Prices are damped, not a pure fn of stock** → store **both** stock and price and
  overwrite both on load, or prices snap and drift. Restore the **arsenal before** rebuilding
  the fleet so a reload never downgrades guns; the fleet is rebuilt by class via
  `reference_loadout_quality`.
- **Format:** bincode shipping save + JSON dev export on the same `SaveState`; `load_bytes`
  auto-detects (`{` ⇒ JSON else bincode). Bincode isn't self-describing, so cross-version
  tolerance is the JSON path's `#[serde(default)]` job.

### 7.3 The dilemma framework (universal macro-decision vehicle)

- Act-now exceptions are **multi-option dilemmas** (`DecisionKind`:
  Shortage/Wreck/RaidThreat/WarCollateral). The shell panel is **generic** over
  `decision_title`/`decision_options`/`resolve_decision`, so a **new `DecisionKind` needs
  zero shell changes** — it's the universal vehicle for "macro decision with stakes" (war
  collateral = one kind + a tick schedule, no UI work). `complete_op` runs centrally in
  `resolve_decision` so answering *any* dilemma climbs the §0 spine. Dilemmas are transient
  (TTL, capped at 3, deduped per kind), not persisted.

### 7.4 GDScript / Godot traps (each was fatal once)

- **`:=` inference fails on a gdext method return and on Variant-returning calls** (`lerp`,
  `Dictionary` access, `abs`) → type the local (`var x: int = sim.foo()`), use
  `lerpf/maxf/maxi/absf/absi`. An **untyped array-literal index** returns Variant → type it
  (`var a: Array = …; int(a[0])`). `node.material_override` is typed `Material` → **cast**
  (`as StandardMaterial3D`), not `: StandardMaterial3D =`.
- **A parse error in a `class_name` script that `main.gd` depends on makes the scene HANG on
  load** (even `--headless`) → diagnose with `godot --headless --path godot --import` (prints
  the real `Parse Error file:line`). Always `--import` after adding a `class_name` (registers
  the global class). A **fresh checkout has no `.godot/extension_list.cfg`** → a bare
  `--headless` can't resolve gdext types → run one import pass first.
- **A new `#[func]` needs a *debug* `cargo build`** (the editor loads `target/debug`), or
  Godot reports "Nonexistent function." Verify a binding end-to-end headless before trusting it.
- `Camera3D.look_at` needs the node **in the tree** (add_child *then* look_at).
  `Viewport.get_texture().get_image()` lags **one frame** → switch-then-wait-N-frames before a
  screenshot. A **`SubViewport` owns its own world** → give it its own lights + WorldEnvironment.
- **Procedural body shaders (the "proper textures" path, no texture pipeline):** under
  `gl_compatibility` the **fragment** stage has no `NODE_POSITION_WORLD` / `CAMERA_POSITION_WORLD`
  ("Unknown identifier") → compute the node-to-sun dir in `vertex()` from `MODEL_MATRIX[3].xyz`
  and pass it as a **varying**; do fresnel rims with view-space `NORMAL`/`VIEW` (both
  fragment-available). Putting **Sol at the world origin** makes day/night a one-liner
  (`-normalize(node_origin)`). At fine star-grid scales a per-cell star is **sub-pixel** (cell
  ≈9px, star <1px) → use bigger cells + a core + 3× halo so it reads (`PlanetShaders.space_sky`).
- **Mobile:** `project.godot` orientation enum **4 = sensor-landscape** (1 = Portrait!).
  Pinch-zoom needs real multitouch tracking (`InputEventScreenTouch`/`Drag`), **not**
  `InputEventMagnifyGesture` (that's a trackpad gesture); keep `emulate_mouse_from_touch` on so
  on-screen buttons + tap still work. Op-buttons default 104px wide → a row of single-char
  cycle buttons overflows; use a tiny 26×26 button for arrows. `-1 << 30` is rejected
  ("only positive operands for `<<`") → use a literal.
- **Map gestures reach `_unhandled_input` only if nothing STOPs them first.** A full-rect
  **layout host** Control defaults to `MOUSE_FILTER_STOP` and silently eats every tap/drag/pinch
  over the 3D map behind it (the interactive child panels are themselves STOP, so they keep
  working) → set the **host to `IGNORE`**. With that, one-finger drag = orbit, two-finger
  pinch = zoom + **twist (`Vector2.angle()` delta) = rotate**; mark a `_was_drag` flag so the
  release isn't read as a tap-to-focus.
- **`content_scale_factor` (the HUD magnify lever for handheld readability) breaks 3D picking.**
  Under `canvas_items` stretch, `_unhandled_input` event positions arrive in **scaled canvas
  space** (physical ÷ factor) while `Camera3D.unproject_position` returns **render pixels** — so
  a tap projected against unproject coords misses by the factor. Convert at the boundary:
  `pick(event.position * content_scale_factor)` (a no-op at 1.0, so PC mode is unaffected).
  **Verify headlessly** by `push_input`-ing a click at a body's unproject position (push_input
  applies the same stretch transform a real finger does) and asserting the focus lands.
- **Drag-to-move a HUD panel:** translate it by shifting **all four offsets** by the drag delta
  (works under any anchor preset, including anchored-right). Make the panel's *content
  container* `IGNORE` so empty regions fall through to the panel's `gui_input` while STOP
  children (toggles) keep working. With `emulate_mouse_from_touch` a touch fires **both**
  `ScreenDrag` *and* an emulated `MouseMotion` → gate by `pc_mode` (touch ⇒ ScreenDrag only,
  PC ⇒ MouseMotion only) or the panel translates twice. Drag deltas are canvas-space like the
  offsets, so — unlike picking — they need **no** content-scale conversion. (`push_input`
  drag tests are unreliable under content-scale; verify the logic in PC mode at 1.0.)
- **Touch-first agency:** prefer **contextual actions** (show only the verbs the tapped body
  affords) over a persistent op-button grid, and make act-now dilemmas a **large centred modal
  that hard-pauses** (`decision_count() >= 1`) behind a STOP scrim — the popup *is* the moment,
  not a corner toast. Pause/play live in the top bar (the handheld's spacebar). Open the camera
  **centred + zoomed on the home station** (focus `market_body(0)`, a tight zoom) — at the
  orrery scale (`1 AU = 1` world unit) a "zoomed-in" zoom is ~2.2, not 4.
- **Gate story removed (placeholder, until the proper arc lands):** the gate mystery was a stand-in;
  it's now fully hidden from the player until `MID_LATE_GAME_STORY.md` is built. `reveal_gate_beat`
  still advances the counter (keeps the machinery + save field + QA `gate_carrot` live) but **voices
  nothing**; the **Mysteries ledger tab** is removed; the **golden ring visual + progress-glow** are
  hidden in the orrery (the gate **sim body stays** — `body_count` is load-bearing for the salvage
  RNG, §7.1); the late tiers are reframed off the gate (**The Gate→The Frontier**, **Beyond the
  Gate→The Far Reaches**). A *persona reaches the Gate tier in the QA sample*, so the tier rename is
  visible → **regenerate QA** (rename-only diff, no new concerns). `GATE_LORE`/`reveal_gate`/
  `transit_gate`/`Tier::Gate|Beyond` all stay dormant for that arc to re-wire.
- **Adding a 6th nav view** means extending **every** `view`-indexed const in lockstep —
  `V_*`, `VIEW_GLYPH`, `VIEW_CAP`, **and `VIEW_TITLE`** (the easy one to miss → an out-of-bounds
  in `_refresh_chrome`). A generic **sortable ledger** = a tab index + `_led_sort`/`_led_asc`, a
  per-tab `columns()`/`rows()` (rows are `Array`s of mixed int/String cells), a type-aware
  comparator (numbers numerically, else lexically) feeding `rows.sort_custom`, rebuilt into a
  `GridContainer` each refresh with clickable header buttons — pure shell over existing bindings.
- **3D picking under `content_scale_factor` needs NO conversion.** `_unhandled_input` event
  positions **and** `Camera3D.unproject_position` are *both* in the viewport's canvas space (Godot
  pre-transforms input by the stretch/screen transform), so picking compares them directly — an
  earlier `event.position * content_scale_factor` mis-scaled **every** click (the bug behind
  "clicking doesn't select"). **`push_input` is unreliable** for verifying this (it skips the
  stretch transform); use the **viewport screen-transform** numbers + **`Input.parse_input_event`**
  (which applies the transform) to confirm. Lesson: don't trust a push_input picking test.
- **Event cadence is a watchability lever.** With "hard-pause on every dilemma," frequent ambient
  events make the game unwatchable. The cadences (raid 72→**300**, wreck 96→**420**, war
  130→**460**+, contest-flare 90→**560**) are tuned for a sim you *let run* — events are an
  occasional beat, not constant chatter. A deliberate gameplay change → regenerate QA (act-now
  29→4 over 4000t, no new concerns).
- **Object-contextual model:** make the *tapped object the centre* — the right panel re-centres
  on `_focus_body` (identity + a detail block) and a single action stack shows **only the verbs
  that body affords**, classified in the refresh by cheap **body→index lookups** in GDScript
  (`_colony_index_for_body` / `_contested_index_for_body` loop the existing `*_body(i)`
  accessors) plus `can_mine_body` / `miner_at` / `can_found_shipyard_at` / `shipyard_body`. New
  verbs slot in by adding a hidden button + one visibility line — no new panels. Belt **Asteroid**
  bodies must be in `can_mine_body` (they're the literal "asteroid-belt sections" to mine).
- **Desktop / PC mode window:** a plain `Window.MODE_WINDOWED` opens a small floating box that a
  tiling WM (niri) won't fill — open **`MODE_MAXIMIZED`** instead (back it with project
  `display/window/size/mode=2` so frame 0 is already maximized), and offer **F11** for true
  `MODE_FULLSCREEN`. Bump `UI_SCALE_PC` a little above 1.0 (1.2) for table legibility on a big
  monitor — the `_to_view` content-scale conversion already makes picking factor-agnostic.
- **PC map controls:** mouse-drag should **pan**, Shift-drag should **rotate** (the touch build
  only had drag=rotate). Pan is a free **`_pan` ecliptic-plane offset** added to the camera focus
  in `_focus_pos()` (cleared on `_pick_body`/`_reset_view` so a click re-centres); convert the
  screen delta along the camera's *flattened* right/forward axes, scaled by `_zoom`, so it tracks
  the cursor at any scale. Mark `_was_drag` past a ~1px threshold so the button-release isn't read
  as a click-to-focus, and reset it on the left-button **press**.
- **Stepped orbit lines / jaggies:** the orrery viewport defaults to **no AA** → enable
  `anti_aliasing/quality/msaa_3d` (4×) + `screen_space_aa` (FXAA) in `project.godot` (the battle
  diorama SubViewport opts out on its own, so it's unaffected). A large `TorusMesh` orbit ring also
  reads as a polygon at the default 64 `rings` — scale `tm.rings` with radius (`clampi(r*48,96,384)`)
  and drop `ring_segments` to ~6 (the tube is a hairline) for a smooth circle that stays cheap.
- **Space-sky tuning:** untethered nebula `fbm` blobs read as ugly "blurry balls" — confine the
  coloured nebulosity to the **Milky-Way `bandmask`** (so it's galactic dust, not floating balls),
  darken the base (`~0.005`), and tighten the band (`smoothstep(0.55,1.0,…)^2.2`, intensity ~0.6).
  Let the multi-scale star layers carry the look.
- **Composition idiom = stateless utilities, not host-ref components.** The shell's reusable
  units (`UiKit`, `PlanetShaders`, `MiniChartS`, `FlowGraphS`, `OrreryKit`) are all
  `class_name` scripts of pure `static func` factories the host *calls* — preloaded as a const
  (`const OrreryKit := preload("res://ui/orrery_kit.gd")`), intra-class siblings called bare
  (`ring` calls `emissive_mat(...)`). Extract a `main.gd` method **only when it's pure** (args +
  consts in, node out, no `_member`/`sim` access): those lift cleanly into a Kit and the call
  sites become `Kit.foo(`. The *host-coupled* code (3D build/refresh over `_body_nodes`/`_cam`,
  the gesture/picking controller, per-view builders that assign panel members) is **not** a
  clean component — wrapping it in a host-ref object just relocates the coupling and risks the
  §7.4 picking/gesture traps. Leave it as well-named SRP methods in `main.gd`; only split when a
  second consumer actually appears. Verify a Kit extraction by diffing the distinct `sim.X(` set
  (unchanged ⇒ QA byte-identical) + an `--import` (fail-fast on any missed rename) + render.

### 7.5 Render-verify workflow

- The env has xvfb + software Mesa GL, so the UI can be **captured and looked at** — a
  visual/3D change isn't "done" at headless-parse:
  `LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a -s "-screen 0 1280x720x24" godot --path godot
  --rendering-method gl_compatibility --rendering-driver opengl3` (must override the project's
  mobile/Vulkan renderer to gl_compatibility+opengl3 on llvmpipe). Capture via a temp
  `_process` hook (`get_viewport().get_texture().get_image().save_png(path)` then quit; revert
  the hook). `pip install Pillow` to crop/zoom dense panels. Software GL is too slow to route
  ships across the system in-frame — rely on unit tests for those paths.
- **The cloud/web env CAN run Godot — install it; don't assume "no runtime."** The container
  ships no `godot` binary, but egress to the GitHub releases CDN works: download
  `Godot_v4.6.3-stable_linux.x86_64.zip` (match the 4.6.3 pin), unzip, symlink onto PATH. With
  that, the full GDScript loop is a *real gate*, not advisory: `cargo build` (debug cdylib the
  extension loads) → `godot --headless --path godot --import` (registers gdext + `class_name`s;
  a fresh checkout has none) → GUT (`-gexit` fails non-zero) → the §7.5 xvfb render-verify. To
  shoot a non-default view, instantiate `main.tscn` under a `SceneTree` script and call
  `inst._select_view(idx)` a few frames before the capture. Confirm a `main.gd` refactor didn't
  move the QA UI facet by diffing the **distinct `sim.X(` name set** (was 292) — unchanged ⇒ the
  QA review regenerates byte-identical.
- **GUT integration tests silently rot when nobody runs Godot.** They drive the real gdext
  binding, so a *sim* behaviour change (warships now go through a timed shipyard build queue —
  `commission_ship` lays a hull down, it stands up `commission_build_ticks` later, not
  instantly) breaks the GUT assertions while `cargo test` stays green. Fix by stepping the sim
  through the build via the exposed `pending_ship_count()` binding (the `finish_pending_ships()`
  test helper is `pub(crate)`, not a `#[func]`) — which also makes the test exercise
  `run_shipyard_builds` end-to-end. **Run GUT after any binding/sim change**, not just parse.

### 7.6 Build / CI / tooling

- **Pin `rust-toolchain.toml`** to an exact version (rustfmt output isn't stable across
  versions; CI must == local). **gdext 0.2.x tops out at `api-4-3`**, forward-compatible → ship
  on Godot **4.6.3** with `compatibility_minimum=4.3` (building against a *newer* API than the
  runtime panics). `crate-type=["cdylib","rlib"]` (the rlib lets pure `sim` modules be
  cargo-tested without Godot).
- **Clippy under `-D warnings`:** crate-level `#![allow(clippy::result_large_err)]` (gdext
  macros expand to large `CallError` Results); `#[allow(clippy::too_many_arguments)]` on 8+ arg
  fns; rust-1.94 lints — `derivable_impls` (derive `Default` + `#[default]`),
  `manual_is_multiple_of` (`x%2==0` → `x.is_multiple_of(2)`), `dead_code`.
- **Android APK** (`android.yml`, `barichello/godot-ci:4.6.3` + cargo-ndk): the *headless*
  export fails with an **empty** "configuration errors:" unless — `import_etc2_astc=true`
  (ETC2/ASTC mandatory, the silent final blocker); editor-settings filename
  `editor_settings-4.6.tres`; `use_gradle_build=true` + `--install-android-build-template`;
  build-tools match `target_sdk=34`; `HOME=/github/home`; stage the host `libtorch_core.so`
  (`cargo build`) + the arm64 lib at `godot/bin/android/arm64/`.
- **Updatable releases (install *over* the prior version):** two non-obvious requirements.
  **(1) Stable signing key** — Android rejects an update signed with a *different* key, so the CI
  must **not** regenerate the keystore each run; the repo ships a committed stable
  `godot/debug.keystore` (the preset references it), overridable by an `ANDROID_KEYSTORE_BASE64`
  secret for a private/Play key. **(2) Increasing `versionCode`** — a build is only seen as
  *newer* when its `version/code` is higher; the tag job stamps `version/code` + `version/name`
  into `export_presets.cfg` from the `vMAJOR.MINOR.PATCH` tag (`code = MA*10000+MI*100+PA`). Saves
  already survive updates via the §7.2 `#[serde(default)]` discipline.
- **GUT view tests:** pin **≥9.4.0** (9.3.0's `Logger` shadows Godot 4.6's native `Logger` →
  the addon won't compile). One `--import` pass on a fresh checkout registers GUT's
  class_names; headless GUT needs no xvfb; `gut_cmdln … -gexit` exits non-zero on failure (a
  real gate). A gated verb stays testable via a private-field helper (`yard()`) + a sandbox
  binding (`dev_grant_shipyard()`).

### 7.7 Economy & balance tuning

- **Pricing anchor must be piecewise** so `stock==target ⇒ basePrice` (not a band-midpoint).
  Trade spreads come from **decoupling the stabilizer setpoint from the price anchor**
  (`target_stock`); a *stiff* spring (20%/tick) neutralizes trade, so make it **gentle**
  (4%/tick) and rely on **hard stock walls** `[0, max]` for the no-death-spiral guarantee.
- `best_route` must consider **all ordered market pairs** (never hard-code `(0,1)/(1,0)`) —
  generalize the moment a 2nd instance appears.
- **Sinks:** a brokerage fee (3%/leg) prices instant trade; a wealth-scaled overhead above a
  100k free float caps hoarding. **Watch absolute spread × qty, not just relative** — high
  *absolute*-value upper-tier goods get **administered prices** (neutral setpoint, jitter 0,
  which also short-circuits the RNG draw so the lower-tier stream stays byte-identical).
- Frontier hubs sit in scarcity (×0.7) to **pull** supply without out-bidding inner spreads;
  outer hauls need `MAX_HAULERS=16` + `CRUISE_SPEED=60k`. The fattest-spread router skews NPC
  *destinations* (fine for the player-facing economy; a distance-aware dispatcher is the fix if
  it ever matters).

### 7.8 Combat tuning

- **The opening salvo must land tick 1** (init reload 0) or the capital shreds the wing before
  torpedoes fly. Combat is **decisive/short** (2–3 ticks). **Matched fights need variance:** a
  deterministic force-ratio is a curbstomp (0%↔100%) — add **initiative** (one side wins the
  opening exchange, +60% tick-1 dmg) → matched fleets become a coin-flip (10–90% / 64 seeds),
  too little to overturn a real advantage. **Frigates have no railgun** → they must knife-fight
  **Close** (the PDC brawl), not Medium (a guaranteed stalemate). Saturation: `leakers = salvo −
  screen×band`. Heat is **front-loaded upside** that only vents in *prolonged* fights
  (`aggressive_fire` default false skips the branch). A Battleship needs 120 crew vs the 60
  starting pool → "stand up a squadron" tests must commission **Frigates** (12 crew ⇒ five fit).

### 7.9 QA harness

- **Observe campaign state directly** (poll `tier()` each tick) for player-caused changes, not
  the event stream — the engine bug it exposed (a between-tick player verb's events wiped by the
  next `step()`'s `events.clear()`) is fixed: `step()` now drains only the leading `returned`
  events, keeping the player tail. A test must **find its decision by kind**
  (`position(|d| d.kind==…)`), not assume index 0, and drive the real auto-raise path.
  `super::pressure::X` doesn't resolve inside `mod tests` → use `crate::sim::pressure::X`.
- **Three lenses:** `review`/`design_review` (works & balanced?), `engagement` (fun? — 6
  structural proxies; it can't feel delight, it scores "where is fun at *risk*"), `ui` (a static
  binding⟷shell audit: phantom calls, unreached capability, exception→verb, status visibility).
  The lopsided-combat heuristic only judges off `battles>=12` (combat is crew-capped/decisive →
  balance is proven by the unit test, not the small sample).

### 7.10 Design philosophy & architecture

- **The destination pull (§0) is the over-invest priority;** keep the spine attributable to the
  **player** (reuse the player path, not ambient events). **macro > micro** (player steer):
  fleet *loadouts* not mid-fight commands; an empire-wide development *doctrine* not per-colony
  policy; standing-order **policy → execute → exception**. Keep it un-fiddly (§0.2): one verb +
  escalating cost is the depth.
- **Additive, not gating:** add the new path *beside* the old verb (`assemble_ship` alongside
  `commission_ship`) when it mustn't perturb established balance. Route player-attributed
  triggers through **centralized hooks** (`ripple_reputation` for any cut, `complete_op` for any
  ascent) so one hook covers manual + managed.
- **Boundary rule:** all game logic in `sim` (no `godot` imports); `lib.rs` is the thin binding.
  `Nav`/`Doctrine`/`TradeRoute`/`Station` are `Copy` → copy out of `self.corp.fleet()` **before**
  the `fleet_mut()`/`debit()` mutation to dodge the borrow checker (disjoint two-field borrows —
  catalog immutable + rng mutable — compile cleanly).
- **Art identity:** Earth = navy-blue + yellow hazard banding (its signature); each power a
  distinct hull *grammar* + palette. Render at a **common fixed scale** to compare sizes (a
  per-subject normalized render hides the size axis). Primitives give "believable industrial
  silhouette + livery," not voxel detail — the §25 offline tool is the true-voxel path.
- **Stack pivot (2026-06-14):** from a TS prototype (Vite/Canvas + Capacitor, archived on
  `prototype/ts`) to **Godot 4.x + Rust (gdext)** per GDD §26. The prototype validated the
  economy/orbit/§7b design that ported directly to the Rust core.

### 7.11 Carried-over design learnings from the TS prototype (still authoritative)

- **Economy pricing anchor.** Price target must be piecewise so `stock == target ⇒ basePrice`,
  sliding to ceiling under scarcity / floor under glut — not a band-midpoint map. Otherwise
  settled prices ignore each commodity's reference.
- **Market self-sufficiency vs. emergent trade.** Making every market self-sufficient (base
  production ≈ 1.1× full demand) passes the stability sweep but *suppresses* deficit-driven
  trade. Drive §7b haulers by **price arbitrage** (cheapest surplus → dearest with room)
  instead; equilibrium prices differ between markets, so trade flows and *damps* the spread.
  Comparative-advantage specialization would deepen trade but needs a fresh stability check.
- **Stability test performance.** Assert invariants via plain boolean accumulation in the hot
  loop, once at the end — not per-tick `assert` calls.
