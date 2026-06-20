//! `empire` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// The frontier colonies (the empire layer) — static identity + faction.
    pub fn colonies(&self) -> &[Colony] {
        &self.colonies
    }

    /// Whether the player controls colony `i`.
    pub fn colony_controlled(&self, i: usize) -> bool {
        self.controlled.get(i).copied().unwrap_or(false)
    }

    /// How many frontier colonies the player controls — the empire's size.
    pub fn controlled_colony_count(&self) -> usize {
        self.controlled.iter().filter(|&&c| c).count()
    }

    /// Total holdings the player runs: the stations they built + the colonies they
    /// control (the unified empire view the EMPIRE panel reads).
    pub fn holding_count(&self) -> usize {
        self.stations.len() + self.controlled_colony_count()
    }

    /// The empire's standing in the system, by holdings (E6) — the headline of the
    /// expansion spine: a legible rank that climbs as you consolidate the frontier.
    pub fn empire_rank(&self) -> &'static str {
        match self.holding_count() {
            0 => "Independent Operator",
            1..=2 => "Local Power",
            3..=5 => "Regional Power",
            6..=9 => "Great Power",
            _ => "Hegemon",
        }
    }

    /// The next empire rank and the holdings it takes to reach it (E6), or `None` at
    /// the summit — the *next* rung of the expansion spine, always visible.
    pub fn next_empire_rank(&self) -> Option<(&'static str, usize)> {
        match self.holding_count() {
            0 => Some(("Local Power", 1)),
            1..=2 => Some(("Regional Power", 3)),
            3..=5 => Some(("Great Power", 6)),
            6..=9 => Some(("Hegemon", 10)),
            _ => None,
        }
    }

    /// Independent colonies the player could **buy** right now (not a great power's
    /// territory, not already controlled) — the economic acquisition targets.
    pub fn acquirable_colonies(&self) -> Vec<usize> {
        (0..self.colonies.len())
            .filter(|&i| self.is_acquirable(i))
            .collect()
    }

    pub(crate) fn is_acquirable(&self, i: usize) -> bool {
        matches!(self.colonies.get(i), Some(c) if c.faction == Faction::Independents)
            && !self.colony_controlled(i)
    }

    /// The credit price to buy colony `i` (markets cost more than outposts), or
    /// `None` if it isn't an acquirable target.
    pub fn colony_acquire_cost(&self, i: usize) -> Option<i64> {
        let c = self.colonies.get(i)?;
        if c.faction != Faction::Independents {
            return None;
        }
        Some(if c.is_market {
            COLONY_PRICE_MARKET
        } else {
            COLONY_PRICE_OUTPOST
        })
    }

    /// **Buy out an independent frontier colony** (the empire layer's economic
    /// acquisition path): pay its price, take control, and pay the political cost —
    /// the inner powers grow wary of a rising outer corporation while the Belt
    /// approves (`Relations::on_player_expand`). Taking ground is a spine op (§0).
    pub fn acquire_colony(&mut self, i: usize) -> Result<(), AcquireError> {
        if self.colony_controlled(i) {
            return Err(AcquireError::AlreadyControlled);
        }
        if !self.is_acquirable(i) {
            return Err(AcquireError::NotAcquirable);
        }
        let cost = self
            .colony_acquire_cost(i)
            .ok_or(AcquireError::NotAcquirable)?;
        if self.corp.credits() < cost {
            return Err(AcquireError::CantAfford);
        }
        self.corp.debit(cost);
        self.controlled[i] = true;
        // The political cost: expansion is never free (be careful not to overextend).
        self.relations.on_player_expand();
        // …and it spikes the inners' alarm — expand too fast and they unite (E3/E7):
        // taking the independent frontier is watched by Earth and Mars alike.
        self.raise_alarm(Faction::Earth, ALARM_PER_ACQUISITION);
        self.raise_alarm(Faction::Mars, ALARM_PER_ACQUISITION);
        // Buying a colony out from under its operator sours the relationship (E8).
        if let Some(ci) = self.diplomacy.company_for_colony(i) {
            self.diplomacy.adjust(ci, -BUYOUT_RELATION_HIT);
        }
        self.events.push(Event::ColonyAcquired { colony: i });
        self.complete_op();
        Ok(())
    }

    /// The player's current Influence — the statecraft resource for diplomatic
    /// annexation (E4).
    pub fn influence(&self) -> i64 {
        self.influence
    }

    // ---- contested colonies (gather influence over the major hubs, early game) ----

    /// The contested major frontier hubs — the powers' tug-of-war state (early game).
    pub fn contested_colonies(&self) -> &[ContestedColony] {
        &self.contested
    }

    /// Number of contested colonies.
    pub fn contested_count(&self) -> usize {
        self.contested.len()
    }

    /// Read contested colony `i` (None out of range).
    pub fn contested_colony(&self, i: usize) -> Option<&ContestedColony> {
        self.contested.get(i)
    }

    /// **Court a contested colony** (the slow early-game gather-influence loop):
    /// spend Influence to build your standing over it. Reach the claim threshold to
    /// take it. Counts as a spine op (§0). Fails without enough Influence.
    pub fn court_contested_colony(&mut self, i: usize) -> Result<(), ContestError> {
        let cc = self.contested.get(i).ok_or(ContestError::NoSuchColony)?;
        if self.colony_controlled(cc.colony) {
            return Err(ContestError::AlreadyControlled);
        }
        if self.influence < contest::COURT_COST {
            return Err(ContestError::NotEnoughInfluence);
        }
        self.influence -= contest::COURT_COST;
        let cc = &mut self.contested[i];
        cc.player_influence =
            (cc.player_influence + contest::COURT_GAIN).min(contest::CONTEST_TOTAL);
        self.complete_op();
        Ok(())
    }

    /// **Claim a contested colony** you've built enough standing over — the
    /// influence path to control (cheaper than a buyout, but slow to earn). Taking a
    /// hub from the powers spikes the inners' alarm and is a spine op. Fails until your
    /// standing clears the claim threshold.
    pub fn claim_contested_colony(&mut self, i: usize) -> Result<(), ContestError> {
        let cc = self.contested.get(i).ok_or(ContestError::NoSuchColony)?;
        let colony = cc.colony;
        if self.colony_controlled(colony) {
            return Err(ContestError::AlreadyControlled);
        }
        if !cc.claimable() {
            return Err(ContestError::NotStrongEnough);
        }
        self.controlled[colony] = true;
        // Taking a contested hub from the powers is watched as expansion (E3/E7).
        self.relations.on_player_expand();
        self.raise_alarm(Faction::Earth, ALARM_PER_ACQUISITION);
        self.raise_alarm(Faction::Mars, ALARM_PER_ACQUISITION);
        self.events.push(Event::ColonyAcquired { colony });
        self.complete_op();
        Ok(())
    }

    // ---- corporate diplomacy with the independent companies (E8) ----

    /// The independent companies — the negotiable diplomatic actors (E8).
    pub fn companies(&self) -> &[crate::sim::diplomacy::Company] {
        self.diplomacy.companies()
    }

    /// Number of independent companies (E8).
    pub fn company_count(&self) -> usize {
        self.diplomacy.companies().len()
    }

    /// Company `i`'s relation dial with the player (E8).
    pub fn company_relation(&self, i: usize) -> i64 {
        self.diplomacy.relation(i)
    }

    /// Company `i`'s stance toward the player (E8).
    pub fn company_stance(&self, i: usize) -> Stance {
        self.diplomacy.stance(i)
    }

    /// How many allied companies are lending you escorts (E8).
    pub fn ally_count(&self) -> usize {
        self.diplomacy.ally_count()
    }

    /// The company operating colony `colony`, if any (E8).
    pub fn colony_company(&self, colony: usize) -> Option<usize> {
        self.diplomacy.company_for_colony(colony)
    }

    /// The stance of the company operating `colony` (Neutral if none) (E8).
    pub(crate) fn colony_company_stance(&self, colony: usize) -> Stance {
        self.colony_company(colony)
            .map(|ci| self.diplomacy.stance(ci))
            .unwrap_or(Stance::Neutral)
    }

    /// **Court an independent company** (E8) — the macro diplomacy move: spend
    /// Influence to deepen the relationship a step (Neutral → Partner → Ally). An
    /// Ally's colony joins you freely and its ships help screen your trade.
    pub fn court_company(&mut self, i: usize) -> Result<(), CourtError> {
        if i >= self.diplomacy.companies().len() {
            return Err(CourtError::InvalidCompany);
        }
        if self.influence < COURT_INFLUENCE_COST {
            return Err(CourtError::NotEnoughInfluence);
        }
        self.influence -= COURT_INFLUENCE_COST;
        self.diplomacy.adjust(i, COURT_RELATION_GAIN);
        Ok(())
    }

    /// How a colony may be annexed (E4/E8): free (its company is an Ally), influence-
    /// gated (a Partner company, or good generic Independents standing), or blocked
    /// (a Rival won't join, or it isn't an acquirable target).
    fn annex_kind(&self, i: usize) -> AnnexKind {
        if !self.is_acquirable(i) {
            return AnnexKind::Blocked;
        }
        match self.colony_company_stance(i) {
            Stance::Ally => AnnexKind::Free,
            Stance::Rival => AnnexKind::Blocked,
            stance => {
                let eligible = stance >= Stance::Partner
                    || self.relations.standing(Faction::Independents) >= ANNEX_STANDING_REQ;
                if eligible {
                    AnnexKind::Influence
                } else {
                    AnnexKind::Blocked
                }
            }
        }
    }

    /// Whether colony `i` can be **diplomatically annexed** right now (E4/E8): a
    /// Partner/Ally company's colony (or good Independents standing), with the
    /// Influence to pay (waived for an Ally).
    pub fn can_annex(&self, i: usize) -> bool {
        match self.annex_kind(i) {
            AnnexKind::Free => true,
            AnnexKind::Influence => self.influence >= ANNEX_INFLUENCE_COST,
            AnnexKind::Blocked => false,
        }
    }

    /// **Diplomatically annex an independent colony** (E4/E8) — the peaceful path: it
    /// *joins* you. An **Ally** company's colony joins for free; otherwise it costs
    /// Influence and a Partner relationship (or good Independents standing). Pays the
    /// gentler political cost (`on_player_annex` + a smaller alarm spike) than a buyout.
    pub fn annex_colony(&mut self, i: usize) -> Result<(), AnnexError> {
        if self.colony_controlled(i) {
            return Err(AnnexError::AlreadyControlled);
        }
        match self.annex_kind(i) {
            AnnexKind::Blocked if !self.is_acquirable(i) => return Err(AnnexError::NotAcquirable),
            AnnexKind::Blocked => return Err(AnnexError::StandingTooLow),
            AnnexKind::Influence => {
                if self.influence < ANNEX_INFLUENCE_COST {
                    return Err(AnnexError::NotEnoughInfluence);
                }
                self.influence -= ANNEX_INFLUENCE_COST;
            }
            AnnexKind::Free => {} // an Ally joins willingly, no Influence spent
        }
        self.controlled[i] = true;
        self.relations.on_player_annex();
        // A peaceful annexation alarms the inners less (E7).
        self.raise_alarm(Faction::Earth, ALARM_PER_ANNEX);
        self.raise_alarm(Faction::Mars, ALARM_PER_ANNEX);
        self.events.push(Event::ColonyAcquired { colony: i });
        self.complete_op();
        Ok(())
    }

    /// The defending garrison size for colony `i` (E5) — scaled by its owner: the
    /// inner powers garrison hard, the Independents barely at all, so taking Earth's
    /// ground by force needs a real battlefleet while an outpost falls to a frigate or two.
    pub fn garrison_size(&self, i: usize) -> usize {
        match self.colonies.get(i).map(|c| c.faction) {
            Some(Faction::Earth) => 8,
            Some(Faction::Mars) => 6,
            Some(Faction::Belt) => 4,
            _ => 2,
        }
    }

    /// **Seize a colony by force** (E5) — the aggressive path: muster the fleet and
    /// assault the colony's garrison (any colony, not just independents). A won siege
    /// takes control but at the harshest political price (`on_player_seize` craters
    /// the owner's standing + the biggest alarm spike); a lost one just costs ships.
    /// Returns the battle outcome on a resolved assault.
    pub fn seize_colony(&mut self, i: usize, band: Band) -> Result<BattleOutcome, SeizeError> {
        if i >= self.colonies.len() {
            return Err(SeizeError::InvalidTarget);
        }
        if self.colony_controlled(i) {
            return Err(SeizeError::AlreadyControlled);
        }
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return Err(SeizeError::NoFleet);
        }
        let owner = self.colonies[i].faction;
        let garrison: Vec<Loadout> = (0..self.garrison_size(i))
            .map(|_| {
                ships::reference_loadout_quality(
                    ShipClass::Frigate,
                    GARRISON_QUALITY,
                    &mut self.rng,
                )
            })
            .collect();
        let player_doctrine = Doctrine {
            band,
            ..self.combat_doctrine
        };
        let garrison_doctrine = Doctrine {
            band,
            ..Doctrine::default()
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine: player_doctrine,
            },
            &Fleet {
                ships: &garrison,
                doctrine: garrison_doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        let won = outcome.winner == Some(0);
        let all: Vec<usize> = (0..player_ships.len()).collect();
        self.corp.resolve_engagement_for(all, survivors, won);
        if won {
            self.controlled[i] = true;
            self.relations.on_player_seize(owner);
            // Open aggression spikes the **victim's** alarm hardest (E7 — taking
            // Mars's colony brings Mars down on you), with lesser inner wariness.
            self.raise_alarm(owner, ALARM_PER_SEIZE);
            if owner != Faction::Earth {
                self.raise_alarm(Faction::Earth, ALARM_PER_ACQUISITION);
            }
            if owner != Faction::Mars {
                self.raise_alarm(Faction::Mars, ALARM_PER_ACQUISITION);
            }
            // Taking a company's colony by force makes it a Rival (E8).
            if let Some(ci) = self.diplomacy.company_for_colony(i) {
                self.diplomacy.adjust(ci, -SEIZE_RELATION_HIT);
            }
            self.events.push(Event::ColonyAcquired { colony: i });
            self.complete_op();
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), garrison.len()], outcome.clone()));
        Ok(outcome)
    }

    /// How many holdings the player can govern efficiently (E2) — a base plus a
    /// slice earned through the CEO track. Beyond this, holdings strain (§ Stellaris
    /// admin cap): a seasoned operator runs a wider empire than a green one.
    pub fn admin_capacity(&self) -> usize {
        ADMIN_BASE_CAPACITY
            + (self.progression.ceo.level() / ADMIN_CAPACITY_PER_CEO_LEVELS).max(0) as usize
    }

    /// The administrative load on the company — one per holding (E2).
    pub fn admin_load(&self) -> usize {
        self.holding_count()
    }

    /// Holdings over administrative capacity (E2) — the overextension amount; 0 when
    /// comfortably within reach.
    pub fn admin_strain(&self) -> usize {
        self.admin_load().saturating_sub(self.admin_capacity())
    }

    /// Empire-wide tribute efficiency in basis points (E2): 100% within capacity,
    /// falling with each over-capacity holding down to a floor.
    pub fn holdings_efficiency_bp(&self) -> i64 {
        let strain = self.admin_strain() as i64;
        (10_000 - strain * STRAIN_EFFICIENCY_PENALTY_BP).max(STRAIN_EFFICIENCY_FLOOR_BP)
    }

    /// Per-tick empire income/upkeep (the empire layer): controlled colonies pay
    /// tribute, scaled by administrative efficiency, minus the strain upkeep of any
    /// over-capacity holdings (E2). Within capacity it's pure income; overextended,
    /// holdings go net-negative. A pure credit flow — no market RNG — so a fresh sim
    /// (which controls nothing) is byte-identical and the §7c gate holds.
    pub(crate) fn run_holdings(&mut self) {
        // Influence accrues slowly toward its cap (E4) — the statecraft resource for
        // diplomatic annexation. Pure (no RNG), so a fresh sim stays byte-identical.
        self.influence = (self.influence + INFLUENCE_PER_TICK).min(INFLUENCE_MAX);
        let (out_bp, trib_bp, _) = self.dev_doctrine.weights();
        let gross: i64 = self
            .controlled
            .iter()
            .enumerate()
            .filter(|(_, &held)| held)
            .map(|(i, _)| {
                let base = match self.colonies[i].is_market {
                    true => COLONY_TRIBUTE_MARKET,
                    false => COLONY_TRIBUTE_OUTPOST,
                };
                // Phase C: tribute scales with development, tilted by the doctrine.
                base * self.effective_colony_dev(i) * trib_bp / 10_000
            })
            .sum();
        if gross == 0 {
            return; // no holdings → byte-identical no-op
        }
        let tribute = gross * self.holdings_efficiency_bp() / 10_000;
        let upkeep = self.admin_strain() as i64 * STRAIN_UPKEEP_PER_HOLDING;
        let net = tribute - upkeep;
        if net >= 0 {
            self.corp.credit(net);
        } else {
            // Overextension can drain the treasury, but not below zero.
            let drain = (-net).min(self.corp.credits());
            self.corp.debit(drain);
        }
        // EP1: each controlled colony produces its specialty raw into your warehouse —
        // holdings are supply nodes feeding your production (refine it) and logistics
        // (route/sell it), not just a credit drip. Warehouse-only ⇒ no market RNG, so
        // a fresh sim (which controls nothing) stays byte-identical and §7c holds.
        let outputs: Vec<(usize, i64)> = (0..self.controlled.len())
            .filter(|&i| self.controlled[i])
            .map(|i| (self.colony_specialty(i), self.effective_colony_dev(i)))
            .collect();
        for (c, dev) in outputs {
            // Phase C: output scales with dev, tilted by the doctrine.
            self.corp
                .store(c, COLONY_OUTPUT_PER_TICK * dev * out_bp / 10_000);
        }
    }

    /// The empire-wide development doctrine (Phase C).
    pub fn dev_doctrine(&self) -> DevDoctrine {
        self.dev_doctrine
    }

    /// Cycle the development doctrine (a macro empire knob).
    pub fn cycle_dev_doctrine(&mut self) {
        self.dev_doctrine = self.dev_doctrine.next();
    }

    /// A controlled colony's development level (Phase C) — `DEV_BASE` until invested.
    pub fn colony_dev(&self, i: usize) -> i64 {
        self.colony_dev.get(i).copied().unwrap_or(DEV_BASE)
    }

    /// The **operational** development level of colony `i` — the most-recent level whose build
    /// has finished (one level lower while a development build is still in progress).
    pub(crate) fn effective_colony_dev(&self, i: usize) -> i64 {
        let lvl = self.colony_dev(i);
        if self.colony_dev_ready.get(i).is_some_and(|&t| self.tick < t) {
            (lvl - 1).max(DEV_BASE)
        } else {
            lvl
        }
    }

    /// Days left on colony `i`'s development build (0 if none / done).
    pub fn colony_build_days(&self, i: usize) -> u64 {
        match self.colony_dev_ready.get(i) {
            Some(&t) if self.tick < t => (t - self.tick).div_ceil(6),
            _ => 0,
        }
    }

    /// The credit cost to raise colony `i` one development level (escalates with level).
    /// `None` if it can't be developed (not controlled, maxed, or a build is in progress).
    pub fn develop_cost(&self, i: usize) -> Option<i64> {
        if !self.colony_controlled(i)
            || self.colony_dev(i) >= MAX_DEV
            || self.colony_build_days(i) > 0
        {
            return None;
        }
        let (_, _, cost_bp) = self.dev_doctrine.weights();
        Some(DEV_COST_BASE * self.colony_dev(i) * cost_bp / 10_000)
    }

    /// **Develop** a controlled colony (Phase C, the *tall* growth axis): spend credits
    /// to raise its development a level, scaling its tribute + output. Unlike acquiring a
    /// *new* colony, improving your **own** draws **no coalition alarm** — the safe way
    /// to grow. Counts as an operation on the §0 climb.
    pub fn develop_colony(&mut self, i: usize) -> Result<(), DevelopError> {
        let cost = match self.develop_cost(i) {
            Some(c) => c,
            None if !self.colony_controlled(i) => return Err(DevelopError::NotControlled),
            None => return Err(DevelopError::Maxed),
        };
        if self.corp.credits() < cost {
            return Err(DevelopError::CantAfford);
        }
        self.corp.debit(cost);
        // Raise the level now but arm the build timer — the new capacity (tribute + output)
        // only comes online when construction finishes (~180 days), via effective_colony_dev.
        self.colony_dev[i] += 1;
        self.colony_dev_ready[i] = self.tick + COLONY_DEVELOP_TICKS;
        self.complete_op();
        Ok(())
    }

    /// The highest development among the player's holdings (for the EMPIRE headline).
    pub fn peak_dev(&self) -> i64 {
        (0..self.colonies.len())
            .filter(|&i| self.colony_controlled(i))
            .map(|i| self.colony_dev(i))
            .max()
            .unwrap_or(0)
    }

    /// The specialty raw commodity a colony produces (EP1) — thematic by its faction
    /// (Belters mine ice, Mars ore, Earth volatiles), independents varying by location.
    /// Deterministic; one of the raw tiers `[0,1,2]`.
    pub fn colony_specialty(&self, i: usize) -> usize {
        match self.colonies.get(i).map(|c| c.faction) {
            Some(Faction::Belt) => 0,  // Ice
            Some(Faction::Mars) => 1,  // Ore
            Some(Faction::Earth) => 2, // Volatiles
            _ => i % 3,                // Independents vary by location
        }
    }

    // ---- faction alarm & the coalition (E3) ---------------------------------

    /// The loudest great-power alarm at the player's expansion (E3/E7), `0..=ALARM_MAX`
    /// — the overall coalition pressure (the most-threatened power).
    pub fn coalition_alarm(&self) -> i64 {
        [Faction::Earth, Faction::Mars, Faction::Belt]
            .iter()
            .map(|&f| self.faction_alarm[f.index()])
            .max()
            .unwrap_or(0)
    }

    /// A single great power's alarm at your expansion (E7).
    pub fn faction_alarm(&self, f: Faction) -> i64 {
        self.faction_alarm[f.index()]
    }

    /// The great power leading the coalition (the most alarmed) — whose sphere you've
    /// most provoked (E7).
    pub fn coalition_leader(&self) -> Faction {
        [Faction::Earth, Faction::Mars, Faction::Belt]
            .into_iter()
            .max_by_key(|&f| self.faction_alarm[f.index()])
            .unwrap_or(Faction::Earth)
    }

    /// Whether a great-power coalition has formed and is striking the holdings (E3).
    pub fn coalition_active(&self) -> bool {
        self.coalition_alarm() >= COALITION_THRESHOLD
    }

    /// Whether a coalition strike is bearing on the holdings, awaiting a defense (E3).
    pub fn coalition_strike_pending(&self) -> bool {
        self.pending_coalition.is_some()
    }

    /// Spike a specific faction's alarm (E7) — `by` clamped into `0..=ALARM_MAX`.
    pub(crate) fn raise_alarm(&mut self, f: Faction, by: i64) {
        let a = &mut self.faction_alarm[f.index()];
        *a = (*a + by).clamp(0, ALARM_MAX);
    }

    /// The size-driven alarm baseline for faction `f` (E7): the inners (Earth/Mars)
    /// are made wary by the sheer size of your empire; the Belt is your home and is
    /// only alarmed if you *provoke* it directly (a seized Belt colony), so its
    /// baseline is 0.
    pub(crate) fn alarm_baseline(&self, f: Faction) -> i64 {
        match f {
            Faction::Earth | Faction::Mars => {
                (self.holding_count() as i64 * ALARM_PER_HOLDING).min(ALARM_MAX)
            }
            _ => 0,
        }
    }

    /// The alarm a coalition strike answers to — tighter cadence + bigger packs the
    /// more threatened the powers are.
    pub(crate) fn coalition_period(&self) -> u64 {
        // From the base period at threshold, tightening toward the floor at max alarm.
        let over = (self.coalition_alarm() - COALITION_THRESHOLD).max(0);
        let span = (ALARM_MAX - COALITION_THRESHOLD).max(1);
        let tighten = (COALITION_BASE_PERIOD - COALITION_MIN_PERIOD) as i64 * over / span;
        COALITION_BASE_PERIOD.saturating_sub(tighten as u64)
    }

    /// Per-tick coalition layer (E3/E7): each great power's alarm drifts toward its
    /// size baseline, and once any crosses the threshold a coalition (led by the
    /// angriest power) telegraphs + lands strikes. Inert while the player controls
    /// nothing (baselines 0, spikes 0) — so a fresh sim is byte-identical, §7c holds.
    pub(crate) fn run_coalition(&mut self, now: u64) {
        // Each great power's alarm trends toward its size baseline (a big empire keeps
        // the inners watching); with no holdings every baseline is 0 → alarm decays.
        for f in [Faction::Earth, Faction::Mars, Faction::Belt] {
            let baseline = self.alarm_baseline(f);
            let a = self.faction_alarm[f.index()];
            let next = if a < baseline {
                (a + ALARM_DRIFT).min(baseline)
            } else if a > baseline {
                (a - ALARM_DRIFT).max(baseline)
            } else {
                a
            };
            self.faction_alarm[f.index()] = next;
        }
        if !self.coalition_active() {
            // Cooled below the threshold: the coalition stands down.
            self.coalition_forecast_sent = false;
            self.next_coalition_strike = 0;
            return;
        }
        // Resolve an undefended strike whose window has lapsed.
        if let Some((strength, deadline)) = self.pending_coalition {
            if now >= deadline {
                self.pending_coalition = None;
                self.coalition_seize_holding(strength);
            }
        }
        // Schedule the first strike when the coalition forms.
        if self.next_coalition_strike == 0 {
            self.next_coalition_strike = now + self.coalition_period();
        }
        // Telegraph the incoming strike (§13 forecasting).
        if !self.coalition_forecast_sent
            && now + crate::sim::pressure::FORECAST_LEAD >= self.next_coalition_strike
        {
            let eta = self.next_coalition_strike.saturating_sub(now);
            self.events.push(Event::ThreatForecast {
                kind: PressureKind::FactionWar,
                eta,
            });
            self.coalition_forecast_sent = true;
        }
        // Land a strike (only if none is already pending — one crisis at a time).
        if self.pending_coalition.is_none() && now >= self.next_coalition_strike {
            let strength = self.coalition_alarm();
            self.pending_coalition = Some((strength, now + COALITION_RESPONSE_WINDOW));
            self.events.push(Event::CoalitionStrike { strength });
            self.next_coalition_strike = now + self.coalition_period();
            self.coalition_forecast_sent = false;
        }
    }

    /// An undefended coalition strike seizes a holding (E3): the inners liberate the
    /// player's most valuable controlled colony back to the Independents. Taking it
    /// *relieves* the coalition's alarm (they got what they came for). With no colony
    /// to seize, they exact reparations from the treasury instead.
    pub(crate) fn coalition_seize_holding(&mut self, _strength: i64) {
        // Prefer to seize a market colony (the prize), else any controlled one.
        let target = (0..self.colonies.len())
            .filter(|&i| self.controlled[i])
            .max_by_key(|&i| self.colonies[i].is_market as i64);
        if let Some(i) = target {
            self.controlled[i] = false;
            self.events.push(Event::HoldingLost { colony: i });
            // Taking a holding relieves the leader's resolve (they got their prize).
            let leader = self.coalition_leader();
            self.raise_alarm(leader, -ALARM_RELIEF_ON_DEFEND);
        } else {
            let drain = COALITION_REPARATIONS.min(self.corp.credits());
            self.corp.debit(drain);
            self.events.push(Event::HoldingLost { colony: usize::MAX });
        }
    }

    /// **Defend the holdings** against the pending coalition strike (E3): rally the
    /// fleet against a coalition pack scaled by the strike's strength. A win repels
    /// it (no holding lost, alarm relieved, an op); a loss lets the strike through
    /// (a holding is seized). Returns the battle outcome, or `None` if there's no
    /// strike to answer or no warships to answer with.
    pub fn defend_holdings(&mut self, band: Band) -> Option<BattleOutcome> {
        let (strength, _) = self.pending_coalition?;
        let player_ships: Vec<Loadout> = self
            .corp
            .fleet()
            .iter()
            .map(|s| s.loadout.clone())
            .collect();
        if player_ships.is_empty() {
            return None;
        }
        let over = (strength - COALITION_THRESHOLD).max(0);
        let pack_size = (2 + over / COALITION_STRENGTH_PER_SHIP) as usize;
        let pack: Vec<Loadout> = (0..pack_size)
            .map(|_| {
                ships::reference_loadout_quality(
                    ShipClass::Frigate,
                    COALITION_QUALITY,
                    &mut self.rng,
                )
            })
            .collect();
        let player_doctrine = Doctrine {
            band,
            ..self.combat_doctrine
        };
        let raider_doctrine = Doctrine {
            band,
            ..Doctrine::default()
        };
        let outcome = combat::resolve(
            &Fleet {
                ships: &player_ships,
                doctrine: player_doctrine,
            },
            &Fleet {
                ships: &pack,
                doctrine: raider_doctrine,
            },
            &mut self.rng,
        );
        let survivors = outcome.survivors[0];
        let losses = player_ships.len() - survivors;
        let won = outcome.winner == Some(0);
        let all: Vec<usize> = (0..player_ships.len()).collect();
        self.corp.resolve_engagement_for(all, survivors, won);
        self.pending_coalition = None;
        self.feed.resolve_holdings();
        if won {
            // Repelled — the holdings stand and the coalition leader's resolve cools.
            let leader = self.coalition_leader();
            self.raise_alarm(leader, -ALARM_RELIEF_ON_DEFEND);
            self.complete_op();
        } else {
            self.coalition_seize_holding(strength);
        }
        self.events.push(Event::BattleResolved { won, losses });
        self.last_battle = Some((band, [player_ships.len(), pack.len()], outcome.clone()));
        Some(outcome)
    }
}
