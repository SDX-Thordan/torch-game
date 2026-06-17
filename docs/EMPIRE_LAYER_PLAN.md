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

### E3 — Faction alarm & the coalition 🚨 ✅ DONE (the geopolitical teeth)
**Goal:** the *political* cap. Expansion raises a **coalition alarm**; high alarm →
a **coalition** that actively comes for your holdings.
- **Built:** `coalition_alarm` (0..=1000) trends toward a size baseline
  (`holdings × 90`) and spikes `+120` per acquisition — so a *big* empire stays
  watched and *fast* expansion unites them early. Above `COALITION_THRESHOLD` (500) a
  coalition forms: `run_coalition` telegraphs (`ThreatForecast{FactionWar}`) then
  lands an **act-now `CoalitionStrike`** carrying a `DefendHoldings` verb + a window;
  unanswered it **seizes your most valuable controlled colony** (`HoldingLost`,
  liberated back to the Independents — which *relieves* alarm, a natural equilibrium)
  or exacts reparations if you hold none. `defend_holdings(band)` rallies the fleet vs
  an alarm-scaled pack (2→7 ships, quality 65); a win repels it (alarm relief + an op),
  a loss lets it seize. Cadence tightens with alarm. Persisted via `coalition_alarm`
  (schedule re-arms on load). 4 bindings + a `⚠ COALITION (alarm n)` / `inners wary`
  status warning + a DEFEND HOLDINGS button (lit only under strike). Inert with no
  holdings (alarm pinned at 0) → §7c + QA body byte-identical. Tests
  `overexpansion_provokes_a_coalition_that_seizes_an_undefended_holding`,
  `defending_repels_the_coalition_and_keeps_the_holdings`.
- *Refinement deferred:* per-faction, sphere-aware alarm (taking Mars's backyard
  angers Mars most) — v1 models the coalition as a single united gauge.

### E4 — Diplomatic annexation 🤝 ✅ DONE
**Goal:** the *peaceful* path. Persuade an independent colony to **join** you on high
standing + an **Influence** spend, angering the inners *less* than a buyout.
- **Built:** an `influence` resource accrues slowly each tick (`INFLUENCE_PER_TICK`,
  capped at `INFLUENCE_MAX`); `annex_colony(i)` is gated on `can_annex` (Independents
  at ≥ Cordial `ANNEX_STANDING_REQ` 200 **and** `ANNEX_INFLUENCE_COST` 300 banked),
  spends Influence (not credits), pays the gentler `Relations::on_player_annex`
  (−20 inners vs −40) + a smaller alarm spike (`ALARM_PER_ANNEX` 60 vs 120). The reward
  for the patient, reputation-built path. `AnnexError`; persisted (`influence`). 3
  bindings + an `⊕ ANNEX (DIPLO)` button + an `Influence n` readout. Test
  `diplomatic_annexation_costs_influence_and_good_standing_not_credits`.

### E5 — Military seizure ⚔️ ✅ DONE
**Goal:** the *aggressive* path. Assault a colony's garrison, win, **seize** it —
even a great power's. Reuses combat.
- **Built:** `seize_colony(i, band)` musters the fleet vs a `garrison_size`-scaled
  defending pack (Earth 8 / Mars 6 / Belt 4 / Independents 2 frigates, quality 60), so
  taking the inners' ground needs a real battlefleet while an outpost falls to a
  frigate or two. A won siege flips control (of *any* colony, not just independents),
  applies ship losses, and pays the harshest political price — `Relations::
  on_player_seize` craters the owner's standing (−200) + their rival's bonus + the
  biggest alarm spike (`ALARM_PER_SEIZE` 220). A loss just costs ships. `SeizeError`.
  4 bindings + a `⚔ SEIZE COLONY` button (assaults the lightest-garrisoned target) +
  the diorama. Test
  `military_seizure_takes_a_colony_by_force_at_the_harshest_political_price`.

**The three acquisition pathways are complete** — economy (E1 buy), diplomacy (E4
annex), military (E5 seize) — each with a distinct cost (credits / Influence+standing /
ships+blood) and a distinct political price (alarm 120 / 60 / 220), all governed by
the E2 administrative cap and the E3 coalition. Remaining: **E6** (expansion as the
spine metric + the EMPIRE master-table view + an Expansionist QA persona).

### E6 — Expansion as the spine + the EMPIRE command view 👑 ✅ DONE
**Goal:** make "empire grown" a legible spine metric, a full EMPIRE master-table, and
an Expansionist QA persona so the new core loop is exercised and critiqued.
- **Built:** (1) *Spine* — `empire_rank()` (Independent Operator → Local → Regional →
  Great Power → Hegemon by holdings) + `next_empire_rank()`, surfaced as the headline
  of the SYSTEMS status and the EMPIRE view. (2) *EMPIRE view* — a fifth nav-rail view
  (`✪`): the rank + next-rung headline, an Admin/Influence/alarm meter row, the
  BUY/ANNEX/SEIZE/DEFEND verb deck, and a **master-table** listing your holdings, the
  acquirable independents (cost + garrison), and the seizable great-power colonies
  (color-coded by garrison strength) — the "map + master-tables" empire command
  surface, render-verified. (3) *Expansionist persona* — buys colonies, founds
  stations, defends the coalition; the harness samples `holdings`+`coalition_alarm`
  and `review_empire` reports the loop. **This is the first rung that legitimately
  moves the QA review** (now 7 personas): the Expansionist grew to 13 holdings, maxed
  coalition alarm to 1000, fought 3 defenses, lost a holding to the inners, and soured
  Earth/Mars to −392 — the whole loop, exercised and critiqued. Regenerated honestly.

**The empire layer (E1–E6) is complete** — expansion-by-acquisition (economy /
diplomacy / military) is the core loop, capped by administrative capacity + the
faction coalition, legible through the EMPIRE view, and exercised by its own QA lens.

## Order & risk

**E1 → E2 → E3** is the critical path: an acquisition verb is unsafe without its caps,
so E2/E3 (the overextension + alarm governors) land close behind E1. E4/E5 add the
other two acquisition pathways; E6 makes it the spine and re-aims the QA lens. The one
real risk to watch throughout is the **§7c economy gate** — controlled colonies must
not destabilize the inner market loop (E1 keeps their benefit as a flat treasury
tribute that never touches market RNG, exactly so the gate is provably unaffected).
