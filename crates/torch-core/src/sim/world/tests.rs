//! Unit tests for [`super::Sim`] — the world/tick acceptance suite.

use super::*;
use crate::sim::pressure::Intensity;
use crate::sim::ships::ShipClass;

/// Test helper: stand up a max-tier shipyard directly (no cost) so warship-building
/// tests can build any hull. The shipyard gate (Phase B+) is tested on its own path.
fn yard(sim: &mut Sim) {
    sim.corp_mut().credit(2_000_000); // cover the yard's upkeep during stepped tests
    sim.shipyard_tier = MAX_SHIPYARD_TIER;
    sim.shipyard_body = 1;
}

#[test]
fn a_custom_design_commissions_a_lighter_faster_hull_when_stripped() {
    // A2: commissioning a stripped design (no torpedoes/railgun, less remass) builds
    // a real ship that fits, and a fully-armed one out-guns it — the designer matters.
    let mut sim = Sim::new(0);
    yard(&mut sim);
    sim.corp_mut().credit(2_000_000);
    // A lean frigate: PDC only, no torpedoes, half tanks (the 60-crew pool affords it).
    assert_eq!(
        sim.commission_designed(ShipClass::Frigate, 0, 2, 8, 0, 13, 0, 50),
        Ok(())
    );
    sim.finish_pending_ships();
    assert_eq!(sim.corp().fleet().len(), 1);
    let lean = sim.corp().fleet()[0].loadout.stats();
    // A fully-armed frigate (torpedoes added).
    assert_eq!(
        sim.commission_designed(ShipClass::Frigate, 0, 2, 8, 2, 13, 0, 100),
        Ok(())
    );
    sim.finish_pending_ships();
    let armed = sim.corp().fleet()[1].loadout.stats();
    assert!(
        armed.raw_alpha > lean.raw_alpha,
        "more weapons = more firepower"
    );
    assert!(
        lean.thrust_to_mass > armed.thrust_to_mass,
        "the stripped hull is more mobile"
    );
}

#[test]
fn a_warship_flies_a_committed_trajectory_and_refuels() {
    // §6 / Pillar #2: a move commits a trajectory, spends remass, takes time,
    // and the ship is positional — it can't be re-tasked mid-flight, and a tank
    // refuels at a dock.
    let mut sim = Sim::new(0);
    yard(&mut sim);
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    let full = sim.corp().fleet()[0].nav.remass;
    assert!(
        !sim.corp().fleet()[0].nav.in_transit(sim.tick()),
        "starts docked"
    );

    // Order it from Ceres Yards to Earth (body 3).
    sim.move_ship(0, 3, false)
        .expect("a frigate can reach Earth");
    assert!(
        sim.corp().fleet()[0].nav.in_transit(sim.tick()),
        "now en route"
    );
    assert!(
        sim.corp().fleet()[0].nav.remass < full,
        "spent remass on the burn"
    );
    assert_eq!(
        sim.move_ship(0, 5, false),
        Err(MoveError::Busy),
        "can't re-task mid-flight"
    );

    // Fly it out; it arrives at Earth.
    for _ in 0..3_000 {
        sim.step();
        if !sim.corp().fleet()[0].nav.in_transit(sim.tick()) {
            break;
        }
    }
    assert_eq!(sim.corp().fleet()[0].nav.location, 3, "docked at Earth");

    // Refuel tops the tank (costs credits).
    let before = sim.corp().fleet()[0].nav.remass;
    let credits = sim.corp().credits();
    assert!(sim.refuel_ship(0));
    assert_eq!(sim.corp().fleet()[0].nav.remass, full, "tank full again");
    assert!(sim.corp().fleet()[0].nav.remass > before);
    assert!(sim.corp().credits() < credits, "fuel costs money");
}

#[test]
fn a_run_round_trips_through_a_json_save() {
    // Play a varied run — trade, build, route, research, tune difficulty — so
    // every persisted facet is exercised (§30).
    let mut a = Sim::new(7);
    for _ in 0..40 {
        a.step();
    }
    let _ = a.buy(1, 5, 30); // hold some cargo
    let _ = a.commission_freighter();
    a.set_trade_route(5, 1, 0, 20, 10);
    let _ = a.found_refinery(0, 1, 0);
    let _ = a.commission_ship(ShipClass::Frigate);
    a.set_intensity(Intensity::Harsh);
    a.set_alert_threshold(Priority::Warning);
    a.progression_mut().research.add_points(120);
    a.progression_mut().ceo.gain_xp(300);
    for _ in 0..60 {
        a.step();
    }

    let json = a.save_json();
    let b = Sim::load_json(&json).expect("a valid save reloads");

    // The whole persisted state round-trips bit-for-bit (the SaveState is the
    // complete contract): treasury, warehouse, fleet identity + history,
    // standings, campaign, progression, standing orders, policy, difficulty,
    // and every market's stock/price.
    assert_eq!(a.to_save(), b.to_save());
    assert_eq!(a.tick(), b.tick());
    // Spot-check a few live readers agree, not just the snapshot.
    assert_eq!(a.corp().credits(), b.corp().credits());
    assert_eq!(a.corp().fleet().len(), b.corp().fleet().len());
    assert_eq!(
        a.campaign().gate_progress_bp(),
        b.campaign().gate_progress_bp()
    );
    assert_eq!(
        a.markets()[0].stocks()[5].price,
        b.markets()[0].stocks()[5].price
    );

    // The binary shipping format round-trips identically, and auto-detect loads
    // both formats (§30): binary is smaller than the JSON dev export.
    let bytes = a.save_bytes();
    let c = Sim::load_bytes(&bytes).expect("a binary save reloads");
    assert_eq!(a.to_save(), c.to_save(), "bincode round-trips bit-for-bit");
    assert!(
        bytes.len() < json.len(),
        "binary ({}) is more compact than JSON ({})",
        bytes.len(),
        json.len()
    );
    // load_bytes also accepts the JSON dev export (auto-detected).
    let d = Sim::load_bytes(json.as_bytes()).expect("auto-detect reads JSON too");
    assert_eq!(a.to_save(), d.to_save());
}

#[test]
fn a_bad_save_is_rejected_cleanly() {
    assert!(Sim::load_json("not json").is_err());
    assert!(Sim::load_bytes(b"\x00\x01 not a valid bincode save").is_err());
    // A future version is refused rather than misread (both formats).
    let mut s = Sim::new(1).to_save();
    s.version = 999;
    assert!(Sim::load_json(&s.to_json()).is_err());
    assert!(crate::sim::persist::SaveState::from_bincode(&s.to_bincode()).is_err());
}

#[test]
fn step_advances_tick_and_emits_event() {
    let mut sim = Sim::new(1);
    assert_eq!(sim.tick(), 0);
    let events = sim.step();
    assert!(events.contains(&Event::Tick { tick: 1 }));
    assert_eq!(sim.tick(), 1);
}

#[test]
fn player_verb_events_survive_to_the_next_step() {
    // A player cut between ticks pushes HaulerInterdicted + Scarcity; the
    // next `step` must *surface* them (not wipe them) so the feed voices the
    // player's own cut — previously `events.clear()` dropped them.
    let mut sim = Sim::new(1);
    let id = fly_a_hauler(&mut sim);
    let feed_before = sim.feed().surfaced().len();
    assert!(sim.interdict(id));
    let events = sim.step().to_vec();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::HaulerInterdicted { .. })),
        "the player's cut should reach the returned stream"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Scarcity { .. })));
    assert!(
        sim.feed().surfaced().len() > feed_before,
        "the player's cut should reach the feed"
    );
    // And the carried-over events are not re-surfaced a second time.
    let next = sim.step().to_vec();
    assert!(!next
        .iter()
        .any(|e| matches!(e, Event::HaulerInterdicted { .. })));
}

#[test]
fn a_player_ascent_is_voiced() {
    // The §0.3 fanfare must fire for the *player's* climb, not just for
    // sim-internal ops: a player interdiction's TierAscended now reaches the
    // returned stream.
    use crate::sim::campaign::Tier;
    let mut sim = Sim::new(0);
    let mut saw_ascent = false;
    for _ in 0..400 {
        if let Some(h) = sim.haulers().first() {
            let id = h.id;
            sim.interdict(id);
        }
        for e in sim.step() {
            if matches!(e, Event::TierAscended { .. }) {
                saw_ascent = true;
            }
        }
        if sim.campaign().tier() != Tier::Station {
            break;
        }
    }
    assert!(
        saw_ascent,
        "the player's ascent should emit a TierAscended event"
    );
}

/// Step until a hauler is in flight; return its id.
fn fly_a_hauler(sim: &mut Sim) -> u64 {
    loop {
        sim.step();
        if let Some(h) = sim.haulers().first() {
            return h.id;
        }
    }
}

#[test]
fn rich_interdiction_requires_a_firing_solution() {
    let mut sim = Sim::new(2);
    let id = fly_a_hauler(&mut sim);
    let before = sim.haulers().len();
    // A crawler far off the lane can't reach it: a miss that leaves it flying.
    let crawler = Interceptor {
        pos: (8_000_000, 8_000_000),
        speed: 1,
        skill_bp: 0,
    };
    assert_eq!(sim.interdict_with(id, crawler), Interdiction::NoSolution);
    assert_eq!(
        sim.haulers().len(),
        before,
        "a miss must not remove the hauler"
    );
    // A fast frigate sitting on the hauler always has a solution (it lands or
    // the hauler escapes — never NoSolution).
    let pos = sim
        .haulers()
        .iter()
        .find(|h| h.id == id)
        .unwrap()
        .position(sim.tick());
    let frigate = Interceptor {
        pos,
        speed: 200_000,
        skill_bp: 0,
    };
    assert_ne!(sim.interdict_with(id, frigate), Interdiction::NoSolution);
}

#[test]
fn the_corp_starts_solvent_and_crewed() {
    let sim = Sim::new(0);
    assert!(sim.corp().credits() > 0);
    assert!(sim.corp().trained_crew() > 0);
    assert!(sim.corp().fleet().is_empty());
}

#[test]
fn arbitrage_round_trip_turns_a_profit() {
    // Buy ReactorFuel cheap at Earth (refined producer) and sell it dear at
    // Ceres (refined consumer): the player works the same spread as the NPC
    // haulers, for real credits (§5).
    let mut sim = Sim::new(0);
    let (earth, ceres, rf) = (1usize, 0usize, 5usize);
    assert!(sim.markets()[earth].price(rf) < sim.markets()[ceres].price(rf));
    let start = sim.corp().credits();
    let cost = sim.buy(earth, rf, 10).unwrap();
    assert_eq!(sim.corp().credits(), start - cost);
    assert_eq!(sim.corp().cargo(rf), 10);
    let revenue = sim.sell(ceres, rf, 10).unwrap();
    assert!(revenue > cost, "selling dear should beat buying cheap");
    assert!(sim.corp().credits() > start, "the round trip should profit");
    assert_eq!(sim.corp().cargo(rf), 0);
}

#[test]
fn matched_raider_fights_are_a_competitive_coin_flip() {
    // The fix for the old screened stalemate (§9): a matched pack at Close
    // resolves to decisive outcomes that are neither a guaranteed win nor a
    // guaranteed loss — committing the fleet is a real, two-sided risk (§13).
    let trials = 64;
    let mut wins = 0;
    let mut decisive = 0;
    for seed in 0..trials {
        let mut sim = Sim::new(seed);
        yard(&mut sim);
        for _ in 0..3 {
            sim.commission_ship(ShipClass::Frigate).unwrap();
        }
        sim.finish_pending_ships();
        let out = sim.engage_raiders(Band::Close).unwrap();
        if out.winner.is_some() {
            decisive += 1;
        }
        if out.winner == Some(0) {
            wins += 1;
        }
    }
    assert!(
        decisive > 0,
        "fights should resolve to a winner, not always stalemate"
    );
    let pct = wins * 100 / trials;
    assert!(
        (10..=90).contains(&pct),
        "win rate {pct}% should be competitive, not lopsided"
    );
}

#[test]
fn producing_a_weapon_needs_a_schematic_then_time_and_antagonizes_the_power() {
    // Phase B: you can't *buy* advanced weapons — you need the schematic (earned),
    // then tool up a production line that takes time; building a great power's design
    // sours that power. A newly built ship then fits the produced model.
    let mut sim = Sim::new(0);
    yard(&mut sim);
    let base_pdc = sim.best_weapon_def(WeaponKind::Pdc).intercept;
    let model17 = 2usize; // Model 17 PDC (Earth, tier 2)
    assert!(
        matches!(sim.produce_weapon(model17), Err(CraftError::NoSchematic)),
        "no buying — you need the schematic first"
    );
    // Learn the schematic (as reverse-engineering would) + grant scrap.
    sim.corp_mut().learn_schematic(model17);
    sim.corp_mut().add_scrap(200);
    let earth0 = sim.relations().standing(crate::sim::Faction::Earth);
    sim.produce_weapon(model17).unwrap();
    assert!(
        sim.relations().standing(crate::sim::Faction::Earth) < earth0,
        "building Earth's design antagonises Earth"
    );
    assert!(
        !sim.corp().owns_weapon(model17) && sim.production_remaining(model17) > 0,
        "production takes time, it's not instant"
    );
    // Run until the line completes.
    for _ in 0..400 {
        sim.step();
        if sim.corp().owns_weapon(model17) {
            break;
        }
    }
    assert!(
        sim.corp().owns_weapon(model17),
        "the line eventually finishes"
    );
    let up_pdc = sim.best_weapon_def(WeaponKind::Pdc).intercept;
    assert!(
        up_pdc > base_pdc,
        "the produced PDC screens better ({up_pdc} > {base_pdc})"
    );
    // A newly built Frigate fits the produced PDC.
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    let ship = sim.corp().fleet().last().unwrap();
    assert!(
        ship.loadout
            .weapons()
            .iter()
            .any(|w| w.kind == WeaponKind::Pdc && w.intercept == up_pdc),
        "the new ship fits the produced PDC"
    );
}

#[test]
fn refitting_upgrades_an_old_hull_for_time_and_money() {
    // Phase B: refit re-equips an existing ship with your best-owned weapons, for a
    // yard fee and a stint in the yard (it can't fight while refitting).
    let mut sim = Sim::new(0);
    yard(&mut sim);
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    let pdc = |s: &Sim| {
        s.corp().fleet()[0]
            .loadout
            .weapons()
            .iter()
            .find(|w| w.kind == WeaponKind::Pdc)
            .unwrap()
            .intercept
    };
    let before = pdc(&sim);
    // Produce a better PDC line so "best owned" upgrades.
    sim.corp_mut().learn_schematic(2); // Model 17 PDC
    sim.corp_mut().add_scrap(200);
    sim.produce_weapon(2).unwrap();
    for _ in 0..400 {
        sim.step();
        if sim.corp().owns_weapon(2) {
            break;
        }
    }
    assert_eq!(
        pdc(&sim),
        before,
        "the old hull still has its original screen"
    );
    let credits0 = sim.corp().credits();
    sim.refit_ship(0, usize::MAX, usize::MAX, usize::MAX)
        .unwrap(); // refit to best
    assert!(sim.corp().credits() < credits0, "refit charges a yard fee");
    assert!(
        sim.corp().fleet()[0].is_refitting(sim.tick()),
        "the hull is in the yard, out of action"
    );
    assert!(pdc(&sim) > before, "refit swapped in the better screen");
}

#[test]
fn winning_an_engagement_pays_a_bounty() {
    // Phase B: holding the field credits a bounty per raider hull, so a won fight is
    // net-positive — combat is a viable economic strategy, not pure attrition.
    for seed in 0..64 {
        let mut sim = Sim::new(seed);
        yard(&mut sim);
        for _ in 0..3 {
            sim.commission_ship(ShipClass::Frigate).unwrap();
        }
        sim.finish_pending_ships();
        let before = sim.corp().credits();
        let out = sim.engage_raiders(Band::Close).unwrap();
        if out.winner == Some(0) {
            assert!(sim.last_bounty() > 0, "a win pays a bounty");
            assert!(
                sim.corp().credits() > before,
                "a won engagement is net-positive (ship loss is not a credit cost)"
            );
            return;
        }
    }
    panic!("no winning engagement found across seeds");
}

#[test]
fn exploiting_a_shortage_is_a_one_press_profit() {
    // ReactorFuel is dear at Ceres (the short market): exploiting sources it
    // from the cheaper Earth and sells into Ceres, no pre-held cargo (§0.4).
    let mut sim = Sim::new(0);
    let (ceres, rf) = (0usize, 5usize);
    assert_eq!(sim.corp().cargo(rf), 0);
    let start = sim.corp().credits();
    let profit = sim.exploit_shortage(ceres, rf, 20).unwrap();
    assert!(profit > 0, "exploiting a real shortage should profit");
    assert!(sim.corp().credits() > start);
    assert_eq!(
        sim.corp().cargo(rf),
        0,
        "the cargo round-trips through the warehouse"
    );
}

#[test]
fn a_shortage_dilemma_offers_three_diverging_choices() {
    // Phase A: a shortage isn't a one-press exploit but a menu of trade-offs —
    // speculate (clean profit), profiteer (more credits, rep cost), or relief
    // (forgo profit for goodwill + spine progress). Each pulls a different lever.
    let (ceres, rf) = (0usize, 5usize);
    let owner = Sim::new(0).markets()[ceres].faction();

    // Speculate: profits, leaves reputation untouched.
    let mut a = Sim::new(0);
    let rep0 = a.relations().standing(owner);
    a.push_decision(DecisionKind::Shortage, ceres, rf, 0, 0, a.tick());
    assert_eq!(
        a.decision_options(0).len(),
        3,
        "a dilemma is a menu, not a button"
    );
    let spec = a.resolve_decision(0, 0).unwrap();
    assert!(spec.credits > 0, "speculating a real shortage profits");
    assert_eq!(
        a.relations().standing(owner),
        rep0,
        "speculating costs no rep"
    );
    assert!(a.decisions().is_empty(), "resolving clears the dilemma");

    // Profiteer: out-earns speculating (no fine at neutral standing) but sours the owner.
    let mut b = Sim::new(0);
    b.push_decision(DecisionKind::Shortage, ceres, rf, 0, 0, b.tick());
    let prof = b.resolve_decision(0, 1).unwrap();
    assert!(
        prof.credits > spec.credits,
        "profiteering out-earns the clean play"
    );
    assert!(
        b.relations().standing(owner) < rep0,
        "gouging sours the market's owner"
    );

    // Relief Run: the reputation play — owner standing rises, profit is forgone.
    let mut c = Sim::new(0);
    c.push_decision(DecisionKind::Shortage, ceres, rf, 0, 0, c.tick());
    let relief = c.resolve_decision(0, 2).unwrap();
    assert!(
        c.relations().standing(owner) > rep0,
        "relief earns goodwill"
    );
    assert!(
        relief.credits < prof.credits,
        "relief forgoes the gouge profit"
    );
}

#[test]
fn a_wreck_dilemma_chooses_what_to_extract() {
    // A sighted derelict auto-raises a dilemma: strip for credits, mine data, or
    // gamble on reverse-engineering — the yield follows the *choice*.
    let mut sim = Sim::new(3);
    let mut idx = None;
    for _ in 0..4_000 {
        sim.step();
        if let Some(i) = sim
            .decisions()
            .iter()
            .position(|d| d.kind == DecisionKind::Wreck)
        {
            idx = Some(i);
            break;
        }
    }
    let i = idx.expect("a wreck dilemma should be raised within the run");
    let id = sim.decisions()[i].target;
    assert_eq!(sim.decision_options(i).len(), 3);
    let credits0 = sim.corp().credits();
    let out = sim.resolve_decision(i, 0).unwrap(); // strip for scrap
    assert!(
        out.credits > 0 && sim.corp().credits() > credits0,
        "stripping pays credits"
    );
    assert!(
        sim.wrecks().iter().all(|w| w.id != id),
        "the wreck is consumed"
    );
}

#[test]
fn a_raid_dilemma_eases_the_threat_when_answered() {
    // A telegraphed raid auto-raises a dilemma; hiring escorts is the sure play —
    // pay the fee, the piracy gauge eases.
    let mut sim = Sim::new(0);
    let mut idx = None;
    for _ in 0..2_000 {
        sim.step();
        if let Some(i) = sim
            .decisions()
            .iter()
            .position(|d| d.kind == DecisionKind::RaidThreat)
        {
            idx = Some(i);
            break;
        }
    }
    let i = idx.expect("a raid dilemma should be raised within the run");
    assert_eq!(sim.decision_options(i).len(), 3);
    let before = sim.corp().credits();
    let piracy0 = sim
        .pressure()
        .level(crate::sim::pressure::PressureKind::Piracy);
    let out = sim.resolve_decision(i, 1).unwrap(); // hire escorts
    assert!(
        out.credits < 0 && sim.corp().credits() < before,
        "the fee was paid"
    );
    assert!(
        sim.pressure()
            .level(crate::sim::pressure::PressureKind::Piracy)
            <= piracy0,
        "answering eases the piracy gauge"
    );
}

#[test]
fn an_earth_mars_flashpoint_catches_the_player_in_the_crossfire() {
    // The great-power war is a hazard you live under: a flashpoint raises a dilemma,
    // and rerouting around it costs a sure toll to keep your cargo safe.
    let mut sim = Sim::new(0);
    let mut idx = None;
    for _ in 0..800 {
        sim.step();
        if let Some(i) = sim
            .decisions()
            .iter()
            .position(|d| d.kind == DecisionKind::WarCollateral)
        {
            idx = Some(i);
            break;
        }
    }
    let i = idx.expect("an Earth–Mars flashpoint should fire within the run");
    assert_eq!(sim.decision_options(i).len(), 3);
    let before = sim.corp().credits();
    let out = sim.resolve_decision(i, 0).unwrap(); // reroute
    assert!(
        out.credits < 0 && sim.corp().credits() < before,
        "rerouting around the war costs a toll"
    );
}

#[test]
fn the_top_shortage_is_answerable_in_one_press() {
    // Run until a shortage is surfaced, then answer it from the feed.
    let mut sim = Sim::new(0);
    let mut answered = false;
    for _ in 0..2_000 {
        sim.step();
        if sim.feed().surfaced().iter().any(|a| a.is_act_now()) {
            answered = sim.answer_top_shortage(20);
            break;
        }
    }
    assert!(
        answered,
        "an open act-now shortage should be answerable in one press"
    );
}

#[test]
fn trades_are_guarded() {
    let mut sim = Sim::new(0);
    // Nothing in the warehouse to sell.
    assert_eq!(sim.sell(0, 0, 5), Err(TradeError::InsufficientCargo));
    // More than the market holds.
    assert_eq!(sim.buy(0, 0, 1_000_000), Err(TradeError::InsufficientStock));
    // Affordable stock-wise, but beyond the treasury (200 dear ReactorFuel).
    assert_eq!(sim.buy(0, 5, 200), Err(TradeError::InsufficientCredits));
}

#[test]
fn instant_trades_pay_a_brokerage_fee() {
    // Buying and selling the same lot at one market (no spread) must lose
    // money to the fee — instant liquidity is not free (§5 sink). The fee is
    // what makes hand-trading a decision instead of a riskless skim.
    let mut sim = Sim::new(0);
    let (m, c) = (0usize, 5usize);
    let start = sim.corp().credits();
    let spent = sim.buy(m, c, 10).unwrap();
    let got = sim.sell(m, c, 10).unwrap();
    assert!(
        got < spent,
        "a flat round-trip should lose the fee, got {got} vs {spent}"
    );
    assert!(sim.corp().credits() < start, "the fee leaves the treasury");
}

#[test]
fn overhead_caps_runaway_hoarding() {
    // Operating overhead is a wealth-scaled sink: a treasury far above the
    // free float is skimmed each tick, so hoards can't compound without
    // bound. A small float below the threshold is left untouched.
    let mut sim = Sim::new(0);
    sim.corp.credit(900_000); // well above the free float (private field, test-only)
    let rich = sim.corp().credits();
    sim.step();
    assert!(
        sim.corp().credits() < rich,
        "overhead should skim a large treasury"
    );
    // A company at the float is not taxed (early/mid play stays clean).
    let mut lean = Sim::new(0);
    let base = lean.corp().credits();
    lean.step();
    assert_eq!(
        lean.corp().credits(),
        base,
        "a treasury at the free float pays no overhead"
    );
}

#[test]
fn commissioning_spends_credits_and_crew_with_the_pool_as_the_cap() {
    let mut sim = Sim::new(0);
    yard(&mut sim);
    let (credits0, crew0) = (sim.corp().credits(), sim.corp().trained_crew());
    sim.commission_ship(ShipClass::Frigate).unwrap();
    // Cost is charged + crew reserved at order time; the hull stands up once built.
    assert!(sim.corp().credits() < credits0);
    assert!(sim.corp().trained_crew() < crew0);
    sim.finish_pending_ships();
    assert_eq!(sim.corp().fleet().len(), 1);
    // A battleship needs more crew than the starting pool can field (§8c).
    assert_eq!(
        sim.commission_ship(ShipClass::Battleship),
        Err(CommissionError::NotEnoughCrew)
    );
}

#[test]
fn commissioning_a_hull_is_a_timed_build_not_instant() {
    // The "no instant macro actions" pace re-aim: a commissioned hull is laid down in
    // the yard and only joins the fleet once its build completes (a frigate ~60 days).
    let mut sim = Sim::new(0);
    yard(&mut sim);
    sim.commission_ship(ShipClass::Frigate).unwrap();
    assert_eq!(sim.corp().fleet().len(), 0, "the hull isn't instant");
    assert_eq!(sim.pending_ship_count(), 1, "it's under construction");
    let (class, days) = sim.pending_ship(0).unwrap();
    assert_eq!(class, ShipClass::Frigate);
    assert!(days > 0, "a build countdown is showing ({days} days)");
    // Run past the build; it stands up exactly once, then the queue is empty.
    let build = Sim::commission_build_ticks(ShipClass::Frigate);
    for _ in 0..=build {
        sim.step();
    }
    assert_eq!(
        sim.corp().fleet().len(),
        1,
        "the hull joins the fleet when built"
    );
    assert_eq!(sim.pending_ship_count(), 0, "the queue drained");
}

#[test]
fn transiting_the_gate_is_the_climactic_payoff() {
    // §0.1/§17: standing at the open gate, the deliberate transit verb crosses
    // into the Beyond endgame, voices the gate's answer, and emits GateTransited.
    let mut sim = Sim::new(0);
    assert!(
        !sim.can_transit_gate(),
        "the gate isn't reachable at the start"
    );
    assert!(!sim.transit_gate());
    // Climb the whole spine to the open gate.
    for _ in 0..200 {
        sim.complete_op();
    }
    assert_eq!(sim.campaign().tier(), Tier::Gate);
    assert!(sim.can_transit_gate());
    // Transit — the payoff.
    assert!(sim.transit_gate());
    assert_eq!(sim.campaign().tier(), Tier::Beyond);
    assert!(sim.campaign().transited());
    assert!(!sim.can_transit_gate(), "no second transit");
    assert!(!sim.transit_gate());
    // The transit surfaced a GateTransited event for the feed to voice.
    let events = sim.step().to_vec();
    // (The event was pushed before this step; the feed voices the answer.)
    let _ = events;
}

#[test]
fn buying_a_frontier_colony_grows_the_empire_and_alarms_the_inners() {
    // The empire layer (E1): an independent colony can be bought; it joins the
    // player's holdings, pays tribute, and the political cost lands on the inners.
    use crate::sim::faction::Faction;
    let mut sim = Sim::new(1);
    assert_eq!(sim.controlled_colony_count(), 0);
    let targets = sim.acquirable_colonies();
    assert!(
        !targets.is_empty(),
        "there are independent colonies to take"
    );
    let i = targets[0];
    // You can't buy a great power's territory — only independents.
    let earth_owned =
        (0..sim.colonies().len()).find(|&j| sim.colonies()[j].faction != Faction::Independents);
    if let Some(j) = earth_owned {
        assert_eq!(sim.acquire_colony(j), Err(AcquireError::NotAcquirable));
    }
    // Credit just over the (now mid-game-priced) cost so post-purchase wealth stays under
    // the free-float overhead sink — otherwise the sink masks the small colony tribute.
    let cost = sim.colony_acquire_cost(i).unwrap();
    sim.corp_mut().credit(cost + 8_000);
    let before = sim.corp().credits();
    let earth0 = sim.relations().standing(Faction::Earth);
    let belt0 = sim.relations().standing(Faction::Belt);
    assert_eq!(sim.acquire_colony(i), Ok(()));
    assert!(sim.colony_controlled(i));
    assert_eq!(sim.controlled_colony_count(), 1);
    assert!(sim.corp().credits() < before, "buying costs credits");
    // The inners grew wary; the home Belt approved (the overextension pressure).
    assert!(sim.relations().standing(Faction::Earth) < earth0);
    assert!(sim.relations().standing(Faction::Belt) > belt0);
    // No double purchase.
    assert_eq!(sim.acquire_colony(i), Err(AcquireError::AlreadyControlled));
    // A controlled colony pays tribute — the treasury grows hands-off.
    let held = sim.corp().credits();
    for _ in 0..50 {
        sim.step();
    }
    assert!(
        sim.corp().credits() > held,
        "holdings pay tribute over time"
    );
}

#[test]
fn overextension_strains_an_empire_past_its_administrative_reach() {
    // E2: within admin capacity, holdings are full-efficiency income; past it,
    // efficiency falls and strain upkeep turns extra holdings net-negative.
    let mut sim = Sim::new(2);
    sim.corp_mut().credit(2_000_000);
    let cap = sim.admin_capacity();
    assert!(cap >= ADMIN_BASE_CAPACITY);
    assert_eq!(sim.admin_strain(), 0);
    assert_eq!(
        sim.holdings_efficiency_bp(),
        10_000,
        "unstrained = full income"
    );
    // Buy every independent colony available — almost certainly past capacity.
    let targets = sim.acquirable_colonies();
    assert!(targets.len() > cap, "enough colonies to overextend");
    for i in targets {
        let _ = sim.acquire_colony(i);
    }
    assert!(sim.admin_load() > 0);
    assert!(
        sim.admin_strain() > 0,
        "taking the whole frontier overextends the company"
    );
    assert!(
        sim.holdings_efficiency_bp() < 10_000,
        "overextension cuts efficiency"
    );
}

#[test]
fn courting_a_company_to_ally_opens_a_free_annex_and_lends_an_escort() {
    // E8: the macro diplomacy loop — invest Influence to court an independent
    // company; an Ally's colony joins you for free and its ships screen your trade.
    let mut sim = Sim::new(4);
    // Pick a company and its colony.
    assert!(!sim.companies().is_empty());
    let colony = sim.companies()[0].home_colony;
    let company = 0usize;
    // Bank influence and court the company up to Ally (≈4 courtings).
    for _ in 0..1_000 {
        sim.step();
    }
    let mut courted = 0;
    while sim.company_stance(company) != crate::sim::diplomacy::Stance::Ally && courted < 10 {
        if sim.court_company(company).is_err() {
            // ran out of influence — let it accrue
            for _ in 0..120 {
                sim.step();
            }
        } else {
            courted += 1;
        }
    }
    assert_eq!(
        sim.company_stance(company),
        crate::sim::diplomacy::Stance::Ally,
        "courting reaches alliance"
    );
    // An Ally's colony annexes for free (no Influence spent).
    assert!(sim.can_annex(colony));
    let infl_before = sim.influence();
    assert_eq!(sim.annex_colony(colony), Ok(()));
    assert!(sim.colony_controlled(colony));
    assert_eq!(sim.influence(), infl_before, "an ally joins for free");
}

#[test]
fn seizing_a_companys_colony_makes_it_a_rival() {
    // E8: cross a company (take its colony by force) and it turns Rival, refusing
    // to be annexed thereafter.
    let mut sim = Sim::new(5);
    sim.corp_mut().credit(5_000_000);
    for _ in 0..5 {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    let colony = sim.companies()[0].home_colony;
    let company = 0usize;
    assert_ne!(
        sim.company_stance(company),
        crate::sim::diplomacy::Stance::Rival
    );
    let _ = sim.seize_colony(colony, Band::Close);
    if sim.colony_controlled(colony) {
        assert_eq!(
            sim.company_stance(company),
            crate::sim::diplomacy::Stance::Rival,
            "force makes an enemy"
        );
    }
}

#[test]
fn diplomatic_annexation_costs_influence_and_good_standing_not_credits() {
    // E4: the peaceful path — annex an independent colony with banked Influence
    // and Cordial standing, paying a gentler political cost than a buyout.
    use crate::sim::faction::Faction;
    let mut sim = Sim::new(4);
    let i = sim.acquirable_colonies()[0];
    // Without standing or influence, you can't annex.
    assert_eq!(sim.annex_colony(i), Err(AnnexError::StandingTooLow));
    sim.relations_mut().adjust(Faction::Independents, 400); // Cordial
    assert_eq!(
        sim.annex_colony(i),
        Err(AnnexError::NotEnoughInfluence),
        "still need Influence banked"
    );
    // Bank influence over time (it accrues each tick).
    for _ in 0..ANNEX_INFLUENCE_COST {
        sim.step();
    }
    assert!(sim.influence() >= ANNEX_INFLUENCE_COST);
    let credits_before = sim.corp().credits();
    let earth_before = sim.relations().standing(Faction::Earth);
    assert!(sim.can_annex(i));
    assert_eq!(sim.annex_colony(i), Ok(()));
    assert!(sim.colony_controlled(i));
    assert_eq!(
        sim.corp().credits(),
        credits_before,
        "annexation costs no credits"
    );
    assert!(sim.influence() < ANNEX_INFLUENCE_COST, "it spent Influence");
    // A gentler ding than a buyout (−20 vs −40), but still some inner wariness.
    assert!(sim.relations().standing(Faction::Earth) < earth_before);
    assert!(sim.relations().standing(Faction::Earth) >= earth_before - 25);
}

#[test]
fn military_seizure_takes_a_colony_by_force_at_the_harshest_political_price() {
    // E5: the aggressive path — assault a colony's garrison and, on a win, take
    // it (even a great power's), enraging the owner.
    let mut sim = Sim::new(7);
    yard(&mut sim);
    sim.corp_mut().credit(5_000_000);
    // Need a fleet to mount an assault.
    let indie = sim.acquirable_colonies()[0];
    assert_eq!(
        sim.seize_colony(indie, Band::Close),
        Err(SeizeError::NoFleet)
    );
    for _ in 0..5 {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    sim.finish_pending_ships();
    // Seize a lightly-garrisoned independent colony (2 defenders) — 5 frigates win.
    let owner = sim.colonies()[indie].faction;
    let alarm_before = sim.coalition_alarm();
    let owner_before = sim.relations().standing(owner);
    let outcome = sim
        .seize_colony(indie, Band::Close)
        .expect("a resolved assault");
    assert_eq!(outcome.winner, Some(0), "the squadron takes the colony");
    assert!(sim.colony_controlled(indie));
    // Open aggression: the biggest alarm spike + the owner is enraged.
    assert!(sim.coalition_alarm() > alarm_before);
    assert!(sim.relations().standing(owner) < owner_before);
    // Can't seize what you already hold.
    assert_eq!(
        sim.seize_colony(indie, Band::Close),
        Err(SeizeError::AlreadyControlled)
    );
}

#[test]
fn overexpansion_provokes_a_coalition_that_seizes_an_undefended_holding() {
    // E3: grow too big and the great powers unite; an undefended strike pries a
    // holding from your grip — the geopolitical cap on reckless expansion.
    let mut sim = Sim::new(3);
    sim.corp_mut().credit(5_000_000);
    for i in sim.acquirable_colonies() {
        let _ = sim.acquire_colony(i);
    }
    // A couple of stations push the empire past the alarm baseline.
    let _ = sim.found_refinery(0, 0, 1);
    let _ = sim.found_refinery(1, 0, 1);
    assert!(sim.holding_count() >= 6, "a sizeable empire");
    let mut struck = false;
    for _ in 0..600 {
        sim.step();
        if sim.coalition_strike_pending() {
            struck = true;
            break;
        }
    }
    assert!(
        sim.coalition_active(),
        "overexpansion united the great powers"
    );
    assert!(struck, "the coalition moved on the holdings");
    // Leave it undefended — a holding is seized.
    let before = sim.controlled_colony_count();
    for _ in 0..(COALITION_RESPONSE_WINDOW + 5) {
        sim.step();
    }
    assert!(
        sim.controlled_colony_count() < before,
        "an undefended coalition strike costs a colony"
    );
}

#[test]
fn defending_repels_the_coalition_and_keeps_the_holdings() {
    // E3: with a fleet, you can answer the coalition and hold what you took.
    let mut sim = Sim::new(8);
    yard(&mut sim);
    sim.corp_mut().credit(5_000_000);
    for i in sim.acquirable_colonies() {
        let _ = sim.acquire_colony(i);
    }
    let _ = sim.found_refinery(0, 0, 1);
    let _ = sim.found_refinery(1, 0, 1);
    for _ in 0..5 {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    sim.finish_pending_ships();
    assert!(!sim.corp().fleet().is_empty());
    let mut defended = false;
    for _ in 0..600 {
        sim.step();
        if sim.coalition_strike_pending() {
            let held = sim.controlled_colony_count();
            let outcome = sim.defend_holdings(Band::Close);
            assert!(outcome.is_some(), "the fleet answers");
            assert!(!sim.coalition_strike_pending(), "the strike is resolved");
            assert_eq!(
                sim.controlled_colony_count(),
                held,
                "a won defense loses no holding"
            );
            defended = true;
            break;
        }
    }
    assert!(defended, "a coalition strike arrived to defend against");
}

#[test]
fn souring_a_faction_brings_customs_surcharges_and_inspection_fines() {
    // EP4: anger a great power and trading in its space costs more (customs
    // surcharge), and — once you hold assets — it inspects and fines your
    // shipping. Countered by repairing the relationship.
    use crate::sim::faction::Faction;
    let mut sim = Sim::new(3);
    sim.corp_mut().credit(500_000);
    // Find an Earth-owned market; the fee is the baseline while neutral.
    let m = (0..sim.markets().len())
        .find(|&m| sim.markets()[m].faction() == Faction::Earth)
        .expect("an Earth market");
    let neutral_fee = sim.market_trade_fee(m, 100_000);
    // Sour Earth hard → trading there now carries a customs surcharge.
    sim.relations_mut().adjust(Faction::Earth, -800);
    assert!(
        sim.market_trade_fee(m, 100_000) > neutral_fee,
        "trading in soured space costs more"
    );
    // Take a colony so you're a trader with assets to inspect, then run.
    let c = sim.acquirable_colonies()[0];
    let _ = sim.acquire_colony(c);
    let mut inspected = false;
    for _ in 0..(INSPECTION_INTERVAL * 2) {
        if sim
            .step()
            .iter()
            .any(|e| matches!(e, Event::Inspected { .. }))
        {
            inspected = true;
        }
    }
    assert!(inspected, "a soured power inspects and fines your shipping");
    // Mend fences (standing back above the threshold) → inspections stop.
    sim.relations_mut().adjust(Faction::Earth, 1_000);
    assert!(sim.worst_standing() > INSPECTION_THRESHOLD);
    let mut inspected_after = false;
    for _ in 0..(INSPECTION_INTERVAL * 2) {
        if sim
            .step()
            .iter()
            .any(|e| matches!(e, Event::Inspected { .. }))
        {
            inspected_after = true;
        }
    }
    assert!(
        !inspected_after,
        "repairing the relationship stops the sweeps"
    );
}

#[test]
fn seizing_a_powers_colony_alarms_that_power_most() {
    // E7: the coalition is per-faction — taking Mars's colony by force spikes
    // *Mars's* alarm hardest, and Mars leads the response. Buying the independent
    // frontier, by contrast, alarms the inners evenly.
    use crate::sim::faction::Faction;
    let mut sim = Sim::new(6);
    sim.corp_mut().credit(5_000_000);
    for _ in 0..6 {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    // Find a Mars-owned colony with a light enough garrison to take with 6 frigates.
    let mars = (0..sim.colonies().len())
        .filter(|&i| sim.colonies()[i].faction == Faction::Mars)
        .min_by_key(|&i| sim.garrison_size(i))
        .expect("a Mars colony");
    let earth_before = sim.faction_alarm(Faction::Earth);
    let _ = sim.seize_colony(mars, Band::Close);
    if sim.colony_controlled(mars) {
        // A successful seizure alarms Mars far more than Earth.
        assert!(
            sim.faction_alarm(Faction::Mars) > sim.faction_alarm(Faction::Earth),
            "the victim power is the most alarmed"
        );
        assert!(
            sim.faction_alarm(Faction::Earth) > earth_before,
            "others note it too"
        );
        assert_eq!(
            sim.coalition_leader(),
            Faction::Mars,
            "Mars leads the response"
        );
    }
}

#[test]
fn an_unescorted_trade_empire_is_raided_but_a_navy_protects_it() {
    // EP3: a growing empire with too few escorts on station is preyed upon by
    // pirates; a navy that scales with the empire deters them. Real but counterable.
    let mut sim = Sim::new(2);
    yard(&mut sim);
    sim.corp_mut().credit(150_000);
    for i in sim.acquirable_colonies() {
        let _ = sim.acquire_colony(i);
    }
    assert!(sim.holding_count() > 0);
    assert!(sim.escorts_needed() >= 1);
    assert!(!sim.empire_secure(), "no warships yet → unescorted");
    // With no navy, a raid event fires within a few cadences.
    let mut raided = false;
    for _ in 0..(PIRACY_INTERVAL * 3) {
        if sim
            .step()
            .iter()
            .any(|e| matches!(e, Event::EmpireRaided { .. }))
        {
            raided = true;
        }
    }
    assert!(raided, "an unescorted empire is preyed upon");
    // Stand up a navy that covers the empire → secure, and raids stop.
    for _ in 0..(sim.escorts_needed() + 2) {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    sim.finish_pending_ships();
    assert!(
        sim.empire_secure(),
        "a navy that scales with the empire protects it"
    );
    let mut raided_after = false;
    for _ in 0..(PIRACY_INTERVAL * 3) {
        if sim
            .step()
            .iter()
            .any(|e| matches!(e, Event::EmpireRaided { .. }))
        {
            raided_after = true;
        }
    }
    assert!(!raided_after, "escorted shipping is no longer raided");
}

#[test]
fn owning_a_market_cuts_your_fee_and_earns_a_tariff_on_npc_trade() {
    // EP2: a colony you control is a market you own — you trade there fee-reduced,
    // and NPC deliveries into it pay your treasury a tariff (your empire earns from
    // the living economy). A market you don't own does neither.
    let mut sim = Sim::new(1);
    // Just enough for the (mid-game-priced) market colony, kept under the free-float so
    // the wealth sink doesn't swamp the tribute/tariff we're measuring.
    sim.corp_mut().credit(330_000);
    // Find a market-colony to take, and its market index (same body).
    let colony = (0..sim.colonies().len())
        .find(|&i| {
            sim.colonies()[i].is_market && sim.colonies()[i].faction == Faction::Independents
        })
        .expect("an independent market colony");
    let body = sim.colonies()[colony].body;
    let m = (0..sim.markets().len())
        .find(|&m| sim.markets()[m].body() == body)
        .expect("its market");
    assert!(!sim.market_is_owned(m), "not owned before acquiring");
    assert_eq!(sim.acquire_colony(colony), Ok(()));
    assert!(sim.market_is_owned(m), "owned after acquiring");
    // The fee on a buy at the owned market is the reduced rate.
    let owned_fee = sim.market_trade_fee(m, 100_000);
    let other = (0..sim.markets().len())
        .find(|&x| !sim.market_is_owned(x))
        .expect("an unowned market");
    assert!(
        owned_fee < sim.market_trade_fee(other, 100_000),
        "owning the broker is cheaper"
    );
    // NPC deliveries into the owned market grow the treasury over time (the tariff).
    let before = sim.corp().credits();
    for _ in 0..800 {
        sim.step();
    }
    assert!(
        sim.corp().credits() > before,
        "tariff + tribute grow the treasury from NPC trade through your market"
    );
}

#[test]
fn controlled_colonies_supply_raw_goods_into_your_warehouse() {
    // EP1: a controlled colony produces its specialty raw into your warehouse each
    // tick — holdings feed your supply chain, not just a credit drip.
    let mut sim = Sim::new(1);
    sim.corp_mut().credit(500_000);
    let i = sim.acquirable_colonies()[0];
    let specialty = sim.colony_specialty(i);
    let before = sim.corp().cargo(specialty);
    assert_eq!(sim.acquire_colony(i), Ok(()));
    for _ in 0..50 {
        sim.step();
    }
    let after = sim.corp().cargo(specialty);
    assert!(
        after >= before + 50 * COLONY_OUTPUT_PER_TICK,
        "the colony stocked your warehouse with its specialty good"
    );
}

#[test]
fn developing_a_colony_scales_output_and_draws_no_coalition_alarm() {
    // Phase C (the *tall* axis): investing in a colony raises its development, which
    // scales its specialty output — and unlike acquiring a *new* colony, improving
    // your own draws no extra coalition alarm.
    let mut sim = Sim::new(1);
    sim.corp_mut().credit(2_000_000);
    let i = sim.acquirable_colonies()[0];
    sim.acquire_colony(i).unwrap();
    let specialty = sim.colony_specialty(i);
    // Output at base development.
    let c0 = sim.corp().cargo(specialty);
    for _ in 0..20 {
        sim.step();
    }
    let base_out = sim.corp().cargo(specialty) - c0;
    // Develop it: costs credits, raises the level, no extra alarm.
    let alarm_before = sim.coalition_alarm();
    let cost = sim.develop_cost(i).unwrap();
    let credits0 = sim.corp().credits();
    sim.develop_colony(i).unwrap();
    assert_eq!(
        sim.corp().credits(),
        credits0 - cost,
        "development costs credits"
    );
    assert_eq!(sim.colony_dev(i), 2);
    assert!(
        sim.coalition_alarm() <= alarm_before,
        "developing your own colony draws no coalition alarm"
    );
    // Developing is a ~180-day build: the new level's benefit only lands once it's built.
    assert!(sim.colony_build_days(i) > 0);
    while sim.colony_build_days(i) > 0 {
        sim.step();
    }
    // Output now scales with the higher (now operational) development.
    let c1 = sim.corp().cargo(specialty);
    for _ in 0..20 {
        sim.step();
    }
    let dev_out = sim.corp().cargo(specialty) - c1;
    assert!(
        dev_out > base_out,
        "a developed colony out-produces a bare one ({dev_out} > {base_out})"
    );
}

#[test]
fn the_development_doctrine_tilts_holding_yield() {
    // Phase C: Industry favours raw output (vs Trade), and Growth cheapens dev.
    let out_under = |doc: DevDoctrine| -> i64 {
        let mut sim = Sim::new(1);
        sim.corp_mut().credit(2_000_000);
        let i = sim.acquirable_colonies()[0];
        sim.acquire_colony(i).unwrap();
        while sim.dev_doctrine() != doc {
            sim.cycle_dev_doctrine();
        }
        let specialty = sim.colony_specialty(i);
        let c0 = sim.corp().cargo(specialty);
        for _ in 0..30 {
            sim.step();
        }
        sim.corp().cargo(specialty) - c0
    };
    assert!(
        out_under(DevDoctrine::Industry) > out_under(DevDoctrine::Trade),
        "Industry tilts holding output above Trade"
    );
    // Growth cheapens development.
    let mut sim = Sim::new(1);
    sim.corp_mut().credit(2_000_000);
    let i = sim.acquirable_colonies()[0];
    sim.acquire_colony(i).unwrap();
    let balanced_cost = sim.develop_cost(i).unwrap();
    while sim.dev_doctrine() != DevDoctrine::Growth {
        sim.cycle_dev_doctrine();
    }
    assert!(
        sim.develop_cost(i).unwrap() < balanced_cost,
        "Growth doctrine cheapens development"
    );
}

#[test]
fn warships_need_a_shipyard_except_corvettes_with_opa_standing() {
    // Ship sourcing (Phase B+): civilians + (with OPA standing) corvettes come from
    // Tycho; everything bigger needs your own shipyard, unlocked by tier.
    let mut sim = Sim::new(0);
    sim.corp_mut().credit(2_000_000);
    assert!(matches!(
        sim.commission_ship(ShipClass::Frigate),
        Err(CommissionError::NeedShipyard)
    ));
    assert!(matches!(
        sim.commission_ship(ShipClass::Cruiser),
        Err(CommissionError::NeedShipyard)
    ));
    // Good OPA standing buys corvettes from Tycho — but not warships.
    sim.relations_mut()
        .adjust(crate::sim::Faction::Belt, CORVETTE_STANDING);
    assert!(
        sim.commission_ship(ShipClass::Frigate).is_ok(),
        "corvettes come from Tycho with OPA standing"
    );
    assert!(matches!(
        sim.commission_ship(ShipClass::Destroyer),
        Err(CommissionError::NeedShipyard)
    ));
    // A shipyard unlocks warships, gated by its tier — but founding is a ~1-year build,
    // so it lays down nothing until construction finishes.
    let home = sim.markets()[0].body();
    sim.found_shipyard(home).unwrap();
    assert!(sim.shipyard_build_days() > 0);
    assert!(
        matches!(
            sim.commission_ship(ShipClass::Destroyer),
            Err(CommissionError::NeedShipyard)
        ),
        "a building yard lays down nothing yet"
    );
    while sim.shipyard_build_days() > 0 {
        sim.step();
    }
    assert_eq!(sim.shipyard_max_hull(), "Destroyer");
    assert!(matches!(
        sim.commission_ship(ShipClass::Cruiser),
        Err(CommissionError::NeedShipyard)
    ));
    sim.corp_mut().credit(2_000_000); // the year-long build drained the treasury (overhead sink)
    sim.expand_shipyard().unwrap();
    while sim.shipyard_build_days() > 0 {
        sim.step();
    }
    assert_eq!(sim.shipyard_max_hull(), "Cruiser");
    assert!(matches!(
        sim.found_shipyard(home),
        Err(ShipyardError::AlreadyBuilt)
    ));
}

#[test]
fn an_outpost_pays_tribute_develops_and_boosts_a_co_located_miner() {
    let mut sim = Sim::new(0);
    sim.corp_mut().credit(50_000); // below the 100k wealth-overhead sink, so tribute nets out
                                   // Found an outpost at a mineable belt body (Vesta).
    let body = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Vesta")
        .unwrap();
    assert!(sim.can_found_outpost(body));
    sim.found_outpost(body).unwrap();
    assert_eq!(sim.outposts().len(), 1);
    // Founding is a slow build (~180 days): inert while under construction.
    assert!(!sim.outpost_at(body).unwrap().is_ready(sim.tick()));
    assert_eq!(sim.outpost_build_remaining(body), Some(180));
    let c0 = sim.corp().credits();
    sim.step();
    assert_eq!(
        sim.corp().credits(),
        c0,
        "an outpost under construction pays nothing"
    );
    // Fast-forward past the build: it comes online and pays tribute.
    while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
        sim.step();
    }
    assert!(sim.outpost_build_remaining(body).is_none());
    let c1 = sim.corp().credits();
    sim.step();
    assert!(
        sim.corp().credits() > c1,
        "an operational outpost pays tribute"
    );
    // A miner on the (ready) outpost's body hauls to the on-site station: +50% output.
    let mineral = sim.body_mineral(body);
    sim.buy_miner(body).unwrap();
    let before = sim.corp().cargo(mineral);
    sim.step();
    assert!(
        sim.corp().cargo(mineral) > before + MINER_OUTPUT_PER_TICK,
        "a co-located miner is boosted by the operational outpost"
    );
    // Developing re-arms the build timer (the new level isn't instant).
    sim.develop_outpost(body).unwrap();
    assert!(!sim.outpost_at(body).unwrap().is_ready(sim.tick()));
}

#[test]
fn an_outpost_needs_a_mine_to_produce_raw_goods() {
    let mut sim = Sim::new(0);
    sim.corp_mut().credit(60_000);
    let body = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Eros")
        .unwrap();
    sim.found_outpost(body).unwrap();
    while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
        sim.step();
    }
    // Operational but Mine-less: produces no raw goods (only its credit tribute).
    let mineral = sim.body_mineral(body);
    let before = sim.corp().cargo(mineral);
    for _ in 0..10 {
        sim.step();
    }
    assert_eq!(
        sim.corp().cargo(mineral),
        before,
        "no Mine ⇒ no raw production"
    );
    assert_eq!(sim.outpost_stored(body).0, 0, "no Mine ⇒ empty local store");
    // Build a Mine (a ~120-day build); once it's up, the outpost extracts raw into its
    // LOCAL store each tick (not the warehouse — per-asset inventory, §10).
    assert!(!sim.outpost_has_facility(body, FAC_MINE));
    sim.build_facility(body, FAC_MINE).unwrap();
    while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
        sim.step();
    }
    let wh_before = sim.corp().cargo(mineral);
    for _ in 0..10 {
        sim.step();
    }
    assert!(
        sim.outpost_stored(body).0 > 0,
        "a Mine fills the local store"
    );
    assert_eq!(
        sim.corp().cargo(mineral),
        wh_before,
        "no Hangar ⇒ the goods stay on-site, not in your warehouse"
    );
    // A Hangar ships the local stock out to your warehouse.
    sim.corp_mut().credit(20_000);
    sim.build_facility(body, FAC_HANGAR).unwrap();
    while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
        sim.step();
    }
    let wh2 = sim.corp().cargo(mineral);
    for _ in 0..10 {
        sim.step();
    }
    assert!(
        sim.corp().cargo(mineral) > wh2,
        "a Hangar ships local goods to your warehouse"
    );
}

#[test]
fn a_collector_hauler_drains_a_hangarless_outpost_at_a_route_slot_cost() {
    use crate::sim::corp::HaulerClass;
    // §10: a hauler dedicated to collection ferries a Mine-only (no-Hangar) outpost's local
    // store to the warehouse — the freighter alternative to a Hangar — and is then off the
    // trade-route pool.
    let mut sim = Sim::new(0);
    sim.corp_mut().credit(200_000);
    let body = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Eros")
        .unwrap();
    sim.found_outpost(body).unwrap();
    while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
        sim.step();
    }
    sim.build_facility(body, FAC_MINE).unwrap();
    while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
        sim.step();
    }
    // Buy a hauler, then dedicate it to collecting this outpost.
    sim.commission_hauler(HaulerClass::Light).unwrap();
    assert!(sim.can_assign_collector(body));
    assert!(sim.assign_collector(body));
    assert_eq!(sim.collectors_assigned(), 1);
    // Can't dedicate more collectors than haulers.
    assert!(
        !sim.can_assign_collector(body),
        "already collecting / no free hauler"
    );
    let mineral = sim.body_mineral(body);
    let wh_before = sim.corp().cargo(mineral);
    for _ in 0..20 {
        sim.step();
    }
    assert!(
        sim.corp().cargo(mineral) > wh_before,
        "the collector hauler drains the store to the warehouse (no Hangar needed)"
    );
    // Recalling frees the hauler back to the route pool.
    assert!(sim.recall_collector(body));
    assert_eq!(sim.collectors_assigned(), 0);
}

#[test]
fn a_fully_built_outpost_can_be_promoted_to_a_colony() {
    let mut sim = Sim::new(0);
    sim.corp_mut().credit(5_000_000);
    let body = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Io")
        .unwrap();
    sim.found_outpost(body).unwrap();
    let finish_build = |sim: &mut Sim| {
        while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
            sim.step();
        }
    };
    finish_build(&mut sim);
    // Not promotable until maxed + all facilities.
    assert!(!sim.can_promote_outpost(body));
    while sim.outpost_at(body).unwrap().level < MAX_OUTPOST_LEVEL {
        sim.corp_mut().credit(100_000);
        sim.develop_outpost(body).unwrap();
        finish_build(&mut sim);
    }
    for kind in [FAC_MINE, FAC_STORAGE, FAC_HANGAR] {
        sim.build_facility(body, kind).unwrap();
        finish_build(&mut sim);
    }
    // Maxed + facilities, but population must first be grown by supplying Ice.
    assert!(!sim.can_promote_outpost(body), "needs population too");
    sim.corp_mut().store(ICE_COMMODITY, 5_000); // a stockpile of the basic good
    while sim.outpost_at(body).unwrap().population < PROMOTE_POP {
        sim.step();
    }
    assert!(
        sim.can_promote_outpost(body),
        "maxed + facilities + population ⇒ promotable"
    );
    assert_eq!(sim.outpost_at(body).unwrap().rank, RANK_OUTPOST);
    sim.corp_mut().credit(100_000);
    sim.promote_outpost(body).unwrap();
    assert_eq!(sim.outpost_at(body).unwrap().rank, RANK_COLONY);
    finish_build(&mut sim);
    // A colony out-yields the bare outpost it was (3× tribute) — credits climb faster.
    let c0 = sim.corp().credits();
    for _ in 0..30 {
        sim.step();
    }
    assert!(
        sim.corp().credits() > c0,
        "a promoted colony pays a fat tribute"
    );
}

#[test]
fn a_colony_climbs_through_hub_to_a_single_capital() {
    // The full ladder: Outpost → Colony → Hub → Capital. Each rung needs the prior
    // promotion finished, more population, and more credits; there can be only one Capital.
    let mut sim = Sim::new(0);
    sim.corp_mut().credit(50_000_000);
    let body = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Io")
        .unwrap();
    sim.found_outpost(body).unwrap();
    let finish_build = |sim: &mut Sim| {
        while !sim.outpost_at(body).unwrap().is_ready(sim.tick()) {
            sim.step();
        }
    };
    finish_build(&mut sim);
    while sim.outpost_at(body).unwrap().level < MAX_OUTPOST_LEVEL {
        sim.corp_mut().credit(200_000);
        sim.develop_outpost(body).unwrap();
        finish_build(&mut sim);
    }
    for kind in [FAC_MINE, FAC_STORAGE, FAC_HANGAR] {
        sim.corp_mut().credit(200_000);
        sim.build_facility(body, kind).unwrap();
        finish_build(&mut sim);
    }
    sim.corp_mut().store(ICE_COMMODITY, 100_000); // plenty to grow on
                                                  // Walk the three promotions, each gated on its own (rising) population threshold.
    for expected in [RANK_COLONY, RANK_HUB, RANK_CAPITAL] {
        let need = promote_pop_threshold(expected - 1);
        while sim.outpost_at(body).unwrap().population < need {
            sim.corp_mut().store(ICE_COMMODITY, 1_000); // keep it fed
            sim.step();
        }
        assert!(
            sim.can_promote_outpost(body),
            "rung {expected}: maxed + facilities + population ⇒ promotable"
        );
        sim.corp_mut().credit(1_000_000);
        sim.promote_outpost(body).unwrap();
        assert_eq!(sim.outpost_at(body).unwrap().rank, expected);
        finish_build(&mut sim);
    }
    assert!(sim.has_capital());
    // A Capital is the top — no further promotion.
    assert!(!sim.can_promote_outpost(body), "Capital is the top rung");
}

#[test]
fn outposts_are_inert_by_default() {
    // No outposts ⇒ the economy is byte-identical to a world that never had the layer.
    let mut a = Sim::new(5);
    let mut b = Sim::new(5);
    for _ in 0..400 {
        a.step();
        let _ = b.outposts();
        b.step();
    }
    assert_eq!(a.corp().credits(), b.corp().credits());
}

#[test]
fn a_deployed_miner_stocks_the_warehouse_with_the_bodys_mineral() {
    // Early industry: a miner bought from Tycho and stationed at a body extracts
    // that body's raw into your warehouse each tick — the bootstrap before colonies.
    let mut sim = Sim::new(0);
    let body = sim.markets()[0].body(); // the home body (Ceres)
    let mineral = sim.body_mineral(body);
    let before = sim.corp().cargo(mineral);
    let credits0 = sim.corp().credits();
    sim.buy_miner(body).unwrap();
    assert_eq!(sim.miners().len(), 1);
    assert!(
        sim.corp().credits() < credits0,
        "buying a miner costs credits"
    );
    assert!(matches!(sim.buy_miner(0), Err(MinerError::BadSite))); // not the sun
                                                                   // Player mining is confined to the belts + outer moons; the Earth/Mars AO is off-limits.
    let earth = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Earth")
        .unwrap();
    let luna = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Luna")
        .unwrap();
    let titan = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Titan")
        .unwrap();
    assert!(matches!(sim.buy_miner(earth), Err(MinerError::BadSite)));
    assert!(matches!(sim.buy_miner(luna), Err(MinerError::BadSite))); // Earth's moon
    assert!(sim.can_mine_body(titan), "an outer moon is a valid site");
    assert!(sim.can_mine_body(body), "the belt (Ceres) is a valid site");
    for _ in 0..30 {
        sim.step();
    }
    assert!(
        sim.corp().cargo(mineral) >= before + 30 * MINER_OUTPUT_PER_TICK,
        "the miner stocked the warehouse with the body's mineral"
    );
    // The base buy is a christened, Prospector-class rig (the dedicated-ship treatment).
    assert_eq!(sim.miners()[0].class, MinerClass::Prospector);
    assert!(!sim.miners()[0].name.is_empty(), "a miner is christened");
}

#[test]
fn miner_tiers_cost_more_and_crew_but_out_yield_the_prospector() {
    // The dedicated-ship treatment: a Harvester is a pricier, crew-heavy, higher-yield
    // asset than the base Prospector — every rig a costly asset that gates expansion.
    let mut sim = Sim::new(0);
    let belt = sim.markets()[0].body();
    let mineral = sim.body_mineral(belt);
    sim.corp_mut().credit(200_000);
    // A Harvester needs crew the empty pool... actually a fresh corp has crew; drain it
    // to prove the crew gate bites.
    let crew0 = sim.corp().trained_crew();
    assert!(
        crew0 >= MinerClass::Harvester.crew(),
        "starting pool covers a Harvester"
    );
    // Yield: run a Harvester for 30 ticks and compare to the Prospector base rate.
    sim.commission_miner(belt, MinerClass::Harvester).unwrap();
    assert_eq!(sim.miners()[0].class, MinerClass::Harvester);
    assert!(
        sim.corp().trained_crew() < crew0,
        "a Harvester ties up crew (the §8c gate)"
    );
    let before = sim.corp().cargo(mineral);
    for _ in 0..30 {
        sim.step();
    }
    let harvested = sim.corp().cargo(mineral) - before;
    assert!(
        harvested >= 30 * MINER_OUTPUT_PER_TICK * MinerClass::Harvester.yield_mult(),
        "a Harvester out-yields the Prospector ({harvested} units in 30 ticks)"
    );
    // The crew gate bites: spend the pool down so a Refinery Barge can't be crewed.
    sim.corp_mut().credit(200_000);
    let titan = super::orbit::default_system()
        .iter()
        .position(|b| b.name == "Titan")
        .unwrap();
    let leave = MinerClass::RefineryBarge.crew() - 1; // one short of what the Barge needs
    let spend = sim.corp().trained_crew() - leave;
    sim.corp_mut().assign_crew(spend);
    assert!(matches!(
        sim.commission_miner(titan, MinerClass::RefineryBarge),
        Err(MinerError::NoCrew)
    ));
}

#[test]
fn the_base_miner_buy_is_byte_identical_to_the_prospector_commission() {
    // buy_miner must stay the exact old behaviour (the QA early-audit first move).
    let mut a = Sim::new(4);
    let mut b = Sim::new(4);
    let belt = a.markets()[0].body();
    a.buy_miner(belt).unwrap();
    b.commission_miner(belt, MinerClass::Prospector).unwrap();
    for _ in 0..50 {
        a.step();
        b.step();
    }
    assert_eq!(a.corp().credits(), b.corp().credits());
    assert_eq!(a.miners(), b.miners());
}

#[test]
fn haulers_are_tiered_named_ships_and_the_base_buy_is_byte_identical() {
    use crate::sim::corp::HaulerClass;
    // The base commission is a Light hauler (the byte-identical old freighter).
    let mut a = Sim::new(2);
    let mut b = Sim::new(2);
    a.corp_mut().credit(200_000);
    b.corp_mut().credit(200_000);
    a.commission_freighter().unwrap();
    b.commission_hauler(HaulerClass::Light).unwrap();
    assert_eq!(a.corp().credits(), b.corp().credits());
    assert_eq!(a.corp().haulers(), b.corp().haulers());
    assert_eq!(a.corp().haulers()[0].class, HaulerClass::Light);
    assert!(!a.corp().haulers()[0].name.is_empty(), "a hauler is named");
    // A Bulk hauler costs far more, ties up more crew, and lifts much more cargo.
    let credits0 = a.corp().credits();
    let crew0 = a.corp().trained_crew();
    a.commission_hauler(HaulerClass::Bulk).unwrap();
    assert!(credits0 - a.corp().credits() >= HaulerClass::Bulk.cost());
    assert!(crew0 - a.corp().trained_crew() == HaulerClass::Bulk.crew());
    assert_eq!(a.corp().best_hauler_cargo(), HaulerClass::Bulk.cargo());
    assert!(HaulerClass::Bulk.cargo() > HaulerClass::Light.cargo());
}

#[test]
fn a_bulk_hauler_lifts_a_fatter_route_than_a_light_one() {
    use crate::sim::corp::HaulerClass;
    // The tier's cargo cap is the route-throughput limit (the dispatch carries
    // min(route.qty, best_hauler_cargo)): a fat route only moves its full load once a big
    // enough hull is in the pool.
    let big_qty = HaulerClass::Light.cargo() + 60; // above Light's cap, within Bulk's
    let mut light = Sim::new(5);
    light.corp_mut().credit(2_000_000);
    light.commission_hauler(HaulerClass::Light).unwrap();
    let mut bulk = Sim::new(5);
    bulk.corp_mut().credit(2_000_000);
    bulk.commission_hauler(HaulerClass::Bulk).unwrap();
    assert!(
        light.corp().best_hauler_cargo() < big_qty,
        "a Light hull caps the fat route below its qty"
    );
    assert!(
        bulk.corp().best_hauler_cargo() >= big_qty,
        "a Bulk hull lifts the fat route whole"
    );
}

#[test]
fn a_miner_convoyed_with_a_hauler_out_yields_a_lone_rig() {
    use crate::sim::corp::HaulerClass;
    // Phase 4 synergy: pair a miner with a hauler in a convoy and it mines faster (the
    // hauler ferries the ore so the rig never stops). Measured against a lone rig.
    let mineral_at = |sim: &Sim| sim.body_mineral(sim.markets()[0].body());
    let run = |convoyed: bool| -> i64 {
        let mut sim = Sim::new(0);
        sim.corp_mut().credit(200_000);
        let belt = sim.markets()[0].body();
        sim.buy_miner(belt).unwrap();
        sim.commission_hauler(HaulerClass::Light).unwrap();
        if convoyed {
            assert!(sim.form_mining_convoy(belt).is_some());
            assert!(sim.miner_has_convoy_synergy(belt));
        }
        let m = mineral_at(&sim);
        let before = sim.corp().cargo(m);
        for _ in 0..50 {
            sim.step();
        }
        sim.corp().cargo(m) - before
    };
    let lone = run(false);
    let convoyed = run(true);
    assert!(
        convoyed > lone,
        "a convoyed miner ({convoyed}) out-yields a lone one ({lone})"
    );
}

#[test]
fn a_warship_can_escort_a_convoy_and_tighten_the_screen() {
    use crate::sim::corp::HaulerClass;
    use crate::sim::ships::ShipClass;
    let mut sim = Sim::new(1);
    sim.dev_grant_shipyard();
    sim.corp_mut().credit(2_000_000);
    let belt = sim.markets()[0].body();
    sim.buy_miner(belt).unwrap();
    sim.commission_hauler(HaulerClass::Light).unwrap();
    let id = sim.form_mining_convoy(belt).expect("a convoy forms");
    // No warship yet ⇒ can't escort.
    assert!(!sim.escort_convoy(id), "no free warship to assign");
    // Stand up a frigate (instant, via the test drain) and assign it as an escort.
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    let screen0 = sim.effective_escorts();
    assert!(sim.escort_convoy(id), "a warship escorts the convoy");
    assert_eq!(sim.convoy_escorts_at(belt), 1);
    assert_eq!(
        sim.effective_escorts(),
        screen0 + 1,
        "the escort tightens the screen"
    );
    // Can't assign more escorts than warships.
    assert!(
        !sim.escort_convoy(id),
        "only one warship, already escorting"
    );
    // Recall frees it.
    assert!(sim.recall_escort(id));
    assert_eq!(sim.convoy_escorts_at(belt), 0);
}

#[test]
fn convoys_are_inert_until_formed_byte_identical() {
    // With no convoys formed, the world is byte-identical to one that never had the layer.
    let mut a = Sim::new(6);
    let mut b = Sim::new(6);
    let belt = a.markets()[0].body();
    a.buy_miner(belt).unwrap();
    b.buy_miner(belt).unwrap();
    for _ in 0..300 {
        a.step();
        let _ = b.convoys();
        b.step();
    }
    assert_eq!(a.corp().credits(), b.corp().credits());
    assert_eq!(a.miners(), b.miners());
}

#[test]
fn armed_haulers_screen_the_convoy_and_the_opa_runner_needs_standing() {
    use crate::sim::corp::HaulerClass;
    let mut sim = Sim::new(3);
    sim.corp_mut().credit(500_000);
    // The OPA Q-Runner (torpedo-armed hull) is gated on OPA standing.
    assert!(!sim.can_commission_hauler(HaulerClass::OpaRunner));
    assert!(matches!(
        sim.commission_hauler(HaulerClass::OpaRunner),
        Err(CommissionError::NeedShipyard)
    ));
    sim.relations_mut()
        .adjust(crate::sim::Faction::Belt, CORVETTE_STANDING);
    assert!(sim.can_commission_hauler(HaulerClass::OpaRunner));
    sim.commission_hauler(HaulerClass::OpaRunner).unwrap();
    // Arm it: 1 PDC + 2 Ramshackle torpedoes → defense weight 1 + 2×2 = 5.
    let escorts0 = sim.effective_escorts();
    let credits0 = sim.corp().credits();
    sim.arm_hauler(0, 1, 2).unwrap();
    assert!(sim.corp().credits() < credits0, "weapons cost credits");
    assert_eq!(sim.corp().hauler_defense(), 5);
    // The armed hull adds to the empire's escort screen (5 / 4 = 1 escort).
    assert_eq!(sim.effective_escorts(), escorts0 + 1);
    // Mounts are capped: a Light hauler can't carry torpedoes.
    sim.commission_hauler(HaulerClass::Light).unwrap();
    sim.arm_hauler(1, 5, 3).unwrap(); // over-asks
    assert_eq!(sim.corp().haulers()[1].pdc, 1, "Light caps at 1 PDC");
    assert_eq!(
        sim.corp().haulers()[1].torpedo,
        0,
        "no torpedoes on a Light"
    );
}

#[test]
fn an_unarmed_trade_fleet_is_byte_identical_to_the_old_freighter_pool() {
    // Phase 3 must not perturb the economy for an unarmed fleet (the QA personas).
    let mut a = Sim::new(9);
    let mut b = Sim::new(9);
    a.corp_mut().credit(100_000);
    b.corp_mut().credit(100_000);
    a.commission_freighter().unwrap();
    b.commission_freighter().unwrap();
    for _ in 0..200 {
        a.step();
        b.step();
    }
    assert_eq!(a.corp().hauler_defense(), 0);
    assert_eq!(a.effective_escorts(), b.effective_escorts());
    assert_eq!(a.corp().credits(), b.corp().credits());
}

#[test]
fn the_powers_contest_the_major_hubs_and_courting_lets_you_claim_one() {
    // The major frontier hubs are fought over (the Ganymede conflict). Ambient
    // Earth/Mars flares shift the balance; the player gathers influence to claim one.
    let mut sim = Sim::new(0);
    assert!(sim.contested_count() > 0, "major hubs should be contested");
    // The ambient contest shifts influence over time (Earth/Mars tug-of-war).
    let before: Vec<[i64; 4]> = sim
        .contested_colonies()
        .iter()
        .map(|c| c.influence)
        .collect();
    for _ in 0..contest::FLARE_INTERVAL * 2 + 5 {
        sim.step();
    }
    let after: Vec<[i64; 4]> = sim
        .contested_colonies()
        .iter()
        .map(|c| c.influence)
        .collect();
    assert_ne!(
        before, after,
        "ambient great-power flares shift the balance"
    );
    // Court a colony enough to claim it (the slow gather-influence loop).
    sim.influence = 100_000; // plenty of statecraft resource
    let colony = sim.contested_colony(0).unwrap().colony;
    assert!(!sim.colony_controlled(colony));
    // Claiming before you've built standing is rejected.
    assert!(matches!(
        sim.claim_contested_colony(0),
        Err(ContestError::NotStrongEnough)
    ));
    while !sim.contested_colony(0).unwrap().claimable() {
        sim.court_contested_colony(0).unwrap();
    }
    sim.claim_contested_colony(0).unwrap();
    assert!(sim.colony_controlled(colony), "claimed the contested hub");
}

#[test]
fn the_contest_does_not_perturb_the_economy() {
    // The contest layer touches only its own numbers + the feed — never the market
    // RNG — so a world that watches/courts it stays bit-identical in the economy.
    let mut a = Sim::new(7);
    let mut b = Sim::new(7);
    for _ in 0..600 {
        a.step();
        // b reads the contest every tick (and the ambient flares fire in both).
        let _ = b.contested_colonies();
        b.step();
    }
    let stocks_a: Vec<Vec<i64>> = a
        .markets()
        .iter()
        .map(|m| m.stocks().iter().map(|s| s.stock).collect())
        .collect();
    let stocks_b: Vec<Vec<i64>> = b
        .markets()
        .iter()
        .map(|m| m.stocks().iter().map(|s| s.stock).collect())
        .collect();
    assert_eq!(stocks_a, stocks_b, "the contest never perturbs the economy");
}

#[test]
fn a_fresh_world_controls_no_colonies() {
    // The empire layer is inert by default — a fresh sim owns nothing, so the
    // §7c gate + existing economy are unaffected (no tribute, no rep shift).
    let mut sim = Sim::new(0);
    for _ in 0..200 {
        sim.step();
    }
    assert_eq!(sim.controlled_colony_count(), 0);
    assert_eq!(sim.holding_count(), 0);
}

#[test]
fn the_bridgehead_is_a_post_transit_endgame_verb() {
    // §17/G3: the far-side foothold can only be founded after transiting the gate,
    // costs credits, and is itself a spine op. Upgrading reinforces it.
    let mut sim = Sim::new(3);
    assert!(!sim.bridgehead().is_founded());
    // Can't found before the Beyond, even flush with cash.
    sim.corp_mut().credit(500_000);
    assert_eq!(
        sim.found_bridgehead(),
        Err(BridgeheadError::NotBeyond),
        "no foothold before the ring"
    );
    // Climb + transit into the Beyond.
    for _ in 0..200 {
        sim.complete_op();
    }
    assert!(sim.transit_gate());
    assert!(sim.campaign().transited());
    // Found it — costs credits, stands at level 1, and counts as an op.
    let before = sim.corp().credits();
    assert_eq!(sim.found_bridgehead(), Ok(()));
    assert!(sim.bridgehead().is_founded());
    assert_eq!(sim.bridgehead().level(), 1);
    assert!(sim.corp().credits() < before, "founding costs credits");
    assert_eq!(
        sim.found_bridgehead(),
        Err(BridgeheadError::AlreadyFounded),
        "no second founding"
    );
    // Upgrade reinforces it (raises the level + integrity).
    let max1 = sim.bridgehead().max_integrity();
    assert_eq!(sim.upgrade_bridgehead(), Ok(()));
    assert_eq!(sim.bridgehead().level(), 2);
    assert!(sim.bridgehead().max_integrity() > max1);
}

#[test]
fn incursions_only_fire_after_transit_and_damage_an_undefended_bridgehead() {
    // §17/G4: pre-transit no incursion ever fires (byte-identical world); after
    // transit they escalate, and an undefended one chips the bridgehead.
    let mut sim = Sim::new(5);
    // A long pre-transit run raises no incursion at all.
    for _ in 0..600 {
        sim.step();
    }
    assert!(!sim.incursion_pending());
    assert!(!sim.pressure().endgame());
    // Climb, transit, found the foothold.
    for _ in 0..200 {
        sim.complete_op();
    }
    assert!(sim.transit_gate());
    assert!(
        sim.pressure().endgame(),
        "transit lights the incursion clock"
    );
    sim.corp_mut().credit(500_000);
    assert_eq!(sim.found_bridgehead(), Ok(()));
    let full = sim.bridgehead().integrity();
    // Run long enough for an incursion to land and (undefended) lapse onto the
    // foothold — its integrity must fall.
    for _ in 0..400 {
        sim.step();
        if sim.bridgehead().integrity() < full {
            break;
        }
    }
    assert!(
        sim.bridgehead().integrity() < full,
        "an undefended incursion damages the bridgehead"
    );
}

#[test]
fn defending_an_incursion_protects_the_bridgehead() {
    // §17/G4: with a strong enough fleet, answering the incursion repels it and
    // the bridgehead takes no damage.
    let mut sim = Sim::new(11);
    yard(&mut sim);
    for _ in 0..200 {
        sim.complete_op();
    }
    assert!(sim.transit_gate());
    sim.corp_mut().credit(5_000_000);
    assert_eq!(sim.found_bridgehead(), Ok(()));
    // Stand up a frigate squadron (the 60-crew pool affords five) — a heavy
    // numeric edge over the 2-ship opening incursion pack, so the defense wins.
    for _ in 0..5 {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    sim.finish_pending_ships();
    assert!(!sim.corp().fleet().is_empty(), "a squadron stands ready");
    let full = sim.bridgehead().integrity();
    // Advance until an incursion is pending, then defend it.
    let mut defended = false;
    for _ in 0..400 {
        sim.step();
        if sim.incursion_pending() {
            let outcome = sim.defend_bridgehead(Band::Close);
            assert!(outcome.is_some(), "the fleet answers");
            assert!(!sim.incursion_pending(), "the incursion is resolved");
            defended = true;
            break;
        }
    }
    assert!(defended, "an incursion arrived to defend against");
    // A won defense leaves the foothold unscathed.
    assert_eq!(
        sim.bridgehead().integrity(),
        full,
        "a successful defense costs the bridgehead no integrity"
    );
}

#[test]
fn the_endgame_is_won_by_growing_and_holding_the_bridgehead() {
    // §17/G5: the journey completes when the bridgehead reaches the target level
    // *and* has weathered the required incursions — a genuine victory state.
    let mut sim = Sim::new(11);
    yard(&mut sim);
    for _ in 0..200 {
        sim.complete_op();
    }
    assert!(sim.transit_gate());
    assert_eq!(sim.endgame_outcome(), EndgameOutcome::Undecided);
    sim.corp_mut().credit(50_000_000);
    assert_eq!(sim.found_bridgehead(), Ok(()));
    for _ in 0..5 {
        let _ = sim.commission_ship(ShipClass::Frigate);
    }
    let (target_level, target_survived) = sim.endgame_targets();
    // Grow the bridgehead to (just below) the target — not yet a win without the
    // incursions weathered.
    while sim.bridgehead().level() < target_level {
        assert_eq!(sim.upgrade_bridgehead(), Ok(()));
    }
    assert_eq!(
        sim.endgame_outcome(),
        EndgameOutcome::Undecided,
        "level alone does not win — the far side must be held"
    );
    // Repel incursions until the threshold is met; the win then fires.
    let mut guard = 0;
    while sim.endgame_outcome() == EndgameOutcome::Undecided {
        sim.step();
        if sim.incursion_pending() {
            // Refit if the squadron was thinned, so defenses keep winning.
            while sim.corp().fleet().len() < 5 {
                if sim.commission_ship(ShipClass::Frigate).is_err() {
                    break;
                }
            }
            sim.defend_bridgehead(Band::Close);
        }
        guard += 1;
        assert!(guard < 20_000, "the endgame should resolve in bounded time");
    }
    assert_eq!(sim.endgame_outcome(), EndgameOutcome::Triumph);
    assert!(sim.incursions_survived() >= target_survived);
    // Resolution is terminal — no further incursions press.
    assert!(!sim.incursion_pending());
}

#[test]
fn the_endgame_is_lost_if_the_bridgehead_is_overrun() {
    // §17/G5: an undefended bridgehead ground to zero is the loss ending.
    let mut sim = Sim::new(5);
    for _ in 0..200 {
        sim.complete_op();
    }
    assert!(sim.transit_gate());
    sim.corp_mut().credit(500_000);
    assert_eq!(sim.found_bridgehead(), Ok(()));
    // Never defend — incursions grind the foothold down to nothing.
    let mut guard = 0;
    while sim.endgame_outcome() == EndgameOutcome::Undecided {
        sim.step();
        guard += 1;
        assert!(
            guard < 50_000,
            "an undefended bridgehead must eventually fall"
        );
    }
    assert_eq!(sim.endgame_outcome(), EndgameOutcome::Fallen);
    assert!(sim.bridgehead().has_fallen());
}

#[test]
fn the_far_side_markets_exist_in_deep_scarcity_without_perturbing_the_inner_economy() {
    // §17 endgame: the far-side markets are appended after the inner economy and
    // step on a dedicated RNG, so the pre-transit world is byte-identical. Prove
    // (a) they're present and correctly partitioned, (b) they sit deeper in
    // scarcity than the inner markets, and (c) running the world for a while
    // leaves the inner markets bit-identical to a sim that never reads them.
    let mut a = Sim::new(9);
    let mut b = Sim::new(9);
    let split = a.far_market_start;
    assert!(split > 0 && split < a.markets.len(), "far side is appended");
    for m in 0..a.markets.len() {
        assert_eq!(a.is_far_side_market(m), m >= split);
    }
    // Far-side raw/refined tiers start in deep scarcity (so prices ride high) —
    // dearer than the matching inner consumer market on the same good.
    let raw = 0usize;
    let far_price = a.markets[split].price(raw);
    let inner_dearest = a.markets[..split]
        .iter()
        .map(|m| m.price(raw))
        .max()
        .unwrap();
    assert!(
        far_price > inner_dearest,
        "the far side should be dearer ({far_price} vs {inner_dearest})"
    );
    // Drive both worlds; `a` polls the far side every tick, `b` never does.
    for _ in 0..400 {
        a.step();
        b.step();
        for m in a.far_market_start..a.markets.len() {
            let _ = a.markets[m].price(0);
        }
    }
    for m in 0..split {
        for c in 0..a.markets[m].defs().len() {
            assert_eq!(
                a.markets[m].price(c),
                b.markets[m].price(c),
                "inner market {m} commodity {c} drifted — far side perturbed it"
            );
        }
    }
}

#[test]
fn building_and_routing_advance_the_spine_too() {
    // The retention spine used to count only interdictions; now the build and
    // logistics side of the influence model climbs it as well (§0). A few
    // commissions plus a self-running route should advance past the Station
    // with no raiding at all.
    use crate::sim::campaign::Tier;
    let mut sim = Sim::new(0);
    yard(&mut sim);
    assert_eq!(sim.campaign().tier(), Tier::Station);
    // Two commissions are two operations on their own.
    sim.commission_freighter().unwrap();
    sim.commission_ship(ShipClass::Frigate).unwrap();
    // A standing route then delivers itself toward the next rung.
    sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
    for _ in 0..3_000 {
        sim.step();
        if sim.campaign().tier() != Tier::Station {
            break;
        }
    }
    assert_ne!(
        sim.campaign().tier(),
        Tier::Station,
        "build + route operations should climb the spine without interdiction"
    );
    // ...and none of it touched reputation (no cuts were made).
    for m in sim.markets() {
        assert!(sim.relations().standing(m.faction()) >= 0);
    }
}

#[test]
fn a_warship_can_be_assembled_from_produced_components() {
    // §7d payoff: a player who has built up the production chain can *assemble*
    // a warship from their own Assembled-tier stock for a fraction of the
    // off-the-yard credit price — the bill-of-materials link from economy to fleet.
    let mut sim = Sim::new(0);
    yard(&mut sim);
    // Empty warehouse ⇒ no parts ⇒ can't assemble.
    assert_eq!(
        sim.assemble_ship(ShipClass::Frigate),
        Err(CommissionError::MissingParts)
    );
    // Stock the frigate's bill of materials (2 Machinery #10, 1 Drives #11).
    for &(c, q) in Sim::ship_bom(ShipClass::Frigate) {
        sim.corp_mut().store(c, q);
    }
    let credits_before = sim.corp().credits();
    let fleet_before = sim.corp().fleet().len();
    sim.assemble_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    assert_eq!(
        sim.corp().fleet().len(),
        fleet_before + 1,
        "hull joined the fleet"
    );
    // The parts were consumed...
    assert_eq!(sim.corp().cargo(10), 0, "Machinery consumed");
    assert_eq!(sim.corp().cargo(11), 0, "Drives consumed");
    // ...and assembling cost far less than buying the hull off the yard.
    let assembly_spend = credits_before - sim.corp().credits();
    let yard_price = ships::hull(ShipClass::Frigate).dry_mass * SHIP_PRICE_PER_MASS;
    assert!(
        assembly_spend < yard_price,
        "assembling from owned parts ({assembly_spend}) is cheaper than the yard ({yard_price})"
    );
}

#[test]
fn a_ship_can_be_renamed_keeping_its_class() {
    // §14 expressive identity: the player renames a hull's call-sign; the class
    // suffix is preserved and an empty name is rejected.
    let mut sim = Sim::new(0);
    yard(&mut sim);
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    assert!(sim.rename_ship(0, "Valkyrie"));
    assert_eq!(sim.corp().fleet()[0].name, "Valkyrie (Frigate)");
    assert!(!sim.rename_ship(0, "   "), "blank names are rejected");
    assert!(!sim.rename_ship(9, "Ghost"), "no such ship");
}

#[test]
fn the_fleet_can_actually_fight() {
    // The combat resolver was unreachable from the live loop; now a player
    // with warships can engage a raider pack and the battle resolves into a
    // BattleResolved event (§9). With no fleet, there's nothing to send.
    use crate::sim::combat::Band;
    let mut sim = Sim::new(0);
    yard(&mut sim);
    assert!(
        sim.engage_raiders(Band::Medium).is_none(),
        "no warships ⇒ no engagement"
    );
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    let fleet_before = sim.corp().fleet().len();
    let outcome = sim.engage_raiders(Band::Medium).expect("a fleet can fight");
    assert!(outcome.winner.is_some() || outcome.ticks > 0);
    // The battle resolves into an event the feed can voice (surviving the
    // step's player-event plumbing).
    let events = sim.step().to_vec();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::BattleResolved { .. })),
        "the engagement should emit a BattleResolved event"
    );
    // Losses are applied to the fleet (it can only shrink).
    assert!(sim.corp().fleet().len() <= fleet_before);
}

#[test]
fn an_off_station_fleet_cannot_defend_the_core() {
    // Pillar #2: combat is positional. Warships defend the home core only when
    // on station there; fly them away and the core is undefended until they
    // burn home — so the delta-v movement layer is consequential.
    use crate::sim::combat::Band;
    let mut sim = Sim::new(0);
    yard(&mut sim);
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.commission_ship(ShipClass::Frigate).unwrap();
    sim.finish_pending_ships();
    assert_eq!(sim.warships_on_station(), 2, "fresh hulls dock at the core");
    assert!(
        sim.engage_raiders(Band::Medium).is_some(),
        "on-station fleet can fight"
    );

    // Send the survivors to Earth (body 3): in transit ⇒ off station.
    for i in 0..sim.corp().fleet().len() {
        let _ = sim.move_ship(i, 3, false);
    }
    assert_eq!(
        sim.warships_on_station(),
        0,
        "a departed fleet is off station"
    );
    assert!(
        sim.engage_raiders(Band::Medium).is_none(),
        "the core is undefended while the fleet is away"
    );

    // Let them arrive at Earth — docked, but at the wrong body, still no defence.
    for _ in 0..3_000 {
        sim.step();
        if !sim
            .corp()
            .fleet()
            .iter()
            .any(|s| s.nav.in_transit(sim.tick()))
        {
            break;
        }
    }
    assert_eq!(
        sim.warships_on_station(),
        0,
        "docked at Earth is not on station at the core"
    );

    // Recall one hull home; the core can be defended again.
    let muster = sim.markets()[0].body();
    sim.refuel_ship(0);
    sim.move_ship(0, muster, false)
        .expect("a frigate can burn home");
    for _ in 0..3_000 {
        sim.step();
        if !sim.corp().fleet()[0].nav.in_transit(sim.tick()) {
            break;
        }
    }
    assert_eq!(
        sim.warships_on_station(),
        1,
        "the recalled hull stands guard"
    );
    assert!(
        sim.engage_raiders(Band::Medium).is_some(),
        "a fleet back on station can fight again"
    );
}

#[test]
fn a_refinery_runs_the_value_add_chain_for_profit() {
    // Found a refinery (Ore → Metals): it sources cheap raw, refines it into a
    // dearer good, and auto-sells the surplus — hands-off (§3.1, Example A).
    let mut sim = Sim::new(0);
    let before = sim.corp().credits();
    sim.found_refinery(1, 0, 0).unwrap(); // Ore, buy+sell at Ceres
    assert_eq!(sim.stations().len(), 1);
    assert!(sim.corp().credits() < before, "founding costs capital");
    let after_found = sim.corp().credits();
    for _ in 0..1_500 {
        sim.step();
    }
    assert!(
        sim.corp().credits() > after_found,
        "the refinery should net profit"
    );
}

#[test]
fn the_production_chain_runs_four_tiers_deep() {
    // §7d: the chain is Raw → Refined → Components → Assembled. A station can be
    // founded at any non-top tier, refining into the next tier up its line —
    // Ore(1) → Metals(4) → Alloys(7) → Machinery(10). Each step is a real
    // value-add: the output anchors dearer than the input.
    let defs = super::super::economy::default_commodities();
    // The line is contiguous +3 and strictly climbs in price.
    for &i in &[1usize, 4, 7] {
        assert!(
            defs[i + 3].base_price > defs[i].base_price,
            "tier {i} refines into a dearer good"
        );
    }
    // A component factory (Metals → Alloys) is a valid recipe and produces its
    // tier-2 output hands-off.
    let mut sim = Sim::new(0);
    sim.found_refinery(4, 0, 0).unwrap(); // Metals → Alloys at Ceres
    assert_eq!(sim.stations()[0].output, 7, "Metals refines into Alloys");
    // Seed some Metals into the source market so the factory has feedstock.
    for _ in 0..2_000 {
        sim.step();
    }
    assert!(
        sim.corp().cargo(7) > 0 || sim.markets()[0].stock(7) > 0,
        "the component factory should have produced Alloys somewhere"
    );
    // The top tier has nothing higher to refine into.
    assert_eq!(
        sim.found_refinery(10, 0, 0),
        Err(FoundError::NotProcessable)
    );
}

#[test]
fn refineries_are_guarded() {
    let mut sim = Sim::new(0);
    // A top-tier finished good has no higher tier to refine into (§7d).
    let top = sim.markets()[0].defs().len() - 1; // Drives
    assert_eq!(
        sim.found_refinery(top, 0, 0),
        Err(FoundError::NotProcessable)
    );
    // ...but a mid-chain commodity (Metals → Alloys) now *is* a valid recipe.
    assert!(
        sim.found_refinery(4, 0, 0).is_ok(),
        "components are producible"
    );
    // Found stations until a guard fires. Founding is an op that climbs the
    // spine, and the cap *widens* with the tier (§0.3), so the count is never
    // allowed to exceed the *current* tier's cap, and a guard (cap or capital)
    // eventually stops the spree.
    let mut last_err = None;
    for _ in 0..20 {
        match sim.found_refinery(1, 0, 0) {
            Ok(()) => assert!(
                sim.stations().len() <= sim.campaign().station_cap(),
                "must never exceed the tier station cap"
            ),
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }
    assert!(
        matches!(
            last_err,
            Some(FoundError::TooManyStations) | Some(FoundError::CantAfford)
        ),
        "founding is bounded by the tier cap or capital, got {last_err:?}"
    );
}

#[test]
fn a_trade_route_runs_itself_for_profit() {
    // The standing-order heart (§4): set the params + own a freighter, and the
    // sim flies the loop, banking the spread with no further input.
    let mut sim = Sim::new(0);
    sim.commission_freighter().unwrap();
    sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
    let start = sim.corp().credits();
    for _ in 0..2_000 {
        sim.step();
    }
    assert!(
        sim.corp().credits() > start,
        "the route should bank profit hands-off"
    );
}

#[test]
fn a_route_needs_a_freighter_and_respects_its_margin() {
    // No freighter ⇒ no trips.
    let mut sim = Sim::new(0);
    sim.set_trade_route(5, 1, 0, 20, 1);
    let start = sim.corp().credits();
    for _ in 0..500 {
        sim.step();
    }
    assert_eq!(
        sim.corp().credits(),
        start,
        "no freighter ⇒ the route can't run"
    );
    // With a freighter but an unreachable margin, the route stays idle.
    let mut sim = Sim::new(0);
    sim.commission_freighter().unwrap();
    sim.set_trade_route(5, 1, 0, 20, 100_000);
    let start = sim.corp().credits();
    for _ in 0..500 {
        sim.step();
    }
    assert_eq!(
        sim.corp().credits(),
        start,
        "spread below margin ⇒ idle (an exception)"
    );
}

#[test]
fn the_route_table_runs_many_routes_on_a_shared_freighter_pool() {
    // The §4 master-table: several standing routes run concurrently, bounded
    // by how many freighters are in the pool. Two freighters + three routes
    // ⇒ at most two trips in flight at once, and the table still banks profit.
    let mut sim = Sim::new(0);
    sim.commission_freighter().unwrap();
    sim.commission_freighter().unwrap();
    sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
    sim.set_trade_route(4, 0, 1, 20, 1); // Metals, Ceres → Earth
    sim.set_trade_route(1, 0, 1, 20, 1); // Ore, Ceres → Earth
    assert_eq!(sim.routes().len(), 3, "three routes sit in the table");
    let start = sim.corp().credits();
    let mut max_in_flight = 0;
    for _ in 0..2_000 {
        sim.step();
        let flying = sim.routes().iter().filter(|r| r.in_transit).count();
        max_in_flight = max_in_flight.max(flying);
    }
    assert!(
        max_in_flight <= 2,
        "two freighters cap concurrent trips at 2, saw {max_in_flight}"
    );
    assert!(max_in_flight >= 2, "both freighters should get used");
    assert!(sim.corp().credits() > start, "the table should bank profit");
}

#[test]
fn a_flying_freighter_has_a_real_position_on_its_lane() {
    // §6 positional logistics: a freighter running a standing route is a located
    // asset — its position sits between the origin and destination market bodies
    // and advances along the lane as the trip progresses.
    let mut sim = Sim::new(0);
    sim.commission_freighter().unwrap();
    sim.set_trade_route(5, 1, 0, 20, 1); // ReactorFuel, Earth → Ceres
                                         // Step until a freighter is dispatched.
    let mut flying = Vec::new();
    for _ in 0..2_000 {
        sim.step();
        flying = sim.flying_routes();
        if !flying.is_empty() {
            break;
        }
    }
    assert!(!flying.is_empty(), "the route should dispatch a freighter");
    let r = flying[0];
    let p0 = sim.route_freighter_pos(r);
    let early = sim.route_progress_bp(r);
    // Position is a real point (not the origin-only placeholder of the old model).
    assert!(p0 != (0, 0), "a flying freighter has a position");
    // Advance and confirm the trip progresses toward the destination.
    for _ in 0..30 {
        sim.step();
        if !sim.routes()[r].in_transit {
            break;
        }
    }
    if sim.routes()[r].in_transit {
        assert!(
            sim.route_progress_bp(r) > early,
            "the freighter advances along its lane over time"
        );
    }
}

#[test]
fn a_route_trip_burns_remass_scaled_by_distance() {
    // §6 delta-v as opex: a freighter refuels with Remass at the origin port,
    // an amount scaled by trip length — so a long outer haul burns more fuel
    // than a short inner hop. (The fuel is debited + drawn from the port at
    // dispatch in run_logistics; here we assert the distance-scaling that drives
    // it, which is deterministic — market stock is too noisy to assert on.)
    let mut sim = Sim::new(0);
    sim.set_trade_route(1, 0, 1, 20, 1); // inner: Ceres → Mars
    sim.set_trade_route(1, 0, 5, 20, 1); // outer: Ceres → a frontier hub
    let inner = sim.route_remass_units(0);
    let outer = sim.route_remass_units(1);
    assert!(inner >= 1, "every trip burns at least one unit of fuel");
    assert!(
        outer > inner,
        "the long outer haul ({outer}) burns more fuel than the inner hop ({inner})"
    );
}

#[test]
fn the_route_table_is_capped() {
    let mut sim = Sim::new(0);
    for _ in 0..10 {
        sim.set_trade_route(5, 1, 0, 20, 1);
    }
    assert_eq!(
        sim.routes().len(),
        4,
        "the table is capped at the tier route cap"
    );
    sim.clear_trade_route();
    assert!(sim.routes().is_empty(), "clearing empties the whole table");
}

#[test]
fn operations_climb_the_retention_spine() {
    // Each player interdiction is an operation on the climb; three of them
    // ascend past the Station and draw the gate closer (§0.3).
    use crate::sim::campaign::Tier;
    let mut sim = Sim::new(0);
    assert_eq!(sim.campaign().tier(), Tier::Station);
    let mut ops = 0;
    for _ in 0..400 {
        if let Some(h) = sim.haulers().first() {
            let id = h.id;
            if sim.interdict(id) {
                ops += 1;
            }
        }
        sim.step();
        if sim.campaign().tier() != Tier::Station {
            break;
        }
    }
    assert!(ops >= 3, "should have completed operations, got {ops}");
    assert_ne!(
        sim.campaign().tier(),
        Tier::Station,
        "should climb past the Station"
    );
    assert!(
        sim.campaign().gate_progress_bp() > 0,
        "the gate should draw closer"
    );
}

#[test]
fn progression_advances_through_the_sim() {
    let mut sim = Sim::new(0);
    sim.progression_mut().ceo.gain_xp(3_000);
    assert_eq!(sim.progression().ceo.level(), 4);
    sim.progression_mut().research.add_points(1_000);
    assert!(sim.progression_mut().research.research(0).is_ok());
    assert!(sim.progression().research.is_unlocked(0));
    // Generic blueprint discoverable; the Martian design stays rep-gated
    // until Mars standing is high enough (§10).
    assert!(sim.discover_blueprint(0));
    assert!(!sim.discover_blueprint(2));
    sim.relations_mut()
        .adjust(crate::sim::faction::Faction::Mars, 500);
    assert!(sim.discover_blueprint(2));
}

#[test]
fn automation_interdicts_only_targeted_shipping() {
    // Set a standing order to hunt Earth shipping; the manager runs it for
    // us, souring Earth while leaving off-target factions alone (§12).
    let mut sim = Sim::new(0);
    sim.policy_mut().interdiction.enabled = true;
    sim.policy_mut().interdiction.target = Some(crate::sim::faction::Faction::Earth);
    for _ in 0..1_000 {
        sim.step();
    }
    assert!(
        sim.relations()
            .standing(crate::sim::faction::Faction::Earth)
            < 0,
        "the patrol should have cut Earth shipping"
    );
    assert_eq!(
        sim.relations().standing(crate::sim::faction::Faction::Belt),
        0,
        "Belt shipping was off-target and untouched"
    );
}

#[test]
fn automation_min_cargo_spares_small_fry() {
    let mut sim = Sim::new(0);
    sim.policy_mut().interdiction.enabled = true;
    sim.policy_mut().interdiction.min_cargo = 1_000_000; // nothing is this big
    for _ in 0..1_000 {
        sim.step();
    }
    for m in sim.markets() {
        assert_eq!(sim.relations().standing(m.faction()), 0);
    }
}

#[test]
fn automation_auto_researches_when_funded() {
    let mut sim = Sim::new(0);
    sim.policy_mut().auto_research = true;
    sim.progression_mut().research.add_points(1_000);
    sim.step();
    assert!(sim.progression().research.unlocked_count() > 0);
}

#[test]
fn player_interdiction_sours_relations() {
    // Cutting a faction's hauler lowers the player's standing with them (§7b/§10).
    let mut sim = Sim::new(0);
    let id = fly_a_hauler(&mut sim);
    let origin = sim.haulers().iter().find(|h| h.id == id).unwrap().origin;
    let faction = sim.markets()[origin].faction();
    assert!(sim.interdict(id));
    assert!(
        sim.relations().standing(faction) < 0,
        "the owner should resent it"
    );
}

#[test]
fn hostility_recovers_once_the_raiding_stops() {
    // Drive Earth to Hostile, then stop: standing must drift back toward
    // neutral over time (§10) — the cliff is now a dial.
    use crate::sim::faction::Faction;
    let mut sim = Sim::new(0);
    sim.relations_mut().adjust(Faction::Earth, -1_000);
    assert_eq!(sim.relations().standing(Faction::Earth), -1_000);
    for _ in 0..2_000 {
        sim.step();
    }
    let healed = sim.relations().standing(Faction::Earth);
    assert!(
        healed > -1_000,
        "Earth should be recovering, still at {healed}"
    );
    assert!(
        healed < 0,
        "but a deep grudge shouldn't fully heal that fast"
    );
}

#[test]
fn salvage_discovers_wrecks_without_perturbing_the_economy() {
    // A world where the player strips every sighted wreck keeps bit-identical
    // *markets* to one that ignores them — the salvage field's own RNG (§15)
    // never advances the world economy (the §27 contract-board lesson).
    let mut control = Sim::new(5);
    let mut salvager = Sim::new(5);
    let (mut sighted, mut stripped) = (0, 0);
    for _ in 0..2_000 {
        control.step();
        for e in salvager.step().to_vec() {
            if let Event::WreckSighted { .. } = e {
                sighted += 1;
            }
        }
        // Strip whatever's adrift; rewards land in the corp/progression, not
        // the markets.
        while salvager.salvage_top() {
            stripped += 1;
        }
        for (cm, sm) in control.markets().iter().zip(salvager.markets()) {
            assert_eq!(cm.stocks(), sm.stocks(), "salvage perturbed the economy");
        }
    }
    assert!(sighted > 0, "the field should turn up wrecks over the run");
    assert_eq!(sighted, stripped, "every sighted wreck was strippable");
}

#[test]
fn pirate_raids_do_not_blame_the_player() {
    // Pirates thin the lanes for thousands of ticks; the player's standings
    // stay neutral (the raids aren't attributed to them).
    let mut sim = Sim::new(0);
    for _ in 0..2_000 {
        sim.step();
    }
    for m in sim.markets() {
        assert_eq!(sim.relations().standing(m.faction()), 0);
    }
}

#[test]
fn the_alert_feed_voices_the_run() {
    // Over a run the feed fills with ranked alerts, including act-now
    // shortages tagged with a verb (§19/§0.4). Act-now alerts age out after a
    // TTL, so we watch the whole run rather than only the final tick.
    let mut sim = Sim::new(0);
    let mut saw_act_now = false;
    for _ in 0..3_000 {
        sim.step();
        if sim
            .feed()
            .surfaced()
            .iter()
            .any(|a| a.is_act_now() && a.verb.is_some())
        {
            saw_act_now = true;
        }
    }
    assert!(
        !sim.feed().surfaced().is_empty(),
        "the feed should have something to say"
    );
    assert!(
        saw_act_now,
        "an interdicted run should raise act-now shortages"
    );
}

#[test]
fn pirates_raid_the_lanes() {
    // Over a long run the ambient raider lands strikes, each tagging a
    // destination scarcity (§7b/§13).
    let mut sim = Sim::new(0);
    let (mut cuts, mut scarcities) = (0, 0);
    for _ in 0..4_000 {
        for e in sim.step() {
            match e {
                Event::HaulerInterdicted { .. } => cuts += 1,
                Event::Scarcity { .. } => scarcities += 1,
                _ => {}
            }
        }
    }
    assert!(cuts > 0, "pirates never struck the lanes");
    assert_eq!(cuts, scarcities, "every cut should leave a scarcity");
}

#[test]
fn snapshot_has_bodies_and_markets() {
    let mut sim = Sim::new(1);
    for _ in 0..50 {
        sim.step();
    }
    let snap = sim.snapshot();
    assert_eq!(snap.tick, 50);
    assert_eq!(snap.bodies.len(), default_system().len());
    // 6 inner markets + 2 far-side endgame markets (§17).
    assert_eq!(snap.markets.len(), 8);
    assert_eq!((snap.bodies[0].x, snap.bodies[0].y), (0, 0)); // Sol fixed
}

#[test]
fn same_seed_yields_identical_runs() {
    let mut a = Sim::new(42);
    let mut b = Sim::new(42);
    for _ in 0..600 {
        assert_eq!(a.step(), b.step());
        assert_eq!(a.snapshot(), b.snapshot());
    }
}

#[test]
fn markets_carry_a_standing_arbitrage_spread() {
    // Ceres (producer) is cheaper than Earth (consumer) on raw Ore.
    let sim = Sim::new(0);
    let ore = 1;
    assert!(sim.markets()[0].price(ore) < sim.markets()[1].price(ore));
    // ...and dearer than Earth on refined Metals.
    let metals = 4;
    assert!(sim.markets()[0].price(metals) > sim.markets()[1].price(metals));
}

#[test]
fn haulers_fly_the_routes() {
    let mut sim = Sim::new(3);
    let mut saw_hauler = false;
    for _ in 0..500 {
        sim.step();
        saw_hauler |= !sim.haulers().is_empty();
    }
    assert!(saw_hauler, "arbitrage never spawned a hauler");
}

#[test]
fn trade_damps_the_spread() {
    // ReactorFuel carries the largest spread (Ceres dear, Earth cheap), so it
    // gets the most traffic; with haulers flowing its average spread settles
    // below the no-trade structural value.
    let mut sim = Sim::new(5);
    let rf = 5;
    let spread = |s: &Sim| s.markets()[0].price(rf) - s.markets()[1].price(rf);
    let structural = spread(&sim);
    for _ in 0..2_000 {
        sim.step();
    }
    let (mut sum, mut count) = (0i64, 0i64);
    for _ in 0..400 {
        sim.step();
        sum += spread(&sim);
        count += 1;
    }
    let avg = sum / count;
    assert!(
        avg < structural,
        "avg spread {avg} not damped below {structural}"
    );
    assert!(avg > 0, "the structural spread should persist, just damped");
}

#[test]
fn interdiction_starves_the_destination() {
    // Two identical runs; in one we cut the first hauler. The RNG (market
    // jitter) stays aligned across both, so the only divergence is the
    // denied delivery — leaving the destination dearer (a shortage, §7b).
    let mut control = Sim::new(1);
    let mut cut = Sim::new(1);
    let (id, dest, commodity, arrival) = loop {
        control.step();
        cut.step();
        if let Some(h) = cut.haulers().first() {
            break (h.id, h.dest, h.commodity, h.arrival_tick);
        }
    };
    assert!(cut.interdict(id));
    assert!(!cut.interdict(id), "a cut hauler cannot be cut twice");
    while cut.tick() < arrival {
        control.step();
        cut.step();
    }
    assert!(
        cut.markets()[dest].price(commodity) > control.markets()[dest].price(commodity),
        "interdiction did not raise the destination price"
    );
}

/// The §7c gate, re-checked with the §7b traffic layer running: trade must
/// not destabilize any market on any seed.
#[test]
fn no_death_spiral_with_traffic_on_any_seed() {
    for seed in 0..32u64 {
        let mut sim = Sim::new(seed);
        let mut ok = true;
        for _ in 0..5_000 {
            sim.step();
            for m in sim.markets() {
                for (d, s) in m.defs().iter().zip(m.stocks()) {
                    ok &= s.stock > 0 && s.stock < d.max_stock + d.target_stock;
                    ok &= s.price > d.floor && s.price < d.ceiling;
                }
            }
        }
        assert!(ok, "death-spiral with traffic on seed {seed}");
    }
}

#[test]
fn the_board_posts_and_caps_contracts() {
    let mut sim = Sim::new(7);
    // The board fills to its cap and never exceeds it.
    for _ in 0..2_000 {
        sim.step();
        assert!(sim.open_contract_count() <= MAX_CONTRACTS);
    }
    assert_eq!(
        sim.open_contract_count(),
        MAX_CONTRACTS,
        "a healthy world keeps the job menu full"
    );
}

#[test]
fn fulfilling_a_contract_pays_and_lifts_reputation() {
    let mut sim = Sim::new(11);
    // Let the board post some offers.
    for _ in 0..CONTRACT_INTERVAL {
        sim.step();
    }
    let c = *sim.contracts().first().expect("an offer should be posted");
    // Stock the warehouse with exactly what the contract owes, then fulfil.
    sim.corp.store(c.commodity, c.qty);
    let before_credits = sim.corp().credits();
    let before_rep = sim.relations().standing(c.faction);
    assert!(sim.accept_contract(c.id));
    let reward = sim
        .fulfill_contract(c.id)
        .expect("fulfilment should succeed");
    assert_eq!(reward, c.reward);
    assert_eq!(sim.corp().credits(), before_credits + c.reward);
    assert_eq!(sim.relations().standing(c.faction), before_rep + c.rep);
    assert_eq!(
        sim.corp().cargo(c.commodity),
        0,
        "the owed cargo is consumed"
    );
    assert!(
        sim.contracts().iter().all(|o| o.id != c.id),
        "a fulfilled contract leaves the board"
    );
}

#[test]
fn a_contract_must_be_accepted_and_stocked_to_fulfil() {
    let mut sim = Sim::new(13);
    for _ in 0..CONTRACT_INTERVAL {
        sim.step();
    }
    let c = *sim.contracts().first().expect("an offer should be posted");
    // Not accepted yet → NotAccepted.
    assert_eq!(sim.fulfill_contract(c.id), Err(ContractError::NotAccepted));
    // Accepted but empty warehouse → InsufficientCargo.
    assert!(sim.accept_contract(c.id));
    assert_eq!(
        sim.fulfill_contract(c.id),
        Err(ContractError::InsufficientCargo)
    );
    // A bogus id is NotFound.
    assert_eq!(sim.fulfill_contract(99_999), Err(ContractError::NotFound));
}

#[test]
fn unaccepted_contracts_lapse_but_accepted_ones_persist() {
    let mut sim = Sim::new(17);
    for _ in 0..CONTRACT_INTERVAL {
        sim.step();
    }
    let c = *sim.contracts().first().expect("an offer should be posted");
    assert!(sim.accept_contract(c.id));
    // Run well past the delivery window; the accepted contract is still owed.
    for _ in 0..(CONTRACT_WINDOW + CONTRACT_INTERVAL) {
        sim.step();
    }
    assert!(
        sim.contracts().iter().any(|o| o.id == c.id && o.accepted),
        "an accepted contract does not lapse"
    );
}

#[test]
fn the_contract_board_does_not_perturb_the_economy() {
    // The board has its own RNG, so a world *with* contract postings must be
    // bit-identical in its economy to one where we never read the board —
    // proving offer generation never advances the shared world streams (§27).
    let mut a = Sim::new(23);
    let mut b = Sim::new(23);
    for _ in 0..1_000 {
        a.step();
        // `b` additionally pokes the board read paths every tick.
        b.step();
        let _ = b.contracts();
        let _ = b.open_contract_count();
    }
    for (ma, mb) in a.markets().iter().zip(b.markets()) {
        for c in 0..ma.defs().len() {
            assert_eq!(ma.price(c), mb.price(c), "economy diverged");
            assert_eq!(ma.stock(c), mb.stock(c), "stock diverged");
        }
    }
}
