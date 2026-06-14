import { auToMetres, MU_SUN } from "../core/units.js";
import { Vec2, vec2 } from "../core/vec2.js";

/**
 * A simplified circular, coplanar orbit (§6: "simplified, deterministic orbits
 * ... not full n-body — fidelity serves playability").
 *
 * Position is a pure function of time, so the orrery is perfectly deterministic
 * and cheap to evaluate at any t without stepping.
 */
export interface BodyDef {
  id: string;
  name: string;
  /** Semi-major axis in AU. For moons we still approximate with a heliocentric radius. */
  semiMajorAxisAu: number;
  /** Phase angle at t=0, in radians. Lets us spread bodies around the system. */
  phase0?: number;
  /** Optional explicit faction/owner tag for flavour and economy wiring. */
  owner?: string;
}

export class Body {
  readonly id: string;
  readonly name: string;
  readonly radiusM: number;
  readonly phase0: number;
  readonly owner: string | undefined;
  /** Orbital period in seconds, from Kepler's third law. */
  readonly periodSeconds: number;
  /** Mean angular rate, rad/s. */
  readonly angularRate: number;

  constructor(def: BodyDef) {
    this.id = def.id;
    this.name = def.name;
    this.radiusM = auToMetres(def.semiMajorAxisAu);
    this.phase0 = def.phase0 ?? 0;
    this.owner = def.owner;
    this.periodSeconds = 2 * Math.PI * Math.sqrt((this.radiusM * this.radiusM * this.radiusM) / MU_SUN);
    this.angularRate = (2 * Math.PI) / this.periodSeconds;
  }

  /** Heliocentric angle (radians) at time t. */
  angleAt(t: number): number {
    return this.phase0 + this.angularRate * t;
  }

  /** Heliocentric position (metres) at time t. */
  positionAt(t: number): Vec2 {
    const a = this.angleAt(t);
    return vec2(this.radiusM * Math.cos(a), this.radiusM * Math.sin(a));
  }

  /** Circular orbital speed (m/s). */
  orbitalSpeed(): number {
    return Math.sqrt(MU_SUN / this.radiusM);
  }
}
