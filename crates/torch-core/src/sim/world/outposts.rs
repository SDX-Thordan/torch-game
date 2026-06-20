//! `outposts` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// The player's founded outposts.
    pub fn outposts(&self) -> &[Outpost] {
        &self.outposts
    }

    /// The outpost at `body`, if any (object-contextual lookup).
    pub fn outpost_at(&self, body: usize) -> Option<&Outpost> {
        self.outposts.iter().find(|o| o.body == body)
    }

    /// Whether `body` is a valid, free site to found an outpost — a real world (not the
    /// sun/gate/far-side) that you don't already hold (no outpost, shipyard, or colony there).
    pub fn can_found_outpost(&self, body: usize) -> bool {
        use crate::sim::orbit::BodyKind;
        if self.outposts.len() >= MAX_OUTPOSTS || self.outpost_at(body).is_some() {
            return false;
        }
        if self.shipyard_tier > 0 && self.shipyard_body == body {
            return false;
        }
        // Not on a frontier colony body you control.
        if self
            .colonies
            .iter()
            .enumerate()
            .any(|(i, c)| c.body == body && self.colony_controlled(i))
        {
            return false;
        }
        matches!(
            crate::sim::orbit::default_system()
                .get(body)
                .map(|b| b.kind),
            Some(
                BodyKind::Planet
                    | BodyKind::GasGiant
                    | BodyKind::DwarfPlanet
                    | BodyKind::Moon
                    | BodyKind::Asteroid
            )
        )
    }

    /// **Found an outpost** at `body` (the body-built station the early-game empire grows on).
    /// Cheaper than a shipyard; starts at level 1 and develops from there. A spine op (§0).
    pub fn found_outpost(&mut self, body: usize) -> Result<(), OutpostError> {
        if self.outposts.len() >= MAX_OUTPOSTS {
            return Err(OutpostError::Full);
        }
        if !self.can_found_outpost(body) {
            return Err(OutpostError::BadSite);
        }
        if self.corp.credits() < OUTPOST_FOUND_COST {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(OUTPOST_FOUND_COST);
        // A slow build (~180 days): the outpost is laid down now but inert until `ready_tick`.
        self.outposts.push(Outpost {
            body,
            level: 1,
            ready_tick: self.tick + OUTPOST_BUILD_TICKS,
            facilities: 0,
            rank: RANK_OUTPOST,
            population: OUTPOST_POP_BASE,
            stored: 0,
            collector: false,
        });
        self.complete_op();
        Ok(())
    }

    /// Whether the outpost at `body` has facility `kind` (a `FAC_*` bit) built.
    pub fn outpost_has_facility(&self, body: usize, kind: u8) -> bool {
        self.outpost_at(body)
            .is_some_and(|o| o.facilities & kind != 0)
    }

    /// The local-storage capacity of `o` — deepened by a Storage facility, scaled by level (§10).
    pub(crate) fn outpost_store_cap(o: &Outpost) -> i64 {
        let per = if o.facilities & FAC_STORAGE != 0 {
            STORE_CAP_WITH_STORAGE
        } else {
            STORE_CAP_BASE
        };
        per * o.level
    }

    /// The outpost-at-`body`'s local stored stock + its cap (per-asset inventory readout).
    pub fn outpost_stored(&self, body: usize) -> (i64, i64) {
        match self.outpost_at(body) {
            Some(o) => (o.stored, Self::outpost_store_cap(o)),
            None => (0, 0),
        }
    }

    // ---- collector haulers (§10): freighters draining outpost stores -----------------

    /// How many haulers are tied up collecting from outpost stores (off the trade-route pool).
    pub fn collectors_assigned(&self) -> i64 {
        self.outposts.iter().filter(|o| o.collector).count() as i64
    }

    /// Whether the outpost at `body` has a collector hauler assigned.
    pub fn outpost_has_collector(&self, body: usize) -> bool {
        self.outpost_at(body).is_some_and(|o| o.collector)
    }

    /// Whether a hauler is free to collect from the outpost at `body` (an operational outpost
    /// here, not already collecting, + an unassigned hauler in the pool).
    pub fn can_assign_collector(&self, body: usize) -> bool {
        self.outpost_at(body)
            .is_some_and(|o| o.is_ready(self.tick) && !o.collector)
            && self.collectors_assigned() < self.corp.freighters()
    }

    /// Dedicate a hauler from the pool to ferry the outpost-at-`body`'s store to the warehouse
    /// (§10) — the freighter alternative to a Hangar. Returns whether one was assigned.
    pub fn assign_collector(&mut self, body: usize) -> bool {
        if !self.can_assign_collector(body) {
            return false;
        }
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.collector = true;
            true
        } else {
            false
        }
    }

    /// Recall the collector hauler from the outpost at `body` back to the trade pool.
    pub fn recall_collector(&mut self, body: usize) -> bool {
        if let Some(o) = self
            .outposts
            .iter_mut()
            .find(|o| o.body == body && o.collector)
        {
            o.collector = false;
            true
        } else {
            false
        }
    }

    /// **Build a facility** at the outpost on `body` (a `FAC_*` bit). The outpost must be
    /// operational (not mid-build) and not already have it; it's a ~120-day build, during which
    /// the outpost is under construction again. Without a Mine an outpost produces no raw goods.
    pub fn build_facility(&mut self, body: usize, kind: u8) -> Result<(), OutpostError> {
        let o = self.outpost_at(body).ok_or(OutpostError::NoneThere)?;
        if !o.is_ready(self.tick) || o.facilities & kind != 0 {
            return Err(OutpostError::BadSite);
        }
        if self.corp.credits() < OUTPOST_FACILITY_COST {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(OUTPOST_FACILITY_COST);
        let tick = self.tick;
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.facilities |= kind;
            o.ready_tick = tick + OUTPOST_FACILITY_TICKS;
        }
        self.complete_op();
        Ok(())
    }

    /// Whether the player already holds a Capital (only one is allowed — the late-game seat).
    pub fn has_capital(&self) -> bool {
        self.outposts.iter().any(|o| o.rank >= RANK_CAPITAL)
    }

    /// Whether the settlement at `body` is ready to **promote to the next rank** (outpost →
    /// colony → hub → capital): operational, below Capital, maxed level, all facilities built,
    /// and enough population. Only **one** Capital is allowed.
    pub fn can_promote_outpost(&self, body: usize) -> bool {
        let capital_held = self.has_capital();
        self.outpost_at(body).is_some_and(|o| {
            o.is_ready(self.tick)
                && o.rank < RANK_CAPITAL
                && o.level >= MAX_OUTPOST_LEVEL
                && o.facilities & FAC_ALL == FAC_ALL
                && o.population >= promote_pop_threshold(o.rank)
                && (o.rank != RANK_HUB || !capital_held) // only one Capital
        })
    }

    /// **Promote** a settlement to its next rank (the headline progression) — a major ~1-year
    /// build that multiplies its yield. Cost escalates with the target rank.
    pub fn promote_outpost(&mut self, body: usize) -> Result<(), OutpostError> {
        if !self.can_promote_outpost(body) {
            return Err(OutpostError::BadSite);
        }
        let next = self.outpost_at(body).map(|o| o.rank + 1).unwrap_or(1);
        let cost = OUTPOST_PROMOTE_COST * next as i64;
        if self.corp.credits() < cost {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(cost);
        let tick = self.tick;
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.rank = next;
            o.ready_tick = tick + OUTPOST_PROMOTE_TICKS;
        }
        self.complete_op();
        Ok(())
    }

    /// Build progress for the outpost at `body` as `(days_remaining, total_days)`, or `None`
    /// if there's no outpost there or it's already operational.
    pub fn outpost_build_remaining(&self, body: usize) -> Option<u64> {
        let o = self.outpost_at(body)?;
        if o.is_ready(self.tick) {
            None
        } else {
            Some((o.ready_tick - self.tick) / 6) // 6 ticks = 1 day
        }
    }

    /// The credit cost to develop the outpost at `body` one level (escalates with level), or
    /// `None` if there's none there, it's maxed, or a build is still in progress.
    pub fn outpost_develop_cost(&self, body: usize) -> Option<i64> {
        let o = self.outpost_at(body)?;
        if o.level >= MAX_OUTPOST_LEVEL || !o.is_ready(self.tick) {
            return None;
        }
        Some(OUTPOST_DEVELOP_BASE * o.level)
    }

    /// **Develop** the outpost at `body` a level — raising its tribute (and, like colonies,
    /// drawing no coalition alarm: improving your own is the safe growth). A spine op (§0).
    pub fn develop_outpost(&mut self, body: usize) -> Result<(), OutpostError> {
        let cost = match self.outpost_develop_cost(body) {
            Some(c) => c,
            None if self.outpost_at(body).is_none() => return Err(OutpostError::NoneThere),
            None => return Err(OutpostError::Maxed),
        };
        if self.corp.credits() < cost {
            return Err(OutpostError::CantAfford);
        }
        self.corp.debit(cost);
        // Developing is also a build (~120 days): raise the level now but re-arm the timer so
        // the new capacity only comes online when construction finishes.
        let tick = self.tick;
        if let Some(o) = self.outposts.iter_mut().find(|o| o.body == body) {
            o.level += 1;
            o.ready_tick = tick + OUTPOST_DEVELOP_TICKS;
        }
        self.complete_op();
        Ok(())
    }

    /// Each **operational** outpost pays a per-level tribute into the treasury (one still
    /// under construction pays nothing). No-op without outposts (byte-identical).
    pub(crate) fn run_outposts(&mut self) {
        if self.outposts.is_empty() {
            return;
        }
        let tick = self.tick;
        // Announce any outpost whose construction completes exactly this tick (the "you'll be
        // told when it's ready" payoff for the slow build).
        let completed: Vec<usize> = self
            .outposts
            .iter()
            .filter(|o| o.ready_tick != 0 && o.ready_tick == tick)
            .map(|o| o.body)
            .collect();
        for body in completed {
            let name = crate::sim::orbit::default_system()
                .get(body)
                .map(|b| b.name)
                .unwrap_or("the site");
            self.feed.announce(
                "Foundry",
                format!("Outpost at {name} is now operational — it joins your industrial base."),
                tick,
            );
        }
        // A promoted colony triples its yield (tribute + production) over a bare outpost.
        let rank_mult = |o: &Outpost| rank_yield_mult(o.rank);
        let tribute: i64 = self
            .outposts
            .iter()
            .filter(|o| o.is_ready(tick))
            .map(|o| o.level * OUTPOST_TRIBUTE_PER_LEVEL * rank_mult(o))
            .sum();
        self.corp.credit(tribute);
        // Per-asset inventory (§10): a Mine-equipped operational outpost extracts the body's raw
        // each tick into its **local store** (capped by its Storage facility) — not the global
        // warehouse. A **Hangar** then ships the local stock out to your warehouse; without one
        // the goods pile up on-site (you'll later send a freighter). Without a Mine: nothing.
        let count = self.outposts.len();
        for i in 0..count {
            let o = &self.outposts[i];
            if !o.is_ready(tick) {
                continue;
            }
            let has_mine = o.facilities & FAC_MINE != 0;
            let has_hangar = o.facilities & FAC_HANGAR != 0;
            let cap = Self::outpost_store_cap(o);
            let mult = rank_mult(o);
            let level = o.level;
            if has_mine {
                let produced = level * OUTPOST_MINE_OUTPUT * mult;
                self.outposts[i].stored = (self.outposts[i].stored + produced).min(cap);
            }
            // The store ships out to the warehouse via a built **Hangar** (level-scaled) and/or a
            // dedicated **collector hauler** (§10) — the freighter alternative to a Hangar.
            let has_collector = self.outposts[i].collector;
            if (has_hangar || has_collector) && self.outposts[i].stored > 0 {
                let mut rate = 0;
                if has_hangar {
                    rate += level * HANGAR_SHIP_PER_TICK;
                }
                if has_collector {
                    rate += COLLECTOR_SHIP_PER_TICK;
                }
                let shipped = rate.min(self.outposts[i].stored);
                self.outposts[i].stored -= shipped;
                let commodity = self.body_mineral(self.outposts[i].body);
                self.corp.store(commodity, shipped);
            }
        }
        // Population: each operational outpost draws settlers while you can **supply it with
        // Ice** from your stores (capped by its level); without Ice it stagnates. The supply
        // loop that gates promotion — you must feed people before an outpost becomes a colony.
        let count = self.outposts.len();
        for i in 0..count {
            if !self.outposts[i].is_ready(tick) {
                continue;
            }
            // Population cap grows with both level and rank, so a colony can grow the people a
            // hub needs, and a hub the people a capital needs.
            let cap =
                self.outposts[i].level * POP_CAP_PER_LEVEL * (self.outposts[i].rank as i64 + 1);
            let fed = self.corp.cargo(ICE_COMMODITY) >= ICE_FEED_PER_TICK;
            let pop = self.outposts[i].population;
            if fed && pop < cap {
                self.corp.unstore(ICE_COMMODITY, ICE_FEED_PER_TICK);
                self.outposts[i].population = (pop + POP_GROWTH).min(cap);
            } else if !fed {
                self.outposts[i].population = (pop - POP_DECAY).max(OUTPOST_POP_BASE);
            }
        }
    }

    // ---- the great-power war (Earth/Mars conflict that haunts the early game) ----
}
