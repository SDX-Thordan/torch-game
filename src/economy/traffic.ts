import { Rng } from "../core/rng.js";
import { SECONDS_PER_DAY } from "../core/units.js";
import { SolSystem } from "../orbit/system.js";
import { Economy } from "./economy.js";

/**
 * Physical traffic layer (§7b). NPC haulers fly representative routes between
 * markets, moving surplus to deficit, and **can be intercepted**. Cutting a
 * convoy denies its delivery — a local, temporary shortage that moves prices —
 * and hands its cargo to the interdictor.
 *
 * Design constraint: traffic must not break the §7c stability guarantee. Because
 * haulers only ever move *surplus above target* into *deficit below target* in
 * bounded amounts, the layer damps price spreads between markets rather than
 * amplifying them — it makes the economy more stable, not less. The combined
 * economy+traffic sim is stability-tested across seeds.
 */
export interface Hauler {
  id: number;
  commodity: string;
  originId: string;
  destId: string;
  /** Units of cargo aboard (already removed from the origin at departure). */
  cargo: number;
  departTime: number;
  arriveTime: number;
  state: "enroute" | "delivered" | "intercepted";
}

export interface TrafficOptions {
  /** Max simultaneous haulers in flight. */
  maxHaulers?: number;
  /** Average departures per day when profitable routes exist. */
  spawnsPerDay?: number;
  /** Fraction of a source's *above-target surplus* a hauler may carry. */
  cargoFraction?: number;
  /** Absolute cargo bounds per hauler. */
  minCargo?: number;
  maxCargo?: number;
  /** Minimum price spread (dest - origin) to bother running a route. */
  minSpread?: number;
  /** Acceleration of a freight burn, in g (sets travel time). */
  freightAccelG?: number;
  /** Probability per day a hauler is hit by pirates on an undefended lane. */
  pirateRate?: number;
}

const DEFAULTS: Required<TrafficOptions> = {
  maxHaulers: 24,
  spawnsPerDay: 6,
  cargoFraction: 0.25,
  minCargo: 15,
  maxCargo: 220,
  minSpread: 6,
  freightAccelG: 0.05,
  pirateRate: 0,
};

export interface InterceptResult {
  hauler: Hauler;
  /** Cargo handed to the interdictor. */
  loot: number;
}

export class TrafficSystem {
  private readonly haulers: Hauler[] = [];
  private nextId = 1;
  private readonly opt: Required<TrafficOptions>;

  /** Rolling counters for situation reports. */
  delivered = 0;
  intercepted = 0;

  constructor(
    private readonly economy: Economy,
    private readonly system: SolSystem,
    private readonly rng: Rng,
    options: TrafficOptions = {},
  ) {
    this.opt = { ...DEFAULTS, ...options };
  }

  get active(): readonly Hauler[] {
    return this.haulers;
  }

  /** Fraction of the route completed (0..1) at time `now`. */
  progress(h: Hauler, now: number): number {
    const span = h.arriveTime - h.departTime;
    if (span <= 0) return 1;
    const p = (now - h.departTime) / span;
    return p < 0 ? 0 : p > 1 ? 1 : p;
  }

  step(tickSeconds: number, now: number): void {
    const dtDays = tickSeconds / SECONDS_PER_DAY;

    // Deliver arrivals; drop finished haulers.
    for (const h of this.haulers) {
      if (h.state !== "enroute") continue;
      if (now >= h.arriveTime) {
        this.economy.market(h.destId)?.adjustStock(h.commodity, h.cargo);
        h.state = "delivered";
        this.delivered++;
      }
    }
    this.prune();

    // Pirates pick off an undefended convoy (off by default).
    if (this.opt.pirateRate > 0 && this.rng.chance(this.opt.pirateRate * dtDays)) {
      const enroute = this.haulers.filter((h) => h.state === "enroute");
      if (enroute.length > 0) {
        const victim = enroute[this.rng.int(0, enroute.length - 1)]!;
        this.intercept(victim.id);
      }
    }

    // Dispatch new traffic on the most profitable route.
    if (
      this.haulers.length < this.opt.maxHaulers &&
      this.rng.chance(this.opt.spawnsPerDay * dtDays)
    ) {
      this.trySpawn(now);
    }
  }

  /**
   * Interdict a hauler: its delivery is denied (the cargo never reaches the
   * destination) and is handed to the interdictor. The §7b hook for player and
   * pirate raids.
   */
  intercept(id: number): InterceptResult | undefined {
    const h = this.haulers.find((x) => x.id === id && x.state === "enroute");
    if (!h) return undefined;
    h.state = "intercepted";
    this.intercepted++;
    return { hauler: h, loot: h.cargo };
  }

  private prune(): void {
    for (let i = this.haulers.length - 1; i >= 0; i--) {
      if (this.haulers[i]!.state !== "enroute") this.haulers.splice(i, 1);
    }
  }

  /**
   * Pick the most profitable arbitrage route across all commodities: ship from
   * the cheapest market (which must hold surplus above target) to the dearest
   * (which must have storage room). Price-driven rather than deficit-driven,
   * because a self-sufficient economy rarely runs an outright deficit but its
   * equilibrium prices still differ between markets. Moving goods down the price
   * gradient damps the spread — stabilizing, never amplifying.
   */
  private trySpawn(now: number): void {
    let best:
      | { commodity: string; originId: string; destId: string; spread: number; cargo: number }
      | undefined;

    for (const commodity of this.economy.commodities.keys()) {
      let src: { id: string; price: number; surplus: number } | undefined;
      let dst: { id: string; price: number; room: number } | undefined;

      for (const m of this.economy.markets.values()) {
        const s = m.states.get(commodity);
        if (!s) continue;
        const surplus = s.stock - m.target(commodity);
        const room = s.capacity - s.stock;
        // Cheapest exporter that has surplus to spare.
        if (surplus > 0 && (!src || s.price < src.price)) {
          src = { id: m.id, price: s.price, surplus };
        }
        // Dearest importer that still has storage room.
        if (room > 0 && (!dst || s.price > dst.price)) {
          dst = { id: m.id, price: s.price, room };
        }
      }

      if (!src || !dst || src.id === dst.id) continue;
      const spread = dst.price - src.price;
      if (spread < this.opt.minSpread) continue;

      const cargo = Math.min(this.opt.maxCargo, src.surplus * this.opt.cargoFraction, dst.room);
      if (cargo < this.opt.minCargo) continue;

      if (!best || spread > best.spread) {
        best = { commodity, originId: src.id, destId: dst.id, spread, cargo };
      }
    }

    if (!best) return;

    const origin = this.economy.market(best.originId)!;
    // Load: actually remove cargo from the origin (clamped).
    const loaded = -origin.adjustStock(best.commodity, -best.cargo);
    if (loaded < this.opt.minCargo) return;

    const originBody = origin.bodyId;
    const destBody = this.economy.market(best.destId)!.bodyId;
    const freight = this.system.hardBurn(originBody, destBody, now, this.opt.freightAccelG);

    this.haulers.push({
      id: this.nextId++,
      commodity: best.commodity,
      originId: best.originId,
      destId: best.destId,
      cargo: loaded,
      departTime: now,
      arriveTime: now + freight.timeSeconds,
      state: "enroute",
    });
  }
}
