# TORCH — Corporate diplomacy with the independent companies (E8+)

**Why this doc.** A player design call (2026-06-17):

> Diplomacy is good — but with other **independent factions/companies**. Earth and Mars
> are just watchful giants. About inspections: I don't want too much micro decisions,
> I'd rather focus on **macro decisions**.

So: Earth/Mars stay the looming great-power pressure (the E3/E7 coalition — you avoid
provoking them, you don't negotiate). The **independent companies** that operate the
frontier colonies become the negotiable diplomatic actors, played through a few
**macro** moves with **passive, standing-based** effects — no per-event micro prompts.

## E8 — Independent companies & courting 🤝 ✅ DONE
**Goal:** the macro diplomacy loop. Each independent colony is operated by a named
company; the player invests **Influence** to court it up the stance ladder
(Neutral → Partner → Ally), and reaps passive benefits.
- **Built:** `sim::diplomacy` — `Company { name, home_colony, relation }`, a `Stance`
  ladder (Rival < Cold < Neutral < Partner < Ally), one company per independent colony
  (Ganymede Free Traders, Callisto Shipwrights, Enceladus Hydro Combine, Triton
  Pioneers). `court_company(i)` spends `COURT_INFLUENCE_COST` (100) → `COURT_RELATION_
  GAIN` (150). **Passive effects:** an **Ally**'s colony annexes **for free** (joins
  willingly, no Influence); allied companies **lend an escort each** (`effective_escorts`
  = navy + `ally_count`), so diplomacy buys piracy security (EP3). A **Partner** colony
  is annexable with Influence even at low generic Independents standing; a **Rival**
  won't be annexed at all. Buying a colony out from under its operator sours it
  (`-BUYOUT_RELATION_HIT`); **seizing** it makes it a **Rival** (`-SEIZE_RELATION_HIT`).
  Persisted as `company_relations: Vec<i64>` (content in code, only the dials saved).
  7 bindings + a 🤝 COURT verb + an INDEPENDENT RELATIONS section in the EMPIRE view.
  All gated on the player courting/acquiring (personas don't) → §7c + QA body
  byte-identical. Tests `courting_a_company_to_ally_opens_a_free_annex_and_lends_an_
  escort`, `seizing_a_companys_colony_makes_it_a_rival`.

## Candidate follow-ups (macro, not micro)
- **Company-supplied trade:** allied companies route more NPC traffic to your owned
  markets (more EP2 tariff income) — a passive economic ally benefit.
- **Rival competition:** a Rival company undercuts your trade or contests acquisitions
  (a passive economic friction), giving rivalry a downside beyond "can't annex".
- **A Diplomat QA persona** that courts companies to allies and annexes the frontier
  peacefully — the first persona to exercise the diplomacy loop in the review.
- **Treaties/pacts** as standing toggles (non-aggression, trade pact) with passive
  modifiers — more macro relationship texture if wanted.
