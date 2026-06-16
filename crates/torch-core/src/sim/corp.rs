//! The player corporation (§1, §5) — the player's *place in the world*.
//!
//! Per the playable-state review, this is the foundational gap: the sim modelled
//! a convincing NPC world but had no player economic actor. `Corp` is that actor
//! — a treasury, a cargo warehouse, an owned fleet, and the scarce trained-crew
//! pool (§8c). The trade/commission *verbs* live on `Sim` (which owns the markets
//! and RNG); this holds the state and its guarded mutations. Integer (§27).

use super::movement::Nav;
use super::ships::Loadout;

/// Credits the corporation starts with — enough for escorts and then some, so
/// the trained-crew pool (not the treasury) is what caps capital ships (§8c).
const STARTING_CREDITS: i64 = 50_000;
/// Trained crew on the books at founding (§8c bottleneck): a few frigates' worth.
const STARTING_CREW: i64 = 60;

/// A ship in the player's fleet: a validated fit, a christened name (§14), and an
/// accruing **service history** (§11/§13) — the age and battle record that turn a
/// hull into a *beloved hero ship* and make losing a veteran a felt, permanent
/// loss (the load-bearing source of tension, §13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedShip {
    pub name: String,
    pub loadout: Loadout,
    /// Tick the hull was commissioned (for its age in service).
    pub commissioned_tick: u64,
    /// Engagements fought, and how many were won — its blooding (§8c/§13).
    pub battles: u16,
    pub battles_won: u16,
    /// Position + delta-v/remass budget (§6) — the ship is a positional asset.
    pub nav: Nav,
}

/// Battles a hull must have won to read as a *veteran* (a hero ship, §14).
const VETERAN_WINS: u16 = 1;

impl OwnedShip {
    /// A freshly commissioned hull (no history yet), docked at `location` with a
    /// full tank (§6).
    pub fn new(name: String, loadout: Loadout, commissioned_tick: u64, location: usize) -> Self {
        let remass_max = loadout.hull().remass_capacity;
        Self {
            name,
            loadout,
            commissioned_tick,
            battles: 0,
            battles_won: 0,
            nav: Nav::docked(location, remass_max),
        }
    }

    /// Record an engagement this hull lived through (§13 blooding).
    pub fn record_battle(&mut self, won: bool) {
        self.battles = self.battles.saturating_add(1);
        if won {
            self.battles_won = self.battles_won.saturating_add(1);
        }
    }

    /// A blooded hull the player has reason to care about (the Rocinante effect).
    pub fn is_veteran(&self) -> bool {
        self.battles_won >= VETERAN_WINS
    }

    /// Ticks in service as of `now`.
    pub fn age(&self, now: u64) -> u64 {
        now.saturating_sub(self.commissioned_tick)
    }
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
    /// Freighters owned, for running trade-route standing orders (§4).
    freighters: i64,
}

impl Corp {
    /// Found the corporation with a starting treasury and crew.
    pub fn new(commodity_count: usize) -> Self {
        Self {
            credits: STARTING_CREDITS,
            warehouse: vec![0; commodity_count],
            fleet: Vec::new(),
            trained_crew: STARTING_CREW,
            freighters: 0,
        }
    }

    pub fn freighters(&self) -> i64 {
        self.freighters
    }

    /// Add a freighter to the books (a commissioned civilian hauler).
    pub fn add_freighter(&mut self) {
        self.freighters += 1;
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

    /// Mutable fleet access — for advancing ship navigation (§6).
    pub fn fleet_mut(&mut self) -> &mut [OwnedShip] {
        &mut self.fleet
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

    /// Resolve an engagement against the fleet (§11/§13): the most-storied hulls
    /// pull through first (the Rocinante effect — veterans survive preferentially),
    /// the green ships are lost, and every survivor is blooded by the fight. Returns
    /// the names of the hulls lost, so the feed/log can mourn them by name.
    pub fn resolve_engagement(&mut self, survivors: usize, won: bool) -> Vec<String> {
        // Veterans (by wins, then battles, then seniority) sort to the front and
        // survive; the truncated tail is lost.
        self.fleet.sort_by(|a, b| {
            b.battles_won
                .cmp(&a.battles_won)
                .then(b.battles.cmp(&a.battles))
                .then(a.commissioned_tick.cmp(&b.commissioned_tick))
        });
        let lost: Vec<String> = self
            .fleet
            .iter()
            .skip(survivors.min(self.fleet.len()))
            .map(|s| s.name.clone())
            .collect();
        self.fleet.truncate(survivors);
        for s in &mut self.fleet {
            s.record_battle(won);
        }
        lost
    }

    /// The most decorated hull in the fleet (most wins), if any — the hero ship the
    /// shell can spotlight (§14 Rocinante effect).
    pub fn flagship(&self) -> Option<&OwnedShip> {
        self.fleet.iter().max_by_key(|s| (s.battles_won, s.battles))
    }

    /// Cargo held per commodity (for persistence, §30).
    pub fn warehouse(&self) -> &[i64] {
        &self.warehouse
    }

    /// Overwrite the whole holdings from a loaded save (§30). The fleet is rebuilt
    /// by the caller (loadouts are reconstructed from class + crew quality).
    pub fn restore(
        &mut self,
        credits: i64,
        warehouse: Vec<i64>,
        trained_crew: i64,
        freighters: i64,
        fleet: Vec<OwnedShip>,
    ) {
        self.credits = credits;
        self.warehouse = warehouse;
        self.trained_crew = trained_crew;
        self.freighters = freighters;
        self.fleet = fleet;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::rng::Pcg32;
    use crate::sim::ships::{reference_loadout, ShipClass};

    /// A fleet of frigates with the given names, all commissioned at tick 0.
    fn fleet(names: &[&str]) -> Corp {
        let mut rng = Pcg32::new(1);
        let mut corp = Corp::new(6);
        for n in names {
            let loadout = reference_loadout(ShipClass::Frigate, &mut rng);
            corp.add_ship(OwnedShip::new(n.to_string(), loadout, 0, 3));
        }
        corp
    }

    #[test]
    fn a_surviving_hull_is_blooded_and_becomes_a_veteran() {
        let mut corp = fleet(&["Lodestar", "Kestrel"]);
        assert!(!corp.fleet()[0].is_veteran());
        let lost = corp.resolve_engagement(2, true); // both live, both win
        assert!(lost.is_empty());
        for s in corp.fleet() {
            assert_eq!((s.battles, s.battles_won), (1, 1));
            assert!(s.is_veteran(), "a won engagement bloods the hull");
        }
    }

    #[test]
    fn veterans_pull_through_first_and_the_green_are_lost() {
        // One hull already has a win; a brutal fight leaves only one survivor.
        let mut corp = fleet(&["Green", "Hero"]);
        // Blood "Hero" in an earlier all-survive fight, then reset to two hulls.
        corp.fleet[1].record_battle(true);
        let lost = corp.resolve_engagement(1, false);
        assert_eq!(corp.fleet().len(), 1);
        assert_eq!(corp.fleet()[0].name, "Hero", "the veteran pulls through");
        assert_eq!(
            lost,
            vec!["Green".to_string()],
            "the green hull is mourned by name"
        );
    }

    #[test]
    fn flagship_is_the_most_decorated() {
        let mut corp = fleet(&["A", "B", "C"]);
        corp.fleet[1].battles_won = 3;
        corp.fleet[1].battles = 5;
        assert_eq!(corp.flagship().unwrap().name, "B");
    }

    #[test]
    fn age_counts_ticks_in_service() {
        let ship = {
            let mut rng = Pcg32::new(2);
            OwnedShip::new(
                "X".into(),
                reference_loadout(ShipClass::Frigate, &mut rng),
                100,
                3,
            )
        };
        assert_eq!(ship.age(360), 260);
        assert_eq!(ship.age(50), 0, "never negative");
    }
}
