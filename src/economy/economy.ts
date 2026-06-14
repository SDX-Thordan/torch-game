import { Rng } from "../core/rng.js";
import { SECONDS_PER_DAY } from "../core/units.js";
import { Market } from "./market.js";
import { loadEconomyData } from "./data.js";
import {
  CommodityDef,
  DEFAULT_TUNING,
  EconomyData,
  EconomyTuning,
  RecipeDef,
} from "./types.js";

export interface EconomyOptions {
  seed?: number;
  tuning?: Partial<EconomyTuning>;
  /** Override the data set (tests inject custom worlds). Defaults to /data. */
  data?: EconomyData;
}

/**
 * The living economy (§7): a collection of self-stabilizing markets. It runs
 * with zero player input and must reach a damped equilibrium on any seed — the
 * headless acceptance criterion in §7c.
 */
export class Economy {
  readonly markets = new Map<string, Market>();
  readonly commodities = new Map<string, CommodityDef>();
  readonly tuning: EconomyTuning;
  private readonly rng: Rng;

  constructor(opts: EconomyOptions = {}) {
    const data = opts.data ?? loadEconomyData();
    this.tuning = { ...DEFAULT_TUNING, ...opts.tuning };
    this.rng = new Rng(opts.seed ?? 1);

    for (const c of data.commodities) this.commodities.set(c.id, c);
    const recipesById = new Map<string, RecipeDef>(data.recipes.map((r) => [r.id, r]));

    for (const mc of data.markets) {
      this.markets.set(mc.id, new Market(mc, this.commodities, recipesById, this.tuning));
    }
  }

  /** Advance every market by one tick of `tickSeconds`. */
  step(tickSeconds: number): void {
    const dtDays = tickSeconds / SECONDS_PER_DAY;
    for (const m of this.markets.values()) m.step(dtDays, this.rng);
  }

  /** Run `ticks` ticks (helper for headless runs/tests). */
  run(ticks: number, tickSeconds: number): void {
    for (let i = 0; i < ticks; i++) this.step(tickSeconds);
  }

  market(id: string): Market | undefined {
    return this.markets.get(id);
  }

  /** Convenience price lookup across the whole economy. */
  price(marketId: string, commodityId: string): number | undefined {
    return this.markets.get(marketId)?.price(commodityId);
  }
}
