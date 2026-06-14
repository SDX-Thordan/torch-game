import { describe, it, expect } from "vitest";
import { World } from "../src/sim/world.js";
import { loadBodies, loadEconomyData } from "../src/economy/data.js";

const DATA = loadEconomyData();
const BODIES = loadBodies();

function makeWorld(seed: number, traffic = {}) {
  return new World({ seed, data: DATA, bodies: BODIES, traffic });
}

describe("TrafficSystem — NPC haulers (§7b)", () => {
  it("spawns and delivers convoys over time", () => {
    const w = makeWorld(1);
    w.run(15_000);
    expect(w.traffic.delivered).toBeGreaterThan(0);
  });

  it("is deterministic: same seed => identical traffic outcome", () => {
    const a = makeWorld(7);
    const b = makeWorld(7);
    a.run(8_000);
    b.run(8_000);
    expect(a.traffic.delivered).toBe(b.traffic.delivered);
    expect(a.traffic.active.length).toBe(b.traffic.active.length);
    for (let i = 0; i < a.traffic.active.length; i++) {
      expect(a.traffic.active[i]!.cargo).toBe(b.traffic.active[i]!.cargo);
      expect(a.traffic.active[i]!.destId).toBe(b.traffic.active[i]!.destId);
    }
  });

  it("interdiction denies the delivery and yields loot (§7b)", () => {
    const w = makeWorld(3);
    // Run until at least one hauler is in flight.
    let guard = 0;
    while (w.traffic.active.length === 0 && guard++ < 40_000) w.step();
    expect(w.traffic.active.length).toBeGreaterThan(0);

    const victim = w.traffic.active[0]!;
    const cargo = victim.cargo;
    const deliveredBefore = w.traffic.delivered;

    const result = w.traffic.intercept(victim.id);
    expect(result).toBeDefined();
    expect(result!.loot).toBe(cargo);
    expect(result!.hauler.state).toBe("intercepted");

    // It must not later count as delivered.
    w.run(20_000);
    expect(w.traffic.intercepted).toBeGreaterThanOrEqual(1);
    // Intercepted hauler is gone; deliveries continue from others.
    expect(w.traffic.active.find((h) => h.id === victim.id)).toBeUndefined();
    expect(w.traffic.delivered).toBeGreaterThanOrEqual(deliveredBefore);
  });

  it("pirate raids intercept some haulers when enabled", () => {
    const w = makeWorld(5, { pirateRate: 0.5 });
    w.run(20_000);
    expect(w.traffic.intercepted).toBeGreaterThan(0);
  });
});

describe("Economy + traffic — stability still holds (§7c with §7b active)", () => {
  const TOTAL = 15_000;
  const WINDOW = 3_000;
  for (const seed of [0, 1, 2, 42]) {
    it(`stays bounded and settled with traffic on seed ${seed}`, () => {
      const w = makeWorld(seed);
      const late = new Map<string, { min: number; max: number }>();
      let violations = 0;

      for (let tick = 0; tick < TOTAL; tick++) {
        w.step();
        for (const m of w.economy.markets.values()) {
          for (const [cid, s] of m.states) {
            if (
              !Number.isFinite(s.stock) ||
              !Number.isFinite(s.price) ||
              s.stock < -1e-6 ||
              s.stock > s.capacity + 1e-6 ||
              s.price < s.floor - 1e-6 ||
              s.price > s.ceiling + 1e-6
            ) {
              violations++;
            }
            if (tick >= TOTAL - WINDOW) {
              const key = `${m.id}|${cid}`;
              const r = late.get(key);
              if (!r) late.set(key, { min: s.price, max: s.price });
              else {
                if (s.price < r.min) r.min = s.price;
                if (s.price > r.max) r.max = s.price;
              }
            }
          }
        }
      }

      expect(violations).toBe(0);
      for (const m of w.economy.markets.values()) {
        for (const [cid, s] of m.states) {
          const r = late.get(`${m.id}|${cid}`)!;
          // Allow a little more late-run movement than the empty economy, since
          // deliveries are discrete events — but it must not run wild.
          expect(r.max - r.min).toBeLessThanOrEqual(0.4 * (s.ceiling - s.floor));
        }
      }
    });
  }
});
