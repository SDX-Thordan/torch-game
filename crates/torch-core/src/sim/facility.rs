//! Buildable production facilities — **one kind per industrial/consumer good** (§7 re-aim).
//!
//! A facility belongs to a [`PlayerId`], sits at a body, and each tick consumes an input good
//! and produces an output good per its [`Recipe`]. Not wired to the UI this iteration — it
//! lives in the data model and is exercised by `step()` + tests.

use super::commodity::{Recipe, ALLOYS, ELECTRONICS, FOOD, FUSION_FUEL};
use super::player::PlayerId;
use serde::{Deserialize, Serialize};

/// One facility kind per industrial/consumer good.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacilityKind {
    AlloyPlant,
    FusionRefinery,
    ElectronicsFab,
    Hydroponics,
}

impl FacilityKind {
    pub fn name(self) -> &'static str {
        match self {
            FacilityKind::AlloyPlant => "Alloy Plant",
            FacilityKind::FusionRefinery => "Fusion Refinery",
            FacilityKind::ElectronicsFab => "Electronics Fab",
            FacilityKind::Hydroponics => "Hydroponics",
        }
    }
    /// The good this facility produces (its catalog index).
    pub fn output(self) -> usize {
        match self {
            FacilityKind::AlloyPlant => ALLOYS,
            FacilityKind::FusionRefinery => FUSION_FUEL,
            FacilityKind::ElectronicsFab => ELECTRONICS,
            FacilityKind::Hydroponics => FOOD,
        }
    }
    /// The recipe driving this facility (looked up from the catalog by output good).
    pub fn recipe(self) -> Recipe {
        super::commodity::recipes()
            .into_iter()
            .find(|r| r.out == self.output())
            .expect("every facility kind has a recipe")
    }
    /// Build cost in credits.
    pub fn cost(self) -> i64 {
        20_000
    }
}

/// A built facility: owner + body + kind + per-tick throughput.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facility {
    pub owner: PlayerId,
    pub body: usize,
    pub kind: FacilityKind,
    pub rate: i64,
}

impl Facility {
    pub fn new(owner: PlayerId, body: usize, kind: FacilityKind) -> Self {
        Self {
            owner,
            body,
            kind,
            rate: 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::commodity::{ICE, ORE};
    use super::*;

    #[test]
    fn recipes_resolve() {
        assert_eq!(FacilityKind::AlloyPlant.recipe().input, ORE);
        assert_eq!(FacilityKind::FusionRefinery.recipe().input, ICE);
        assert_eq!(FacilityKind::Hydroponics.recipe().input, ICE);
    }
}
