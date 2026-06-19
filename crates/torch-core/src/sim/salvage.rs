//! Wreck salvage — the discovery & wonder seed (§15).
//!
//! Derelicts drift in the system to be found and stripped: scrap for credits,
//! salvaged data for research, or — the prize — a **reverse-engineered blueprint**
//! (§25), so discovery feeds both the wallet and curiosity. Like the contract
//! board, the field carries its **own** [`Pcg32`] (seed ⊕ a salt) so generating
//! discoveries never advances the shared world RNG — a world that reads the field
//! every tick stays bit-identical to one that ignores it (§27).

use super::rng::Pcg32;

/// Keeps the field's RNG independent of the world economy's (§27).
const SALT: u64 = 0x5A1_7A6E;
/// Ticks between wreck sightings — a rare discovery, not a faucet (events should be rare).
const SPAWN_INTERVAL: u64 = 420;
/// Most undiscovered wrecks adrift at once (a small menu, §19 hygiene).
const MAX_WRECKS: usize = 3;
/// Credit value band for a scrap haul.
const SCRAP_MIN: i64 = 1_500;
const SCRAP_SPAN: u32 = 4_500;
/// Research-point band for salvaged data.
const DATA_MIN: i64 = 40;
const DATA_SPAN: u32 = 90;
/// Basis-point chance a wreck holds a reverse-engineerable blueprint.
const BLUEPRINT_CHANCE_BP: u32 = 1_500;

/// Names for drifting derelicts (deterministic, evocative §27).
const WRECK_NAMES: [&str; 12] = [
    "Hulk of the Ardent",
    "Derelict Sparrow",
    "Broken Anvil",
    "Ghost of Ceres",
    "Silent Maru",
    "Cold Ladle",
    "Wreck of the Pellucid",
    "Drifting Castellan",
    "Husk Nine",
    "The Forgotten",
    "Rusted Halo",
    "Lost Tessera",
];

/// What stripping a wreck yields (§15: strip / reverse-engineer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SalvageReward {
    /// Scrap sold for credits.
    Scrap(i64),
    /// Salvaged data → research points (curiosity → progress).
    Data(i64),
    /// A reverse-engineered design (index into the blueprint catalog, §25).
    Blueprint(usize),
}

/// A drifting derelict to find and strip (§15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wreck {
    pub id: u64,
    pub name: &'static str,
    /// The body it drifts near, for orrery placement (§21).
    pub body: usize,
    pub reward: SalvageReward,
    pub sighted_tick: u64,
}

/// The field of discoverable wrecks (§15).
#[derive(Clone, Debug)]
pub struct SalvageField {
    rng: Pcg32,
    wrecks: Vec<Wreck>,
    next_id: u64,
    blueprint_count: usize,
    body_count: usize,
}

impl SalvageField {
    /// A field that can reward up to `blueprint_count` designs and place wrecks
    /// among `body_count` bodies. Its RNG is decoupled from the world's (§27).
    pub fn new(seed: u64, blueprint_count: usize, body_count: usize) -> Self {
        Self {
            rng: Pcg32::new(seed ^ SALT),
            wrecks: Vec::new(),
            next_id: 0,
            blueprint_count,
            body_count: body_count.max(1),
        }
    }

    pub fn wrecks(&self) -> &[Wreck] {
        &self.wrecks
    }

    /// The first sighted wreck's id — the target of the one-press salvage verb.
    pub fn first(&self) -> Option<u64> {
        self.wrecks.first().map(|w| w.id)
    }

    /// Maybe sight a new wreck this tick (the discovery cadence). Returns the new
    /// wreck's id if one appeared. Draws only from the field's own RNG, so the
    /// world economy is untouched.
    pub fn maybe_sight(&mut self, tick: u64) -> Option<u64> {
        if !tick.is_multiple_of(SPAWN_INTERVAL) || self.wrecks.len() >= MAX_WRECKS {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let name = WRECK_NAMES[self.rng.below(WRECK_NAMES.len() as u32) as usize];
        // Drift near a planet/station, never the sun (body 0).
        let body = 1 + self.rng.below(self.body_count.max(2) as u32 - 1) as usize;
        let reward = if self.blueprint_count > 0 && self.rng.chance_bp(BLUEPRINT_CHANCE_BP) {
            SalvageReward::Blueprint(self.rng.below(self.blueprint_count as u32) as usize)
        } else if self.rng.below(100) < 55 {
            SalvageReward::Scrap(SCRAP_MIN + self.rng.below(SCRAP_SPAN) as i64)
        } else {
            SalvageReward::Data(DATA_MIN + self.rng.below(DATA_SPAN) as i64)
        };
        self.wrecks.push(Wreck {
            id,
            name,
            body,
            reward,
            sighted_tick: tick,
        });
        Some(id)
    }

    /// Strip the wreck with `id`, returning its reward, or `None` if unknown.
    pub fn claim(&mut self, id: u64) -> Option<SalvageReward> {
        let i = self.wrecks.iter().position(|w| w.id == id)?;
        Some(self.wrecks.remove(i).reward)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sightings_are_deterministic_and_bounded() {
        let mut a = SalvageField::new(7, 4, 6);
        let mut b = SalvageField::new(7, 4, 6);
        let mut sighted = 0;
        for t in 0..5_000 {
            assert_eq!(a.maybe_sight(t), b.maybe_sight(t)); // same seed ⇒ same finds
            if a.wrecks().last().map(|w| w.sighted_tick) == Some(t) {
                sighted += 1;
            }
            assert!(a.wrecks().len() <= MAX_WRECKS, "the menu stays bounded");
            // Wrecks drift near a real body, never the sun.
            for w in a.wrecks() {
                assert!(w.body >= 1 && w.body < 6);
            }
        }
        assert!(
            sighted > 0,
            "the field should turn up derelicts over a long run"
        );
    }

    #[test]
    fn claiming_removes_the_wreck_and_returns_its_reward() {
        let mut f = SalvageField::new(3, 4, 6);
        // Drive sightings until one appears.
        let mut id = None;
        for t in 0..1_000 {
            if let Some(new) = f.maybe_sight(t) {
                id = Some(new);
                break;
            }
        }
        let id = id.expect("a wreck should appear");
        let before = f.wrecks().len();
        assert!(f.claim(id).is_some());
        assert_eq!(f.wrecks().len(), before - 1);
        assert!(f.claim(id).is_none(), "a wreck can't be stripped twice");
    }

    #[test]
    fn rewards_stay_in_range() {
        let mut f = SalvageField::new(11, 4, 6);
        for t in 0..20_000 {
            if let Some(id) = f.maybe_sight(t) {
                match f.claim(id).unwrap() {
                    SalvageReward::Scrap(c) => {
                        assert!((SCRAP_MIN..SCRAP_MIN + SCRAP_SPAN as i64).contains(&c))
                    }
                    SalvageReward::Data(d) => {
                        assert!((DATA_MIN..DATA_MIN + DATA_SPAN as i64).contains(&d))
                    }
                    SalvageReward::Blueprint(i) => assert!(i < 4),
                }
            }
        }
    }
}
