//! `mining` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// The deployed miners (early industry).
    pub fn miners(&self) -> &[Miner] {
        &self.miners
    }

    /// The raw mineral a body yields when mined (0 Ice / 1 Ore / 2 Volatiles) —
    /// deterministic by the body, so *where* you deploy decides *what* you extract.
    pub fn body_mineral(&self, body: usize) -> usize {
        body.wrapping_mul(2_654_435_761) % 3
    }

    /// Whether the player may deploy a miner at `body`. Player mining is confined to
    /// the **asteroid/Kuiper belts** (the dwarf bodies, Ceres & Pluto) and the **rings
    /// and moons of the outer systems** (moons of the gas giants + outer dwarfs). The
    /// **Earth and Mars AO** — the inner planets and their moons (Luna, Phobos, Deimos)
    /// — is off-limits to player miners.
    pub fn can_mine_body(&self, body: usize) -> bool {
        use crate::sim::orbit::BodyKind;
        let bodies = crate::sim::orbit::default_system();
        let Some(b) = bodies.get(body) else {
            return false;
        };
        match b.kind {
            // The belts: Ceres/Pluto (dwarf bodies) and the named belt asteroids
            // (Eros/Pallas/Vesta/Tycho) are all workable mining ground.
            BodyKind::DwarfPlanet | BodyKind::Asteroid => true,
            // Moons/rings — but only of the *outer* systems. A moon of a Planet
            // (Earth/Mars) is inner-system AO; a moon of a GasGiant/DwarfPlanet is fair.
            BodyKind::Moon => matches!(
                bodies.get(b.parent).map(|p| p.kind),
                Some(BodyKind::GasGiant | BodyKind::DwarfPlanet)
            ),
            // Stars, gates, the inner/gas-giant surfaces themselves, the far side: no.
            _ => false,
        }
    }

    /// Buy + deploy a miner at `body` (a civilian bought from Tycho — no shipyard needed).
    /// It mines that body's raw into your warehouse each tick. Cheap; the first step.
    /// Restricted to the belts + the outer moons/rings (not the Earth/Mars AO).
    pub fn buy_miner(&mut self, body: usize) -> Result<(), MinerError> {
        self.commission_miner(body, MinerClass::Prospector)
    }

    /// Commission a mining ship of `class` at `body` — the tiered version. The Prospector is
    /// the cheap, crewless first step (so [`buy_miner`] stays byte-identical); the Harvester
    /// and Refinery Barge are pricier, crew-heavy, higher-yield assets (a real expansion
    /// gate). The hull is christened by deployment order (no RNG → economy untouched, §27).
    pub fn commission_miner(&mut self, body: usize, class: MinerClass) -> Result<(), MinerError> {
        if self.miners.len() >= MAX_MINERS {
            return Err(MinerError::Full);
        }
        if !self.can_mine_body(body) {
            return Err(MinerError::BadSite);
        }
        if self.corp.credits() < class.cost() {
            return Err(MinerError::CantAfford);
        }
        if self.corp.trained_crew() < class.crew() {
            return Err(MinerError::NoCrew);
        }
        self.corp.debit(class.cost());
        if class.crew() > 0 {
            self.corp.assign_crew(class.crew());
        }
        let commodity = self.body_mineral(body);
        let name = MINER_NAMES[self.miners.len() % MINER_NAMES.len()].to_string();
        self.miners.push(Miner {
            body,
            commodity,
            class,
            name,
            commissioned_tick: self.tick,
            convoy: None,
        });
        self.complete_op();
        Ok(())
    }

    // ---- convoys (Phase 4): group civilian ships; the miner+hauler synergy ----------

    /// The player's formed convoys.
    pub fn convoys(&self) -> &[Convoy] {
        &self.convoys
    }

    /// Form a new named convoy, returning its stable id.
    pub fn form_convoy(&mut self, name: String) -> u32 {
        let id = self.next_convoy_id;
        self.next_convoy_id += 1;
        let name = if name.is_empty() {
            format!("Convoy {id}")
        } else {
            name
        };
        self.convoys.push(Convoy {
            id,
            name,
            escorts: 0,
        });
        id
    }

    /// Total warships assigned as convoy escorts across all convoys (Phase 5).
    pub fn total_escorts_assigned(&self) -> i64 {
        self.convoys.iter().map(|c| c.escorts as i64).sum()
    }

    /// Assign one warship from the fleet to escort convoy `id` — an actively-screened convoy
    /// deters piracy. Fails if the convoy is unknown or every warship is already on escort duty.
    pub fn escort_convoy(&mut self, id: u32) -> bool {
        if self.total_escorts_assigned() >= self.corp.fleet().len() as i64 {
            return false; // no free warship to assign
        }
        if let Some(c) = self.convoys.iter_mut().find(|c| c.id == id) {
            c.escorts = c.escorts.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// Recall one escort from convoy `id` back to home defense. Returns whether one was freed.
    pub fn recall_escort(&mut self, id: u32) -> bool {
        if let Some(c) = self
            .convoys
            .iter_mut()
            .find(|c| c.id == id && c.escorts > 0)
        {
            c.escorts -= 1;
            true
        } else {
            false
        }
    }

    /// Escorts on the convoy of the miner at `body` (for the shell readout).
    pub fn convoy_escorts_at(&self, body: usize) -> i64 {
        self.miner_convoy_at(body)
            .and_then(|id| self.convoys.iter().find(|c| c.id == id))
            .map(|c| c.escorts as i64)
            .unwrap_or(0)
    }

    /// Assign the miner working `body` to convoy `id` (or `None` to detach). Returns success.
    pub fn set_miner_convoy(&mut self, body: usize, id: Option<u32>) -> bool {
        if let Some(id) = id {
            if !self.convoys.iter().any(|c| c.id == id) {
                return false;
            }
        }
        if let Some(m) = self.miners.iter_mut().find(|m| m.body == body) {
            m.convoy = id;
            true
        } else {
            false
        }
    }

    /// Assign hauler `i` to convoy `id` (or `None` to detach). Returns success.
    pub fn set_hauler_convoy(&mut self, i: usize, id: Option<u32>) -> bool {
        if let Some(id) = id {
            if !self.convoys.iter().any(|c| c.id == id) {
                return false;
            }
        }
        if let Some(h) = self.corp.hauler_mut(i) {
            h.convoy = id;
            true
        } else {
            false
        }
    }

    /// One-press convoy at a mining body: form a convoy named for the body, put the miner there
    /// into it, and assign the first unconvoyed hauler to it — instantly granting the synergy.
    /// Returns the convoy id, or `None` if there's no miner here or no free hauler.
    pub fn form_mining_convoy(&mut self, body: usize) -> Option<u32> {
        if !self.miners.iter().any(|m| m.body == body) {
            return None;
        }
        let free_hauler = self
            .corp
            .haulers()
            .iter()
            .position(|h| h.convoy.is_none())?;
        let body_name = self.bodies.get(body).map(|b| b.name).unwrap_or("Belt");
        let name = format!("{body_name} Convoy");
        let id = self.form_convoy(name);
        self.set_miner_convoy(body, Some(id));
        self.set_hauler_convoy(free_hauler, Some(id));
        Some(id)
    }

    /// The convoy id of the miner working `body`, if any (for the shell readout).
    pub fn miner_convoy_at(&self, body: usize) -> Option<u32> {
        self.miners
            .iter()
            .find(|m| m.body == body)
            .and_then(|m| m.convoy)
    }

    /// Whether the miner at `body` is in a convoy that also has a hauler (the active synergy).
    pub fn miner_has_convoy_synergy(&self, body: usize) -> bool {
        self.miner_convoy_at(body)
            .is_some_and(|id| self.corp.convoy_has_hauler(id))
    }

    /// Whether the player has a miner deployed at `body`.
    pub fn miner_at(&self, body: usize) -> bool {
        self.miners.iter().any(|m| m.body == body)
    }

    /// Recall one miner from `body` (the "until withdrawn" half of the loop). The hull is
    /// retired — no refund — so redeploying is a deliberate decision. Returns whether a
    /// miner was there to withdraw.
    pub fn withdraw_miner(&mut self, body: usize) -> bool {
        if let Some(i) = self.miners.iter().position(|m| m.body == body) {
            self.miners.remove(i);
            true
        } else {
            false
        }
    }

    // ---- outposts: the body-built station layer (found anywhere, develop into a base) ----
}
