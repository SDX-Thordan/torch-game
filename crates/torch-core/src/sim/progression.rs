//! Progression — the three advancement tracks of §10 (reputation is its own
//! module). Kept light per §0.2: satisfying, never a grind wall. Data-driven and
//! deterministic (§27); player-driven, so no RNG.
//!
//! - **Research** — module-tier efficiencies behind cheap prerequisites.
//! - **Blueprints** — a design = a compact seed + parameter set (§25); discovered
//!   by purchase/salvage/reverse-engineering, faction designs gated by reputation.
//! - **CEO skills** — a level track plus one perk branch of passive buffs.

use super::faction::{Faction, Relations};
use super::ships::ShipClass;

// ---- Research tree (§10) -------------------------------------------------

/// What unlocking a tech improves (percent bonuses applied to ship stats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TechEffect {
    Drive(i64),
    Armor(i64),
    Screen(i64),
}

/// A node in the research tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TechDef {
    pub name: &'static str,
    pub cost: i64,
    pub prereq: Option<usize>,
    pub effect: TechEffect,
}

fn tech_catalog() -> Vec<TechDef> {
    vec![
        TechDef {
            name: "Fusion Drives I",
            cost: 100,
            prereq: None,
            effect: TechEffect::Drive(10),
        },
        TechDef {
            name: "Fusion Drives II",
            cost: 250,
            prereq: Some(0),
            effect: TechEffect::Drive(15),
        },
        TechDef {
            name: "Composite Armor",
            cost: 120,
            prereq: None,
            effect: TechEffect::Armor(15),
        },
        TechDef {
            name: "Reactive Plating",
            cost: 300,
            prereq: Some(2),
            effect: TechEffect::Armor(20),
        },
        TechDef {
            name: "PDC Fire Control",
            cost: 150,
            prereq: None,
            effect: TechEffect::Screen(20),
        },
        TechDef {
            name: "Networked PDC",
            cost: 320,
            prereq: Some(4),
            effect: TechEffect::Screen(25),
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResearchError {
    AlreadyKnown,
    PrereqMissing,
    NotEnoughPoints,
}

/// The player's research progress.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Research {
    catalog: Vec<TechDef>,
    points: i64,
    unlocked: Vec<bool>,
}

impl Default for Research {
    fn default() -> Self {
        Self::new()
    }
}

impl Research {
    pub fn new() -> Self {
        let catalog = tech_catalog();
        let unlocked = vec![false; catalog.len()];
        Self {
            catalog,
            points: 0,
            unlocked,
        }
    }

    pub fn catalog(&self) -> &[TechDef] {
        &self.catalog
    }

    pub fn points(&self) -> i64 {
        self.points
    }

    /// Earn research points (from contracts/operations).
    pub fn add_points(&mut self, n: i64) {
        self.points += n.max(0);
    }

    pub fn is_unlocked(&self, i: usize) -> bool {
        self.unlocked.get(i).copied().unwrap_or(false)
    }

    /// Whether tech `i` can be researched right now.
    pub fn can_research(&self, i: usize) -> Result<(), ResearchError> {
        let def = &self.catalog[i];
        if self.unlocked[i] {
            return Err(ResearchError::AlreadyKnown);
        }
        if let Some(p) = def.prereq {
            if !self.unlocked[p] {
                return Err(ResearchError::PrereqMissing);
            }
        }
        if self.points < def.cost {
            return Err(ResearchError::NotEnoughPoints);
        }
        Ok(())
    }

    /// Spend points to unlock tech `i`.
    pub fn research(&mut self, i: usize) -> Result<(), ResearchError> {
        self.can_research(i)?;
        self.points -= self.catalog[i].cost;
        self.unlocked[i] = true;
        Ok(())
    }

    /// Count of unlocked techs.
    pub fn unlocked_count(&self) -> usize {
        self.unlocked.iter().filter(|u| **u).count()
    }

    /// Aggregate percent bonus of a kind across all unlocked techs.
    fn bonus(&self, pick: impl Fn(TechEffect) -> Option<i64>) -> i64 {
        self.catalog
            .iter()
            .zip(&self.unlocked)
            .filter(|(_, u)| **u)
            .filter_map(|(d, _)| pick(d.effect))
            .sum()
    }

    pub fn drive_bonus(&self) -> i64 {
        self.bonus(|e| {
            if let TechEffect::Drive(v) = e {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn armor_bonus(&self) -> i64 {
        self.bonus(|e| {
            if let TechEffect::Armor(v) = e {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn screen_bonus(&self) -> i64 {
        self.bonus(|e| {
            if let TechEffect::Screen(v) = e {
                Some(v)
            } else {
                None
            }
        })
    }
}

// ---- Blueprints (§10, §25) ----------------------------------------------

/// A design's parameter set: stored with a seed instead of a baked mesh (§25).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlueprintParams {
    pub hull: ShipClass,
}

/// A discoverable design (§25): a compact seed + parameter set, optionally a
/// faction-specific design gated behind reputation (§10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlueprintDef {
    pub name: &'static str,
    pub seed: u64,
    pub params: BlueprintParams,
    pub faction: Option<Faction>,
    pub rep_required: i64,
}

fn blueprint_catalog() -> Vec<BlueprintDef> {
    vec![
        BlueprintDef {
            name: "Belter Frigate",
            seed: 0x5EED_0001,
            params: BlueprintParams {
                hull: ShipClass::Frigate,
            },
            faction: None,
            rep_required: 0,
        },
        BlueprintDef {
            name: "Convoy Q-ship",
            seed: 0x5EED_0002,
            params: BlueprintParams {
                hull: ShipClass::QShip,
            },
            faction: None,
            rep_required: 0,
        },
        BlueprintDef {
            name: "Martian Cruiser",
            seed: 0x5EED_0003,
            params: BlueprintParams {
                hull: ShipClass::Cruiser,
            },
            faction: Some(Faction::Mars),
            rep_required: 400,
        },
        BlueprintDef {
            name: "Earth Battleship",
            seed: 0x5EED_0004,
            params: BlueprintParams {
                hull: ShipClass::Battleship,
            },
            faction: Some(Faction::Earth),
            rep_required: 600,
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlueprintError {
    AlreadyKnown,
    RepLocked,
}

/// The designs the player has discovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blueprints {
    catalog: Vec<BlueprintDef>,
    known: Vec<bool>,
}

impl Default for Blueprints {
    fn default() -> Self {
        Self::new()
    }
}

impl Blueprints {
    pub fn new() -> Self {
        let catalog = blueprint_catalog();
        let known = vec![false; catalog.len()];
        Self { catalog, known }
    }

    pub fn catalog(&self) -> &[BlueprintDef] {
        &self.catalog
    }

    pub fn is_known(&self, i: usize) -> bool {
        self.known.get(i).copied().unwrap_or(false)
    }

    pub fn known_count(&self) -> usize {
        self.known.iter().filter(|k| **k).count()
    }

    /// Discover design `i` (purchase / salvage / reverse-engineer). A faction
    /// design needs enough standing with its owner (§10).
    pub fn discover(&mut self, i: usize, relations: &Relations) -> Result<(), BlueprintError> {
        if self.known[i] {
            return Err(BlueprintError::AlreadyKnown);
        }
        let def = &self.catalog[i];
        if let Some(f) = def.faction {
            if relations.standing(f) < def.rep_required {
                return Err(BlueprintError::RepLocked);
            }
        }
        self.known[i] = true;
        Ok(())
    }
}

// ---- CEO skill track (§10) ----------------------------------------------

/// The CEO's perk branch (§10): one chosen path of passive buffs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Branch {
    Industrialist,
    Trader,
    Warlord,
    Diplomat,
}

/// A domain a buff applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Buff {
    Industry,
    Trade,
    Combat,
    Diplomacy,
}

impl Branch {
    fn favours(self) -> Buff {
        match self {
            Branch::Industrialist => Buff::Industry,
            Branch::Trader => Buff::Trade,
            Branch::Warlord => Buff::Combat,
            Branch::Diplomat => Buff::Diplomacy,
        }
    }
}

/// XP needed per CEO level.
const XP_PER_LEVEL: i64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CeoError {
    BranchAlreadyChosen,
}

/// The persistent, immortal CEO (§11): a level track plus one perk branch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Ceo {
    xp: i64,
    branch: Option<Branch>,
}

impl Ceo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Earn CEO experience.
    pub fn gain_xp(&mut self, n: i64) {
        self.xp += n.max(0);
    }

    /// Level 1 at 0 XP, rising every `XP_PER_LEVEL`.
    pub fn level(&self) -> i64 {
        self.xp / XP_PER_LEVEL + 1
    }

    pub fn branch(&self) -> Option<Branch> {
        self.branch
    }

    /// Commit to a perk branch — a one-time choice (§10).
    pub fn choose_branch(&mut self, branch: Branch) -> Result<(), CeoError> {
        if self.branch.is_some() {
            return Err(CeoError::BranchAlreadyChosen);
        }
        self.branch = Some(branch);
        Ok(())
    }

    /// Passive percent buff in a domain: a little from every level, a lot more in
    /// the chosen branch's domain.
    pub fn buff(&self, kind: Buff) -> i64 {
        let base = self.level() * 2;
        let branch = match self.branch {
            Some(b) if b.favours() == kind => self.level() * 5,
            _ => 0,
        };
        base + branch
    }
}

// ---- Aggregate ----------------------------------------------------------

/// The player's advancement across the three tracks (§10).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progression {
    pub research: Research,
    pub blueprints: Blueprints,
    pub ceo: Ceo,
}

impl Progression {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_needs_prereqs_and_points() {
        let mut r = Research::new();
        assert_eq!(r.research(1), Err(ResearchError::PrereqMissing)); // needs node 0
        assert_eq!(r.research(0), Err(ResearchError::NotEnoughPoints));
        r.add_points(1_000);
        assert_eq!(r.research(0), Ok(()));
        assert!(r.is_unlocked(0));
        assert_eq!(r.research(0), Err(ResearchError::AlreadyKnown));
        assert_eq!(r.research(1), Ok(())); // prereq now met
    }

    #[test]
    fn research_bonuses_accumulate() {
        let mut r = Research::new();
        r.add_points(10_000);
        r.research(0).unwrap(); // Drive(10)
        r.research(1).unwrap(); // Drive(15)
        assert_eq!(r.drive_bonus(), 25);
        assert_eq!(r.armor_bonus(), 0);
    }

    #[test]
    fn generic_blueprints_discover_but_faction_ones_are_rep_gated() {
        let mut b = Blueprints::new();
        let mut rel = Relations::new();
        assert_eq!(b.discover(0, &rel), Ok(())); // generic
        assert!(b.is_known(0));
        assert_eq!(b.discover(0, &rel), Err(BlueprintError::AlreadyKnown));
        // The Martian cruiser (index 2) needs Mars standing >= 400.
        assert_eq!(b.discover(2, &rel), Err(BlueprintError::RepLocked));
        rel.adjust(Faction::Mars, 500);
        assert_eq!(b.discover(2, &rel), Ok(()));
        assert_eq!(b.catalog()[2].params.hull, ShipClass::Cruiser);
        assert_ne!(b.catalog()[2].seed, 0);
    }

    #[test]
    fn ceo_levels_and_branch_buffs() {
        let mut c = Ceo::new();
        assert_eq!(c.level(), 1);
        c.gain_xp(2_500);
        assert_eq!(c.level(), 3); // 2500 / 1000 + 1
        let plain_combat = c.buff(Buff::Combat);
        c.choose_branch(Branch::Warlord).unwrap();
        assert!(
            c.buff(Buff::Combat) > plain_combat,
            "the branch should boost its domain"
        );
        assert!(c.buff(Buff::Trade) < c.buff(Buff::Combat));
        assert_eq!(
            c.choose_branch(Branch::Trader),
            Err(CeoError::BranchAlreadyChosen)
        );
    }
}
