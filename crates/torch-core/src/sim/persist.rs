//! Save / load (§30) — deterministic snapshot persistence.
//!
//! TORCH's sim is deterministic from a seed (§27), and its *content* lives in
//! code/data (§31). A save therefore needs only the **seed**, the **tick**, and
//! the **mutable run state** the player has shaped — never the static catalogs
//! (commodities, hulls, blueprints, bodies), which are rebuilt on load. On load
//! we re-sim the ambient world from the seed up to the saved tick (so its phase
//! lines up), then overlay the player + economy state captured here. This keeps
//! save files small and dodges the `&'static str` content fields entirely.
//!
//! Format is JSON (the §31 dependency already in the tree); a binary bincode
//! export can sit on the same [`SaveState`] later.

use super::alerts::Priority;
use super::automation::AutomationPolicy;
use super::campaign::Campaign;
use super::faction::Relations;
use super::industry::Station;
use super::logistics::TradeRoute;
use super::pressure::Intensity;
use super::progression::Branch;
use super::ships::ShipClass;
use serde::{Deserialize, Serialize};

/// Bumped whenever the on-disk shape changes; load refuses mismatches.
pub const SAVE_VERSION: u32 = 1;

/// serde default for `gate_revealed` (beat 0 is always shown).
fn one() -> usize {
    1
}

/// One owned hull, captured by class + crew quality + service history (§14). The
/// loadout is rebuilt from the class on load (content is code, §31).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipSave {
    pub name: String,
    pub class: ShipClass,
    pub commissioned_tick: u64,
    pub battles: u16,
    pub battles_won: u16,
    pub crew_quality: i64,
    /// Position + remass budget (§6).
    pub nav: super::movement::Nav,
}

/// One market's dynamic state — the stock/price pair per commodity (§7a). Defs
/// and setpoints are rebuilt from code, so only the live numbers are stored.
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

    // ---- the player corporation (§5) ----
    pub credits: i64,
    pub warehouse: Vec<i64>,
    pub trained_crew: i64,
    pub freighters: i64,
    pub fleet: Vec<ShipSave>,

    // ---- standings, campaign, progression (§4/§0/§10) ----
    pub relations: Relations,
    pub campaign: Campaign,
    pub research_unlocked: Vec<bool>,
    pub research_points: i64,
    pub blueprints_known: Vec<bool>,
    pub ceo_xp: i64,
    pub ceo_branch: Option<Branch>,

    // ---- the authored thread: opening missions + gate mystery (§0.1/§16) ----
    #[serde(default)]
    pub mission_done: Vec<bool>,
    #[serde(default = "one")]
    pub gate_revealed: usize,

    // ---- standing orders + automation (§4/§12) ----
    pub routes: Vec<TradeRoute>,
    pub stations: Vec<Station>,
    pub policy: AutomationPolicy,

    // ---- world tuning (§13/§19) ----
    pub intensity: Intensity,
    pub alert_threshold: Priority,

    // ---- the economy (§7a) ----
    pub markets: Vec<MarketSave>,
}

impl SaveState {
    /// Serialize to a pretty JSON document (the dev export, §30).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("SaveState serializes")
    }

    /// Parse a save document, rejecting an unsupported version.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let s: SaveState = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if s.version != SAVE_VERSION {
            return Err(format!(
                "unsupported save version {} (this build reads {})",
                s.version, SAVE_VERSION
            ));
        }
        Ok(s)
    }
}
