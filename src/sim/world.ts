import { Clock } from "../core/clock.js";
import { Rng } from "../core/rng.js";
import { BodyDef } from "../orbit/body.js";
import { SolSystem } from "../orbit/system.js";
import { Economy } from "../economy/economy.js";
import { TrafficSystem, TrafficOptions } from "../economy/traffic.js";
import { EconomyData, EconomyTuning } from "../economy/types.js";

export interface WorldOptions {
  seed?: number;
  data: EconomyData;
  bodies: BodyDef[];
  tickSeconds?: number;
  tuning?: Partial<EconomyTuning>;
  traffic?: TrafficOptions;
}

/**
 * Top-level deterministic world: the clock, the orrery, the living economy, and
 * the physical traffic layer composed into one steppable unit. This is the
 * object every front end (headless CLI, web client, tests) drives. Content is
 * injected, so the world runs identically in Node and the browser.
 */
export class World {
  readonly clock: Clock;
  readonly system: SolSystem;
  readonly economy: Economy;
  readonly traffic: TrafficSystem;

  constructor(opts: WorldOptions) {
    const seed = opts.seed ?? 1;
    this.clock = new Clock({ tickSeconds: opts.tickSeconds ?? 3600 });
    this.system = new SolSystem(opts.bodies);
    this.economy = new Economy({ seed, data: opts.data, tuning: opts.tuning });
    // Derive an independent RNG stream for traffic so it never disturbs the
    // economy's own deterministic stream.
    this.traffic = new TrafficSystem(
      this.economy,
      this.system,
      new Rng(seed ^ 0x9e3779b9),
      opts.traffic,
    );
  }

  /** Advance one tick. Returns dt applied (0 if paused). */
  step(): number {
    const dt = this.clock.tick();
    if (dt > 0) {
      this.economy.step(dt);
      this.traffic.step(dt, this.clock.now);
    }
    return dt;
  }

  /** Run `ticks` ticks (headless helper). */
  run(ticks: number): void {
    for (let i = 0; i < ticks; i++) this.step();
  }
}
