# TORCH — Playable-State Review & Path Forward

> **Date:** 2026-06-15
> **Scope:** Assessment of the current build against the Unified Design Document
> (`TORCH_Unified_Design_Document2.md`) and a concrete, sequenced path to a
> **playable full MVP** (GDD §34, the Tiers 1–2 slice with the gate visible).
> Snapshot: branch `claude/playable-state-review-jwfa9f`, 67 native tests green,
> Android APK pipeline green.

---

## 1. Executive summary

TORCH has a **strong, mature, deterministic simulation core** and a **proven
Android build pipeline** — the two hardest *technical* de-risking items in the
GDD are done. What it does **not yet have is a game**: the Godot shell is still
the hello-world text dump, there is **no player-agent state** (no treasury,
owned ships, station, or contracts), and **none of the §0 "soul"** (destination
pull, tier ascent, three-horizon goal stack) exists in code.

In one line: **we have built a convincing NPC *world*, but not the *player's
place in it*, and not the screen you look at.**

The roadmap (CLAUDE.md §6) reads as ~9/15 steps done, but those nine are almost
entirely **headless sim mechanics in isolation**. The remaining work is where
the game actually becomes playable: a player economic loop, a presentation
layer, and the retention spine. This review re-frames the path around those.

| Area | State | Verdict |
| --- | --- | --- |
| Deterministic core, RNG, fixed-point (§27) | Done | Solid foundation |
| Android APK / toolchain (§33) | Done | De-risked |
| Economy + anti-death-spiral gate (§7a/7c) | Done | Strong, well-tested |
| Interdiction + traffic + pirates (§7b) | Done | The "fun engine" works headless |
| Ships / fitting / crew (§8) | Done | Data-as-Rust, no persistence |
| Combat resolver (§9) | Done (headless) | No diorama (§22) |
| Alerts (§19), factions (§4), progression (§10) | Done (headless) | Wired into sim |
| **Player-agent state (treasury/fleet/station)** | **Missing** | **Foundational gap** |
| **Presentation / UI / orrery (§18–§22)** | **Hello-world only** | **Biggest gap to "playable"** |
| **§0 spine: destination, tiers, goal stack** | **Missing** | **Over-invest priority, not started** |
| Persistence (§30) | Missing | No serde/bincode dep |
| Data-driven tuning (§31) | Missing | All numbers hardcoded in Rust |
| Trajectory planning verb (§6, verb #4) | Stub | Straight-line travel, no transfers |
| Automation / managers (§12) | Missing | |
| Procedural assembly + art (§24/§25) | Missing | Placeholder meshes only |
| Pressure/forecasting/pacing (§13) | Missing | |
| Juice & audio (§23) | Missing | "Deliverable of fun" |

---

## 2. What is genuinely solid (keep building on this)

- **Determinism is real and enforced.** `Pcg32` with bias-free `below()`/integer
  `chance_bp()`, integer Bhaskara trig (`fixed.rs`), no floats in any probability
  or position path. `same_seed_yields_identical_runs` checks step + snapshot
  equality over 600 ticks. This is the bedrock the GDD demands (§27) and it is
  honoured everywhere.
- **The economy is the highlight.** Piecewise damped pricing, decoupled
  stabilizer setpoints producing standing spreads, hard stock walls, and the
  `no_death_spiral_on_any_seed` gate (64 seeds × 5000 ticks) — *re-run with
  traffic + pirates* (`no_death_spiral_with_traffic_on_any_seed`). This is
  exactly the §7c discipline, and the hard-won tuning lessons are captured.
- **The §7b loop closes end-to-end (headless).** Arbitrage haulers fly the
  orrery → deliveries damp spreads → a cut (`interdict_with`, real
  intercept-geometry + odds) starves the destination → a `Scarcity` event →
  the alert feed voices it as an **act-now verb** → a *player* cut ripples
  reputation while *pirate* raids don't. The control-vs-cut test isolating a
  single denied delivery is a genuinely good piece of test design.
- **Clean architecture.** The `sim` boundary is strict: pure Rust, no `godot`
  imports, fully native-testable; `lib.rs` is a thin scalar binding. The
  snapshot + event-stream contract (§29) is live. This will pay off the moment a
  real view sits on top.

The builder's-joy half of the project (§0.2) is in excellent shape.

---

## 3. The three gaps that block "playable"

Everything below §3.3 is sequenced after these. These are the load-bearing
missing pieces.

### 3.1 No player-agent state — the foundational gap

`Sim` models the **NPC world**: bodies, markets, NPC haulers, NPC pirates,
relations, and an abstract progression. It contains **no representation of the
player as an economic actor**:

- no **treasury / credits**;
- no **player-owned ships** (the shipyard is a static catalog demo);
- no **player-owned station or production** (markets are NPC-owned);
- no **contracts**, jobs, or any way to *earn* or *spend*.

`"player"` appears in the code only in the reputation ripple and in tests. The
player can *perturb* the world (cut a hauler, which the GDD wants — §2 pillar 3)
but cannot **build, own, trade for profit, or accumulate** anything. Without
this there is no core loop (§5), no Tier-1 station puzzle (§0.3), and nothing
for progression/automation to govern.

> This is the single most important thing to build next. It is a sim-core task
> (headless, testable) and unblocks almost everything else.

### 3.2 No presentation layer — the most *visible* gap

`godot/main.gd` steps the sim 240 times in `_ready()` and prints a text dump to
one `Label`. There is:

- no **game loop** driving `step()` over time (no `_process`, no pause/speed —
  §28 says pause/game-speed scale the tick; the shell does none of it);
- no **orrery** (§21) — the live 3D orbital scene that "owns the screen";
- no **command console / panels / top bar / alert feed UI** (§18, §19);
- no **input** — nothing is interactive; you cannot select, tap, or issue an
  order;
- no **combat diorama** (§22).

The sim exposes the data (snapshot accessors are bound); nothing renders or
reacts to it as a game.

### 3.3 No §0 spine — the retention soul is absent

The GDD is emphatic (§0.2): **over-invest** in the destination pull, tier
ascent, and three-horizon goal stack. None of it exists in code:

- no **gate** entity, foreshadowing, or far-goal;
- no **tier** model or transition milestones (§0.3);
- no **goal stack** (now / tier / far — §0.4) surfaced anywhere.

Roadmap step 2 ("lock the §0 spine on paper") and step 12 ("ascent + gate
foreshadowing") are both untouched. This is *the* retention risk the GDD calls
out (§36), and it is currently a blank.

---

## 4. Secondary gaps (real, but sequenced after §3)

- **Persistence (§30).** No `serde`/`bincode` dependency, no save/load, no
  version header. Free-save/slots is a named MVP feature. Required before the
  game is "a game you return to."
- **Data-driven tuning (§31).** All commodity/ship/combat numbers are hardcoded
  in Rust (`CommodityDef` even has a comment promising RON/JSON "later"). The
  GDD wants balance without recompiling; roadmap step 4 explicitly lists this
  as todo. Lower urgency than §3 but cheap and high-leverage for iteration.
- **Trajectory planning (§6, verb of joy #4).** The orbit model is circular and
  evaluated directly from tick (good, deterministic), but travel is a
  **straight line at a fixed `CRUISE_SPEED`** — no transfer windows, no
  Hohmann/patched-conic, no per-ship delta-v *consumed* on a route, no
  economical-vs-hard-burn choice. One of the four named "verbs of joy" is
  effectively absent. The TS prototype reportedly had a Hohmann model to port.
- **Automation / managers (§12, step 10).** Policy-driven, run-by-exception
  execution does not exist; the alert feed (the exception *surface*) does.
- **Combat diorama (§22)** — combat is headless-only; the watchable payoff is
  unbuilt. Depends on the presentation layer.
- **Procedural assembly + art pipeline (§24/§25, step 11).** No voxel kit, no
  meshes, no baking. Placeholder-art is fine for a playable MVP, but visible
  loadouts and the "Rocinante effect" (§14) are MVP-named.
- **Pressure / forecasting / pacing governor (§13, step 13).** None built.
- **Juice & audio (§23).** None. The GDD insists this is "the deliverable of
  fun," not polish, for a watch-heavy game.
- **Expressive identity (§14), wreck salvage (§15).** MVP-named, not started.

---

## 5. Recommended path to a playable full MVP

Re-sequenced from the GDD build order (§35) to front-load the three blockers.
Each phase is a small set of focused PRs that keeps `main` green and ends in
something *more playable than before*. Headless-first remains the rule: build
sim state behind `cargo test`, then bind, then render.

### Phase A — Make the player a real actor (sim core, headless)
The foundation of the core loop (§5) and Tier 1 (§0.3).
1. **Player corporation state:** treasury (credits), owned-ship roster, owned
   station with production slots, and a per-tick player economy that
   produces/consumes against the existing markets. All integer, all tested.
2. **Buy/sell verbs:** let the player trade against `Market` (the pricing
   already exists); earn/spend credits. First real *agency*.
3. **Contracts (light):** a handful of generated jobs (haul X to Y, supply Z)
   as the structured income + the first authored-thread hook.
4. **Wire fitting → ownership:** buying/building a ship from `sim::ships` adds it
   to the player fleet and debits the treasury and trained-crew pool (§8c).

### Phase B — Make it playable on screen (Godot shell)
Turn the binding into a game you operate.
5. **Game loop + time control:** `_process` drives `step()`; pause/speed scale
   the tick (§28); backgrounding pauses. Top-bar clock + controls (§18).
6. **The orrery (§21):** render bodies/haulers from the snapshot in a real (even
   if rough) 3D scene with the constrained camera; tap-to-select; compressed
   visual scale over true sim distances.
7. **Command console panels (§18) + alert feed UI (§19):** assets panel,
   selected-detail panel, the ranked/tunable feed with act-now verbs wired to
   actual actions (interdict, exploit shortage). This makes interdiction —
   the featured mechanic — *playable*, not just simulated.

### Phase C — Make it have a point (the §0 spine)
The over-invest priority; do not let it slip further.
8. **Lock the spine (design note in repo)** — destination pull, tier
   transitions, three-horizon goal stack (roadmap step 2).
9. **Gate entity + foreshadowing** as a visible far-goal from minute one (§0.1).
10. **Tier-1 → Tier-2 ascent** with an arrival milestone and a visible "next
    rung" (§0.3, step 12). Surface the now/tier/far goal stack in the UI.

### Phase D — Make it persist & tune
11. **Persistence (§30):** `serde` + `bincode`, version header, save slots.
12. **Data-driven tuning (§31):** move commodity/ship/combat tables to RON/JSON,
    hot-reload in dev. (Can slot earlier if iteration pain demands it.)

### Phase E — Make it *feel* like a game
13. **Automation / managers (§12)** — policy + run-by-exception over the now-real
    player assets.
14. **Combat diorama (§22)** over the existing resolver.
15. **Pressure/forecasting/pacing (§13)**, then the **juice + audio pass (§23)**
    and UX polish (felt vastness, §21) — the deliverable of fun.

Placeholder art throughout; the **procedural assembly tool (§25)** and final
art (§24) come last and do not block a playable MVP.

### Suggested next PR
**Phase A.1 — player corporation state** (treasury + owned station/production +
fleet roster, headless and tested). It is the smallest change that creates
genuine player agency and unblocks the core loop, the UI, and the spine.

---

## 6. Risks called out by the GDD that this review reaffirms

- **Minute-to-minute fun (§36, top risk).** The fun engine (§7b) is built but
  not *playable* — it has no verb surface a human can press. Phase B item 7 is
  the real test of the top risk; treat it as a milestone, not a chore.
- **Retention / "session 20" (§36).** Entirely unaddressed in code — the §0
  spine is the mitigation and it is blank. Phase C is non-negotiable for MVP.
- **Trajectory verb missing.** "Verbs of joy" #4 (§0.4) is a stub; flag it as a
  deliberate decision (port the prototype Hohmann model) rather than an
  oversight.
- **Juice/audio as an afterthought (§36).** Currently zero; keep it as a
  scheduled deliverable (Phase E), not a someday.

---

## 7. Bottom line

The project has spent its first ~12 PRs buying down **technical** risk
(determinism, the economy gate, the Android toolchain) and has done so
excellently. The next phase must buy down **game** risk: a player who can act, a
screen to act on, and a reason to keep climbing. Build the player corporation
first (headless), then the orrery + console + interdiction verb surface, then
the gate-and-tiers spine. That ordering turns the strong simulation we have into
the game the GDD describes.
