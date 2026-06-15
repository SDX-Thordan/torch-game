//! TORCH GDExtension entry point.
//!
//! This is the thin Godot binding (§26 Godot 4.x + Rust gdext). All game logic
//! lives in the engine-agnostic [`sim`] module; this file only exposes it to the
//! Godot shell. Keeping the boundary thin is what lets the core stay headless
//! and native-testable (§27, §32).

// gdext's `#[godot_api]` macro expands to `Result`s carrying Godot's large
// `CallError`; this clippy lint fires on generated code we don't control.
#![allow(clippy::result_large_err)]

pub mod sim;

use godot::prelude::*;

struct TorchExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TorchExtension {}

/// Bridge object the Godot shell instantiates to talk to the Rust core. For the
/// toolchain de-risk (§35.1) it just proves the binding is live; it will grow
/// into the snapshot + event-stream contract (§29).
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchCore {
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchCore {
    fn init(base: Base<RefCounted>) -> Self {
        Self { _base: base }
    }
}

#[godot_api]
impl TorchCore {
    /// Core crate version.
    #[func]
    fn version(&self) -> GString {
        GString::from(sim::VERSION)
    }

    /// Hello-world greeting from the Rust core.
    #[func]
    fn greeting(&self) -> GString {
        GString::from(sim::greeting())
    }

    /// Deterministic fingerprint of a seed — lets the shell verify the same
    /// seed yields the same result through the binding (§27 determinism).
    #[func]
    fn fingerprint(&self, seed: i64) -> i64 {
        sim::fingerprint(seed as u64) as i64
    }
}

/// Godot-facing handle to the deterministic [`sim::Sim`] (§29). Exposes the
/// fixed-tick advance plus scalar snapshot accessors the shell renders; the
/// real game logic stays in `sim`, this is only the binding.
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchSim {
    sim: sim::Sim,
    /// Whether the last `step` raised a fresh act-now alert (a shortage) — the
    /// shell uses this for auto-pause-on-exception (§28/§0.4).
    just_alerted: bool,
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchSim {
    fn init(base: Base<RefCounted>) -> Self {
        Self {
            sim: sim::Sim::new(0),
            just_alerted: false,
            _base: base,
        }
    }
}

#[godot_api]
impl TorchSim {
    /// Reseed and restart the simulation (§27 determinism).
    #[func]
    fn reset(&mut self, seed: i64) {
        self.sim = sim::Sim::new(seed as u64);
        self.just_alerted = false;
    }

    /// Advance one fixed sim tick (§28); returns the new tick. Also records
    /// whether this tick raised a fresh act-now shortage (for auto-pause).
    #[func]
    fn step(&mut self) -> i64 {
        let events = self.sim.step();
        self.just_alerted = events
            .iter()
            .any(|e| matches!(e, sim::Event::Scarcity { .. }));
        self.sim.tick() as i64
    }

    /// Whether the last `step` raised a fresh act-now alert — the shell pauses on
    /// this so the player never idles through dead time (§28/§36).
    #[func]
    fn just_alerted(&self) -> bool {
        self.just_alerted
    }

    /// Current tick.
    #[func]
    fn tick(&self) -> i64 {
        self.sim.tick() as i64
    }

    /// Number of bodies in the snapshot.
    #[func]
    fn body_count(&self) -> i64 {
        self.sim.bodies().len() as i64
    }

    #[func]
    fn body_name(&self, index: i64) -> GString {
        GString::from(
            self.sim
                .bodies()
                .get(index as usize)
                .map(|b| b.name)
                .unwrap_or(""),
        )
    }

    #[func]
    fn body_x(&self, index: i64) -> i64 {
        self.sim
            .snapshot()
            .bodies
            .get(index as usize)
            .map(|b| b.x)
            .unwrap_or(0)
    }

    #[func]
    fn body_y(&self, index: i64) -> i64 {
        self.sim
            .snapshot()
            .bodies
            .get(index as usize)
            .map(|b| b.y)
            .unwrap_or(0)
    }

    /// Number of markets (§7a).
    #[func]
    fn market_count(&self) -> i64 {
        self.sim.markets().len() as i64
    }

    #[func]
    fn market_name(&self, market: i64) -> GString {
        GString::from(
            self.sim
                .markets()
                .get(market as usize)
                .map(|m| m.name())
                .unwrap_or(""),
        )
    }

    /// Number of commodities (shared across markets).
    #[func]
    fn commodity_count(&self) -> i64 {
        self.sim.markets()[0].defs().len() as i64
    }

    #[func]
    fn commodity_name(&self, index: i64) -> GString {
        GString::from(
            self.sim.markets()[0]
                .defs()
                .get(index as usize)
                .map(|d| d.name)
                .unwrap_or(""),
        )
    }

    /// Hot-reload commodity numbers from a JSON tuning file at `path` (§31).
    /// Returns an empty string on success, or a human-readable error (bad path,
    /// invalid JSON, unknown commodity) for the shell to surface — the live sim is
    /// left untouched on any failure. File I/O lives here in the shell binding, not
    /// in the pure deterministic core.
    #[func]
    fn reload_commodity_data(&mut self, path: GString) -> GString {
        let path = path.to_string();
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .and_then(|json| self.sim.reload_commodities(&json));
        GString::from(result.err().unwrap_or_default())
    }

    /// Save the run to a JSON file at `path` (§30). Returns "" on success or a
    /// human-readable error. File I/O lives here in the shell binding, not the core.
    #[func]
    fn save_game(&self, path: GString) -> GString {
        let path = path.to_string();
        let result = std::fs::write(&path, self.sim.save_json())
            .map_err(|e| format!("cannot write {path}: {e}"));
        GString::from(result.err().unwrap_or_default())
    }

    /// Load a run from a JSON save file at `path` (§30), replacing the live sim.
    /// Returns "" on success or a human-readable error; the live sim is left
    /// untouched on any failure (it parses + rebuilds before swapping).
    #[func]
    fn load_game(&mut self, path: GString) -> GString {
        let path = path.to_string();
        match std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .and_then(|json| sim::Sim::load_json(&json))
        {
            Ok(sim) => {
                self.sim = sim;
                GString::new()
            }
            Err(e) => GString::from(e),
        }
    }

    /// Price of commodity `c` at market `m`.
    #[func]
    fn price(&self, market: i64, commodity: i64) -> i64 {
        self.sim
            .markets()
            .get(market as usize)
            .map(|m| m.price(commodity as usize))
            .unwrap_or(0)
    }

    /// Stock of commodity `c` at market `m`.
    #[func]
    fn stock(&self, market: i64, commodity: i64) -> i64 {
        self.sim
            .markets()
            .get(market as usize)
            .map(|m| m.stock(commodity as usize))
            .unwrap_or(0)
    }

    /// Number of haulers currently in flight (§7b).
    #[func]
    fn hauler_count(&self) -> i64 {
        self.sim.haulers().len() as i64
    }

    /// Id of the in-flight hauler at `index` (−1 if out of range).
    #[func]
    fn hauler_id(&self, index: i64) -> i64 {
        self.sim
            .haulers()
            .get(index as usize)
            .map(|h| h.id as i64)
            .unwrap_or(-1)
    }

    /// Position of the in-flight hauler at `index` (for the orrery, §21).
    #[func]
    fn hauler_x(&self, index: i64) -> i64 {
        self.sim
            .haulers()
            .get(index as usize)
            .map(|h| h.position(self.sim.tick()).0)
            .unwrap_or(0)
    }

    #[func]
    fn hauler_y(&self, index: i64) -> i64 {
        self.sim
            .haulers()
            .get(index as usize)
            .map(|h| h.position(self.sim.tick()).1)
            .unwrap_or(0)
    }

    /// Cut the in-flight hauler with `id`; returns whether one was interdicted.
    #[func]
    fn interdict(&mut self, id: i64) -> bool {
        self.sim.interdict(id as u64)
    }

    /// Attempt to interdict hauler `id` with an interceptor at `(x, y)` of the
    /// given `speed` and crew `skill_bp` (§7b). Returns the outcome:
    /// 0 = no solution, 1 = escaped, 2 = interdicted.
    #[func]
    fn attempt_interdict(&mut self, id: i64, x: i64, y: i64, speed: i64, skill_bp: i64) -> i64 {
        let interceptor = sim::Interceptor {
            pos: (x, y),
            speed,
            skill_bp,
        };
        match self.sim.interdict_with(id as u64, interceptor) {
            sim::Interdiction::NoSolution => 0,
            sim::Interdiction::Escaped => 1,
            sim::Interdiction::Interdicted => 2,
        }
    }

    /// Set the alert-feed surfacing threshold (§19): 0 = info, 1 = notice,
    /// 2 = warning, 3 = critical.
    #[func]
    fn set_alert_threshold(&mut self, level: i64) {
        let p = match level {
            0 => sim::Priority::Info,
            2 => sim::Priority::Warning,
            3 => sim::Priority::Critical,
            _ => sim::Priority::Notice,
        };
        self.sim.set_alert_threshold(p);
    }

    /// Number of alerts currently surfaced by the feed (§19).
    #[func]
    fn alert_count(&self) -> i64 {
        self.sim.feed().surfaced().len() as i64
    }

    /// Ranked surfaced-alert message at `index` (loudest, newest first).
    #[func]
    fn alert_message(&self, index: i64) -> GString {
        self.sim
            .feed()
            .surfaced()
            .get(index as usize)
            .map(|a| GString::from(a.message.as_str()))
            .unwrap_or_default()
    }

    /// Whether the surfaced alert at `index` is an act-now (verb) alert (§0.4).
    #[func]
    fn alert_is_act_now(&self, index: i64) -> bool {
        self.sim
            .feed()
            .surfaced()
            .get(index as usize)
            .map(|a| a.is_act_now())
            .unwrap_or(false)
    }

    /// One-press answer to the loudest open act-now shortage: exploit it (§0.4).
    /// Returns whether a shortage was answered.
    #[func]
    fn answer_shortage(&mut self) -> bool {
        self.sim.answer_top_shortage(20)
    }

    /// Number of derelicts currently sighted and awaiting salvage (§15).
    #[func]
    fn wreck_count(&self) -> i64 {
        self.sim.wrecks().len() as i64
    }

    /// Name of sighted wreck `i` (§15), or "".
    #[func]
    fn wreck_name(&self, i: i64) -> GString {
        GString::from(
            self.sim
                .wrecks()
                .get(i as usize)
                .map(|w| w.name)
                .unwrap_or(""),
        )
    }

    /// Strip the first sighted wreck (§15); returns whether one was salvaged.
    #[func]
    fn salvage_wreck(&mut self) -> bool {
        self.sim.salvage_top()
    }

    /// A §13 pressure gauge, `0..=100`: 0 = FactionWar, 1 = Piracy, 2 = Scarcity.
    #[func]
    fn pressure_level(&self, kind: i64) -> i64 {
        let k = match kind {
            0 => sim::PressureKind::FactionWar,
            2 => sim::PressureKind::Scarcity,
            _ => sim::PressureKind::Piracy,
        };
        self.sim.pressure().level(k) as i64
    }

    /// The loudest pressure gauge — the shell's overall threat read + §23c audio.
    #[func]
    fn pressure_peak(&self) -> i64 {
        self.sim.pressure().peak_level() as i64
    }

    /// Ticks until the next telegraphed raid strikes (§13 forecasting).
    #[func]
    fn raid_eta(&self) -> i64 {
        self.sim.pressure().raid_eta(self.sim.tick()) as i64
    }

    /// The pressure-intensity difficulty (§13): 0 = Calm, 1 = Normal, 2 = Harsh.
    #[func]
    fn intensity(&self) -> i64 {
        match self.sim.pressure().intensity() {
            sim::Intensity::Calm => 0,
            sim::Intensity::Normal => 1,
            sim::Intensity::Harsh => 2,
        }
    }

    /// Set the pressure-intensity difficulty (§13); clamps to 0 = Calm, 1 = Normal,
    /// 2 = Harsh.
    #[func]
    fn set_intensity(&mut self, level: i64) {
        let i = match level {
            0 => sim::Intensity::Calm,
            2 => sim::Intensity::Harsh,
            _ => sim::Intensity::Normal,
        };
        self.sim.set_intensity(i);
    }

    /// Number of factions the player has standings with (§10).
    #[func]
    fn faction_count(&self) -> i64 {
        sim::Faction::ALL.len() as i64
    }

    #[func]
    fn faction_name(&self, index: i64) -> GString {
        sim::Faction::ALL
            .get(index as usize)
            .map(|f| GString::from(f.name()))
            .unwrap_or_default()
    }

    /// Player standing with faction `index` (§10).
    #[func]
    fn faction_standing(&self, index: i64) -> i64 {
        sim::Faction::ALL
            .get(index as usize)
            .map(|f| self.sim.relations().standing(*f))
            .unwrap_or(0)
    }

    /// Reputation tier label with faction `index` (§10).
    #[func]
    fn faction_tier(&self, index: i64) -> GString {
        let tier = sim::Faction::ALL
            .get(index as usize)
            .map(|f| self.sim.relations().tier(*f));
        let label = match tier {
            Some(sim::RepTier::Hostile) => "hostile",
            Some(sim::RepTier::Cold) => "cold",
            Some(sim::RepTier::Neutral) => "neutral",
            Some(sim::RepTier::Cordial) => "cordial",
            Some(sim::RepTier::Allied) => "allied",
            None => "",
        };
        GString::from(label)
    }

    // ---- Progression (§10) ----

    /// Current CEO level.
    #[func]
    fn ceo_level(&self) -> i64 {
        self.sim.progression().ceo.level()
    }

    /// Grant the CEO experience (§10).
    #[func]
    fn ceo_gain_xp(&mut self, n: i64) {
        self.sim.progression_mut().ceo.gain_xp(n);
    }

    /// Commit to a perk branch (0 industrialist, 1 trader, 2 warlord,
    /// 3 diplomat); returns whether it was accepted (one-time choice).
    #[func]
    fn ceo_choose_branch(&mut self, code: i64) -> bool {
        let branch = match code {
            0 => sim::Branch::Industrialist,
            1 => sim::Branch::Trader,
            2 => sim::Branch::Warlord,
            _ => sim::Branch::Diplomat,
        };
        self.sim.progression_mut().ceo.choose_branch(branch).is_ok()
    }

    #[func]
    fn ceo_branch_name(&self) -> GString {
        let label = match self.sim.progression().ceo.branch() {
            Some(sim::Branch::Industrialist) => "Industrialist",
            Some(sim::Branch::Trader) => "Trader",
            Some(sim::Branch::Warlord) => "Warlord",
            Some(sim::Branch::Diplomat) => "Diplomat",
            None => "(none)",
        };
        GString::from(label)
    }

    /// Earn research points.
    #[func]
    fn research_add_points(&mut self, n: i64) {
        self.sim.progression_mut().research.add_points(n);
    }

    /// Attempt to unlock tech `i`; returns whether it was researched.
    #[func]
    fn research_tech(&mut self, i: i64) -> bool {
        self.sim
            .progression_mut()
            .research
            .research(i as usize)
            .is_ok()
    }

    /// Number of unlocked techs.
    #[func]
    fn research_unlocked_count(&self) -> i64 {
        self.sim.progression().research.unlocked_count() as i64
    }

    /// Unspent research points (earned through operations, §10).
    #[func]
    fn research_points(&self) -> i64 {
        self.sim.progression().research.points()
    }

    /// Aggregate drive-efficiency research bonus (percent).
    #[func]
    fn research_drive_bonus(&self) -> i64 {
        self.sim.progression().research.drive_bonus()
    }

    /// Discover blueprint `i` (honors its reputation gate); returns success.
    #[func]
    fn blueprint_discover(&mut self, i: i64) -> bool {
        self.sim.discover_blueprint(i as usize)
    }

    /// Number of known blueprints.
    #[func]
    fn blueprint_known_count(&self) -> i64 {
        self.sim.progression().blueprints.known_count() as i64
    }

    // ---- Managers & automation (§12) ----

    /// Set the standing interdiction policy: whether the patrol hunts, which
    /// faction to target (−1 = any, 0..3 by index), and a minimum cargo size.
    #[func]
    fn set_interdiction_policy(&mut self, enabled: bool, target: i64, min_cargo: i64) {
        let pol = &mut self.sim.policy_mut().interdiction;
        pol.enabled = enabled;
        pol.target = sim::Faction::ALL.get(target as usize).copied();
        pol.min_cargo = min_cargo;
    }

    /// Toggle auto-investment of research points (§10/§12).
    #[func]
    fn set_auto_research(&mut self, enabled: bool) {
        self.sim.policy_mut().auto_research = enabled;
    }

    // ---- The retention spine (§0) ----

    /// Current tier name (§0.3).
    #[func]
    fn tier_name(&self) -> GString {
        GString::from(self.sim.campaign().tier().name())
    }

    /// The *now* goal text — the current tier objective (§0.4).
    #[func]
    fn now_goal(&self) -> GString {
        GString::from(self.sim.campaign().now_goal().0)
    }

    /// Progress toward the next tier, in operations completed.
    #[func]
    fn now_goal_progress(&self) -> i64 {
        self.sim.campaign().now_goal().1
    }

    /// Operations needed to reach the next tier (0 = summit reached).
    #[func]
    fn now_goal_target(&self) -> i64 {
        self.sim.campaign().now_goal().2
    }

    /// How close the ring-gate is to opening, in percent (the far goal, §0.1).
    #[func]
    fn gate_progress_pct(&self) -> i64 {
        self.sim.campaign().gate_progress_bp() / 100
    }

    /// The current tier's "different kind of game" briefing (§0.3).
    #[func]
    fn tier_briefing(&self) -> GString {
        GString::from(self.sim.campaign().briefing())
    }

    /// How many stations the player may run at the current tier (scope widens as
    /// the company climbs, §0.3).
    #[func]
    fn station_cap(&self) -> i64 {
        self.sim.campaign().station_cap() as i64
    }

    /// How many standing trade routes the player may run at the current tier.
    #[func]
    fn route_cap(&self) -> i64 {
        self.sim.campaign().route_cap() as i64
    }

    // ---- The player corporation (§1/§5) ----

    /// Treasury balance in credits.
    #[func]
    fn credits(&self) -> i64 {
        self.sim.corp().credits()
    }

    /// Untasked trained crew available for new warships (§8c).
    #[func]
    fn trained_crew(&self) -> i64 {
        self.sim.corp().trained_crew()
    }

    /// Number of ships in the player fleet.
    #[func]
    fn fleet_size(&self) -> i64 {
        self.sim.corp().fleet().len() as i64
    }

    /// Name of fleet ship `i` (its christened call-sign + class, §14).
    #[func]
    fn ship_name(&self, i: i64) -> GString {
        GString::from(
            self.sim
                .corp()
                .fleet()
                .get(i as usize)
                .map(|s| s.name.as_str())
                .unwrap_or(""),
        )
    }

    /// Ticks ship `i` has been in service (its age, §11).
    #[func]
    fn ship_age(&self, i: i64) -> i64 {
        let now = self.sim.tick();
        self.sim
            .corp()
            .fleet()
            .get(i as usize)
            .map(|s| s.age(now) as i64)
            .unwrap_or(0)
    }

    /// Engagements ship `i` has fought, and how many it won (its blooding, §13).
    #[func]
    fn ship_battles(&self, i: i64) -> i64 {
        self.sim
            .corp()
            .fleet()
            .get(i as usize)
            .map(|s| s.battles as i64)
            .unwrap_or(0)
    }

    #[func]
    fn ship_battles_won(&self, i: i64) -> i64 {
        self.sim
            .corp()
            .fleet()
            .get(i as usize)
            .map(|s| s.battles_won as i64)
            .unwrap_or(0)
    }

    /// The fleet's most decorated hull — the hero ship to spotlight (§14), or "".
    #[func]
    fn flagship_name(&self) -> GString {
        GString::from(
            self.sim
                .corp()
                .flagship()
                .map(|s| s.name.as_str())
                .unwrap_or(""),
        )
    }

    /// Warehouse cargo held of commodity `c`.
    #[func]
    fn cargo(&self, commodity: i64) -> i64 {
        self.sim.corp().cargo(commodity as usize)
    }

    /// Buy `qty` of commodity `c` at market `m`; returns the credits spent, or
    /// −1 if the order could not be filled (§5).
    #[func]
    fn buy(&mut self, market: i64, commodity: i64, qty: i64) -> i64 {
        self.sim
            .buy(market as usize, commodity as usize, qty)
            .unwrap_or(-1)
    }

    /// Sell `qty` of commodity `c` into market `m`; returns the revenue, or −1 if
    /// it could not be filled (§5).
    #[func]
    fn sell(&mut self, market: i64, commodity: i64, qty: i64) -> i64 {
        self.sim
            .sell(market as usize, commodity as usize, qty)
            .unwrap_or(-1)
    }

    /// Commission a warship (0 frigate, 1 destroyer, 2 cruiser, 3 battleship)
    /// into the fleet; returns whether it was built (§5/§8c).
    #[func]
    fn commission_ship(&mut self, class: i64) -> bool {
        let class = match class {
            1 => sim::ShipClass::Destroyer,
            2 => sim::ShipClass::Cruiser,
            3 => sim::ShipClass::Battleship,
            _ => sim::ShipClass::Frigate,
        };
        self.sim.commission_ship(class).is_ok()
    }

    /// Freighters owned, for running trade-route standing orders (§4).
    #[func]
    fn freighters(&self) -> i64 {
        self.sim.corp().freighters()
    }

    /// Commission a civilian freighter; returns whether it was built (§5/§4).
    #[func]
    fn commission_freighter(&mut self) -> bool {
        self.sim.commission_freighter().is_ok()
    }

    /// Engage the player fleet against a raider pack at `band` (0 close, 1 medium,
    /// 2 long) and resolve the battle (§9). Returns 1 if the fleet held the field,
    /// 0 if it lost or stalemated, −1 if there were no warships to send.
    #[func]
    fn engage(&mut self, band: i64) -> i64 {
        let band = match band {
            0 => sim::Band::Close,
            2 => sim::Band::Long,
            _ => sim::Band::Medium,
        };
        match self.sim.engage_raiders(band) {
            Some(o) if o.winner == Some(0) => 1,
            Some(_) => 0,
            None => -1,
        }
    }

    /// Set a Trade Route standing order: buy `commodity` at `origin`, sell at
    /// `dest`, `qty`/trip, while the spread clears `min_margin` (§4).
    #[func]
    fn set_trade_route(
        &mut self,
        commodity: i64,
        origin: i64,
        dest: i64,
        qty: i64,
        min_margin: i64,
    ) {
        self.sim.set_trade_route(
            commodity as usize,
            origin as usize,
            dest as usize,
            qty,
            min_margin,
        );
    }

    /// Cancel the standing trade route.
    #[func]
    fn clear_trade_route(&mut self) {
        self.sim.clear_trade_route();
    }

    /// A one-line description of the current trade route and its state (§4).
    #[func]
    fn route_status(&self) -> GString {
        let Some(r) = self.sim.route() else {
            return GString::from("none — set one with [D]");
        };
        let names = self.sim.markets()[0].defs();
        let commodity = names.get(r.commodity).map(|d| d.name).unwrap_or("?");
        let origin = self.sim.markets()[r.origin].name();
        let dest = self.sim.markets()[r.dest].name();
        let state = if r.in_transit {
            "in transit"
        } else {
            let spread = self.sim.markets()[r.dest].price(r.commodity)
                - self.sim.markets()[r.origin].price(r.commodity);
            if spread >= r.min_margin {
                "loading"
            } else {
                "idle — spread below margin"
            }
        };
        let extra = self.sim.routes().len().saturating_sub(1);
        let suffix = if extra > 0 {
            format!(" (+{extra} more)")
        } else {
            String::new()
        };
        GString::from(format!(
            "{commodity} {origin}→{dest} ×{} [{state}]{suffix}",
            r.qty
        ))
    }

    /// How many standing trade routes are in the table (§4).
    #[func]
    fn route_count(&self) -> i64 {
        self.sim.routes().len() as i64
    }

    /// Found a refinery turning raw commodity `raw` (0..2) into its refined
    /// product, sourcing at `buy_market` and selling surplus at `sell_market`
    /// (§3.1). Returns whether it was built.
    #[func]
    fn found_refinery(&mut self, raw: i64, buy_market: i64, sell_market: i64) -> bool {
        self.sim
            .found_refinery(raw as usize, buy_market as usize, sell_market as usize)
            .is_ok()
    }

    /// Number of production stations the player owns (§3.1).
    #[func]
    fn station_count(&self) -> i64 {
        self.sim.stations().len() as i64
    }

    /// A one-line description of station `i` (§3.1).
    #[func]
    fn station_desc(&self, index: i64) -> GString {
        let Some(st) = self.sim.stations().get(index as usize) else {
            return GString::default();
        };
        let names = self.sim.markets()[0].defs();
        let input = names.get(st.input).map(|d| d.name).unwrap_or("?");
        let output = names.get(st.output).map(|d| d.name).unwrap_or("?");
        let at = self.sim.markets()[st.buy_market].name();
        GString::from(format!("Refinery {input}→{output} @ {at}"))
    }

    // ---- Faction contracts (§3.3/§16) ----

    /// Number of contracts on the board (open + accepted).
    #[func]
    fn contract_count(&self) -> i64 {
        self.sim.contracts().len() as i64
    }

    /// Number of open (not-yet-accepted) contracts on the board.
    #[func]
    fn open_contract_count(&self) -> i64 {
        self.sim.open_contract_count() as i64
    }

    /// A one-line description of contract `i` (§3.3): who wants what, where, the
    /// reward, and whether it's been accepted.
    #[func]
    fn contract_desc(&self, index: i64) -> GString {
        let Some(c) = self.sim.contracts().get(index as usize) else {
            return GString::default();
        };
        let good = self.sim.markets()[0]
            .defs()
            .get(c.commodity)
            .map(|d| d.name)
            .unwrap_or("?");
        let at = self.sim.markets()[c.market].name();
        let tag = if c.accepted { "[ACCEPTED] " } else { "" };
        GString::from(format!(
            "{tag}{}: {}× {good} → {at} for {} cr (+{} rep)",
            c.faction.name(),
            c.qty,
            c.reward,
            c.rep
        ))
    }

    /// Accept the first open contract on the board (§3.3). Returns whether one
    /// was accepted.
    #[func]
    fn accept_first_contract(&mut self) -> bool {
        let id = self
            .sim
            .contracts()
            .iter()
            .find(|c| !c.accepted)
            .map(|c| c.id);
        match id {
            Some(id) => self.sim.accept_contract(id),
            None => false,
        }
    }

    /// Accept-and-deliver the first contract whose owed cargo is already in the
    /// warehouse (§3.3 one-press). Returns whether one was fulfilled.
    #[func]
    fn fulfill_ready_contract(&mut self) -> bool {
        self.sim.fulfill_ready_contract().is_some()
    }

    // ---- The command deck: standing policy a CEO sets (§12) ----

    /// Whether the standing interdiction patrol is hunting.
    #[func]
    fn patrol_enabled(&self) -> bool {
        self.sim.policy().interdiction.enabled
    }

    /// Toggle the standing interdiction patrol on/off (§12).
    #[func]
    fn toggle_patrol(&mut self) {
        let on = self.sim.policy().interdiction.enabled;
        self.sim.policy_mut().interdiction.enabled = !on;
    }

    /// Name of the patrol's current target filter ("any" or a faction).
    #[func]
    fn patrol_target_name(&self) -> GString {
        let label = match self.sim.policy().interdiction.target {
            None => "any",
            Some(f) => f.name(),
        };
        GString::from(label)
    }

    /// Cycle the patrol target: any → Earth → Mars → Belt → Independents → any.
    #[func]
    fn cycle_patrol_target(&mut self) {
        let cur = self.sim.policy().interdiction.target;
        let next = match cur {
            None => Some(sim::Faction::ALL[0]),
            Some(f) => {
                let i = sim::Faction::ALL.iter().position(|x| *x == f).unwrap_or(0);
                sim::Faction::ALL.get(i + 1).copied()
            }
        };
        self.sim.policy_mut().interdiction.target = next;
    }

    /// Whether managers auto-invest research points.
    #[func]
    fn auto_research_enabled(&self) -> bool {
        self.sim.policy().auto_research
    }

    /// Toggle manager auto-research (§12).
    #[func]
    fn toggle_auto_research(&mut self) {
        let on = self.sim.policy().auto_research;
        self.sim.policy_mut().auto_research = on ^ true;
    }

    /// Research the cheapest currently-affordable tech; returns success (§10).
    #[func]
    fn research_next(&mut self) -> bool {
        let prog = self.sim.progression_mut();
        match prog.research.cheapest_researchable() {
            Some(i) => prog.research.research(i).is_ok(),
            None => false,
        }
    }

    /// Current alert-feed threshold as 0..3 (info..critical, §19).
    #[func]
    fn alert_threshold(&self) -> i64 {
        match self.sim.feed().threshold() {
            sim::Priority::Info => 0,
            sim::Priority::Notice => 1,
            sim::Priority::Warning => 2,
            sim::Priority::Critical => 3,
        }
    }

    /// Nudge the alert threshold by `delta`, clamped to 0..3 (§19).
    #[func]
    fn nudge_alert_threshold(&mut self, delta: i64) {
        let level = (self.alert_threshold() + delta).clamp(0, 3);
        self.set_alert_threshold(level);
    }
}

/// Godot-facing view of the warship catalog and reference fits (§8). Exposes the
/// derived stats of a sensible reference loadout per class so the shell can show
/// the railgun escalation axis; the fitting logic stays in `sim::ships`.
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchShipyard {
    classes: Vec<(GString, sim::ShipStats)>,
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchShipyard {
    fn init(base: Base<RefCounted>) -> Self {
        use sim::ships::{reference_loadout, ShipClass};
        let mut rng = sim::rng::Pcg32::new(1);
        let classes = [
            ShipClass::Frigate,
            ShipClass::Destroyer,
            ShipClass::Cruiser,
            ShipClass::Battleship,
        ]
        .into_iter()
        .map(|c| {
            let lo = reference_loadout(c, &mut rng);
            (GString::from(lo.hull().name), lo.stats())
        })
        .collect();
        Self {
            classes,
            _base: base,
        }
    }
}

#[godot_api]
impl TorchShipyard {
    #[func]
    fn class_count(&self) -> i64 {
        self.classes.len() as i64
    }

    #[func]
    fn class_name(&self, index: i64) -> GString {
        self.classes
            .get(index as usize)
            .map(|c| c.0.clone())
            .unwrap_or_default()
    }

    #[func]
    fn railguns(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.railguns as i64)
            .unwrap_or(0)
    }

    #[func]
    fn alpha(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.effective_alpha())
            .unwrap_or(0)
    }

    #[func]
    fn delta_v(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.delta_v)
            .unwrap_or(0)
    }

    #[func]
    fn mobility(&self, index: i64) -> i64 {
        self.classes
            .get(index as usize)
            .map(|c| c.1.thrust_to_mass)
            .unwrap_or(0)
    }

    /// Resolve a demo duel: `n` torpedo frigates vs one battleship at `band`
    /// (0 = close, 1 = medium, 2 = long). Returns a one-line result (§9).
    #[func]
    fn duel(&self, n: i64, band: i64) -> GString {
        use sim::combat::demo_duel;
        let band = match band {
            0 => sim::Band::Close,
            2 => sim::Band::Long,
            _ => sim::Band::Medium,
        };
        let out = demo_duel(n.max(0) as usize, band, 0);
        let who = match out.winner {
            Some(0) => "frigates win",
            Some(1) => "battleship wins",
            _ => "stalemate",
        };
        GString::from(format!("{who} in {} ticks", out.ticks))
    }
}
