# TORCH — Empire Layer Phase 2: the living trade empire

**Why this doc.** A player review (2026-06-17) after the empire layer (E1–E6) landed:

> Are the colonies and stations integrable into your own market — supply, production,
> logistics? And larger trade empires need protection from pirates (and faction
> "inspections").

Both are real gaps. After E1–E6 you can *acquire* holdings, but a controlled colony
was only a **flat credit tribute** — it didn't feed your supply chain, and nothing
preyed on a large trade empire (pirates hit only NPC haulers; factions couldn't
enforce against your shipping). Phase 2 makes holdings **economic nodes** and makes a
big empire **need defending**.

## The two threads

**A — Economic integration (holdings become supply-chain nodes).**
**B — Security (a trade empire must be protected).**

## Sequenced rungs (small focused PRs)

Each keeps `main` green and the §7c stability gate intact (everything is gated on the
player actually *holding* assets, which a fresh sim / the non-expanding personas never
do — so the gate stays byte-identical and only the Expansionist's review moves, which
we regenerate honestly).

### EP1 — Holdings supply your chain 🟢 ✅ DONE
**Goal:** a controlled colony produces a **specialty raw good into your warehouse**
every tick — so holdings are *supply sources*, not a credit drip.
- **Built:** `colony_specialty(i)` (thematic by faction — Belt→Ice, Mars→Ore,
  Earth→Volatiles, independents vary by location); `run_holdings` deposits
  `COLONY_OUTPUT_PER_TICK` (3) of it into the warehouse each tick (warehouse-only, no
  market RNG → §7c untouched). **Emergent integration proven by QA:** `run_industry`
  already sources a refinery's input from your warehouse before buying from the market,
  so colony output now *feeds your refineries directly* — supply → production →
  logistics, end-to-end. The EMPIRE view shows each holding's "supplies X". Binding
  `colony_specialty`. Test `controlled_colonies_supply_raw_goods_into_your_warehouse`.

### EP2 — Owned markets 💱 ✅ DONE
**Goal:** a controlled colony's market is *yours*. Trade there at a **reduced/zero
brokerage fee** (you own the broker), and the EMPIRE/MARKET views flag it as owned.
Optionally: preferential (cheaper) station siting at your holdings.
- **Core:** map market → colony → controlled; `buy`/`sell` skip/reduce `TRADE_FEE_BP`
  at owned markets. Watch the faucet risk (the §5 fee is a sink — a *reduced* fee, not
  zero, likely safest; let the Expansionist persona catch a runaway).

### EP3 — Piracy against your empire 🏴‍☠️ ✅ DONE
**Goal:** as your trade grows, pirates prey on **your** freighters and holdings, not
just NPC lanes. A piracy pressure that scales with holdings + trade volume; raids deny
a route delivery (the §7b cut, applied to *your* freighter) or chip a holding —
**unless you run security**: a standing patrol/escort policy (the §12 automation layer
half-exists) or a garrison. Bigger empire ⇒ more raids ⇒ you must field a navy.
- **Core:** a piracy gauge off holdings/trade; raids target player routes/holdings;
  a patrol/escort policy that intercepts them (reuse `interdiction::resolve`).

### EP4 — Faction inspections & enforcement 🛂
**Goal:** factions you've **soured or alarmed** enforce against you — customs
inspections of your shipping (fines / contraband seizure), embargoes at their markets,
and sanctions on your holdings in their sphere. Countered by reputation repair
(contracts), escorts, bribes, or routing around hostile space. Ties reputation + E3
alarm to concrete trade friction.
- **Core:** an inspection/enforcement layer keyed to standing + `coalition_alarm`;
  trade friction at hostile markets; a player counter (pay the fine / fight / reroute).

## Order & risk

**EP1 → EP2** (economic integration) then **EP3 → EP4** (security). The one balance
risk is EP2's fee change (don't reopen the arbitrage faucet — keep a reduced fee and
let QA verify). EP3/EP4 reuse the interdiction + pressure + alarm machinery already in
the core, so they're additive, not new subsystems.
