//! Ships, weapons, fitting, and crew (§8) — data-driven hull/weapon catalogs,
//! integer fitting validation, and derived stats. This is the platform the
//! combat resolver (§35 step 7) and fleet systems consume; it stays headless and
//! deterministic (§27), with placeholder numbers as data (§31).

use super::rng::Pcg32;

/// The three weapon systems (§8a).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponKind {
    /// Rapid kinetic: anti-torpedo screen + close-band damage. The backbone.
    Pdc,
    /// Guided, slow, magazine-limited alpha; the great equalizer.
    Torpedo,
    /// High-velocity hull-killer; scarce and capital-defining (escalation axis).
    Railgun,
}

/// A fittable weapon (§8a). Combat stats are integers the resolver will use; the
/// `mass`/`power` are the fitting budget costs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponDef {
    pub name: &'static str,
    pub kind: WeaponKind,
    /// Hull damage per volley (railgun high, PDC low).
    pub damage: i64,
    /// Torpedo-stopping power (PDC only).
    pub intercept: i64,
    pub mass: i64,
    pub power: i64,
}

/// The default weapon catalog.
pub fn weapon_catalog() -> Vec<WeaponDef> {
    vec![
        WeaponDef {
            name: "PDC",
            kind: WeaponKind::Pdc,
            damage: 4,
            intercept: 20,
            mass: 30,
            power: 15,
        },
        WeaponDef {
            name: "Torpedo Tube",
            kind: WeaponKind::Torpedo,
            damage: 120,
            intercept: 0,
            mass: 60,
            power: 10,
        },
        WeaponDef {
            name: "Railgun",
            kind: WeaponKind::Railgun,
            damage: 300,
            intercept: 0,
            mass: 200,
            power: 80,
        },
    ]
}

/// Pick a weapon of a kind from the default catalog.
pub fn weapon(kind: WeaponKind) -> WeaponDef {
    weapon_catalog()
        .into_iter()
        .find(|w| w.kind == kind)
        .expect("catalog has every kind")
}

/// Ship classes (§8b military, §8d Q-ship, §8e civilian).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ShipClass {
    Frigate,
    Destroyer,
    Cruiser,
    Battleship,
    QShip,
    Freighter,
    Miner,
    Tanker,
}

impl ShipClass {
    /// Whether this is a true warship (§8b) — the precious, crew-heavy core.
    pub fn is_warship(self) -> bool {
        matches!(
            self,
            Self::Frigate | Self::Destroyer | Self::Cruiser | Self::Battleship
        )
    }
}

/// A hull: its mobility/armor envelope, drive, power budget, and weapon mounts.
/// Railgun mounts are the escalation axis (§8b): 0 → 1 → 1 → 2 up the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HullDef {
    pub name: &'static str,
    pub class: ShipClass,
    pub dry_mass: i64,
    pub armor: i64,
    pub max_thrust: i64,
    pub remass_capacity: i64,
    /// Exhaust-velocity proxy for the (simplified) delta-v model (§6).
    pub drive_efficiency: i64,
    pub power_capacity: i64,
    pub pdc_mounts: u32,
    pub torpedo_mounts: u32,
    pub railgun_mounts: u32,
    pub utility_mounts: u32,
    /// Trained crew the hull needs to operate — the §8c bottleneck.
    pub crew_required: i64,
}

/// The default hull catalog: the four warships (§8b), the Q-ship (§8d), and the
/// core civilian classes (§8e).
pub fn hull_catalog() -> Vec<HullDef> {
    vec![
        HullDef {
            name: "Frigate",
            class: ShipClass::Frigate,
            dry_mass: 800,
            armor: 40,
            max_thrust: 900,
            remass_capacity: 600,
            drive_efficiency: 90,
            power_capacity: 140,
            pdc_mounts: 2,
            torpedo_mounts: 2,
            railgun_mounts: 0,
            utility_mounts: 1,
            crew_required: 12,
        },
        HullDef {
            name: "Destroyer",
            class: ShipClass::Destroyer,
            dry_mass: 1_800,
            armor: 90,
            max_thrust: 1_500,
            remass_capacity: 1_100,
            drive_efficiency: 85,
            power_capacity: 320,
            pdc_mounts: 3,
            torpedo_mounts: 4,
            railgun_mounts: 1,
            utility_mounts: 2,
            crew_required: 32,
        },
        HullDef {
            name: "Cruiser",
            class: ShipClass::Cruiser,
            dry_mass: 3_200,
            armor: 160,
            max_thrust: 2_100,
            remass_capacity: 1_700,
            drive_efficiency: 80,
            power_capacity: 360,
            pdc_mounts: 4,
            torpedo_mounts: 2,
            railgun_mounts: 1,
            utility_mounts: 3,
            crew_required: 60,
        },
        HullDef {
            name: "Battleship",
            class: ShipClass::Battleship,
            dry_mass: 6_400,
            armor: 320,
            max_thrust: 3_400,
            remass_capacity: 2_800,
            drive_efficiency: 75,
            power_capacity: 520,
            pdc_mounts: 6,
            torpedo_mounts: 4,
            railgun_mounts: 2,
            utility_mounts: 4,
            crew_required: 120,
        },
        HullDef {
            name: "Q-ship",
            class: ShipClass::QShip,
            dry_mass: 1_400,
            armor: 50,
            max_thrust: 1_000,
            remass_capacity: 1_400,
            drive_efficiency: 78,
            power_capacity: 120,
            pdc_mounts: 2,
            torpedo_mounts: 1,
            railgun_mounts: 0,
            utility_mounts: 2,
            crew_required: 14,
        },
        HullDef {
            name: "Freighter",
            class: ShipClass::Freighter,
            dry_mass: 2_600,
            armor: 30,
            max_thrust: 1_100,
            remass_capacity: 2_200,
            drive_efficiency: 82,
            power_capacity: 90,
            pdc_mounts: 0,
            torpedo_mounts: 0,
            railgun_mounts: 0,
            utility_mounts: 3,
            crew_required: 8,
        },
        HullDef {
            name: "Miner",
            class: ShipClass::Miner,
            dry_mass: 1_900,
            armor: 25,
            max_thrust: 800,
            remass_capacity: 1_300,
            drive_efficiency: 80,
            power_capacity: 110,
            pdc_mounts: 0,
            torpedo_mounts: 0,
            railgun_mounts: 0,
            utility_mounts: 4,
            crew_required: 10,
        },
        HullDef {
            name: "Tanker",
            class: ShipClass::Tanker,
            dry_mass: 2_200,
            armor: 28,
            max_thrust: 950,
            remass_capacity: 4_000,
            drive_efficiency: 84,
            power_capacity: 80,
            pdc_mounts: 0,
            torpedo_mounts: 0,
            railgun_mounts: 0,
            utility_mounts: 1,
            crew_required: 9,
        },
    ]
}

/// Find a hull by class in the default catalog.
pub fn hull(class: ShipClass) -> HullDef {
    hull_catalog()
        .into_iter()
        .find(|h| h.class == class)
        .expect("catalog has every class")
}

/// Procedural-name tables for captains (§11).
const FIRST_NAMES: [&str; 8] = ["Ana", "Bso", "Cira", "Dao", "Esi", "Fen", "Goro", "Hale"];
const LAST_NAMES: [&str; 8] = [
    "Vega", "Okonkwo", "Reyes", "Tan", "Mwangi", "Sato", "Cole", "Ndiaye",
];

/// Evocative call-signs a christened hull draws from (§14 ship naming). A pool of
/// original names so a few hulls become *beloved hero ships* — the Rocinante
/// effect — while the procedural fleet stays wallpaper (§25).
const SHIP_NAMES: [&str; 16] = [
    "Lodestar",
    "Ironwake",
    "Halcyon",
    "Kestrel",
    "Saltire",
    "Cormorant",
    "Mistral",
    "Perdido",
    "Vigil",
    "Banshee",
    "Tessellate",
    "Grawl",
    "Marrow",
    "Quiet Riot",
    "Long Haul",
    "Last Word",
];

/// A deterministically chosen call-sign for a new hull (§14, §27).
pub fn christen_ship(rng: &mut Pcg32) -> &'static str {
    SHIP_NAMES[rng.below(SHIP_NAMES.len() as u32) as usize]
}

/// The crew of a ship (§8c): a named captain plus the wider crew as an abstract
/// quality rating that improves with experience.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crew {
    pub captain: String,
    pub count: i64,
    /// Abstract quality rating, 0..=100.
    pub quality: i64,
    pub experience: i64,
}

/// Experience needed per quality point gained.
const XP_PER_QUALITY: i64 = 100;

impl Crew {
    /// Recruit a crew with a deterministically generated captain name (§11, §27).
    pub fn recruit(rng: &mut Pcg32, count: i64, quality: i64) -> Self {
        let first = FIRST_NAMES[rng.below(FIRST_NAMES.len() as u32) as usize];
        let last = LAST_NAMES[rng.below(LAST_NAMES.len() as u32) as usize];
        Self {
            captain: format!("{first} {last}"),
            count,
            quality: quality.clamp(0, 100),
            experience: 0,
        }
    }

    /// Blood the crew: experience accrues and slowly lifts quality (capped at 100).
    pub fn gain_experience(&mut self, xp: i64) {
        self.experience += xp.max(0);
        while self.experience >= XP_PER_QUALITY && self.quality < 100 {
            self.experience -= XP_PER_QUALITY;
            self.quality += 1;
        }
    }
}

/// Derived combat/mobility stats of a fitted ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShipStats {
    pub total_mass: i64,
    /// Simplified delta-v proxy (§6): efficiency × remass ÷ total mass.
    pub delta_v: i64,
    /// Mobility: thrust per unit mass (×1000).
    pub thrust_to_mass: i64,
    /// Raw alpha throughput (sum of weapon damage).
    pub raw_alpha: i64,
    /// Torpedo-stopping screen (sum of PDC intercept).
    pub pdc_screen: i64,
    /// Railgun count — the §8b escalation axis.
    pub railguns: u32,
    /// Crew quality of the fitted ship (0..=100).
    pub crew_quality: i64,
}

impl ShipStats {
    /// Alpha as actually delivered, scaled by crew quality (50 ⇒ ×1.0, the
    /// trained-crew payoff of §8c): crews make the guns matter.
    pub fn effective_alpha(&self) -> i64 {
        self.raw_alpha * (50 + self.crew_quality) / 100
    }
}

/// Why a loadout failed to fit (§8 functional slots + budgets).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitError {
    TooManyPdc,
    TooManyTorpedo,
    TooManyRailgun,
    OverPower,
    OverRemass,
    Undercrewed,
}

/// A validated ship: a hull with weapons fitted to its slots, a remass load, and
/// a crew. Build with [`Loadout::fit`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loadout {
    hull: HullDef,
    weapons: Vec<WeaponDef>,
    remass_load: i64,
    crew: Crew,
}

impl Loadout {
    /// Fit `weapons` onto `hull` with `remass_load` and `crew`, validating slot
    /// counts, the power budget, tankage, and the crew minimum (§8).
    pub fn fit(
        hull: HullDef,
        weapons: Vec<WeaponDef>,
        remass_load: i64,
        crew: Crew,
    ) -> Result<Self, FitError> {
        let count = |k: WeaponKind| weapons.iter().filter(|w| w.kind == k).count() as u32;
        if count(WeaponKind::Pdc) > hull.pdc_mounts {
            return Err(FitError::TooManyPdc);
        }
        if count(WeaponKind::Torpedo) > hull.torpedo_mounts {
            return Err(FitError::TooManyTorpedo);
        }
        if count(WeaponKind::Railgun) > hull.railgun_mounts {
            return Err(FitError::TooManyRailgun);
        }
        let power: i64 = weapons.iter().map(|w| w.power).sum();
        if power > hull.power_capacity {
            return Err(FitError::OverPower);
        }
        if remass_load < 0 || remass_load > hull.remass_capacity {
            return Err(FitError::OverRemass);
        }
        if crew.count < hull.crew_required {
            return Err(FitError::Undercrewed);
        }
        Ok(Self {
            hull,
            weapons,
            remass_load,
            crew,
        })
    }

    pub fn hull(&self) -> &HullDef {
        &self.hull
    }

    pub fn crew(&self) -> &Crew {
        &self.crew
    }

    /// The fitted weapons (read by the combat resolver, §35 step 7).
    pub fn weapons(&self) -> &[WeaponDef] {
        &self.weapons
    }

    /// Compute the derived stats of this loadout.
    pub fn stats(&self) -> ShipStats {
        let weapon_mass: i64 = self.weapons.iter().map(|w| w.mass).sum();
        let total_mass = self.hull.dry_mass + weapon_mass + self.remass_load;
        let delta_v = self.hull.drive_efficiency * self.remass_load / total_mass;
        let thrust_to_mass = self.hull.max_thrust * 1_000 / total_mass;
        let raw_alpha = self.weapons.iter().map(|w| w.damage).sum();
        let pdc_screen = self
            .weapons
            .iter()
            .filter(|w| w.kind == WeaponKind::Pdc)
            .map(|w| w.intercept)
            .sum();
        let railguns = self
            .weapons
            .iter()
            .filter(|w| w.kind == WeaponKind::Railgun)
            .count() as u32;
        ShipStats {
            total_mass,
            delta_v,
            thrust_to_mass,
            raw_alpha,
            pdc_screen,
            railguns,
            crew_quality: self.crew.quality,
        }
    }
}

/// A sensible reference fit for a class: every weapon mount filled, full tanks,
/// a green crew. Used for catalog comparisons and the shipyard demo.
pub fn reference_loadout(class: ShipClass, rng: &mut Pcg32) -> Loadout {
    reference_loadout_quality(class, 50, rng)
}

/// A reference fit crewed at `quality` (0..=100) — lets callers field veteran
/// hulls or low-quality "rabble" (e.g. raider packs) off the same template (§8c).
pub fn reference_loadout_quality(class: ShipClass, quality: i64, rng: &mut Pcg32) -> Loadout {
    let h = hull(class);
    let mut weapons = Vec::new();
    for _ in 0..h.pdc_mounts {
        weapons.push(weapon(WeaponKind::Pdc));
    }
    for _ in 0..h.torpedo_mounts {
        weapons.push(weapon(WeaponKind::Torpedo));
    }
    for _ in 0..h.railgun_mounts {
        weapons.push(weapon(WeaponKind::Railgun));
    }
    let crew = Crew::recruit(rng, h.crew_required, quality);
    let remass = h.remass_capacity;
    Loadout::fit(h, weapons, remass, crew).expect("reference loadout must fit")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn railgun_count_is_the_escalation_axis() {
        assert_eq!(hull(ShipClass::Frigate).railgun_mounts, 0);
        assert_eq!(hull(ShipClass::Destroyer).railgun_mounts, 1);
        assert_eq!(hull(ShipClass::Cruiser).railgun_mounts, 1);
        assert_eq!(hull(ShipClass::Battleship).railgun_mounts, 2);
    }

    #[test]
    fn every_reference_loadout_fits() {
        let mut rng = Pcg32::new(1);
        for h in hull_catalog() {
            let _ = reference_loadout(h.class, &mut rng); // panics if it doesn't fit
        }
    }

    #[test]
    fn christening_is_deterministic_and_from_the_pool() {
        let (mut a, mut b) = (Pcg32::new(9), Pcg32::new(9));
        for _ in 0..10 {
            let name = christen_ship(&mut a);
            assert_eq!(name, christen_ship(&mut b)); // same seed ⇒ same name (§27)
            assert!(SHIP_NAMES.contains(&name));
        }
    }

    #[test]
    fn over_fitting_slots_is_rejected() {
        let mut rng = Pcg32::new(2);
        let frigate = hull(ShipClass::Frigate); // 0 railgun mounts
        let crew = Crew::recruit(&mut rng, frigate.crew_required, 50);
        let weapons = vec![weapon(WeaponKind::Railgun)];
        assert_eq!(
            Loadout::fit(frigate, weapons, 100, crew),
            Err(FitError::TooManyRailgun)
        );
    }

    #[test]
    fn undercrewed_and_overtanked_are_rejected() {
        let mut rng = Pcg32::new(3);
        let h = hull(ShipClass::Cruiser);
        let thin = Crew::recruit(&mut rng, h.crew_required - 1, 50);
        assert_eq!(
            Loadout::fit(h.clone(), vec![], 0, thin),
            Err(FitError::Undercrewed)
        );
        let crew = Crew::recruit(&mut rng, h.crew_required, 50);
        assert_eq!(
            Loadout::fit(h.clone(), vec![], h.remass_capacity + 1, crew),
            Err(FitError::OverRemass)
        );
    }

    #[test]
    fn capitals_out_hit_escorts_but_escorts_out_maneuver() {
        let mut rng = Pcg32::new(4);
        let frigate = reference_loadout(ShipClass::Frigate, &mut rng).stats();
        let battleship = reference_loadout(ShipClass::Battleship, &mut rng).stats();
        assert!(
            battleship.raw_alpha > frigate.raw_alpha,
            "capital should hit harder"
        );
        assert!(
            battleship.railguns > frigate.railguns,
            "capital defines the railgun axis"
        );
        assert!(
            frigate.thrust_to_mass > battleship.thrust_to_mass,
            "escort should be nimbler"
        );
    }

    #[test]
    fn more_remass_buys_more_delta_v() {
        let mut rng = Pcg32::new(5);
        let h = hull(ShipClass::Freighter);
        let light = Loadout::fit(
            h.clone(),
            vec![],
            h.remass_capacity / 4,
            Crew::recruit(&mut rng, h.crew_required, 50),
        )
        .unwrap();
        let full = Loadout::fit(
            h.clone(),
            vec![],
            h.remass_capacity,
            Crew::recruit(&mut rng, h.crew_required, 50),
        )
        .unwrap();
        assert!(full.stats().delta_v > light.stats().delta_v);
    }

    #[test]
    fn crew_quality_lifts_effective_alpha() {
        let mut rng = Pcg32::new(6);
        let h = hull(ShipClass::Destroyer);
        let weapons = vec![weapon(WeaponKind::Torpedo)];
        let green = Loadout::fit(
            h.clone(),
            weapons.clone(),
            500,
            Crew::recruit(&mut rng, h.crew_required, 20),
        )
        .unwrap();
        let veteran = Loadout::fit(
            h.clone(),
            weapons,
            500,
            Crew::recruit(&mut rng, h.crew_required, 90),
        )
        .unwrap();
        assert!(veteran.stats().effective_alpha() > green.stats().effective_alpha());
    }

    #[test]
    fn crews_grow_with_experience() {
        let mut rng = Pcg32::new(7);
        let mut crew = Crew::recruit(&mut rng, 30, 40);
        assert!(!crew.captain.is_empty());
        crew.gain_experience(350); // +3 quality, 50 xp banked
        assert_eq!(crew.quality, 43);
        assert_eq!(crew.experience, 50);
    }

    #[test]
    fn recruitment_is_deterministic() {
        let a = Crew::recruit(&mut Pcg32::new(99), 10, 50);
        let b = Crew::recruit(&mut Pcg32::new(99), 10, 50);
        assert_eq!(a.captain, b.captain);
    }
}
