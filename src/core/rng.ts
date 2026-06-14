/**
 * Deterministic, seedable pseudo-random number generator.
 *
 * The whole point of TORCH's core sim is determinism (§7c: "the economy is a
 * deterministic headless sim ... on any seed"). `Math.random` is unseedable and
 * therefore forbidden anywhere in the sim. Use this instead.
 *
 * Implementation: mulberry32 — small, fast, good statistical quality for game
 * use, and trivially reproducible across machines.
 */
export class Rng {
  private state: number;

  constructor(seed: number) {
    // Force to a 32-bit unsigned integer so identical seeds always start identically.
    this.state = seed >>> 0;
  }

  /** Derive an independent stream from a string label (e.g. a market id). */
  static fromString(seed: string): Rng {
    let h = 2166136261 >>> 0; // FNV-1a
    for (let i = 0; i < seed.length; i++) {
      h ^= seed.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return new Rng(h >>> 0);
  }

  /** Next float in [0, 1). */
  next(): number {
    this.state = (this.state + 0x6d2b79f5) | 0;
    let t = Math.imul(this.state ^ (this.state >>> 15), 1 | this.state);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  }

  /** Float in [min, max). */
  range(min: number, max: number): number {
    return min + this.next() * (max - min);
  }

  /** Integer in [min, max] inclusive. */
  int(min: number, max: number): number {
    return Math.floor(this.range(min, max + 1));
  }

  /** True with probability p. */
  chance(p: number): boolean {
    return this.next() < p;
  }

  /** Snapshot the internal state (for save/load). */
  getState(): number {
    return this.state;
  }

  setState(state: number): void {
    this.state = state >>> 0;
  }
}
