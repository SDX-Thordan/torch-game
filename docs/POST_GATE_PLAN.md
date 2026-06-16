# TORCH — Post-Gate Sandbox Plan (§17 endgame)

The deviation review (`docs/GDD_DEVIATION_REVIEW.md`) is closed: every 🔴/🟠/🟡 is
done, and the endgame's **climax** is in — `transit_gate` crosses into `Tier::Beyond`
and voices the gate's answer (#79). This doc sequences the rest of §17 — *the larger
game beyond the ring* — into focused, individually-shippable PRs, in the project's
established style (small PRs, headless-first, `main` always green).

## The through-line

`Tier::Beyond`'s briefing promises **"a new sky, a new economy, and whatever was
counting on the far side."** The post-gate sandbox makes that mechanical:
1. a **place** to be (the far side as real bodies on the map),
2. a **new economy** there (isolated, scarce, high-value),
3. the **bridgehead** you hold (colonization — the thing at stake),
4. the **incursions** that attack it (the far-side enemy — the §0.1 payoff "now it
   knows your face"),
5. the **larger game** that resolves it (empire / win-state).

## The load-bearing safety rule

**Everything post-gate is gated on `campaign.transited()`**, which is false for the
default `Sim` and every QA persona (none call `transit_gate`). So each PR below keeps
the **§7c stability gate and the QA review byte-identical** — the pre-transit game is
untouched, exactly as the gate-transit PR (#79) already proved. This is what makes a
big content phase shippable in safe increments.

## Reused machinery (don't rebuild)

- `orbit::default_system()` + `BodyKind::Gate` — append far-side bodies *past* the
  ring (revealed only when transited); `position_of` already resolves them.
- `frontier::Colony` + `economy::Market`/`markets_from_defs` — the far-side hubs and
  their economy, the same model as the Saturn/Europa colonies.
- `pressure::PressureSystem` — the incursion threat track (a new `PressureKind`).
- `combat::resolve` + `engage_raiders` — incursion battles reuse the resolver.
- `industry::Station` / `found_refinery` + `logistics::TradeRoute` — far-side
  production + routes through the gate.

---

## Sequenced PRs

### G1 — The far side is a place (the map opens) 🗺️ — ✅ DONE
**Shipped:** `BodyKind::FarSide` + a far-side cluster (the dead star **Erebus** with
**Threshold** and **The Tally** orbiting it) **appended** past the Ring-Gate, so every
inner index is unmoved. Bindings `body_is_far_side` / `far_side_revealed` (=
`transited()`); the orrery keeps the far side **hidden until transit**, then reveals it
(cold-violet worlds) and jumps the camera through. Unit-tested
(`the_far_side_lies_beyond_the_gate`); QA regenerated (salvage reseeds off the longer
body list — 0 concerns). *Note:* a revealed-far-side render wasn't captured (xvfb is
too slow to route to the gate in-frame); the reveal is a visibility toggle verified by
the unit test + a clean headless run that exercises the branch every frame.

**Goal:** transiting reveals somewhere to *be*. Append a small **far-side cluster**
beyond the ring (a star/anomaly + 2–3 bodies) to the body list, hidden until
transited; the orrery reveals them on transit and lets the camera focus them.
- **Core:** extend `orbit::default_system` with the far-side bodies (kind tagged,
  e.g. a new `BodyKind::Frontier`); a `Sim`/binding `far_side_revealed()` =
  `transited()`. Bodies exist always (determinism) but the shell only *shows* them
  post-transit.
- **Safety:** body indices append after the gate (11), so all existing index-based
  refs (markets, frontier colonies) are unmoved → §7c + QA byte-identical.
- **Shell:** render the far-side bodies + a "through the ring" camera jump on transit.
- **Tests:** the far-side bodies exist and resolve positions; `far_side_revealed`
  flips only on transit. GUT: the reveal binding.

### G2 — The far-side economy 💱
**Goal:** "a new economy." 1–2 **far-side markets** (on G1's bodies) — isolated,
deep in scarcity (everything dear), a different commodity emphasis (the far side
exports something Sol lacks). Routes/logistics can run *through the gate* once
transited (a long, fuel-heavy haul — the freighter-remass system already prices it).
- **Core:** add the far-side colonies as `is_market` (gated so they only join
  `markets_from_defs` post-transit — or always present but unreachable until transit).
  *Decision to make:* simplest is always-present markets that the player simply
  *can't profitably reach* until through the gate (no special-casing the economy), so
  the §7c gate runs them too — needs a stability re-check (the one risk in this phase).
- **Shell:** the far-side markets appear in the MARKET board post-transit.
- **Tests:** the far-side market trades; §7c gate holds with it; QA regen if the
  default economy gained a market (else byte-identical).

### G3 — The bridgehead (colonization) 🏗️
**Goal:** "hold the bridgehead." A `found_bridgehead`/colonize verb available only in
`Beyond` that plants the player's **own** far-side foothold (a colony/station that
produces + anchors presence). This is the thing incursions threaten (G4) and the
spine of the endgame loop.
- **Core:** a `Bridgehead` state on the corp/campaign (health/level); a verb to found
  + upgrade it; it's a spine op (advances within Beyond).
- **Shell:** a BUILD/SYSTEMS verb + a bridgehead status panel in the endgame.
- **Tests:** found/upgrade; only in Beyond.

### G4 — Incursions (the far side answers) 👁️
**Goal:** the `GATE_ANSWER` payoff — "now it knows your face." A new
`PressureKind::Incursion` track that activates **only post-transit**: escalating raids
from beyond the ring that target the bridgehead (G3) and the core, voiced as
incursions (a distinct, dread voice in the feed). Reuses the pressure cadence +
`engage_raiders`/combat. Difficulty ramps with time-in-Beyond (the count rising).
- **Core:** `PressureSystem` endgame escalation gated on a `set_endgame(true)` set at
  transit; incursion forecasts/raids; bridgehead damage on an unanswered incursion.
- **Safety:** gated on transit → ambient pre-transit pressure unchanged → §7c + QA
  byte-identical (the `set_endgame` flag is off until transit).
- **Shell:** incursion alerts + a pressure-gauge for the far-side threat.
- **Tests:** incursions only fire post-transit; they escalate; an answered incursion
  protects the bridgehead.

### G5 — The larger game resolves (empire / win-state) 👑
**Goal:** "own what comes through — or be owned by it." The culminating loop: hold the
bridgehead through rising incursions to a **victory state** (you control the far side
/ the gate), or lose it (the bridgehead falls). The §0 destination pull finally
*completes* — a genuine end, not an open treadmill.
- **Core:** a `Campaign`/endgame resolution (a win threshold on bridgehead level +
  incursions survived; a loss on bridgehead destroyed); a voiced finale.
- **Shell:** an endgame/victory screen; the destination panel shows the final goal.
- **Tests:** the win/loss conditions trigger correctly.

---

## Parallel art track (independent of G1–G5)

### A1 — Procedural assembly tool + baking pipeline (roadmap #11, §24/§25)
Offline tool that authors voxel designs and **bakes** them to meshes the shell loads
(replacing the primitive-mesh ships/bodies). Pure tooling + asset pipeline; no sim
change. The biggest single art lift; can land any time.

### A2 — Voxel combat diorama (§22)
Replace the text BattleLog playback with a **voxel** diorama once A1 lands. Builds on
the existing diorama (#63/#71) — swap the presentation, keep the BattleLog data.

---

## Recommended order

**G1 → G2 → G3 → G4 → G5** is the natural dependency chain (place → economy →
bridgehead → threat → resolution). G2's "does the new market destabilize §7c?" is the
one real risk to validate early; everything else is transit-gated and therefore
QA-neutral by construction. The art track (A1/A2) is independent and can interleave.

Start with **G1** (the far side as a place) — small, safe (append-only bodies), and it
gives every later piece somewhere to live.
