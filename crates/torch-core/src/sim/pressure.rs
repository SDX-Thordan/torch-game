//! Pressure, tension & pacing (§13).
//!
//! Three layered pressures — **faction war**, **piracy & raiders**, **survival &
//! scarcity** — plus the calibration §13 calls for: *recoverable ≠
//! consequence-free*. The load-bearing mechanics here are the ones that keep
//! tension from tipping into stress:
//!
//! - **Forecasting** kills the unforeseeable: an incoming raid is *telegraphed*
//!   [`FORECAST_LEAD`] ticks before it strikes, so the player can pre-position an
//!   escort or divert a convoy — nothing arrives out of nowhere.
//! - **A pacing governor** stops simultaneous spikes: a raid never lands within
//!   [`PACING_COOLDOWN`] ticks of another flashpoint (e.g. a fresh scarcity), so
//!   crises queue rather than dogpile.
//! - **Biting-but-recoverable decay**: each pressure ebbs every tick, so a quiet
//!   stretch heals the world (the grudge isn't a one-way cliff), while a sustained
//!   assault outruns the drift and the pressure stays high.
//! - An independent **intensity** knob (§13 difficulty) scales how often raids
//!   come and how hard events push the gauges, without touching the core economy.
//!
//! Pure and integer-only (§27): it draws no RNG and reads no wall-clock, so the
//! schedule is bit-identical across platforms. The shell reads the gauges for the
//! pressure HUD and the §23c audio state (calm hum vs. alarm bed).

use super::event::Event;

/// Base ticks between raid windows at [`Intensity::Normal`] (matches the legacy
/// ambient-pirate cadence, so default play is unchanged).
const BASE_RAID_PERIOD: u64 = 72;
/// How far ahead an incoming raid is telegraphed (§13 forecasting).
pub const FORECAST_LEAD: u64 = 18;
/// No raid lands within this many ticks of another flashpoint (the governor).
const PACING_COOLDOWN: u64 = 24;
/// Gauge ceiling and the per-tick ebb (biting-but-recoverable).
const LEVEL_MAX: i32 = 100;
const LEVEL_DECAY: i32 = 1;
/// Per-event gauge gains at Normal intensity (scaled by [`Intensity`]).
const RAID_GAIN: i32 = 30;
const SCARCITY_GAIN: i32 = 20;
const WAR_GAIN: i32 = 25;
const INCURSION_GAIN: i32 = 35;
/// Basis-point denominator for intensity scaling.
const BP: i32 = 10_000;

// ---- the far-side endgame: incursions (§17, G4) ----
// An escalating threat from beyond the ring, *only* active once the player has
// transited (the `endgame` flag is off until then, so the pre-transit world is
// byte-identical). The longer you hold the far side, the more often and harder it
// answers — "now it knows your face" (the GATE_ANSWER payoff).

/// Ticks between incursions at the moment you transit (the gentlest cadence).
const BASE_INCURSION_PERIOD: u64 = 120;
/// The cadence floor — incursions never come closer than this (escalation cap).
const INCURSION_MIN_PERIOD: u64 = 40;
/// Every this-many ticks held on the far side, the cadence tightens one step…
const INCURSION_RAMP: u64 = 240;
/// …by this many ticks.
const INCURSION_PERIOD_STEP: u64 = 10;
/// Severity (bridgehead damage / pack strength) of the first incursion…
const BASE_INCURSION_SEVERITY: i64 = 30;
/// …rising this much every [`INCURSION_RAMP`] ticks held (the count climbing).
const INCURSION_SEVERITY_STEP: i64 = 10;

/// The independent **pressure-intensity** difficulty setting (§13): scales raid
/// cadence and gauge gains without rubber-banding the player's earned power.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Intensity {
    /// Rare raids, gentle gauges — a builder's sandbox.
    Calm,
    /// The tuned default.
    #[default]
    Normal,
    /// Frequent raids, steep gauges — for players who want the pressure.
    Harsh,
}

impl Intensity {
    /// Ticks between raid windows at this intensity.
    fn raid_period(self) -> u64 {
        match self {
            Intensity::Calm => BASE_RAID_PERIOD * 3 / 2,
            Intensity::Normal => BASE_RAID_PERIOD,
            Intensity::Harsh => BASE_RAID_PERIOD / 2,
        }
    }

    /// Basis-point multiplier applied to every gauge gain.
    fn gain_scale_bp(self) -> i32 {
        match self {
            Intensity::Calm => 5_000,
            Intensity::Normal => 10_000,
            Intensity::Harsh => 20_000,
        }
    }
}

/// The layered pressures (§13/§17). The discriminant indexes the gauges. The
/// fourth, **Incursion**, is the far-side endgame threat — dormant until transit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureKind {
    FactionWar = 0,
    Piracy = 1,
    Scarcity = 2,
    Incursion = 3,
}

/// The pressure/tension/pacing model (§13). Owns the raid schedule, the forecast
/// telegraph, the pacing governor, and the three decaying gauges.
#[derive(Clone, Debug)]
pub struct PressureSystem {
    intensity: Intensity,
    /// Gauges per [`PressureKind`], `0..=LEVEL_MAX`.
    levels: [i32; 4],
    /// Tick the next raid window opens.
    next_raid: u64,
    /// Whether the upcoming raid has already been forecast.
    forecast_sent: bool,
    /// Tick of the most recent flashpoint, for the governor.
    last_flashpoint: Option<u64>,
    // ---- far-side endgame (§17, G4) — all dormant until `endgame` is set ----
    /// Whether the far-side incursion layer is live (set at gate transit).
    endgame: bool,
    /// The tick the player transited — the escalation clock's zero.
    beyond_start: u64,
    /// Tick the next incursion lands.
    next_incursion: u64,
    /// Whether the upcoming incursion has been telegraphed.
    incursion_forecast_sent: bool,
}

impl PressureSystem {
    pub fn new(intensity: Intensity) -> Self {
        Self {
            intensity,
            levels: [0; 4],
            next_raid: intensity.raid_period(),
            forecast_sent: false,
            last_flashpoint: None,
            endgame: false,
            beyond_start: 0,
            next_incursion: 0,
            incursion_forecast_sent: false,
        }
    }

    pub fn intensity(&self) -> Intensity {
        self.intensity
    }

    /// Retune the difficulty knob live (§13). Re-bases the next raid window from
    /// the current schedule so a mid-run change takes effect without a double-fire.
    pub fn set_intensity(&mut self, intensity: Intensity) {
        self.intensity = intensity;
    }

    /// Current gauge for `kind` (`0..=LEVEL_MAX`).
    pub fn level(&self, kind: PressureKind) -> i32 {
        self.levels[kind as usize]
    }

    /// Ease a gauge (a player answering a threat — Phase A raid dilemma). Floored at 0.
    pub fn relieve(&mut self, kind: PressureKind, amount: i32) {
        let l = &mut self.levels[kind as usize];
        *l = (*l - amount).max(0);
    }

    /// The loudest gauge — the shell's overall threat read (§23c audio state).
    pub fn peak_level(&self) -> i32 {
        *self.levels.iter().max().unwrap_or(&0)
    }

    /// Tick the next raid window opens.
    pub fn next_raid(&self) -> u64 {
        self.next_raid
    }

    /// Forecasting (§13): should the upcoming raid be telegraphed this tick? True
    /// once we come within [`FORECAST_LEAD`] of the raid and haven't told the
    /// player yet.
    pub fn should_forecast(&self, now: u64) -> bool {
        !self.forecast_sent && now + FORECAST_LEAD >= self.next_raid
    }

    /// Record that this tick's forecast was emitted (so it fires once per window).
    pub fn mark_forecast_sent(&mut self) {
        self.forecast_sent = true;
    }

    /// Ticks until the forecast raid strikes (for the telegraph message).
    pub fn raid_eta(&self, now: u64) -> u64 {
        self.next_raid.saturating_sub(now)
    }

    /// Pacing governor (§13): the raid is due *and* no flashpoint fired within the
    /// cooldown, so two acute spikes never dogpile. A due-but-blocked raid is
    /// **deferred**, not skipped — it fires as soon as the window clears.
    pub fn raid_ready(&self, now: u64) -> bool {
        now >= self.next_raid && self.clear_of_recent_flashpoint(now)
    }

    fn clear_of_recent_flashpoint(&self, now: u64) -> bool {
        match self.last_flashpoint {
            Some(t) => now.saturating_sub(t) >= PACING_COOLDOWN,
            None => true,
        }
    }

    /// Called after a raid window resolved: a landed strike is itself a flashpoint;
    /// the next window is scheduled either way and the forecast re-arms.
    pub fn after_raid(&mut self, now: u64, struck: bool) {
        if struck {
            self.mark_flashpoint(now);
        }
        self.next_raid = now + self.intensity.raid_period();
        self.forecast_sent = false;
    }

    /// Mark `now` as a flashpoint for the governor (an acute spike just landed).
    pub fn mark_flashpoint(&mut self, now: u64) {
        self.last_flashpoint = Some(now);
    }

    // ---- the far-side endgame: incursions (§17, G4) ----

    /// Light the incursion layer at the transit tick (§17). Idempotent — a second
    /// call (e.g. a reload that re-asserts the endgame) leaves the clock alone.
    pub fn begin_endgame(&mut self, now: u64) {
        if !self.endgame {
            self.endgame = true;
            self.beyond_start = now;
            self.next_incursion = now + BASE_INCURSION_PERIOD;
            self.incursion_forecast_sent = false;
        }
    }

    /// Whether the far-side incursion layer is live (post-transit).
    pub fn endgame(&self) -> bool {
        self.endgame
    }

    /// Incursion cadence right now — tightens as the player holds the far side
    /// (§17 escalation), floored at [`INCURSION_MIN_PERIOD`].
    fn incursion_period(&self, now: u64) -> u64 {
        let steps = now.saturating_sub(self.beyond_start) / INCURSION_RAMP;
        BASE_INCURSION_PERIOD
            .saturating_sub(steps * INCURSION_PERIOD_STEP)
            .max(INCURSION_MIN_PERIOD)
    }

    /// The strength of an incursion arriving at `now` (bridgehead damage on a
    /// failed defense, and the raider-pack size) — climbs with time-in-Beyond.
    pub fn incursion_severity(&self, now: u64) -> i64 {
        let steps = (now.saturating_sub(self.beyond_start) / INCURSION_RAMP) as i64;
        BASE_INCURSION_SEVERITY + steps * INCURSION_SEVERITY_STEP
    }

    /// Telegraph an incoming incursion (§13 forecasting carried into the endgame).
    pub fn should_forecast_incursion(&self, now: u64) -> bool {
        self.endgame && !self.incursion_forecast_sent && now + FORECAST_LEAD >= self.next_incursion
    }

    pub fn mark_incursion_forecast_sent(&mut self) {
        self.incursion_forecast_sent = true;
    }

    /// Ticks until the forecast incursion strikes.
    pub fn incursion_eta(&self, now: u64) -> u64 {
        self.next_incursion.saturating_sub(now)
    }

    /// The incursion is due (only ever in the endgame). No governor — the far side
    /// does not wait its turn (§17); the pacing dial is the cadence itself.
    pub fn incursion_ready(&self, now: u64) -> bool {
        self.endgame && now >= self.next_incursion
    }

    /// Called after an incursion lands: raise the Incursion gauge and schedule the
    /// next at the (escalating) cadence; re-arm the telegraph.
    pub fn after_incursion(&mut self, now: u64) {
        self.raise(PressureKind::Incursion, INCURSION_GAIN);
        self.next_incursion = now + self.incursion_period(now);
        self.incursion_forecast_sent = false;
    }

    /// Feed the gauges from the world event stream and record acute flashpoints.
    /// Scarcity and lost battles are flashpoints the governor honors; routine
    /// events are ignored.
    pub fn note_event(&mut self, event: &Event, now: u64) {
        match event {
            Event::HaulerInterdicted { .. } => self.raise(PressureKind::Piracy, RAID_GAIN),
            Event::Scarcity { .. } => {
                self.raise(PressureKind::Scarcity, SCARCITY_GAIN);
                self.mark_flashpoint(now);
            }
            Event::BattleResolved { won, .. } => {
                self.raise(PressureKind::FactionWar, WAR_GAIN);
                if !won {
                    self.mark_flashpoint(now);
                }
            }
            _ => {}
        }
    }

    fn raise(&mut self, kind: PressureKind, by: i32) {
        let scaled = by * self.intensity.gain_scale_bp() / BP;
        let v = &mut self.levels[kind as usize];
        *v = (*v + scaled).min(LEVEL_MAX);
    }

    /// Biting-but-recoverable (§13): every gauge ebbs one notch per tick, so a
    /// quiet stretch heals the world while a sustained assault outruns the drift.
    pub fn decay(&mut self) {
        for v in &mut self.levels {
            *v = (*v - LEVEL_DECAY).max(0);
        }
    }
}

impl Default for PressureSystem {
    fn default() -> Self {
        Self::new(Intensity::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forecast_precedes_the_raid_by_the_lead() {
        let p = PressureSystem::new(Intensity::Normal);
        let raid = p.next_raid();
        // Not yet telegraphed well before the window.
        assert!(!p.should_forecast(raid - FORECAST_LEAD - 1));
        // Telegraphed exactly FORECAST_LEAD ahead, with the right eta.
        assert!(p.should_forecast(raid - FORECAST_LEAD));
        assert_eq!(p.raid_eta(raid - FORECAST_LEAD), FORECAST_LEAD);
    }

    #[test]
    fn forecast_fires_once_per_window() {
        let mut p = PressureSystem::new(Intensity::Normal);
        let raid = p.next_raid();
        assert!(p.should_forecast(raid - 1));
        p.mark_forecast_sent();
        assert!(
            !p.should_forecast(raid - 1),
            "already telegraphed this window"
        );
        // After the raid resolves the telegraph re-arms for the next window.
        p.after_raid(raid, true);
        assert!(p.next_raid() > raid);
        assert!(!p.should_forecast(p.next_raid() - FORECAST_LEAD - 1));
        assert!(p.should_forecast(p.next_raid() - FORECAST_LEAD));
    }

    #[test]
    fn the_governor_defers_a_raid_that_would_dogpile_a_flashpoint() {
        let mut p = PressureSystem::new(Intensity::Normal);
        let raid = p.next_raid();
        // A scarcity flashpoint lands just before the raid window.
        p.note_event(
            &Event::Scarcity {
                market: 0,
                commodity: 0,
            },
            raid - 1,
        );
        // The raid is due but governed off — the crisis isn't allowed to dogpile.
        assert!(!p.raid_ready(raid));
        // It is deferred, not skipped: once the cooldown clears, it fires.
        assert!(!p.raid_ready(raid - 1 + PACING_COOLDOWN - 1));
        assert!(p.raid_ready(raid - 1 + PACING_COOLDOWN));
    }

    #[test]
    fn an_undisturbed_raid_fires_on_schedule() {
        let p = PressureSystem::new(Intensity::Normal);
        assert!(!p.raid_ready(p.next_raid() - 1));
        assert!(p.raid_ready(p.next_raid()));
    }

    #[test]
    fn gauges_rise_on_events_and_ebb_when_quiet() {
        let mut p = PressureSystem::new(Intensity::Normal);
        p.note_event(&Event::HaulerInterdicted { id: 1 }, 10);
        let raised = p.level(PressureKind::Piracy);
        assert!(raised > 0);
        // A quiet stretch heals it back toward zero (recoverable).
        for _ in 0..raised {
            p.decay();
        }
        assert_eq!(p.level(PressureKind::Piracy), 0);
    }

    #[test]
    fn sustained_assault_outruns_the_drift() {
        let mut p = PressureSystem::new(Intensity::Normal);
        // One raid per tick swamps the 1/tick decay — the gauge climbs and pins
        // at the ceiling (the loop ends on a decay, so one notch below the cap).
        for t in 0..200 {
            p.note_event(&Event::HaulerInterdicted { id: t }, t);
            p.decay();
        }
        assert!(p.level(PressureKind::Piracy) >= LEVEL_MAX - LEVEL_DECAY);
    }

    #[test]
    fn intensity_scales_cadence_and_gains() {
        // Harsh raids come at least twice as often as Calm.
        let calm = PressureSystem::new(Intensity::Calm);
        let harsh = PressureSystem::new(Intensity::Harsh);
        assert!(calm.next_raid() >= harsh.next_raid() * 2);

        // ...and push the gauges harder for the same event.
        let mut c = PressureSystem::new(Intensity::Calm);
        let mut h = PressureSystem::new(Intensity::Harsh);
        c.note_event(
            &Event::Scarcity {
                market: 0,
                commodity: 0,
            },
            1,
        );
        h.note_event(
            &Event::Scarcity {
                market: 0,
                commodity: 0,
            },
            1,
        );
        assert!(h.level(PressureKind::Scarcity) > c.level(PressureKind::Scarcity));
    }

    #[test]
    fn incursions_are_dormant_until_the_endgame() {
        // Pre-transit the incursion layer is entirely off — no forecast, no strike,
        // the gauge never moves (so the pre-transit world is byte-identical, §17).
        let p = PressureSystem::new(Intensity::Normal);
        assert!(!p.endgame());
        for t in 0..1000 {
            assert!(!p.should_forecast_incursion(t));
            assert!(!p.incursion_ready(t));
        }
        assert_eq!(p.level(PressureKind::Incursion), 0);
    }

    #[test]
    fn incursions_fire_and_escalate_after_transit() {
        let mut p = PressureSystem::new(Intensity::Normal);
        p.begin_endgame(0);
        assert!(p.endgame());
        let first = p.next_incursion;
        assert_eq!(first, BASE_INCURSION_PERIOD);
        // Telegraphed ahead, then ready on schedule.
        assert!(p.should_forecast_incursion(first - FORECAST_LEAD));
        assert!(!p.incursion_ready(first - 1));
        assert!(p.incursion_ready(first));
        let early_sev = p.incursion_severity(first);
        // Drive a long run of incursions; the cadence tightens and severity climbs.
        let mut now = first;
        let mut prev_gap = BASE_INCURSION_PERIOD;
        for _ in 0..30 {
            assert!(p.incursion_ready(now));
            p.after_incursion(now);
            let gap = p.next_incursion - now;
            assert!(gap <= prev_gap, "cadence never loosens");
            assert!(gap >= INCURSION_MIN_PERIOD, "cadence floored");
            prev_gap = gap;
            now = p.next_incursion;
        }
        assert!(
            p.incursion_severity(now) > early_sev,
            "severity escalates with time held"
        );
        assert!(p.level(PressureKind::Incursion) > 0);
    }

    #[test]
    fn begin_endgame_is_idempotent() {
        let mut p = PressureSystem::new(Intensity::Normal);
        p.begin_endgame(100);
        let scheduled = p.next_incursion;
        p.begin_endgame(500); // a reload re-asserting the endgame must not reset the clock
        assert_eq!(p.next_incursion, scheduled);
    }

    #[test]
    fn the_schedule_is_deterministic() {
        // No RNG, no clock: two systems driven identically stay bit-identical.
        let mut a = PressureSystem::new(Intensity::Normal);
        let mut b = PressureSystem::new(Intensity::Normal);
        for t in 0..500 {
            if a.should_forecast(t) {
                a.mark_forecast_sent();
            }
            if b.should_forecast(t) {
                b.mark_forecast_sent();
            }
            if a.raid_ready(t) {
                a.after_raid(t, true);
            }
            if b.raid_ready(t) {
                b.after_raid(t, true);
            }
            a.decay();
            b.decay();
        }
        assert_eq!(a.levels, b.levels);
        assert_eq!(a.next_raid(), b.next_raid());
    }
}
