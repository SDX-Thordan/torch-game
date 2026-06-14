/** Economic tier of a commodity, for UI grouping and chain reasoning. */
export type Tier = "raw" | "refined" | "component" | "assembled";

export interface CommodityDef {
  id: string;
  name: string;
  tier: Tier;
  basePrice: number;
  priceFloor: number;
  priceCeiling: number;
}

/** A production recipe: one output, zero or more inputs (qty per unit of output). */
export interface RecipeDef {
  id: string;
  output: string;
  inputs: Record<string, number>;
}

export interface MarketConfig {
  id: string;
  name: string;
  bodyId: string;
  faction: string;
  /** recipeId -> base output rate (units per day). */
  production: Record<string, number>;
  /** commodityId -> base end-use demand (units per day). */
  consumption: Record<string, number>;
}

export interface EconomyData {
  commodities: CommodityDef[];
  recipes: RecipeDef[];
  markets: MarketConfig[];
}

/** Live per-commodity state inside a market. */
export interface CommodityState {
  id: string;
  stock: number;
  /** Desired equilibrium stock — derived from throughput. */
  target: number;
  /** Hard storage ceiling; stock never exceeds this. */
  capacity: number;
  price: number;
  floor: number;
  ceiling: number;
  basePrice: number;
}

/**
 * Tunable constants for the stabilizer laws (§7c). These are the knobs that
 * keep the market in a damped negative-feedback loop instead of a spiral.
 */
export interface EconomyTuning {
  /** Stock buffer expressed in days of throughput. */
  coverageDays: number;
  /** capacity = target * capacityMultiple. */
  capacityMultiple: number;
  /** Emergency production ramp ceiling (multiple of base rate). */
  emergencyCap: number;
  /** Below this fill ratio, NPCs start rationing consumption. */
  rationThreshold: number;
  /** Floor on consumption scale under extreme scarcity (never fully zero). */
  minConsumeScale: number;
  /** Price relaxation rate toward target, per day. */
  priceLerpPerDay: number;
  /** Amplitude of seeded NPC demand noise (0 = none). */
  demandNoise: number;
}

export const DEFAULT_TUNING: EconomyTuning = {
  coverageDays: 8,
  capacityMultiple: 3,
  emergencyCap: 2,
  rationThreshold: 0.5,
  minConsumeScale: 0.2,
  priceLerpPerDay: 0.2,
  demandNoise: 0.15,
};
