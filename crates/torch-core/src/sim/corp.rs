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

/// Corporation name presets the player cycles for self-expression (§14). Evocative
/// of the hard-SF setting; the player picks an identity, not a blank field (mobile).
pub const CORP_NAMES: [&str; 8] = [
    "Helios Combine",
    "Tycho Salvage & Freight",
    "Ceres Mutual",
    "Vesta Industrial",
    "Hyperion Logistics",
    "Pallas Holdings",
    "Kuiper Reach",
    "Meridian Drift Co.",
];

/// Livery colours (RGB 0..=255) painted across the fleet (and the UI accent) — the
/// company's flag, the §14 self-expression half (corp branding/livery).
pub const LIVERY: [(u8, u8, u8); 6] = [
    (102, 242, 255), // cyan (default)
    (255, 176, 64),  // amber
    (120, 230, 140), // green
    (240, 110, 120), // crimson
    (180, 150, 255), // violet
    (235, 235, 240), // white
];

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
    /// Tick a refit completes (0 = not in the yard). While refitting the hull can't
    /// move or fight (Phase B). Transient — not persisted.
    pub refit_until: u64,
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
            refit_until: 0,
        }
    }

    /// Whether the hull is in the yard being refitted (can't move or fight).
    pub fn is_refitting(&self, tick: u64) -> bool {
        self.refit_until > tick
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
    /// The player's corporation name (§14 expressive identity).
    name: String,
    /// Livery colour index into [`LIVERY`] — the company's flag across the fleet.
    livery: usize,
    /// Scrap parts recovered from combat — the input to weapon crafting (Phase B).
    scrap: i64,
    /// Weapon-model ids whose **schematic** the player holds — the designs you know how
    /// to build. Advanced/faction schematics are *earned* (reverse-engineering), never
    /// bought. Production requires the schematic first.
    schematics: Vec<usize>,
    /// Weapon-model ids in **production** (the line is tooled up → fittable on ships).
    /// Starts with the basic tier-0 of each kind.
    arsenal: Vec<usize>,
}

impl Corp {
    /// Found the corporation with a starting treasury and crew.
    pub fn new(commodity_count: usize) -> Self {
        let basics = vec![
            super::weapons::BASIC_PDC,
            super::weapons::BASIC_TORPEDO,
            super::weapons::BASIC_RAILGUN,
        ];
        Self {
            credits: STARTING_CREDITS,
            warehouse: vec![0; commodity_count],
            fleet: Vec::new(),
            trained_crew: STARTING_CREW,
            freighters: 0,
            name: CORP_NAMES[0].to_string(),
            livery: 0,
            scrap: 0,
            schematics: basics.clone(),
            arsenal: basics,
        }
    }

    /// Scrap parts on hand (the production input).
    pub fn scrap(&self) -> i64 {
        self.scrap
    }
    pub fn add_scrap(&mut self, n: i64) {
        self.scrap += n.max(0);
    }
    pub fn spend_scrap(&mut self, n: i64) -> bool {
        if self.scrap >= n {
            self.scrap -= n;
            true
        } else {
            false
        }
    }
    /// Weapon schematics the player holds (the designs they can produce).
    pub fn schematics(&self) -> &[usize] {
        &self.schematics
    }
    pub fn knows_schematic(&self, id: usize) -> bool {
        self.schematics.contains(&id)
    }
    /// Learn a schematic (earned via reverse-engineering — never bought).
    pub fn learn_schematic(&mut self, id: usize) -> bool {
        if self.schematics.contains(&id) {
            false
        } else {
            self.schematics.push(id);
            true
        }
    }
    /// The weapon-model ids the player can fit (production lines established).
    pub fn arsenal(&self) -> &[usize] {
        &self.arsenal
    }
    pub fn owns_weapon(&self, id: usize) -> bool {
        self.arsenal.contains(&id)
    }
    pub fn add_weapon(&mut self, id: usize) {
        if !self.arsenal.contains(&id) {
            self.arsenal.push(id);
        }
    }
    /// Restore the arsenal + schematics + scrap on load (§30).
    pub fn restore_arsenal(&mut self, scrap: i64, schematics: Vec<usize>, arsenal: Vec<usize>) {
        self.scrap = scrap;
        if !schematics.is_empty() {
            self.schematics = schematics;
        }
        if !arsenal.is_empty() {
            self.arsenal = arsenal;
        }
    }

    /// The corporation's name (§14).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Adopt the name preset at `i` (cycled by the player). Returns the new name.
    pub fn set_name_preset(&mut self, i: usize) -> &str {
        self.name = CORP_NAMES[i % CORP_NAMES.len()].to_string();
        &self.name
    }

    /// The livery colour index, and its RGB (§14).
    pub fn livery(&self) -> usize {
        self.livery
    }

    pub fn livery_rgb(&self) -> (u8, u8, u8) {
        LIVERY[self.livery % LIVERY.len()]
    }

    /// Cycle to the next livery; returns the new index.
    pub fn cycle_livery(&mut self) -> usize {
        self.livery = (self.livery + 1) % LIVERY.len();
        self.livery
    }

    /// Restore the chosen identity from a save (§30).
    pub fn set_identity(&mut self, name: String, livery: usize) {
        if !name.is_empty() {
            self.name = name;
        }
        self.livery = livery % LIVERY.len();
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

    /// Resolve an engagement fought by a **subset** of the fleet (§6/§13): only
    /// the ships in `participants` (by index) are at risk — the rest were off
    /// station and untouched. Within the participants the Rocinante effect still
    /// holds (veterans pull through, the green tail is lost); survivors are blooded.
    /// Returns the lost hulls' names. This is the position-aware counterpart of
    /// [`resolve_engagement`].
    pub fn resolve_engagement_for(
        &mut self,
        mut participants: Vec<usize>,
        survivors: usize,
        won: bool,
    ) -> Vec<String> {
        // Veterans (wins → battles → seniority) sort to the front of the engaged
        // group, so the green hulls are the ones lost.
        participants.sort_by(|&a, &b| {
            let (sa, sb) = (&self.fleet[a], &self.fleet[b]);
            sb.battles_won
                .cmp(&sa.battles_won)
                .then(sb.battles.cmp(&sa.battles))
                .then(sa.commissioned_tick.cmp(&sb.commissioned_tick))
        });
        let kept = survivors.min(participants.len());
        let lost_idx: Vec<usize> = participants.iter().skip(kept).copied().collect();
        let lost_names: Vec<String> = lost_idx
            .iter()
            .map(|&i| self.fleet[i].name.clone())
            .collect();
        // Blood the engaged survivors.
        for &i in participants.iter().take(kept) {
            self.fleet[i].record_battle(won);
        }
        // Remove the lost hulls, high index first so the earlier indices stay valid.
        let mut lost_sorted = lost_idx;
        lost_sorted.sort_unstable_by(|a, b| b.cmp(a));
        for i in lost_sorted {
            self.fleet.remove(i);
        }
        lost_names
    }

    /// The most decorated hull in the fleet (most wins), if any — the hero ship the
    /// shell can spotlight (§14 Rocinante effect).
    pub fn flagship(&self) -> Option<&OwnedShip> {
        self.fleet.iter().max_by_key(|s| (s.battles_won, s.battles))
    }

    /// Fleet index of the flagship (the most-decorated hull), or -1 if no ships —
    /// the handle the shell renames (§14).
    pub fn flagship_index(&self) -> i64 {
        self.fleet
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| (s.battles_won, s.battles))
            .map(|(i, _)| i as i64)
            .unwrap_or(-1)
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
    fn corp_identity_picks_a_name_and_cycles_livery() {
        let mut c = Corp::new(6);
        assert_eq!(c.name(), CORP_NAMES[0]);
        assert_eq!(c.livery(), 0);
        c.set_name_preset(2);
        assert_eq!(c.name(), CORP_NAMES[2]);
        // Cycling wraps through the whole palette and back to 0.
        for i in 1..LIVERY.len() {
            assert_eq!(c.cycle_livery(), i);
        }
        assert_eq!(c.cycle_livery(), 0, "wraps");
        // Restore overlays a saved identity.
        c.set_identity("Custom Co.".into(), 3);
        assert_eq!(c.name(), "Custom Co.");
        assert_eq!(c.livery(), 3);
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
