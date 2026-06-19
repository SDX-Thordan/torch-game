# TORCH — State of the Game & Backlog (consolidated, 2026-06-18)

**This is the single live source of truth for status + what's left.** It supersedes the
scattered "Next:/Deferred:" notes in the CLAUDE.md learnings log and the prior (2026-06-17)
version of this file. The per-area plan docs (`EMPIRE_LAYER_PLAN`, `EMPIRE_PHASE2_PLAN`,
`EMPIRE_DIPLOMACY_PLAN`, `POST_GATE_PLAN`, `ART_TRACK_PLAN`) are now **completed archives**
— historical decision records, not live TODO lists. Forward narrative design lives in
`MID_LATE_GAME_STORY.md` (notes, not built). When something here ships, tick it and move on.

---

## 1. Where we are

TORCH is a **hard-sci-fi empire/economy sim** (Distant Worlds / Stellaris identity in the
Expanse's Sol) on a **deterministic Rust core + Godot 4.6 shell**, Android-first. The entire
GDD build order (steps 1–15) and the empire re-aim (Part VI) are **built and green**, plus
three review-driven depth phases (Agency, Combat purpose, Colony development) and the
early-game focus pass.

**Scale:** ~31 sim modules of deterministic Rust + ~4k LOC GDScript shell; **201 core +
10 QA + 17 GUT tests**, all green; 282 sim bindings; a 7-persona / 3-lens automated QA
harness (works? / engaging? / reachable?).

### Built & shipped (the complete arcs — archives in parentheses)

- **Foundation (build steps 1–10, 12, 13, QA):** Android-on-Rust, the §0 retention spine,
  deterministic core, the stable multi-market economy + §7c no-death-spiral gate, §7b
  interceptable traffic, ships/fitting/combat, alert feed, factions/reputation/research/CEO,
  managers/automation, tier ascent + gate foreshadowing, pressure/pacing, the QA harness.
  The tagged-release APK is now **updatable** — stable committed signing key + tag-derived
  `versionCode`/`versionName` (see CLAUDE.md §7.6).
- **Empire layer (E1–E8, EP1–EP4)** — acquisition by economy/diplomacy/military, admin-cap
  overextension, the per-faction coalition, holdings→supply→markets economic integration,
  the piracy/inspection security layer, corporate diplomacy. *(EMPIRE_*_PLAN.md)*
- **Post-gate endgame (G1–G5)** — the far side as a place, its economy, the bridgehead, the
  incursions, the win/loss. Every rung transit-gated. *(POST_GATE_PLAN.md)*
- **Art track (A1–A7)** — procedural ship forge + interactive designer, faction-distinct
  hull *shapes* + palettes, civilian/station kit, orrery fleet, the 3D combat diorama,
  single-mesh bake. *(ART_TRACK_PLAN.md)*
- **Phase A — Agency:** act-now exceptions are now multi-option **dilemmas** (shortage /
  wreck / raid / war-collateral), answering pays + climbs the spine, hard-pause on stacked
  dilemmas, a Responder QA persona.
- **Phase B — Combat purpose:** bounty + scrap on a win; the tiered named-weapon catalog;
  **earn schematics → slowly produce → refit** (no buying advanced weapons); per-slot model
  loadouts + per-model refit (macro fleet-loadout, not mid-fight micro).
- **Phase C — Colony development:** the *tall* growth axis (develop levels) + an empire-wide
  development doctrine + a Developer QA persona that proves tall pays.
- **Ship sourcing:** civilians (+ OPA-gated corvettes) from Tycho; capital hulls need your
  own expensive shipyard.
- **Early-game focus:** functional miners (the industrial first step) confined to the belts +
  outer moons; the Earth/Mars war that haunts the early game with **space-asset collateral**;
  visible miners on the orrery; **contested colonies** (the Ganymede conflict) with an
  influence gauge + COURT/CLAIM, across the jovian/cronian markets **and the belt's major
  stations** (Eros/Pallas/Vesta/Tycho).

---

## 2. The backlog (prioritized, deduplicated)

Ordered by value to the *vision* (early→mid→late game) and the measured gaps. Every rung is a
small focused PR in the established style — additive, gated, determinism-preserving (§7c + QA
byte-identical unless it legitimately changes the loop, then regenerate the review honestly).

### P0 — The relaxing macro-pace re-aim *(active player direction)*
The game should feel like a slow trade/management sim: macro decisions, then *wait* and react
occasionally — not "clicky." Founding things **takes time** (an outpost ~180 days), events are
rare (~1 per 180 days, already tuned), and the stake-less gate-progression is being removed.
- [x] **Timed outpost construction** — founding is a ~180-day build (developing ~120); the
  outpost is inert until it comes online, then announces itself. The "set it and wait" loop.
- [x] **Removed the gate-transit endgame** — the stake-less "⟁ Transit Gate" verb + the far-side
  bridgehead/defend verbs + the gate-endgame status line are gone from the shell (the sim code
  stays, dormant). The ring remains a slow-burn mystery in the Mysteries tab. *(Still to do: soften
  the late campaign tiers so "The Gate" isn't framed as the destination.)*
- [~] **Timed everything** — outpost founding/develop (~180d), shipyard founding/expand (~360/240d), and colony development (~180d) are now multi-day builds with visible countdowns + lagged benefit. Only ship commission is still instant.
  become multi-day builds with a visible countdown (no instant macro actions).
- [ ] **UI toward the Stellaris-style mockup** — top-bar resource **rates** (+/tick), an
  **OUTLINER** (sectors/fleets/civilians/shipyards), an **OVERVIEW** home view, a **NOTIFICATIONS**
  panel, and the object panel's action icons.

### P1 — The mid/late-game authored arc *(the headline forward direction)*
The vision's whole middle and end. Design notes in `MID_LATE_GAME_STORY.md`; **subsumes the
old "war as a state" and "narrative onboarding" items** (they're now this arc). Sequence:

- [ ] **M1. Protomolecule lore thread** — a second time-gated `*_LORE` array (the derelict →
  the lost lab → **Eros** atrocity → **Venus** → the artifact builds the Ring → hands off to
  the existing gate-mystery beats). Pure `feed.announce`, byte-identical.
- [ ] **M2. `CrisisState` governor** — a mid-game time gate that escalates the *already-built*
  `FactionWar` flashpoint cadence + `sim::contest` flare magnitude into the **Earth/Mars war +
  OPA uprising** crisis. Telegraphed via `pressure`.
- [ ] **L1. Population flight → Sol economic collapse** (transit-gated) — a decaying demand/crew
  drift on the inner markets as people leave through the gate.
- [ ] **L2. EMC gate blockade** — re-aim the E3 coalition to garrison/toll the ring (a transit
  + far-side-route gate the player runs, breaks, or bribes).
- [ ] **L3. Free Navy ⟷ EMC war-state** — a two-bloc front at the ring resolved via
  `combat::resolve`, with the four player stances (back Belt / back inners / profit both / stay out).
- [ ] **L4. Powers wane → the player's opening** — collapse + infighting lower faction alarm
  ceilings / garrisons / contest grip; fold into the G5 win-state ("inherit the broken Sol").

### P2 — Living diplomacy *(cheap; covers the one uncovered system, GDD §46)*
- [ ] **D1. Diplomacy payoffs** — allied independent companies route more NPC trade to your
  owned markets (passive tariff income); rivals undercut/contest you (passive friction), so
  rivalry has a downside beyond "can't annex."
- [ ] **D2. A Diplomat QA persona** — courts companies to Ally + annexes the frontier
  peacefully; first QA coverage of the E8 loop (so we can *see* if diplomacy is fun).

### P3 — Art & assembly pipeline *(the biggest single lift; independent track)*
- [ ] **Build step #11 — procedural assembly tool (offline) + baking pipeline** (§24/§25): the
  true voxel assembly authoring path, beyond the current in-engine primitive forge.
- [ ] **Voxel combat diorama** (§22) — upgrade the diorama from primitive hulls to voxel meshes.
- [ ] **UV-atlas pass** (§25) — one surface/one texture; only worth it once hulls carry real
  texture detail.

### P4 — UX / juice polish *(incremental)*
- [x] **Desktop polish pass** — PC mode now opens **maximized** (project `window/size/mode=2`
  + `Window.MODE_MAXIMIZED`) so it fills the screen / a tiling-WM column instead of a small
  floating box, with **F11** toggling true fullscreen; PC HUD scale nudged 1.0→1.2 for table
  legibility; **mouse-drag pans the map**, **Shift-drag rotates** (`_pan` ecliptic offset on the
  camera focus); **MSAA 4× + FXAA** + denser orbit-torus segments kill the stepped orbit lines;
  darker space sky with the nebulosity confined to the Milky-Way band (no free-floating blobs).
- [x] **Mobile readability + map gestures** — HUD magnified for handhelds (window
  `content_scale_factor`), pinch-to-zoom fixed (the content host was eating map touches),
  and map **rotation** added (one-finger drag / two-finger twist / ↺↻ buttons / `,`·`.` keys).
- [x] **Touch-first UI pass** — dropped the persistent op-button grid for **contextual
  actions** (only the verbs the tapped body affords) + finger map control; **pause/play**
  buttons in the top bar; dilemma popups are now a **large centred modal that hard-pauses
  for every popup**; HUD panels are **drag-to-move**; opens centred/zoomed on the home station.
- [x] **Ledger view (EU4-style)** — a 6th nav view: a sortable, tabbed numeric overview of
  every asset class (Fleet / Miners / Outposts / Colonies / Markets) with click-to-sort column
  headers + an asset-summary line. Pure shell over existing bindings (byte-identical).
- [x] **Gate-as-mystery re-aim (Expanse model)** — removed the always-visible RING-GATE progress
  bar/% (the §0.1 "carrot from minute one"); the gate mystery now starts **hidden** (nobody knows
  of the ring or the protomolecule) and lives in a new **Mysteries** ledger tab that escalates
  N/7 as play turns up fragments. Sets up P1/M1 (the protomolecule thread as a 2nd mystery).
- [x] **Object-contextual interaction model** — the tapped object is the centre: the right
  panel re-centres on it (identity + a detail block: mineral yield / miner status / contested
  **influence gauge** / colony dev) and the action stack shows only what it affords — **Send/
  Withdraw Miner** on a belt/moon site, **Build Shipyard** on an uninhabited body, **Court/
  Claim** on a contested hub, **Develop** on a colony you own, **Expand** on your shipyard.
- [x] **Outpost layer (first slice)** — **multiple body-built outposts**: found one at any
  uninhabited body (Build Outpost), **develop** it through levels (each pays a per-level credit
  tribute), and a **co-located miner hauls to it for +50%** output. Wired to the object panel +
  contextual verbs. Player-only ⇒ byte-identical.
- [~] **Outpost → colony progression** — full chain: found outpost → develop (L1-5) → build Mine/Storage/Hangar → **grow population by supplying Ice** (the basic good; gates promotion) → promote to Colony (3× yield). Next rungs: colony→hub→capital, Storage/Hangar effects, per-asset inventory (#10).
  production, each a building with its own effect) instead of a flat level; an outpost
  **developing into a named colony** (joining the holdings/colony track); and true
  **miner→nearest-station routing** (today the boost is on-body co-location). Shell-ready.
- [ ] **Save UX on mobile** — manual save/load buttons were removed in the touch pass;
  persistence is the *Ironman autosave* toggle. Reinstate a save entry (e.g. a pause-menu
  item or an always-on periodic autosave decoupled from Ironman's no-reload rule).
- [ ] **Bundled sci-fi font** — the biggest remaining gap from the UI mockups' feel.
- [ ] **Deeper console-chrome + richer juice** (§20/§23 minus audio) — the diegetic shell pass.
- [ ] **Right-sized crew depth** (§11) — optional portraits / light service arcs, kept un-deep
  per §0.2 (low priority).

---

## 3. Non-goals — explicitly dropped (do not re-raise)

Recorded so they stop resurfacing as "unfinished":

- **Audio** — deferred indefinitely by player choice (§23c). The one consciously dropped MVP item.
- **Live mid-fight combat commands (B3)** — player call: *macro > micro*; fleet **loadouts** are
  the decision that matters, resolved pre-engagement. The diorama stays presentation + doctrine knobs.
- **Per-colony micro policies** — superseded by the empire-wide **development doctrine** (macro tilt),
  per the same macro>micro steer.
- **Voxel-true art / per-hull fidelity** — the primitive forge + per-material bake is the right
  ceiling for now; revisit only if hulls fill the frame at higher fidelity (tracked under P3).
- **RimWorld-deep crew simulation** — §0.2 keeps crew as light flavour (name + trait + service history).

---

## 4. Health check (what to protect)

- **The determinism discipline is the asset** — seed-reproducible reviews, byte-identical gating,
  the §7c gate, content-in-code persistence. It's *why* big layers land fast and safe. Hold the bar.
- **The 3-lens QA harness** catches feel-regressions a unit test can't; regenerate
  `SAMPLE_GAMEPLAY_REVIEW.md` after any gameplay change (it has a hand-added do-not-edit header line).
- **Workflow:** small focused PRs, **squash-merged to `main`**, one concern each; CI is the gate;
  `main` always green; tick this backlog + append a learnings entry per PR.

---

## 5. Where the records live

| Topic | Doc |
| --- | --- |
| **Live status + backlog** | **this file** |
| Authoritative design | `TORCH_Unified_Design_Document2.md` (GDD; Part VI = empire re-aim) |
| Player agency / control model | `docs/TORCH_Player_Influence_and_Interaction_Model.md` |
| Mid/late-game story (notes, not built) | `docs/MID_LATE_GAME_STORY.md` |
| Empire layer (done) | `docs/EMPIRE_LAYER_PLAN.md` · `EMPIRE_PHASE2_PLAN.md` · `EMPIRE_DIPLOMACY_PLAN.md` |
| Post-gate endgame (done) | `docs/POST_GATE_PLAN.md` |
| Art track (done) | `docs/ART_TRACK_PLAN.md` |
| Latest QA verdict | `docs/SAMPLE_GAMEPLAY_REVIEW.md` (regenerated each run) |
| Decision/learnings history | `CLAUDE.md` §7 (append-only) |
