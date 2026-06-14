# TORCH — Game Design Document

> Working title. Alternatives: *REMASS*, *The Long Burn*, *Torch & Tonnage*, *Sol Incorporated*, *Delta-V*.
>
> **Personal project — no monetization, no market constraints.** Scope discipline here serves one goal: what's fun to build and finishable solo. Build the game you want to play.

A hard sci-fi industrial sandbox for mobile. You found a corporation in the Sol system and build a real vertical supply chain — inside a living economy where Earth, Mars, and the Belt each have their own needs and politics. Real-time with pause. Offline. The war is mostly logistics.

---

## 1. High Concept

You are a persistent founder-CEO, not a ship captain. Starting from one station or claim in a single, fully realized Sol system, you build the chains that keep civilization breathing — ice into reaction mass, ore into hulls, fissionables into fuel — and you defend those chains across real orbital distances. There is no FTL. Travel costs time and delta-v. Ships are fragile and lethal. Earth, Mars, and the Belt are sliding toward open war, and you are the supplier, the opportunist, and eventually the power broker in the middle of it.

**Tagline:** *Logistics is the weapon. Time is the terrain.*

**One line:** X4's industrial heart × The Expanse's scarcity politics × an authored story spine — on a phone, offline.

---

## 2. Design Pillars

1. **Time is terrain.** A live orrery map. Bodies move on their orbits. Every fleet movement is a committed trajectory with a real travel time and a delta-v cost.
2. **Delta-v is the universal constraint.** Reaction mass and reactor fuel are the true limiters, not abstract energy. Refueling infrastructure is strategic ground.
3. **The economy is alive and you are inside it.** You build a vertical supply chain, but a self-sustaining NPC economy runs around you — and you are a perturbation on it, never its foundation.
4. **Combat is doctrine first, physics always.** Design ships, set doctrines, nudge battles with a few live commands. Newtonian and lethal. The deaths land on the people you hire, not on you.
5. **Calm to build, tense to hold.** Contemplative construction by default; layered, *telegraphed* pressure keeps the calm fragile but never unfair.

---

## 3. Player Fantasy & Arc

Turn a single industrial foothold into the dominant private power in Sol.

- **Early** — one station/claim. Stand up your first supply chain, hire your first manager, survive your first raider.
- **Mid** — a logistics web across multiple bodies, your first warships, reputation leverage, blueprint hunting.
- **Late** — a corporate power whose embargo or supply decision can tilt the cold war; eyes on the dormant ring-gate.

The player is **independent** by default — navigating Earth, Mars, and the Belt rather than belonging to any of them, though reputation and CEO perks can pull you toward a faction.

---

## 4. Setting & Factions

Hard sci-fi, present-tense Sol. No aliens (until the gate question), no magic, no shields. Texture: scarcity, radiation, vacuum, politics.

- **Earth / inner authority** — populous, resource-hungry, bureaucratic, militarily strongest. Deepest markets.
- **Mars / militarist republic** — high-tech, disciplined, expansionist. Best shipyards and drive tech.
- **The Belt / Belter coalition** — decentralized, resource-rich, fragile life-support, resentful of the inners. Industrial frontier and your natural home turf.
- **Plus:** rival/partner corporations and **raider factions** preying on weak lanes.

The three-way cold war is a living tension meter; flashpoints create embargoes, warzones, and lucrative-but-dangerous contracts.

---

## 5. Core Loops

**Moment-to-moment (paused planning):** read the system → spot a need or opportunity → queue trajectories, production, trades, builds, policies → unpause → watch it unfold → re-pause to react.

**Session:** advance the industrial web, run a contract or trade run, fight or avoid an engagement, invest in research/blueprints/reputation/CEO perks, respond to a flagged crisis.

**Campaign-long:** rise from one claim to a Sol-spanning corporation; shift the cold war; uncover and confront the ring-gate.

---

## 6. Time & Travel Model

- **Real-time with pause** (X4-style). Runs while open; pause freely to issue orders. **The clock pauses when the app is backgrounded** — you never lose a fleet while away. (Open: time-compression baseline + adjustable game-speed.)
- **Orbital map as a live orrery.** Bodies/stations on simplified, deterministic orbits (precomputed / patched-conic, **not** full n-body — fidelity serves playability).
- **Trajectory choice per movement:** economical (slow, low delta-v, Hohmann-like) vs. hard burn (fast, expensive, taxes the crew). Transfer windows make some routes cheap only at certain alignments.
- **Delta-v budget per ship** set by remass tankage and drive efficiency. Running dry strands you — fuel logistics is a real failure mode.

---

## 7. Economy & Industry

The deepest system in the game and the biggest engineering lift. **Build simplified for MVP, deepen over time.** Two decoupled layers:

### 7a. Pricing backbone — stockpile simulation (cheap)
Each market holds real inventory that fills and drains from NPC production/consumption; prices track stock levels. Emergent shortages without agent-based cost.

### 7b. Physical traffic layer — interceptable haulers
NPC haulers fly representative real routes and **can be intercepted by you or by pirates.** Interdiction feeds back into the stockpiles — cut a convoy and you cause a *local, temporary* shortage that moves prices and can trigger a scarcity event. The X4 thrill of raiding cargo flows without full agent-based market math.

### 7c. Stability design (anti-death-spiral)
The economy must reach **stable equilibrium with zero player input** — it ran before you existed.

- **Damped pricing:** price *lerps* toward a stock-based target each tick — never raw supply÷demand (that ratio is what oscillates into a spiral).
- **Hard floors and ceilings** per commodity; crises push toward the ceiling, never past it. Nothing goes to zero or infinity.
- **NPC stabilizers:** strategic reserves and emergency production auto-trigger when a stockpile crosses a threshold; consumption throttles under scarcity. The market actively seeks equilibrium.
- **Locality:** disruptions are local and self-correcting — NPCs reroute and draw down reserves over time. The player can dent the system, never collapse it by existing.
- **Acceptance test (headless):** the economy is a deterministic headless sim. GUT criterion — *no market may death-spiral across thousands of ticks with no player present, on any seed.* If it isn't stable while empty, it isn't done.

### 7d. Commodities & chains (illustrative)
- **Raw:** ice/water, ores, volatiles (CHON), fissionables / He-3.
- **Refined:** reaction mass, oxygen/water, refined metals, reactor fuel, feedstock.
- **Components:** hull plate, frames, electronics, drive parts, munitions.
- **Assembled:** ships, station modules, torpedoes, life-support units.

Chains: mine → refine → manufacture → assemble. **MVP:** ~6–8 commodities, 2–3 tiers, price response on key goods only.

---

## 8. Ships & Design

**Modular hull-slot design** (not voxel — too heavy for solo/mobile). Pick a hull class, fill its slots, live with the tradeoffs.

- **Hull class** sets mass budget and slot count.
- **Slots:** drive (thrust vs. efficiency), reactor (power + heat), **radiators** (shed heat), reactor-fuel tank, remass tank (→ delta-v), weapons, magazines, armor, cargo, crew, sensors/EW.
- **Core tradeoffs:** mass vs. delta-v; heat vs. sustained fire; cargo vs. teeth.

**Scale: mixed fleets, corvettes → capitals.** Cheap corvettes screen; rare capitals are major investments. Role composition matters — escorts, haulers, torpedo boats, railgun line, command ships.

---

## 9. Combat

**Doctrine + light tactical input**, Newtonian and lethal. No shields; armor and positioning decide outcomes.

**Tactical model — abstract range bands.** You fight in close / medium / long brackets and manage *which band* you engage in, not pixel positions. Touch-clean, and choosing your band is effectively a doctrine setting.

**Hard sci-fi primitives:**
- **Torpedoes** — slow, expensive, must *saturate* enemy point-defense. The alpha-strike threat.
- **PDCs** — down torpedoes and brawl in the close band.
- **Railguns** — high-velocity hull-killers needing firing solutions; dominate the long band.
- **Heat** — weapons/reactors generate it; radiators shed it; overheating forces a throttle-down or lights you up.
- **Drive plume & running dark** — burning hard makes you visible; stealth is a real tradeoff.
- **Crew g-tolerance** — hard burns degrade crew; "the juice" mitigates.

**Doctrine (preset):** engagement band, torpedo discipline, PDC priority, heat ceiling, target priority, retreat threshold.
**Live commands (a few):** focus fire, launch salvo, flip-and-burn / retreat, go dark, brace.

**Lethality:** ships die hard and stay dead. Wrecks are salvageable — feeding blueprints and materials.

---

## 10. Progression (four layered tracks)

- **Research tree** — invest money + lab capacity to unlock module tiers and efficiencies.
- **Blueprint discovery** — acquire designs by purchase, **salvage of wrecks**, or **reverse-engineering** captured/derelict ships. Combat and exploration feed industrial growth.
- **Faction reputation** — standing with Earth/Mars/Belt/corps/pirates gates tech catalogs, contracts, station access, and prices.
- **CEO skill track** — your persistent founder grows via a **mix** of passive corp buffs (logistics, negotiation, R&D speed, manager effectiveness), a few unlock gates (ship classes, station tiers, contract types), and a **defining perk branch**: **Industrialist / Trader / Warlord / Diplomat.** The perk branch also tilts how you meet the three pressures — a Diplomat defuses faction war, a Warlord thrives in it.

---

## 11. Managers, Crew & CEO Identity

- **Managers and captains are named characters** with skills, quirks, and loyalty. You recruit them, develop them, assign them to run stations and command ships. An HR/officer layer that feeds the narrative.
- **The CEO is persistent and immortal**, growing through the skill track above. No succession/dynasty.
- **The key asymmetry:** *you can't die, but the people you hire can.* A veteran captain lost in a bad burn is a real, permanent wound — that's where the lethality's emotional weight lives, not on a game-over screen. Lean into it.

---

## 12. Automation — Run by Exception

**High automation / set-and-forget managers.** You set policy; managers and fleet AIs execute trade, mining, and defense autonomously.

This reframes the whole game as **policy + exceptions:** the interesting decisions are strategic setup, crisis handling, expansion, combat, research, and diplomacy. The routine runs itself. **Rule:** managers handle the routine, never the judgment. The situation-report loop becomes *"here's what your managers did, and where they need you."*

---

## 13. Pressure Systems (all three, layered) — Tense, Not Stressful

- **Faction war** — the cold war heats and cools dynamically; flashpoints spawn embargoes, warzones, high-value contracts. Danger and opportunity in one event.
- **Piracy & raiders** — undefended lanes/stations get hit; escorts, defense, patrols, bounties matter. Escalates if ignored.
- **Survival & scarcity** — life-support is fragile; stations and colonies consume remass, air, food, fuel. Disruption can spiral into unrest and collapse.

**Anti-stress design (the difference between tension and anxiety):** stress comes from *unforeseeable* and *unrecoverable* loss. Both are designed out.

- **Forecasting UI** kills the unforeseeable: every station shows "at current burn, remass runs dry in N days." You always see the cliff coming.
- **Staged, reversible decay** kills the unrecoverable: rationing → unrest → emigration → collapse, recoverable at every stage until the last. Nothing dies instantly.
- **Pause is the valve** — you never need speed, only judgment.
- **Pacing governor** — a light director keeps the three pressures from all spiking at once.
- **Opt-in intensity** — difficulty settings let players dial pressure aggressiveness (see §14).

---

## 14. Failure, Permanence & Difficulty

- **Free saving / multiple slots** by default — experiment freely, low stress, build-focused. (No hard game-over; bankruptcy is a setback to recover from, not an end screen.)
- **Optional Ironman toggle** at campaign start — one save, permanent losses, EVE-grade stakes — for players who want enforced permanence. One toggle, both audiences.
- **Pressure-intensity setting** — independent of save model, for dialing how aggressive war/piracy/scarcity feel.

---

## 15. Narrative Campaign

A story spine that **also teaches the systems**, then opens into sandbox.

- **Structure:** a short act-based campaign (~5–8 missions for MVP) following your corporation's rise amid the cold war. Each mission introduces one system: first station, first supply chain, first ship build, first combat, first hire, first reputation choice, first crisis.
- **Themes:** loyalty vs. profit, inners vs. Belt, the cost of keeping people breathing.
- **Payoff:** ends near the **discovery of the dormant ring-gate** — the hook into expansion and open sandbox.

---

## 16. Ring-Gates (Post-MVP Expansion Horizon)

Designed in now, built later. A slow, ominous, Expanse-style gate that, once activated, opens a second system and the first hint of the unknown.

> **Tone flag:** the gate is the one place the pure hard-sci-fi grounding bends. Keep it rare, costly, and ominous — not a casual fast-travel network. The long-term content engine, **not** part of MVP.

---

## 17. Presentation & UX

- **Art:** low-poly 3D, orthographic/diorama framing — plays to existing strengths. Clean readable silhouettes; the orrery as a glowing schematic.
- **Art direction note:** a diegetic "corporate terminal" framing helps compress dense data into a coherent, readable aesthetic.

### UI architecture (the hardest problem — solved by one rule)
**The orrery map + the alert feed must be a complete, playable game on their own.** Everything else is opt-in depth.

- **One screen per job, never nested:** orrery (home/hub), station, ship bay, market, research, HR/officers, combat — each a top-level destination reachable in one tap from the map.
- **Progressive disclosure:** high automation means the default view is summary + exceptions; you drill into detail only to design or expand.
- **Alert feed as spine:** the UI pushes only what needs attention (forecasts, threats, manager flags, contract offers).
- **Situation report on resume:** a digest of what changed while away.
- **Portrait-first:** tap-to-expand cards and lists, not desktop spreadsheets.
- **Cheap combat UI:** range-band brackets + a handful of command buttons, not a spatial RTS.

---

## 18. MVP Scope (The Honest Cut)

Prove the core loop before adding depth.

**In:**
- A **slice of Sol**: Earth/Luna, Mars, Ceres, a few belt asteroids, one outer moon.
- **3 factions + 1 pirate faction.**
- **Economy:** stockpile backbone + light interceptable traffic, ~6–8 commodities, 2–3 tiers, stability-tested.
- **Ships:** ~6–8 hull classes, corvette → capital, modular slots.
- **Combat:** doctrine + range-band tactical, watchable and auto-resolvable.
- **Progression:** small research tree, blueprint acquisition, basic reputation, CEO skills.
- **Managers:** named hires with traits.
- **Narrative:** a 5–8 mission teaching campaign + sandbox handoff.
- **Pressure:** all three, simplified, with forecasting + staged decay.
- **Save:** free-save default + optional Ironman.

**Out (for now):** ring-gates / second system; full multi-tier living economy; large fleets; deep diplomacy; complex EW.

**Recommended build order (headless-first, mirrors TAKE AND HOLD):**
1. Deterministic core sim — time, orbits, delta-v travel.
2. Data-driven economy & industry (JSON commodities/recipes) **+ headless stability test.**
3. Ship design system.
4. Combat resolver (headless doctrine sim first, visuals after).
5. Progression (research / blueprints / reputation / CEO skills).
6. Managers & automation layer.
7. Narrative wrapper + tutorialization.
8. Pressure systems + forecasting/decay.
9. UX polish + situation-report loop.

---

## 19. Risks & Mitigations

- **Living economy complexity** *(biggest risk).* → Decoupled cheap stockpile pricing; damped prices with floors/ceilings; NPC stabilizers; **headless equilibrium test as acceptance criteria.** Ship simplified first.
- **Mobile UI density.** → "Map + alerts = a complete game"; one-screen-per-job; progressive disclosure; run-by-exception automation; cheap range-band combat UI.
- **Tense slipping to stressful.** → Forecasting (no unforeseeable loss) + staged reversible decay (no unrecoverable loss) + pause valve + pacing governor + opt-in intensity.
- **Time behavior.** → Clock pauses when backgrounded; confirm compression baseline + game-speed control.
- **Orbital fidelity vs. playability.** → Lock the simplified orbital model early; resist n-body.
- **Tutorialization of a deep sim.** → The campaign carries the curve, one system per mission.
- **Solo scope.** → The MVP cut is the safeguard. Guard it. It's your game — finish a playable core before expanding.
