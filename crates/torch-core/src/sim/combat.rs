//! Headless range-band combat resolver (§9, §35 step 7).
//!
//! Doctrine-first and lethal: two fleets meet at an **abstract range band** and
//! trade fire each tick until one side is gone. The three weapon systems play
//! their §8a roles — **railguns** are the capital hull-killers (best at range),
//! **PDC** is the close-band brawl + the anti-torpedo **screen**, and
//! **torpedoes** are the equalizer: launched in salvos that must *saturate* the
//! screen, so enough light hulls can threaten a capital. Integer/deterministic
//! (§27); it consumes the fitted [`Loadout`] stats from §8 and emits a
//! BattleLog-style event stream (§29) for the future diorama (§22).

use super::rng::Pcg32;
use super::ships::{Loadout, WeaponKind};

/// Basis-point denominator.
const BP: i64 = 10_000;
/// Hard cap on battle length; reaching it is a stalemate (draw).
const MAX_TICKS: u64 = 4_000;
/// Opening-exchange bonus to the side that wins initiative, in basis points (§9):
/// enough to decide an otherwise-even fight, far too little to overturn a real
/// force advantage.
const INITIATIVE_BONUS_BP: i64 = 6_000;
/// Structure granted per unit of hull dry mass (armor adds on top).
const MASS_TO_HP: i64 = 10;
/// Torpedo shots stored per tube.
const MAG_PER_TUBE: i64 = 10;
/// Divisor turning raw PDC `intercept` into torpedoes stopped per salvo.
const SCREEN_DIVISOR: i64 = 5;

/// Abstract engagement range (§9). The doctrine picks it; the faster fleet wins
/// the say (it controls the range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Close,
    Medium,
    Long,
}

impl Band {
    /// Railgun firing-solution quality at this band, basis points (§8a: needs a
    /// solution — best at range, poor knife-fighting).
    fn railgun_bp(self) -> i64 {
        match self {
            Band::Close => 1_500,
            Band::Medium => 5_500,
            Band::Long => 9_000,
        }
    }

    /// Whether PDCs add their close-band direct damage here (§8a).
    fn pdc_brawl(self) -> bool {
        matches!(self, Band::Close)
    }

    /// Fraction of the defender screen that bites against a salvo, basis points:
    /// long crossings give PDC more time, close-in rushes less (saturation lever).
    fn intercept_bp(self) -> i64 {
        match self {
            Band::Close => 4_000,
            Band::Medium => 7_000,
            Band::Long => 10_000,
        }
    }
}

/// Which enemy to focus (§9 target priority).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPriority {
    /// Concentrate on the largest hull (crack the capital).
    Biggest,
    /// Finish the most-wounded hull first.
    Weakest,
}

/// A fleet's standing orders (§9). Live tactical commands come later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Doctrine {
    pub band: Band,
    pub salvo_reload: u64,
    pub target: TargetPriority,
    /// Retreat threshold in basis points (§9): a fleet **disengages** (keeping its
    /// survivors, conceding the field) the moment its strength drops below this
    /// fraction of its starting count. `0` = fight to the death (the cautious vs.
    /// committed tactical dial). The retreating fleet survives but loses the battle.
    pub retreat_bp: i64,
    /// Fire railguns **hot** (§8a/§9 heat discipline): aggressive fire boosts
    /// railgun output by [`AGGRESSIVE_FIRE_BP`] but builds heat; over the radiator
    /// ceiling the fleet must **vent** — skipping a railgun volley to shed heat
    /// (a `CombatEvent::Overheat`). `false` (default) = disciplined fire, no heat,
    /// so a default fight is byte-identical to before this knob existed.
    pub aggressive_fire: bool,
}

impl Default for Doctrine {
    fn default() -> Self {
        Self {
            band: Band::Medium,
            salvo_reload: 6,
            target: TargetPriority::Biggest,
            retreat_bp: 0,
            aggressive_fire: false,
        }
    }
}

/// Railgun damage multiplier (basis points) when firing **aggressively** (§9 heat):
/// 30% more alpha, paid for with heat that periodically forces a vent.
const AGGRESSIVE_FIRE_BP: i64 = 13_000;
/// Heat one railgun adds per aggressive volley.
const HEAT_PER_RAILGUN: i64 = 10;
/// Radiator capacity per surviving ship — over this a fleet must vent.
const HEAT_CEILING_PER_SHIP: i64 = 24;
/// Heat shed each tick by radiators (and the extra dumped on a vent).
const HEAT_DISSIPATION: i64 = 6;

/// A fleet entering the engagement.
pub struct Fleet<'a> {
    pub ships: &'a [Loadout],
    pub doctrine: Doctrine,
}

/// BattleLog-style events (§29) the diorama and alert feed will consume.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CombatEvent {
    Salvo {
        side: usize,
        torpedoes: i64,
        leakers: i64,
    },
    Volley {
        side: usize,
        damage: i64,
    },
    Destroyed {
        side: usize,
        name: String,
    },
    /// A fleet broke off below its retreat threshold (§9), keeping its survivors.
    Retreat {
        side: usize,
    },
    /// A fleet firing aggressively hit its heat ceiling and **vented** (§8a/§9),
    /// skipping a railgun volley to shed heat.
    Overheat {
        side: usize,
    },
}

/// The result of a resolved battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleOutcome {
    /// Winning side index, or `None` for a stalemate.
    pub winner: Option<usize>,
    pub ticks: u64,
    /// Surviving ship counts `[side0, side1]`.
    pub survivors: [usize; 2],
    pub log: Vec<CombatEvent>,
}

/// One ship's combat-runtime state, distilled from its fit (§8).
struct Ship {
    name: String,
    side: usize,
    hp: i64,
    /// Torpedoes stopped per salvo (crew-scaled).
    screen: i64,
    /// Close-band PDC direct damage (crew-scaled).
    pdc_dmg: i64,
    /// Railgun hull damage if every railgun lands (crew-scaled).
    railgun_volley: i64,
    /// Number of railgun mounts (heat source for aggressive fire, §9).
    railguns: i64,
    tubes: i64,
    mag: i64,
    /// Torpedo warhead per leaker (crew-scaled).
    torp_dmg: i64,
    mobility: i64,
    alive: bool,
}

impl Ship {
    fn build(loadout: &Loadout, side: usize) -> Self {
        let hull = loadout.hull();
        let stats = loadout.stats();
        // Crew quality is the §8c payoff: 50 ⇒ ×1.0, scaling offense and screen.
        let q = |v: i64| v * (50 + loadout.crew().quality) / 100;
        let pdc_dmg = loadout
            .weapons()
            .iter()
            .filter(|w| w.kind == WeaponKind::Pdc)
            .map(|w| w.damage)
            .sum();
        let railgun_volley = loadout
            .weapons()
            .iter()
            .filter(|w| w.kind == WeaponKind::Railgun)
            .map(|w| w.damage)
            .sum();
        let tubes = loadout
            .weapons()
            .iter()
            .filter(|w| w.kind == WeaponKind::Torpedo)
            .count() as i64;
        let torp_dmg = super::ships::weapon(WeaponKind::Torpedo).damage;
        Self {
            name: hull.name.to_string(),
            side,
            hp: hull.armor + hull.dry_mass / MASS_TO_HP,
            screen: q(stats.pdc_screen) / SCREEN_DIVISOR,
            pdc_dmg: q(pdc_dmg),
            railgun_volley: q(railgun_volley),
            railguns: stats.railguns as i64,
            tubes,
            mag: tubes * MAG_PER_TUBE,
            torp_dmg: q(torp_dmg),
            mobility: stats.thrust_to_mass,
            alive: true,
        }
    }
}

/// The faster fleet dictates the range; ties go to side 0 (the attacker).
fn negotiate_band(ships: &[Ship], a: Doctrine, b: Doctrine) -> Band {
    let mob: [i64; 2] = ships.iter().fold([0, 0], |mut m, s| {
        m[s.side] += s.mobility;
        m
    });
    if mob[0] >= mob[1] {
        a.band
    } else {
        b.band
    }
}

/// Pick a target index on `side` by the attacker's priority.
fn pick_target(ships: &[Ship], side: usize, priority: TargetPriority) -> Option<usize> {
    ships
        .iter()
        .enumerate()
        .filter(|(_, s)| s.alive && s.side == side)
        .min_by_key(|(_, s)| match priority {
            TargetPriority::Biggest => -s.hp,
            TargetPriority::Weakest => s.hp,
        })
        .map(|(i, _)| i)
}

/// Focus-fire `damage` onto `side`, overflowing to the next target on a kill.
fn apply_damage(
    ships: &mut [Ship],
    side: usize,
    mut damage: i64,
    priority: TargetPriority,
    log: &mut Vec<CombatEvent>,
) {
    while damage > 0 {
        let Some(t) = pick_target(ships, side, priority) else {
            break;
        };
        let ship = &mut ships[t];
        if damage < ship.hp {
            ship.hp -= damage;
            break;
        }
        damage -= ship.hp;
        ship.hp = 0;
        ship.alive = false;
        log.push(CombatEvent::Destroyed {
            side,
            name: ship.name.clone(),
        });
    }
}

/// Damage side `s` deals this tick (railguns + close PDC brawl), with a small
/// deterministic jitter, plus the torpedo salvo handled separately. Threads the
/// side's `heat` state so **aggressive fire** (§9) can boost railgun output and
/// vent over the ceiling. With `aggressive_fire == false` (the default), the heat
/// branch is skipped and `heat` stays 0, so the result is byte-identical to before.
fn volley_damage(
    ships: &[Ship],
    side: usize,
    band: Band,
    doctrine: Doctrine,
    heat: &mut i64,
    rng: &mut Pcg32,
    log: &mut Vec<CombatEvent>,
) -> i64 {
    // Railgun output this tick (and its mount count, the heat source).
    let mut railgun = 0;
    let mut railgun_mounts = 0;
    for s in ships.iter().filter(|s| s.alive && s.side == side) {
        railgun += s.railgun_volley * band.railgun_bp() / BP;
        railgun_mounts += s.railguns;
    }
    // Aggressive fire (§8a/§9 heat discipline): more alpha, paid for with heat —
    // and a vent (skipped volley) once the radiators saturate.
    if doctrine.aggressive_fire && railgun > 0 {
        let surv = ships.iter().filter(|s| s.alive && s.side == side).count() as i64;
        let ceiling = HEAT_CEILING_PER_SHIP * surv.max(1);
        if *heat > ceiling {
            railgun = 0; // vent — hold fire to shed heat
            log.push(CombatEvent::Overheat { side });
            *heat = (*heat - HEAT_DISSIPATION * 2).max(0);
        } else {
            railgun = railgun * AGGRESSIVE_FIRE_BP / BP;
            *heat += HEAT_PER_RAILGUN * railgun_mounts;
        }
    }
    // Close-band PDC brawl.
    let mut pdc = 0;
    if band.pdc_brawl() {
        for s in ships.iter().filter(|s| s.alive && s.side == side) {
            pdc += s.pdc_dmg;
        }
    }
    // Radiators shed a little heat every tick.
    *heat = (*heat - HEAT_DISSIPATION).max(0);
    let dmg = railgun + pdc;
    if dmg == 0 {
        return 0;
    }
    // ±12% jitter so engagements aren't perfectly mechanical (§27 integer rng).
    let jitter = rng.below(2_401) as i64 - 1_200; // [-1200, 1200] bp
    dmg + dmg * jitter / BP
}

/// Launch side `s`'s torpedo salvo and return the leaker damage that gets
/// through the enemy screen (§8a saturation).
fn salvo_damage(ships: &mut [Ship], side: usize, band: Band, log: &mut Vec<CombatEvent>) -> i64 {
    let enemy = 1 - side;
    let mut torpedoes = 0;
    for s in ships
        .iter_mut()
        .filter(|s| s.alive && s.side == side && s.mag > 0)
    {
        let fire = s.tubes.min(s.mag);
        s.mag -= fire;
        torpedoes += fire;
    }
    if torpedoes == 0 {
        return 0;
    }
    let screen: i64 = ships
        .iter()
        .filter(|s| s.alive && s.side == enemy)
        .map(|s| s.screen)
        .sum::<i64>()
        * band.intercept_bp()
        / BP;
    let leakers = (torpedoes - screen).max(0);
    let warhead = ships
        .iter()
        .find(|s| s.alive && s.side == side)
        .map(|s| s.torp_dmg)
        .unwrap_or(0);
    log.push(CombatEvent::Salvo {
        side,
        torpedoes,
        leakers,
    });
    leakers * warhead
}

/// Resolve a battle between two fleets to the death (§9). Deterministic per seed.
pub fn resolve(a: &Fleet, b: &Fleet, rng: &mut Pcg32) -> BattleOutcome {
    let mut ships: Vec<Ship> = Vec::new();
    for l in a.ships {
        ships.push(Ship::build(l, 0));
    }
    for l in b.ships {
        ships.push(Ship::build(l, 1));
    }
    let band = negotiate_band(&ships, a.doctrine, b.doctrine);
    let doctrine = [a.doctrine, b.doctrine];
    // Both fleets open with a salvo on tick 1, then reload on their cadence.
    let mut reload = [0u64, 0u64];
    let mut heat = [0i64, 0i64]; // §9 railgun heat, per side (0 unless aggressive)
    let mut log = Vec::new();

    let alive_on = |ships: &[Ship], side: usize| ships.iter().any(|s| s.alive && s.side == side);

    // Initiative: one side wins the opening exchange (the ambush / better firing
    // solution, §9). Without it the resolver is a deterministic force-ratio
    // curbstomp — matched fleets always mutually annihilate; with it, an evenly
    // matched fight is a real coin-flip while a force advantage still decides.
    let initiative = rng.below(2) as usize;

    let initial = [a.ships.len(), b.ships.len()];
    let mut retreated: Option<usize> = None;
    let mut ticks = 0;
    while ticks < MAX_TICKS && alive_on(&ships, 0) && alive_on(&ships, 1) {
        // A fleet below its retreat threshold breaks off *before* the next round
        // (§9), so it keeps the survivors it has rather than being wiped this tick.
        for side in 0..2 {
            let surv = ships.iter().filter(|s| s.alive && s.side == side).count();
            if doctrine[side].retreat_bp > 0
                && surv > 0
                && initial[side] > 0
                && (surv as i64 * BP / initial[side] as i64) < doctrine[side].retreat_bp
            {
                retreated = Some(side);
            }
        }
        if let Some(side) = retreated {
            log.push(CombatEvent::Retreat { side });
            break;
        }
        ticks += 1;
        // Both sides' damage is computed on the start-of-tick living set, then
        // applied together — no within-tick ordering bias.
        let mut dealt = [0i64; 2];
        for side in 0..2 {
            dealt[side] += volley_damage(
                &ships,
                side,
                band,
                doctrine[side],
                &mut heat[side],
                rng,
                &mut log,
            );
            if reload[side] == 0 {
                dealt[side] += salvo_damage(&mut ships, side, band, &mut log);
                reload[side] = doctrine[side].salvo_reload;
            } else {
                reload[side] -= 1;
            }
        }
        if ticks == 1 {
            dealt[initiative] += dealt[initiative] * INITIATIVE_BONUS_BP / BP;
        }
        for side in 0..2 {
            if dealt[side] > 0 {
                log.push(CombatEvent::Volley {
                    side,
                    damage: dealt[side],
                });
                apply_damage(
                    &mut ships,
                    1 - side,
                    dealt[side],
                    doctrine[side].target,
                    &mut log,
                );
            }
        }
    }

    let survivors = [
        ships.iter().filter(|s| s.alive && s.side == 0).count(),
        ships.iter().filter(|s| s.alive && s.side == 1).count(),
    ];
    // A retreating fleet concedes the field even with survivors left.
    let winner = match retreated {
        Some(side) => Some(1 - side),
        None => match (survivors[0] > 0, survivors[1] > 0) {
            (true, false) => Some(0),
            (false, true) => Some(1),
            _ => None,
        },
    };
    BattleOutcome {
        winner,
        ticks,
        survivors,
        log,
    }
}

/// Build `n` torpedo frigates of the given crew quality.
fn frigate_wing(n: usize, quality: i64, rng: &mut Pcg32) -> Vec<Loadout> {
    use super::ships::{hull, weapon, Crew, ShipClass, WeaponKind};
    let h = hull(ShipClass::Frigate);
    (0..n)
        .map(|_| {
            let w = vec![
                weapon(WeaponKind::Pdc),
                weapon(WeaponKind::Pdc),
                weapon(WeaponKind::Torpedo),
                weapon(WeaponKind::Torpedo),
            ];
            Loadout::fit(
                h.clone(),
                w,
                400,
                Crew::recruit(rng, h.crew_required, quality),
            )
            .unwrap()
        })
        .collect()
}

/// Build a fully-armed battleship of the given crew quality.
fn lone_battleship(quality: i64, rng: &mut Pcg32) -> Vec<Loadout> {
    use super::ships::{hull, weapon, Crew, ShipClass, WeaponKind};
    let h = hull(ShipClass::Battleship);
    let mut w = vec![weapon(WeaponKind::Pdc); 6];
    w.extend(vec![weapon(WeaponKind::Torpedo); 4]);
    w.extend(vec![weapon(WeaponKind::Railgun); 2]);
    let crew = Crew::recruit(rng, h.crew_required, quality);
    vec![Loadout::fit(h.clone(), w, h.remass_capacity, crew).unwrap()]
}

/// A reference engagement (the shell's combat demo): `n` torpedo frigates versus
/// one battleship at `band`. Shows the §8a/§8f tension live.
pub fn demo_duel(n_frigates: usize, band: Band, seed: u64) -> BattleOutcome {
    let mut rng = Pcg32::new(seed);
    let frigs = frigate_wing(n_frigates, 50, &mut rng);
    let bs = lone_battleship(50, &mut rng);
    let doctrine = Doctrine {
        band,
        ..Doctrine::default()
    };
    resolve(
        &Fleet {
            ships: &frigs,
            doctrine,
        },
        &Fleet {
            ships: &bs,
            doctrine,
        },
        &mut rng,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::ships::{hull, weapon, Crew, Loadout, ShipClass, WeaponKind};

    fn doctrine(band: Band) -> Doctrine {
        Doctrine {
            band,
            ..Doctrine::default()
        }
    }

    #[test]
    fn a_fleet_retreats_below_its_threshold_keeping_survivors() {
        // An overwhelmed fleet with a retreat threshold breaks off (§9), conceding
        // the field but saving its survivors — the cautious tactical dial.
        let mut rng = Pcg32::new(5);
        let cautious_wing = frigate_wing(10, 50, &mut rng);
        let foe = frigate_wing(13, 50, &mut rng);
        let cautious = Doctrine {
            band: Band::Close,
            retreat_bp: 9_000, // skittish — break off after the first loss
            ..Doctrine::default()
        };
        let out = resolve(
            &Fleet {
                ships: &cautious_wing,
                doctrine: cautious,
            },
            &Fleet {
                ships: &foe,
                doctrine: doctrine(Band::Close),
            },
            &mut rng,
        );
        assert_eq!(
            out.winner,
            Some(1),
            "the overwhelmed side concedes the field"
        );
        assert!(
            out.survivors[0] > 0,
            "but retreats with survivors, not annihilated"
        );
        assert!(
            out.log
                .iter()
                .any(|e| matches!(e, CombatEvent::Retreat { side: 0 })),
            "the retreat is logged for the diorama"
        );
    }

    fn duel(n: usize, band: Band, seed: u64) -> BattleOutcome {
        let mut rng = Pcg32::new(seed);
        let frigs = frigate_wing(n, 50, &mut rng);
        let bs = lone_battleship(50, &mut rng);
        resolve(
            &Fleet {
                ships: &frigs,
                doctrine: doctrine(band),
            },
            &Fleet {
                ships: &bs,
                doctrine: doctrine(band),
            },
            &mut rng,
        )
    }

    #[test]
    fn aggressive_fire_eventually_vents_in_a_prolonged_fight() {
        // §9 heat discipline: combat is decisive (§13), so in a quick fight firing
        // hot is pure upside — but a *prolonged* engagement (a squadron grinding
        // through a big swarm) saturates the radiators and forces a vent (Overheat,
        // a diorama beat). A capital squadron vs a large frigate swarm at Long range
        // drags long enough for the heat to bite.
        let mut rng = Pcg32::new(3);
        let mut squadron = lone_battleship(50, &mut rng);
        squadron.extend(lone_battleship(50, &mut rng));
        squadron.extend(lone_battleship(50, &mut rng));
        let swarm = frigate_wing(40, 50, &mut rng);
        let hot = Doctrine {
            band: Band::Long,
            aggressive_fire: true,
            ..Doctrine::default()
        };
        let out = resolve(
            &Fleet {
                ships: &squadron,
                doctrine: hot,
            },
            &Fleet {
                ships: &swarm,
                doctrine: doctrine(Band::Long),
            },
            &mut rng,
        );
        assert!(
            out.log
                .iter()
                .any(|e| matches!(e, CombatEvent::Overheat { side: 0 })),
            "sustained aggressive railgun fire eventually vents"
        );
    }

    #[test]
    fn disciplined_fire_never_overheats() {
        // The default doctrine builds no heat, so it never vents — and a default
        // fight is byte-identical to before the knob existed (asserted by the wider
        // suite + the byte-identical QA review).
        let out = duel(8, Band::Close, 1);
        assert!(
            !out.log
                .iter()
                .any(|e| matches!(e, CombatEvent::Overheat { .. })),
            "disciplined fire produces no heat vents"
        );
    }

    /// Seeds swept per scenario — enough to prove an outcome is decisive, not a
    /// lucky roll, while staying fast.
    const DUEL_SEEDS: u64 = 16;

    /// A frigate-wing-vs-battleship matchup and the outcome it must yield on every
    /// seed: which side wins, and whether the loser is wiped out.
    struct DuelCase {
        wing: usize,
        band: Band,
        winner: usize,
        loser_wiped: bool,
        why: &'static str,
    }

    #[test]
    fn range_band_decides_the_frigate_wing_vs_battleship_duel() {
        let cases = [
            DuelCase {
                wing: 1,
                band: Band::Close,
                winner: 1,
                loser_wiped: true,
                why: "a lone frigate is annihilated by a battleship",
            },
            DuelCase {
                wing: 8,
                band: Band::Close,
                winner: 0,
                loser_wiped: false,
                why: "massed torpedoes saturate the PDC screen up close",
            },
            DuelCase {
                wing: 8,
                band: Band::Long,
                winner: 1,
                loser_wiped: false,
                why: "the battleship holds the line at long range (full screen + reach)",
            },
        ];
        for c in cases {
            for seed in 0..DUEL_SEEDS {
                let out = duel(c.wing, c.band, seed);
                assert_eq!(out.winner, Some(c.winner), "{} (seed {seed})", c.why);
                if c.loser_wiped {
                    let loser = 1 - c.winner;
                    assert_eq!(out.survivors[loser], 0, "lethal: loser wiped (seed {seed})");
                }
            }
        }
    }

    #[test]
    fn veteran_crew_beats_a_green_mirror() {
        let mut rng = Pcg32::new(2);
        let h = hull(ShipClass::Cruiser);
        let arms = || {
            let mut w = vec![weapon(WeaponKind::Pdc); 4];
            w.extend(vec![weapon(WeaponKind::Torpedo); 2]);
            w.push(weapon(WeaponKind::Railgun));
            w
        };
        let vet = vec![Loadout::fit(
            h.clone(),
            arms(),
            h.remass_capacity,
            Crew::recruit(&mut rng, h.crew_required, 90),
        )
        .unwrap()];
        let green = vec![Loadout::fit(
            h.clone(),
            arms(),
            h.remass_capacity,
            Crew::recruit(&mut rng, h.crew_required, 25),
        )
        .unwrap()];
        let out = resolve(
            &Fleet {
                ships: &vet,
                doctrine: doctrine(Band::Medium),
            },
            &Fleet {
                ships: &green,
                doctrine: doctrine(Band::Medium),
            },
            &mut Pcg32::new(3),
        );
        assert_eq!(
            out.winner,
            Some(0),
            "the veteran crew should win the mirror (§8c)"
        );
    }

    #[test]
    fn battles_are_deterministic() {
        let a = duel(6, Band::Medium, 9);
        let b = duel(6, Band::Medium, 9);
        assert_eq!(a, b);
    }

    #[test]
    fn the_battlelog_records_salvos_and_kills() {
        let out = duel(8, Band::Close, 0);
        assert!(out
            .log
            .iter()
            .any(|e| matches!(e, CombatEvent::Salvo { .. })));
        assert!(out
            .log
            .iter()
            .any(|e| matches!(e, CombatEvent::Destroyed { .. })));
    }
}
