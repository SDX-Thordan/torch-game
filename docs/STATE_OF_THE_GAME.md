# TORCH — State of the Game & Next Steps (2026-06-17)

A full review of where TORCH stands and a sequenced, prioritized roadmap. Grounded in
the automated QA verdict (`docs/SAMPLE_GAMEPLAY_REVIEW.md`, seed 7 × 4000 ticks × 7
personas), the GDD (`TORCH_Unified_Design_Document2.md`, esp. Part VI / §46), and a
structural pass over the codebase.

---

## 1. Executive summary

TORCH is a **hard-sci-fi empire/economy sim** (Distant Worlds / Stellaris identity in
the Expanse's Sol, GDD Part VI) on a **deterministic Rust core + Godot 4.6 shell**,
Android-first. The original X4-style foundation (Parts 0–V) and the empire re-aim are
**both fully built and green**: the §0 retention spine, a stable multi-market economy,
interceptable traffic, ships/fitting/combat, factions/reputation, progression,
automation, pressure/pacing, the empire-acquisition loop (buy/annex/seize + admin cap +
faction coalition + economic integration + security + corporate diplomacy), the
post-gate endgame (place → economy → bridgehead → incursions → win/loss), persistence,
and the full art track (procedural forge, faction-distinct hulls, 3D combat diorama,
mesh baking).

**Scale:** ~12.4k LOC deterministic Rust sim across 28 modules; ~3.8k LOC GDScript
shell; **187 core tests + 17 GUT view tests**, all green; a 7-persona / 3-lens automated
QA harness. The roadmap (build steps 1–15) is effectively **complete** — what remains is
*depth*, not *foundation*.

**The headline finding:** the systems all work and several play styles are genuinely
engaging (Tycoon 98, Privateer 90, Expansionist 87 / 100). The single measured fun gap
is **Agency** (avg 42/100, the weakest engagement dimension) — the world raises
moment-to-moment exceptions but most play styles have neither a reason nor a pressable
path to engage them. That is the #1 thing to invest in next.

---

## 2. By the numbers (QA, seed 7)

| Persona | Overall | Weakest dim | Note |
| --- | --- | --- | --- |
| Tycoon (intended full loop) | **98** | Variety 86 | Answers 82/82 shortages — the model citizen |
| Privateer | **90** | Agency 60 | Gate in 38 days; rep cost real |
| Expansionist (empire) | **87** | Agency 60 | 13 holdings, coalition + piracy + customs all bite |
| Logistician | **76** | Stakes 25 | Hands-off routing climbs to the gate |
| Warlord (combat) | **49** | **Agency 19** | Only 3 fights / 4000t; loses money; 905-tick dead stretch |
| Arbitrageur | 28 | Direction 0 | Degenerate by design (doesn't climb) |
| Spectator | 22 | Direction 0 | Passive by design |

- **Strongest dimension: Flow (89 avg)** — time-compression + auto-pause-on-exception
  works; dead air is fast-forwardable.
- **Weakest dimension: Agency (42 avg)** — explicit "biggest fun gap to invest in."
- **Watchability 61/100** — the world is alive hands-off (166 convoys fly, raids
  telegraphed) before there's something to do.
- **Economy is balanced:** no death-spiral (the §7c gate holds on 64 seeds), no runaway
  faucet (overhead caps the Arbitrageur at ~2×), hand-trade and routing are
  complementary not strictly ordered.
- **UI audit:** 232 bindings, 76% wired, no phantom calls; status/legend/exception-verb
  all present; 42 keyboard bindings + native touch (keep touch first-class for §33).

---

## 3. What's working (keep / build on)

1. **The deterministic discipline is a genuine asset.** Seed-reproducible reviews,
   byte-identical gating, the §7c stability gate, content-in-code persistence. New
   features land without destabilizing the economy — this is why the empire + endgame
   layers could be added so fast. *Keep this bar.*
2. **The three-lens QA harness** (works? / engaging? / reachable?) catches real design
   gaps a unit test can't and makes "feel" regressions diff. It is correctly flagging
   Agency as the gap right now.
3. **The empire loop is the strongest new pillar** — acquisition by economy/diplomacy/
   military, capped by admin strain + a per-faction coalition, with economic
   integration and a security layer that bites but is counterable. The Expansionist
   persona proves the whole arc is reachable and consequential.
4. **The full loop is fun when engaged** (Tycoon 98). The intended operator — trade,
   route, raid to climb, auto-research, answer shortages — scores top marks.
5. **Art is now coherent and faction-legible** — tower silhouettes, four distinct
   faction grammars/palettes, a 3D combat diorama, baked single-mesh hulls.

---

## 4. What's weak (the gaps, with evidence)

### G1 — Agency is thin outside the full-loop operator *(the #1 gap)*
Only Tycoon answers act-now alerts (82/82); every other style answers **0**. The
`exploit_shortage` one-press verb exists, but the *recurring* exception is a single
kind (price shortages), and most play styles have no standing reason to engage it
moment-to-moment. The world's exception stream is rich; the **player's pressable menu
against it is narrow**. (QA: "Agency is the weakest engagement dimension, avg 42/100.")

### G2 — Combat is reachable but shallow and unrewarding
Warlord scores 49 (lowest non-degenerate). Combat fires rarely (3 fights/4000t), has a
905-tick dead stretch, and *loses money* (−12k) — there's no economic reason to fight
and no live agency mid-fight. The 3D diorama is presentation-only; the doctrine knobs
are pre-set, not in-the-moment. Yet the EP3 empire-piracy loop fires **constantly** —
combat and the thing that most needs defending aren't joined for the player.

### G3 — Holdings have no development depth *(deepest 4X structural gap)*
A controlled colony is a flat tribute + a fixed specialty output. There is no
population, no development/investment, no per-holding policy — the one classic 4X axis
still absent (GDD §46). The empire can grow *wide* but not *tall*.

### G4 — Diplomacy is built but uncovered and economically light
Corporate diplomacy (E8: court → Partner/Ally → free annex + lent escorts) exists but
**no QA persona exercises it**, and rivalry/alliance has thin passive economic payoff.
We can't see whether the diplomacy loop is fun.

### G5 — No sustained war; the endgame narrative is light
Great-power conflict is a one-off `seize`, not a war footing with goals and peace. The
gate mystery + opening missions exist but are a thin narrative skin over strong systems.

---

## 5. Roadmap — sequenced, macro-first, each a small focused PR

Ordered by **impact on the measured gaps**. Every rung keeps the determinism bar (gated/
additive ⇒ §7c + QA byte-identical unless it legitimately changes the loop, then
regenerate the review honestly).

### Phase A — Close the Agency gap (highest leverage; directly moves the #1 fun metric)

- **A1. Broaden the act-now verb surface.** Turn more of the existing event stream into
  *pressable, rewarding* one-press decisions beyond shortages: a boardable wreck
  (salvage already exists — surface it as act-now with a verb), a faction contract offer,
  a raid-defense choice, a diplomatic overture. Each carries a verb + a window + a payoff,
  routed through the existing alert→verb plumbing.
- **A2. Make answering pay.** A small, visible reward for engaging exceptions (credits/
  rep/research/spine-op) so every play style has a *reason* to act, not just the Tycoon.
- **A3. A Diplomat + a Defender QA persona** (see also Phase C/D) so the review *measures*
  the broadened agency surface, not just asserts it.
  *Target: lift avg Agency from 42 toward the 60+ the climbing personas already hit.*

### Phase B — Combat purpose & live command (lift Warlord from 49)

- **B1. Join combat to what needs defending.** Tie engagements to the frequent EP3
  empire-piracy / convoy-raid loop so fighting *protects income* — a reason to keep a
  navy, not a money sink.
- **B2. Bounties / loot / salvage on victory** so winning is net-positive and combat is a
  viable economic strategy, not just attrition.
- **B3. Live mid-fight commands** (focus-fire / brace / go-dark / commit-reserves) on the
  diorama the §22 scene is already built to host — the in-the-moment agency the doctrine
  knobs only approximate.

### Phase C — Pops / colony development (deepest 4X gap; grow *tall*)

- **C1. A light development tier per holding** — a population/development value that scales
  output, plus 2–3 macro policies (invest / specialize / tax), kept un-fiddly per §0.2.
- **C2. Development as a spine + economic input** so a tall empire is a real alternative to
  a wide one, and holdings become places you cultivate, not just collect.

### Phase D — Living diplomacy (cover + deepen E8)

- **D1. Diplomacy payoffs:** allied companies route more NPC trade to your owned markets
  (passive tariff income); rivals undercut/contest you (passive friction) — rivalry gets a
  downside beyond "can't annex."
- **D2. The Diplomat persona** courts allies and annexes the frontier peacefully — first QA
  coverage of the loop.

### Phase E — War as a state, and narrative onboarding (depth & framing)

- **E1. War footing** with a great power: war goals, sustained conflict, a peace — vs. the
  one-off seize.
- **E2. Onboarding/narrative pass** — make the opening missions + gate mystery teach and
  pull harder (the §0 retention priority is systemically present but thinly voiced).

### Deferred / non-goals
- **Audio** — deferred indefinitely by player choice (§23c).
- **Voxel-true art / UV atlas** — the primitive forge + per-material bake is the right
  ceiling for now; revisit only if hulls fill the frame at higher fidelity.

---

## 6. Recommendation

Start with **Phase A (Agency)** — it's the one gap the harness explicitly flags, it's
cheap (mostly wiring existing systems — salvage, contracts, defense — into the act-now
verb surface), and it lifts the engagement floor for *every* play style, not just the
Tycoon. Phase B (combat purpose) is the natural follow-on because it shares the same
"give the player a pressable, rewarding reason to act" theme and fixes the lowest-scoring
non-degenerate persona. Phases C–E add 4X *depth* once the moment-to-moment loop is
tight.
