/**
 * Headless demo driver for the TORCH core sim.
 *
 * Mirrors the §18 build order: a deterministic clock advances both the orrery
 * and the living economy, and we print a small situation report. No graphics —
 * this is the simulation core the rest of the game will sit on top of.
 *
 *   npm run sim
 */
import { Clock } from "./core/clock.js";
import { secondsToDays } from "./core/units.js";
import { SolSystem } from "./orbit/system.js";
import { Economy } from "./economy/economy.js";
import { loadBodies } from "./economy/data.js";

function fmt(n: number, w = 8): string {
  return n.toFixed(1).padStart(w);
}

function main(): void {
  const seed = Number(process.env.SEED ?? 1);
  const days = Number(process.env.DAYS ?? 200);

  const clock = new Clock({ tickSeconds: 3600 }); // 1 hour / tick
  const system = new SolSystem(loadBodies());
  const economy = new Economy({ seed });

  const ticks = Math.round((days * 86_400) / clock.tickSeconds);
  for (let i = 0; i < ticks; i++) {
    const dt = clock.tick();
    economy.step(dt);
  }

  console.log(`TORCH headless sim — seed ${seed}, ${secondsToDays(clock.now).toFixed(0)} days, ${clock.tickCount} ticks\n`);

  // --- Orrery snapshot ------------------------------------------------------
  console.log("ORRERY (heliocentric angle / period):");
  for (const b of system.bodies.values()) {
    const angleDeg = ((b.angleAt(clock.now) * 180) / Math.PI) % 360;
    const periodDays = b.periodSeconds / 86_400;
    console.log(`  ${b.name.padEnd(16)} angle ${fmt(angleDeg, 6)}°   period ${fmt(periodDays, 8)} d`);
  }

  // --- A sample transfer ----------------------------------------------------
  const hoh = system.hohmann("earth", "ceres");
  const burn = system.hardBurn("earth", "ceres", clock.now, 0.3);
  console.log("\nTRANSFER Earth -> Ceres:");
  console.log(`  Hohmann   dv ${fmt(hoh.deltaV, 9)} m/s   time ${fmt(secondsToDays(hoh.timeSeconds), 7)} d`);
  console.log(`  Hard burn dv ${fmt(burn.deltaV, 9)} m/s   time ${fmt(secondsToDays(burn.timeSeconds), 7)} d`);

  // --- Market snapshot ------------------------------------------------------
  console.log("\nMARKET PRICES (settled with zero player input):");
  for (const m of economy.markets.values()) {
    const parts: string[] = [];
    for (const [id, s] of m.states) parts.push(`${id}=${s.price.toFixed(0)}`);
    console.log(`  ${m.name.padEnd(24)} ${parts.join("  ")}`);
  }
}

main();
