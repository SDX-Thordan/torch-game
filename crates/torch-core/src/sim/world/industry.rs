//! `industry` behaviours for [`crate::sim::Sim`] (split out of the monolithic world impl).

use super::*;

impl Sim {
    /// Travel time in ticks between two markets at the current orrery geometry.
    pub(crate) fn travel_ticks(&self, origin: usize, dest: usize) -> u64 {
        let o = orbit::position_of(&self.bodies, self.markets[origin].body(), self.tick);
        let d = orbit::position_of(&self.bodies, self.markets[dest].body(), self.tick);
        let (dx, dy) = (d.0 - o.0, d.1 - o.1);
        let dist = (dx * dx + dy * dy).isqrt();
        brachistochrone_ticks(dist, ACCEL_CIV).max(MIN_TRAVEL)
    }

    /// Run the whole route table this tick (§4): land every arriving trip, then
    /// dispatch idle routes against the **shared freighter pool** (a route can
    /// only set out if a freighter is free). The routes run themselves; the
    /// player only set the parameters, and exceptions (below margin, no free
    /// freighter) simply stay idle.
    pub(crate) fn run_logistics(&mut self) {
        if self.routes.is_empty() {
            return;
        }
        // Haulers dedicated to outpost collection (§10) aren't available for trade routes — the
        // opportunity cost. No collectors ⇒ the full pool, so the base economy is byte-identical.
        let freighters = self.corp.freighters() - self.collectors_assigned();
        // Move the table out so the per-route mutations don't fight the
        // `markets`/`corp` borrows (same pattern as the single-route version).
        let mut routes = std::mem::take(&mut self.routes);

        // Deliveries first, freeing up freighters for this tick's dispatch.
        for rt in routes.iter_mut() {
            if rt.in_transit && self.tick >= rt.arrival {
                let revenue = self.markets[rt.dest].price(rt.commodity) * rt.carrying;
                self.markets[rt.dest].add_stock(rt.commodity, rt.carrying);
                self.corp.credit(revenue);
                rt.in_transit = false;
                rt.carrying = 0;
                self.complete_op(); // a delivered standing order is an op (§0/§4)
            }
        }

        // Dispatch idle routes while freighters remain in the pool.
        let mut in_flight = routes.iter().filter(|r| r.in_transit).count() as i64;
        for rt in routes.iter_mut() {
            if in_flight >= freighters {
                break;
            }
            if rt.in_transit || !rt.active {
                continue;
            }
            let buy = self.markets[rt.origin].price(rt.commodity);
            let spread = self.markets[rt.dest].price(rt.commodity) - buy;
            // A trip carries at most the best owned hauler's cargo cap (the tier throughput
            // limit); the Light tier's cap sits above the early routes, so this is a no-op for
            // the base economy and only bites once a player sets a route fatter than their
            // hulls can lift.
            let load = rt.qty.min(self.corp.best_hauler_cargo());
            let cost = buy * load;
            // Fuel (§6): the freighter refuels with Remass at the origin port, an
            // amount scaled by the trip distance. Long outer hauls cost far more
            // fuel — the delta-v constraint as opex — and a hub that produces cheap
            // Remass lowers the whole network's running cost.
            let travel = self.travel_ticks(rt.origin, rt.dest);
            let remass_units = (travel / FREIGHTER_REMASS_DIVISOR).max(1) as i64;
            let fuel_cost = remass_units * self.markets[rt.origin].price(REMASS_COMMODITY);
            let cargo_stocked = self.markets[rt.origin].stock(rt.commodity) > load;
            let fuel_stocked = self.markets[rt.origin].stock(REMASS_COMMODITY) >= remass_units;
            if spread >= rt.min_margin
                && cargo_stocked
                && fuel_stocked
                && self.corp.credits() >= cost + fuel_cost
            {
                self.markets[rt.origin].remove_stock(rt.commodity, load);
                self.markets[rt.origin].remove_stock(REMASS_COMMODITY, remass_units);
                self.corp.debit(cost + fuel_cost);
                rt.in_transit = true;
                rt.carrying = load;
                rt.departed = self.tick;
                rt.arrival = self.tick + travel;
                in_flight += 1;
            }
        }

        self.routes = routes;
    }

    /// The player's production stations (§3.1).
    pub fn stations(&self) -> &[Station] {
        &self.stations
    }

    /// Found a refinery that turns a raw commodity into its refined product:
    /// source `raw` at `buy_market`, refine, and auto-sell the surplus at
    /// `sell_market` (§3.1 Produce + sell-surplus). Costs capital.
    pub fn found_refinery(
        &mut self,
        input: usize,
        buy_market: usize,
        sell_market: usize,
    ) -> Result<(), FoundError> {
        // A station refines `input` into the next tier in its line (`input + 3`),
        // so any non-top-tier commodity can host one (§7d): Ore→Metals→Alloys→
        // Machinery, etc. Only the top-tier finished goods have nowhere to go.
        let output = input + RAW_COUNT;
        if output >= self.markets[0].defs().len() {
            return Err(FoundError::NotProcessable);
        }
        if self.stations.len() >= self.campaign.tier().station_cap() {
            return Err(FoundError::TooManyStations);
        }
        if self.corp.credits() < STATION_COST {
            return Err(FoundError::CantAfford);
        }
        self.corp.debit(STATION_COST);
        self.stations.push(Station {
            body: self.markets[buy_market].body(),
            input,
            output,
            rate: REFINERY_RATE,
            buy_market,
            sell_market,
            sell_above: REFINERY_SELL_ABOVE,
            output_target: REFINERY_TARGET,
        });
        self.complete_op(); // founding industry is progress on the climb (§0)
        Ok(())
    }

    /// Run every station's Produce standing order this tick (§3.1/§4): source
    /// input from a market, transform raw → refined, and dump the surplus output
    /// for credits. Hands-off; the player only set the recipe.
    pub(crate) fn run_industry(&mut self) {
        for i in 0..self.stations.len() {
            let st = self.stations[i];
            let producing = self.corp.cargo(st.output) < st.output_target;
            // Source the input recipe from its market when short.
            if producing && self.corp.cargo(st.input) < st.rate {
                let price = self.markets[st.buy_market].price(st.input);
                let cost = price * st.rate;
                if self.markets[st.buy_market].stock(st.input) > st.rate
                    && self.corp.credits() >= cost
                {
                    self.markets[st.buy_market].remove_stock(st.input, st.rate);
                    self.corp.debit(cost);
                    self.corp.store(st.input, st.rate);
                }
            }
            // Transform input → output (the value-add).
            if producing && self.corp.cargo(st.input) >= st.rate {
                self.corp.unstore(st.input, st.rate);
                self.corp.store(st.output, st.rate);
            }
            // Sell-surplus rule: dump output held above the threshold.
            let surplus = self.corp.cargo(st.output) - st.sell_above;
            if surplus > 0 {
                let price = self.markets[st.sell_market].price(st.output);
                self.corp.unstore(st.output, surplus);
                self.markets[st.sell_market].add_stock(st.output, surplus);
                self.corp.credit(price * surplus);
            }
        }
    }

    /// The faction job board — open and accepted delivery contracts (§3.3/§16).
    pub fn contracts(&self) -> &[crate::sim::contracts::Contract] {
        self.board.offers()
    }

    /// Number of open (not-yet-accepted) contracts on the board.
    pub fn open_contract_count(&self) -> usize {
        self.board.open_count()
    }

    /// Maintain the contract board each tick (§3.3/§16): lapse stale unaccepted
    /// offers, then — on the posting cadence and while the menu has room — post a
    /// fresh delivery job. A faction asks for `qty` of a commodity delivered to
    /// its market for a premium reward and a standing bump; accepting and
    /// fulfilling it ties the economy (you must source the goods) to reputation
    /// (§10) and the §0 climb (a fulfilment is an operation). The board draws from
    /// its **own** RNG so generating offers never perturbs the world streams.
    pub(crate) fn run_contracts(&mut self) {
        self.board.expire_unaccepted(self.tick);
        if !self.tick.is_multiple_of(CONTRACT_INTERVAL) || self.board.open_count() >= MAX_CONTRACTS
        {
            return;
        }
        // Contracts target the inner markets only (the far side trades post-transit
        // via its own verbs) — and bounding to the inner count keeps the board's RNG
        // draw byte-identical to before the far-side markets existed.
        let market = self.board.rng().below(self.far_market_start as u32) as usize;
        let commodity_count = self.markets[market].defs().len();
        let commodity = self.board.rng().below(commodity_count as u32) as usize;
        let qty = CONTRACT_QTY_MIN + self.board.rng().below(CONTRACT_QTY_SPAN as u32) as i64;
        let faction = self.markets[market].faction();
        let face = self.markets[market].price(commodity) * qty;
        let reward = face * CONTRACT_PREMIUM_BP / FEE_DEN;
        let deadline = self.tick + CONTRACT_WINDOW;
        self.board.post(
            faction,
            market,
            commodity,
            qty,
            reward,
            CONTRACT_REP,
            deadline,
        );
    }

    /// Accept open contract `id` (§3.3): the player now owes the delivery until
    /// its deadline (accepted contracts no longer lapse). Returns whether it was
    /// accepted.
    pub fn accept_contract(&mut self, id: u64) -> bool {
        self.board.accept(id)
    }

    /// Fulfil accepted contract `id` from the warehouse (§3.3/§16): consumes the
    /// owed cargo, lands it at the faction's market, pays the reward, lifts the
    /// standing (§10), and counts the delivery as an operation on the climb (§0).
    /// Returns the reward credited, or why it could not be fulfilled.
    pub fn fulfill_contract(&mut self, id: u64) -> Result<i64, ContractError> {
        let c = *self.board.find(id).ok_or(ContractError::NotFound)?;
        if !c.accepted {
            return Err(ContractError::NotAccepted);
        }
        if self.corp.cargo(c.commodity) < c.qty {
            return Err(ContractError::InsufficientCargo);
        }
        self.corp.unstore(c.commodity, c.qty);
        self.markets[c.market].add_stock(c.commodity, c.qty);
        self.corp.credit(c.reward);
        self.relations.adjust(c.faction, c.rep);
        self.board.remove(id);
        self.complete_op(); // a delivered contract is progress on the climb (§0)
        Ok(c.reward)
    }

    /// Accept and immediately attempt to fulfil the first open contract whose
    /// owed cargo is already in the warehouse (the one-press path the influence
    /// model wants). Returns the reward credited, if any.
    pub fn fulfill_ready_contract(&mut self) -> Option<i64> {
        let ready = self
            .board
            .offers()
            .iter()
            .find(|c| self.corp.cargo(c.commodity) >= c.qty)
            .map(|c| c.id)?;
        self.accept_contract(ready);
        self.fulfill_contract(ready).ok()
    }

    /// Standings, mutable — for diplomacy/contracts that move reputation (§10).
    pub fn relations_mut(&mut self) -> &mut Relations {
        &mut self.relations
    }

    /// The player's advancement across research / blueprints / CEO skills (§10).
    pub fn progression(&self) -> &Progression {
        &self.progression
    }

    /// Advancement, mutable — for research/CEO progress driven by play.
    pub fn progression_mut(&mut self) -> &mut Progression {
        &mut self.progression
    }

    /// The standing automation policy the managers execute (§12).
    pub fn policy(&self) -> &AutomationPolicy {
        &self.policy
    }

    /// Set the automation policy the managers execute (§12).
    pub fn policy_mut(&mut self) -> &mut AutomationPolicy {
        &mut self.policy
    }

    /// Discover blueprint `i`, honoring its reputation gate against the player's
    /// current standings (§10/§25). Returns whether it was learned.
    pub fn discover_blueprint(&mut self, i: usize) -> bool {
        self.progression
            .blueprints
            .discover(i, &self.relations)
            .is_ok()
    }

    /// The wrecks currently sighted and awaiting salvage (§15).
    pub fn wrecks(&self) -> &[crate::sim::salvage::Wreck] {
        self.salvage.wrecks()
    }

    /// Strip the sighted wreck `id` (§15): bank its reward — scrap → credits, data
    /// → research, or a reverse-engineered blueprint (no rep gate) — and count it
    /// as an operation on the climb (§0). Returns whether a wreck was salvaged.
    pub fn salvage_wreck(&mut self, id: u64) -> bool {
        let Some(reward) = self.salvage.claim(id) else {
            return false;
        };
        match reward {
            SalvageReward::Scrap(credits) => self.corp.credit(credits),
            SalvageReward::Data(points) => self.progression.research.add_points(points),
            SalvageReward::Blueprint(i) => {
                self.progression.blueprints.reverse_engineer(i);
            }
        }
        self.events.push(Event::WreckSalvaged { id });
        // Salvaged data sometimes seeds the gate mystery (§15 anomaly → §0.1 lore).
        self.reveal_gate_beat();
        self.complete_op();
        true
    }

    /// One-press salvage of the first sighted wreck (§15/§0.4). Returns whether one
    /// was stripped.
    pub fn salvage_top(&mut self) -> bool {
        match self.salvage.first() {
            Some(id) => self.salvage_wreck(id),
            None => false,
        }
    }

    /// Set the player-tunable alert surfacing threshold (§19).
    pub fn set_alert_threshold(&mut self, min_priority: Priority) {
        self.feed.set_threshold(min_priority);
    }
}
