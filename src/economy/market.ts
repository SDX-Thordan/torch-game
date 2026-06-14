import { clamp, lerp, smoothstep } from "../core/units.js";
import { Rng } from "../core/rng.js";
import {
  CommodityDef,
  CommodityState,
  EconomyTuning,
  MarketConfig,
  RecipeDef,
} from "./types.js";

/** A recipe instance running in a market, with its base output rate. */
interface RecipeRun {
  recipe: RecipeDef;
  baseRate: number; // units of output per day
}

/**
 * A single market: real inventory that fills and drains from NPC production and
 * consumption (§7a), governed by the stabilizer laws (§7c). It self-corrects to
 * a damped equilibrium with no player input.
 */
export class Market {
  readonly id: string;
  readonly name: string;
  readonly bodyId: string;
  readonly faction: string;
  readonly states = new Map<string, CommodityState>();

  private readonly recipes: RecipeRun[];
  private readonly endUse: Map<string, number>; // commodityId -> base demand/day
  private readonly tuning: EconomyTuning;

  constructor(
    config: MarketConfig,
    commodities: Map<string, CommodityDef>,
    recipesById: Map<string, RecipeDef>,
    tuning: EconomyTuning,
  ) {
    this.id = config.id;
    this.name = config.name;
    this.bodyId = config.bodyId;
    this.faction = config.faction;
    this.tuning = tuning;

    this.recipes = Object.entries(config.production).map(([recipeId, baseRate]) => {
      const recipe = recipesById.get(recipeId)!;
      return { recipe, baseRate };
    });
    this.endUse = new Map(Object.entries(config.consumption));

    this.initStates(commodities);
  }

  /**
   * Size each commodity's target stock from its throughput (the larger of the
   * production and consumption flows it sees). This keeps the control laws
   * well-conditioned regardless of the absolute rates in the data.
   */
  private initStates(commodities: Map<string, CommodityDef>): void {
    const produced = new Map<string, number>();
    const consumed = new Map<string, number>();

    for (const run of this.recipes) {
      produced.set(run.recipe.output, (produced.get(run.recipe.output) ?? 0) + run.baseRate);
      for (const [inputId, qty] of Object.entries(run.recipe.inputs)) {
        consumed.set(inputId, (consumed.get(inputId) ?? 0) + run.baseRate * qty);
      }
    }
    for (const [id, rate] of this.endUse) {
      consumed.set(id, (consumed.get(id) ?? 0) + rate);
    }

    const ids = new Set([...produced.keys(), ...consumed.keys()]);
    for (const id of ids) {
      const def = commodities.get(id);
      if (!def) throw new Error(`Market ${this.id} references unknown commodity ${id}`);
      const throughput = Math.max(produced.get(id) ?? 0, consumed.get(id) ?? 0, 1);
      const target = throughput * this.tuning.coverageDays;
      this.states.set(id, {
        id,
        stock: target, // start near equilibrium
        target,
        capacity: target * this.tuning.capacityMultiple,
        price: def.basePrice,
        floor: def.priceFloor,
        ceiling: def.priceCeiling,
        basePrice: def.basePrice,
      });
    }
  }

  price(id: string): number | undefined {
    return this.states.get(id)?.price;
  }

  stock(id: string): number | undefined {
    return this.states.get(id)?.stock;
  }

  /**
   * Player or pirate interdiction: instantly remove a fraction of stock,
   * creating a local, temporary shortage (§7b). The stabilizers absorb it.
   */
  applyShock(id: string, fraction: number): void {
    const s = this.states.get(id);
    if (!s) return;
    s.stock = clamp(s.stock * (1 - clamp(fraction, 0, 1)), 0, s.capacity);
  }

  /**
   * Advance the market by `dtDays`. `rng` drives optional NPC demand noise so
   * the equilibrium is stress-tested, not just a static fixed point.
   */
  step(dtDays: number, rng: Rng): void {
    const t = this.tuning;
    // Working copy of stock used for input availability this tick; outputs are
    // accumulated separately so a recipe's output isn't usable as its own input
    // within the same tick.
    const work = new Map<string, number>();
    const produced = new Map<string, number>();
    for (const [id, s] of this.states) work.set(id, s.stock);

    // --- Production (recipes) -------------------------------------------------
    for (const run of this.recipes) {
      const out = run.recipe.output;
      const outState = this.states.get(out)!;
      const fill = outState.stock / outState.target;
      // Self-throttling: ramp up to emergencyCap when scarce, shut off as stock
      // approaches 2x target. This is the core negative feedback.
      const selfScale = clamp(1 + (1 - fill), 0, t.emergencyCap);
      let desiredOut = run.baseRate * selfScale;

      // Input gating: cannot output more than available inputs allow.
      for (const [inputId, qty] of Object.entries(run.recipe.inputs)) {
        if (qty <= 0) continue;
        const avail = work.get(inputId) ?? 0;
        const maxOut = avail / (qty * dtDays);
        if (maxOut < desiredOut) desiredOut = maxOut;
      }
      desiredOut = Math.max(0, desiredOut);

      for (const [inputId, qty] of Object.entries(run.recipe.inputs)) {
        if (qty <= 0) continue;
        work.set(inputId, (work.get(inputId) ?? 0) - desiredOut * qty * dtDays);
      }
      produced.set(out, (produced.get(out) ?? 0) + desiredOut * dtDays);
    }

    // --- Consumption (end use, with rationing) -------------------------------
    for (const [id, baseRate] of this.endUse) {
      const s = this.states.get(id)!;
      const fill = s.stock / s.target;
      const rationScale = clamp(fill / t.rationThreshold, t.minConsumeScale, 1);
      const noise = t.demandNoise > 0 ? 1 + (rng.next() * 2 - 1) * t.demandNoise : 1;
      const draw = baseRate * rationScale * Math.max(0, noise) * dtDays;
      const avail = work.get(id) ?? 0;
      work.set(id, avail - Math.min(draw, avail));
    }

    // --- Commit stock + relax prices -----------------------------------------
    const priceLerp = clamp(t.priceLerpPerDay * dtDays, 0, 1);
    for (const [id, s] of this.states) {
      const next = (work.get(id) ?? 0) + (produced.get(id) ?? 0);
      s.stock = clamp(next, 0, s.capacity);

      // Damped, stock-based pricing — never raw supply/demand (§7c).
      // The target price is anchored so that stock == target => basePrice,
      // sliding toward the ceiling under scarcity and the floor under glut.
      let priceTarget: number;
      if (s.stock <= s.target) {
        const f = s.target > 0 ? s.stock / s.target : 1;
        priceTarget = lerp(s.ceiling, s.basePrice, smoothstep(f));
      } else {
        const f = s.target > 0 ? clamp((s.stock - s.target) / s.target, 0, 1) : 1;
        priceTarget = lerp(s.basePrice, s.floor, smoothstep(f));
      }
      s.price = clamp(s.price + (priceTarget - s.price) * priceLerp, s.floor, s.ceiling);
    }
  }
}
