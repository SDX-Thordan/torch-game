/**
 * Canvas renderer for the orrery (GDD §17: "the orrery as a glowing schematic").
 * Deliberately 2D and cheap — matches the diorama aesthetic and runs well in a
 * webview on a phone.
 */
import type { SolSystem } from "../src/orbit/system.js";

const FACTION_COLOUR: Record<string, string> = {
  earth: "#5b8def",
  mars: "#e2603e",
  belt: "#e9c46a",
};

/** A hauler leg to draw: between two body ids, at fractional progress. */
export interface HaulerSegment {
  fromBody: string;
  toBody: string;
  progress: number;
}

export class OrreryView {
  private readonly ctx: CanvasRenderingContext2D;
  /** Screen position of each body after the most recent draw(). */
  readonly screen = new Map<string, { x: number; y: number }>();

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly system: SolSystem,
  ) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("2D canvas context unavailable");
    this.ctx = ctx;
    this.resize();
    window.addEventListener("resize", () => this.resize());
  }

  resize(): void {
    const dpr = window.devicePixelRatio || 1;
    const rect = this.canvas.getBoundingClientRect();
    this.canvas.width = Math.max(1, Math.floor(rect.width * dpr));
    this.canvas.height = Math.max(1, Math.floor(rect.height * dpr));
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  }

  /** Largest orbital radius, for scaling. */
  private maxRadius(): number {
    let max = 1;
    for (const b of this.system.bodies.values()) max = Math.max(max, b.radiusM);
    return max;
  }

  /** Compress radii (sqrt) so inner planets aren't crushed against the Sun. */
  private screenRadius(rM: number, pxMax: number): number {
    return Math.sqrt(rM / this.maxRadius()) * pxMax;
  }

  draw(t: number): void {
    const { ctx } = this;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    const cx = w / 2;
    const cy = h / 2;
    const pxMax = Math.min(w, h) / 2 - 28;

    ctx.clearRect(0, 0, w, h);

    // Sun.
    ctx.fillStyle = "#ffd27d";
    ctx.beginPath();
    ctx.arc(cx, cy, 6, 0, Math.PI * 2);
    ctx.fill();

    for (const b of this.system.bodies.values()) {
      const rPx = this.screenRadius(b.radiusM, pxMax);

      // Orbit ring.
      ctx.strokeStyle = "rgba(90,120,160,0.18)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.arc(cx, cy, rPx, 0, Math.PI * 2);
      ctx.stroke();

      // Body.
      const a = b.angleAt(t);
      const x = cx + rPx * Math.cos(a);
      const y = cy + rPx * Math.sin(a);
      const colour = FACTION_COLOUR[b.owner ?? ""] ?? "#9fb6d4";

      ctx.fillStyle = colour;
      ctx.beginPath();
      ctx.arc(x, y, 4, 0, Math.PI * 2);
      ctx.fill();

      ctx.fillStyle = "#9fb6d4";
      ctx.font = "11px ui-monospace, monospace";
      ctx.fillText(b.name, x + 7, y + 3);

      this.screen.set(b.id, { x, y });
    }
  }

  /** Draw in-flight haulers as ticks moving along their routes (§7b). */
  drawHaulers(segments: HaulerSegment[]): void {
    const { ctx } = this;
    ctx.fillStyle = "#ffb347";
    for (const seg of segments) {
      const a = this.screen.get(seg.fromBody);
      const b = this.screen.get(seg.toBody);
      if (!a || !b) continue;
      const x = a.x + (b.x - a.x) * seg.progress;
      const y = a.y + (b.y - a.y) * seg.progress;
      ctx.fillRect(x - 1.5, y - 1.5, 3, 3);
    }
  }
}
