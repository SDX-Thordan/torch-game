//! Ambient system chatter (§19 texture) — occasional flavour beats from around Sol.
//!
//! A relaxing trade/management sim wants the world to feel *alive* while you let it
//! run: spot news from the Belt, the inners, the markets, the deep dark. These are
//! pure **flavour** — voiced to the feed as low-key `Notice`/`Fyi` chatter, never a
//! demand and never a mechanical effect. Like the salvage field and the contract
//! board, the generator carries its **own** [`Pcg32`] (seed ⊕ a salt) so producing a
//! beat never advances the shared world RNG — a world that reads it every tick stays
//! bit-identical to one that ignores it (§27). With no new `Event` variant and no
//! economy touch, the §7c economy gate and the QA gameplay tally are untouched.

use super::rng::Pcg32;

/// Keeps the chatter RNG independent of the world economy's (§27).
const SALT: u64 = 0x0A3B_1E27;
/// Base ticks between beats (~30 days at 6 ticks/day) — gentle texture, not noise.
const INTERVAL: u64 = 180;
/// Random stagger so beats don't fall on a metronome (and don't dogpile other systems).
const JITTER: u32 = 90;

/// The flavour pool: `(voice, message)`. Grounded trade / politics / space colour — no
/// gate or protomolecule lore (that story is removed until its proper arc lands). Each
/// reads as a snippet from a different corner of the system, so a long run feels lived-in.
const CHATTER: [(&str, &str); 24] = [
    // The Belt / OPA wire.
    ("Belt Wire", "A water-haulers' union slows loading at Ceres docks over hazard pay. Spot ice tightens for a cycle."),
    ("Belt Wire", "Tycho Station lights a new drive-yard berth — civilian hull prices may ease this quarter."),
    ("Belt Wire", "Prospectors burn hard for a rich seam rumoured off a Kuiper shard. Most will find rock."),
    ("Belt Wire", "A rock-hopper co-op declares a 'founding day' holiday. Half the Belt's depots run a skeleton shift."),
    ("Belt Wire", "Pallas refiners post record throughput. Somewhere, a foreman is very tired and very proud."),
    // The inner powers.
    ("Inner Feeds", "Martian naval exercises near Deimos snarl commercial traffic for a cycle. File your burns early."),
    ("Inner Feeds", "Earth's relief board quietly buys up reactor fuel — a price floor that won't hold long."),
    ("Inner Feeds", "A trade delegation shuttles between Luna and Ceres. Nobody will say what about."),
    ("Inner Feeds", "Mars tightens export paperwork on machined parts. Brokers grumble; prices drift up."),
    ("Inner Feeds", "An Earth-Mars summit is announced, then postponed, then denied. Standard."),
    // Space weather / science.
    ("Sol Watch", "A coronal mass ejection washes the inner system — comms crackle and outsystem relays stutter for a day."),
    ("Sol Watch", "Observatories log a slow comet inbound from the dark. Harmless. Beautiful. Already named twice."),
    ("Sol Watch", "A quiet stretch on the deep-range beacons. Probably just the sunspot cycle, the duty officer notes."),
    ("Sol Watch", "Solar wind picks up; navigators add a hair of margin to long burns this week."),
    ("Sol Watch", "A survey drone returns from the outer dark with nothing to report and a beautiful, useless photograph."),
    // Markets / economy.
    ("Ledger", "Machinery demand firms across the outer hubs as new outposts break ground."),
    ("Ledger", "A glut of refined metals presses prices down in the Belt this quarter."),
    ("Ledger", "Insurers raise convoy premiums after a string of quiet losses outsystem."),
    ("Ledger", "Volatiles tick up on the inner exchanges; somebody knows something, or pretends to."),
    ("Ledger", "A mid-sized hauling firm folds; its routes are carved up before the ink dries."),
    // Dockside colour.
    ("Dockside", "A freighter crew wins big in a Ceres card hall and tips the whole bar. The story grows by morning."),
    ("Dockside", "Two corp scouts trade notes over cheap whiskey, then each pretends they didn't."),
    ("Dockside", "A dock chaplain blesses an outbound hull. The crew is touched; the cargo is overdue."),
    ("Dockside", "An old hand swears the lanes were busier in her day. They were not. They never are."),
];

/// The ambient flavour-beat generator (§19 texture).
#[derive(Clone, Debug)]
pub struct AmbientChatter {
    rng: Pcg32,
    next_tick: u64,
    /// The last beat index, to avoid an immediate repeat (variety hygiene).
    last: usize,
}

impl AmbientChatter {
    /// A generator decoupled from the world economy's RNG (§27).
    pub fn new(seed: u64) -> Self {
        let mut s = Self {
            rng: Pcg32::new(seed ^ SALT),
            next_tick: 0,
            last: usize::MAX,
        };
        s.schedule(0);
        s
    }

    fn schedule(&mut self, tick: u64) {
        self.next_tick = tick + INTERVAL + self.rng.below(JITTER) as u64;
    }

    /// The most-recently voiced beat's `(voice, message)`, for a dedicated "system wire"
    /// ticker that always shows the latest chatter (the alert feed pins urgent items, so
    /// low-priority chatter would otherwise be buried). `None` before the first beat.
    pub fn latest(&self) -> Option<(&'static str, &'static str)> {
        if self.last == usize::MAX {
            None
        } else {
            Some(CHATTER[self.last])
        }
    }

    /// Maybe produce a flavour beat this tick — `(voice, message)` to voice as chatter, or
    /// `None`. Draws only from its own RNG, so the world economy is untouched (§27).
    pub fn maybe_chatter(&mut self, tick: u64) -> Option<(&'static str, &'static str)> {
        if tick < self.next_tick {
            return None;
        }
        self.schedule(tick);
        let n = CHATTER.len();
        let mut i = self.rng.below(n as u32) as usize;
        if i == self.last {
            i = (i + 1) % n; // never repeat the immediately-previous beat
        }
        self.last = i;
        Some(CHATTER[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatter_is_occasional_and_deterministic() {
        let mut a = AmbientChatter::new(7);
        let mut beats = 0;
        for t in 0..4_000 {
            if a.maybe_chatter(t).is_some() {
                beats += 1;
            }
        }
        // ~1 beat per 180–270 ticks over 4000 ticks → a teens-count of gentle texture.
        assert!(
            (10..=24).contains(&beats),
            "occasional, not a faucet (got {beats})"
        );
        // Same seed ⇒ same schedule + same beats.
        let mut b = AmbientChatter::new(7);
        let mut b_beats = 0;
        for t in 0..4_000 {
            if b.maybe_chatter(t).is_some() {
                b_beats += 1;
            }
        }
        assert_eq!(beats, b_beats, "deterministic from the seed");
    }

    #[test]
    fn no_immediate_repeats() {
        let mut a = AmbientChatter::new(3);
        let mut prev: Option<&str> = None;
        for t in 0..20_000 {
            if let Some((_, msg)) = a.maybe_chatter(t) {
                if let Some(p) = prev {
                    assert_ne!(p, msg, "a beat never repeats back-to-back");
                }
                prev = Some(msg);
            }
        }
    }
}
