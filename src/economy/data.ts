import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { BodyDef } from "../orbit/body.js";
import { CommodityDef, EconomyData, MarketConfig, RecipeDef } from "./types.js";

const here = dirname(fileURLToPath(import.meta.url));
/** Repo-root /data directory (src/economy -> ../../data). */
const DATA_DIR = join(here, "..", "..", "data");

function readJson<T>(file: string): T {
  return JSON.parse(readFileSync(join(DATA_DIR, file), "utf8")) as T;
}

export function loadEconomyData(): EconomyData {
  const commodities = readJson<{ commodities: CommodityDef[] }>("commodities.json").commodities;
  const recipes = readJson<{ recipes: RecipeDef[] }>("recipes.json").recipes;
  const markets = readJson<{ markets: MarketConfig[] }>("markets.json").markets;
  validate(commodities, recipes, markets);
  return { commodities, recipes, markets };
}

export function loadBodies(): BodyDef[] {
  return readJson<{ bodies: BodyDef[] }>("bodies.json").bodies;
}

/** Fail loudly on data that references undefined commodities/recipes. */
function validate(commodities: CommodityDef[], recipes: RecipeDef[], markets: MarketConfig[]): void {
  const cIds = new Set(commodities.map((c) => c.id));
  const rIds = new Set(recipes.map((r) => r.id));

  for (const c of commodities) {
    if (!(c.priceFloor < c.basePrice && c.basePrice < c.priceCeiling)) {
      throw new Error(`Commodity ${c.id}: require floor < basePrice < ceiling`);
    }
  }
  for (const r of recipes) {
    if (!cIds.has(r.output)) throw new Error(`Recipe ${r.id}: unknown output ${r.output}`);
    for (const input of Object.keys(r.inputs)) {
      if (!cIds.has(input)) throw new Error(`Recipe ${r.id}: unknown input ${input}`);
    }
  }
  for (const m of markets) {
    for (const recipeId of Object.keys(m.production)) {
      if (!rIds.has(recipeId)) throw new Error(`Market ${m.id}: unknown recipe ${recipeId}`);
    }
    for (const cId of Object.keys(m.consumption)) {
      if (!cIds.has(cId)) throw new Error(`Market ${m.id}: unknown consumption commodity ${cId}`);
    }
  }
}
