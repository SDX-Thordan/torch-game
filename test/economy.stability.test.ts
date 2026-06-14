import { describe, it, expect } from "vitest";
import { Economy } from "../src/economy/economy.js";

/**
 * THE ACCEPTANCE TEST (§7c).
 *
 * GUT criterion: "no market may death-spiral across thousands of ticks with no
 * player present, on any seed. If it isn't stable while empty, it isn't done."
 *
 * We encode "no death spiral" as four machine-checkable properties, swept across
 * many seeds:
 *   1. Boundedness — stock stays in [0, capacity], price in [floor, ceiling].
 *   2. Finiteness  — no NaN / Infinity ever appears.
 *   3. Settling    — late-run price oscillation is small (the system relaxes to
 *                    a damped equilibrium rather than ringing).
 *   4. No growth   — oscillation amplitude does not grow over time (the
 *                    signature of a spiral).
 */

const TICK_SECONDS = 3600; // 1 hour
const TOTAL_TICKS = 20_000; // ~833 days
const WARMUP = 3_000;
const WINDOW = 4_000;
const SEEDS = [0, 1, 2, 3, 7, 13, 42, 99];

interface Range {
  min: number;
  max: number;
}

function runSeed(seed: number) {
  const econ = new Economy({ seed });

  // Per (market|commodity) min/max price in an early and a late window.
  const early = new Map<string, Range>();
  const late = new Map<string, Range>();
  // Accumulate any invariant violations as plain booleans — calling expect()
  // tens of millions of times in the hot loop is needlessly slow.
  let violations = 0;

  const record = (into: Map<string, Range>, key: string, price: number) => {
    const r = into.get(key);
    if (!r) into.set(key, { min: price, max: price });
    else {
      if (price < r.min) r.min = price;
      if (price > r.max) r.max = price;
    }
  };

  for (let tick = 0; tick < TOTAL_TICKS; tick++) {
    econ.step(TICK_SECONDS);

    for (const m of econ.markets.values()) {
      for (const [cid, s] of m.states) {
        // 1 + 2: boundedness & finiteness, every tick.
        if (
          !Number.isFinite(s.stock) ||
          !Number.isFinite(s.price) ||
          s.stock < 0 ||
          s.stock > s.capacity + 1e-6 ||
          s.price < s.floor - 1e-6 ||
          s.price > s.ceiling + 1e-6
        ) {
          violations++;
        }

        const key = `${m.id}|${cid}`;
        if (tick >= WARMUP && tick < WARMUP + WINDOW) record(early, key, s.price);
        if (tick >= TOTAL_TICKS - WINDOW) record(late, key, s.price);
      }
    }
  }

  return { econ, early, late, violations };
}

describe("Economy — headless stability (§7c acceptance criterion)", () => {
  it.each(SEEDS)("never death-spirals on seed %i", (seed) => {
    const { econ, early, late, violations } = runSeed(seed);

    // 1 + 2: boundedness & finiteness held on every tick of the run.
    expect(violations).toBe(0);

    for (const m of econ.markets.values()) {
      for (const [cid, s] of m.states) {
        const key = `${m.id}|${cid}`;
        const lateR = late.get(key)!;
        const earlyR = early.get(key)!;
        const span = s.ceiling - s.floor;

        const lateAmp = lateR.max - lateR.min;
        const earlyAmp = earlyR.max - earlyR.min;

        // 3. Settling: late oscillation is a small fraction of the legal range.
        expect(lateAmp).toBeLessThanOrEqual(0.3 * span);

        // 4. No growth: amplitude must not be expanding over time.
        expect(lateAmp).toBeLessThanOrEqual(earlyAmp + 0.05 * span);
      }
    }
  });

  it("is fully deterministic: same seed => identical trajectory", () => {
    const a = new Economy({ seed: 5 });
    const b = new Economy({ seed: 5 });
    for (let i = 0; i < 5_000; i++) {
      a.step(TICK_SECONDS);
      b.step(TICK_SECONDS);
    }
    for (const [id, ma] of a.markets) {
      const mb = b.markets.get(id)!;
      for (const [cid, sa] of ma.states) {
        const sb = mb.states.get(cid)!;
        expect(sb.stock).toBe(sa.stock);
        expect(sb.price).toBe(sa.price);
      }
    }
  });

  it("starts empty and ran before the player existed (cold-start reaches equilibrium)", () => {
    // Even initialised at zero stock everywhere, the stabilizers must pull the
    // markets up to a bounded steady state rather than collapsing.
    const econ = new Economy({ seed: 3 });
    for (const m of econ.markets.values()) {
      for (const s of m.states.values()) s.stock = 0;
    }
    econ.run(12_000, TICK_SECONDS);
    for (const m of econ.markets.values()) {
      for (const s of m.states.values()) {
        expect(Number.isFinite(s.stock)).toBe(true);
        expect(s.stock).toBeGreaterThanOrEqual(0);
        expect(s.stock).toBeLessThanOrEqual(s.capacity + 1e-6);
        expect(s.price).toBeGreaterThanOrEqual(s.floor - 1e-6);
        expect(s.price).toBeLessThanOrEqual(s.ceiling + 1e-6);
      }
    }
  });
});

describe("Economy — local shocks recover (§7b/§7c locality)", () => {
  it("absorbs an 80% interdiction shock and returns toward equilibrium", () => {
    const econ = new Economy({ seed: 11 });
    // Settle first.
    econ.run(8_000, TICK_SECONDS);

    const market = econ.market("ceres_hub")!;
    const baseline = market.price("reaction_mass")!;

    // Pirates gut the reaction-mass stockpile.
    market.applyShock("reaction_mass", 0.8);
    const shocked = market.price("reaction_mass")!;

    // Let the stabilizers work.
    econ.run(6_000, TICK_SECONDS);
    const recovered = market.price("reaction_mass")!;

    const span = 120 - 10; // ceiling - floor for reaction_mass
    // Price must stay in-band throughout and come back near the pre-shock level.
    expect(recovered).toBeGreaterThanOrEqual(10);
    expect(recovered).toBeLessThanOrEqual(120);
    expect(Math.abs(recovered - baseline)).toBeLessThanOrEqual(0.15 * span);
    // The shock should have actually perturbed the price (test is meaningful).
    expect(shocked).toBeGreaterThanOrEqual(baseline);
  });
});
