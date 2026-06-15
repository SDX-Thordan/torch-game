//! Faction contracts (§3.3 / §16) — the structured-income + authored-thread hook.
//!
//! A faction posts a **delivery** job: bring `qty` of a commodity to its market
//! for a credit reward and a standing bump. The player accepts from the board and
//! fulfils it from the warehouse — tying the economy (you must source the goods),
//! reputation (§10), and the §0 climb (a fulfilled contract is an operation)
//! together. The board carries its **own** RNG so generating offers never
//! perturbs the economy/combat streams (deterministic, §27).

use super::faction::Faction;
use super::rng::Pcg32;

/// A delivery contract offered by a faction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Contract {
    pub id: u64,
    pub faction: Faction,
    /// Market to deliver to.
    pub market: usize,
    pub commodity: usize,
    pub qty: i64,
    pub reward: i64,
    /// Standing gained with the offering faction on fulfilment (§10).
    pub rep: i64,
    /// Tick by which it must be fulfilled or it lapses.
    pub deadline: u64,
    pub accepted: bool,
}

/// The board of offered/accepted contracts.
#[derive(Clone, Debug)]
pub struct ContractBoard {
    rng: Pcg32,
    next_id: u64,
    offers: Vec<Contract>,
}

impl ContractBoard {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Pcg32::new(seed ^ 0xC011_7AC7),
            next_id: 0,
            offers: Vec::new(),
        }
    }

    /// The contracts currently on the board.
    pub fn offers(&self) -> &[Contract] {
        &self.offers
    }

    /// Open (not-yet-accepted) offers.
    pub fn open_count(&self) -> usize {
        self.offers.iter().filter(|c| !c.accepted).count()
    }

    /// The board's own RNG — used only to generate offers, kept separate so it
    /// never advances the world streams (§27 determinism).
    pub(crate) fn rng(&mut self) -> &mut Pcg32 {
        &mut self.rng
    }

    /// Post a new contract; returns its id.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn post(
        &mut self,
        faction: Faction,
        market: usize,
        commodity: usize,
        qty: i64,
        reward: i64,
        rep: i64,
        deadline: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.offers.push(Contract {
            id,
            faction,
            market,
            commodity,
            qty,
            reward,
            rep,
            deadline,
            accepted: false,
        });
        id
    }

    pub fn find(&self, id: u64) -> Option<&Contract> {
        self.offers.iter().find(|c| c.id == id)
    }

    pub(crate) fn accept(&mut self, id: u64) -> bool {
        if let Some(c) = self.offers.iter_mut().find(|c| c.id == id) {
            if !c.accepted {
                c.accepted = true;
                return true;
            }
        }
        false
    }

    pub(crate) fn remove(&mut self, id: u64) -> Option<Contract> {
        let pos = self.offers.iter().position(|c| c.id == id)?;
        Some(self.offers.remove(pos))
    }

    /// Drop **unaccepted** offers whose deadline has passed (accepted ones stay,
    /// so the player still owes the delivery). Returns nothing.
    pub(crate) fn expire_unaccepted(&mut self, now: u64) {
        self.offers.retain(|c| c.accepted || c.deadline > now);
    }
}
