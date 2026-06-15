# TORCH — Player Influence & Interaction Model

> Companion to the Unified Design Document. This doc answers one question concretely: **what can the player influence, by what mechanism, and where in the UI is it pressable?**
>
> **Identity:** a *spreadsheet simulator in space* — depth of decision is the fun. Aurora 4X and EVE are the patron saints.

---

## 1. The Interaction Spine

Three locked decisions define how all influence flows:

- **Hybrid control — map + tables.** The orrery handles *spatial* selection and orders; sortable **master tables** handle deep management. Tapping a thing on the map and selecting its row in a table lead to the *same* place.
- **Parameterized standing orders.** Automation isn't scripted logic and isn't dumb dropdowns — every controllable unit runs a **behavior preset with tunable parameters**. You tune; the sim executes; deviations surface as exceptions.
- **Function tabs + object context panels.** Global tabs (each a master table) for overview and bulk action; a slide-over **context panel** for the specific object you selected.

**The three levels of agency** (every interaction is one of these):
1. **Policy** — set standing orders; the routine runs itself.
2. **Initiative** — proactive moves you choose (expand, raid, fit, research, court a faction).
3. **Exception** — reactive responses to what the world throws (alerts → verbs).

---

## 2. Control Surfaces (where influence lives)

| Surface | Role | What's pressable |
|---|---|---|
| **Orrery (map)** — home view | Spatial selection & spatial orders | Tap any body/station/ship/contact to select; issue move / raid / escort / rally orders; plan trajectories (drag a burn, see the ghost arc + arrival readout) |
| **Function tabs** (global master tables) | Overview + bulk management | Sort/filter/select rows; batch-apply orders; drill into any row |
| **Context panel** (slide-over, right) | Per-object detail & management | The selected object's stats, actions, and **standing-order config**; appears from map *or* table selection |
| **Assets panel** (slide-over, left) | Quick navigation | Your fleets / stations / contracts tree for fast select |
| **Alert feed** (top bar / drawer) | Exceptions → verbs | Ranked alerts; tapping an act-now alert jumps to the relevant object with the action primed |
| **Top bar** | Time & vitals | Pause / game-speed, date-clock, headline resources, **gate-progress indicator** |

**The function tabs (each a sortable master table):**
- **Overview / Empire** — KPIs, the situation report, tier progress, distance-to-gate.
- **Production** — every station: recipes, throughput, stockpiles, upkeep, defenses.
- **Market / Trade** — prices across markets, your goods, your sell-prices, contracts, trade routes.
- **Fleet** — every ship: class, fit, location, assignment, crew quality, remass, standing order.
- **Personnel** — captains & managers: skills, traits, loyalty, assignment, service history.
- **Research** — tech tree, active projects, lab capacity, queue.
- **Diplomacy** — faction reputations, the cold-war meter, flashpoints, contracts on offer.

---

## 3. The Influence Catalog

What the player influences → how → where → and whether it's a standing order. This is the master list of levers.

### 3.1 Industry & Stations
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Found / claim a station | Pick a site, commit capital + materials | Orrery → build dialog | One-time |
| Upgrade station tier | Spend materials/capital | Context panel · Production tab | One-time |
| Set production | Choose recipe + output targets | Context panel · Production tab | **Standing** (preset + params) |
| Input priority / throughput | Tune parameters | Production tab | **Standing** |
| Sell-surplus rule | Threshold + market | Context panel | **Standing** |
| Station defenses | Assign turrets / garrison / patrol | Context panel | **Standing** |

### 3.2 Logistics & Fleet Movement
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Move a ship/fleet | Select → destination → trajectory (economical / hard burn) | Orrery | One-time |
| Mining assignment | Preset "Mine [resource] @ [body] → [station]" | Context · Fleet tab | **Standing** (resource, site, dropoff, remass reserve) |
| Trade/haul route | Preset "Haul [commodity] [A→B]" | Fleet tab · Context | **Standing** (commodity, route, min-margin, threat response) |
| Refuel / remass policy | Reserve threshold + depot | Context | **Standing** (keep ≥ X% remass) |
| Military posture | Preset patrol / escort / garrison + zone | Orrery + Context | **Standing** (zone, doctrine, retreat threshold) |

### 3.3 Markets & Trade
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Spot buy/sell | Order on a market | Market tab · Context (market) | One-time |
| Set your sell price | Tune price/auto-price rule | Market tab | **Standing** |
| Faction contract | Accept / fulfil / break | Diplomacy · Market tab | One-time |
| Embargo / withhold | Toggle supply to a buyer or faction (leverage) | Market · Diplomacy | **Standing** |
| Speculate | Buy ahead of a shortage (often one you caused) | Market tab + judgment | One-time |

### 3.4 Interdiction & Raids — *the fun engine*
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Scout / identify convoys | Read sensor contacts | Orrery | — |
| Order a raid | Select strike fleet → "Raid" → designate convoy → plan intercept (lead the moving target) | Orrery | One-time |
| Raid posture | Toll / board-capture / destroy | Context (the engagement) | Per-raid choice |
| Standing piracy | Preset "Patrol & interdict lane X" | Fleet tab · Orrery | **Standing** (lane, ransom threshold, avoid-navy rule) |
| Exploit the aftermath | Trade into the shortage you caused | Market tab | One-time |

### 3.5 Ship Design & Fitting
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Acquire blueprint | Buy / salvage / reverse-engineer | Research · Context (wreck) | One-time |
| Fit a hull | Assign modules to slots (mass/delta-v/heat tradeoffs) | **Ship Bay** (fitting screen) | One-time per design |
| Save a fit | Store as reusable template | Ship Bay | Reusable |
| Build ship | Queue at a shipyard | Production · Context (shipyard) | One-time |
| Name / livery | Personalize | Ship Bay · Fleet tab | One-time |

### 3.6 Combat
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Doctrine | Preset + params: band, torpedo discipline, PDC priority, heat ceiling, target priority, retreat threshold | Context · Fleet tab | **Standing** |
| Engage or avoid | The strategic call on contact | Orrery · alert | Per-encounter |
| Live tactical commands | Focus fire / launch salvo / flip-and-burn / go dark / brace | Combat view | Live |
| Cut losses | Manual retreat override | Combat view | Live |

### 3.7 Crew & Personnel
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Recruit | Hire captain/manager from a pool | Personnel tab | One-time |
| Assign | Captain → ship, manager → station | Personnel tab · Context | One-time |
| Grow trained-crew capacity | Invest in the crew pipeline | Personnel tab | **Standing** (investment) |
| Personalize | Rename | Personnel tab | One-time |

### 3.8 Research & CEO Progression
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Research priority | Order/queue the tech tree | Research tab | **Standing** (queue) |
| Lab capacity allocation | Tune | Research tab | **Standing** |
| CEO skill / perk branch | Allocate; pick Industrialist / Trader / Warlord / Diplomat | Overview / CEO panel | One-time choices |

### 3.9 Diplomacy & Politics
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Tilt reputation | Contracts, supply choices, actions | Emergent + Diplomacy tab | Ongoing |
| Align / stay neutral | Strategic stance | Diplomacy tab | **Standing** stance |
| Respond to flashpoint | Accept contract / pick a side / exploit | Alert → Diplomacy | Per-event |

### 3.10 Exploration & Excursions
| Lever | Mechanism | Surface | Standing? |
|---|---|---|---|
| Survey / salvage | Designate target (derelict / anomaly / frontier) | Orrery | One-time |
| Mount an excursion | Assemble task force + objective + supply plan | Orrery · Fleet tab | One-time (major op) |
| Pursue the gate mystery | Follow leads | Emergent + Overview | Ongoing |

---

## 4. The Standing-Orders Model (parameterized)

The heart of the spreadsheet-sim. Every controllable unit carries a **Behavior Preset + Parameters**. The player tunes the fields; the deterministic sim executes; anything the preset can't handle (blocked, threatened, depleted, unprofitable) surfaces to the **alert feed** as an exception.

**Template:** `Unit → [Preset ▾] → tunable parameter fields → (sim executes) → exceptions to feed`

**Examples:**

| Unit | Preset | Parameters you tune |
|---|---|---|
| **Hauler** | Trade Route | origin, destination, commodity, min profit margin, remass reserve %, threat response (flee / call escort) |
| **Miner** | Extract & Deliver | resource, extraction site, dropoff station, remass reserve %, full-hold behavior |
| **Refinery** | Produce | input recipe, output stockpile target, input priority, sell-surplus threshold + market |
| **Warship** | Patrol / Defend | zone, engagement doctrine, retreat threshold, avoid-superior-force toggle |
| **Strike group** | Interdict | lane, target filter (cargo value), ransom-vs-destroy, navy-avoidance |
| **Station** | Operate | production presets (above) + defense posture + upkeep priority |

Presets are the **policy** layer (§1). Tuning them is initiative. When one trips, that's an exception — and the alert resolves *into* the relevant context panel with the fix primed.

---

## 5. Touch & Interaction Flow

- **Select:** tap a map object **or** a table row → the **context panel** slides in (right). Same destination, two routes.
- **Spatial order:** with a fleet selected, tap a destination/target on the orrery, or use the context panel's action buttons.
- **Bulk:** multi-select rows in a table (checkboxes) → a **batch-action bar** appears (apply order / set preset / assign to all).
- **Sort & filter:** every master table sorts and filters (by location, status, profit, threat, crew quality…). This *is* the spreadsheet feel.
- **Standing order:** set in the context panel — preset dropdown + parameter fields.
- **Exception → verb:** tap an act-now alert → jump to the object with the action ready.

**Phone vs. tablet:** tables shine on a tablet (multi-column, side-by-side with the map). On a 6-inch phone, tables collapse to single-column cards and the map and a panel are rarely shown together — the **alert feed + Overview** become the primary triage surface, full tables when you want depth.

---

## 6. Worked Examples (end-to-end)

**A. Stand up a mining → refining → sell chain.**
Orrery: tap an ice-rich asteroid → context → "Claim/Build mining station" (commit materials). Tap your refinery station → context → set Produce preset (ice→remass, stock target 10k, sell surplus above 8k @ Ceres). Fleet tab: select a hauler → Trade Route preset (remass, refinery→Ceres, min margin 12%, flee on threat). Done — it runs; you'll only hear about it if it breaks.

**B. Plan and run a raid.**
Orrery: spot a Martian convoy on a lane. Select your frigate group → "Raid" → tap the convoy → the intercept solver draws a ghost arc to its *future* position with a delta-v/time cost; confirm the burn. On contact, the combat diorama opens; set posture (toll vs board vs destroy), issue a live salvo, cut and run before the navy responds. Aftermath: Market tab shows the Ceres remass price spiking from the shortage you just caused — sell into it.

**C. Respond to a scarcity alert.**
Feed: "Ceres remass dry in 6 days (manager: Vela)." Tap it → jumps to Ceres' context panel showing the broken supply line. Options surfaced: reroute a hauler, raise the buy price to pull NPC supply, or dispatch reserve. Pick one; the staged-decay clock resets.

**D. Fit and deploy a warship.**
Personnel tab: assign a veteran captain to a new cruiser hull. Ship Bay: fit it (turreted railgun, heavy armor — accepting the delta-v hit), save the template, name it, apply livery. Production: queue the build at your shipyard. Fleet tab: when complete, set a Patrol/Defend standing order on your home lane.

---

## 7. Information Architecture Principles

- **Map + alerts = a complete game.** A player can run everything from the orrery and the feed alone; the tables are opt-in depth.
- **One screen per job, never nested.** Each function tab is a top-level destination; the context panel is the universal "act on this" surface.
- **Progressive disclosure.** Default views are summary + exceptions (automation handles the rest); drill in only to design or expand.
- **Two routes to everything.** Spatial (map) and tabular (tabs) both reach the same context panel — pick whichever fits the moment.
