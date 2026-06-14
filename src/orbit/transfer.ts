import { MU_SUN } from "../core/units.js";
import { Body } from "./body.js";

/**
 * Delta-v and travel-time model (§2: "delta-v is the universal constraint";
 * §6: "economical (slow, low delta-v, Hohmann-like) vs. hard burn").
 *
 * We use the textbook Hohmann transfer between two circular orbits for the
 * economical option, and a simple constant-thrust "brachistochrone" model for
 * the hard burn. Both are deterministic closed-form approximations — good
 * enough for a strategy layer, and they capture the real tradeoff: the fast
 * route costs far more delta-v.
 */
export interface TransferResult {
  /** Total delta-v required, m/s. */
  deltaV: number;
  /** Travel time, seconds. */
  timeSeconds: number;
}

/**
 * Hohmann transfer between two circular heliocentric orbits.
 * The economical baseline route.
 */
export function hohmannTransfer(from: Body, to: Body): TransferResult {
  const r1 = from.radiusM;
  const r2 = to.radiusM;
  const mu = MU_SUN;

  const v1 = Math.sqrt(mu / r1);
  const v2 = Math.sqrt(mu / r2);

  // Velocities at peri/apo of the transfer ellipse.
  const aTransfer = (r1 + r2) / 2;
  const vTransfer1 = Math.sqrt(mu * (2 / r1 - 1 / aTransfer));
  const vTransfer2 = Math.sqrt(mu * (2 / r2 - 1 / aTransfer));

  const deltaV = Math.abs(vTransfer1 - v1) + Math.abs(v2 - vTransfer2);
  const timeSeconds = Math.PI * Math.sqrt((aTransfer * aTransfer * aTransfer) / mu);

  return { deltaV, timeSeconds };
}

/**
 * Hard burn: constant-acceleration flip-and-burn between two bodies, evaluated
 * against their straight-line separation at departure.
 *
 * t = 2 * sqrt(d / a) for accelerate-to-midpoint, flip, decelerate.
 * delta-v = a * t (you burn the whole way).
 *
 * @param accelG  acceleration in g (e.g. 0.3 for a fusion torch cruise).
 */
export function hardBurnTransfer(from: Body, to: Body, t0: number, accelG: number): TransferResult {
  const g = 9.80665;
  const a = accelG * g;
  const p1 = from.positionAt(t0);
  const p2 = to.positionAt(t0);
  const d = Math.hypot(p1.x - p2.x, p1.y - p2.y);
  const timeSeconds = 2 * Math.sqrt(d / a);
  const deltaV = a * timeSeconds;
  return { deltaV, timeSeconds };
}

/**
 * Whether a ship with a given delta-v budget can afford a transfer.
 * Running dry strands you (§6: "fuel logistics is a real failure mode").
 */
export function canAfford(budgetMs: number, transfer: TransferResult): boolean {
  return budgetMs >= transfer.deltaV;
}
