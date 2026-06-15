//! The alert feed (§19) — a system, not a panel: the game's voice and pacing.
//!
//! It consumes the world's typed [`Event`] stream (§29) and turns it into
//! **ranked**, **voiced** alerts with a hard **FYI vs act-now** split. Act-now
//! alerts resolve into a verb (§0.4 "exceptions are verbs, not acknowledgments").
//! A player-tunable threshold decides what surfaces, so the feed is neither
//! notification anxiety nor a missed crisis. Deterministic (§27): same events ⇒
//! same feed.

use super::event::Event;
use super::rng::Pcg32;

/// Most alerts the feed retains (a ring buffer; pacing, not a ledger).
const MAX_ALERTS: usize = 64;

/// How loud an alert is. Ordered: `Info < Notice < Warning < Critical`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Info,
    Notice,
    Warning,
    Critical,
}

/// The hard split (§19): something to *know* vs. something to *do now*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Urgency {
    Fyi,
    ActNow,
}

/// The verb an act-now alert resolves into (§0.4). Extended as systems land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    /// A shortage is an opportunity: sell into it, or relieve it.
    ExploitShortage { market: usize, commodity: usize },
}

/// One entry in the feed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub tick: u64,
    pub priority: Priority,
    pub urgency: Urgency,
    /// The manager/captain who voices it (§11 personality).
    pub voice: String,
    pub message: String,
    /// Present iff this is an act-now alert.
    pub verb: Option<Verb>,
}

impl Alert {
    pub fn is_act_now(&self) -> bool {
        self.urgency == Urgency::ActNow
    }
}

/// How a manager phrases things (§11 texture).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    Terse,
    Wry,
}

/// A named character who voices part of the feed (§11). Which desk they speak
/// for is determined by the feed field they sit in (markets vs. security).
struct Manager {
    name: String,
    tone: Tone,
}

const NAMES: [&str; 8] = [
    "Okonkwo", "Reyes", "Sato", "Mwangi", "Vega", "Tan", "Cole", "Ndiaye",
];

impl Manager {
    fn new(rng: &mut Pcg32) -> Self {
        let name = NAMES[rng.below(NAMES.len() as u32) as usize].to_string();
        let tone = if rng.below(2) == 0 {
            Tone::Terse
        } else {
            Tone::Wry
        };
        Self { name, tone }
    }
}

/// The ranked, voiced alert feed.
pub struct AlertFeed {
    min_priority: Priority,
    market_names: Vec<String>,
    commodity_names: Vec<String>,
    markets_mgr: Manager,
    security_mgr: Manager,
    alerts: Vec<Alert>,
}

impl AlertFeed {
    /// Build a feed that can name the world's markets and commodities.
    pub fn new(seed: u64, market_names: Vec<String>, commodity_names: Vec<String>) -> Self {
        let mut rng = Pcg32::new(seed ^ 0xA1E2_7FED);
        Self {
            // Default: surface Notice and louder; Info stays FYI-quiet.
            min_priority: Priority::Notice,
            market_names,
            commodity_names,
            markets_mgr: Manager::new(&mut rng),
            security_mgr: Manager::new(&mut rng),
            alerts: Vec::new(),
        }
    }

    /// Set the player-tunable surfacing threshold (§19).
    pub fn set_threshold(&mut self, min_priority: Priority) {
        self.min_priority = min_priority;
    }

    pub fn threshold(&self) -> Priority {
        self.min_priority
    }

    /// Classify a world event into an alert (or nothing, for routine noise).
    pub fn ingest(&mut self, event: &Event, tick: u64) {
        let alert = match event {
            Event::Scarcity { market, commodity } => Some(self.scarcity(*market, *commodity, tick)),
            Event::HaulerInterdicted { .. } => Some(self.raid(tick)),
            Event::TierAscended { tier } => Some(Self::milestone(tier, tick)),
            // Routine traffic and ticks are not feed-worthy.
            Event::Tick { .. } | Event::HaulerDeparted { .. } | Event::HaulerArrived { .. } => None,
        };
        if let Some(a) = alert {
            self.push(a);
        }
    }

    fn push(&mut self, alert: Alert) {
        self.alerts.push(alert);
        if self.alerts.len() > MAX_ALERTS {
            let overflow = self.alerts.len() - MAX_ALERTS;
            self.alerts.drain(0..overflow);
        }
    }

    fn name_of(names: &[String], i: usize) -> &str {
        names.get(i).map(String::as_str).unwrap_or("?")
    }

    fn scarcity(&self, market: usize, commodity: usize, tick: u64) -> Alert {
        let m = Self::name_of(&self.market_names, market);
        let c = Self::name_of(&self.commodity_names, commodity);
        let mgr = &self.markets_mgr;
        let message = match mgr.tone {
            Tone::Terse => format!("{}: Scarcity — {c} short at {m}.", mgr.name),
            Tone::Wry => format!("{}: {c} just got dear at {m}. Someone's hurting.", mgr.name),
        };
        Alert {
            tick,
            priority: Priority::Warning,
            urgency: Urgency::ActNow,
            voice: mgr.name.clone(),
            message,
            verb: Some(Verb::ExploitShortage { market, commodity }),
        }
    }

    /// A tier ascent — the loudest, most welcome line in the feed (§0.3).
    fn milestone(tier: &str, tick: u64) -> Alert {
        Alert {
            tick,
            priority: Priority::Critical,
            urgency: Urgency::Fyi,
            voice: "The Board".to_string(),
            message: format!("The Board: We've reached {tier}. The ring-gate draws closer."),
            verb: None,
        }
    }

    fn raid(&self, tick: u64) -> Alert {
        let mgr = &self.security_mgr;
        let message = match mgr.tone {
            Tone::Terse => format!("{}: A convoy was cut on the lanes.", mgr.name),
            Tone::Wry => format!(
                "{}: Lost a convoy out there. Pirates eat well today.",
                mgr.name
            ),
        };
        Alert {
            tick,
            priority: Priority::Notice,
            urgency: Urgency::Fyi,
            voice: mgr.name.clone(),
            message,
            verb: None,
        }
    }

    /// All retained alerts, newest first.
    pub fn all(&self) -> impl Iterator<Item = &Alert> {
        self.alerts.iter().rev()
    }

    /// Alerts at or above the threshold, ranked by priority then recency (§19).
    pub fn surfaced(&self) -> Vec<&Alert> {
        let mut out: Vec<&Alert> = self
            .alerts
            .iter()
            .filter(|a| a.priority >= self.min_priority)
            .collect();
        out.sort_by(|x, y| y.priority.cmp(&x.priority).then(y.tick.cmp(&x.tick)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed() -> AlertFeed {
        AlertFeed::new(
            1,
            vec!["Ceres Yards".into(), "Earth Hub".into()],
            vec!["Ice".into(), "Ore".into()],
        )
    }

    #[test]
    fn scarcity_is_an_act_now_alert_with_a_verb() {
        let mut f = feed();
        f.ingest(
            &Event::Scarcity {
                market: 1,
                commodity: 1,
            },
            10,
        );
        let a = f.surfaced()[0];
        assert_eq!(a.priority, Priority::Warning);
        assert!(a.is_act_now());
        assert_eq!(
            a.verb,
            Some(Verb::ExploitShortage {
                market: 1,
                commodity: 1
            })
        );
        assert!(a.message.contains("Ore") && a.message.contains("Earth Hub"));
    }

    #[test]
    fn a_raid_is_an_fyi_notice() {
        let mut f = feed();
        f.ingest(&Event::HaulerInterdicted { id: 3 }, 5);
        let a = f.surfaced()[0];
        assert_eq!(a.priority, Priority::Notice);
        assert!(!a.is_act_now());
        assert_eq!(a.verb, None);
    }

    #[test]
    fn routine_traffic_is_not_feed_worthy() {
        let mut f = feed();
        f.ingest(&Event::Tick { tick: 1 }, 1);
        f.ingest(
            &Event::HaulerDeparted {
                id: 0,
                commodity: 0,
                origin: 0,
                dest: 1,
                qty: 100,
            },
            1,
        );
        f.ingest(&Event::HaulerArrived { id: 0 }, 9);
        assert!(f.all().next().is_none());
    }

    #[test]
    fn threshold_suppresses_quieter_alerts() {
        let mut f = feed();
        f.ingest(&Event::HaulerInterdicted { id: 1 }, 1); // Notice
        f.ingest(
            &Event::Scarcity {
                market: 0,
                commodity: 0,
            },
            2,
        ); // Warning
        assert_eq!(f.surfaced().len(), 2);
        f.set_threshold(Priority::Warning);
        let surfaced = f.surfaced();
        assert_eq!(surfaced.len(), 1);
        assert_eq!(surfaced[0].priority, Priority::Warning);
    }

    #[test]
    fn alerts_rank_loudest_and_newest_first() {
        let mut f = feed();
        f.ingest(&Event::HaulerInterdicted { id: 1 }, 1); // Notice@1
        f.ingest(
            &Event::Scarcity {
                market: 0,
                commodity: 0,
            },
            2,
        ); // Warning@2
        f.ingest(
            &Event::Scarcity {
                market: 1,
                commodity: 1,
            },
            3,
        ); // Warning@3
        let s = f.surfaced();
        assert_eq!(s[0].tick, 3); // newest Warning first
        assert_eq!(s[1].tick, 2);
        assert_eq!(s[2].priority, Priority::Notice); // notice last
    }

    #[test]
    fn the_feed_is_bounded() {
        let mut f = feed();
        for t in 0..(MAX_ALERTS as u64 * 3) {
            f.ingest(
                &Event::Scarcity {
                    market: 0,
                    commodity: 0,
                },
                t,
            );
        }
        assert_eq!(f.all().count(), MAX_ALERTS);
    }

    #[test]
    fn voice_is_deterministic_and_named() {
        let a = AlertFeed::new(7, vec!["M".into()], vec!["C".into()]);
        let b = AlertFeed::new(7, vec!["M".into()], vec!["C".into()]);
        assert_eq!(a.markets_mgr.name, b.markets_mgr.name);
        assert!(!a.markets_mgr.name.is_empty());
    }
}
