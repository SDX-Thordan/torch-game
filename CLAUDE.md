# CLAUDE.md — TORCH project working notes

This file is the durable memory for building **TORCH** (see
`TORCH_Game_Design_Document1.md`). It records the plan, the architecture
decisions, the working process, and an append-only **learnings log**. Read it
at the start of every session; update it whenever a decision is made or a
lesson is learned.

---

## 1. Goal

Implement the **full Game Design Document**, ending in a **buildable Android
APK** produced by a GitHub Actions release workflow. TORCH is a hard sci-fi
industrial sandbox for mobile: real-time-with-pause, offline, logistics-first.

## 2. Working process (how we ship)

- **Small, focused PRs.** One concern per PR. Each PR must keep `main` green.
- **Squash-merge to `main`.** Every feature lands as a single squash commit so
  `main`'s history reads as a clean release log. Branch naming: `feat/...`,
  `chore/...`, `fix/...`, `ci/...`.
- **CI is the gate.** Every PR runs typecheck + tests (`.github/workflows/ci.yml`).
  Nothing merges red.
- **Headless-first** (the GDD's own §18 rule): simulation logic is pure,
  deterministic, and unit-tested before any UI sits on top of it.
- **Update this file every PR**: tick the roadmap, append to the learnings log.
- **Commit/PR hygiene:** never put model identifiers or internal tooling names
  in commits, PR text, or code.

## 3. Architecture & tech stack (decision)

The single source of truth is a **deterministic TypeScript simulation core**.
Everything else is a consumer of it.

| Layer | Choice | Rationale |
| --- | --- | --- |
| Sim core | TypeScript, pure, no I/O in hot paths | Deterministic, unit-testable, engine-agnostic, reusable by every front end. Already built & tested (steps 1–2). |
| Game client | Web: TypeScript + Vite, HTML5 **Canvas 2D** for the orrery, DOM cards/lists for panels | GDD §17: "map + alerts = a complete game", "glowing schematic", portrait card UI, cheap combat UI. 2D canvas matches the diorama/schematic aesthetic and is mobile-cheap. No 3D engine until proven necessary. |
| Mobile packaging | **Capacitor** wrapping the Vite web build into an Android APK | Reuses the TS core verbatim; a UI-heavy real-time-with-pause strategy game runs fine in a webview. Solo-friendly; no engine rewrite. |
| CI/Release | GitHub Actions: PR CI (typecheck+test) + release workflow (web build → Capacitor → Gradle `assembleDebug`/`Release` → upload APK) | Proves "buildable APK" continuously; APK attached to GitHub Releases on tags. |

**Determinism rules:** no `Math.random` in sim code (use `core/rng.ts`); no
wall-clock or `Date.now` in sim; the sim only advances in whole fixed ticks; all
content is data-driven JSON under `data/`.

**Portability rule:** the sim core must not import Node built-ins (`fs`, `path`)
in modules the web client uses. Node-only loaders (`economy/data.ts`) stay at the
edges; the core accepts injected data (`Economy({ data })`). The web client
imports JSON directly (Vite handles JSON).

## 4. Repo layout

```
data/            JSON content (commodities, recipes, markets, bodies, ...)
src/
  core/          rng, clock, units, vec2  (pure primitives)
  orbit/         bodies, orrery system, delta-v transfers
  economy/       living economy (markets, stabilizers, data loader)
  sim/           (planned) top-level world tying systems together
  index.ts       headless situation-report demo (Node)
  tools/         headless CLIs (stability report, ...)
app/             (planned) Vite web client; imports from src/
android/         (planned) Capacitor Android project
test/            vitest suites; economy.stability.test.ts = §7c acceptance gate
.github/workflows/
```

## 5. Commands

```bash
npm install
npm test           # full vitest suite incl. §7c stability acceptance test
npm run typecheck
npm run sim        # headless situation report
npm run stability  # economy stability sweep report
# (planned) npm run dev / npm run build / npm run apk
```

## 6. Roadmap (GDD → PRs)

Build order follows GDD §18. Status: [x] done, [~] in progress, [ ] todo.

- [x] **Core sim** — time, orbits, delta-v travel (§2, §6).
- [x] **Economy backbone** — stockpile pricing + stabilizers + §7c headless
  stability acceptance test (§7a, §7c, §7d).
- [x] **Process & CI** — CLAUDE.md, PR CI workflow (#1).
- [x] **APK skeleton** — Vite/Canvas web client driven by the sim core +
  Capacitor Android project + release workflow producing a debug APK.
- [ ] **Physical traffic layer (§7b)** — NPC haulers on real routes,
  interceptable; interdiction feeds back into stockpiles.
- [ ] **Ship design system (§8)** — hull classes, slots, mass/delta-v/heat
  tradeoffs, derived stats.
- [ ] **Combat resolver (§9)** — doctrine + range-band, headless first.
- [ ] **Progression (§10)** — research, blueprints, reputation, CEO skills.
- [ ] **Managers & automation (§11, §12)** — named hires, run-by-exception.
- [ ] **Pressure systems (§13)** — faction war, piracy, scarcity + forecasting
  + staged reversible decay.
- [ ] **Narrative campaign (§15)** — 5–8 teaching missions + sandbox handoff.
- [ ] **UX (§17)** — orrery hub, alert feed, one-screen-per-job, situation report.
- [ ] **Save/load + Ironman (§14)**.
- [ ] Ring-gates (§16) — post-MVP, designed-in only.

## 7. Learnings & decisions log (append-only)

- **2026-06-14 — Economy pricing anchor.** First pass mapped fill→price across
  the whole floor/ceiling band centred on the band midpoint, so settled prices
  ignored each commodity's `basePrice`. Fixed: price target is now piecewise so
  `stock == target ⇒ basePrice`, sliding to ceiling under scarcity / floor under
  glut. Settled prices now match the data's reference prices.
- **2026-06-14 — Market self-sufficiency.** Initial market data had supply gaps
  (e.g. Mars bred fuel but mined no fissile ore), which left several commodities
  pinned at their ceilings — bounded & stable, but not a "healthy" economy.
  Rebalanced each market to be self-sufficient (base production ≈ 1.1× full
  demand incl. downstream recipe inputs) so prices settle near reference.
  Inter-market hauling (§7b) is deferred — markets currently self-supply.
- **2026-06-14 — Stability test performance.** Per-tick `expect()` (tens of
  millions of calls) made the acceptance test take minutes. Switched the hot
  loop to plain boolean accumulation, asserting once at the end. ~3.5s for
  8 seeds × 20k ticks, same rigor.
- **2026-06-14 — Stack decision.** TS sim core + Vite/Canvas web client +
  Capacitor APK (see §3). Chosen for solo scope and to reuse the tested core
  rather than rewriting in a game engine.
- **2026-06-14 — Browser portability of the core.** `economy.ts` statically
  imported the `fs`-based data loader as a default, which would have pulled
  `node:fs` into the browser bundle. Fix: `Economy` now requires injected
  `data`; the fs loaders (`economy/data.ts`) stay Node-only (tests/CLIs call
  `loadEconomyData()`, the web client assembles data from bundled JSON in
  `app/data.ts`). Rule of thumb: core sim modules import no `node:*`.
- **2026-06-14 — Vite + TS `.js` ESM extensions.** The core uses explicit `.js`
  import specifiers (correct for Node ESM). Vite resolves these to the `.ts`
  sources automatically, so the core bundles unchanged — no alias/plugin needed.
- **2026-06-14 — Capacitor Android in git.** The generated `android/` Gradle
  project is committed (incl. `gradle-wrapper.jar`) so CI only builds, never
  scaffolds. Built web assets (`dist/`, `android/app/src/main/assets/public`)
  and `android/**/build` are gitignored and regenerated by `npm run build` +
  `npx cap sync android` in CI. Two typecheck projects: `tsconfig.json`
  (core/tests, no DOM) and `tsconfig.app.json` (web client, DOM lib).
- **2026-06-14 — APK build env.** Capacitor 6 → compileSdk 34, Gradle 8.2.1,
  JDK 17. Release workflow installs `platforms;android-34` + `build-tools;34.0.0`
  and runs `./gradlew assembleDebug`, on tag `v*` or manual dispatch.
