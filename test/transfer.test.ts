import { describe, it, expect } from "vitest";
import { Body } from "../src/orbit/body.js";
import { hohmannTransfer, hardBurnTransfer, canAfford } from "../src/orbit/transfer.js";
import { secondsToDays } from "../src/core/units.js";

const earth = new Body({ id: "earth", name: "Earth", semiMajorAxisAu: 1 });
const mars = new Body({ id: "mars", name: "Mars", semiMajorAxisAu: 1.52, phase0: 0.7 });

describe("Hohmann transfer", () => {
  it("matches the textbook Earth->Mars figures (~5.6 km/s, ~259 days)", () => {
    const t = hohmannTransfer(earth, mars);
    expect(t.deltaV).toBeGreaterThan(5_000);
    expect(t.deltaV).toBeLessThan(6_500);
    const days = secondsToDays(t.timeSeconds);
    expect(days).toBeGreaterThan(240);
    expect(days).toBeLessThan(280);
  });

  it("is symmetric in delta-v cost", () => {
    expect(hohmannTransfer(earth, mars).deltaV).toBeCloseTo(
      hohmannTransfer(mars, earth).deltaV,
      3,
    );
  });
});

describe("Hard burn vs Hohmann — the core tradeoff (§6)", () => {
  it("is faster but vastly more expensive in delta-v", () => {
    const hoh = hohmannTransfer(earth, mars);
    const burn = hardBurnTransfer(earth, mars, 0, 0.3);
    expect(burn.timeSeconds).toBeLessThan(hoh.timeSeconds);
    expect(burn.deltaV).toBeGreaterThan(hoh.deltaV);
  });
});

describe("Delta-v budget gating (§6: running dry strands you)", () => {
  it("affords a cheap route but not an unaffordable one", () => {
    const hoh = hohmannTransfer(earth, mars);
    expect(canAfford(hoh.deltaV + 1, hoh)).toBe(true);
    expect(canAfford(hoh.deltaV - 1, hoh)).toBe(false);
  });
});
