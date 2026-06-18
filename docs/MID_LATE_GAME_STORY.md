# TORCH — Mid- & Late-Game Story Arc (design notes)

**Status: design notes / forward plan — not yet implemented.** This captures the
authored narrative spine for the *middle* and *end* of a run, so the systems we build
(and the order we build them) point at a coherent story. It sits alongside the
already-shipped **gate-mystery** thread (`sim::missions::GATE_LORE` / `GATE_ANSWER`, §0.1)
and the **post-gate sandbox** (`docs/POST_GATE_PLAN.md`, G1–G5). The Expanse is the
tonal model (the repo already leans on it — OPA, the Rocination/Rocinante effect, Eros,
the Ring); names here are placeholders to be made legally-distinct in authoring.

The through-line, in one breath: **a slow alien-artifact mystery (the protomolecule)
time-gates into a system-wide war that cracks the old order; the gate opens at the peak
of that crisis; the powers try to seize the chokepoint while their own people flee
through it; the resulting economic collapse and infighting hollow Earth and Mars out —
and that vacuum is the player's late-game opening to become a real power.**

This is the *authored pull* the GDD (§0.2/§16) reserves real investment for. Like the
gate mystery, it should be **mostly notes + a few hard time/condition gates**, never a
railroaded campaign — the emergent empire sim keeps running underneath it.

---

## The load-bearing safety rule (same as everywhere else)

Each beat lands as an **additive, integer, rng-free, gated** layer — voiced through
`feed.announce(...)` (no new `Event` variants → no QA exhaustive-match churn) and gated
on a clock/condition the default `Sim` and the QA personas never trip. So the **§7c
stability gate and the QA *gameplay* body stay byte-identical**, exactly as the war /
contest / post-gate layers already do. The mid-game crisis is **time-gated** (a tick
schedule that scales with tier, like `war_flashpoint_interval`); the late-game arc is
**transit-gated** (`campaign.transited()`, like all of G1–G5).

---

## Act II — The Mid-Game Crisis (the protomolecule war)

### The protomolecule thread (time-gated lore)

A second slow-burn authored thread, structured exactly like `GATE_LORE`: a small array
of beats, revealed on a **time/tier cadence** (and via deep-frontier salvage finds, the
§15 anomaly hook). It tells the artifact story that *causes* the crisis — the Expanse
arc, reframed:

1. A derelict survey hauler is found at the edge of the Belt, its crew gone, its hold
   coated in something that is not ice and not alive — and not quite *not* alive.
2. A research station (the "Phoebe"/outer-moon lab) goes dark; both inners deny running
   it; both are lying.
3. **Eros**: a Belt station is quarantined, then *lost* — a quarter-million souls, and
   the thing inside wearing the station like a coat. The single atrocity that detonates
   the cold war. (Reuses the existing **Eros** body/colony — the iconic OPA station now
   carries the incident.)
4. **Venus**: the artifact reaches the inner system and begins *building* something in
   the clouds — vast, deliberate, ignoring every weapon thrown at it.
5. The thing on Venus finishes, leaves, and **builds the Ring** — the gate beyond Pluto
   warms and stirs (this is where the protomolecule thread *hands off* to the existing
   `GATE_LORE` beats 5–7; the two threads converge on the same waking ring).

These beats are **flavor + foreshadowing**, drawing no rng — pure `feed.announce`.

### The crisis it triggers: Earth/Mars war + OPA uprising

At a mid-game time gate (telegraphed first via `pressure::ThreatForecast`, the §13
pacing machinery), the protomolecule thread **boils over into a system-wide crisis** —
an escalation of the already-built **`FactionWar`** pressure gauge and the **war
flashpoint** dilemma:

- **Earth–Mars open war.** The flashpoint cadence tightens hard and the stakes rise:
  the `WarCollateral` dilemma fires often, lanes are dangerous, and *space-asset
  collateral* (the miner-loss mechanic) scales up — the inners are no longer skirmishing,
  they are *fighting*.
- **OPA uprising.** The **contest layer** (`sim::contest`) is the natural home: the
  inners' grip on the contested belt/jovian hubs (Ceres, Eros, Pallas, Vesta, Tycho,
  Ganymede…) is contested *violently* now — flares swing influence in big jumps, hubs
  flip control, and the Belt presses to throw the inners out. The player's
  gather-influence/claim loop runs against (or rides) this turmoil.
- **The player's angle.** Crisis = opportunity. Soaring scarcity (the war disrupts
  trade) means fat shortage dilemmas; rep is in flux (pick a side, or play both); the
  contested hubs are *up for grabs* while the powers are distracted. This is where a
  patient early-game operator cashes in.

**New mechanic surface (small):** a `CrisisState` clock on `Sim` (Dormant → Brewing →
War), advanced by a time gate, that (a) scales the existing war-flashpoint interval and
contest-flare magnitude, and (b) gates the late-game arc's start. Everything it touches
already exists — it's a *governor*, not new systems.

---

## Act III — The Late Game (after the gate opens)

The gate opens at the crisis peak — narratively, the protomolecule's work completes;
mechanically, the player **`transit_gate`s** (the existing deliberate verb, already the
threshold into `Tier::Beyond`). Crossing **lights the late-game arc** (transit-gated, so
the inner game stays byte-identical for anyone who hasn't crossed). The far side already
has its sandbox (G1–G5: place, economy, bridgehead, incursions, win/loss); this arc is
what happens **back home in Sol** while the player builds beyond the ring:

### 1. Population flight → economic collapse

With a thousand new worlds one jump away, **people leave**. Model it as a decaying drain
on the Sol economy: market demand and the trained-crew pool bleed toward the gate
(NPC haulers increasingly route *outbound* and don't come back; setpoints drift toward
glut on consumer goods, scarcity on the goods needed to *leave*). The inner markets
soften and wobble — the first crack in the old order, and a buyer's market for a player
with capital.

### 2. The Earth–Mars Coalition (EMC) blockades the gate

The inners, terrified of losing their populations and the chokepoint, **bury the
hatchet and seize the ring**. This is a *direct extension of the already-built E3
coalition*: instead of striking the player's holdings, the unified `faction_alarm`
coalition now **garrisons the gate** — a standing EMC fleet at the ring that taxes,
inspects, or *blocks* transit (a toll/▒gate-permit gate on `transit_gate` and on
far-side trade routes through the ring). The player who already transited runs the
blockade to resupply their bridgehead; the player who hasn't must break or bribe it.

### 3. A Free Navy forms and fights the EMC

The blockade and the inners' war crimes radicalize the Belt: a **Free Navy** (Belt +
defected hulls) coalesces and **fights the EMC for the gate**. Model it as a new
**faction-war *state*** (not just the ambient gauge): two AI blocs (EMC vs Free Navy)
with a contested front *at the ring*, resolved on a cadence via the existing
`combat::resolve`. The player can:
- **Back the Free Navy** (break the blockade, open the gate for the exodus — Belt/OPA
  rep, EMC enmity),
- **Back the EMC** (hold the chokepoint, tax the outflow — inner rep, Belt enmity),
- **Profit off both** (run guns/supplies to the front, the war-as-market play), or
- **Stay out** and let it grind while you build beyond.

### 4. Collapse + infighting hollow out the powers → the player's opening

The combination — population gone, economy collapsed, navies spent fighting each other
at the ring — **weakens Earth and Mars structurally**. Mechanically this is the payoff
the late-game vision already names ("*only in the late game the influence of Earth and
Mars will dwindle*"): their `faction_alarm` ceilings fall, their garrisons thin, their
contested-hub influence decays, and the player's **admin cap / coalition threshold /
seizure odds** all tilt favorable. The vacuum the artifact mystery opened is now the
player's to fill — **become the real powerhouse**, in Sol *and* beyond the ring. This
dovetails with the post-gate win-state (G5): victory is no longer just holding a
bridgehead, but **inheriting the Sol that the old powers broke**.

---

## Sequencing (when we build it)

Suggested order, each a small shippable PR in the established style:

- **M1.** Protomolecule lore thread (a second `*_LORE` array + a time/tier cadence +
  the Eros/Venus framing) — pure `feed.announce`, byte-identical.
- **M2.** `CrisisState` governor — scales the existing war-flashpoint + contest-flare
  intensity at the mid-game gate; telegraphed via `pressure`. Byte-identical until the
  gate trips (default/QA never reach it, or it's intensity-gated off).
- **L1.** Population-flight economic drain (transit-gated) — a decaying demand/crew
  drift on the Sol markets post-transit.
- **L2.** EMC gate blockade — extend the E3 coalition to garrison/toll the ring.
- **L3.** Free Navy ⟷ EMC war-state at the ring — two-bloc front resolved via
  `combat::resolve`, with the player's four stances.
- **L4.** Power-decay payoff — wire the collapse into faction_alarm/garrison/contest so
  the inners genuinely wane, and fold into the G5 win-state.

Each keeps `main` green and the pre-condition game byte-identical; the QA review only
moves when a layer is actually reachable by a persona (most won't be — these are
mid/late-game, behind time/transit gates the personas don't pass).

---

## How it reuses what's built (don't rebuild)

| Beat | Reuses |
| --- | --- |
| Protomolecule lore | `missions`/`GATE_LORE` pattern; salvage anomaly hook (§15) |
| Mid-game war | `pressure::FactionWar` gauge + forecasting; `WarCollateral` dilemma; miner-loss collateral |
| OPA uprising | `sim::contest` (the contested hubs already exist — Eros/Pallas/Vesta/Tycho/Ganymede…) |
| Gate opens | `campaign::transit_gate` → `Tier::Beyond` (already the threshold) |
| Population flight | `economy::Market` setpoint/stock drift; NPC `traffic` routing |
| EMC blockade | the E3 **coalition** (`faction_alarm`/`run_coalition`) re-aimed at the ring |
| Free Navy vs EMC | `combat::resolve` + a new faction-war *state*; the four player stances |
| Powers wane | `faction_alarm` ceilings, garrisons, contest influence, admin cap; G5 win-state |
