//! `fleet` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// Ticks between Earth/Mars war flashpoints — frequent early, dwindling as you climb
    /// the tiers (the inners' grip on Sol wanes once you're a power, §0).
    pub(crate) fn war_flashpoint_interval(&self) -> u64 {
        match self.campaign.tier() {
            Tier::Station | Tier::Region => 460,
            Tier::Sol => 680,
            _ => 1000, // Gate/Beyond — Earth & Mars influence dwindles
        }
    }

    /// The two inners by your standing: the one you favour, and their rival.
    pub(crate) fn favored_inner(&self) -> (Faction, Faction) {
        if self.relations.standing(Faction::Earth) >= self.relations.standing(Faction::Mars) {
            (Faction::Earth, Faction::Mars)
        } else {
            (Faction::Mars, Faction::Earth)
        }
    }

    /// The ambient Earth/Mars conflict (the early-game "weather"): on a flashpoint, if the
    /// lanes you work are exposed, catch the player in the crossfire as a dilemma.
    pub(crate) fn run_war(&mut self) {
        if self.tick < self.next_war_flashpoint {
            return;
        }
        self.next_war_flashpoint = self.tick + self.war_flashpoint_interval();
        if self.haulers.is_empty()
            || self
                .decisions
                .iter()
                .any(|d| d.kind == DecisionKind::WarCollateral)
        {
            return; // no exposure, or a war dilemma is already open
        }
        let tick = self.tick;
        self.push_decision(DecisionKind::WarCollateral, 0, 0, 0, WAR_STAKE, tick);
        self.feed.announce(
            "The Inners",
            "An Earth–Mars flashpoint flares across your lanes — mind your cargo.".to_string(),
            tick,
        );
    }

    // ---- contested colonies (the great powers fight over the major hubs) ----

    /// The ambient great-power contest over the major frontier hubs (early-game
    /// "weather"): on a flare, Earth and Mars shove over one colony, shifting its
    /// influence balance — voiced via the feed (the Ganymede conflict as the model).
    /// Pure integer + rng-free + touches only contest numbers, so it's byte-identical.
    pub(crate) fn run_contest(&mut self) {
        if self.contested.is_empty() || self.tick < self.next_contest_flare {
            return;
        }
        self.next_contest_flare = self.tick + contest::FLARE_INTERVAL;
        let tick = self.tick;
        let step = tick / contest::FLARE_INTERVAL;
        // Round-robin which colony flares; alternate which inner presses its claim.
        let which = (step as usize) % self.contested.len();
        let earth = Faction::Earth.index();
        let mars = Faction::Mars.index();
        let (gain, lose) = if step.is_multiple_of(2) {
            (mars, earth)
        } else {
            (earth, mars)
        };
        let shift = contest::FLARE_SHIFT.min(self.contested[which].influence[lose]);
        self.contested[which].influence[lose] -= shift;
        self.contested[which].influence[gain] += shift;
        let colony = self.contested[which].colony;
        let colony_name = self.colonies[colony].name;
        let winner = Faction::ALL[gain].name();
        self.feed.announce(
            "The Frontier",
            format!("Earth and Mars clash over {colony_name} — {winner} presses its claim."),
            tick,
        );
    }

    /// Deposit each miner's haul into the warehouse. A miner working an outpost's body hauls
    /// to that on-site station for **+50%** output. No-op without miners (byte-identical: with
    /// no outposts the bonus is 0, so output == the original `MINER_OUTPUT_PER_TICK`).
    pub(crate) fn run_miners(&mut self) {
        if self.miners.is_empty() {
            return;
        }
        let deposits: Vec<(usize, i64)> = self
            .miners
            .iter()
            .map(|m| {
                let has_ready_outpost = self
                    .outpost_at(m.body)
                    .is_some_and(|o| o.is_ready(self.tick));
                // Yield scales with the rig's class (Prospector ×1 keeps the original rate);
                // a co-located ready outpost adds its hauling bonus, and a hauler in the same
                // convoy adds the Phase 4 synergy (the hauler ferries ore so the rig never stops).
                let base = MINER_OUTPUT_PER_TICK * m.class.yield_mult();
                let mut bonus_bp = 0;
                if has_ready_outpost {
                    bonus_bp += OUTPOST_MINER_BONUS_BP;
                }
                if m.convoy.is_some_and(|id| self.corp.convoy_has_hauler(id)) {
                    bonus_bp += CONVOY_SYNERGY_BP;
                }
                (m.commodity, base + base * bonus_bp / 10_000)
            })
            .collect();
        for (c, qty) in deposits {
            self.corp.store(c, qty);
        }
    }

    /// Drain the shipyard's per-tick maintenance (expensive to keep). No-op without one.
    pub(crate) fn run_shipyard_upkeep(&mut self) {
        if self.shipyard_tier > 0 {
            let upkeep = self.shipyard_tier * SHIPYARD_UPKEEP_PER_TIER;
            let drain = upkeep.min(self.corp.credits());
            self.corp.debit(drain);
        }
    }

    /// Assemble a warship of `class` from the player's **own component stock** (§7d):
    /// consumes the Assembled-tier goods in [`ship_bom`] from the warehouse plus a
    /// small labour fee + crew — far cheaper than buying a finished hull, the payoff
    /// of building out the production chain. Fails if any part or the crew is short.
    pub fn assemble_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        self.hull_source_ok(class)?;
        let hull = self.catalog.hull(class);
        let fee = hull.dry_mass * ASSEMBLY_FEE_PER_MASS;
        if self.corp.credits() < fee {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        let bom = Self::ship_bom(class);
        if bom.iter().any(|&(c, q)| self.corp.cargo(c) < q) {
            return Err(CommissionError::MissingParts);
        }
        for &(c, q) in bom {
            self.corp.unstore(c, q);
        }
        self.corp.debit(fee);
        self.lay_down_ship(class, None);
        Ok(())
    }

    /// The Assembled-tier bill of materials to build a hull of `class` from parts
    /// (§7d): `(commodity index, quantity)`. Bigger hulls need more Machinery (10)
    /// and Drives (11); capitals also need Habitats (9) for their crew.
    pub fn ship_bom(class: ShipClass) -> &'static [(usize, i64)] {
        match class {
            ShipClass::Frigate => &[(10, 2), (11, 1)],
            ShipClass::Destroyer => &[(10, 4), (11, 2)],
            ShipClass::Cruiser => &[(10, 7), (11, 3), (9, 1)],
            ShipClass::Battleship => &[(10, 12), (11, 5), (9, 2)],
            ShipClass::QShip => &[(10, 2), (11, 1)],
            ShipClass::Freighter => &[(10, 3)],
            ShipClass::Miner => &[(10, 2)],
            ShipClass::Tanker => &[(10, 2)],
        }
    }

    /// Commission a warship of `class` to the **player's custom design** (A2): the
    /// chosen weapon counts + remass (as a percent of tankage), validated through the
    /// fitting (`FitError` → `BadFit`). Same hull price + crew draw as the reference
    /// commission; the design only changes what's bolted on (and thus the stats).
    #[allow(clippy::too_many_arguments)]
    pub fn commission_designed(
        &mut self,
        class: ShipClass,
        pdc_model: usize,
        pdc: u32,
        torp_model: usize,
        torp: u32,
        rail_model: usize,
        rail: u32,
        remass_bp: i64,
    ) -> Result<(), CommissionError> {
        self.hull_source_ok(class)?;
        let hull = self.catalog.hull(class);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        // Validate the fit now (so a bad design is rejected before any cost is paid); the
        // loadout is rebuilt from the persisted `DesignSpec` when the build completes.
        let design = DesignSpec {
            pdc_model,
            pdc,
            torp_model,
            torp,
            rail_model,
            rail,
            remass_bp: remass_bp.clamp(0, 100),
        };
        self.build_designed_loadout(class, &design)
            .map_err(|_| CommissionError::BadFit)?;
        self.corp.debit(price);
        self.lay_down_ship(class, Some(design));
        Ok(())
    }

    /// Rebuild a custom [`Loadout`] from a [`DesignSpec`] (validation at order time + stand-up
    /// at completion share this), threading the current weapon catalog + rng.
    pub(crate) fn build_designed_loadout(
        &mut self,
        class: ShipClass,
        d: &DesignSpec,
    ) -> Result<Loadout, ()> {
        let hull = self.catalog.hull(class);
        let remass = hull.remass_capacity * d.remass_bp.clamp(0, 100) / 100;
        let pdc_def = self.chosen_weapon_def(WeaponKind::Pdc, d.pdc_model);
        let torp_def = self.chosen_weapon_def(WeaponKind::Torpedo, d.torp_model);
        let rail_def = self.chosen_weapon_def(WeaponKind::Railgun, d.rail_model);
        self.catalog
            .custom_loadout_with(
                class,
                &pdc_def,
                d.pdc,
                &torp_def,
                d.torp,
                &rail_def,
                d.rail,
                remass,
                50,
                &mut self.rng,
            )
            .map_err(|_| ())
    }

    /// The [`WeaponDef`] for a player-chosen weapon model of `kind` — if the model is in
    /// service (owned), use it; otherwise fall back to the best-owned of that kind.
    pub fn chosen_weapon_def(&self, kind: WeaponKind, model_id: usize) -> WeaponDef {
        match weapons::model(model_id) {
            Some(m) if m.kind == kind && self.corp.owns_weapon(model_id) => m.to_def(),
            _ => self.best_weapon_def(kind),
        }
    }

    /// The [`WeaponDef`] of the highest-tier model of `kind` the player owns (Phase B) —
    /// crafted upgrades flow into newly built ships. Falls back to the generic catalog.
    pub fn best_weapon_def(&self, kind: WeaponKind) -> WeaponDef {
        weapons::weapon_models()
            .into_iter()
            .filter(|m| m.kind == kind && self.corp.owns_weapon(m.id))
            .max_by_key(|m| m.tier)
            .map(|m| m.to_def())
            .unwrap_or_else(|| self.catalog.weapon(kind))
    }

    /// Shared tail of commission/assemble: fit the hull with the player's **best-owned**
    /// weapons (Phase B), draw its crew, christen it (§14), dock it (§6), count the op.
    pub(crate) fn stand_up_hull(&mut self, class: ShipClass) {
        let pdc = self.best_weapon_def(WeaponKind::Pdc);
        let torp = self.best_weapon_def(WeaponKind::Torpedo);
        let rail = self.best_weapon_def(WeaponKind::Railgun);
        let loadout = self
            .catalog
            .loadout_with(class, &pdc, &torp, &rail, 50, &mut self.rng);
        self.stand_up_loadout(loadout);
    }

    /// Stand a fitted hull up into the fleet (shared by reference + custom builds). The crew
    /// was **reserved at lay-down** ([`lay_down_ship`]), so this does not draw it again.
    pub(crate) fn stand_up_loadout(&mut self, loadout: Loadout) {
        let hull_name = loadout.hull().name;
        // A christened call-sign + class, e.g. "Lodestar (Frigate)" (§14). It rolls
        // off the line docked at Ceres Yards (the shipyard) with a full tank (§6).
        let name = format!("{} ({})", ships::christen_ship(&mut self.rng), hull_name);
        let home = self.markets[0].body();
        self.corp
            .add_ship(OwnedShip::new(name, loadout, self.tick, home));
        // The op + the FirstWarship beat are counted when the hull is *ordered*
        // (`lay_down_ship`) — committing to the build is the macro decision that climbs the
        // spine (§0); this is just its delivery.
    }

    /// Build-out time for a commissioned hull, in ticks (6 = 1 day): bigger hulls take
    /// longer. A frigate is weeks; a battleship the better part of a year — so building a
    /// fleet is a paced commitment, not an instant purchase.
    pub fn commission_build_ticks(class: ShipClass) -> u64 {
        match class {
            ShipClass::Frigate | ShipClass::QShip => 180, // ~30 days (a small hull)
            ShipClass::Destroyer => 360,                  // ~60 days
            ShipClass::Cruiser => 600,                    // ~100 days
            ShipClass::Battleship => 1080,                // ~180 days (a capital)
            _ => 120,                                     // civilians (~20 days)
        }
    }

    /// Lay a hull down in the shipyard: **reserve** its crew now and queue it to stand up
    /// into the fleet once the build completes. Cost/parts are already paid by the caller.
    pub(crate) fn lay_down_ship(&mut self, class: ShipClass, design: Option<DesignSpec>) {
        let crew_required = self.catalog.hull(class).crew_required;
        self.corp.assign_crew(crew_required);
        let ready_tick = self.tick + Self::commission_build_ticks(class);
        self.pending_ships.push(PendingShip {
            class,
            ready_tick,
            design,
        });
        self.note_mission(crate::sim::missions::Trigger::FirstWarship); // §16 tutorial
        self.complete_op(); // committing to the build is progress on the climb (§0)
    }

    /// Stand up any queued hull whose build has completed (called each `step`).
    pub(crate) fn run_shipyard_builds(&mut self) {
        if self.pending_ships.is_empty() {
            return;
        }
        let tick = self.tick;
        let ready: Vec<PendingShip> = {
            let mut done = Vec::new();
            self.pending_ships.retain(|p| {
                if p.ready_tick <= tick {
                    done.push(*p);
                    false
                } else {
                    true
                }
            });
            done
        };
        for p in ready {
            match p.design {
                Some(d) => {
                    if let Ok(loadout) = self.build_designed_loadout(p.class, &d) {
                        self.stand_up_loadout(loadout);
                    } else {
                        self.stand_up_hull(p.class); // design no longer valid — fall back
                    }
                }
                None => self.stand_up_hull(p.class),
            }
        }
    }

    /// Warships currently under construction (count) — for the shell's build queue.
    pub fn pending_ship_count(&self) -> usize {
        self.pending_ships.len()
    }

    /// `(class, days-left)` for queued build `i`, soonest-ordered first.
    pub fn pending_ship(&self, i: usize) -> Option<(ShipClass, u64)> {
        self.pending_ships
            .get(i)
            .map(|p| (p.class, p.ready_tick.saturating_sub(self.tick).div_ceil(6)))
    }

    /// Test helper: stand up every queued hull at the current tick (skip the build wait),
    /// leaving the rest of the world untouched — so an acceptance test reads as if
    /// commissioning were instant (the behavior these tests assert pre-dates timed builds).
    #[cfg(test)]
    pub(crate) fn finish_pending_ships(&mut self) {
        let t = self.tick;
        for p in self.pending_ships.iter_mut() {
            p.ready_tick = t;
        }
        self.run_shipyard_builds();
    }

    /// Order warship `idx` to fly to `dest` body (§6): commit a trajectory at the
    /// live orbital distance, spend remass, and take time derived from the ship's
    /// drive and the chosen burn (economical vs. hard). Fails if the ship is busy,
    /// already there, or lacks the remass to make the burn (stranding is real).
    pub fn move_ship(&mut self, idx: usize, dest: usize, hard_burn: bool) -> Result<(), MoveError> {
        if dest >= self.bodies.len() {
            return Err(MoveError::BadDestination);
        }
        let ship = self.corp.fleet().get(idx).ok_or(MoveError::NoSuchShip)?;
        if ship.nav.in_transit(self.tick) {
            return Err(MoveError::Busy);
        }
        if ship.nav.location == dest {
            return Err(MoveError::AlreadyThere);
        }
        let here = orbit::position_of(&self.bodies, ship.nav.location, self.tick);
        let there = orbit::position_of(&self.bodies, dest, self.tick);
        let (dx, dy) = (there.0 - here.0, there.1 - here.1);
        let distance = (dx * dx + dy * dy).isqrt();
        let plan = movement::plan(&ship.loadout, distance, hard_burn);
        let nav = ship.nav; // `Nav` is `Copy`; ends the immutable borrow of `ship`
        if nav.remass < plan.remass_cost {
            return Err(MoveError::InsufficientRemass);
        }
        self.corp.fleet_mut()[idx].nav = movement::Nav {
            location: nav.location,
            dest,
            depart_tick: self.tick,
            arrival_tick: self.tick + plan.travel_ticks,
            remass: nav.remass - plan.remass_cost,
            remass_max: nav.remass_max,
        };
        Ok(())
    }

    /// Refuel docked warship `idx` to a full tank (§6), buying the reaction mass at
    /// the cheapest market price for ReactorFuel. Returns whether it refuelled.
    pub fn refuel_ship(&mut self, idx: usize) -> bool {
        let nav = match self.corp.fleet().get(idx) {
            Some(s) => s.nav, // `Copy` — ends the borrow of `self.corp`
            None => return false,
        };
        if nav.in_transit(self.tick) {
            return false;
        }
        let need = nav.remass_max - nav.remass;
        if need <= 0 {
            return false;
        }
        let unit = self
            .markets
            .iter()
            .map(|m| m.price(REMASS_COMMODITY))
            .min()
            .unwrap_or(1);
        let cost = (need * unit / REMASS_PER_FUEL).max(0);
        if !self.corp.debit(cost) {
            return false;
        }
        self.corp.fleet_mut()[idx].nav.remass = nav.remass_max;
        true
    }

    /// Advance in-flight ships: any whose trajectory has completed docks at its
    /// destination (§6). Called each tick.
    pub(crate) fn run_fleet_nav(&mut self) {
        let tick = self.tick;
        for s in self.corp.fleet_mut() {
            if s.nav.dest != s.nav.location && tick >= s.nav.arrival_tick {
                s.nav.location = s.nav.dest;
            }
        }
    }

    /// Absolute position of owned ship `idx` (§6/§21): its dock body when docked,
    /// or interpolated along its trajectory when in transit.
    pub fn ship_position(&self, idx: usize) -> (i64, i64) {
        let Some(s) = self.corp.fleet().get(idx) else {
            return (0, 0);
        };
        let from = orbit::position_of(&self.bodies, s.nav.location, self.tick);
        if !s.nav.in_transit(self.tick) {
            return from;
        }
        let to = orbit::position_of(&self.bodies, s.nav.dest, self.tick);
        let span = (s.nav.arrival_tick - s.nav.depart_tick).max(1) as i64;
        let t = (self.tick - s.nav.depart_tick).min(span as u64) as i64;
        // Flip-and-burn distance fraction (accelerate to mid-flight, brake to the dock),
        // matching the NPC hauler profile in `traffic::Hauler::position`.
        let den = span * span;
        let num = if 2 * t <= span {
            2 * t * t
        } else {
            den - 2 * (span - t) * (span - t)
        };
        (
            from.0 + (to.0 - from.0) * num / den,
            from.1 + (to.1 - from.1) * num / den,
        )
    }

    /// Commission a civilian freighter to run trade-route standing orders (§4).
    pub fn commission_freighter(&mut self) -> Result<(), CommissionError> {
        self.commission_hauler(crate::sim::corp::HaulerClass::Light)
    }

    /// Commission a hauler of `class` — the tiered version. The Light tier is byte-identical
    /// to the old `commission_freighter` (same price + crew); the Heavy/Bulk are pricier,
    /// crew-heavier ships that carry far more per trip (fatter routes need a real hull).
    pub fn commission_hauler(
        &mut self,
        class: crate::sim::corp::HaulerClass,
    ) -> Result<(), CommissionError> {
        // The OPA Q-Runner is a Belt-built hull — it needs OPA standing, like the corvette.
        if class.needs_opa_standing() && self.relations.standing(Faction::Belt) < CORVETTE_STANDING
        {
            return Err(CommissionError::NeedShipyard);
        }
        if self.corp.credits() < class.cost() {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < class.crew() {
            return Err(CommissionError::NotEnoughCrew);
        }
        self.corp.debit(class.cost());
        self.corp.assign_crew(class.crew());
        self.corp.add_hauler(class, self.tick);
        self.complete_op(); // standing up logistics is progress on the climb (§0)
        Ok(())
    }

    /// Whether the player may commission hauler `class` right now (OPA standing for the
    /// Q-Runner) — lets the shell gate the option before the player tries.
    pub fn can_commission_hauler(&self, class: crate::sim::corp::HaulerClass) -> bool {
        !class.needs_opa_standing() || self.relations.standing(Faction::Belt) >= CORVETTE_STANDING
    }

    /// Arm hauler `i` with `pdc` point-defense cannons (and, on the OPA Q-Runner, `torpedo`
    /// Ramshackle tubes) for self-protection against piracy — clamped to the hull's mounts.
    /// Charges the weapon cost for any *added* mounts (disarming is free). Armed haulers add to
    /// the empire's effective escort screen (`effective_escorts`).
    pub fn arm_hauler(&mut self, i: usize, pdc: u8, torpedo: u8) -> Result<(), CommissionError> {
        let Some(h) = self.corp.haulers().get(i) else {
            return Err(CommissionError::NeedShipyard);
        };
        let class = h.class;
        let new_pdc = pdc.min(class.pdc_mounts());
        let new_torp = torpedo.min(class.torpedo_mounts());
        let added_pdc = new_pdc.saturating_sub(h.pdc) as i64;
        let added_torp = new_torp.saturating_sub(h.torpedo) as i64;
        let cost = added_pdc * HAULER_PDC_COST + added_torp * HAULER_TORPEDO_COST;
        if self.corp.credits() < cost {
            return Err(CommissionError::CantAfford);
        }
        self.corp.debit(cost);
        if let Some(h) = self.corp.hauler_mut(i) {
            h.pdc = new_pdc;
            h.torpedo = new_torp;
        }
        Ok(())
    }

    /// The player's table of standing trade routes (§4).
    pub fn routes(&self) -> &[TradeRoute] {
        &self.routes
    }

    /// The first standing route, if any — a convenience for the single-route
    /// status view in the shell (§4).
    pub fn route(&self) -> Option<TradeRoute> {
        self.routes.first().copied()
    }

    /// Indices into [`routes`](Self::routes) whose freighter is **in flight** right
    /// now (§6 positional logistics) — one flying freighter per in-transit route.
    pub fn flying_routes(&self) -> Vec<usize> {
        self.routes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.in_transit)
            .map(|(i, _)| i)
            .collect()
    }

    /// Live position of route `i`'s freighter, interpolated along its orbital path
    /// (origin → dest market body) by trip progress — the same lane model the NPC
    /// haulers use, so the logistics wing is a real positional asset on the map (§6).
    pub fn route_freighter_pos(&self, i: usize) -> (i64, i64) {
        match self.routes.get(i) {
            Some(rt) if rt.in_transit => {
                let o = orbit::position_of(&self.bodies, self.markets[rt.origin].body(), self.tick);
                let d = orbit::position_of(&self.bodies, self.markets[rt.dest].body(), self.tick);
                let span = rt.arrival.saturating_sub(rt.departed).max(1) as i64;
                let t = (self.tick.saturating_sub(rt.departed) as i64).clamp(0, span);
                (o.0 + (d.0 - o.0) * t / span, o.1 + (d.1 - o.1) * t / span)
            }
            _ => (0, 0),
        }
    }

    /// The destination body position for route `i` (for the freighter's lane trail).
    pub fn route_dest_pos(&self, i: usize) -> (i64, i64) {
        match self.routes.get(i) {
            Some(rt) => orbit::position_of(&self.bodies, self.markets[rt.dest].body(), self.tick),
            None => (0, 0),
        }
    }

    /// The Remass a freighter burns on route `i`'s current geometry (§6) — the
    /// distance-scaled fuel load it refuels at the origin port each trip.
    pub fn route_remass_units(&self, i: usize) -> i64 {
        match self.routes.get(i) {
            Some(rt) => {
                (self.travel_ticks(rt.origin, rt.dest) / FREIGHTER_REMASS_DIVISOR).max(1) as i64
            }
            None => 0,
        }
    }

    /// Trip progress of route `i`'s freighter in basis points (0..=10000), for the
    /// FLEET view's en-route readout.
    pub fn route_progress_bp(&self, i: usize) -> i64 {
        match self.routes.get(i) {
            Some(rt) if rt.in_transit => {
                let span = rt.arrival.saturating_sub(rt.departed).max(1) as i64;
                let t = (self.tick.saturating_sub(rt.departed) as i64).clamp(0, span);
                t * 10_000 / span
            }
            _ => 0,
        }
    }

    /// Add a parameterized Trade Route standing order to the table — buy
    /// `commodity` at `origin`, sell at `dest`, `qty` per trip, only while the
    /// spread clears `min_margin` (§4). Many routes run concurrently against the
    /// shared freighter pool; exceptions go idle. Capped at the tier's route cap.
    pub fn set_trade_route(
        &mut self,
        commodity: usize,
        origin: usize,
        dest: usize,
        qty: i64,
        min_margin: i64,
    ) {
        if self.routes.len() < self.campaign.tier().route_cap() {
            self.routes
                .push(TradeRoute::new(commodity, origin, dest, qty, min_margin));
            self.note_mission(crate::sim::missions::Trigger::FirstRoute); // §16 tutorial
        }
    }

    /// Clear the whole route table.
    pub fn clear_trade_route(&mut self) {
        self.routes.clear();
    }
}
