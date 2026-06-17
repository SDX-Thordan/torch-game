# TORCH — The Empire Layer (expansion-by-acquisition)

**Why this doc.** A vision check (2026-06-17) found TORCH had been built faithfully
to the GDD — an **X4-style corporate sandbox** (you're a CEO who *perturbs* a living
economy and climbs to a gate) — but the actual north star is a **Distant
Worlds / Stellaris empire sim** (you *are* a colonizing state) in the Expanse's
hard-sci-fi Sol. The setting matches; the **genre / player identity** did not.

The chosen reconciliation (player's words):

> Grow the empire layer so the player can **acquire/build more stations** and later
> **take control of smaller independent colonies / other stations** — via
> **economy, diplomacy, or (later) military** — while being **careful not to
> overextend and anger the other factions**. Acquiring new assets is the **core loop**.

So expansion-by-acquisition becomes the spine, and the existing economy, factions,
fleet, and combat all become *means of acquisition* governed by a political cost.

## The loop

```
            ┌────────── ECONOMY (buy / build) ──────────┐
spot an  →  ├────────── DIPLOMACY (persuade) ───────────┤ → take an asset →
target      └────────── MILITARY (seize, later) ─────────┘   grow the empire
                                   │
                          raises faction ALARM
                          + administrative STRAIN
                                   │
              overextend → embargo / raids / a coalition comes for you
```

The fun is the **judgment**: which asset, by which means, at what political price —
and when to stop. Careful, political expansion *is* the game.

## What's reused (not thrown away)

The deterministic core survives wholesale. The economy (markets, 4-tier chains,
routes), the fleet + delta-v movement, combat, the alert feed, the salvage/discovery
layer, persistence, and the QA harness all stay. **`Relations`** (today mostly a
side effect of interdiction) becomes the load-bearing *governor* of expansion. The
`Tier` spine reframes from "ops climbed" to "empire grown."

## Sequenced rungs (small, focused PRs — the project's working style)

Each rung keeps `main` green; the inner-economy §7c stability gate must hold at every
step. Unlike the post-gate sandbox, this **changes the core loop**, so the QA review
will legitimately move (and gains an Expansionist persona at E6) — we regenerate it
honestly rather than chasing byte-identity.

### E1 — Holdings as a first-class layer + economic annexation 🟢 ✅ DONE
**Goal:** the player can **buy** an independent frontier colony and it becomes a
holding. The foundation everything else hangs on, with a built-in political cost so
expansion is never free.
- **Core:** a unified **holdings** view (the stations you build + colonies you
  control); independent colonies become **acquirable by purchase** (`acquire_colony`);
  acquiring debits credits, flips the colony to player control, pays a per-tick
  **tribute**, and **raises the inners' wariness** (`Relations::on_player_expand` —
  Earth & Mars distrust a rising outer power; the Belt, your home, approves). A spine op.
- **Shell:** an EMPIRE readout (holdings + acquire verb).
- **Tests:** acquire costs credits + flips control + angers the inners; a fresh sim
  controls nothing (existing tests + §7c gate unaffected; personas don't acquire, so
  the QA body stays put — only UI-wiring moves).

### E2 — Overextension: administrative capacity ⚙️ ✅ DONE
**Goal:** the *economic* cap on sprawl. You have an **admin capacity** (grows with the
CEO track); each holding consumes it; over capacity → rising upkeep + falling
efficiency on your holdings. Reckless expansion stops paying for itself.
- **Built:** `admin_capacity()` = `ADMIN_BASE` (3) + CEO-level/3 (earned, Stellaris
  admin-cap style); `admin_load()` = `holding_count`; `admin_strain()` = load over cap.
  `run_holdings` now scales tribute by `holdings_efficiency_bp()` (−15%/excess holding,
  floored at 20%) **and** bleeds `STRAIN_UPKEEP_PER_HOLDING` (35/tick) per over-capacity
  holding, so past your reach holdings go net-negative. 4 bindings + a `⚠ Holdings n/cap
  (strained · x%)` status readout. Inert with no holdings → §7c + QA body byte-identical.
  Test `overextension_strains_an_empire_past_its_administrative_reach`.

### E3 — Faction alarm & the coalition 🚨 (the geopolitical teeth)
**Goal:** the *political* cap. A per-faction **alarm** that rises with your total size
and with each acquisition **in that faction's sphere of influence**; high alarm →
embargo, targeted raids, and finally a **coalition** that actively comes for your
holdings. Reuses `pressure` + `faction`. Bodies gain a `sphere` (which great power
claims that region), so taking Mars's backyard angers Mars most.
- **Core:** `alarm[faction]`; sphere-aware expansion cost; coalition escalation.
- **Shell:** alarm meters; coalition forecasts in the feed.

### E4 — Diplomatic annexation 🤝
**Goal:** the *peaceful* path. Persuade an independent colony to **join** you on high
standing + an **Influence** spend (a new slow resource, Stellaris-style), angering the
inners *less* than buying in their sphere. Influence also offsets overextension.
- **Core:** `Influence` resource; `annex_colony` (standing + influence gated).

### E5 — Military seizure ⚔️
**Goal:** the *aggressive* path. Move a fleet to a target, win the engagement, **seize**
the station/colony. The biggest alarm spike (open aggression) and the tie that makes
the positional fleet layer (Pillar #2) serve expansion. Reuses combat.
- **Core:** `siege`/`seize_colony` (fleet on station + a won engagement).

### E6 — Expansion as the spine + the EMPIRE command view 👑
**Goal:** make "empire grown" the real tier metric (reframing the §0 three-horizon
stack around holdings/territory), and a full EMPIRE master-table (holdings, capacity,
alarm). Add an **Expansionist** QA persona so the new core loop is exercised and
critiqued; regenerate the gameplay review around it.

## Order & risk

**E1 → E2 → E3** is the critical path: an acquisition verb is unsafe without its caps,
so E2/E3 (the overextension + alarm governors) land close behind E1. E4/E5 add the
other two acquisition pathways; E6 makes it the spine and re-aims the QA lens. The one
real risk to watch throughout is the **§7c economy gate** — controlled colonies must
not destabilize the inner market loop (E1 keeps their benefit as a flat treasury
tribute that never touches market RNG, exactly so the gate is provably unaffected).
