//! `tick` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// Advance exactly one fixed sim tick (§28) and return the events produced.
    /// The returned slice is valid until the next call to `step`.
    pub fn step(&mut self) -> &[Event] {
        // Drop only the events the previous `step` already surfaced, retaining
        // any a player verb pushed since (so player-caused events aren't lost to
        // a blanket clear — the §0.3 fanfare and §0.4 shortage fire for the
        // player too, not just for pirate/automation cuts).
        self.events.drain(0..self.returned);
        self.tick += 1;
        self.step_markets();
        self.run_subsystems();
        self.sight_salvage();
        self.emit_ambient_chatter();
        self.charge_upkeep();
        self.recover_reputation();
        self.events.push(Event::Tick { tick: self.tick });
        self.ingest_tick_events();
        self.pressure.decay();
        self.surface_dilemmas();
        self.returned = self.events.len();
        &self.events
    }

    /// Step every market. Inner markets advance on the shared rng exactly as before
    /// (byte-identical); far-side markets use their own `far_rng` so they never
    /// perturb the inner economy.
    pub(crate) fn step_markets(&mut self) {
        let split = self.far_market_start;
        for m in self.markets[..split].iter_mut() {
            m.step(&mut self.rng);
        }
        for m in self.markets[split..].iter_mut() {
            m.step(&mut self.far_rng);
        }
    }

    /// Run the per-tick world subsystems in their fixed, deterministic order.
    pub(crate) fn run_subsystems(&mut self) {
        self.deliver_arrivals();
        self.spawn_traffic();
        self.run_pressure();
        self.run_automation();
        self.run_logistics();
        self.run_industry();
        self.run_fleet_nav();
        self.run_contracts();
        self.run_weapon_production();
        self.run_shipyard_upkeep();
        self.run_shipyard_builds();
        self.run_miners();
        self.run_outposts();
        self.run_war();
        self.run_contest();
        self.run_holdings();
        self.run_coalition(self.tick);
        self.run_empire_piracy(self.tick);
        self.run_inspections(self.tick);
    }

    /// Discovery (§15): the field may turn up a derelict to strip. Its own RNG keeps
    /// the economy bit-identical whether or not anyone salvages.
    pub(crate) fn sight_salvage(&mut self) {
        if let Some(id) = self.salvage.maybe_sight(self.tick) {
            self.events.push(Event::WreckSighted { id });
        }
    }

    /// Occasional system colour (§19 texture) — its own RNG, no mechanical effect, so
    /// the economy stays bit-identical.
    pub(crate) fn emit_ambient_chatter(&mut self) {
        if let Some((voice, msg)) = self.ambient.maybe_chatter(self.tick) {
            self.feed.chatter(voice, msg.to_string(), self.tick);
        }
    }

    /// Standings drift back toward neutral on a slow cadence (§10 recovery).
    pub(crate) fn recover_reputation(&mut self) {
        if self.tick.is_multiple_of(REP_RECOVERY_INTERVAL) {
            self.relations.decay_toward_neutral(REP_RECOVERY_STEP);
        }
    }

    /// Feed every event surfacing this tick (the carried-over player events plus this
    /// tick's own) to the alert feed (§19/§29) and the pressure layer.
    pub(crate) fn ingest_tick_events(&mut self) {
        let tick = self.tick;
        for e in &self.events {
            self.feed.ingest(e, tick);
            self.pressure.note_event(e, tick);
        }
    }

    /// Phase A: turn this tick's fresh act-now exceptions into player dilemmas (menus
    /// of trade-off options) and drop any that timed out.
    pub(crate) fn surface_dilemmas(&mut self) {
        let now = self.tick;
        let mut shortages: Vec<(usize, usize)> = Vec::new();
        let mut wrecks: Vec<u64> = Vec::new();
        let mut raid = false;
        for e in &self.events {
            match e {
                Event::Scarcity { market, commodity } => shortages.push((*market, *commodity)),
                Event::WreckSighted { id } => wrecks.push(*id),
                Event::ThreatForecast {
                    kind: PressureKind::Piracy,
                    ..
                } => raid = true,
                _ => {}
            }
        }
        for (m, c) in shortages {
            self.push_decision(DecisionKind::Shortage, m, c, 0, 0, now);
        }
        for id in wrecks {
            self.push_decision(DecisionKind::Wreck, 0, 0, id, 0, now);
        }
        if raid {
            let piracy = self.pressure.level(PressureKind::Piracy) as i64;
            let mag = RAID_MAG_BASE + piracy * RAID_MAG_PER_PIRACY;
            self.push_decision(DecisionKind::RaidThreat, 0, 0, 0, mag, now);
        }
        self.decisions.retain(|d| d.deadline_tick > now);
    }

    /// The §13 pressure layer, run each tick: telegraph an incoming raid ahead of
    /// time (forecasting), then fire the ambient raider only when the pacing
    /// governor allows (no dogpiling another flashpoint). Pure scheduling — the
    /// raid itself still resolves with geometry + odds in [`Sim::pirate_raid`].
    pub(crate) fn run_pressure(&mut self) {
        let now = self.tick;
        if self.pressure.should_forecast(now) {
            let eta = self.pressure.raid_eta(now);
            self.events.push(Event::ThreatForecast {
                kind: PressureKind::Piracy,
                eta,
            });
            self.pressure.mark_forecast_sent();
        }
        if self.pressure.raid_ready(now) {
            let struck = self.pirate_raid();
            self.pressure.after_raid(now, struck);
        }
        // The far-side endgame threat (§17, G4) — dormant until the gate is transited.
        if self.pressure.endgame() {
            self.run_incursions(now);
        }
    }
}
