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
}

/// The frontier's standing colonies, spread across all four powers so the outer
/// system reads as contested, lived-in space (§17) — Saturn especially is busy.
/// Bodies are resolved **by name** so the list survives any re-layout of the
/// moon indices.
pub fn default_colonies() -> Vec<Colony> {
    use Faction::*;
    let bodies = default_system();
    let on = |moon: &str, faction: Faction, name: &'static str| -> Colony {
        let body = bodies
            .iter()
            .position(|b| b.name == moon)
            .unwrap_or_else(|| panic!("no body named {moon}"));
        Colony {
            body,
            faction,
            name,
        }
    };
    vec![
        on("Luna", Earth, "Luna Dock"),
        on("Europa", Mars, "Europa Deep"),
        on("Ganymede", Independents, "Ganymede Free Port"),
        on("Callisto", Independents, "Callisto Yards"),
        // Saturn's settled moons — the OPA frontier with Earth/Mars footholds.
        on("Titan", Belt, "Titan Station (OPA)"),
        on("Rhea", Belt, "Rhea Hold (OPA)"),
        on("Dione", Mars, "Dione Garrison"),
        on("Enceladus", Independents, "Enceladus Wells"),
        on("Iapetus", Belt, "Iapetus Watch (OPA)"),
        on("Tethys", Earth, "Tethys Relay"),
        // The deep frontier.
        on("Triton", Independents, "Triton Outpost"),
        on("Charon", Belt, "Charon Watch (OPA)"),
    ]
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
