//! `endgame` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// The far-side incursion layer (§17, G4), run each tick once past the ring: an
    /// escalating threat that telegraphs, lands on the bridgehead as an act-now
    /// "defend" exception, and — if unanswered within the window — damages the
    /// foothold. Gated on `pressure.endgame()`, which is off until transit, so the
    /// pre-transit world never enters here.
    pub(crate) fn run_incursions(&mut self, now: u64) {
        // Once the endgame has resolved (§17, G5) the far side stops pressing — the
        // journey has reached its end, win or lose.
        if self.endgame_outcome != EndgameOutcome::Undecided {
            return;
        }
        // An unanswered incursion strikes the bridgehead when its window lapses.
        if let Some((severity, deadline)) = self.pending_incursion {
            if now >= deadline {
                self.pending_incursion = None;
                self.strike_bridgehead(severity);
            }
        }
        // Telegraph the next incursion ahead of time (§13 forecasting carried over).
        if self.pressure.should_forecast_incursion(now) {
            let eta = self.pressure.incursion_eta(now);
            self.events.push(Event::ThreatForecast {
                kind: PressureKind::Incursion,
                eta,
            });
            self.pressure.mark_incursion_forecast_sent();
        }
        // A new incursion lands (only if none is already pending — one crisis at a
        // time on the foothold).
        if self.pending_incursion.is_none() && self.pressure.incursion_ready(now) {
            let severity = self.pressure.incursion_severity(now);
            self.pending_incursion = Some((severity, now + INCURSION_RESPONSE_WINDOW));
            self.events.push(Event::IncursionStruck { severity });
            self.pressure.after_incursion(now);
        }
    }

    /// Apply incursion damage to the bridgehead and voice it; if it falls, emit the
    /// loss beat (§17, G4/G5). No-op without a founded foothold (the incursion finds
    /// nothing to hit).
    pub(crate) fn strike_bridgehead(&mut self, severity: i64) {
        if !self.bridgehead.is_founded() {
            return;
        }
        let fell = self.bridgehead.damage(severity);
        self.events.push(Event::BridgeheadDamaged {
            integrity: self.bridgehead.integrity(),
        });
        if fell {
            self.events.push(Event::BridgeheadFell);
            // The bridgehead is overrun — the endgame is lost (§17, G5).
            if self.endgame_outcome == EndgameOutcome::Undecided {
                self.endgame_outcome = EndgameOutcome::Fallen;
                self.events.push(Event::EndgameLost);
            }
        }
    }

    /// Check whether the far-side endgame has been won (§17, G5): the bridgehead has
    /// been grown to [`WIN_BRIDGEHEAD_LEVEL`] *and* held through
    /// [`WIN_INCURSIONS_SURVIVED`] repelled incursions. Fires once.
    pub(crate) fn check_endgame_won(&mut self) {
        if self.endgame_outcome == EndgameOutcome::Undecided
            && self.bridgehead.level() >= WIN_BRIDGEHEAD_LEVEL
            && self.incursions_survived >= WIN_INCURSIONS_SURVIVED
        {
            self.endgame_outcome = EndgameOutcome::Triumph;
            self.events.push(Event::EndgameWon);
            self.complete_op();
        }
    }

    /// How the far-side endgame resolved (§17, G5): `Undecided`/`Triumph`/`Fallen`.
    pub fn endgame_outcome(&self) -> EndgameOutcome {
        self.endgame_outcome
    }

    /// Incursions repelled so far (§17, G5) — progress toward the victory threshold.
    pub fn incursions_survived(&self) -> u64 {
        self.incursions_survived
    }

    /// The victory thresholds for the destination panel (§17, G5):
    /// `(target bridgehead level, target incursions survived)`.
    pub fn endgame_targets(&self) -> (u32, u64) {
        (WIN_BRIDGEHEAD_LEVEL, WIN_INCURSIONS_SURVIVED)
    }

    // ---- the empire layer: holdings & acquisition (E1) ----------------------
}
