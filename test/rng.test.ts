import { describe, it, expect } from "vitest";
import { Rng } from "../src/core/rng.js";

describe("Rng — determinism", () => {
  it("produces identical streams for identical seeds", () => {
    const a = new Rng(12345);
    const b = new Rng(12345);
    for (let i = 0; i < 1000; i++) expect(a.next()).toBe(b.next());
  });

  it("produces different streams for different seeds", () => {
    const a = new Rng(1);
    const b = new Rng(2);
    let differ = false;
    for (let i = 0; i < 100; i++) if (a.next() !== b.next()) differ = true;
    expect(differ).toBe(true);
  });

  it("stays within [0,1) and is roughly uniform", () => {
    const r = new Rng(99);
    let sum = 0;
    const n = 100_000;
    for (let i = 0; i < n; i++) {
      const x = r.next();
      expect(x).toBeGreaterThanOrEqual(0);
      expect(x).toBeLessThan(1);
      sum += x;
    }
    expect(sum / n).toBeGreaterThan(0.48);
    expect(sum / n).toBeLessThan(0.52);
  });

  it("save/restore state reproduces the same continuation", () => {
    const r = new Rng(7);
    for (let i = 0; i < 50; i++) r.next();
    const snap = r.getState();
    const a = [r.next(), r.next(), r.next()];
    r.setState(snap);
    const b = [r.next(), r.next(), r.next()];
    expect(a).toEqual(b);
  });
});
