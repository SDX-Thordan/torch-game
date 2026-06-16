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
| 1 | Delta-v movement / per-ship position — **✅ complete** (warships + combat positioning + freighters, all delta-v-costed) | §2 / §6 | 🟢 (was 🔴) |
| 2 | Authored gate-mystery thread + opening missions — **✅ done** (MVP seed) | §0.1 / §16 | 🟢 (was 🔴) |
| 3 | Combat command layer + diorama — **✅ done** (engage verb, doctrine knobs, BattleLog playback) | §9 / §22 | 🟡 (was 🟠) |
| 4 | Save slots + Ironman — **✅ done** (3 slots + Ironman autosave) | §13 / §30 | 🟢 (was 🟠) |
| 5 | Expressive identity — **corp name + livery ✅ done** (logo deferred) | §14 | 🟡 (was 🟠) |
| 6 | Persistence is JSON, not binary bincode | §30 | 🟡 (intentional) |
| 7 | Multi-view shell vs. §18 slide-over panels — **✅ reconciled** (GDD amended) | §18 | 🟢 (was 🟡) |
| 8 | Commodity chain — **✅ deepened to 4 tiers** (Raw→Refined→Components→Assembled, 12 goods) | §7d | 🟢 (was 🟡) |
| 9 | Combat: **heat / aggressive-fire ✅** + retreat/target doctrine; facing/spinal still pending | §8a / §9 | 🟡 (narrowed) |
| 10 | Civilian classes partial (no Courier/Salvager/Survey) | §8e | 🟡 |
| 11 | Crew depth: name + quality only (no portraits/traits/quirks/loyalty/rename) | §11 | 🟡 (right-sized) |
| 12 | Data pipeline: commodities **+ ship class specs** externalized; factions/curves in code | §31 | 🟡 (narrowed) |
| 13 | Orrery omits trajectory ghosts, range/band rings, two-finger azimuth | §21 | 🟡 |
| 14 | No view interpolation (positions snap per tick) | §28 | 🟡 |
| 15 | GUT view/integration tests — **✅ added** (15 tests in CI, the sim↔view contract) | §32 | 🟢 (was 🟡) |
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
  fleet to the focused world. Persistence saves the nav state.
- **Combat positioning ADDRESSED 2026-06-16.** `engage_raiders` is now gated on
  position: raiders muster on the inner lanes at the **home core** (`markets[0]`'s
  body, where hulls commission), and **only warships on station there** answer —
  losses fall on those ships alone (`Corp::resolve_engagement_for`, Rocinante effect
  preserved within the engaged group), while a fleet flown to the outer system
  **can't defend the core** until it burns home. So the delta-v movement layer is
  *consequential*: positioning the fleet is now a real defensive decision.
  `warships_on_station()` drives an accurate shell readout (FLEET doctrine line +
  "recall the fleet" message). Backward-compatible (fresh hulls dock at the core →
  on station), so all tests + the QA review are unchanged.
- **Freighters positional ADDRESSED 2026-06-16.** A freighter running a standing
  route now has a **live map position**, interpolated along its orbital lane
  (origin → dest market body) by trip progress — the same lane model the NPC haulers
  use (`route_freighter_pos`/`flying_routes`, a `departed` tick on `TradeRoute`).
  Freighters render as a distinct muted-green marker with a lane trail on the orrery,
  and the FLEET view shows each one's **real trip + progress** ("Mars Colony → Ceres
  Yards · In transit 44%"). The pool-dispatch semantics are unchanged, so the route
  tests + the QA review stay byte-identical.
- **Freighter remass ADDED 2026-06-16.** The last delta-v nuance: a dispatched
  route freighter now **refuels with Remass at the origin port**, an amount scaled
  by the trip distance (`route_remass_units = travel_ticks / 10`). Long outer hauls
  cost far more fuel than inner hops (the delta-v constraint as opex), and a hub
  that produces cheap Remass (the Ice→Remass chain) lowers the whole network's
  running cost — closing the production→logistics loop. A route only dispatches if
  it can source + afford the fuel. The FLEET view shows the per-trip fuel; the QA
  Logistician still profits (~4×). **Pillar #2 (delta-v / positional fleet) is now
  complete** — every player ship is positional *and* delta-v-costed.

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

### 3. 🟡 Combat command layer + diorama — ✅ done (live commands deferred) — §9, §22
- **GDD:** Doctrine + light tactical input. Doctrine presets: band, torpedo
  discipline, PDC priority, heat ceiling, target priority, retreat threshold. Live
  commands: focus fire, launch salvo, flip-and-burn/retreat, go dark, brace. A
  **watchable voxel diorama** renders the range-band sim (§22).
- **Built (this pass):** the combat *command layer* + *presentation* are now in.
  `Doctrine` gained **target priority** (biggest hull / most wounded) and a
  **retreat threshold** (break off below a chosen fraction); both are pre-engagement
  doctrine knobs the player sets in the FLEET view (RANGE / TARGET / RETREAT
  cycles), and the resolver honours them (`CombatEvent::Retreat`, winner credited
  to the side that holds the field). The **engage verb** is wired to the shell
  (FLEET-view `◆ ENGAGE` button + `W` key), and resolving a fight opens a
  full-screen **diorama** (`sim::combat`'s BattleLog played back beat-by-beat —
  salvos, volleys, kills, retreats — colour-coded by side, ending on a verdict +
  survivor tally). The world pauses for it; tap to dismiss.
- **Status:** Combat is now interactive and watchable end-to-end: set doctrine →
  engage → watch the BattleLog resolve. The remaining GDD texture — **mid-fight
  live commands** (focus fire / go dark / brace), **heat**, and a true **voxel**
  diorama (vs. the current text BattleLog) — is deferred to a later combat pass
  (the heat/facing omissions are tracked separately as #9, 🟡).

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

### 7. 🟢 Multi-view shell vs. §18 — ✅ reconciled (GDD amended) — §18
- **GDD (original):** "The live 3D orrery owns the screen … panels slide from the
  edges … **the map never fully occludes.**"
- **Built:** The 2026-06-16 shell is a **nav-rail view switcher**: SYSTEMS shows the
  orrery with a right context panel (map visible), but FLEET / BUILD / MARKET are
  **full-screen panels that fully replace** the orrery.
- **Resolution (2026-06-16):** §18 is **amended** to bless the map-anchored nav-rail
  console as the **mobile-first realization** — full-screen data views on the
  6-inch-phone target are consistent with §18's own "either map *or* data" honesty
  bullet, and the player is always one rail-tap from the map. The slide-over,
  never-occlude treatment remains the documented ideal for wide (tablet/desktop)
  screens. The build matches the player-supplied mockups; the GDD now matches the
  build.

### 8. 🟢 Commodity chain — ✅ deepened to four tiers — §7d
- **GDD:** Raw → Refined → **Components** → **Assembled** (hull plate, frames,
  electronics, munitions; ships/modules/torpedoes). MVP target ~6–8 commodities,
  2–3 tiers.
- **Built (this pass):** **12 commodities** in a 3-line × **4-tier** grid (§7d):
  Raw (Ice/Ore/Volatiles) → Refined (Remass/Metals/ReactorFuel) → Components
  (Composites/Alloys/Circuitry) → Assembled (Habitats/Machinery/Drives), each
  refining into the one **+3 indices** along its line. `found_refinery` (now any
  non-top-tier input) lets the player chain factories the full depth
  (Ore→Metals→Alloys→Machinery). Value rises ~3–4× per tier.
- **Design note:** the designed NPC producer/consumer spread + demand jitter apply
  only to the **lower two tiers** — finished goods sit at administered prices (jitter
  0, neutral setpoints), so they're *produced* by the player up the chain, not an
  instant-arbitrage faucet (their high absolute prices would otherwise turn tiny
  jitter into huge spreads — caught by the QA harness). A nice property: jitter 0
  draws no RNG, so the lower-tier economy stays **byte-identical** — the §7c gate and
  the QA review are unchanged.
- **Bill-of-materials link ADDED 2026-06-16.** The chain now *pays off* in the
  fleet: alongside the buy-for-credits `commission_ship`, `assemble_ship(class)`
  builds a hull from the player's **own Assembled-tier stock** (`ship_bom`: Machinery
  10 / Drives 11 / Habitats 9, scaled by hull) for a small labour fee — far below the
  off-the-yard price. So the full industrial loop closes: mine → refine → make
  components → make finished goods → **assemble warships**. The BUILD view shows the
  BOM (lit green when in stock) with an `⚙ ASSEMBLE FROM PARTS` button next to
  `◆ COMMISSION HULL`. Backward-compatible (empty warehouse ⇒ can't assemble, but
  buying still works), so all tests + the QA review are unchanged.
- **Status:** Exceeds the MVP "2–3 tiers" target, and the economy→fleet
  bill-of-materials link is now in. Fully closed.

### 9. 🟡 Combat heat now in; facing/spinal still pending — §8a, §9
- **GDD:** Fixed/spinal vs. turreted railguns (a **facing** consideration, §8a/§9);
  **heat** ceiling + heat-soaked radiators (§9, §23a); retreat threshold, target
  priority, PDC priority doctrine.
- **Built:** Railguns are a flat per-class count with band-based effectiveness +
  **target priority + retreat threshold** (#3) + a **heat / aggressive-fire model**
  (this pass): firing railguns *hot* (`aggressive_fire`) boosts alpha
  (`AGGRESSIVE_FIRE_BP`) but builds heat that periodically forces a **vent**
  (`CombatEvent::Overheat`, a diorama beat) once the radiators saturate. It's an
  opt-in doctrine knob (FLEET-view `FIRE` toggle), so **default fights are
  byte-identical** — the §7c gate and the QA review are unchanged. *Design note:*
  combat is decisive (§13), so in a quick fight aggressive is mostly front-loaded
  upside; the vent bites in *prolonged* engagements (a squadron grinding a swarm).
- **Status:** The §8b escalation axis, the §8a torpedo-saturation equalizer, the
  target/retreat doctrine, and now **heat discipline** are modeled. Still missing:
  **facing/spinal-vs-turret** and PDC-priority doctrine — the last combat texture.

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

### 12. 🟡 Data/tuning pipeline — extended to ship class specs (faction/economy curves still compiled) — §31
- **GDD:** Hot-reloadable data (JSON/RON) for **all** tunable values — economy
  curves, commodity defs, **class specs, faction params**.
- **Built (this pass):** the §31 "numbers in data" overlay now covers **ship class
  specs** too — `data/ships.json` (`reload_ship_data`) tunes the full numeric
  envelope of every hull (mass/armor/thrust/tankage/drive/power/mounts/crew → and
  thus build cost + crew bottleneck) and weapon (damage/intercept/mass/power),
  matched by name with partial-overlay + typo-error + `include_str!` sync-guard
  (`ship_data_matches_compiled_defaults`) — the exact pattern proven for
  commodities. A Sim-held `ShipCatalog` makes it take effect: future commissions
  fit from the tuned numbers. Identity (class/mount-kind) stays code-defined.
- **Status:** "numbers in data, logic in Rust" now spans the **two highest-leverage
  domains** (economy + ships). Still compiled-in: faction params, economy *curves*
  (the pricing math, deliberately code), and pressure constants — a later overlay
  if they ever need live tuning.

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

### 15. 🟢 GUT view/integration tests — ✅ added — §32
- **GDD:** Native cargo tests **+ GUT** for the Godot/view + integration layer.
- **Built:** Native cargo tests (149) **+ a GUT 9.4.0 suite** (`godot/test/`, 15
  tests / 108 asserts) that boots the real gdext core headless and exercises the
  **sim↔view binding contract** main.gd depends on — world/economy/commission/
  freighter-position/combat-on-station/BOM bindings + the `TorchShipyard` catalog +
  the `UiKit`/`MiniChart` UI helpers. Runs in CI (`ci.yml` `gut` job: a Godot-4.6.3
  container, builds the debug cdylib, `--import`, `gut_cmdln -gexit`) and exits
  non-zero on any failure, so view-layer regressions a Rust unit test can't see are
  now caught automatically (not just by manual render captures).
- **Note:** vendored GUT **9.4.0** specifically — 9.3.0 shadows Godot 4.6's new
  native `Logger` class and won't load; 9.4.0 renamed it to `GutLogger`.
- **Status:** The §32 GUT counterpart now exists alongside cargo + the (still useful)
  xvfb render workflow.

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

1. ~~**Delta-v movement (#1, 🔴).**~~ **✅ done (warships).** The biggest fidelity
   gap and the unlock for verb #4, an honest FLEET view, trajectory ghosts/band
   rings (#13), and real interdiction geometry. Freighters + combat-positioning
   follow.
2. ~~**Combat command + diorama (#3, 🟠).**~~ **✅ done.** Doctrine knobs (target +
   retreat) + the engage verb + a played-back BattleLog diorama. Mid-fight live
   commands + heat + a voxel diorama remain a later combat pass.
3. ~~**Authored gate-mystery thread + opening missions (#2, 🔴).**~~ **✅ done
   (MVP seed).** The #1 design priority's missing half.
4. ~~**Reconcile §18 vs. the mockups (#7).**~~ **✅ done.** §18 amended to bless the
   map-anchored nav-rail console as the mobile-first realization.
5. ~~**Identity, save-slots/Ironman, data-pipeline breadth (#5, #4, #12).**~~
   **✅ done** (corp name + livery; 3 slots + Ironman; §31 overlay extended to ship
   class specs via `data/ships.json`). Faction params + economy curves stay
   compiled by design — a later overlay only if they need live tuning.

**All pillar-level (🔴) and MVP-scoped (🟠) deviations are now reconciled.** The
remaining items are intentional 🟡 right-sizing (§0.2) or GDD-sanctioned 🟢 post-MVP
scope (logo/voxel art, endgame arc), tracked above and needing no further action.

The remaining simplifications (🟡) and deferrals (🟢) are either intentional
right-sizing (§0.2) or GDD-sanctioned post-MVP scope, and need no action beyond
being tracked here.
