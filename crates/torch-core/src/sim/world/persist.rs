//! `persist` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// Capture the run as a deterministic [`SaveState`] (§30): seed + tick + the
    /// mutable player/economy state. Static content (catalogs, bodies) is rebuilt
    /// on load, so it isn't stored.
    pub fn to_save(&self) -> crate::sim::persist::SaveState {
        use crate::sim::persist::{MarketSave, SaveState, ShipSave, SAVE_VERSION};
        let fleet = self
            .corp
            .fleet()
            .iter()
            .map(|s| ShipSave {
                name: s.name.clone(),
                class: s.loadout.hull().class,
                commissioned_tick: s.commissioned_tick,
                battles: s.battles,
                battles_won: s.battles_won,
                crew_quality: s.loadout.crew().quality,
                nav: s.nav,
            })
            .collect();
        let markets = self
            .markets
            .iter()
            .map(|m| MarketSave {
                stocks: m.stocks().iter().map(|s| s.stock).collect(),
                prices: m.stocks().iter().map(|s| s.price).collect(),
            })
            .collect();
        SaveState {
            version: SAVE_VERSION,
            seed: self.seed,
            tick: self.tick,
            credits: self.corp.credits(),
            warehouse: self.corp.warehouse().to_vec(),
            trained_crew: self.corp.trained_crew(),
            freighters: self.corp.freighters(),
            haulers: self.corp.haulers().to_vec(),
            scrap: self.corp.scrap(),
            schematics: self.corp.schematics().to_vec(),
            arsenal: self.corp.arsenal().to_vec(),
            weapon_production: self.weapon_production.clone(),
            fleet,
            corp_name: self.corp.name().to_string(),
            corp_livery: self.corp.livery(),
            relations: self.relations.clone(),
            campaign: self.campaign,
            research_unlocked: self.progression.research.flags().to_vec(),
            research_points: self.progression.research.points(),
            blueprints_known: self.progression.blueprints.flags().to_vec(),
            ceo_xp: self.progression.ceo.xp(),
            ceo_branch: self.progression.ceo.branch(),
            mission_done: self.missions.done_flags(),
            gate_revealed: self.missions.gate_beats_revealed(),
            bridgehead: self.bridgehead,
            endgame_since: self.endgame_since,
            incursions_survived: self.incursions_survived,
            endgame_outcome: self.endgame_outcome,
            controlled_colonies: self.controlled.clone(),
            colony_dev: self.colony_dev.clone(),
            colony_dev_ready: self.colony_dev_ready.clone(),
            dev_doctrine: self.dev_doctrine,
            shipyard_tier: self.shipyard_tier,
            shipyard_ready_tick: self.shipyard_ready_tick,
            shipyard_body: self.shipyard_body,
            pending_ships: self.pending_ships.clone(),
            miners: self.miners.clone(),
            convoys: self.convoys.clone(),
            next_convoy_id: self.next_convoy_id,
            outposts: self.outposts.clone(),
            next_war_flashpoint: self.next_war_flashpoint,
            contested_player_influence: self.contested.iter().map(|c| c.player_influence).collect(),
            faction_alarm: self.faction_alarm,
            influence: self.influence,
            company_relations: self.diplomacy.relations(),
            routes: self.routes.clone(),
            stations: self.stations.clone(),
            policy: self.policy,
            intensity: self.pressure.intensity(),
            alert_threshold: self.feed.threshold(),
            markets,
        }
    }

    /// Serialize the run to a JSON save document (the dev export, §30).
    pub fn save_json(&self) -> String {
        self.to_save().to_json()
    }

    /// Serialize the run to the compact **binary** shipping save (§30): bincode.
    pub fn save_bytes(&self) -> Vec<u8> {
        self.to_save().to_bincode()
    }

    /// Rebuild a [`Sim`] from a JSON save (§30).
    pub fn load_json(json: &str) -> Result<Self, String> {
        Self::rebuild_from_save(crate::sim::persist::SaveState::from_json(json)?)
    }

    /// Rebuild a [`Sim`] from a save document, **auto-detecting** the format (§30):
    /// a leading `{`/whitespace is the JSON dev export, anything else is the binary
    /// shipping format. So new binary saves and old JSON saves both load.
    pub fn load_bytes(bytes: &[u8]) -> Result<Self, String> {
        let looks_json = bytes
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|&b| b == b'{');
        let save = if looks_json {
            let json = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
            crate::sim::persist::SaveState::from_json(json)?
        } else {
            crate::sim::persist::SaveState::from_bincode(bytes)?
        };
        Self::rebuild_from_save(save)
    }

    /// Reconstruct the seeded world, re-sim the ambient layer up to the saved tick
    /// so its phase lines up, then overlay the saved player + economy state (§30).
    pub(crate) fn rebuild_from_save(save: crate::sim::persist::SaveState) -> Result<Self, String> {
        let mut sim = Sim::new(save.seed);
        // Advance the ambient world (traffic, pressure, salvage, RNG phase) to the
        // saved tick. Player automation is off in a fresh sim, so these steps add
        // no player-driven state — the overlay below restores all of that.
        for _ in 0..save.tick {
            sim.step();
        }
        sim.apply_save(&save);
        Ok(sim)
    }

    /// Overlay a loaded [`SaveState`] onto a sim already re-simmed to its tick.
    pub(crate) fn apply_save(&mut self, s: &crate::sim::persist::SaveState) {
        self.tick = s.tick;
        // Restore the arsenal (+ schematics) first so the fleet rebuilds with the
        // player's best-owned weapons (Phase B) — a reload never downgrades your guns.
        self.corp
            .restore_arsenal(s.scrap, s.schematics.clone(), s.arsenal.clone());
        self.weapon_production = s.weapon_production.clone();
        let pdc = self.best_weapon_def(WeaponKind::Pdc);
        let torp = self.best_weapon_def(WeaponKind::Torpedo);
        let rail = self.best_weapon_def(WeaponKind::Railgun);
        // Rebuild each hull's loadout from its class + crew quality (§14), then
        // restore its name and service history.
        let fleet = s
            .fleet
            .iter()
            .map(|sh| {
                let loadout = self.catalog.loadout_with(
                    sh.class,
                    &pdc,
                    &torp,
                    &rail,
                    sh.crew_quality,
                    &mut self.rng,
                );
                let mut ship = OwnedShip::new(
                    sh.name.clone(),
                    loadout,
                    sh.commissioned_tick,
                    sh.nav.location,
                );
                ship.battles = sh.battles;
                ship.battles_won = sh.battles_won;
                ship.nav = sh.nav;
                ship
            })
            .collect();
        // Haulers: use the saved tiered list, or rebuild Light haulers from the legacy count
        // (old saves predate hull tiers).
        let haulers: Vec<crate::sim::corp::Hauler> = if s.haulers.is_empty() && s.freighters > 0 {
            (0..s.freighters)
                .map(|_| crate::sim::corp::Hauler {
                    class: crate::sim::corp::HaulerClass::Light,
                    name: String::new(),
                    commissioned_tick: 0,
                    pdc: 0,
                    torpedo: 0,
                    convoy: None,
                })
                .collect()
        } else {
            s.haulers.clone()
        };
        self.corp.restore(
            s.credits,
            s.warehouse.clone(),
            s.trained_crew,
            haulers,
            fleet,
        );
        self.corp.set_identity(s.corp_name.clone(), s.corp_livery);
        self.relations = s.relations.clone();
        self.campaign = s.campaign;
        self.progression
            .research
            .restore(s.research_unlocked.clone(), s.research_points);
        self.progression
            .blueprints
            .restore(s.blueprints_known.clone());
        self.progression.ceo.restore(s.ceo_xp, s.ceo_branch);
        self.missions.restore(&s.mission_done, s.gate_revealed);
        self.bridgehead = s.bridgehead;
        // Resume the far-side endgame clock if this is a post-transit save (§17, G4).
        self.endgame_since = s.endgame_since;
        if let Some(start) = s.endgame_since {
            self.pressure.begin_endgame(start);
        }
        self.incursions_survived = s.incursions_survived;
        self.endgame_outcome = s.endgame_outcome;
        // The empire layer (E1): restore controlled colonies if the save carries them
        // (old saves / fresh games control none → keep the all-false default).
        if s.controlled_colonies.len() == self.controlled.len() {
            self.controlled = s.controlled_colonies.clone();
        }
        if s.colony_dev.len() == self.colony_dev.len() {
            self.colony_dev = s.colony_dev.clone();
        }
        if s.colony_dev_ready.len() == self.colony_dev_ready.len() {
            self.colony_dev_ready = s.colony_dev_ready.clone();
        }
        self.dev_doctrine = s.dev_doctrine;
        self.shipyard_tier = s.shipyard_tier;
        self.shipyard_ready_tick = s.shipyard_ready_tick;
        self.shipyard_body = s.shipyard_body;
        self.pending_ships = s.pending_ships.clone();
        self.miners = s.miners.clone();
        self.convoys = s.convoys.clone();
        self.next_convoy_id = s.next_convoy_id.max(1);
        self.outposts = s.outposts.clone();
        if s.next_war_flashpoint > 0 {
            self.next_war_flashpoint = s.next_war_flashpoint;
        }
        // The ambient powers' influence + flare schedule replayed during the re-sim
        // above; overlay only the player's accumulated standing (player-driven, not
        // re-simmed). Length-guarded so the contest layout can evolve safely.
        if s.contested_player_influence.len() == self.contested.len() {
            for (c, &pi) in self
                .contested
                .iter_mut()
                .zip(s.contested_player_influence.iter())
            {
                c.player_influence = pi.clamp(0, contest::CONTEST_TOTAL);
            }
        }
        // E3/E7: restore per-faction alarm; the strike schedule re-arms from it.
        self.faction_alarm = s.faction_alarm;
        for a in &mut self.faction_alarm {
            *a = (*a).clamp(0, ALARM_MAX);
        }
        self.next_coalition_strike = 0;
        self.influence = s.influence.clamp(0, INFLUENCE_MAX); // E4
        if !s.company_relations.is_empty() {
            self.diplomacy.restore(&s.company_relations); // E8
        }
        self.routes = s.routes.clone();
        self.stations = s.stations.clone();
        self.policy = s.policy;
        self.pressure.set_intensity(s.intensity);
        self.feed.set_threshold(s.alert_threshold);
        for (m, ms) in self.markets.iter_mut().zip(&s.markets) {
            m.restore_stocks(&ms.stocks, &ms.prices);
        }
    }
}
