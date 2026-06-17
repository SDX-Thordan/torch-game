//! Independent powers & corporate diplomacy (§4) — the **negotiable** actors.
//!
//! Earth and Mars are watchful giants: you don't negotiate with them, you avoid
//! provoking them (the coalition, E3/E7). The **independent companies** that operate
//! the frontier colonies are the real diplomatic counterparties. The player makes a
//! few *macro* moves — court a company (a standing investment of Influence) to turn
//! it Partner, then Ally — and reaps **passive** benefits: an ally's colony joins you
//! willingly, and its ships help screen your trade from piracy. Cross a company
//! (seize its colony) and it turns Rival. No per-event micro; relationships are
//! standing states with ongoing effects.
//!
//! Integer/deterministic (§27); relations are a plain dial like faction standings.

use super::faction::Faction;
use super::frontier::default_colonies;

/// Relation clamp magnitude (matches faction standings).
const RELATION_CAP: i64 = 1_000;

/// A company's diplomatic stance toward the player, derived from the relation dial.
/// Ordered `Rival < Cold < Neutral < Partner < Ally`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stance {
    Rival,
    Cold,
    Neutral,
    Partner,
    Ally,
}

impl Stance {
    pub fn of(relation: i64) -> Stance {
        match relation {
            r if r <= -400 => Stance::Rival,
            r if r <= -100 => Stance::Cold,
            r if r < 200 => Stance::Neutral,
            r if r < 600 => Stance::Partner,
            _ => Stance::Ally,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Stance::Rival => "Rival",
            Stance::Cold => "Cold",
            Stance::Neutral => "Neutral",
            Stance::Partner => "Partner",
            Stance::Ally => "Ally",
        }
    }
}

/// An independent company operating a frontier colony — a diplomatic actor. Content
/// (name/home) lives in code; only the `relation` dial is save state (persisted as a
/// plain `Vec<i64>`, like the §31 tuning split), so no serde on the struct itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Company {
    pub name: &'static str,
    /// The colony it operates — index into `frontier::default_colonies`.
    pub home_colony: usize,
    /// The player's standing with it, `-RELATION_CAP..=RELATION_CAP`.
    pub relation: i64,
}

/// The diplomatic field of independent companies (§4) — one per independent colony.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diplomacy {
    companies: Vec<Company>,
}

/// The flavour name of the company operating a given independent colony.
fn operator_name(colony_name: &str) -> &'static str {
    match colony_name {
        "Ganymede Free Port" => "Ganymede Free Traders",
        "Callisto Yards" => "Callisto Shipwrights",
        "Enceladus Wells" => "Enceladus Hydro Combine",
        "Triton Outpost" => "Triton Pioneers",
        _ => "Independent Operators",
    }
}

impl Default for Diplomacy {
    fn default() -> Self {
        Self::new()
    }
}

impl Diplomacy {
    /// Build the diplomatic field: one company per **independent** frontier colony,
    /// all starting Neutral (the player is independent by default, §3).
    pub fn new() -> Self {
        let companies = default_colonies()
            .iter()
            .enumerate()
            .filter(|(_, c)| c.faction == Faction::Independents)
            .map(|(i, c)| Company {
                name: operator_name(c.name),
                home_colony: i,
                relation: 0,
            })
            .collect();
        Self { companies }
    }

    pub fn companies(&self) -> &[Company] {
        &self.companies
    }

    pub fn relation(&self, i: usize) -> i64 {
        self.companies.get(i).map(|c| c.relation).unwrap_or(0)
    }

    pub fn stance(&self, i: usize) -> Stance {
        Stance::of(self.relation(i))
    }

    /// Move a company's relation, clamped.
    pub fn adjust(&mut self, i: usize, delta: i64) {
        if let Some(c) = self.companies.get_mut(i) {
            c.relation = (c.relation + delta).clamp(-RELATION_CAP, RELATION_CAP);
        }
    }

    /// The company operating `colony`, if any (independent colonies only).
    pub fn company_for_colony(&self, colony: usize) -> Option<usize> {
        self.companies.iter().position(|c| c.home_colony == colony)
    }

    /// How many companies are Allies — each lends an escort to your trade security
    /// (EP3): a passive benefit of standing diplomacy.
    pub fn ally_count(&self) -> usize {
        self.companies
            .iter()
            .filter(|c| Stance::of(c.relation) == Stance::Ally)
            .count()
    }

    /// Restore persisted relations (by company order) — content (names/homes) stays
    /// in code; only the relation dials are save state.
    pub fn restore(&mut self, relations: &[i64]) {
        for (c, &r) in self.companies.iter_mut().zip(relations.iter()) {
            c.relation = r.clamp(-RELATION_CAP, RELATION_CAP);
        }
    }

    /// The relation dials, in company order (for persistence).
    pub fn relations(&self) -> Vec<i64> {
        self.companies.iter().map(|c| c.relation).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_company_per_independent_colony_all_neutral() {
        let d = Diplomacy::new();
        assert!(!d.companies().is_empty());
        for (i, c) in d.companies().iter().enumerate() {
            assert_eq!(c.relation, 0);
            assert_eq!(d.stance(i), Stance::Neutral);
            // Each operates an independent colony.
            assert_eq!(
                d.company_for_colony(c.home_colony),
                Some(i),
                "the reverse lookup matches"
            );
        }
        assert_eq!(d.ally_count(), 0);
    }

    #[test]
    fn courting_climbs_the_stances_and_souring_drops_them() {
        let mut d = Diplomacy::new();
        d.adjust(0, 600);
        assert_eq!(d.stance(0), Stance::Ally);
        assert_eq!(d.ally_count(), 1);
        d.adjust(0, -1_200);
        assert_eq!(d.stance(0), Stance::Rival);
        assert_eq!(d.ally_count(), 0);
    }
}
