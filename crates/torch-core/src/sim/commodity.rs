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
    /// Mined directly from bodies (Ice / Ore / Rare Materials).
    Raw,
    /// Refined from a raw good in a facility (Alloys / Fusion Fuel / Electronics).
    Industrial,
    /// End-use good consumed by population (Food).
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
pub const ICE: usize = 0;
pub const ORE: usize = 1;
pub const RARE: usize = 2;
pub const ALLOYS: usize = 3;
pub const FUSION_FUEL: usize = 4;
pub const ELECTRONICS: usize = 5;
pub const FOOD: usize = 6;

/// The goods catalog (the *starting* set — extend by appending rows).
pub fn commodities() -> Vec<CommodityDef> {
    use GoodTier::*;
    vec![
        CommodityDef {
            name: "Ice",
            tier: Raw,
        },
        CommodityDef {
            name: "Ore",
            tier: Raw,
        },
        CommodityDef {
            name: "Rare Materials",
            tier: Raw,
        },
        CommodityDef {
            name: "Alloys",
            tier: Industrial,
        },
        CommodityDef {
            name: "Fusion Fuel",
            tier: Industrial,
        },
        CommodityDef {
            name: "Electronics",
            tier: Industrial,
        },
        CommodityDef {
            name: "Food",
            tier: Consumer,
        },
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

/// A production recipe: `ratio` units of `input` → one unit of `out`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipe {
    pub out: usize,
    pub input: usize,
    pub ratio: i64,
}

/// The production chains (data — extend by appending). Alloys←Ore, Fusion Fuel←Ice,
/// Electronics←Rare Materials, Food←Ice.
pub fn recipes() -> Vec<Recipe> {
    vec![
        Recipe {
            out: ALLOYS,
            input: ORE,
            ratio: 2,
        },
        Recipe {
            out: FUSION_FUEL,
            input: ICE,
            ratio: 2,
        },
        Recipe {
            out: ELECTRONICS,
            input: RARE,
            ratio: 2,
        },
        Recipe {
            out: FOOD,
            input: ICE,
            ratio: 1,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_self_consistent() {
        assert_eq!(commodity_count(), 7);
        assert_eq!(raw_count(), 3);
        // Every recipe's input/output indexes a real good, and its input is a raw or
        // industrial good (never a consumer good).
        for r in recipes() {
            assert!(r.out < commodity_count() && r.input < commodity_count());
            assert_ne!(commodities()[r.input].tier, GoodTier::Consumer);
        }
        assert_eq!(commodities()[ALLOYS].tier, GoodTier::Industrial);
        assert_eq!(commodities()[FOOD].tier, GoodTier::Consumer);
    }
}
