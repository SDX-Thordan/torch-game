//! `shipyard` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// Commission a warship of `class` into the fleet (§5/§8c): pays its build cost and
    /// **reserves** its crew now, then lays the hull down in the shipyard — it stands up into
    /// the fleet once the build completes ([`commission_build_ticks`]), not instantly.
    pub fn commission_ship(&mut self, class: ShipClass) -> Result<(), CommissionError> {
        self.hull_source_ok(class)?;
        let hull = self.catalog.hull(class);
        let price = hull.dry_mass * SHIP_PRICE_PER_MASS;
        if self.corp.credits() < price {
            return Err(CommissionError::CantAfford);
        }
        if self.corp.trained_crew() < hull.crew_required {
            return Err(CommissionError::NotEnoughCrew);
        }
        self.corp.debit(price);
        self.lay_down_ship(class, None);
        Ok(())
    }

    // ---- shipyards: where warships come from --------------------------------

    /// Whether `class` can be built given your shipyard + OPA standing. Civilians come
    /// from Tycho freely; **corvettes** (Frigates) need a yard *or* good Belt standing;
    /// **Destroyer/Cruiser/Battleship** need a yard of tier ≥ 1/2/3.
    /// The shipyard tier that's **operational** right now — 0 while a found/expand build is
    /// still in progress (a building yard can't lay down hulls yet).
    pub(crate) fn operational_shipyard_tier(&self) -> i64 {
        if self.shipyard_tier > 0 && self.tick >= self.shipyard_ready_tick {
            self.shipyard_tier
        } else {
            0
        }
    }

    pub(crate) fn hull_source_ok(&self, class: ShipClass) -> Result<(), CommissionError> {
        let tier = self.operational_shipyard_tier();
        let need = match class {
            ShipClass::Frigate => {
                if tier >= 1 || self.relations.standing(Faction::Belt) >= CORVETTE_STANDING {
                    return Ok(());
                }
                return Err(CommissionError::NeedShipyard);
            }
            ShipClass::Destroyer => 1,
            ShipClass::Cruiser => 2,
            ShipClass::Battleship => 3,
            _ => return Ok(()), // civilians: bought from Tycho
        };
        if tier >= need {
            Ok(())
        } else {
            Err(CommissionError::NeedShipyard)
        }
    }

    /// Days left on the shipyard's current build (0 iff operational — ceiling so it only
    /// reads 0 once `tick >= ready_tick`, matching `operational_shipyard_tier`).
    pub fn shipyard_build_days(&self) -> u64 {
        if self.shipyard_tier > 0 && self.tick < self.shipyard_ready_tick {
            (self.shipyard_ready_tick - self.tick).div_ceil(6)
        } else {
            0
        }
    }

    /// Whether `class` can be sourced right now (shipyard tier / OPA standing) — for the
    /// shell to show availability before the player tries.
    pub fn can_commission(&self, class: ShipClass) -> bool {
        self.hull_source_ok(class).is_ok()
    }

    /// The player's shipyard tier (0 = none).
    pub fn shipyard_tier(&self) -> i64 {
        self.shipyard_tier
    }

    /// Sandbox/test affordance: a free max-tier shipyard (the gated path is covered by
    /// the native tests + the personas).
    pub fn dev_grant_shipyard(&mut self) {
        self.shipyard_tier = MAX_SHIPYARD_TIER;
        self.shipyard_body = 1;
    }
    /// The body the shipyard orbits (0 if none).
    pub fn shipyard_body(&self) -> usize {
        self.shipyard_body
    }
    /// The largest hull class the current yard tier can lay down (for the shell).
    pub fn shipyard_max_hull(&self) -> &'static str {
        match self.shipyard_tier {
            0 => "—",
            1 => "Destroyer",
            2 => "Cruiser",
            _ => "Battleship",
        }
    }

    /// Found a shipyard at `body` (an asteroid/moon/dwarf or a station body — not the sun
    /// or the gate). Very expensive; sets tier 1. Errors if you already have one.
    pub fn found_shipyard(&mut self, body: usize) -> Result<(), ShipyardError> {
        if self.shipyard_tier > 0 {
            return Err(ShipyardError::AlreadyBuilt);
        }
        let bodies = crate::sim::orbit::default_system();
        match bodies.get(body) {
            Some(b)
                if !matches!(
                    b.kind,
                    crate::sim::orbit::BodyKind::Star | crate::sim::orbit::BodyKind::Gate
                ) => {}
            _ => return Err(ShipyardError::BadSite),
        }
        if self.corp.credits() < SHIPYARD_FOUND_COST {
            return Err(ShipyardError::CantAfford);
        }
        self.corp.debit(SHIPYARD_FOUND_COST);
        self.shipyard_tier = 1;
        self.shipyard_body = body;
        self.shipyard_ready_tick = self.tick + SHIPYARD_FOUND_TICKS; // ~a year to stand up
        self.complete_op();
        Ok(())
    }

    /// Expand the shipyard one tier (unlocks the next hull class). Each tier is dearer.
    pub fn expand_shipyard(&mut self) -> Result<(), ShipyardError> {
        if self.shipyard_tier == 0 {
            return Err(ShipyardError::NoneBuilt);
        }
        if self.shipyard_tier >= MAX_SHIPYARD_TIER {
            return Err(ShipyardError::Maxed);
        }
        // Can't start a new expansion while the current build is still in progress.
        if self.tick < self.shipyard_ready_tick {
            return Err(ShipyardError::NoneBuilt);
        }
        let cost = SHIPYARD_EXPAND_COST * self.shipyard_tier;
        if self.corp.credits() < cost {
            return Err(ShipyardError::CantAfford);
        }
        self.corp.debit(cost);
        self.shipyard_ready_tick = self.tick + SHIPYARD_EXPAND_TICKS;
        self.shipyard_tier += 1;
        self.complete_op();
        Ok(())
    }

    /// Credit cost to expand the yard (−1 if none / maxed).
    pub fn expand_shipyard_cost(&self) -> i64 {
        if self.shipyard_tier == 0 || self.shipyard_tier >= MAX_SHIPYARD_TIER {
            -1
        } else {
            SHIPYARD_EXPAND_COST * self.shipyard_tier
        }
    }

    // ---- early-game mining: bootstrap the industrial empire -----------------
}
