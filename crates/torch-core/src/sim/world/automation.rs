//! `automation` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// The authored thread — opening missions + the gate mystery (§0.1/§16).
    pub fn missions(&self) -> &crate::sim::missions::Missions {
        &self.missions
    }

    /// Adopt corp name preset `i` (§14 expressive identity).
    pub fn set_corp_name_preset(&mut self, i: usize) {
        self.corp.set_name_preset(i);
    }

    /// Cycle the fleet livery colour (§14); returns the new index.
    pub fn cycle_corp_livery(&mut self) -> usize {
        self.corp.cycle_livery()
    }

    /// Remove the hauler at `index`, denying its delivery and tagging the
    /// resulting shortage at the destination (§7b). Returns the cut hauler.
    pub(crate) fn cut_hauler(&mut self, index: usize) -> Hauler {
        let h = self.haulers.remove(index);
        self.events.push(Event::HaulerInterdicted { id: h.id });
        self.events.push(Event::Scarcity {
            market: h.dest,
            commodity: h.commodity,
        });
        h
    }

    /// Run the standing automation policy this tick (§12 run-by-exception): the
    /// interdiction patrol cuts matching shipping on its cadence, and research is
    /// auto-invested. The player set the policy; the managers do the work.
    pub(crate) fn run_automation(&mut self) {
        let pol = self.policy; // Copy — no borrow held over the mutations below
        if pol.interdiction.enabled && self.tick.is_multiple_of(AUTOMATION_INTERVAL) {
            let target = self
                .haulers
                .iter()
                .enumerate()
                .filter(|(_, h)| h.qty >= pol.interdiction.min_cargo)
                .filter(|(_, h)| match pol.interdiction.target {
                    Some(f) => self.markets[h.origin].faction() == f,
                    None => true,
                })
                .max_by_key(|(_, h)| h.qty)
                .map(|(i, _)| i);
            if let Some(i) = target {
                let outcome = resolve(&self.haulers[i], &pol.patrol, self.tick, &mut self.rng);
                if outcome == Interdiction::Interdicted {
                    let h = self.cut_hauler(i);
                    self.ripple_reputation(&h); // the player's managed asset → their tab
                }
            }
        }
        if pol.auto_research {
            if let Some(i) = self.progression.research.cheapest_researchable() {
                let _ = self.progression.research.research(i);
            }
        }
    }

    /// NPC pirates periodically strike at the fattest cargo in flight (§13).
    /// Resolve one ambient raider strike against the fattest in-flight cargo (§13);
    /// the *when* is decided by the pressure layer ([`Sim::run_pressure`]), not a
    /// raw interval. Returns whether a convoy was actually cut (a flashpoint).
    pub(crate) fn pirate_raid(&mut self) -> bool {
        if self.haulers.is_empty() {
            return false;
        }
        let target = self
            .haulers
            .iter()
            .enumerate()
            .max_by_key(|(_, h)| h.qty)
            .map(|(i, _)| i);
        if let Some(i) = target {
            let outcome = resolve(&self.haulers[i], &self.pirate, self.tick, &mut self.rng);
            if outcome == Interdiction::Interdicted {
                self.cut_hauler(i); // pirates, not the player → no reputation hit
                return true;
            }
        }
        false
    }

    /// Land cargo for any hauler arriving this tick, damping the spread.
    pub(crate) fn deliver_arrivals(&mut self) {
        let tick = self.tick;
        let mut landed: Vec<(usize, usize, i64, u64)> = Vec::new();
        self.haulers.retain(|h| {
            if h.arrival_tick == tick {
                landed.push((h.dest, h.commodity, h.qty, h.id));
                false
            } else {
                true
            }
        });
        for (dest, commodity, qty, id) in landed {
            // EP2: an NPC delivery into a market you own pays a tariff to the treasury
            // — your empire earns from the living economy autonomously. (Pure credit,
            // no RNG; owned-only, so a fresh sim is byte-identical and §7c holds.)
            if self.market_is_owned(dest) {
                let value = self.markets[dest].price(commodity) * qty;
                self.corp.credit(value * NPC_TARIFF_BP / FEE_DEN);
            }
            self.markets[dest].add_stock(commodity, qty);
            self.events.push(Event::HaulerArrived { id });
        }
    }

    /// Spawn at most one arbitrage hauler on the most profitable open route.
    pub(crate) fn spawn_traffic(&mut self) {
        if !self.tick.is_multiple_of(SPAWN_INTERVAL) || self.haulers.len() >= MAX_HAULERS {
            return;
        }
        let Some((commodity, origin, dest, qty)) = self.best_route() else {
            return;
        };
        // Lift the cargo now (origin sheds surplus); land it on arrival.
        self.markets[origin].remove_stock(commodity, qty);
        let origin_pos = orbit::position_of(&self.bodies, self.markets[origin].body(), self.tick);
        let dest_pos = orbit::position_of(&self.bodies, self.markets[dest].body(), self.tick);
        let (dx, dy) = (dest_pos.0 - origin_pos.0, dest_pos.1 - origin_pos.1);
        let dist = (dx * dx + dy * dy).isqrt();
        let travel = brachistochrone_ticks(dist, ACCEL_CIV).max(MIN_TRAVEL);
        let id = self.next_hauler_id;
        self.next_hauler_id += 1;
        self.events.push(Event::HaulerDeparted {
            id,
            commodity,
            origin,
            dest,
            qty,
        });
        self.haulers.push(Hauler {
            id,
            commodity,
            origin,
            dest,
            qty,
            depart_tick: self.tick,
            arrival_tick: self.tick + travel,
            origin_pos,
            dest_pos,
        });
    }

    /// The (commodity, origin, dest, qty) with the largest profitable spread
    /// where the origin has surplus and the destination has room.
    pub(crate) fn best_route(&self) -> Option<(usize, usize, usize, i64)> {
        let n = self.markets[0].defs().len();
        // NPC haulers route only the **inner** economy — the far-side markets (§17)
        // are unreachable to ambient traffic, so the inner game is unchanged.
        let m = self.far_market_start;
        let mut best: Option<(usize, usize, usize, i64)> = None;
        let mut best_spread = MIN_SPREAD;
        for c in 0..n {
            let qty = (self.markets[0].defs()[c].target_stock / 10).max(1);
            // Every ordered market pair — so a third market (or more) joins the
            // arbitrage on its own merits, not just a hard-coded two (§7b).
            for o in 0..m {
                for d in 0..m {
                    if o == d {
                        continue;
                    }
                    let spread = self.markets[d].price(c) - self.markets[o].price(c);
                    let has_surplus = self.markets[o].stock(c) > qty;
                    let has_room = self.markets[d].stock(c) + qty < self.markets[d].wall_high(c);
                    if spread > best_spread && has_surplus && has_room {
                        best = Some((c, o, d, qty));
                        best_spread = spread;
                    }
                }
            }
        }
        best
    }
}
