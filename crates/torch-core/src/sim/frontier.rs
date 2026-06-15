//! The settled frontier (§17) — established colonies scattered across the outer
//! system, each aligned to a power (§4). For now this is *content + identity*: the
//! colonies give the moons of the gas giants a populated, factional feel (rendered
//! as faction-coloured markers on the orrery). Wiring them as tradeable markets —
//! with the long-haul traffic tuning the outer system needs — is the next step.
//!
//! The Belt faction stands for the **OPA**-aligned Belters (§4); Earth, Mars and
//! the Independents hold their own outposts. Body indices match `orbit::default_system`.

use super::faction::Faction;
use super::orbit::default_system;

/// One settled outpost on a moon or dwarf world (§17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colony {
    /// The body (moon/dwarf) it sits on — index into `orbit::default_system`.
    pub body: usize,
    pub faction: Faction,
    pub name: &'static str,
    /// Whether this outpost is a full **trading market** (a frontier hub) or just
    /// a settled marker. The major hubs trade; lesser outposts are flavour (§17).
    pub is_market: bool,
}

/// The frontier's standing colonies, spread across all four powers so the outer
/// system reads as contested, lived-in space (§17) — Saturn especially is busy.
/// Bodies are resolved **by name** so the list survives any re-layout of the
/// moon indices. The major hubs (`is_market`) are tradeable markets.
pub fn default_colonies() -> Vec<Colony> {
    use Faction::*;
    let bodies = default_system();
    let on = |moon: &str, faction: Faction, name: &'static str, is_market: bool| -> Colony {
        let body = bodies
            .iter()
            .position(|b| b.name == moon)
            .unwrap_or_else(|| panic!("no body named {moon}"));
        Colony {
            body,
            faction,
            name,
            is_market,
        }
    };
    vec![
        on("Luna", Earth, "Luna Dock", false),
        on("Europa", Mars, "Europa Deep", true),
        on("Ganymede", Independents, "Ganymede Free Port", true),
        on("Callisto", Independents, "Callisto Yards", false),
        // Saturn's settled moons — the OPA frontier with Earth/Mars footholds.
        on("Titan", Belt, "Titan Station (OPA)", true),
        on("Rhea", Belt, "Rhea Hold (OPA)", false),
        on("Dione", Mars, "Dione Garrison", false),
        on("Enceladus", Independents, "Enceladus Wells", false),
        on("Iapetus", Belt, "Iapetus Watch (OPA)", false),
        on("Tethys", Earth, "Tethys Relay", false),
        // The deep frontier.
        on("Triton", Independents, "Triton Outpost", false),
        on("Charon", Belt, "Charon Watch (OPA)", false),
    ]
}

/// The frontier colonies that are full trading markets (§17), each as
/// `(body, faction, short market name)` — the short name reads cleanly as a
/// market-board column.
pub fn market_colonies() -> Vec<Colony> {
    default_colonies()
        .into_iter()
        .filter(|c| c.is_market)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::orbit::default_system;

    #[test]
    fn colonies_sit_on_real_outer_bodies() {
        let bodies = default_system();
        for c in default_colonies() {
            assert!(c.body < bodies.len(), "{} has no body", c.name);
            // Colonies live on moons/dwarf worlds, never the sun or a gas giant
            // surface, and out past the inner planets (the frontier, §17).
            assert!(c.body >= 12, "{} should be an outer-system outpost", c.name);
        }
    }

    #[test]
    fn every_power_holds_some_frontier() {
        // The outer system is contested — each faction is represented (§4/§17).
        for f in [
            Faction::Earth,
            Faction::Mars,
            Faction::Belt,
            Faction::Independents,
        ] {
            assert!(
                default_colonies().iter().any(|c| c.faction == f),
                "{f:?} holds no frontier outpost"
            );
        }
    }
}
