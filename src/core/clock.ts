import { SECONDS_PER_DAY } from "./units.js";

/**
 * The game clock (§6: "Real-time with pause").
 *
 * Deterministic fixed-step integration: the sim only ever advances in whole
 * `tickSeconds` steps. Game speed and "real time" are a presentation concern —
 * the sim itself just counts ticks, which is what makes it reproducible and
 * headless-testable.
 *
 * Backgrounding the app pauses the clock (§6: "the clock pauses when the app is
 * backgrounded — you never lose a fleet while away"); that is modelled simply as
 * `paused`.
 */
export interface ClockOptions {
  /** Seconds of in-game time advanced per tick. Default: 1 hour. */
  tickSeconds?: number;
}

export class Clock {
  readonly tickSeconds: number;
  private elapsedSeconds = 0;
  private ticks = 0;
  paused = false;

  constructor(opts: ClockOptions = {}) {
    this.tickSeconds = opts.tickSeconds ?? 3600;
  }

  /** Advance exactly one tick. Returns the dt applied (0 if paused). */
  tick(): number {
    if (this.paused) return 0;
    this.elapsedSeconds += this.tickSeconds;
    this.ticks += 1;
    return this.tickSeconds;
  }

  get now(): number {
    return this.elapsedSeconds;
  }

  get tickCount(): number {
    return this.ticks;
  }

  get days(): number {
    return this.elapsedSeconds / SECONDS_PER_DAY;
  }
}
