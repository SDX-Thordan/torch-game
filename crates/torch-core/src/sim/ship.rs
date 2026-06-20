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
    fn fitted(slot: SlotKind, hp: i64, fitting: u16) -> Self {
        Self {
            slot,
            hp,
            max_hp: hp,
            fitted: Some(fitting),
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

/// An owned ship: an `owner`, a class, a name, its sections + a fuel tank, a cargo hold, and a
/// movement state. `body` is the dock when `dest is None`, and the **origin while in flight**.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ship {
    pub owner: PlayerId,
    pub class: ShipClass,
    pub name: String,
    pub sections: Vec<Section>,
    /// Stocked Fusion Fuel (sourced from Ice).
    pub fuel: i64,
    /// Dock body when idle; origin body while in flight (`dest.is_some()`).
    pub body: usize,
    /// Per-good payload, sized by `commodity::commodity_count()`.
    #[serde(default)]
    pub cargo: Vec<i64>,
    /// Destination body while in flight; `None` when docked/idle.
    #[serde(default)]
    pub dest: Option<usize>,
    /// Tick the current flight began.
    #[serde(default)]
    pub departed: u64,
    /// Tick the current flight docks at `dest`.
    #[serde(default)]
    pub arrival: u64,
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
            cargo: vec![0; super::commodity::commodity_count()],
            dest: None,
            departed: 0,
            arrival: 0,
        }
    }

    /// Whether the ship is mid-flight (between bodies).
    pub fn in_flight(&self) -> bool {
        self.dest.is_some()
    }

    /// Add `qty` of good `c` to the cargo hold (clamped ≥0).
    pub fn add_cargo(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.cargo.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }

    pub fn cargo_of(&self, c: usize) -> i64 {
        self.cargo.get(c).copied().unwrap_or(0)
    }

    /// Total units in the hold.
    pub fn cargo_total(&self) -> i64 {
        self.cargo.iter().sum()
    }
}

// Fitting-catalog indices (into `fittings()`), for setting `Subsystem.fitted`.
pub const CARGO_POD: u16 = 0;
pub const DRIVE_UNIT: u16 = 1;
pub const FUEL_TANK: u16 = 2;
pub const MINING_RIG: u16 = 3;

/// A fittable module placed in a slot — the loadout that gives a section its capability. The
/// derived ship stats ([`ship_stats`]) sum these across all fitted slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FittingDef {
    pub name: &'static str,
    pub slot: SlotKind,
    pub cargo: i64,
    pub drive: i64,
    pub fuel: i64,
}

/// The fitting catalog (content-in-code, extensible). `Subsystem.fitted` indexes this.
pub fn fittings() -> Vec<FittingDef> {
    vec![
        FittingDef {
            name: "Cargo Pod",
            slot: SlotKind::Utility,
            cargo: 200,
            drive: 0,
            fuel: 0,
        },
        FittingDef {
            name: "Drive Unit",
            slot: SlotKind::Drive,
            cargo: 0,
            drive: 80,
            fuel: 0,
        },
        FittingDef {
            name: "Fuel Tank",
            slot: SlotKind::Utility,
            cargo: 0,
            drive: 0,
            fuel: 150,
        },
        FittingDef {
            name: "Mining Rig",
            slot: SlotKind::Utility,
            cargo: 50,
            drive: 0,
            fuel: 0,
        },
    ]
}

/// Derived ship stats, computed from the sections + their fitted loadout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShipStats {
    pub cargo_capacity: i64,
    /// Distance units traversed per tick (floored at 1 so travel time never divides by zero).
    pub speed: i64,
    pub fuel_capacity: i64,
}

/// Compute a ship's capabilities from its sections (thrust) + fitted modules (cargo/drive/fuel).
pub fn ship_stats(ship: &Ship) -> ShipStats {
    let base_speed: i64 = ship.sections.iter().map(|s| s.thrust).sum();
    let mut cargo = 0;
    let mut speed = base_speed;
    let mut fuel = ship.class.fuel_capacity();
    let cat = fittings();
    for s in &ship.sections {
        for ss in &s.subsystems {
            if let Some(f) = ss.fitted {
                if let Some(d) = cat.get(f as usize) {
                    cargo += d.cargo;
                    speed += d.drive;
                    fuel += d.fuel;
                }
            }
        }
    }
    ShipStats {
        cargo_capacity: cargo.max(0),
        speed: speed.max(1),
        fuel_capacity: fuel,
    }
}

/// The code-defined default section layout per class (the "content is code" catalog) with a
/// default loadout fitted. Civilians carry 2 sections (Drive + Payload); combat carries 3
/// (Command + Drive + Weapons). The Drive section's drive slot + the Payload's utility slot
/// come pre-fitted, so a fresh hauler actually has cargo + speed.
pub fn default_sections(class: ShipClass) -> Vec<Section> {
    let drive = Section {
        kind: SectionKind::Drive,
        hp: 120,
        max_hp: 120,
        armor: 12,
        thrust: 100,
        power: 0,
        subsystems: vec![Subsystem::fitted(SlotKind::Drive, 40, DRIVE_UNIT)],
    };
    match class {
        ShipClass::Hauler => vec![
            drive,
            Section {
                kind: SectionKind::Payload,
                hp: 90,
                max_hp: 90,
                armor: 8,
                thrust: 0,
                power: 20,
                subsystems: vec![Subsystem::fitted(SlotKind::Utility, 30, CARGO_POD)],
            },
        ],
        ShipClass::Miner => vec![
            drive,
            Section {
                kind: SectionKind::Payload,
                hp: 90,
                max_hp: 90,
                armor: 8,
                thrust: 0,
                power: 20,
                subsystems: vec![Subsystem::fitted(SlotKind::Utility, 30, MINING_RIG)],
            },
        ],
        ShipClass::Combat => vec![
            Section {
                kind: SectionKind::Command,
                hp: 100,
                max_hp: 100,
                armor: 10,
                thrust: 0,
                power: 40,
                subsystems: vec![Subsystem::new(SlotKind::Utility, 30)],
            },
            drive,
            Section {
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
            },
        ],
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

    #[test]
    fn a_default_hauler_has_cargo_and_above_base_speed_from_its_loadout() {
        let h = Ship::new(0, ShipClass::Hauler, "H", 5);
        let st = ship_stats(&h);
        assert!(st.cargo_capacity >= 200, "the Cargo Pod gives it a hold");
        let base: i64 = h.sections.iter().map(|s| s.thrust).sum();
        assert!(
            st.speed > base,
            "the Drive Unit adds speed over the bare thrust"
        );
        assert!(st.speed >= 1);
        // A fresh ship starts docked with a correctly-sized empty hold.
        assert!(!h.in_flight());
        assert_eq!(h.cargo.len(), super::super::commodity::commodity_count());
        assert_eq!(h.cargo_total(), 0);
        // A Miner's payload is a Mining Rig (less cargo than a hauler's pod).
        let m = Ship::new(0, ShipClass::Miner, "M", 5);
        assert!(ship_stats(&m).cargo_capacity < st.cargo_capacity);
    }
}
