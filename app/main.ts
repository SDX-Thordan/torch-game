/**
 * TORCH web client bootstrap (APK skeleton).
 *
 * Proves the architecture end to end: the deterministic TypeScript sim core runs
 * unchanged in the browser, driving a live orrery and a market panel. This is the
 * "map + alerts = a complete game" seed (GDD §17); UI depth grows in later PRs.
 */
import { Clock } from "../src/core/clock.js";
import { secondsToDays } from "../src/core/units.js";
import { SolSystem } from "../src/orbit/system.js";
import { Economy } from "../src/economy/economy.js";
import { economyData, bodyDefs } from "./data.js";
import { OrreryView } from "./orreryView.js";

const TICK_SECONDS = 3600; // 1 in-game hour per sim tick.

const clock = new Clock({ tickSeconds: TICK_SECONDS });
const system = new SolSystem(bodyDefs);
const economy = new Economy({ seed: 1, data: economyData });

const canvas = document.getElementById("orrery") as HTMLCanvasElement;
const orrery = new OrreryView(canvas, system);

const clockEl = document.getElementById("clock")!;
const marketsEl = document.getElementById("markets")!;
const transferEl = document.getElementById("transfer")!;

// --- Speed control (real-time-with-pause, GDD §6) ---------------------------
let ticksPerSecond = 1; // 0 = paused; multiplier set by the speed buttons.
for (const btn of document.querySelectorAll<HTMLButtonElement>("#speed-controls button")) {
  btn.addEventListener("click", () => {
    ticksPerSecond = Number(btn.dataset.speed);
    clock.paused = ticksPerSecond === 0;
    document
      .querySelectorAll("#speed-controls button")
      .forEach((b) => b.classList.toggle("active", b === btn));
  });
}

// --- Panel rendering (throttled; the sim runs far faster than the eye) -------
function renderPanel(): void {
  let html = "";
  for (const m of economy.markets.values()) {
    const tags = [...m.states]
      .map(([id, s]) => `<span class="tag">${id} <b>${s.price.toFixed(0)}</b></span>`)
      .join("");
    html += `<div class="market-row"><span class="market-name">${m.name}</span>${tags}</div>`;
  }
  marketsEl.innerHTML = html;

  const hoh = system.hohmann("earth", "ceres");
  const burn = system.hardBurn("earth", "ceres", clock.now, 0.3);
  transferEl.innerHTML =
    `Hohmann&nbsp;&nbsp; Δv ${(hoh.deltaV / 1000).toFixed(1)} km/s · ${secondsToDays(hoh.timeSeconds).toFixed(0)} d<br>` +
    `Hard burn Δv ${(burn.deltaV / 1000).toFixed(0)} km/s · ${secondsToDays(burn.timeSeconds).toFixed(1)} d`;
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
  while (steps-- > 0) {
    const dt = clock.tick();
    if (dt > 0) economy.step(dt);
  }

  clockEl.textContent = `T+${clock.days.toFixed(1)}d`;
  orrery.draw(clock.now);

  panelTimer += dtSec;
  if (panelTimer >= 0.25) {
    panelTimer = 0;
    renderPanel();
  }

  requestAnimationFrame(frame);
}

renderPanel();
requestAnimationFrame(frame);
