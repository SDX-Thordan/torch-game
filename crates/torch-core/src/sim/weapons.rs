//! Tiered weapon models + the player's arsenal (Phase B / §8a). Weapons aren't a flat
//! three-kind catalog any more: each kind has a ladder of **named models** running from
//! basic to advanced — lower-listed models are stronger but dearer and harder to get
//! (and, for railguns, **less accurate** — raw power traded for hit quality).
//!
//! You can't simply buy the best: advanced/faction designs are **crafted from scrap**
//! recovered in combat, and building a great power's design **antagonizes** that power
//! (an independent arming up). The basic (tier-0) model of each kind has stats identical
//! to the old generic weapon, so a fresh, un-crafted fleet is byte-identical (§27).

use super::faction::Faction;
use super::ships::{WeaponDef, WeaponKind};

/// Where a weapon design comes from — sets the crafting gate / who it antagonises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponOrigin {
    /// Open/scrapyard design — craftable freely.
    Common,
    /// Pirate/scavenged — craftable from scrap, no great power cares.
    Pirate,
    /// A great power's design — crafting it sours that power (copying their tech).
    Faction(Faction),
}

impl WeaponOrigin {
    /// The great power a craft of this design antagonises, if any.
    pub fn antagonist(self) -> Option<Faction> {
        match self {
            WeaponOrigin::Faction(f) => Some(f),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            WeaponOrigin::Common => "Open",
            WeaponOrigin::Pirate => "Pirate",
            WeaponOrigin::Faction(f) => f.name(),
        }
    }
}

/// A named weapon model on the tier ladder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponModel {
    pub id: usize,
    pub name: &'static str,
    pub kind: WeaponKind,
    /// Tier within the kind: 0 basic … higher = stronger/dearer/rarer.
    pub tier: u8,
    /// Raw per-volley hull damage (or, for PDC, the close-band damage).
    pub damage: i64,
    /// Torpedo-stopping screen (PDC only).
    pub intercept: i64,
    /// Hit quality in basis points (10000 = perfect). Railguns lose accuracy up the
    /// ladder — more raw power, but it lands less reliably. 10000 for PDC/torpedo.
    pub accuracy_bp: i64,
    /// A railgun on a rotating turret (vs a fixed/spinal mount).
    pub turreted: bool,
    pub origin: WeaponOrigin,
    pub mass: i64,
    pub power: i64,
    /// Crafting cost: scrap parts + credits. Tier 0 is owned from the start (cost 0).
    pub scrap_cost: i64,
    pub credit_cost: i64,
}

impl WeaponModel {
    /// The fittable [`WeaponDef`] this model becomes — railgun damage is scaled by its
    /// accuracy, so a stronger-but-sloppier gun nets less than its raw number suggests.
    pub fn to_def(&self) -> WeaponDef {
        WeaponDef {
            name: self.name,
            kind: self.kind,
            damage: self.damage * self.accuracy_bp / 10_000,
            intercept: self.intercept,
            mass: self.mass,
            power: self.power,
        }
    }
}

fn m(
    id: usize,
    name: &'static str,
    kind: WeaponKind,
    tier: u8,
    origin: WeaponOrigin,
    turreted: bool,
) -> WeaponModel {
    let t = tier as i64;
    let (damage, intercept, accuracy_bp, mass, power) = match kind {
        WeaponKind::Pdc => (4 + t, 20 + t * 4, 10_000, 30 + t * 3, 15 + t * 2),
        WeaponKind::Torpedo => (120 + t * 40, 0, 10_000, 60 + t * 6, 10 + t * 2),
        // Railguns: damage climbs, accuracy falls (raw power vs hit quality).
        WeaponKind::Railgun => (300 + t * 60, 0, 10_000 - t * 450, 200 + t * 18, 80 + t * 9),
    };
    let (scrap_cost, credit_cost) = match kind {
        WeaponKind::Pdc => (t * 6, t * 900),
        WeaponKind::Torpedo => (t * 8, t * 1_300),
        WeaponKind::Railgun => (t * 12, t * 1_400),
    };
    WeaponModel {
        id,
        name,
        kind,
        tier,
        damage,
        intercept,
        accuracy_bp,
        turreted,
        origin,
        mass,
        power,
        scrap_cost,
        credit_cost,
    }
}

/// The full named weapon catalog (player-supplied tiers; list order = power ladder).
pub fn weapon_models() -> Vec<WeaponModel> {
    use Faction::*;
    use WeaponKind::*;
    use WeaponOrigin::{Common, Faction as Fac, Pirate};
    vec![
        // ---- PDCs (anti-torpedo screen + close brawl) -----------------------
        m(0, "Hashari Flak", Pdc, 0, Common, false),
        m(1, "Fragmata PDC", Pdc, 1, Pirate, false),
        m(2, "Model 17 PDC", Pdc, 2, Fac(Earth), false),
        m(3, "Nariman PDC", Pdc, 3, Fac(Mars), false),
        m(4, "OPA PDC", Pdc, 4, Fac(Belt), false),
        m(5, "Cronian PDC", Pdc, 5, Pirate, false),
        m(6, "Redfield PDC", Pdc, 6, Fac(Earth), false),
        m(7, "Maegnus PDC", Pdc, 7, Fac(Mars), false),
        // ---- Torpedoes (saturating alpha) -----------------------------------
        m(8, "Ramshackle Torpedo", Torpedo, 0, Common, false),
        m(9, "Improvised 160mm", Torpedo, 1, Pirate, false),
        m(10, "180mm Torpedo", Torpedo, 2, Fac(Earth), false),
        m(11, "190mm Torpedo", Torpedo, 3, Fac(Mars), false),
        m(12, "Stealthed 170mm", Torpedo, 4, Fac(Mars), false),
        // ---- Railguns (hull-killers; fixed/spinal vs turreted) --------------
        m(13, "Ramshackle Coilgun", Railgun, 0, Common, false),
        m(14, "UN-38 Railgun", Railgun, 1, Fac(Earth), false),
        m(15, "Tachi Railgun", Railgun, 2, Fac(Mars), false),
        m(16, "Stiletto Light Railgun", Railgun, 3, Fac(Mars), true),
        m(17, "Dawson Medium Railgun", Railgun, 4, Fac(Earth), true),
        m(18, "Ashford Medium Railgun", Railgun, 5, Fac(Belt), true),
        m(19, "Foehammer Heavy Railgun", Railgun, 6, Fac(Mars), true),
        m(
            20,
            "Farren Pattern Heavy Railgun",
            Railgun,
            7,
            Fac(Earth),
            true,
        ),
    ]
}

/// The basic (tier-0) model id for each kind — what a fresh fleet fits.
pub const BASIC_PDC: usize = 0;
pub const BASIC_TORPEDO: usize = 8;
pub const BASIC_RAILGUN: usize = 13;

/// Look up a model by id.
pub fn model(id: usize) -> Option<WeaponModel> {
    weapon_models().into_iter().find(|w| w.id == id)
}
