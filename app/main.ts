/**
 * TORCH web client bootstrap.
 *
 * Drives the deterministic `World` (clock + orrery + living economy + traffic)
 * unchanged in the browser, rendering a live orrery, in-flight convoys, and a
 * market panel. The "map + alerts = a complete game" seed (GDD §17); UI depth
 * grows in later PRs.
 */
import { secondsToDays } from "../src/core/units.js";
import { World } from "../src/sim/world.js";
import { economyData, bodyDefs } from "./data.js";
import { OrreryView, HaulerSegment } from "./orreryView.js";

const world = new World({ seed: 1, data: economyData, bodies: bodyDefs });

const canvas = document.getElementById("orrery") as HTMLCanvasElement;
const orrery = new OrreryView(canvas, world.system);

const clockEl = document.getElementById("clock")!;
const marketsEl = document.getElementById("markets")!;
const transferEl = document.getElementById("transfer")!;

// Resolve market ids -> body ids once for hauler rendering.
const marketBody = new Map<string, string>();
for (const m of world.economy.markets.values()) marketBody.set(m.id, m.bodyId);

// --- Speed control (real-time-with-pause, GDD §6) ---------------------------
let ticksPerSecond = 1; // 0 = paused; multiplier set by the speed buttons.
for (const btn of document.querySelectorAll<HTMLButtonElement>("#speed-controls button")) {
  btn.addEventListener("click", () => {
    ticksPerSecond = Number(btn.dataset.speed);
    world.clock.paused = ticksPerSecond === 0;
    document
      .querySelectorAll("#speed-controls button")
      .forEach((b) => b.classList.toggle("active", b === btn));
  });
}

// --- Panel rendering (throttled; the sim runs far faster than the eye) -------
function renderPanel(): void {
  let html = "";
  for (const m of world.economy.markets.values()) {
    const tags = [...m.states]
      .map(([id, s]) => `<span class="tag">${id} <b>${s.price.toFixed(0)}</b></span>`)
      .join("");
    html += `<div class="market-row"><span class="market-name">${m.name}</span>${tags}</div>`;
  }
  marketsEl.innerHTML = html;

  const hoh = world.system.hohmann("earth", "ceres");
  const burn = world.system.hardBurn("earth", "ceres", world.clock.now, 0.3);
  transferEl.innerHTML =
    `Convoys in flight: <b>${world.traffic.active.length}</b> · delivered ${world.traffic.delivered} · raided ${world.traffic.intercepted}<br>` +
    `Hohmann&nbsp;&nbsp; Δv ${(hoh.deltaV / 1000).toFixed(1)} km/s · ${secondsToDays(hoh.timeSeconds).toFixed(0)} d<br>` +
    `Hard burn Δv ${(burn.deltaV / 1000).toFixed(0)} km/s · ${secondsToDays(burn.timeSeconds).toFixed(1)} d`;
}

function haulerSegments(): HaulerSegment[] {
  const t = world.clock.now;
  const segs: HaulerSegment[] = [];
  for (const h of world.traffic.active) {
    const fromBody = marketBody.get(h.originId);
    const toBody = marketBody.get(h.destId);
    if (fromBody && toBody) segs.push({ fromBody, toBody, progress: world.traffic.progress(h, t) });
  }
  return segs;
}

// --- Main loop --------------------------------------------------------------
let acc = 0;
let last = performance.now();
let panelTimer = 0;

function frame(now: number): void {
  const dtSec = Math.min(0.1, (now - last) / 1000);
  last = now;

  // Advance whole sim ticks at the chosen rate.
  acc += dtSec * ticksPerSecond;
  let steps = Math.floor(acc);
  acc -= steps;
  while (steps-- > 0) world.step();

  clockEl.textContent = `T+${world.clock.days.toFixed(1)}d`;
  orrery.draw(world.clock.now);
  orrery.drawHaulers(haulerSegments());

  panelTimer += dtSec;
  if (panelTimer >= 0.25) {
    panelTimer = 0;
    renderPanel();
  }

  requestAnimationFrame(frame);
}

renderPanel();
requestAnimationFrame(frame);
