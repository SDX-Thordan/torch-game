//! The player corporation (§1, §5) — the player's *place in the world*.
//!
//! Per the playable-state review, this is the foundational gap: the sim modelled
//! a convincing NPC world but had no player economic actor. `Corp` is that actor
//! — a treasury, a cargo warehouse, an owned fleet, and the scarce trained-crew
//! pool (§8c). The trade/commission *verbs* live on `Sim` (which owns the markets
//! and RNG); this holds the state and its guarded mutations. Integer (§27).

use super::ships::Loadout;

/// Credits the corporation starts with — enough for escorts and then some, so
/// the trained-crew pool (not the treasury) is what caps capital ships (§8c).
const STARTING_CREDITS: i64 = 50_000;
/// Trained crew on the books at founding (§8c bottleneck): a few frigates' worth.
const STARTING_CREW: i64 = 60;

/// A ship in the player's fleet: a validated fit plus a christened name (§14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedShip {
    pub name: String,
    pub loadout: Loadout,
}

/// The player corporation's holdings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Corp {
    credits: i64,
    /// Cargo held per commodity (bought low to sell high).
    warehouse: Vec<i64>,
    fleet: Vec<OwnedShip>,
    /// Untasked trained crew available to stand up new warships (§8c).
    trained_crew: i64,
}

impl Corp {
    /// Found the corporation with a starting treasury and crew.
    pub fn new(commodity_count: usize) -> Self {
        Self {
            credits: STARTING_CREDITS,
            warehouse: vec![0; commodity_count],
            fleet: Vec::new(),
            trained_crew: STARTING_CREW,
        }
    }

    pub fn credits(&self) -> i64 {
        self.credits
    }

    pub fn trained_crew(&self) -> i64 {
        self.trained_crew
    }

    pub fn fleet(&self) -> &[OwnedShip] {
        &self.fleet
    }

    /// Cargo held of commodity `c`.
    pub fn cargo(&self, c: usize) -> i64 {
        self.warehouse.get(c).copied().unwrap_or(0)
    }

    /// Spend `n` credits if affordable; returns whether it went through.
    pub fn debit(&mut self, n: i64) -> bool {
        if self.credits >= n {
            self.credits -= n;
            true
        } else {
            false
        }
    }

    /// Receive `n` credits.
    pub fn credit(&mut self, n: i64) {
        self.credits += n;
    }

    /// Add `qty` of commodity `c` to the warehouse.
    pub fn store(&mut self, c: usize, qty: i64) {
        self.warehouse[c] += qty;
    }

    /// Remove `qty` of commodity `c` from the warehouse if held.
    pub fn unstore(&mut self, c: usize, qty: i64) -> bool {
        if self.warehouse[c] >= qty {
            self.warehouse[c] -= qty;
            true
        } else {
            false
        }
    }

    /// Assign `n` trained crew to a new hull if the pool can cover it (§8c).
    pub fn assign_crew(&mut self, n: i64) -> bool {
        if self.trained_crew >= n {
            self.trained_crew -= n;
            true
        } else {
            false
        }
    }

    /// Add a commissioned ship to the fleet.
    pub fn add_ship(&mut self, ship: OwnedShip) {
        self.fleet.push(ship);
    }
}
