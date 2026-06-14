import { Body, BodyDef } from "./body.js";
import { hardBurnTransfer, hohmannTransfer, TransferResult } from "./transfer.js";

/**
 * The orrery (§6): a live, deterministic system of orbiting bodies. Positions
 * are pure functions of time, so the whole map can be evaluated at any t.
 */
export class SolSystem {
  readonly bodies = new Map<string, Body>();

  constructor(defs: BodyDef[]) {
    for (const def of defs) this.bodies.set(def.id, new Body(def));
  }

  body(id: string): Body | undefined {
    return this.bodies.get(id);
  }

  /** Economical Hohmann route between two bodies. */
  hohmann(fromId: string, toId: string): TransferResult {
    return hohmannTransfer(this.require(fromId), this.require(toId));
  }

  /** Fast hard-burn route between two bodies, evaluated at departure time t0. */
  hardBurn(fromId: string, toId: string, t0: number, accelG: number): TransferResult {
    return hardBurnTransfer(this.require(fromId), this.require(toId), t0, accelG);
  }

  private require(id: string): Body {
    const b = this.bodies.get(id);
    if (!b) throw new Error(`Unknown body: ${id}`);
    return b;
  }
}
