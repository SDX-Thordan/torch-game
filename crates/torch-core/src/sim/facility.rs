//! Buildable production facilities — **one kind per industrial/consumer good** (§7 re-aim).
//!
//! A facility belongs to a [`PlayerId`], sits at a body, and each tick consumes an input good
//! and produces an output good per its [`Recipe`]. Not wired to the UI this iteration — it
//! lives in the data model and is exercised by `step()` + tests.

use super::commodity::{
    Recipe, ALLOYS, BULLION, ELECTRONICS, FOOD, FUSION_FUEL, MACHINE_PARTS, SHIP_COMPONENTS, WAFERS,
};
use super::player::PlayerId;
use serde::{Deserialize, Serialize};

/// One facility kind per industrial/advanced/consumer good.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacilityKind {
    // Industrial (refine raw)
    FusionRefinery,
    AlloyPlant,
    WaferFab,
    Refinery,
    // Advanced (manufacture)
    ElectronicsFab,
    MachineShop,
    Shipworks,
    // Consumer
    Hydroponics,
}

impl FacilityKind {
    pub fn name(self) -> &'static str {
        match self {
            FacilityKind::FusionRefinery => "Fusion Refinery",
            FacilityKind::AlloyPlant => "Alloy Plant",
            FacilityKind::WaferFab => "Wafer Fab",
            FacilityKind::Refinery => "Precious Refinery",
            FacilityKind::ElectronicsFab => "Electronics Fab",
            FacilityKind::MachineShop => "Machine Shop",
            FacilityKind::Shipworks => "Shipworks",
            FacilityKind::Hydroponics => "Hydroponics",
        }
    }
    /// The good this facility produces (its catalog index).
    pub fn output(self) -> usize {
        match self {
            FacilityKind::FusionRefinery => FUSION_FUEL,
            FacilityKind::AlloyPlant => ALLOYS,
            FacilityKind::WaferFab => WAFERS,
            FacilityKind::Refinery => BULLION,
            FacilityKind::ElectronicsFab => ELECTRONICS,
            FacilityKind::MachineShop => MACHINE_PARTS,
            FacilityKind::Shipworks => SHIP_COMPONENTS,
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

/// Input store a facility accepts before it stops taking hauler deliveries (a stop condition /
/// anti-thrash) and output it holds before it stops producing.
pub const FACILITY_INPUT_CAP: i64 = 400;
pub const FACILITY_OUTPUT_CAP: i64 = 400;
/// Below this on-site input, a facility is "starved" and dispatch will route raw to it.
pub const FACILITY_LOW_WATER: i64 = 80;

/// A built facility: owner + body + kind + per-tick throughput + **on-site input/output stores**
/// (the locational inventory — it needs raw on site, or a hauler must bring it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facility {
    pub owner: PlayerId,
    pub body: usize,
    pub kind: FacilityKind,
    pub rate: i64,
    #[serde(default)]
    pub input: Vec<i64>,
    #[serde(default)]
    pub output: Vec<i64>,
}

impl Facility {
    pub fn new(owner: PlayerId, body: usize, kind: FacilityKind) -> Self {
        let n = super::commodity::commodity_count();
        Self {
            owner,
            body,
            kind,
            rate: 4,
            input: vec![0; n],
            output: vec![0; n],
        }
    }
    pub fn add_input(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.input.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn input_of(&self, c: usize) -> i64 {
        self.input.get(c).copied().unwrap_or(0)
    }
    pub fn add_output(&mut self, c: usize, qty: i64) {
        if let Some(s) = self.output.get_mut(c) {
            *s = (*s + qty).max(0);
        }
    }
    pub fn output_of(&self, c: usize) -> i64 {
        self.output.get(c).copied().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::super::commodity::{ICE, ORE};
    use super::*;

    #[test]
    fn recipes_resolve() {
        assert_eq!(FacilityKind::AlloyPlant.recipe().inputs, vec![(ORE, 2)]);
        assert_eq!(FacilityKind::FusionRefinery.recipe().inputs, vec![(ICE, 2)]);
        assert_eq!(FacilityKind::Hydroponics.recipe().inputs, vec![(ICE, 1)]);
        // The advanced tier blends several feedstocks.
        assert_eq!(FacilityKind::Shipworks.recipe().inputs.len(), 2);
    }
}
