# TORCH — Simulation Core

A hard sci-fi industrial sandbox for mobile (see `TORCH_Game_Design_Document1.md`).
This repository currently implements the **headless, deterministic simulation
core** — steps 1–2 of the design doc's recommended build order (§18):

> 1. Deterministic core sim — time, orbits, delta-v travel.
> 2. Data-driven economy & industry (JSON commodities/recipes) **+ headless stability test.**

The guiding principle is the doc's own: *headless-first*. There is no UI yet —
the point of this slice is to nail the deterministic simulation the whole game
sits on, and to prove the economy is stable, because that is the project's
single biggest risk (§19).

## What's here

| Area | Module | Design doc |
| --- | --- | --- |
| Deterministic RNG | `src/core/rng.ts` | §7c "on any seed" |
| Fixed-step game clock | `src/core/clock.ts` | §6 real-time-with-pause |
| Orrery (Keplerian orbits) | `src/orbit/body.ts`, `system.ts` | §6 live orrery |
| Delta-v & travel time | `src/orbit/transfer.ts` | §2/§6 delta-v as the constraint |
| Living economy | `src/economy/*` | §7 economy & industry |
| Data (commodities/recipes/markets/bodies) | `data/*.json` | §7d, §18 data-driven |
| Acceptance test | `test/economy.stability.test.ts` | §7c GUT criterion |

### The economy model (§7)

Two ideas from the design doc drive it:

- **Stockpile pricing backbone (§7a).** Each market holds real inventory that
  fills and drains from NPC production/consumption. Prices track stock levels.
- **Stability by design (§7c).** The market is a damped negative-feedback loop,
  not a raw supply÷demand ratio (the thing that spirals):
  - **Damped pricing** — price *lerps* toward a stock-based target each tick.
  - **Hard floors/ceilings** — price is clamped to a per-commodity band; nothing
    goes to zero or infinity.
  - **Self-throttling production** — output ramps up to an emergency cap when a
    stockpile is low and shuts off as it approaches 2× its target.
  - **Rationing** — NPC consumption throttles down under scarcity.
  - **Chains** — refined goods consume their inputs (`data/recipes.json`), and
    production is input-gated, so shortages propagate realistically but bounded.

Price is anchored so that `stock == target` ⇒ `basePrice`, sliding toward the
ceiling under scarcity and the floor under glut.

### The acceptance test (§7c GUT criterion)

> *No market may death-spiral across thousands of ticks with no player present,
> on any seed. If it isn't stable while empty, it isn't done.*

`test/economy.stability.test.ts` encodes this as four machine-checked properties
swept across 8 seeds × 20,000 ticks (~833 in-game days):

1. **Boundedness** — stock ∈ [0, capacity], price ∈ [floor, ceiling], every tick.
2. **Finiteness** — no NaN/Infinity ever.
3. **Settling** — late-run price oscillation is a small fraction of the band.
4. **No growth** — oscillation amplitude does not expand over time.

Plus determinism, a cold-start-from-empty test (the economy "ran before you
existed"), and a local-shock recovery test (§7b interdiction → temporary
shortage → recovery).

## Running it

```bash
npm install
npm test            # full suite incl. the headless stability acceptance test
npm run sim         # headless situation report (orrery + transfers + prices)
npm run stability   # stability sweep report across seeds
npm run typecheck
```

Env knobs for the demo: `SEED=2 DAYS=400 npm run sim`.

## Design choices / simplifications

- **2D coplanar circular orbits** — "fidelity serves playability" (§6); no n-body.
- **Closed-form transfers** — textbook Hohmann (economical) vs. constant-accel
  flip-and-burn (hard burn). Captures the real tradeoff: fast costs vastly more
  delta-v.
- **Each market self-supplies.** The stockpile sim abstracts NPC production and
  consumption per market (§7a). **Inter-market hauling — the interceptable
  physical traffic layer (§7b) — is the next step, not yet built.** The
  `applyShock` hook that interdiction will use already exists and is tested.

## Not yet built (per §18 build order)

Ship design (3), combat resolver (4), progression (5), managers/automation (6),
narrative wrapper (7), pressure systems (8), and all UX (9). The MVP's honest
cut is the safeguard — this foundation comes first.
