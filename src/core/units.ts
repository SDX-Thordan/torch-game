/**
 * Canonical units for the sim. Keeping these explicit avoids the classic
 * "is this seconds or days?" bug that wrecks deterministic sims.
 *
 * - Distance: astronomical units (AU).
 * - Time: in-game seconds. One tick advances the clock by a fixed step.
 * - Mass: tonnes (t).
 * - Delta-v: metres per second (m/s).
 */
export const AU_IN_METRES = 1.495978707e11;
export const SECONDS_PER_DAY = 86_400;
export const DAYS_PER_YEAR = 365.25;

/** Standard gravitational parameter of the Sun, m^3 / s^2. */
export const MU_SUN = 1.32712440018e20;

export function daysToSeconds(days: number): number {
  return days * SECONDS_PER_DAY;
}

export function secondsToDays(seconds: number): number {
  return seconds / SECONDS_PER_DAY;
}

export function auToMetres(au: number): number {
  return au * AU_IN_METRES;
}

export function metresToAu(m: number): number {
  return m / AU_IN_METRES;
}

export function clamp(x: number, lo: number, hi: number): number {
  return x < lo ? lo : x > hi ? hi : x;
}

/** Linear interpolation. */
export function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Smooth Hermite interpolation of t in [0,1]. */
export function smoothstep(t: number): number {
  const c = clamp(t, 0, 1);
  return c * c * (3 - 2 * c);
}
