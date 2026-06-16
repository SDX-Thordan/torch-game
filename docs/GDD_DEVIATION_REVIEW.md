# TORCH — GDD Deviation Review

**Date:** 2026-06-16
**Reviewer pass:** full audit of the implementation against
`TORCH_Unified_Design_Document2.md` (the authoritative GDD).
**Scope:** the Rust sim core (`crates/torch-core/src/sim/`), the QA harness
(`crates/torch-qa/`), and the Godot shell (`godot/`), cross-checked against the
GDD section by section.

> **Purpose.** State *explicitly* where the build has deviated from the GDD —
> intentional simplifications, format/UX divergences, MVP gaps not yet filled, and
> GDD-sanctioned deferrals — so nothing drifts silently. This is a companion to
> `PLAYABLE_STATE_REVIEW.md`; that doc sequences the path to playable, this one
> tracks **spec fidelity**.

## How to read this

Most of the GDD's *systems* are built and green (131 + 8 native tests; headless
economy-stability gate; QA design-review at 0 concerns). The deviations below are
real but largely **known and tracked** — the value here is making them legible in
one place. Each is tagged:

| Tag | Meaning |
|---|---|
| 🔴 **Pillar-level** | Contradicts a core pillar (§2) or a #1–2 design priority (§0.2). Address deliberately. |
| 🟠 **MVP gap** | Listed "In" for MVP (§34) but not yet built. |
| 🟡 **Simplification** | Implemented, but materially reduced from the spec. Usually intentional. |
| 🟢 **Sanctioned deferral** | GDD explicitly post-MVP / roadmap-staged, or a player-chosen drop. Listed for completeness, not a defect. |

## Summary

| # | Deviation | GDD | Tag |
|---|---|---|---|
| 1 | Delta-v movement / per-ship position — **warships ✅ done** (freighters + combat-positioning follow) | §2 / §6 | 🟠 (was 🔴) |
| 2 | Authored gate-mystery thread + opening missions — **✅ done** (MVP seed) | §0.1 / §16 | 🟢 (was 🔴) |
| 3 | Combat is non-interactive (no live commands, thin doctrine, no diorama) | §9 / §22 | 🟠 |
| 4 | Save slots + Ironman — **✅ done** (3 slots + Ironman autosave) | §13 / §30 | 🟢 (was 🟠) |
| 5 | Expressive identity — **corp name + livery ✅ done** (logo deferred) | §14 | 🟡 (was 🟠) |
| 6 | Persistence is JSON, not binary bincode | §30 | 🟡 (intentional) |
| 7 | Multi-view shell *replaces* the map; not slide-over panels | §18 | 🟡 (follows mockups) |
| 8 | Commodity chain truncated to Raw→Refined (no Components/Assembled) | §7d | 🟡 |
| 9 | Combat omits heat, facing/spinal-vs-turret, retreat/priority doctrine | §8a / §9 | 🟡 |
| 10 | Civilian classes partial (no Courier/Salvager/Survey) | §8e | 🟡 |
| 11 | Crew depth: name + quality only (no portraits/traits/quirks/loyalty/rename) | §11 | 🟡 (right-sized) |
| 12 | Data pipeline: only commodities externalized; specs/factions in code | §31 | 🟡 |
| 13 | Orrery omits trajectory ghosts, range/band rings, two-finger azimuth | §21 | 🟡 |
| 14 | No view interpolation (positions snap per tick) | §28 | 🟡 |
| 15 | No GUT view/integration tests (cargo + headless render only) | §32 | 🟡 |
| 16 | Audio dropped; juice partial | §23 | 🟢 (audio player-chosen) |
| 17 | Voxel art + procedural assembly tool not built (primitive meshes) | §24 / §25 | 🟢 (roadmap #11) |
| 18 | Endgame arc (gate/colonization/empire/incursions) not built | §17 | 🟢 (post-MVP) |

---

## A. Pillar-level deviations (address deliberately)

### 1. 🔴 Delta-v does not govern movement; the player fleet is positionless — §2, §6
- **GDD:** Pillar #2 — "delta-v is the universal constraint." §6 — every move is a
  *committed trajectory* with real travel time + delta-v cost; a per-ship delta-v
  budget (remass × drive efficiency); running dry **strands** you; economical vs.
  hard-burn trajectory choice (verb #4, §0.4).
- **Built:** `ShipStats.delta_v` is computed per fit but consumed **only** for
  combat range/mobility (`sim::combat`) and the shipyard readout. The movement
  layer ignores it: NPC haulers move at a flat `CRUISE_SPEED = 60_000`
  (`world.rs`), with `travel_ticks = distance / CRUISE_SPEED`. Player freighters
  are an abstract **pooled count** (`Corp::freighters: i64`) + an in-transit timer
  on each route; they have **no individual position**. Player warships
  (`OwnedShip`) have **no position at all** and never traverse the map — combat is
  the abstract `engage_raiders(band)` verb.
- **Consequence:** Verb #4 (trajectory problem-solving) does not exist; transfer
  windows, hard-burn-vs-economical, stranding, and refuel-as-strategic-ground are
  all absent. The FLEET view's `location`/`fuel` columns are **synthesized in the
  shell** because the sim has no truth to show.
- **Status — warships ADDRESSED 2026-06-16.** `sim::movement` (`Nav` + `plan`) gives
  every owned **warship** a tracked position + remass budget (`OwnedShip.nav`):
  `Sim::move_ship(idx, dest, hard_burn)` commits a trajectory at the **live orbital
  distance**, spends remass, and takes time derived from the ship's drive and the
  burn (economical vs. hard, verb #4); `Sim::refuel_ship` buys remass at a dock; a
  dry tank **strands** the ship. Ships render on the orrery; the FLEET view shows
  **real** location/fuel; a mobile **SEND FLEET / REFUEL** control dispatches the
  fleet to the focused world. Persistence saves the nav state. **Remaining for full
  Pillar-#2 coverage:** player **freighters** (still pooled-count + route timer) and
  **combat positioning** (`engage_raiders` not yet gated on the fleet being at the
  fight) — tracked as follow-ups, hence re-tagged 🟠.

### 2. 🔴 No authored gate-mystery narrative or opening missions — §0.1, §16
- **GDD:** The destination pull is the **#1 over-invest priority** (§0.2). The gate
  is "a carrot from minute one"; the **gate mystery** is "the one authored thread
  that gets real investment" (§0.1), foreshadowed and advanced across every tier;
  opening missions teach the systems then thin into per-tier objectives (§16).
- **Built:** The *mechanical* spine is solid — `sim::campaign` has tiers, the
  three-horizon goal stack, an always-visible `gate_progress_bp`, voiced ascents,
  and per-tier briefings. But there is **no authored content**: no gate-mystery
  lore, no anomalies seeding it (§15), no opening/tutorial missions, no narrative
  threads. Progress is a pure operations counter.
- **Consequence:** The pull is currently *systemic* (a progress bar to the gate),
  not *narrative* (a mystery you want to chase). The GDD's top-ranked investment is
  the part with the least authored substance.
- **Status — ADDRESSED 2026-06-16 (MVP seed).** `sim::missions` adds the authored
  half: a 5-step **opening-mission** chain that teaches the verbs (First Light →
  Stand Up a Hull → Standing Orders → Cut a Lane → Climb), each firing once the
  player does the thing (hooked into `sell`/`commission_ship`/`set_trade_route`/the
  player-interdict path/`complete_op`); and a **7-beat gate mystery** (`GATE_LORE`)
  revealed across tier ascents and salvage finds (§15 anomaly → §0.1 lore), each
  voiced as "The Gate" through the feed (`AlertFeed::announce`). The SYSTEMS overlay
  now shows the active objective + the latest gate beat + a `mystery N/7` counter;
  saved via `SaveState`. So the pull is now *narrative* (a mystery you chase), not
  only *systemic*. **Remaining (deeper, post-MVP):** branching threads, authored
  characters, and a scripted gate-opening climax (§17 endgame).

---

## B. MVP-scoped gaps (listed "In" per §34, not yet built)

### 3. 🟠 Combat is non-interactive — no live commands, thin doctrine, no diorama — §9, §22
- **GDD:** Doctrine + light tactical input. Doctrine presets: band, torpedo
  discipline, PDC priority, heat ceiling, target priority, retreat threshold. Live
  commands: focus fire, launch salvo, flip-and-burn/retreat, go dark, brace. A
  **watchable voxel diorama** renders the range-band sim (§22).
- **Built:** `sim::combat::resolve` is a correct, deterministic range-band
  resolver with a *minimal* `Doctrine` (band + `salvo_reload` + screen fraction).
  The player's only lever is `engage_raiders(band)` — pick a band, the fight
  resolves autonomously. No live commands, no per-doctrine PDC/target/retreat
  settings, no heat, **no diorama** (combat has no on-screen presentation at all).
- **Status:** The headless resolver (build-order #7, first half) is done and
  balance-tested; the *command layer* and the *diorama* (§22) are not. Combat
  command is verb #3 (§0.4) — currently the shallowest of the four verbs.

### 4. 🟠 Persistence: a single JSON slot; no multiple slots, no Ironman — §13, §30
- **GDD:** Free-save / **multiple slots native**; **optional Ironman** mode (§13).
- **Built / ADDRESSED 2026-06-16:** the core `sim::persist` round-trip drives
  **3 numbered manual slots** (`user://torch_slot_N.json`) with a SLOT/SAVE/LOAD
  touch-control row and a `save_peek` binding that shows each slot's saved day; and
  an **Ironman** toggle (in the settings) that autosaves every 20 s to a dedicated
  slot and **blocks manual reload** — no scumming a bad call (§13). This also fixes
  a mobile gap: save/load was previously desktop-only (F5/F9), now it's touchable.
- **Status:** Multiple slots + Ironman now in; only fancier slot metadata (named
  saves, screenshots) is unbuilt.

### 5. 🟠 Expressive identity is partial — §14
- **GDD:** Corporation name, logo, **livery colors** across fleet + stations; ship
  naming; the Rocinante effect.
- **Built:** Ship naming (`ships::christen_ship`) and the Rocinante effect
  (`Corp::resolve_engagement` sorts veterans-first, flagship spotlight) are done.
  **Corp name + livery now in** (`Corp.name`/`Corp.livery` + `CORP_NAMES`/`LIVERY`
  palettes): the player cycles a corporation name preset and a fleet livery colour
  (RENAME / LIVERY in the FLEET view); the livery paints the warships on the orrery
  and the corp-name label, and both persist in `SaveState`. **Logo** (a baked
  monogram/texture) is deferred — it needs the art pipeline (roadmap #11).
- **Status — ADDRESSED 2026-06-16 (name + livery).** Both halves of §14 are now
  present: attachment (named/blooded hero ships, Rocinante effect) **and**
  self-expression (corp name + livery). Only the logo/texture remains, gated on art.

---

## C. Intentional simplifications (functional, reduced from spec)

### 6. 🟡 Persistence is JSON, not binary bincode — §30
- **GDD:** "Binary (serde + bincode) … ship binary. Dev JSON export for inspection."
- **Built:** JSON *is* the shipping format (`persist.rs` uses `serde_json`,
  `SAVE_VERSION = 1`). The code comment acknowledges the divergence.
- **Why:** `serde_json` is already in the locked dep tree (via gdext / §31); bincode
  was not, and adding it needs a network fetch. JSON satisfies versioned save/load
  today. Low risk; revisit if save size/perf ever matters.

### 7. 🟡 The multi-view shell *replaces* the map; it is not slide-over panels — §18
- **GDD:** "The live 3D orrery owns the screen … panels slide from the edges …
  **the map never fully occludes.**"
- **Built:** The 2026-06-16 shell is a **nav-rail view switcher**: SYSTEMS shows the
  orrery with a right context panel (map visible), but FLEET / BUILD / MARKET are
  **full-screen panels that fully replace** the orrery.
- **Why / tension:** This follows the **player-supplied UI mockups**, which
  themselves depict full-screen fleet/production/market views. So the build matches
  the *mockups* while diverging from *§18's* "never occludes" rule. **Flag for
  reconciliation:** decide whether §18 should be amended to bless full-screen views,
  or whether the non-map views should become non-occluding overlays.

### 8. 🟡 Commodity chain truncated to Raw→Refined — §7d
- **GDD:** Raw → Refined → **Components** → **Assembled** (hull plate, frames,
  electronics, munitions; ships/modules/torpedoes). MVP target ~6–8 commodities,
  2–3 tiers.
- **Built:** 6 commodities (Ice/Ore/Volatiles → Remass/Water/Metals/ReactorFuel)
  and `sim::industry` does **Raw→Refined only** (output index = input + RAW_COUNT).
  No Components or Assembled commodity tiers; ships are commissioned for credits +
  crew, not assembled from a component chain.
- **Status:** Within the loose MVP "2–3 tiers" band at the low end, but the deeper
  chain the §7d vision implies is not present.

### 9. 🟡 Combat omits heat, facing, and most doctrine knobs — §8a, §9
- **GDD:** Fixed/spinal vs. turreted railguns (a **facing** consideration, §8a/§9);
  **heat** ceiling + heat-soaked radiators (§9, §23a); retreat threshold, target
  priority, PDC priority doctrine.
- **Built:** Railguns are a flat per-class count with band-based effectiveness; no
  facing/spinal model, no heat model, doctrine is band + salvo-reload + screen only.
- **Status:** The §8b escalation axis (railgun count) and the §8a torpedo-saturation
  equalizer *are* modeled and emergent; the finer combat texture is not.

### 10. 🟡 Civilian classes partial — §8e
- **GDD:** Courier/Shuttle, Freighter (light→bulk→ore), Miner/Prospector, Tanker,
  Salvager/Tug, Survey/Science.
- **Built:** `ShipClass` = Frigate, Destroyer, Cruiser, Battleship, **QShip**,
  Freighter, Miner, Tanker. **Missing:** Courier/Shuttle, Salvager/Tug,
  Survey/Science (and the freighter size sub-tiers).

### 11. 🟡 Crew depth is name + quality only — §11
- **GDD:** Named characters with **portraits, traits, quirks, loyalty**;
  **rename/personalize**; service history; manager voices.
- **Built:** Captain = a procedural name; wider crew = an abstract quality rating
  (§8c) with experience growth; service history + manager-voiced feed are in.
  **Missing:** portraits, traits, quirks, loyalty, rename/personalize.
- **Status:** Explicitly **right-sized** per §0.2 (#3 "support, not RimWorld-deep").
  A sanctioned simplification, listed for completeness.

### 12. 🟡 Data/tuning pipeline covers only commodities — §31
- **GDD:** Hot-reloadable data (JSON/RON) for **all** tunable values — economy
  curves, commodity defs, **class specs, faction params**.
- **Built:** Only `data/commodities.json` (a per-commodity tuning overlay,
  `reload_commodities`). Class specs, faction params, economy curves, pressure
  constants remain compiled-in.
- **Status:** "numbers in data, logic in Rust" proven for one domain; not yet
  generalized.

### 13. 🟡 Orrery omits trajectory ghosts, band rings, two-finger azimuth — §21
- **GDD:** Schematic overlay incl. **range/band rings** and **trajectory ghosts**;
  camera with **two-finger azimuth** rotation.
- **Built:** Orbit rings, billboarded tags, hauler lane trails, the gate ring, and
  pinch-zoom + tap-to-focus are in. **Missing:** trajectory ghosts and band rings
  (no committed trajectories or on-map combat to draw — gated by deviation #1/#3),
  and azimuth rotation (camera direction is fixed; only zoom + focus).

### 14. 🟡 No view interpolation — §28
- **GDD:** "Fixed sim tick + **view interpolation** (determinism decoupled from
  framerate)."
- **Built:** The shell snaps node positions to the latest sim tick each frame; no
  interpolation between ticks. At 6 ticks/s this can look slightly stepped at 1×.
- **Status:** Minor; cosmetic.

### 15. 🟡 No GUT view/integration tests — §32
- **GDD:** Native cargo tests **+ GUT** for the Godot/view + integration layer.
- **Built:** Native cargo tests (139) + a headless render-capture workflow (xvfb)
  used during development. **No GUT suite** exists.
- **Status:** View correctness is currently verified by manual render captures, not
  automated GUT tests.

---

## D. GDD-sanctioned deferrals (post-MVP / player-chosen — not defects)

### 16. 🟢 Audio dropped; juice partial — §23
- Audio is **explicitly deferred indefinitely by player choice** (the one §23 item
  consciously dropped). Juice is partial: act-now flash, ascension flash, lane
  trails, gate-glow, coloured feed, starfield — in; heat bloom, drive plumes, PDC
  tracers, vacuum-impact silence (mostly combat-presentation juice) — not, pending
  the §22 diorama.

### 17. 🟢 Voxel art + procedural assembly tool not built — §24, §25
- Ships/stations render as **primitive meshes** (spheres, a wireframe capsule
  blueprint). The voxel aesthetic, authored-then-baked meshes, visible loadouts,
  battle damage, and the offline procedural-assembly tool are **roadmap item #11
  (todo)** — explicitly post-foundation. Not a silent deviation.

### 18. 🟢 Endgame arc not built — §17
- The gate opening, multi-system procedural frontier, colonization race, empire,
  and incursions are **§17 "Post-MVP"** and roadmap #15. Salvage (§15 seed) is in;
  excursions/boarding/anomalies are post-MVP. As specified.

---

## Recommended reconciliation order

1. **Delta-v movement (#1, 🔴).** The biggest fidelity gap and the unlock for
   verb #4, an honest FLEET view, trajectory ghosts/band rings (#13), and real
   interdiction geometry. Highest leverage.
2. **Combat command + diorama (#3, 🟠).** Turns verb #3 from a one-button resolve
   into a played mechanic; also where the §23 combat juice lives.
3. **Authored gate-mystery thread + opening missions (#2, 🔴).** The #1 design
   priority's missing half; can proceed in parallel as content, not engine work.
4. **Reconcile §18 vs. the mockups (#7).** A doc decision, not code: amend §18 to
   bless full-screen views, or make non-map views non-occluding.
5. **Identity, save-slots/Ironman, data-pipeline breadth (#5, #4, #12).** Smaller,
   independent fills of named MVP items.

The remaining simplifications (🟡) and deferrals (🟢) are either intentional
right-sizing (§0.2) or GDD-sanctioned post-MVP scope, and need no action beyond
being tracked here.
