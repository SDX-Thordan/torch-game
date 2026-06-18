# TORCH — Unified Design Document

> Working title. Alternatives: *REMASS*, *The Long Burn*, *Torch & Tonnage*, *Sol Incorporated*, *Delta-V*.
>
> **Personal project.** No monetization, no market constraints. Every scope decision answers one question: *what is fun to build and finishable solo?*

A hard sci-fi industrial sandbox for mobile (Android-first). You found a corporation in the Sol system, build a real vertical supply chain, and climb toward the day the ring-gate opens and you can become an empire. Real-time with pause. Offline. The war is mostly logistics.

**Reference DNA:** X4's industrial heart × The Expanse's scarcity politics × an authored story spine.
**Tagline:** *Logistics is the weapon. Time is the terrain.*

> **⚑ Genre re-aim (2026-06-17) — read with Part VI.** Parts 0–V below describe the
> original **X4-style corporate sandbox** (you're a CEO who *perturbs* a living
> economy and climbs to a gate). That foundation is **built and still load-bearing**
> (the orrery, the deterministic economy, interdiction, combat, the gate). But the
> project's actual north star is a **Distant Worlds / Stellaris empire sim** in the
> Expanse's Sol — *you grow a colonizing state*. The reconciliation, and everything
> built to deliver it, is **PART VI — THE EMPIRE LAYER** (the now-canonical core
> loop). Where Part VI and the older text disagree on *genre/player identity*, Part VI
> wins; the older parts remain authoritative for the systems they describe (economy,
> ships, combat, orrery, tech).

---
---

# PART 0 — THE SOUL OF THE GAME

Read this first. Everything else serves what's here.

## 0.1 The Retention Spine — *a journey toward a destination*

The core identity, derived from what actually keeps the player (and the designer) opening the app:

> **A foreshadowed destination pulls you up through tiers of scale, made alive by an emergent cold war, paid off with real dominance, then renewed when the gate opens onto a bigger, more dangerous game.**

- **The destination is the point.** The primary pull is the *goal* — reach the ring-gate and build an empire. So the gate is a **carrot from minute one**: visible, foreshadowed, always ahead, even while you run a single mining station. The slow-burn **gate mystery** is the one authored thread that gets real investment.
- **Emergence is the journey's texture.** The dynamic Earth/Mars/Belt cold war (and §7b interdiction) keeps each leg of the climb from feeling like the same road. Strong secondary investment — it serves the destination, doesn't replace it.
- **Dominance is a real, felt payoff.** You genuinely earn power in Sol; it is not rubber-banded away (§13).
- **The gate renews the journey.** Reaching the destination is not an ending — opening the gate resets the climb at a higher, more dangerous altitude (§17). The cure for goal-chaser deflation: the summit opens a taller staircase.

## 0.2 Design Priorities (over-invest / right-size / keep-light)

Derived from the player's ranked pulls. Spend design love in this order:
1. **Goal / destination pull** *(over-invest).* Foreshadow the gate everywhere; make tier progress legible; make the ascent feel like an ascent.
2. **Emergence — the cold war & interdiction** *(strong investment).* The living world that makes the journey surprising.
3. **Attachment — crew & signature ships** *(support, right-sized).* Gives the journey weight; solid but not RimWorld-deep.
4. **Mastery — fitting & logistics** *(keep light).* Satisfying, never grindy; never gate journey-progress behind optimization. (The deterministic-sim rigor is the *builder's* joy, not forced *player* homework.)

## 0.3 Tiers of Play — the ascent

The game is played as a progression of *scale*, each tier a different **kind** of game (not merely a bigger one — that's the make-or-break for retention):

- **Tier 1 — The Station.** One mining operation. A tight survival/optimization puzzle: keep one thing alive and profitable. Teaches the core verbs.
- **Tier 2 — The Region.** Extended infrastructure across your neighborhood (the Belt, or the Jovian moons). Logistics network-building + your first real predators (piracy, interdiction). The X4 expansion feeling.
- **Tier 3 — Sol & the Cold War.** The whole system and its politics — Earth/Mars/Belt flashpoints, embargo, leverage, fleets. The strategic/geopolitical game, where emergence peaks and you *earn dominance*.
- **Tier 4 — The Gate.** The ring opens: the colonization race, empire beyond Sol, and external threats that re-challenge even a dominant power.

Each transition is a milestone with arrival-fanfare and a clearly visible "next altitude." If the player can ever not see the next rung, that's a retention leak.

## 0.4 The Minute-to-Minute Problem & The Verbs of Joy

TORCH removes embodiment (you're the CEO, never the captain) and automates the routine. So fun must come from somewhere, and **automation must clear chaff to make room for fun verbs you opt into — never remove action.** "Run by exception" only works if the **exceptions are verbs, not acknowledgments.**

The verbs to design around:
1. **Interdiction & piracy** *(the standout — §7b).*
2. **Ship fitting & optimization** *(§8).*
3. **Combat command** *(§9, §22).*
4. **Trajectory problem-solving** *(§6).*

**The three-horizon goal stack** keeps it all moving: always a *now* goal (this session: run this raid, fit this ship), a *tier* goal (this chapter: take the Jovian moons), and the *far* goal (the gate, the empire). The moment a player can't name all three, retention leaks.

---
---

# PART I — GAME DESIGN

## 1. High Concept

You are a persistent founder-CEO, not a ship captain. Starting from one station or claim in Sol, you build the chains that keep civilization breathing — ice into reaction mass, ore into hulls, fissionables into fuel — and you climb toward the day the dormant ring-gate opens and a corporation can become an empire. There is no FTL (until the gate). Travel costs time and delta-v. Ships are fragile and lethal. Earth, Mars, and the Belt are sliding toward open war, and you are the supplier, the opportunist, and eventually a power in your own right.

## 2. Design Pillars

1. **Time is terrain.** A live orrery; bodies move on their orbits. Every move is a committed trajectory with a real travel time and delta-v cost.
2. **Delta-v is the universal constraint.** Reaction mass and reactor fuel are the true limiters. Refueling infrastructure is strategic ground.
3. **The economy is alive and you are inside it.** A self-sustaining NPC economy runs around you; you are a perturbation on it, never its foundation.
4. **Logistics is a weapon you wield.** Interdiction, supply, and embargo are *actions* (§7b).
5. **Combat is doctrine first, physics always.** Newtonian, lethal. The deaths land on the people you hire.
6. **A destination always ahead.** The gate pulls from minute one; the tiers are the ascent (§0).

## 3. Player Fantasy & Strategic Arc

Turn one industrial foothold into a power that can stand against the inner planets — not by out-gunning them in their own pond, but by escaping it. The arc *is* the tier ascent (§0.3): Station → Region → Sol/Cold War → Gate/Empire. Through early/mid game you cannot win a stand-up fight with Earth or Mars (§8f); you survive by other means, climb to genuine dominance in Sol, then the gate opens the next, larger game.

You are **independent** by default, navigating Earth, Mars, and the Belt, though reputation and CEO perks can pull you toward a faction.

## 4. Setting & Factions

Hard sci-fi, present-tense Sol. No aliens (until the gate), no magic, no shields. Texture: scarcity, radiation, vacuum, politics.

- **Earth / inner authority** — populous, resource-hungry, bureaucratic, militarily strongest. Deepest markets. *Visual: utilitarian, boxy, bilateral, hull-heavy.*
- **Mars / militarist republic** — high-tech, disciplined, expansionist. Best shipyards and drive tech. *Visual: elongated, angular, weapon-forward.*
- **The Belt / Belter coalition** — decentralized, resource-rich, fragile life-support, resentful of the inners. Your home turf. *Visual: asymmetric, welded, salvaged.*
- **Independents & corporations** — rivals, partners, contractors; the long tail of the catalog.
- **Raider factions** — prey on weak lanes; persistent, escalating.

**The map, in layers (matching the tiers):**
- **Inner-system core (Tier 1–2 / MVP):** Earth/Luna, Mars, Ceres, the inner belt, one outer moon.
- **Outer-system pirate frontier (Tier 2–3, post-MVP):** the moons of Jupiter, Saturn, Uranus — lawless, escalating risk/reward; home of **excursions** (§15).
- **Beyond the gate (Tier 4, long horizon):** procedurally generated systems (§17).

The three-way cold war is a living tension meter; flashpoints create embargoes, warzones, and lucrative-but-dangerous contracts.

## 5. Core Loops

- **Moment-to-moment (paused planning):** read the system → spot a need/opportunity → queue trajectories, production, trades, builds, policies → unpause → watch → re-pause to react.
- **Session:** advance the web, run a contract or a raid, fight or avoid an engagement, fit a hull, invest in research/blueprints/reputation/CEO perks, handle a flagged crisis, push toward the next tier rung.
- **Campaign-long:** the tier ascent toward the gate and the empire beyond.

## 6. Time & Travel Model

- **Real-time with pause.** Runs while open; pause freely. **Clock pauses when backgrounded.**
- **Orbital map as a live orrery.** Simplified deterministic orbits (precomputed / patched-conic — not n-body).
- **Trajectory choice per move:** economical (slow, low delta-v) vs. hard burn (fast, expensive, taxes crew). Transfer windows make some routes cheap only at certain alignments — a genuine planning puzzle (verb #4).
- **Delta-v budget per ship** from remass tankage + drive efficiency. Running dry strands you. Outer-system excursions (§15) make this bite hardest.

> **Requirement — delta-v governs *all* ship movement (incl. the player fleet).**
> This is Pillar #2 ("delta-v is the universal constraint", §2) made concrete, and
> it is **load-bearing**, not flavor: *every* mobile ship — NPC haulers, player
> freighters, **and player warships** — must have a **tracked position** on the
> orrery and a **per-ship delta-v budget** (= remass tankage × drive efficiency,
> the `ShipStats.delta_v` proxy in §8). A move *commits* a trajectory, *spends*
> delta-v + remass, and takes time derived from the ship's drive and the chosen
> burn (economical vs. hard burn, §6 above) at the live orbital geometry — **never
> a flat speed**. A ship that runs its remass dry is **stranded** until refueled;
> refueling infrastructure is therefore strategic ground. This is what makes the
> fleet a positional, logistical asset (and a real interdiction target) rather than
> an abstract roster.
>
> **Status — closed 2026-06-16 (warships).** `sim::movement` now gives every owned
> **warship** a tracked position + remass budget (`OwnedShip.nav`): `move_ship`
> commits a trajectory at the live orbital distance, spends remass, and takes time
> from the ship's drive and the chosen burn (economical vs. hard); a dry tank
> **strands** the ship until `refuel_ship` buys remass at a dock. Ships render on the
> orrery and the FLEET view shows **real** location/fuel (no longer synthesized).
> *Remaining:* player **freighters** are still the abstract pooled-count + route
> timer (not yet per-ship positional), and combat (`engage_raiders`) is not yet
> gated on fleet position — both follow-ups toward full Pillar-#2 coverage.

## 7. Economy & Industry

The deepest system and the biggest engineering lift. Two decoupled layers, simplified for MVP.

### 7a. Pricing backbone — stockpile simulation (cheap)
Markets hold inventory that fills/drains from NPC production/consumption; prices track stock. Emergent shortages without agent-based cost.

### 7b. Interdiction & physical traffic — *featured mechanic*
**The fun engine and the heart of emergence (§0).** NPC haulers fly representative real routes and can be intercepted by you or pirates. Interdiction feeds the stockpiles — cut a convoy and you cause a *local, temporary* shortage that visibly moves prices, spawns a scarcity event, and ripples through faction relations. Gets the most playtesting attention and the most juice (§23a).

### 7c. Stability design (anti-death-spiral)
Must reach **stable equilibrium with zero player input.** Damped pricing (lerp toward a stock-based target, never raw supply÷demand); hard floors/ceilings; NPC stabilizers (reserves + emergency production at thresholds); locality (disruptions self-correct). **Acceptance test (headless):** *no market may death-spiral across thousands of ticks with no player, on any seed.*

### 7d. Commodities & chains (illustrative)
Raw (ice, ores, volatiles/CHON, fissionables/He-3) → Refined (remass, oxygen/water, metals, reactor fuel, feedstock) → Components (hull plate, frames, electronics, drive parts, munitions) → Assembled (ships, station modules, torpedoes, life-support). **MVP:** ~6–8 commodities, 2–3 tiers.

## 8. Ships, Weapons & Fleet

### 8a. Weapon systems
- **PDC** — rapid kinetic; anti-torpedo defense + close-band damage. On every warship; the defensive backbone.
- **Torpedo** — guided, slow, expensive, magazine-limited; must *saturate* enemy PDC. The alpha and the **great equalizer** — a frigate salvo can threaten a capital if it saturates the screen.
- **Railgun** — high-velocity hull-killer needing a firing solution; **fixed/spinal** (aim the hull) vs **turreted**. The scarce, capital-defining weapon and the power-curve gate.

### 8b. Military classes (the precious core) — railgun count is the escalation axis

| Class | Weapons | Role | Mobility / Armor |
|---|---|---|---|
| **Frigate** | PDCs + torpedoes; no railgun | Fast strike, escort, best **interdiction** platform | Fast, nimble / light |
| **Destroyer** | PDCs + heavy torpedoes; **1 fixed railgun** on *later faction-specific endgame* variants | Torpedo-heavy line; nose-aimed railgun variant | Moderate / moderate |
| **Cruiser** | PDCs, *fewer* torpedoes, **1 turreted railgun** | Dependable heavy; off-axis hull-killing | Slower / heavy |
| **Battleship** | PDCs, sizable torpedoes, **2 railguns** | Apex capital, strategic asset | Slow / heaviest |

**No fighters or carriers** — deliberate, true to the hard-sci-fi frame.

### 8c. Crew — the real bottleneck
Every warship is an **enormous investment in trained crew and materials.** Trained crew is a scarce, slow-to-grow resource capping how much military you can field — you grow and blood crews, you don't buy a navy. **Model:** captain = a named character (§11); the wider crew = an abstract **quality rating** that improves with experience. Losing a warship costs the hull *and* irreplaceable veteran quality — the load-bearing source of tension (§13).

### 8d. Q-ships & the light screen
True warships stay precious; the cheap, expendable screen is **Q-ships** — armed converted freighters with concealed PDCs/light torpedoes. They bridge civilian and military and satisfy the mixed-fleet pillar without diluting the stakes.

### 8e. Civilian classes
Courier/Shuttle; Freighter (light → bulk → ore carrier — trade backbone *and* prime interdiction target); Miner/Prospector; Tanker (remass/fuel logistics); Salvager/Tug (the §15 salvage loop); Survey/Science (exploration, anomalies).

### 8f. Strategic power curve — you are a corporation, not a navy
Through early/mid game you **cannot** win a stand-up fight with Earth or Mars. You survive by asymmetric torpedo threat, fighting pirates, defense, economic leverage, embargo, and diplomacy. Peer power is gated behind the gate arc (§17) — you out-grow the inner powers by escaping their reach, not matching their fleets in Sol.

## 9. Combat

**Doctrine + light tactical input**, Newtonian and lethal. No shields. **Abstract range bands** (close/medium/long); choosing the band is a doctrine setting (fixed-railgun ships add a facing consideration). **Doctrine (preset):** band, torpedo discipline, PDC priority, heat ceiling, target priority, retreat threshold. **Live commands:** focus fire, launch salvo, flip-and-burn/retreat, go dark, brace. **Lethality:** ships die hard and stay dead; wrecks are salvageable.

## 10. Progression (four layered tracks)
- **Research tree** — module tiers and efficiencies. *(Kept light per §0.2 — never a grind wall.)*
- **Blueprint discovery** — purchase, salvage, reverse-engineering *(a blueprint = a compact seed + parameter set; §25)*. Faction-specific endgame designs gated here + behind reputation.
- **Faction reputation** — gates tech catalogs, contracts, station access, prices.
- **CEO skill track** — passive buffs + unlock gates + a perk branch (**Industrialist / Trader / Warlord / Diplomat**).

## 11. Crew, Managers & CEO Identity

Right-sized to *support* the journey (§0.2), but solid enough to carry weight.
- **Named characters with texture:** managers and captains have procedural names, portraits, traits, quirks, loyalty; you can **rename and personalize** them.
- **Growth & history:** they gain skill over time; each carries a visible **service history**.
- **Voice:** they source the alert feed (§19) — reading it is reading *them*; attachment builds passively.
- **Crew-quality model:** captain named; wider crew an abstract quality rating (§8c).
- **The CEO is persistent and immortal**, growing via the skill track. No succession.
- **The asymmetry:** you can't die, but the people you hire can — a permanent, *felt* loss and the load-bearing beam of tension (§13).

## 12. Automation — Run by Exception (a double-edged tool)
You set policy; managers and fleet AIs execute autonomously. **Remove tedium, not agency:** exceptions are verbs (§0.4); a satisfying next decision is always available; watching it pay off is a reward *if it's juiced* (§23).

## 13. Pressure, Tension & Difficulty

Three layered pressures: **faction war**, **piracy & raiders**, **survival & scarcity**.

**Tension calibration — recoverable ≠ consequence-free.** Stress designed out (forecasting kills the unforeseeable; pause means you never need speed; a pacing governor stops simultaneous spikes). Tension preserved: losses bite — costly recovery, visible scars, lasting fallout; crew/warship loss is the one genuinely permanent loss in default mode.

**Dominance is real (§0.1).** Power earned in Sol is *felt and kept*, not rubber-banded away — the payoff of the climb. The re-challenge comes structurally, at the gate (§17), not by quietly scaling Sol's threats to erase your progress.

**Difficulty & permanence:** free-save / multiple slots default; **optional Ironman**; independent **pressure-intensity** setting.

## 14. Expressive Identity
Corporation name, logo, **livery colors** across fleet and stations; **ship naming**; the **Rocinante effect** — a few beloved hero ships carry attachment while the procedural fleet is wallpaper (§25).

## 15. Discovery & Wonder
**Wreck salvage** (derelicts to find, board, strip, reverse-engineer); **anomalies and lore** seeding the gate mystery; **excursions** to the outer-system pirate frontier (Jupiter/Saturn/Uranus) — expeditionary task forces far from supply, high risk/reward. Feeds blueprints *and* curiosity.

## 16. Narrative — Light Threads, One Strong Spine

Mostly emergent, with **light authored threads and faction arcs** — *except* the **gate mystery**, which is the single strong thread (it is the destination carrot, §0.1) and is foreshadowed and advanced across every tier. No hard campaign→sandbox wall: the opening missions teach the systems, then authored content *thins into* per-tier objectives and the slow-burn gate reveal, so a thread of authored pull always runs alongside the emergent world.

## 17. Endgame Arc — The Gate, the Colonization Race & Empire

The long-horizon spine that turns a corporation into an empire, and the *renewal* of the journey (§0.1). **Status (2026-06-17): BUILT** — the full post-gate sandbox (G1–G5: the far side as a place → its economy → a player bridgehead → escalating incursions → a win/loss resolution) shipped, every rung transit-gated so the inner game stays byte-identical. See **§44 (Part VI)** and `docs/POST_GATE_PLAN.md`. The handcrafted Sol far-side cluster is in; the *procedural* multi-system frontier (item 2 below) remains the open art/tooling lift.

1. **The gate opens** — a late-game threshold; Sol's dormant ring-gate activates.
2. **A multi-system frontier beyond** — procedurally generated systems (Avorion-like). Sol stays handcrafted.
3. **The colonization race** — Earth, Mars, independents, and *you* rush the gate. Be fast and strong enough to seize a good system before the inner powers do.
4. **Corporation to empire** — claim and develop your own system, beyond the inner planets' reach; finally rival or surpass them (resolving §8f).
5. **Incursions from beyond** — the open gate cuts both ways: a **breakaway militarized system with superior tech** (a filed-off Laconia-analog; your own IP) re-challenges even a dominant empire. The structural source of late-game stakes (§13).

> **Tone flag:** the one place hard-sci-fi grounding bends. Rare, costly, ominous. The climax of the power fantasy — not MVP.

---
---

# PART II — UX & PRESENTATION

## 18. Orientation & Navigation — The Command Console
- **Landscape-first** for the full experience; matches the sit-down session rhythm.
- **Map-anchored nav-rail console** *(mobile-first realization, amended 2026-06-16).* A slim **left nav rail** switches between top-level views — **SYSTEMS** (the live 3D orrery owns the screen with a right context panel; the map is never occluded *here*), **FLEET**, **BUILD**, **MARKET**. A slim **top bar** carries time controls + the alert feed across every view. On the 6-inch-phone target the data views (FLEET/BUILD/MARKET) are **full-screen** — consistent with the "either map *or* data" honesty below — and the player is always one rail-tap from the map. *(This supersedes the original "panels slide from the edges; the map never fully occludes" framing, which assumed a tablet/desktop canvas; the slide-over treatment remains the ideal for wide screens.)*
- **Portrait triage mode** *(post-core).* Glance, clear exceptions, queue an order; full orrery in landscape when seated.
- **Phone vs. tablet honesty:** side-by-side panels are a tablet luxury; on a 6-inch phone you'll see *either* map *or* data — design panels to stand alone there.

## 19. The Alert Feed — a System, not a Panel
The game's voice and pacing. **Ranked priority** with a hard **FYI vs act-now** split; **player-tunable thresholds**; **personality** (voiced by managers/captains, §11); act-now alerts **resolve into verbs** (§0.4). Mistuned it becomes notification anxiety or missed crises — so it is a designed system.

## 20. UI Visual Language
**Hybrid: clean base, diegetic frame.** Modern legibility underneath; terminal-flavored chrome, typography, color on top. Faction-coded (shape + color, never color alone).

## 21. The Orrery — True 3D Orbital Scene
Real 3D, kept readable by: **constrained camera** (pinch-zoom, two-finger azimuth, clamped pitch, near-top-down default); **schematic overlay** (orbit rings, billboarded constant-size icons, range/band rings, trajectory ghosts); **generous screen-space tap targets**; **compressed visual scale** in the renderer while the **sim keeps true distances**; **LOD/clustering + instancing**.

**Felt vastness — an explicit goal.** Resolve "Mars looks close but reads 47 days out" through **time and motion**, not distance: the crawling dot, the long burn, the clock through days, the quiet. Space feels enormous *temporally* even when spatially compressed. *(This also makes the destination feel far — feeding the §0.1 pull.)*

## 22. Combat Presentation — Watchable Voxel Diorama
A **renderer of the range-band sim, not free positioning.** Ships arrange by band; torpedoes cross the gap, PDC tracers fly, a capital flips to burn. **Bands decide, 3D presents.** Where combat juice (§23) earns its keep.

## 23. Game Feel, Juice & Audio
**For a game you mostly *watch*, this is the deliverable of fun — not polish.**
- **23a. Juice:** glowing/guttering drive plumes; **heat-soaked radiators**; PDC tracers and torpedo trails; **silence-then-impact** vacuum hits; the satisfying tick of a market; weighty UI feel. Interdiction (§7b) gets the most.
- **23b. Industrial sublime (voxel-tone fix):** grime, weathering, hard shadows, glowing drives, heat bloom, dust, cold black — push the look up through lighting/FX, not polygon count.
- **23c. Audio:** calm ambient hum in the build state; sparse, alarm-tinged beds for flashpoints/scarcity; diegetic console blips; the long quiet of a burn. Audio state tracks pressure state (§13).

---
---

# PART III — ART DIRECTION & CONTENT PIPELINE

## 24. Visual Style & Ship/Station Design Model
**Voxel aesthetic** (Space Engineers / Avorion) — grimy, industrial. The *look* via **authored designs, not runtime building.** Designs are **baked to optimized meshes** (greedy meshing + atlas) — full voxel look at normal-mesh runtime cost.
- **Authored voxel hull + functional slots** (§8). Hull form and slot layout are part of the design.
- **Visible loadouts:** snap weapon/radiator/module meshes onto sockets at fit time — the ship looks like *yours*.
- **Visible history:** battle damage, scorch, patched plating; wrecks that look wrecked; a flagship that accrues visible history.
- **Stations** use the same kit; tiered upgrades = distinct authored meshes or snap-on modules.

## 25. Procedural Assembly System
Per-faction voxel block-kits drive a procedural generator (biggest tooling investment), kept tractable by: **shape-grammar not random stacking**; **generate to a class spec** (functional slots first); **faction = a parameter set**; **PCG32-seeded & deterministic** (store seed+params, not meshes); **bake after generate**; **hybrid curation** (hand-pick faction signatures, procedural fills the long tail); **offline authoring tool, not runtime**; **pre-generate a pool** for variety (runtime generation is post-MVP). Beyond-gate systems (§17) lean hardest on this generator.

---
---

# PART IV — TECHNICAL ARCHITECTURE

## 26. Engine & Language
**Godot 4.x** as renderer/shell; **Rust GDExtension (`gdext`)** for the sim core as an **engine-agnostic deterministic library** (Rust over C++ for safety, cargo tests, gdext maturity).

## 27. Simulation Core
**Deterministic** (integer / fixed-point math; PCG32 RNG with integer basis-point probabilities); **engine-agnostic** (runs headless; acceptance tests as native binaries); **authoritative** (the sim holds truth; the renderer presents a view).

## 28. Tick & Time Model
**Fixed sim tick + view interpolation** (determinism decoupled from framerate). **Pause/game-speed scale the tick rate** (pause = 0). Backgrounding pauses ticks.

## 29. Sim ↔ View Contract
**Both: snapshot + typed event stream.** Snapshot = current state for rendering; event stream (BattleLog-style) = what happened this tick, consumed by the combat diorama *and* the alert feed (§19).

## 30. Persistence
**Binary (serde + bincode)** — world state + design seed+param tuples, not meshes. **Dev JSON export** for inspection; ship binary. **Version header** for migration. Free-save / multiple slots native.

## 31. Data & Tuning Pipeline
**Hot-reloadable data** (JSON/RON) for all tunable values — economy curves, commodity defs, class specs, faction params. Logic in Rust, numbers in data: **balance without recompiling.**

## 32. Testing Strategy
**Native cargo tests** for sim-core acceptance (economy stability, combat determinism, orbital correctness); **GUT** for the Godot/view + integration layer (extending the GUT 1–21 discipline).

## 33. Platform & Build
**Android-first.** **De-risk the toolchain day one** ("hello from Rust GDExtension" on a physical device before features). **iOS** possible later — out of MVP scope.

---
---

# PART V — SCOPE & EXECUTION

## 34. MVP Scope (Tiers 1–2 + the destination visible)

**In:** inner-system Sol slice; 3 factions + 1 pirate faction; stockpile economy + **interdiction/traffic (prioritized — it's the fun engine)**; the **four warship classes** + **Q-ships** + core **civilian classes**, via the procedural kit with functional fitting and **visible loadouts**; **PDC/torpedo/railgun** weapons; **captain + crew-quality** with the trained-crew bottleneck; doctrine + range-band combat with a **juiced diorama**; light research tree, blueprints, basic reputation, CEO skills; **named crew** attachment machinery; the **alert-feed system** with manager voices; opening missions + **the gate foreshadowed as a visible far-goal**; all three pressures (simplified) with forecasting and biting-but-recoverable decay; **expressive identity**; the **wreck-salvage discovery seed**; a baseline **juice + audio** pass; free-save + optional Ironman; Android.

**Out (sequenced after MVP):** Tier 3 full geopolitics depth; the outer-system frontier & excursions; **the gate, colonization, empire, and incursions (§17)**; multi-system procedural space; full multi-tier living economy; large fleets; deep diplomacy; complex EW; runtime procedural generation; portrait triage; iOS.

## 35. Recommended Build Order (headless-first)
1. **De-risk Rust-on-Android** — hello-world GDExtension on device.
2. **Lock the §0 spine on paper** — the destination pull, the tier transitions, the three-horizon goal stack.
3. **Deterministic core sim** — fixed tick, PCG32, fixed-point, snapshot+event contract, stub orbital model.
4. **Economy & industry** (data-driven) **+ headless stability test** as a gate.
5. **Interdiction prototype early** — prove the §7b fun engine before deep economy polish.
6. **Ship design & fitting** (classes, slots, weapons, crew model; placeholder meshes).
7. **Combat resolver** — headless range-band doctrine sim first, diorama after.
8. **Crew & alert-feed system** — attachment machinery + the voiced feed.
9. **Progression** — research / blueprints / reputation / CEO skills.
10. **Managers & automation** (run-by-exception, exceptions-as-verbs).
11. **Procedural assembly tool** (offline) → catalog + baking pipeline.
12. **Tier-1→2 ascent + gate foreshadowing** — the destination pull made tangible.
13. **Pressure systems** + forecasting/biting-decay + pacing governor.
14. **Juice & audio pass**, then **UX polish** (console, orrery readability + felt vastness, situation report).
15. *(Post-MVP)* Tier 3 geopolitics → outer-system frontier → the gate, colonization, empire, incursions.

## 36. Risks & Mitigations
- **Minute-to-minute fun** *(top risk).* → Exceptions-are-verbs; promote interdiction; prototype the fun loop (step 5) first.
- **Retention / "session 20."** → The §0 spine: a foreshadowed destination + tiers that each change the *kind* of game + emergence + the three-horizon goal stack. Don't let accumulation pretend to be the answer.
- **Tiers becoming "bigger, not different."** → Each transition must hand over a new verb or new kind of stake.
- **Tension going slack.** → "Recoverable ≠ consequence-free"; dominance is real but the gate re-threatens (§13, §17).
- **Crew attachment as a promissory note.** → First-class personality/growth/voice/history (§11), right-sized as support.
- **Living economy complexity.** → Decoupled stockpile pricing; damped prices + bounds; stabilizers; **headless equilibrium test** as a gate.
- **Military balance / power curve.** → Corp, not navy (§8f); torpedoes as the equalizer; peer power gated behind the gate.
- **Endgame scope (multi-system).** → §17 is explicitly post-MVP and procedural; ship single-system Sol first.
- **Juice/audio as an afterthought.** → A deliverable (§23).
- **Alert feed mistuned.** → Ranked, tunable, voiced *system* (§19).
- **Procedural-assembly tooling cost.** → Offline tool; shape-grammar; hybrid curation; pre-generated pools.
- **Mobile UI density.** → "Map + alerts = a complete game"; one-screen-per-job; phone/tablet honesty; portrait triage (post-core).
- **Over-investing in mastery.** → Keep fitting/logistics light (§0.2); the sim rigor is the builder's joy, not player homework.
- **Native iteration speed.** → Hot-reloadable data; logic in Rust, numbers in data.
- **Android toolchain / orbital fidelity / solo scope.** → De-risk day one; simplified orbital model; guard the MVP cut. The honest test is whether *you'd* keep opening it.

---
---

# PART VI — THE EMPIRE LAYER (the re-aim, 2026-06-17)

> This part is **canonical** and supersedes Parts 0–V wherever they disagree on
> *genre and player identity*. It documents the decision to re-aim TORCH toward an
> empire sim and the systems built to deliver it. Everything here is **shipped and
> green** (native `cargo test` + GUT + the QA harness); the sequenced rationale lives
> in `docs/EMPIRE_LAYER_PLAN.md`, `EMPIRE_PHASE2_PLAN.md`, `EMPIRE_DIPLOMACY_PLAN.md`,
> and `POST_GATE_PLAN.md`.

## 37. The Re-aim — from corporate sandbox to empire sim

A vision check found a genuine genre divergence. TORCH had been built *faithfully to
Parts 0–V* — an **X4-style corporate logistics sandbox** (you're a CEO who *perturbs*
a self-sufficient economy and climbs to a gate; Pillar #3: "you are a perturbation on
it, never its foundation"). But the actual north star is a **Distant Worlds /
Stellaris empire sim** in the Expanse's hard-sci-fi Sol: *you grow a colonizing
state.* The **setting** was always right (Expanse, Sol, delta-v, no FTL, Earth/Mars/
Belt, the ring-gate); the **genre/player-identity** had drifted.

**The reconciliation (the new core loop): expansion-by-acquisition.** You grow a
station/colony empire by taking assets three ways — **economy** (buy/build),
**diplomacy** (court independents into joining), and **military** (seize by force) —
governed by an **overextension + faction-alarm** cost: take too much, or take it in
the wrong power's backyard, and the great powers turn on you. Careful, political
expansion *is* the game. The existing economy, fleet, combat, factions, and gate all
become *means and stakes* of one empire loop instead of sitting beside each other.

**Player identity:** still a persistent founder-CEO (Parts 1, 11), but the fantasy is
now running a **polity**, not a trading company — the CEO *is* the state.

**Who is negotiable:** **Earth and Mars are watchful giants** — you don't negotiate
with them, you avoid provoking them (§39). The **independent companies** are the
diplomatic counterparties (§42). Macro decisions (standing relationships, passive
effects), **not** per-event micro-prompts — an explicit design constraint.

## 38. The Acquisition Loop — three paths, three prices

The core verb is **acquire a holding** (a station you build or a colony you take).
Three pathways, each a distinct cost *and* a distinct political price, so *how* you
expand is a real strategic choice:

| Path | Resource cost | Gate | Coalition-alarm spike | Can take |
|---|---|---|---|---|
| **Buy** (economy) | credits | — | +120 (inners) | independent colonies |
| **Annex** (diplomacy) | Influence (+ standing / company ally) | Independents ≥ Cordial, **or** an allied company | +60 (inners) | independent colonies |
| **Seize** (military) | warships + losses | a fleet vs the garrison | **+220 (the victim power)** | **any** colony, incl. a great power's |

- **Holdings** = built stations + controlled colonies (`holding_count`). A unified
  view; the empire's size.
- **Build** is the fourth, foundational path: found stations/refineries (Part 7) on
  your ground — they *produce* (§40).
- Acquisition is a **spine op** (advances the §0 climb).

## 39. Overextension — the two caps that make expansion *careful*

Expansion is never free; two governors, both inert until you actually hold assets (so
the §7c stability gate and the headless economy stay byte-identical for a fresh world):

- **Administrative capacity (the economic cap).** You can govern `base + CEO-level/3`
  holdings efficiently (capacity is *earned*, Stellaris admin-cap style). Past it,
  empire-wide tribute efficiency falls and per-holding **strain upkeep** mounts, so
  over-reach holdings go **net-negative**.
- **Faction alarm & the coalition (the political cap), sphere-aware.** A **per-faction**
  alarm (`faction_alarm[Earth/Mars/Belt]`): the inners are alarmed by your sheer
  **size**; **any** power is spiked by acquisitions/seizures **in its sphere** — taking
  Mars's colony brings *Mars* down on you (the home Belt is alarmed only if you betray
  it by seizing *its* colony). Above a threshold a **coalition** forms (led by the
  angriest power), telegraphs, and lands an **act-now strike** on your holdings;
  unanswered it **seizes your most valuable colony** (which relieves alarm — a
  self-correcting equilibrium where sustainable empire size = the fleet you can field).
  `defend_holdings` rallies the fleet to repel it.

## 40. Economic Integration — holdings as supply-chain nodes

A controlled colony is not a credit drip; it is a node in your economy:

- **Supply.** Each colony produces a thematic raw (Belt→Ice, Mars→Ore, Earth→Volatiles)
  into your **warehouse** every tick. Emergent end-to-end integration: your refineries
  (Part 7) already pull input from the warehouse before buying market, so **colony
  output feeds your production directly** (supply → production → logistics).
- **Owned markets.** A colony you control is a market *you own*: you trade there
  **fee-reduced** (you run the broker), and you collect a **tariff** on every **NPC
  delivery** into it — *your empire earns from the living economy autonomously*.

## 41. Security — a trade empire must be defended

Two threads with **deliberately distinct counters**, scaling with empire size:

- **Piracy (countered by a navy).** As your trade footprint grows, pirates prey on
  *your* shipping unless you keep escorts **on station** (`escorts_needed` ≈ 1 +
  holdings/3). Neglect the navy and a big empire bleeds cargo.
- **Faction inspections (countered by reputation).** Sour a great power and trading in
  its space carries a rising **customs surcharge**, plus periodic **inspection fines**
  while you're on its bad side. Mend fences (contracts, decay) or reroute.

Tuned **real but counterable**: the QA Expansionist (13 holdings) bled to ~37 raids +
11 inspection sweeps yet stayed net-positive — a player who managed escorts + rep
would keep more.

## 42. Corporate Diplomacy — the independent companies

The negotiable actors (Earth/Mars stay the coalition, §39). Each independent colony is
operated by a named **company** (Ganymede Free Traders, Callisto Shipwrights, Enceladus
Hydro Combine, Triton Pioneers) on a stance ladder **Rival < Cold < Neutral < Partner
< Ally**. The one macro move is **court** (invest Influence to climb a step). Passive,
standing-based payoffs — no micro-prompts:

- An **Ally**'s colony **annexes for free** (joins willingly); each ally **lends an
  escort** (diplomacy buys piracy security, §41).
- A **Partner**'s colony is annexable with Influence even at low generic standing.
- **Rival** (made by *seizing* its colony) refuses to deal; a **buyout** just sours it.

So aggression has a diplomatic price: seize widely and you burn bridges with the
independents you might otherwise have allied.

## 43. The Empire Spine & Command Surface

- **Expansion is the spine metric.** An `empire_rank` ladder (Independent Operator →
  Local → Regional → Great Power → Hegemon, by holdings) headlines the status bar and
  the EMPIRE view — "grow the empire" is the legible goal, always showing the next rung.
- **The EMPIRE view** (a fifth nav-rail view, §18) is the "map + master-tables" command
  surface: the rank headline, admin/influence/per-faction-alarm/escort/diplomacy
  meters, the BUY/ANNEX/SEIZE/DEFEND/COURT verb deck, and a master-table of holdings,
  acquirable + seizable colonies (by garrison), and independent relations.
- **PC mode** (§33 amendment): the Android-first shell now auto-detects desktop and
  swaps to mouse-wheel zoom + keyboard (toggle F8), both schemes coexisting.

## 44. Post-Gate Endgame — BUILT (supersedes §17's "post-MVP" framing)

The §17 arc shipped as a complete loop (`docs/POST_GATE_PLAN.md`), every rung
**transit-gated** so the inner game stays byte-identical:

- **G1 — the far side is a place:** a dead-star cluster (Erebus / Threshold / The Tally)
  appended past the ring; hidden until you `transit_gate`.
- **G2 — its economy:** two far-side markets in deep scarcity, stepped on a dedicated
  RNG so the inner economy is untouched.
- **G3 — the bridgehead:** your own foothold beyond the ring (found / upgrade /
  integrity).
- **G4 — incursions:** an escalating threat from beyond (the gate's *answer*) that
  telegraphs, strikes the bridgehead, and is repelled by `defend_bridgehead`.
- **G5 — the resolution:** win by growing + holding the bridgehead through the
  incursions, or lose if it's overrun — the §0 destination pull finally *completes*.

## 45. Build Status & the Determinism Discipline

**Shipped:** the full empire layer (E1–E8), economic integration + security (EP1–EP4),
per-faction geopolitics (E7), corporate diplomacy (E8), the post-gate sandbox (G1–G5),
delta-v warship movement, the multi-view command shell incl. the EMPIRE view, and PC
mode. ~186 native sim tests + the QA harness (7 personas incl. an **Expansionist** that
exercises the empire loop) + 17 GUT view tests, all green.

**The discipline that made it safe:** every empire/endgame system is **gated** (on
holding assets, or on `transit`), so a fresh world and the non-expanding QA personas
are **byte-identical** — the §7c economy stability gate is provably unaffected, and the
QA review only moves for the persona that actually expands (regenerated honestly). New
subsystems carry **dedicated RNGs** and append (never insert) so load-bearing indices
and the shared RNG stream are untouched. Content stays in code; only mutable dials are
save state (the `&'static str` serde wall).

## 46. Next Steps (candidate rungs, macro-first)

> **Live tracking has moved to `docs/STATE_OF_THE_GAME.md`** (the consolidated backlog).
> The list below is preserved as the original design intent; the backlog is where open
> work is prioritized and ticked (and notes that "war as a state" + the narrative pass are
> now folded into the mid/late-game arc, `docs/MID_LATE_GAME_STORY.md`).

Sequenced, each a small focused PR in the established style:

- **Living diplomacy payoffs** *(macro):* allied companies route more NPC trade to your
  owned markets (passive tariff income); rival companies undercut/contest you (passive
  economic friction) so rivalry has a downside beyond "can't annex."
- **A Diplomat QA persona** — courts companies to allies and annexes the frontier
  peacefully; the first persona to exercise the diplomacy loop in the review.
- **Pops / colony development** *(the one classic 4X axis still absent):* a light
  population/development tier per holding (output + a few macro policies), kept
  un-fiddly per §0.2 — the deepest remaining empire-sim gap.
- **War as a state** *(macro):* a sustained war footing with a great power (vs. the
  one-off seize), with war goals and a peace.
- **The art track** *(independent):* the §24/§25 procedural voxel assembly + baking
  pipeline (the biggest single art lift), then the §22 voxel combat diorama.
- **Audio:** deferred indefinitely by player choice (§23c) — the one consciously
  dropped MVP item.
