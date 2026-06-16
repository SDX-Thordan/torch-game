//! Parameterized trade-route standing orders (§4 of the interaction model).
//!
//! A `TradeRoute` is the **Hauler → Trade Route** preset: the player tunes the
//! parameters (commodity, origin → destination, quantity, minimum margin), the
//! deterministic sim flies a freighter on the loop, and a route that can't profit
//! goes idle — an exception the shell surfaces. This is the spreadsheet-sim's
//! standing-order heart: you set policy, the sim executes (§1.2 Policy agency).

/// A standing trade-route order on one freighter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TradeRoute {
    pub commodity: usize,
    /// Market the freighter buys at.
    pub origin: usize,
    /// Market the freighter sells at.
    pub dest: usize,
    pub qty: i64,
    /// Only run a trip when `price[dest] - price[origin] >= min_margin`.
    pub min_margin: i64,
    pub active: bool,
    // ---- runtime ----
    pub in_transit: bool,
    pub arrival: u64,
    pub carrying: i64,
    /// Tick the current trip departed — lets the freighter's live position be
    /// interpolated along its orbital path (§6 positional logistics). `#[serde(default)]`
    /// so saves from before this field still load.
    #[serde(default)]
    pub departed: u64,
}

impl TradeRoute {
    pub fn new(commodity: usize, origin: usize, dest: usize, qty: i64, min_margin: i64) -> Self {
        Self {
            commodity,
            origin,
            dest,
            qty,
            min_margin,
            active: true,
            in_transit: false,
            arrival: 0,
            carrying: 0,
            departed: 0,
        }
    }
}
