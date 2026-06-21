//! The goods catalog (§31) — **data-driven and extensible by design**.
//!
//! The streamlined economy: basic resources (mined), industrial goods (refined from a
//! basic resource), and consumer goods. The catalog is a table, the count is
//! [`commodity_count()`] (never a hardcoded constant), and every per-good quantity in the
//! sim is a `Vec<i64>` sized by that count — so **adding a new good or raw good later is just
//! appending a row** here (+ an optional [`Recipe`]), with no signature churn elsewhere.

/// A good's tier in the production chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoodTier {
    /// Mined directly from bodies (Ice / Ore / Silicon / Gold / Silver / Platinum).
    Raw,
    /// Refined from raw goods in a facility (Fusion Fuel / Alloys / Silicon Wafers / Bullion).
    Industrial,
    /// Manufactured from refined goods (Electronics / Machine Parts / Ship Components).
    Advanced,
    /// End-use good consumed by population (Food / Consumer Goods).
    Consumer,
}

/// Static definition of a tradable good.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommodityDef {
    pub name: &'static str,
    pub tier: GoodTier,
}

// Named indices for ergonomics — but nothing assumes exactly these; the count is the
// catalog length. Append new goods at the end to keep indices stable for old saves' intent.
// Raw (mined):
pub const ICE: usize = 0;
pub const ORE: usize = 1;
pub const SILICON: usize = 2;
pub const GOLD: usize = 3;
pub const SILVER: usize = 4;
pub const PLATINUM: usize = 5;
// Industrial (refined from raw):
pub const FUSION_FUEL: usize = 6;
pub const ALLOYS: usize = 7;
pub const WAFERS: usize = 8;
pub const BULLION: usize = 9;
// Advanced (manufactured from refined):
pub const ELECTRONICS: usize = 10;
pub const MACHINE_PARTS: usize = 11;
pub const SHIP_COMPONENTS: usize = 12;
// Consumer (end-use):
pub const FOOD: usize = 13;

/// The goods catalog (the *starting* set — extend by appending rows).
pub fn commodities() -> Vec<CommodityDef> {
    use GoodTier::*;
    let def = |name, tier| CommodityDef { name, tier };
    vec![
        // Raw (mined)
        def("Ice", Raw),
        def("Ore", Raw),
        def("Silicon", Raw),
        def("Gold", Raw),
        def("Silver", Raw),
        def("Platinum", Raw),
        // Industrial (refined)
        def("Fusion Fuel", Industrial),
        def("Alloys", Industrial),
        def("Silicon Wafers", Industrial),
        def("Bullion", Industrial),
        // Advanced (manufactured)
        def("Electronics", Advanced),
        def("Machine Parts", Advanced),
        def("Ship Components", Advanced),
        // Consumer (end-use)
        def("Food", Consumer),
    ]
}

/// Number of goods in the catalog — the size of every per-good `Vec<i64>` in the sim.
pub fn commodity_count() -> usize {
    commodities().len()
}

/// Number of *raw* (mineable) goods — the size of a body's basic-goods abundance vector.
pub fn raw_count() -> usize {
    commodities()
        .iter()
        .filter(|c| c.tier == GoodTier::Raw)
        .count()
}

/// A production recipe: each `(input, qty)` consumed (× the facility rate) → one unit of `out`.
/// Multi-input, so the deeper tiers blend several feedstocks (e.g. Electronics ← Wafers + Bullion).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub out: usize,
    pub inputs: Vec<(usize, i64)>,
}

/// The production chains (data — extend by appending). A four-tier tree:
/// raw → refined (1-in) → components (multi-in) → consumer.
pub fn recipes() -> Vec<Recipe> {
    let r = |out, inputs: &[(usize, i64)]| Recipe {
        out,
        inputs: inputs.to_vec(),
    };
    vec![
        // Industrial (refined from raw)
        r(FUSION_FUEL, &[(ICE, 2)]),
        r(ALLOYS, &[(ORE, 2)]),
        r(WAFERS, &[(SILICON, 2)]),
        r(BULLION, &[(GOLD, 1), (SILVER, 1), (PLATINUM, 1)]),
        // Advanced (manufactured from refined)
        r(ELECTRONICS, &[(WAFERS, 2), (BULLION, 1)]),
        r(MACHINE_PARTS, &[(ALLOYS, 2), (WAFERS, 1)]),
        r(SHIP_COMPONENTS, &[(MACHINE_PARTS, 2), (ELECTRONICS, 1)]),
        // Consumer (end-use)
        r(FOOD, &[(ICE, 1)]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_self_consistent() {
        assert_eq!(commodity_count(), 14);
        assert_eq!(raw_count(), 6);
        // Every recipe's output + inputs index a real good, and no input is a consumer good.
        for r in recipes() {
            assert!(r.out < commodity_count());
            assert!(!r.inputs.is_empty());
            for (g, qty) in &r.inputs {
                assert!(*g < commodity_count());
                assert!(*qty > 0);
                assert_ne!(commodities()[*g].tier, GoodTier::Consumer);
            }
        }
        assert_eq!(commodities()[ALLOYS].tier, GoodTier::Industrial);
        assert_eq!(commodities()[ELECTRONICS].tier, GoodTier::Advanced);
        assert_eq!(commodities()[FOOD].tier, GoodTier::Consumer);
    }
}
