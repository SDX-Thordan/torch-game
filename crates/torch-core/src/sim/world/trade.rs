//! `trade` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// Skim operating overhead off the treasury each tick (§5 sink). Overhead is
    /// a fraction of holdings *above* a free float, so it bites only runaway
    /// hoarding — the wealth-scaled sink that turns every income strategy into a
    /// sustainable equilibrium rather than an unbounded faucet.
    pub(crate) fn charge_upkeep(&mut self) {
        let taxable = self.corp.credits() - UPKEEP_FREE_FLOAT;
        if taxable > 0 {
            let upkeep = taxable / UPKEEP_DEN;
            if upkeep > 0 {
                self.corp.debit(upkeep);
            }
        }
    }

    /// Whether the player **owns** market `m` (EP2) — a controlled colony sits on its
    /// body. Owned markets trade fee-reduced and earn a tariff on NPC deliveries.
    pub fn market_is_owned(&self, m: usize) -> bool {
        let Some(market) = self.markets.get(m) else {
            return false;
        };
        let body = market.body();
        self.colonies
            .iter()
            .zip(self.controlled.iter())
            .any(|(c, &held)| held && c.body == body)
    }

    /// The brokerage fee for a trade of `value` at market `m`: reduced at a market you
    /// own (EP2, you run the broker), the standard fee at a neutral market, and a
    /// **customs surcharge** on top at a market whose faction you've soured (EP4) —
    /// trading in hostile space costs more, scaling with how badly you've crossed them.
    pub(crate) fn market_trade_fee(&self, m: usize, value: i64) -> i64 {
        if self.market_is_owned(m) {
            return value * OWNED_TRADE_FEE_BP / FEE_DEN;
        }
        let mut bp = Self::TRADE_FEE_BP;
        let standing = self.relations.standing(self.markets[m].faction());
        if standing < 0 {
            // EP4: customs friction at a soured faction's market, up to the max
            // surcharge at fully-hostile standing.
            bp += (-standing).min(1_000) * INSPECTION_SURCHARGE_MAX_BP / 1_000;
        }
        value * bp / FEE_DEN
    }

    /// Buy `qty` of commodity `c` at market `m` (§5): debits the goods cost plus
    /// the brokerage fee, lifts the goods into the warehouse, and nudges the
    /// price up. Returns the total credits spent (cost + fee).
    pub fn buy(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        let price = self.markets[m].price(c);
        let cost = price * qty;
        let total = cost + self.market_trade_fee(m, cost);
        if self.markets[m].stock(c) < qty {
            return Err(TradeError::InsufficientStock);
        }
        if self.corp.credits() < total {
            return Err(TradeError::InsufficientCredits);
        }
        self.markets[m].remove_stock(c, qty);
        self.corp.debit(total);
        self.corp.store(c, qty);
        Ok(total)
    }

    /// Sell `qty` of commodity `c` into market `m` (§5): lands warehouse cargo at
    /// the current price less the brokerage fee, nudging the price down. Returns
    /// the net credits received (revenue − fee).
    pub fn sell(&mut self, m: usize, c: usize, qty: i64) -> Result<i64, TradeError> {
        if qty <= 0 {
            return Ok(0);
        }
        if self.corp.cargo(c) < qty {
            return Err(TradeError::InsufficientCargo);
        }
        let price = self.markets[m].price(c);
        let revenue = price * qty;
        let net = revenue - self.market_trade_fee(m, revenue);
        self.corp.unstore(c, qty);
        self.markets[m].add_stock(c, qty);
        self.corp.credit(net);
        self.note_mission(crate::sim::missions::Trigger::FirstTrade); // §16 tutorial
        Ok(net)
    }

    /// Answer an act-now shortage in one move (§0.4 / §3.3 speculate): source
    /// `qty` of `commodity` at the cheapest *other* market and sell it into the
    /// short `market` for the premium — no pre-held cargo needed. Resolves the
    /// matching alert and returns the net profit (revenue − cost).
    pub fn exploit_shortage(
        &mut self,
        market: usize,
        commodity: usize,
        qty: i64,
    ) -> Result<i64, TradeError> {
        if market >= self.markets.len() {
            return Err(TradeError::InsufficientStock);
        }
        let source = (0..self.markets.len())
            .filter(|&m| m != market)
            .min_by_key(|&m| self.markets[m].price(commodity))
            .ok_or(TradeError::InsufficientStock)?;
        let cost = self.buy(source, commodity, qty)?;
        let revenue = self.sell(market, commodity, qty)?;
        self.feed.resolve_shortage(market, commodity);
        Ok(revenue - cost)
    }

    /// One-press answer to the loudest open act-now shortage (the alert→verb
    /// path the influence model wants). Returns whether one was answered.
    pub fn answer_top_shortage(&mut self, qty: i64) -> bool {
        let target = self.feed.surfaced().iter().find_map(|a| match a.verb {
            Some(Verb::ExploitShortage { market, commodity }) => Some((market, commodity)),
            _ => None,
        });
        match target {
            Some((m, c)) => self.exploit_shortage(m, c, qty).is_ok(),
            None => false,
        }
    }

    // ---- Phase A: player dilemmas (act-now decisions with trade-offs) ----------

    /// Raise a dilemma for a fresh act-now exception, deduped per kind and capped to a
    /// small menu (no backlog anxiety).
    pub(crate) fn push_decision(
        &mut self,
        kind: DecisionKind,
        market: usize,
        commodity: usize,
        target: u64,
        magnitude: i64,
        now: u64,
    ) {
        if self.decisions.len() >= MAX_DECISIONS {
            return;
        }
        let dup = self.decisions.iter().any(|x| {
            x.kind == kind
                && match kind {
                    DecisionKind::Shortage => x.market == market && x.commodity == commodity,
                    DecisionKind::Wreck => x.target == target,
                    DecisionKind::RaidThreat => true, // one inbound-raid dilemma at a time
                    DecisionKind::WarCollateral => true, // one war flashpoint at a time
                }
        });
        if dup {
            return;
        }
        let id = self.next_decision_id;
        self.next_decision_id += 1;
        self.decisions.push(Decision {
            id,
            kind,
            market,
            commodity,
            target,
            magnitude,
            deadline_tick: now + DECISION_TTL,
        });
    }

    /// A one-line title for the dilemma at `idx` (for the shell header).
    pub fn decision_title(&self, idx: usize) -> String {
        let Some(d) = self.decisions.get(idx) else {
            return String::new();
        };
        match d.kind {
            DecisionKind::Shortage => {
                let c = self.markets[d.market].defs()[d.commodity].name;
                format!("{c} shortage at {}", self.markets[d.market].name())
            }
            DecisionKind::Wreck => {
                let name = self
                    .salvage
                    .wrecks()
                    .iter()
                    .find(|w| w.id == d.target)
                    .map(|w| w.name)
                    .unwrap_or("Derelict");
                format!("Derelict sighted — {name}")
            }
            DecisionKind::RaidThreat => "Raiders inbound on the lanes".to_string(),
            DecisionKind::WarCollateral => "Earth–Mars flashpoint on your lanes".to_string(),
        }
    }

    /// The pending dilemmas (the act-now menu, §0.4).
    pub fn decisions(&self) -> &[Decision] {
        &self.decisions
    }

    /// The cheapest *other* market to source `commodity` for a shortage at `market`,
    /// plus the deal size affordable/available there.
    pub(crate) fn deal_source(&self, market: usize, commodity: usize) -> Option<(usize, i64)> {
        let source = (0..self.markets.len())
            .filter(|&m| m != market && m < self.far_market_start)
            .min_by_key(|&m| self.markets[m].price(commodity))?;
        let price = self.markets[source].price(commodity).max(1);
        let affordable = self.corp.credits() / price;
        let qty = DEAL_QTY
            .min(self.markets[source].stock(commodity))
            .min(affordable);
        Some((source, qty.max(0)))
    }

    /// The trade-off options for a pending dilemma, with live numbers for the shell.
    pub fn decision_options(&self, idx: usize) -> Vec<DecisionOption> {
        let Some(d) = self.decisions.get(idx) else {
            return Vec::new();
        };
        match d.kind {
            DecisionKind::Shortage => self.shortage_options(d),
            DecisionKind::Wreck => Self::wreck_options(),
            DecisionKind::RaidThreat => Self::raid_options(d.magnitude),
            DecisionKind::WarCollateral => self.war_options(d.magnitude),
        }
    }

    pub(crate) fn wreck_options() -> Vec<DecisionOption> {
        vec![
            DecisionOption {
                label: "Strip Hull",
                summary: format!("Cut it up for scrap. +{WRECK_SCRAP} cr, certain."),
                est_credits: WRECK_SCRAP,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Mine Data",
                summary: format!("Pull the data core. +{WRECK_DATA} research, certain."),
                est_credits: 0,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Reverse-Engineer",
                summary: "Crack the tech. ~50%: a recovered blueprint; else only salvage data."
                    .to_string(),
                est_credits: 0,
                rep_delta: 0,
                risky: true,
            },
        ]
    }

    pub(crate) fn raid_options(mag: i64) -> Vec<DecisionOption> {
        vec![
            DecisionOption {
                label: "Hunt Them",
                summary: format!(
                    "Run the raiders off. ~60%: +{mag} cr bounty + calm; else they slip you."
                ),
                est_credits: mag,
                rep_delta: 0,
                risky: true,
            },
            DecisionOption {
                label: "Hire Escorts",
                summary: format!("Pay {ESCORT_FEE} cr for protection — the threat eases, no risk."),
                est_credits: -ESCORT_FEE,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Set an Ambush",
                summary: format!(
                    "Bait a trap. ~38%: +{} cr + calm; else a {} cr loss.",
                    mag * 2,
                    mag / 2
                ),
                est_credits: mag * 2,
                rep_delta: 0,
                risky: true,
            },
        ]
    }

    pub(crate) fn war_options(&self, stake: i64) -> Vec<DecisionOption> {
        let (fav, riv) = self.favored_inner();
        vec![
            DecisionOption {
                label: "Reroute",
                summary: format!("Take the long way around. −{WAR_REROUTE_COST} cr, but safe."),
                est_credits: -WAR_REROUTE_COST,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Run It",
                summary: format!(
                    "Run the blockade. ~55%: through clean; else −{} cr and an inner sours.",
                    stake * 2
                ),
                est_credits: 0,
                rep_delta: 0,
                risky: true,
            },
            DecisionOption {
                label: "Pick a Side",
                summary: format!(
                    "Side with {}: waved through (+rep), but {} won't forget (−rep).",
                    fav.name(),
                    riv.name()
                ),
                est_credits: 0,
                rep_delta: WAR_SIDE_REP,
                risky: false,
            },
        ]
    }

    pub(crate) fn resolve_war_decision(&mut self, d: &Decision, opt: usize) -> DecisionOutcome {
        let stake = d.magnitude.max(0);
        let (fav, riv) = self.favored_inner();
        match opt {
            // Reroute: a sure toll to stay out of the crossfire (the safe play).
            0 => {
                let cost = WAR_REROUTE_COST.min(self.corp.credits());
                self.corp.debit(cost);
                DecisionOutcome {
                    credits: -cost,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Rerouted around the flashpoint: −{cost} cr, cargo safe."),
                }
            }
            // Run it: gamble on slipping the blockade; caught means a loss + a soured inner.
            1 => {
                if self.rng.chance_bp(WAR_RUN_CHANCE_BP) {
                    self.complete_op();
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: false,
                        message: "Slipped the blockade clean — no toll, no scratch.".to_string(),
                    }
                } else {
                    let loss = (stake * 2).min(self.corp.credits());
                    self.corp.debit(loss);
                    self.relations.adjust(riv, -WAR_SIDE_REP / 2);
                    // Collateral on your space assets: a stray round takes out a miner.
                    let asset = if self.miners.pop().is_some() {
                        " A miner was lost in the shooting."
                    } else {
                        ""
                    };
                    DecisionOutcome {
                        credits: -loss,
                        rep_delta: -WAR_SIDE_REP / 2,
                        backfired: true,
                        message: format!(
                            "Caught in the crossfire — lost {loss} cr and soured {}.{asset}",
                            riv.name()
                        ),
                    }
                }
            }
            // Pick a side: free passage + favour with one inner, resentment from the rival.
            _ => {
                self.relations.adjust(fav, WAR_SIDE_REP);
                self.relations.adjust(riv, -WAR_SIDE_REP);
                self.complete_op();
                DecisionOutcome {
                    credits: 0,
                    rep_delta: WAR_SIDE_REP,
                    backfired: false,
                    message: format!(
                        "Sided with {} — waved through, but {} won't forget.",
                        fav.name(),
                        riv.name()
                    ),
                }
            }
        }
    }

    pub(crate) fn shortage_options(&self, d: &Decision) -> Vec<DecisionOption> {
        let cname = self.markets[d.market].defs()[d.commodity].name;
        let owner = self.markets[d.market].faction().name();
        let Some((source, qty)) = self.deal_source(d.market, d.commodity) else {
            return Vec::new();
        };
        let buy = self.markets[source].price(d.commodity);
        let sell = self.markets[d.market].price(d.commodity);
        let spread = (sell - buy).max(0);
        let speculate = qty * spread - qty * sell * Self::TRADE_FEE_BP / 10_000;
        let profiteer = speculate + qty * sell * GOUGE_BONUS_BP / 10_000;
        let relief = qty * (buy * RELIEF_MARGIN_BP / 10_000 - buy);
        vec![
            DecisionOption {
                label: "Speculate",
                summary: format!(
                    "Buy {cname} cheap, sell into the shortage. ~+{speculate} cr, no strings."
                ),
                est_credits: speculate,
                rep_delta: 0,
                risky: false,
            },
            DecisionOption {
                label: "Profiteer",
                summary: format!(
                    "Gouge the panic. ~+{profiteer} cr but {owner} resents it (−rep); risk a fine if they already do."
                ),
                est_credits: profiteer,
                rep_delta: -GOUGE_REP,
                risky: true,
            },
            DecisionOption {
                label: "Relief Run",
                summary: format!(
                    "Sell at cost to break the shortage. ~{relief} cr, but {owner} won't forget the favour (+rep, climbs the spine)."
                ),
                est_credits: relief,
                rep_delta: RELIEF_REP,
                risky: false,
            },
        ]
    }

    /// Resolve the dilemma at `idx` with option `opt`. Returns the outcome (credits,
    /// reputation, whether a risky option backfired) or an error if it can't proceed.
    pub fn resolve_decision(
        &mut self,
        idx: usize,
        opt: usize,
    ) -> Result<DecisionOutcome, TradeError> {
        let Some(d) = self.decisions.get(idx).cloned() else {
            return Err(TradeError::InsufficientStock);
        };
        let outcome = match d.kind {
            DecisionKind::Shortage => {
                let o = self.resolve_shortage_decision(&d, opt)?;
                self.feed.resolve_shortage(d.market, d.commodity);
                o
            }
            DecisionKind::Wreck => self.resolve_wreck_decision(&d, opt)?,
            DecisionKind::RaidThreat => self.resolve_raid_decision(&d, opt),
            DecisionKind::WarCollateral => self.resolve_war_decision(&d, opt),
        };
        self.decisions.retain(|x| x.id != d.id);
        // A2: answering an act-now exception is itself a player **operation** — every
        // dilemma resolved climbs the §0 spine (CEO XP + research + ascent), so the
        // feed pays off for *every* play style, not only the shortage-trading Tycoon.
        self.complete_op();
        Ok(outcome)
    }

    pub(crate) fn resolve_wreck_decision(
        &mut self,
        d: &Decision,
        opt: usize,
    ) -> Result<DecisionOutcome, TradeError> {
        // Claim (remove) the derelict regardless of method; the yield depends on the
        // *choice*, not the pre-rolled reward.
        if self.salvage.claim(d.target).is_none() {
            return Err(TradeError::InsufficientStock);
        }
        let outcome = match opt {
            0 => {
                self.corp.credit(WRECK_SCRAP);
                DecisionOutcome {
                    credits: WRECK_SCRAP,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Stripped the hull: +{WRECK_SCRAP} cr of scrap."),
                }
            }
            1 => {
                self.progression.research.add_points(WRECK_DATA);
                self.reveal_gate_beat(); // data may seed the gate mystery (§15→§0.1)
                DecisionOutcome {
                    credits: 0,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Mined the data core: +{WRECK_DATA} research."),
                }
            }
            2 => {
                // Reverse-engineer: a gamble — recover a **weapon schematic** on success
                // (the route to advanced weapons, Phase B; you can't buy them), else
                // consolation data.
                if self.rng.chance_bp(REVENG_CHANCE_BP) {
                    let learned = self.grant_weapon_schematic();
                    self.reveal_gate_beat();
                    let msg = match learned.and_then(weapons::model) {
                        Some(m) => format!("Cracked it — recovered the {} schematic.", m.name),
                        None => {
                            // Every schematic already known — fall back to a blueprint.
                            let i = (d.target as usize)
                                % self.progression.blueprints.known_count().max(1);
                            self.progression.blueprints.reverse_engineer(i);
                            "Cracked it — a recovered blueprint is yours.".to_string()
                        }
                    };
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: false,
                        message: msg,
                    }
                } else {
                    self.progression.research.add_points(WRECK_DATA / 2);
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: true,
                        message: format!(
                            "The tech resisted — salvaged +{} research instead.",
                            WRECK_DATA / 2
                        ),
                    }
                }
            }
            _ => return Err(TradeError::InsufficientStock),
        };
        self.events.push(Event::WreckSalvaged { id: d.target });
        Ok(outcome)
    }

    pub(crate) fn resolve_raid_decision(&mut self, d: &Decision, opt: usize) -> DecisionOutcome {
        let mag = d.magnitude.max(0);
        match opt {
            // Hunt: gamble for a bounty; success calms piracy, failure costs nothing but
            // the chance (they slip away — the ambient raid still preys on NPC traffic).
            0 => {
                if self.rng.chance_bp(HUNT_CHANCE_BP) {
                    self.corp.credit(mag);
                    self.pressure.relieve(PressureKind::Piracy, RAID_RELIEF);
                    DecisionOutcome {
                        credits: mag,
                        rep_delta: 0,
                        backfired: false,
                        message: format!("Ran the raiders off: +{mag} cr bounty, the lanes calm."),
                    }
                } else {
                    DecisionOutcome {
                        credits: 0,
                        rep_delta: 0,
                        backfired: true,
                        message: "The raiders slipped the net — no bounty.".to_string(),
                    }
                }
            }
            // Hire escorts: a sure thing — pay the fee, the threat eases.
            1 => {
                self.corp.debit(ESCORT_FEE.min(self.corp.credits()));
                self.pressure.relieve(PressureKind::Piracy, RAID_RELIEF);
                DecisionOutcome {
                    credits: -ESCORT_FEE,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Hired escorts for {ESCORT_FEE} cr — the threat eases."),
                }
            }
            // Ambush: longer odds, double bounty on success, a loss on failure.
            _ => {
                if self.rng.chance_bp(AMBUSH_CHANCE_BP) {
                    self.corp.credit(mag * 2);
                    self.pressure.relieve(PressureKind::Piracy, RAID_RELIEF * 2);
                    DecisionOutcome {
                        credits: mag * 2,
                        rep_delta: 0,
                        backfired: false,
                        message: format!(
                            "The trap sprang shut: +{} cr, the lanes go quiet.",
                            mag * 2
                        ),
                    }
                } else {
                    let loss = (mag / 2).min(self.corp.credits());
                    self.corp.debit(loss);
                    DecisionOutcome {
                        credits: -loss,
                        rep_delta: 0,
                        backfired: true,
                        message: format!("The ambush failed — lost {loss} cr."),
                    }
                }
            }
        }
    }

    pub(crate) fn resolve_shortage_decision(
        &mut self,
        d: &Decision,
        opt: usize,
    ) -> Result<DecisionOutcome, TradeError> {
        let (source, qty) = self
            .deal_source(d.market, d.commodity)
            .ok_or(TradeError::InsufficientStock)?;
        if qty <= 0 {
            return Err(TradeError::InsufficientStock);
        }
        let owner = self.markets[d.market].faction();
        match opt {
            // Speculate: the clean merchant play — source cheap, sell at market.
            0 => {
                let cost = self.buy(source, d.commodity, qty)?;
                let revenue = self.sell(d.market, d.commodity, qty)?;
                Ok(DecisionOutcome {
                    credits: revenue - cost,
                    rep_delta: 0,
                    backfired: false,
                    message: format!("Speculated the shortage: +{} cr.", revenue - cost),
                })
            }
            // Profiteer: gouge the panic for extra credits, at a reputation cost — and
            // an already-resentful faction may slap a profiteering fine (the risk).
            1 => {
                let cost = self.buy(source, d.commodity, qty)?;
                let base = self.sell(d.market, d.commodity, qty)?;
                let bonus = base * GOUGE_BONUS_BP / 10_000;
                self.corp.credit(bonus);
                self.relations.adjust(owner, -GOUGE_REP);
                // Risk: a faction you've already soured may claw back part of the gain.
                let standing = self.relations.standing(owner);
                let mut backfired = false;
                let mut fine = 0;
                if standing < 0 {
                    let chance = (-standing).min(6000) as u32; // up to 60%
                    if self.rng.chance_bp(chance) {
                        fine = (base + bonus) / 2;
                        self.corp.debit(fine.min(self.corp.credits()));
                        backfired = true;
                    }
                }
                let net = base + bonus - cost - fine;
                let msg = if backfired {
                    format!(
                        "Profiteered, but {} fined you: net +{net} cr, −rep.",
                        owner.name()
                    )
                } else {
                    format!(
                        "Profiteered the shortage: +{net} cr, but −rep with {}.",
                        owner.name()
                    )
                };
                Ok(DecisionOutcome {
                    credits: net,
                    rep_delta: -GOUGE_REP,
                    backfired,
                    message: msg,
                })
            }
            // Relief Run: flood the market at near cost — forgo profit for goodwill, and
            // count the favour as an operation on the §0 spine.
            2 => {
                let cost = self.buy(source, d.commodity, qty)?;
                let buy_price = cost / qty.max(1);
                let revenue = qty * buy_price * RELIEF_MARGIN_BP / 10_000;
                self.corp.unstore(d.commodity, qty);
                self.markets[d.market].add_stock(d.commodity, qty);
                self.corp.credit(revenue);
                self.relations.adjust(owner, RELIEF_REP);
                Ok(DecisionOutcome {
                    credits: revenue - cost,
                    rep_delta: RELIEF_REP,
                    backfired: false,
                    message: format!("Ran relief to {}: +rep, climbed the spine.", owner.name()),
                })
            }
            _ => Err(TradeError::InsufficientStock),
        }
    }
}
