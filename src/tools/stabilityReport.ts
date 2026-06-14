/**
 * Headless stability report (§7c).
 *
 * Runs the empty economy across several seeds and prints, per market/commodity,
 * the settled price and the late-run oscillation amplitude as a fraction of the
 * legal price band. A healthy economy shows small amplitudes everywhere.
 *
 *   npm run stability
 */
import { Economy } from "../economy/economy.js";
import { loadEconomyData } from "../economy/data.js";

const TICK_SECONDS = 3600;
const TOTAL_TICKS = 20_000;
const WINDOW = 4_000;
const SEEDS = [0, 1, 2, 3, 7, 13, 42, 99];
const DATA = loadEconomyData();

function bar(frac: number, width = 20): string {
  const n = Math.round(Math.min(1, frac) * width);
  return "#".repeat(n).padEnd(width, "·");
}

let worst = 0;
let worstKey = "";

for (const seed of SEEDS) {
  const econ = new Economy({ seed, data: DATA });
  const min = new Map<string, number>();
  const max = new Map<string, number>();

  for (let t = 0; t < TOTAL_TICKS; t++) {
    econ.step(TICK_SECONDS);
    if (t < TOTAL_TICKS - WINDOW) continue;
    for (const m of econ.markets.values()) {
      for (const [cid, s] of m.states) {
        const key = `${m.id}|${cid}`;
        min.set(key, Math.min(min.get(key) ?? Infinity, s.price));
        max.set(key, Math.max(max.get(key) ?? -Infinity, s.price));
      }
    }
  }

  for (const m of econ.markets.values()) {
    for (const [cid, s] of m.states) {
      const key = `${m.id}|${cid}`;
      const amp = (max.get(key)! - min.get(key)!) / (s.ceiling - s.floor);
      if (amp > worst) {
        worst = amp;
        worstKey = `seed ${seed}  ${key}`;
      }
    }
  }
}

console.log(`Stability sweep: ${SEEDS.length} seeds x ${TOTAL_TICKS} ticks`);
console.log(`Worst late-run price oscillation: ${(worst * 100).toFixed(1)}% of band  [${bar(worst)}]`);
console.log(`  at ${worstKey}`);
console.log(worst <= 0.3 ? "PASS — economy settles on every seed (no death spiral)." : "FAIL — investigate damping.");
process.exit(worst <= 0.3 ? 0 : 1);
