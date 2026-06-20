//! `defence` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// The bounty paid for the last won engagement (Phase B) — 0 on a loss/none.
    pub fn last_bounty(&self) -> i64 {
        self.last_bounty
    }

    /// Start **producing** a weapon model (Phase B). You can't *buy* advanced weapons:
    /// you must hold the **schematic** (earned by reverse-engineering, never bought),
    /// then tool up a production line — it costs scrap (from combat) + credits and takes
    /// **time** (`production_ticks`, longer for higher tiers). Building a great power's
    /// design **antagonises** that power. The line finishes in `step()` → the arsenal.
    pub fn produce_weapon(&mut self, id: usize) -> Result<(), CraftError> {
        let model = weapons::model(id).ok_or(CraftError::Unknown)?;
        if self.corp.owns_weapon(id) {
            return Err(CraftError::AlreadyOwned);
        }
        if !self.corp.knows_schematic(id) {
            return Err(CraftError::NoSchematic);
        }
        if self.weapon_production.iter().any(|&(m, _)| m == id) {
            return Err(CraftError::AlreadyProducing);
        }
        if self.corp.scrap() < model.scrap_cost {
            return Err(CraftError::NotEnoughScrap);
        }
        if self.corp.credits() < model.credit_cost {
            return Err(CraftError::CantAfford);
        }
        self.corp.spend_scrap(model.scrap_cost);
        self.corp.debit(model.credit_cost);
        if let Some(f) = model.origin.antagonist() {
            self.relations.adjust(f, -(model.tier as i64) * CRAFT_ANGER);
        }
        let done = self.tick + Self::production_ticks(model.tier);
        self.weapon_production.push((id, done));
        Ok(())
    }

    /// How long tooling up a tier-`t` weapon line takes — advanced designs are slower.
    pub(crate) fn production_ticks(tier: u8) -> u64 {
        PRODUCTION_BASE_TICKS + tier as u64 * PRODUCTION_TICKS_PER_TIER
    }

    /// Tick the weapon-production lines: any that finished this tick join the arsenal
    /// (fittable on new/refitted ships) and count as an operation on the climb (§0).
    pub(crate) fn run_weapon_production(&mut self) {
        let now = self.tick;
        let mut finished = Vec::new();
        self.weapon_production.retain(|&(id, done)| {
            if done <= now {
                finished.push(id);
                false
            } else {
                true
            }
        });
        for id in finished {
            self.corp.add_weapon(id);
            if let Some(model) = weapons::model(id) {
                self.feed.announce(
                    "Foundry",
                    format!("{} line online — fit it on new builds.", model.name),
                    now,
                );
            }
            self.complete_op();
        }
    }

    /// Ticks remaining on the production line for `id` (0 = not in production).
    pub fn production_remaining(&self, id: usize) -> u64 {
        self.weapon_production
            .iter()
            .find(|&&(m, _)| m == id)
            .map(|&(_, done)| done.saturating_sub(self.tick))
            .unwrap_or(0)
    }

    /// Refit ship `idx` to **chosen** weapon models per kind (Phase B): swaps its guns,
    /// charges a yard fee, and puts the hull **in the yard** for a refit period (it can't
    /// move or fight until done). An unowned/invalid model id falls back to best-owned
    /// (so passing `usize::MAX` for all three is "refit to best"). Must be docked at the
    /// home yard and not already refitting.
    pub fn refit_ship(
        &mut self,
        idx: usize,
        pdc_model: usize,
        torp_model: usize,
        rail_model: usize,
    ) -> Result<(), RefitError> {
        let now = self.tick;
        let home = self.markets[0].body();
        let fleet = self.corp.fleet();
        let ship = fleet.get(idx).ok_or(RefitError::NoSuchShip)?;
        if ship.is_refitting(now) {
            return Err(RefitError::Busy);
        }
        if ship.nav.in_transit(now) || ship.nav.location != home {
            return Err(RefitError::NotAtYard);
        }
        let class = ship.loadout.hull().class;
        // A capital hull's refit needs your own shipyard — Tycho only handles the small
        // ones when you've no yard of your own.
        if matches!(class, ShipClass::Cruiser | ShipClass::Battleship) && self.shipyard_tier < 1 {
            return Err(RefitError::NeedShipyard);
        }
        let mass = ship.loadout.hull().dry_mass;
        let fee = mass * REFIT_FEE_PER_MASS;
        if self.corp.credits() < fee {
            return Err(RefitError::CantAfford);
        }
        let crew_quality = ship.loadout.crew().quality;
        // Rebuild the loadout with the chosen models (fall back to best-owned).
        let pdc = self.chosen_weapon_def(WeaponKind::Pdc, pdc_model);
        let torp = self.chosen_weapon_def(WeaponKind::Torpedo, torp_model);
        let rail = self.chosen_weapon_def(WeaponKind::Railgun, rail_model);
        let new_loadout =
            self.catalog
                .loadout_with(class, &pdc, &torp, &rail, crew_quality, &mut self.rng);
        self.corp.debit(fee);
        let until = now + mass as u64 * REFIT_TICKS_PER_MASS;
        if let Some(ship) = self.corp.fleet_mut().get_mut(idx) {
            ship.loadout = new_loadout;
            ship.refit_until = until;
        }
        self.complete_op();
        Ok(())
    }

    /// Learn a weapon schematic the player doesn't yet hold (earned, e.g. from
    /// reverse-engineering a derelict). Returns the learned model id, if any was new.
    pub(crate) fn grant_weapon_schematic(&mut self) -> Option<usize> {
        let unknown: Vec<usize> = weapons::weapon_models()
            .iter()
            .map(|m| m.id)
            .filter(|id| !self.corp.knows_schematic(*id))
            .collect();
        if unknown.is_empty() {
            return None;
        }
        let pick = unknown[self.rng.below(unknown.len() as u32) as usize];
        self.corp.learn_schematic(pick);
        if let Some(model) = weapons::model(pick) {
            let tick = self.tick;
            self.feed.announce(
                "R&D",
                format!(
                    "Schematic recovered: {} — you can produce it now.",
                    model.name
                ),
                tick,
            );
        }
        Some(pick)
    }

    // ---- piracy on your trade empire (EP3) ----------------------------------

    /// How many escorts (warships on station) the empire needs to screen its shipping
    /// from piracy (EP3) — scales with holdings, so a bigger empire needs a bigger
    /// navy. Zero when you hold nothing.
    pub fn escorts_needed(&self) -> usize {
        let h = self.holding_count();
        if h == 0 {
            0
        } else {
            1 + h / HOLDINGS_PER_ESCORT
        }
    }

    /// Escorts effectively screening your trade (EP3/E8): your warships on station, the ships
    /// **allied companies** lend you (diplomacy buys security), plus the screen from **armed
    /// haulers** (a fleet that defends itself). An unarmed fleet adds nothing (byte-identical).
    pub fn effective_escorts(&self) -> usize {
        self.warships_on_station()
            + self.diplomacy.ally_count()
            + (self.corp.hauler_defense() / HAULER_DEFENSE_PER_ESCORT) as usize
            + self.total_escorts_assigned() as usize
    }

    /// Whether the empire's shipping is adequately escorted (EP3/E8) — your navy plus
    /// allied support meet or exceed the need.
    pub fn empire_secure(&self) -> bool {
        self.effective_escorts() >= self.escorts_needed()
    }

    /// Standing predation on your trade (EP3): if your empire's shipping outruns its
    /// escorts, pirates skim cargo on a cadence. Countered by keeping a navy **on
    /// station** that scales with the empire — neglect it and a big empire bleeds.
    /// Gated on holding anything, draws no RNG → a fresh sim is byte-identical.
    pub(crate) fn run_empire_piracy(&mut self, now: u64) {
        if self.holding_count() == 0 || !now.is_multiple_of(PIRACY_INTERVAL) {
            return;
        }
        let needed = self.escorts_needed();
        let escorts = self.effective_escorts();
        if escorts >= needed {
            return; // well-screened (navy + allies) — the patrols hold
        }
        let shortfall = (needed - escorts) as i64;
        let loss = (shortfall * PIRACY_LOSS_PER_ESCORT_SHORT).min(self.corp.credits());
        if loss > 0 {
            self.corp.debit(loss);
        }
        self.events.push(Event::EmpireRaided { loss });
    }

    /// The harshest standing a great power holds against the player — how soured the
    /// inners are (EP4). Negative = wary/hostile.
    pub fn worst_standing(&self) -> i64 {
        [Faction::Earth, Faction::Mars, Faction::Belt]
            .iter()
            .map(|&f| self.relations.standing(f))
            .min()
            .unwrap_or(0)
    }

    /// Political enforcement on a trader you've crossed (EP4): on a cadence, a great
    /// power you've soured past the threshold inspects your shipping and fines you,
    /// scaling with how hostile they are. Countered by **repairing the relationship**
    /// (contracts lift standing; it also heals over time) — distinct from piracy
    /// (countered by a navy). Gated on holding assets + a soured power; draws no RNG.
    pub(crate) fn run_inspections(&mut self, now: u64) {
        if self.holding_count() == 0 || !now.is_multiple_of(INSPECTION_INTERVAL) {
            return;
        }
        // The most-soured great power leads the sweep.
        let worst = self.worst_standing();
        if worst > INSPECTION_THRESHOLD {
            return; // no power is angry enough to enforce
        }
        let fine = ((-worst).min(1_000) * INSPECTION_FINE_PER_STANDING).min(self.corp.credits());
        if fine > 0 {
            self.corp.debit(fine);
        }
        self.events.push(Event::Inspected { fine });
    }

    /// Whether an incursion is currently bearing on the bridgehead (§17, G4) — the
    /// shell lights the DEFEND verb while this holds.
    pub fn incursion_pending(&self) -> bool {
        self.pending_incursion.is_some()
    }

    /// The severity of the pending incursion, or 0 if none (§17, G4).
    pub fn pending_incursion_severity(&self) -> i64 {
        self.pending_incursion.map(|(s, _)| s).unwrap_or(0)
    }

    /// **Defend the bridgehead** against the pending incursion (§17, G4): rally the
    /// fleet and resolve combat against a far-side raider pack scaled by the
    /// incursion's severity. A win repels it cleanly (the foothold takes no damage)
    /// and counts as an op; a loss lets the incursion through (the bridgehead is
    /// struck for its severity). Returns the battle outcome, or `None` if there's no
    /// incursion to answer or no warships to answer with.
    pub fn defend_bridgehead(&mut self, band: Band) -> Option<BattleOutcome> {
        let (severity, _) = self.pending_incursion?;
        // The whole fleet rallies to the far side — defending the foothold is the
        // priority, wherever the ships were (§17). Need at least one warship.
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return None;
        }
        // The incursion pack scales with severity — a tougher, growing enemy (§17).
        let pack_size = ((severity / INCURSION_SEVERITY_PER_SHIP).max(2)) as usize;
        let pack: Vec<Loadout> = (0..pack_size)
            .map(|_| {
                ships::reference_loadout_quality(
                    ShipClass::Frigate,
                    INCURSION_QUALITY,
                    &mut self.rng,
                )
            })
            .collect();
        let player_doctrine = Doctrine {
            band,
            ..self.combat_doctrine
        };
        let raider_doctrine = Doctrine {
            band,
            ..Doctrine::default()
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine: player_doctrine,
            },
            &Fleet {
                ships: &pack,
                doctrine: raider_doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        let won = outcome.winner == Some(0);
        let all: Vec<usize> = (0..player_ships.len()).collect();
        self.corp.resolve_engagement_for(all, survivors, won);
        self.pending_incursion = None;
        self.feed.resolve_incursion();
        if won {
            // Repelled — the foothold is safe, the win is progress (§0), and the
            // far side has been weathered one more time (§17, G5).
            self.complete_op();
            self.incursions_survived += 1;
            self.check_endgame_won();
        } else {
            // The line broke — the incursion reaches the bridgehead.
            self.strike_bridgehead(severity);
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), pack.len()], outcome.clone()));
        Some(outcome)
    }

    /// Administratively cut the in-flight hauler with `id` (a guaranteed delete,
    /// for the binding/tests). Returns whether a hauler was actually cut. For the
    /// positioning-and-odds verb, use [`Sim::interdict_with`].
    pub fn interdict(&mut self, id: u64) -> bool {
        if let Some(i) = self.haulers.iter().position(|h| h.id == id) {
            let h = self.cut_hauler(i);
            self.ripple_reputation(&h);
            true
        } else {
            false
        }
    }

    /// Attempt to interdict hauler `id` with `interceptor` (§7b): the cut only
    /// lands if the interceptor has the legs to reach the hauler *and* wins the
    /// roll. Returns the resolved outcome.
    pub fn interdict_with(&mut self, id: u64, interceptor: Interceptor) -> Interdiction {
        let Some(i) = self.haulers.iter().position(|h| h.id == id) else {
            return Interdiction::NoSolution;
        };
        let outcome = resolve(&self.haulers[i], &interceptor, self.tick, &mut self.rng);
        if outcome == Interdiction::Interdicted {
            let h = self.cut_hauler(i);
            self.ripple_reputation(&h);
        }
        outcome
    }

    /// Send the player fleet against a raider pack at `band` and resolve the
    /// battle (§9). This is the missing trigger the gameplay-QA review flagged:
    /// `sim::combat` had no verb on `Sim`, so commissioned warships never fought.
    /// The raider pack is sized to the fleet for a real contest; losses are
    /// applied to the corp, a win counts as an operation on the climb (§0), and a
    /// `BattleResolved` event is emitted for the feed (§19) and diorama (§22).
    /// Returns the outcome, or `None` if the player has no warships to send.
    pub fn engage_raiders(&mut self, band: Band) -> Option<BattleOutcome> {
        // Raiders muster on the inner lanes at the home core (§6/§13): only
        // warships **on station** there can answer — a fleet flown off to the outer
        // system can't defend the core until it burns home. This is what makes the
        // delta-v movement layer consequential (Pillar #2).
        let muster = self.markets[0].body();
        let on_station: Vec<usize> = self
            .corp
            .fleet()
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !s.nav.in_transit(self.tick)
                    && s.nav.location == muster
                    && !s.is_refitting(self.tick)
            })
            .map(|(i, _)| i)
            .collect();
        if on_station.is_empty() {
            return None;
        }
        let player_ships: Vec<Loadout> = on_station
            .iter()
            .map(|&i| self.corp.fleet()[i].loadout.clone())
            .collect();
        // A matched-count pack of raider frigates at a matched crew quality — a
        // genuine coin-flip, so committing the fleet is a real risk (§13/§9).
        let pack: Vec<Loadout> = (0..player_ships.len())
            .map(|_| {
                ships::reference_loadout_quality(ShipClass::Frigate, RAIDER_QUALITY, &mut self.rng)
            })
            .collect();
        // The player fleet fights under the player's doctrine (target + retreat),
        // at the band they chose; raiders press the attack to the death (§9).
        let player_doctrine = Doctrine {
            band,
            ..self.combat_doctrine
        };
        let raider_doctrine = Doctrine {
            band,
            ..Doctrine::default()
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine: player_doctrine,
            },
            &Fleet {
                ships: &pack,
                doctrine: raider_doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        let won = outcome.winner == Some(0);
        // Only the on-station ships were at risk; veterans pull through (§11/§13).
        self.corp.resolve_engagement_for(on_station, survivors, won);
        self.last_bounty = if won {
            // Phase B: holding the field *pays* (bounty per raider hull) and protects
            // the lanes (calms piracy) — so a navy is a viable economic strategy, not
            // pure attrition. Combat is crew-capped, so this isn't a faucet.
            let bounty = pack.len() as i64 * BOUNTY_PER_RAIDER;
            self.corp.credit(bounty);
            // Scrap recovered from the wrecked raiders — the crafting input (Phase B).
            self.corp.add_scrap(pack.len() as i64 * SCRAP_PER_RAIDER);
            self.pressure
                .relieve(PressureKind::Piracy, COMBAT_PIRACY_RELIEF);
            self.complete_op(); // holding the field is progress on the climb (§0)
            bounty
        } else {
            0
        };
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), pack.len()], outcome.clone()));
        Some(outcome)
    }

    /// The most recent resolved engagement, for the diorama (§22): the band, the
    /// starting `[player, raider]` counts, and the full BattleLog.
    pub fn last_battle(&self) -> Option<&(Band, [usize; 2], BattleOutcome)> {
        self.last_battle.as_ref()
    }

    /// Warships currently **on station** at the home core, ready to answer a raider
    /// muster (§6): docked at `markets[0]`'s body, not in transit. The shell uses
    /// this to tell "no fleet" apart from "fleet is off defending elsewhere."
    pub fn warships_on_station(&self) -> usize {
        let muster = self.markets[0].body();
        self.corp
            .fleet()
            .iter()
            .filter(|s| !s.nav.in_transit(self.tick) && s.nav.location == muster)
            .count()
    }

    /// Set the player's target-priority doctrine (§9).
    pub fn set_combat_target(&mut self, target: TargetPriority) {
        self.combat_doctrine.target = target;
    }

    /// Set the player's retreat threshold in basis points (§9): break off below
    /// this fraction of the starting fleet. `0` = fight to the death.
    pub fn set_combat_retreat(&mut self, bp: i64) {
        self.combat_doctrine.retreat_bp = bp.clamp(0, 10_000);
    }

    /// Fire railguns hot or disciplined (§9 heat): aggressive fire boosts railgun
    /// output but builds heat that periodically forces a vent.
    pub fn set_combat_aggressive(&mut self, on: bool) {
        self.combat_doctrine.aggressive_fire = on;
    }

    /// The player's current tactical doctrine (§9).
    pub fn combat_doctrine(&self) -> Doctrine {
        self.combat_doctrine
    }

    /// A *player* cut sours relations with the hauler's owner faction (§7b/§10)
    /// and counts as an operation on the climb (§0); pirate raids do neither.
    pub(crate) fn ripple_reputation(&mut self, h: &Hauler) {
        let faction = self.markets[h.origin].faction();
        self.relations.on_player_interdict(faction);
        self.note_mission(crate::sim::missions::Trigger::FirstCut); // §16 tutorial
        self.complete_op();
    }

    /// Record a completed player **operation** — the unit of progress on the §0
    /// climb. Interdiction was the *only* verb that called this, so the retention
    /// spine ignored the whole build/trade/route side of the influence model
    /// (the gameplay-QA review's #1 finding). Now every substantive player act —
    /// a cut, a commissioned ship/freighter, a founded station, a completed
    /// route delivery — advances the campaign and earns the CEO/research
    /// progress operations grant (§10, earned through play). Emits the ascent
    /// fanfare on a tier crossing (§0.3).
    pub(crate) fn complete_op(&mut self) {
        self.progression.ceo.gain_xp(OP_XP);
        self.progression.research.add_points(OP_RESEARCH_POINTS);
        if let Some(tier) = self.campaign.record_op() {
            self.events.push(Event::TierAscended { tier });
            // The climb teaches the spine and advances the authored thread (§0.1):
            // each ascent voices the next gate-mystery beat.
            self.note_mission(crate::sim::missions::Trigger::FirstAscent);
            self.reveal_gate_beat();
        }
    }

    /// Voice a completed opening mission (§16) through the feed.
    pub(crate) fn note_mission(&mut self, trigger: crate::sim::missions::Trigger) {
        if let Some(title) = self.missions.note(trigger) {
            let tick = self.tick;
            self.feed
                .announce("The Board", format!("Objective complete — {title}."), tick);
        }
    }

    /// Advance the gate-mystery beat counter (§0.1) — but **no longer voice it**. The
    /// placeholder gate lore is removed from the player's view until the proper mid/late-game
    /// arc lands (`docs/MID_LATE_GAME_STORY.md`); the `Missions::reveal_gate` machinery +
    /// `GATE_LORE` stay live (counter still advances, save field still meaningful) so that
    /// arc can re-wire the feed/UI. Re-enable by re-announcing the returned beat here.
    pub(crate) fn reveal_gate_beat(&mut self) {
        let _ = self.missions.reveal_gate();
    }

    /// **Transit the open ring-gate** (§0.1/§17) — the climactic, deliberate payoff
    /// of the whole climb: cross from the Gate into the `Beyond` endgame. Only
    /// possible standing at the open gate. On transit it tells the rest of the
    /// mystery, voices the gate's *answer*, and counts as an operation. Returns
    /// whether the transit happened.
    pub fn transit_gate(&mut self) -> bool {
        if self.campaign.transit().is_none() {
            return false;
        }
        let tick = self.tick;
        // Tell whatever of the mystery is still untold, then the answer.
        while self.missions.reveal_gate().is_some() {}
        self.events.push(Event::GateTransited);
        self.feed.announce(
            "The Gate",
            crate::sim::missions::GATE_ANSWER.to_string(),
            tick,
        );
        // The far side now knows your face (§17, G4): light the incursion clock.
        self.endgame_since = Some(tick);
        self.pressure.begin_endgame(tick);
        // The transit is itself the supreme operation on the climb (§0).
        self.progression.ceo.gain_xp(OP_XP);
        true
    }

    /// Whether the player can transit the gate right now (standing at the open
    /// ring, not yet through) — drives the shell's transit verb.
    pub fn can_transit_gate(&self) -> bool {
        self.campaign.tier() == Tier::Gate
    }

    /// The player's far-side bridgehead (§17 endgame, G3) — unfounded until transit.
    pub fn bridgehead(&self) -> &Bridgehead {
        &self.bridgehead
    }

    /// **Found the far-side bridgehead** (§17, G3) — plant the first foothold beyond
    /// the ring. Only possible in the `Beyond` (post-transit), once, for a credit
    /// outlay. Founding is itself a spine op (it advances within the endgame).
    pub fn found_bridgehead(&mut self) -> Result<(), BridgeheadError> {
        if !self.campaign.transited() {
            return Err(BridgeheadError::NotBeyond);
        }
        if self.bridgehead.is_founded() {
            return Err(BridgeheadError::AlreadyFounded);
        }
        if self.corp.credits() < BRIDGEHEAD_FOUND_COST {
            return Err(BridgeheadError::CantAfford);
        }
        self.corp.debit(BRIDGEHEAD_FOUND_COST);
        self.bridgehead.found();
        self.events.push(Event::BridgeheadFounded);
        self.complete_op(); // securing the far side is progress on the climb (§0)
        Ok(())
    }

    /// Cost to upgrade the bridgehead from its current level (§17, G3).
    pub(crate) fn bridgehead_upgrade_cost(&self) -> i64 {
        BRIDGEHEAD_UPGRADE_BASE_COST * self.bridgehead.level().max(1) as i64
    }

    /// **Upgrade the far-side bridgehead** (§17, G3) — reinforce the foothold a level,
    /// raising the integrity it can weather under incursion (G4). Requires a standing
    /// bridgehead and the (level-scaled) credits. A spine op.
    pub fn upgrade_bridgehead(&mut self) -> Result<(), BridgeheadError> {
        if !self.bridgehead.is_founded() {
            return Err(BridgeheadError::NotFounded);
        }
        let cost = self.bridgehead_upgrade_cost();
        if self.corp.credits() < cost {
            return Err(BridgeheadError::CantAfford);
        }
        self.corp.debit(cost);
        self.bridgehead.upgrade();
        self.events.push(Event::BridgeheadUpgraded {
            level: self.bridgehead.level(),
        });
        self.complete_op();
        // Reaching the target level may clinch the endgame (§17, G5).
        self.check_endgame_won();
        Ok(())
    }
}
