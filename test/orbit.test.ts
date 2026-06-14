import { describe, it, expect } from "vitest";
import { Body } from "../src/orbit/body.js";
import { SolSystem } from "../src/orbit/system.js";
import { loadBodies } from "../src/economy/data.js";
import { length } from "../src/core/vec2.js";

describe("Body — Keplerian periods", () => {
  it("gives Earth a ~365 day year at 1 AU", () => {
    const earth = new Body({ id: "earth", name: "Earth", semiMajorAxisAu: 1 });
    const periodDays = earth.periodSeconds / 86_400;
    expect(periodDays).toBeGreaterThan(364);
    expect(periodDays).toBeLessThan(367);
  });

  it("obeys Kepler's third law: outer bodies orbit slower", () => {
    const earth = new Body({ id: "e", name: "E", semiMajorAxisAu: 1 });
    const ceres = new Body({ id: "c", name: "C", semiMajorAxisAu: 2.77 });
    expect(ceres.periodSeconds).toBeGreaterThan(earth.periodSeconds);
    // Period^2 ~ a^3.
    const ratio = (ceres.periodSeconds / earth.periodSeconds) ** 2;
    expect(ratio).toBeCloseTo(2.77 ** 3, 0);
  });

  it("keeps a body on its circular orbit (radius constant over time)", () => {
    const mars = new Body({ id: "m", name: "M", semiMajorAxisAu: 1.52, phase0: 1 });
    const r0 = length(mars.positionAt(0));
    const r1 = length(mars.positionAt(1e7));
    expect(r1).toBeCloseTo(r0, 0);
    expect(r0).toBeCloseTo(mars.radiusM, 0);
  });
});

describe("SolSystem — loaded slice of Sol", () => {
  it("loads all designed bodies", () => {
    const system = new SolSystem(loadBodies());
    for (const id of ["earth", "mars", "ceres", "vesta", "pallas", "europa", "luna"]) {
      expect(system.body(id)).toBeDefined();
    }
  });
});
