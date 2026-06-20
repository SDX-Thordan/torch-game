//! Ships as **owned, sectioned hulls** (§8 re-aim) — the data model + a 3D framework hook.
//!
//! Every ship belongs to a [`PlayerId`]. A ship is built from **sections** (2 for civilian
//! hulls, 3 for military), each developable and carrying **slots** — and weapon-slots and the
//! drive are **individually targetable subsystem entities** (so a railgun could later snipe a
//! drive). This iteration ships the model + the per-section structure only; **there is no
//! combat resolver yet**.

use super::player::PlayerId;
use serde::{Deserialize, Serialize};

/// Ship hull classes. Civilians carry 2 sections; military carry 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShipClass {
    Hauler,
    Miner,
    Combat,
}

impl ShipClass {
    pub fn name(self) -> &'static str {
        match self {
            ShipClass::Hauler => "Hauler",
            ShipClass::Miner => "Miner",
            ShipClass::Combat => "Combat Vessel",
        }
    }
    pub fn is_military(self) -> bool {
        matches!(self, ShipClass::Combat)
    }
    pub fn section_count(self) -> usize {
        if self.is_military() {
            3
        } else {
            2
        }
    }
    /// Build cost: `(credits, alloys, electronics)`. Ships are fuelled separately (Fusion Fuel).
    pub fn cost(self) -> (i64, i64, i64) {
        match self {
            ShipClass::Hauler => (12_000, 20, 5),
            ShipClass::Miner => (9_000, 15, 4),
            ShipClass::Combat => (40_000, 60, 30),
        }
    }
    /// Fusion-fuel tank capacity.
    pub fn fuel_capacity(self) -> i64 {
        match self {
            ShipClass::Combat => 400,
            _ => 250,
        }
    }
}

/// A ship section — a developable structural block with its own armor/thrust/power and slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SectionKind {
    /// Crew + control (always present).
    Command,
    /// Houses the drive subsystem(s) — targetable to immobilize.
    Drive,
    /// Cargo / mining / weapons payload depending on the hull.
    Payload,
    /// Military weapons block (combat hulls only) — houses targetable weapon slots.
    Weapons,
}

/// What a slot can hold; weapon + drive slots are the targetable subsystems.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    Weapon,
    Drive,
    Utility,
}

/// One fittable slot — an **individually targetable subsystem entity** with its own HP.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subsystem {
    pub slot: SlotKind,
    pub hp: i64,
    pub max_hp: i64,
    /// Index into a code-defined fitting catalog; `None` = empty slot (framework only for now).
    pub fitted: Option<u16>,
}

impl Subsystem {
    fn new(slot: SlotKind, hp: i64) -> Self {
        Self {
            slot,
            hp,
            max_hp: hp,
            fitted: None,
        }
    }
}

/// A developable section with its subsystem slots and per-section stats.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub kind: SectionKind,
    pub hp: i64,
    pub max_hp: i64,
    pub armor: i64,
    pub thrust: i64,
    pub power: i64,
    pub subsystems: Vec<Subsystem>,
}

/// An owned ship: an `owner`, a class, a name, and its sections + a fuel tank.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ship {
    pub owner: PlayerId,
    pub class: ShipClass,
    pub name: String,
    pub sections: Vec<Section>,
    /// Stocked Fusion Fuel (sourced from Ice).
    pub fuel: i64,
    /// The body the ship is docked at (movement is out of scope this iteration).
    pub body: usize,
}

impl Ship {
    pub fn new(owner: PlayerId, class: ShipClass, name: &str, body: usize) -> Self {
        Self {
            owner,
            class,
            name: name.to_string(),
            sections: default_sections(class),
            fuel: class.fuel_capacity(),
            body,
        }
    }
}

/// The code-defined default section layout per class (the "content is code" catalog). One
/// placeholder section per `SectionKind` for the class; refined per-section stats later.
pub fn default_sections(class: ShipClass) -> Vec<Section> {
    let cmd = Section {
        kind: SectionKind::Command,
        hp: 100,
        max_hp: 100,
        armor: 10,
        thrust: 0,
        power: 40,
        subsystems: vec![Subsystem::new(SlotKind::Utility, 30)],
    };
    let drive = Section {
        kind: SectionKind::Drive,
        hp: 120,
        max_hp: 120,
        armor: 12,
        thrust: 100,
        power: 0,
        subsystems: vec![Subsystem::new(SlotKind::Drive, 40)],
    };
    match class {
        ShipClass::Hauler | ShipClass::Miner => {
            let payload = Section {
                kind: SectionKind::Payload,
                hp: 90,
                max_hp: 90,
                armor: 8,
                thrust: 0,
                power: 20,
                subsystems: vec![Subsystem::new(SlotKind::Utility, 30)],
            };
            vec![cmd, drive, payload]
                .into_iter()
                .take(class.section_count())
                .collect()
        }
        ShipClass::Combat => {
            let weapons = Section {
                kind: SectionKind::Weapons,
                hp: 110,
                max_hp: 110,
                armor: 14,
                thrust: 0,
                power: 30,
                subsystems: vec![
                    Subsystem::new(SlotKind::Weapon, 25),
                    Subsystem::new(SlotKind::Weapon, 25),
                ],
            };
            vec![cmd, drive, weapons]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_counts_match_the_class() {
        assert_eq!(Ship::new(0, ShipClass::Hauler, "H", 5).sections.len(), 2);
        assert_eq!(Ship::new(0, ShipClass::Miner, "M", 5).sections.len(), 2);
        let combat = Ship::new(0, ShipClass::Combat, "C", 5);
        assert_eq!(combat.sections.len(), 3);
        // A combat hull has a targetable Drive subsystem and Weapon subsystems.
        assert!(combat
            .sections
            .iter()
            .any(|s| s.subsystems.iter().any(|ss| ss.slot == SlotKind::Drive)));
        assert!(combat.sections.iter().any(|s| s
            .subsystems
            .iter()
            .filter(|ss| ss.slot == SlotKind::Weapon)
            .count()
            == 2));
    }

    #[test]
    fn military_costs_more_than_civilian() {
        assert!(ShipClass::Combat.cost().0 > ShipClass::Hauler.cost().0);
    }
}
