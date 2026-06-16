//! Opening missions + the authored **gate-mystery** thread (§0.1, §16) — the
//! *narrative* half of the destination pull the GDD ranks its #1 over-invest
//! priority (§0.2). The mechanical spine (tiers, the always-visible gate %, voiced
//! ascents) already exists in `sim::campaign`; this adds the authored content that
//! turns "a progress bar to the gate" into "a mystery you want to chase".
//!
//! Two pieces:
//! - **Opening missions** — a short ordered chain that teaches the four verbs
//!   (trade, build, standing orders, interdiction) then a first climb (§16). Each
//!   completes the first time the player does the thing; they thin into the
//!   campaign's per-tier goals after.
//! - **Gate-mystery beats** — authored fragments revealed across tier ascents and
//!   salvage finds (§15 anomalies seeding it), each voiced as "The Gate".
//!
//! Pure/deterministic (§27): completion is driven by player acts, not RNG.

/// The act that completes an opening mission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    FirstTrade,
    FirstWarship,
    FirstRoute,
    FirstCut,
    FirstAscent,
}

/// One tutorial objective (§16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mission {
    pub trigger: Trigger,
    pub title: &'static str,
    pub hint: &'static str,
    pub done: bool,
}

/// The authored gate-mystery, revealed a beat at a time (§0.1). Beat 0 is shown
/// from minute one (the carrot); the rest unlock as you climb and salvage.
pub const GATE_LORE: [&str; 7] = [
    "The Gate: It hangs dark beyond Pluto — a ring of alien metal, cold a thousand years. No one built it. No one has seen it open. It is the reason you climb.",
    "The Gate: A century-old Belt survey logs a single pulse from the ring — one burst, then silence. Filed as instrument error. The Belt never quite believed that.",
    "The Gate: Stripped from a derelict's drive core — a coordinate lattice no human navigator wrote. Every vector ends at the ring.",
    "The Gate: Earth and Mars both fund 'deep-range survey' charters that never report. Each denies the other does.",
    "The Gate: The ring's surface has warmed four degrees this decade. Slowly. Deliberately. As if waking.",
    "The Gate: There is structure in the pulse now — not noise. A pattern. A question, repeating. Something on the far side is counting.",
    "The Gate: It warms faster each month, and the powers are arming. Whatever opens that ring will own what comes through — or be owned by it.",
];

/// The gate's **answer** (§0.1 payoff) — revealed only when the player *transits*
/// the ring, the climax the seven mystery beats build toward. The resolution, and
/// the threshold into the endgame (§17).
pub const GATE_ANSWER: &str = "Through the ring: a second sky, older than Sol — and ringed around the far aperture, the cold wrecks of everyone who reached it before you. The repeating pulse was never a warning. It was a tally of arrivals, counting up. You are the newest number on it. Whatever has been keeping that count is still out there in the dark — and now it knows your face. The larger game begins here.";

/// The player's authored-thread progress (§0.1/§16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missions {
    opening: Vec<Mission>,
    /// How many gate-mystery beats have been revealed (≥ 1: beat 0 from the start).
    gate_revealed: usize,
}

impl Default for Missions {
    fn default() -> Self {
        Self::new()
    }
}

impl Missions {
    pub fn new() -> Self {
        let m = |trigger, title, hint| Mission {
            trigger,
            title,
            hint,
            done: false,
        };
        Self {
            opening: vec![
                m(
                    Trigger::FirstTrade,
                    "First Light",
                    "Buy a commodity cheap at one market and sell it dear at another.",
                ),
                m(
                    Trigger::FirstWarship,
                    "Stand Up a Hull",
                    "Commission your first warship at the Yards (you'll need crew + credits).",
                ),
                m(
                    Trigger::FirstRoute,
                    "Standing Orders",
                    "Set a trade route so the company hauls a spread without you.",
                ),
                m(
                    Trigger::FirstCut,
                    "Cut a Lane",
                    "Interdict an NPC hauler — deny a delivery, bloom a shortage to exploit.",
                ),
                m(
                    Trigger::FirstAscent,
                    "Climb",
                    "Run operations until the Board promotes you to the next tier.",
                ),
            ],
            gate_revealed: 1,
        }
    }

    /// Record a player act. If it completes the next matching open mission, returns
    /// that mission's title (for the feed to voice). Order-independent: any not-yet
    /// done mission of this trigger completes.
    pub fn note(&mut self, trigger: Trigger) -> Option<&'static str> {
        let mission = self
            .opening
            .iter_mut()
            .find(|m| !m.done && m.trigger == trigger)?;
        mission.done = true;
        Some(mission.title)
    }

    /// Reveal the next gate-mystery beat (§0.1), if any remain. Returns the beat's
    /// text for the feed.
    pub fn reveal_gate(&mut self) -> Option<&'static str> {
        if self.gate_revealed < GATE_LORE.len() {
            let beat = GATE_LORE[self.gate_revealed];
            self.gate_revealed += 1;
            Some(beat)
        } else {
            None
        }
    }

    /// The active opening mission (the first not-done), if the tutorial isn't over.
    pub fn active(&self) -> Option<&Mission> {
        self.opening.iter().find(|m| !m.done)
    }

    /// Opening missions completed / total (for the HUD).
    pub fn opening_progress(&self) -> (usize, usize) {
        (
            self.opening.iter().filter(|m| m.done).count(),
            self.opening.len(),
        )
    }

    /// The most-recently revealed gate-mystery beat (always at least beat 0).
    pub fn latest_gate(&self) -> &'static str {
        GATE_LORE[self
            .gate_revealed
            .saturating_sub(1)
            .min(GATE_LORE.len() - 1)]
    }

    pub fn gate_beats_revealed(&self) -> usize {
        self.gate_revealed
    }

    /// Restore from a save (§30): the done-flags + revealed count.
    pub fn restore(&mut self, done: &[bool], gate_revealed: usize) {
        for (m, &d) in self.opening.iter_mut().zip(done) {
            m.done = d;
        }
        self.gate_revealed = gate_revealed.clamp(1, GATE_LORE.len());
    }

    /// The done-flags, for a save (§30).
    pub fn done_flags(&self) -> Vec<bool> {
        self.opening.iter().map(|m| m.done).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_missions_complete_once_in_any_order() {
        let mut m = Missions::new();
        assert_eq!(m.active().unwrap().trigger, Trigger::FirstTrade);
        // Complete out of order — each still fires once.
        assert_eq!(m.note(Trigger::FirstWarship), Some("Stand Up a Hull"));
        assert_eq!(m.note(Trigger::FirstWarship), None, "no double-complete");
        assert_eq!(m.note(Trigger::FirstTrade), Some("First Light"));
        // The active one is now the first still-open in sequence.
        assert_eq!(m.active().unwrap().trigger, Trigger::FirstRoute);
        let (done, total) = m.opening_progress();
        assert_eq!((done, total), (2, 5));
    }

    #[test]
    fn the_whole_tutorial_can_be_finished() {
        let mut m = Missions::new();
        for t in [
            Trigger::FirstTrade,
            Trigger::FirstWarship,
            Trigger::FirstRoute,
            Trigger::FirstCut,
            Trigger::FirstAscent,
        ] {
            assert!(m.note(t).is_some());
        }
        assert!(m.active().is_none(), "tutorial complete");
        assert_eq!(m.opening_progress(), (5, 5));
    }

    #[test]
    fn gate_lore_reveals_in_order_then_stops() {
        let mut m = Missions::new();
        assert_eq!(m.gate_beats_revealed(), 1, "beat 0 shown from the start");
        assert_eq!(m.latest_gate(), GATE_LORE[0]);
        let mut count = 1;
        while let Some(beat) = m.reveal_gate() {
            assert_eq!(beat, GATE_LORE[count]);
            count += 1;
        }
        assert_eq!(count, GATE_LORE.len());
        assert_eq!(m.reveal_gate(), None, "no more beats");
        assert_eq!(m.latest_gate(), GATE_LORE[GATE_LORE.len() - 1]);
    }

    #[test]
    fn save_round_trips_the_thread() {
        let mut a = Missions::new();
        a.note(Trigger::FirstTrade);
        a.note(Trigger::FirstCut);
        a.reveal_gate();
        a.reveal_gate();
        let mut b = Missions::new();
        b.restore(&a.done_flags(), a.gate_beats_revealed());
        assert_eq!(a, b);
    }
}
