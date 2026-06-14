/**
 * Browser-side world data, assembled from the bundled JSON in /data.
 *
 * The sim core never reads from disk (CLAUDE.md §3 portability rule); the Node
 * loaders use `fs`, but the web client imports the same JSON via the bundler and
 * passes it into the core. This is the only place the web app touches content.
 */
import type { BodyDef } from "../src/orbit/body.js";
import type { EconomyData } from "../src/economy/types.js";

import commoditiesJson from "../data/commodities.json";
import recipesJson from "../data/recipes.json";
import marketsJson from "../data/markets.json";
import bodiesJson from "../data/bodies.json";

export const economyData: EconomyData = {
  commodities: commoditiesJson.commodities as unknown as EconomyData["commodities"],
  recipes: recipesJson.recipes as unknown as EconomyData["recipes"],
  markets: marketsJson.markets as unknown as EconomyData["markets"],
};

export const bodyDefs: BodyDef[] = bodiesJson.bodies as unknown as BodyDef[];
