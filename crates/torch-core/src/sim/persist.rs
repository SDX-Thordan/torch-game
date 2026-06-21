//! Save / load (§30) — deterministic snapshot persistence for the multi-player world.
//!
//! A save carries the **seed + tick + mutable state** only; the bodies, catalogs, recipes, and
//! market price-defs are rebuilt from code on load and the saved numbers overlaid. The player
//! entities, ships, facilities, and settlements all derive serde and round-trip directly; only
//! markets need a reduced [`MarketSave`] (their price-defs are code).

use super::facility::Facility;
use super::player::{Player, PlayerId};
use super::ship::Ship;
use super::world::{Colony, MiningStation};
use serde::{Deserialize, Serialize};

/// Bumped whenever the on-disk shape changes; load refuses mismatches. v4 = the market-trade
/// rebuild — jobs carry a reserved `qty` and the sinks are gone (prior dev saves are intentionally
/// incompatible). Market reservations aren't stored; they're rebuilt from in-flight jobs on load.
pub const SAVE_VERSION: u32 = 5;

/// One market's dynamic state — the stock/price pair per good (price-defs rebuilt from code).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarketSave {
    pub stocks: Vec<i64>,
    pub prices: Vec<i64>,
}

/// A complete deterministic save (§30).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveState {
    pub version: u32,
    pub seed: u64,
    pub tick: u64,
    pub human: PlayerId,
    pub players: Vec<Player>,
    pub ships: Vec<Ship>,
    pub facilities: Vec<Facility>,
    pub colonies: Vec<Colony>,
    pub mining_stations: Vec<MiningStation>,
    #[serde(default)]
    pub zero_g_stations: Vec<super::world::ZeroGStation>,
    pub markets: Vec<MarketSave>,
}

impl SaveState {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("SaveState serializes")
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        let s: SaveState = serde_json::from_str(json).map_err(|e| e.to_string())?;
        Self::check_version(s)
    }
    pub fn to_bincode(&self) -> Vec<u8> {
        bincode::serialize(self).expect("SaveState serializes to bincode")
    }
    pub fn from_bincode(bytes: &[u8]) -> Result<Self, String> {
        let s: SaveState = bincode::deserialize(bytes).map_err(|e| e.to_string())?;
        Self::check_version(s)
    }
    fn check_version(s: Self) -> Result<Self, String> {
        if s.version != SAVE_VERSION {
            return Err(format!(
                "unsupported save version {} (this build reads {})",
                s.version, SAVE_VERSION
            ));
        }
        Ok(s)
    }
}
