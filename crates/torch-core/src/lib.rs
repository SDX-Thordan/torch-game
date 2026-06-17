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

    /// The kind of body `index` (§17): 0 Star, 1 Planet, 2 GasGiant, 3 Dwarf,
    /// 4 Moon, 5 Gate — for the orrery to size and colour it.
    #[func]
    fn body_kind(&self, index: i64) -> i64 {
        use sim::BodyKind::*;
        self.sim
            .bodies()
            .get(index as usize)
            .map(|b| match b.kind {
                Star => 0,
                Planet => 1,
                GasGiant => 2,
                DwarfPlanet => 3,
                Moon => 4,
                Gate => 5,
                FarSide => 6,
            })
            .unwrap_or(1)
    }

    /// Whether body `index` is on the **far side** of the gate (§17) — the shell
    /// hides these until the player transits.
    #[func]
    fn body_is_far_side(&self, index: i64) -> bool {
        self.sim
            .bodies()
            .get(index as usize)
            .map(|b| b.kind == sim::orbit::BodyKind::FarSide)
            .unwrap_or(false)
    }

    /// Whether the far side has been revealed (the player transited, §17).
    #[func]
    fn far_side_revealed(&self) -> bool {
        self.sim.campaign().transited()
    }

    /// The orbital radius of body `index` about its parent, in sim units — for
    /// drawing a moon's orbit ring around its (moving) planet (§17/§21).
    #[func]
    fn body_orbit_radius(&self, index: i64) -> i64 {
        self.sim
            .bodies()
            .get(index as usize)
            .map(|b| b.orbit_radius)
            .unwrap_or(0)
    }

    /// The parent body `index` orbits (its planet, for a moon; itself for Sol).
    #[func]
    fn body_parent(&self, index: i64) -> i64 {
        self.sim
            .bodies()
            .get(index as usize)
            .map(|b| b.parent as i64)
            .unwrap_or(0)
    }

    /// Number of settled frontier colonies (§17).
    #[func]
    fn colony_count(&self) -> i64 {
        sim::default_colonies().len() as i64
    }

    /// The body colony `i` sits on (§17).
    #[func]
    fn colony_body(&self, i: i64) -> i64 {
        sim::default_colonies()
            .get(i as usize)
            .map(|c| c.body as i64)
            .unwrap_or(-1)
    }

    /// The faction aligning colony `i`: 0 Earth, 1 Mars, 2 Belt (OPA), 3 Independents.
    #[func]
    fn colony_faction(&self, i: i64) -> i64 {
        use sim::Faction::*;
        sim::default_colonies()
            .get(i as usize)
            .map(|c| match c.faction {
                Earth => 0,
                Mars => 1,
                Belt => 2,
                Independents => 3,
            })
            .unwrap_or(3)
    }

    /// The name of colony `i` (§17).
    #[func]
    fn colony_name(&self, i: i64) -> GString {
        GString::from(
            sim::default_colonies()
                .get(i as usize)
                .map(|c| c.name)
                .unwrap_or(""),
        )
    }

    // ---- the empire layer: holdings & acquisition (E1) ----------------------

    /// Total holdings the player runs — built stations + controlled colonies.
    #[func]
    fn holding_count(&self) -> i64 {
        self.sim.holding_count() as i64
    }

    /// How many frontier colonies the player controls — the empire's size.
    #[func]
    fn controlled_colony_count(&self) -> i64 {
        self.sim.controlled_colony_count() as i64
    }

    /// The empire's rank by holdings (E6) — the headline of the expansion spine.
    #[func]
    fn empire_rank(&self) -> GString {
        GString::from(self.sim.empire_rank())
    }

    /// The next empire rank's name (E6), or "" at the summit.
    #[func]
    fn next_empire_rank_name(&self) -> GString {
        GString::from(self.sim.next_empire_rank().map(|(n, _)| n).unwrap_or(""))
    }

    /// Holdings needed to reach the next empire rank (E6), or −1 at the summit.
    #[func]
    fn next_empire_rank_at(&self) -> i64 {
        self.sim
            .next_empire_rank()
            .map(|(_, n)| n as i64)
            .unwrap_or(-1)
    }

    /// Holdings the player can govern efficiently before overextension (E2).
    #[func]
    fn admin_capacity(&self) -> i64 {
        self.sim.admin_capacity() as i64
    }

    /// The administrative load — one per holding (E2).
    #[func]
    fn admin_load(&self) -> i64 {
        self.sim.admin_load() as i64
    }

    /// Holdings over capacity (E2) — 0 when within administrative reach.
    #[func]
    fn admin_strain(&self) -> i64 {
        self.sim.admin_strain() as i64
    }

    /// Empire-wide tribute efficiency as a percent (E2): 100 within capacity, lower
    /// when overextended.
    #[func]
    fn holdings_efficiency_pct(&self) -> i64 {
        self.sim.holdings_efficiency_bp() / 100
    }

    /// The great powers' alarm at the player's expansion, 0..=1000 (E3).
    #[func]
    fn coalition_alarm(&self) -> i64 {
        self.sim.coalition_alarm()
    }

    /// Whether a great-power coalition has formed against the player (E3).
    #[func]
    fn coalition_active(&self) -> bool {
        self.sim.coalition_active()
    }

    /// A single faction's alarm at your expansion, 0..=1000 (E7): 0 Earth, 1 Mars,
    /// 2 Belt, 3 Independents.
    #[func]
    fn faction_alarm(&self, faction: i64) -> i64 {
        let f = match faction {
            0 => sim::Faction::Earth,
            1 => sim::Faction::Mars,
            2 => sim::Faction::Belt,
            _ => sim::Faction::Independents,
        };
        self.sim.faction_alarm(f)
    }

    /// The faction leading the coalition — the power whose sphere you've most provoked
    /// (E7): 0 Earth, 1 Mars, 2 Belt.
    #[func]
    fn coalition_leader(&self) -> i64 {
        match self.sim.coalition_leader() {
            sim::Faction::Earth => 0,
            sim::Faction::Mars => 1,
            sim::Faction::Belt => 2,
            sim::Faction::Independents => 3,
        }
    }

    // ---- corporate diplomacy with the independent companies (E8) ----

    /// Number of independent companies — the negotiable diplomatic actors (E8).
    #[func]
    fn company_count(&self) -> i64 {
        self.sim.company_count() as i64
    }

    /// Company `i`'s name (E8).
    #[func]
    fn company_name(&self, i: i64) -> GString {
        GString::from(
            self.sim
                .companies()
                .get(i as usize)
                .map(|c| c.name)
                .unwrap_or(""),
        )
    }

    /// Company `i`'s relation dial with the player (E8).
    #[func]
    fn company_relation(&self, i: i64) -> i64 {
        self.sim.company_relation(i as usize)
    }

    /// Company `i`'s stance (E8): 0 Rival, 1 Cold, 2 Neutral, 3 Partner, 4 Ally.
    #[func]
    fn company_stance(&self, i: i64) -> i64 {
        match self.sim.company_stance(i as usize) {
            sim::Stance::Rival => 0,
            sim::Stance::Cold => 1,
            sim::Stance::Neutral => 2,
            sim::Stance::Partner => 3,
            sim::Stance::Ally => 4,
        }
    }

    /// The colony company `i` operates (E8).
    #[func]
    fn company_home_colony(&self, i: i64) -> i64 {
        self.sim
            .companies()
            .get(i as usize)
            .map(|c| c.home_colony as i64)
            .unwrap_or(-1)
    }

    /// Allied companies lending you escorts against piracy (E8).
    #[func]
    fn ally_count(&self) -> i64 {
        self.sim.ally_count() as i64
    }

    /// Court independent company `i` up a step (E8) — spends Influence. Returns: 0 ok,
    /// 1 invalid company, 2 not enough Influence.
    #[func]
    fn court_company(&mut self, i: i64) -> i64 {
        use sim::world::CourtError as E;
        match self.sim.court_company(i as usize) {
            Ok(()) => 0,
            Err(E::InvalidCompany) => 1,
            Err(E::NotEnoughInfluence) => 2,
        }
    }

    /// Whether a coalition strike is bearing on the holdings right now (E3) — the
    /// shell lights the DEFEND HOLDINGS verb while this holds.
    #[func]
    fn coalition_strike_pending(&self) -> bool {
        self.sim.coalition_strike_pending()
    }

    /// Defend the holdings against the pending coalition strike at `band` (0 close,
    /// 1 medium, 2 long) (E3). Returns: 1 repelled, 0 the line broke, −1 nothing to
    /// answer / no warships.
    #[func]
    fn defend_holdings(&mut self, band: i64) -> i64 {
        let band = match band {
            0 => sim::Band::Close,
            2 => sim::Band::Long,
            _ => sim::Band::Medium,
        };
        match self.sim.defend_holdings(band) {
            Some(o) if o.winner == Some(0) => 1,
            Some(_) => 0,
            None => -1,
        }
    }

    /// Whether the player controls colony `i`.
    #[func]
    fn colony_controlled(&self, i: i64) -> bool {
        self.sim.colony_controlled(i as usize)
    }

    /// The commodity index a controlled colony `i` produces into your warehouse (EP1).
    #[func]
    fn colony_specialty(&self, i: i64) -> i64 {
        self.sim.colony_specialty(i as usize) as i64
    }

    /// Whether the player owns market `m` (EP2) — trades there are fee-reduced and NPC
    /// deliveries pay a tariff into the treasury.
    #[func]
    fn market_is_owned(&self, m: i64) -> bool {
        self.sim.market_is_owned(m as usize)
    }

    /// Escorts (warships on station) the empire needs to screen its shipping from
    /// piracy (EP3) — scales with holdings.
    #[func]
    fn escorts_needed(&self) -> i64 {
        self.sim.escorts_needed() as i64
    }

    /// Whether the empire's shipping is adequately escorted against piracy (EP3).
    #[func]
    fn empire_secure(&self) -> bool {
        self.sim.empire_secure()
    }

    /// The most-soured great-power standing (EP4) — negative means hostile space is
    /// taxing your trade (customs surcharges + inspection fines).
    #[func]
    fn worst_standing(&self) -> i64 {
        self.sim.worst_standing()
    }

    /// Whether colony `i` can be **bought** right now (an independent, not already
    /// yours) — the economic acquisition target.
    #[func]
    fn colony_acquirable(&self, i: i64) -> bool {
        let i = i as usize;
        self.sim.colony_acquire_cost(i).is_some() && !self.sim.colony_controlled(i)
    }

    /// The credit price to buy colony `i`, or −1 if it isn't an acquirable target.
    #[func]
    fn colony_acquire_cost(&self, i: i64) -> i64 {
        self.sim.colony_acquire_cost(i as usize).unwrap_or(-1)
    }

    /// The player's Influence — the statecraft resource for diplomatic annexation (E4).
    #[func]
    fn influence(&self) -> i64 {
        self.sim.influence()
    }

    /// Whether colony `i` can be diplomatically annexed right now (E4).
    #[func]
    fn colony_annexable(&self, i: i64) -> bool {
        self.sim.can_annex(i as usize)
    }

    /// Diplomatically annex independent colony `i` (E4 — the peaceful path). Returns:
    /// 0 ok, 1 not acquirable, 2 already controlled, 3 standing too low, 4 not enough
    /// influence.
    #[func]
    fn annex_colony(&mut self, i: i64) -> i64 {
        use sim::world::AnnexError as E;
        match self.sim.annex_colony(i as usize) {
            Ok(()) => 0,
            Err(E::NotAcquirable) => 1,
            Err(E::AlreadyControlled) => 2,
            Err(E::StandingTooLow) => 3,
            Err(E::NotEnoughInfluence) => 4,
        }
    }

    /// The defending garrison size for colony `i` (E5) — how hard it is to take by
    /// force (the inner powers garrison hard; independents barely at all).
    #[func]
    fn colony_garrison(&self, i: i64) -> i64 {
        self.sim.garrison_size(i as usize) as i64
    }

    /// Seize colony `i` by force at `band` (0 close, 1 medium, 2 long) (E5 — the
    /// aggressive path). Returns: 1 taken, 0 the assault failed, −1 invalid target,
    /// −2 already controlled, −3 no fleet.
    #[func]
    fn seize_colony(&mut self, i: i64, band: i64) -> i64 {
        let band = match band {
            0 => sim::Band::Close,
            2 => sim::Band::Long,
            _ => sim::Band::Medium,
        };
        use sim::world::SeizeError as E;
        match self.sim.seize_colony(i as usize, band) {
            Ok(o) if o.winner == Some(0) => 1,
            Ok(_) => 0,
            Err(E::InvalidTarget) => -1,
            Err(E::AlreadyControlled) => -2,
            Err(E::NoFleet) => -3,
        }
    }

    /// Buy out independent colony `i` (the empire layer's economic path). Returns:
    /// 0 ok, 1 not acquirable, 2 already controlled, 3 can't afford.
    #[func]
    fn acquire_colony(&mut self, i: i64) -> i64 {
        use sim::world::AcquireError as E;
        match self.sim.acquire_colony(i as usize) {
            Ok(()) => 0,
            Err(E::NotAcquirable) => 1,
            Err(E::AlreadyControlled) => 2,
            Err(E::CantAfford) => 3,
        }
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

    /// Whether market `m` is a far-side endgame market (§17) — hidden from the board
    /// until the gate is transited.
    #[func]
    fn market_is_far_side(&self, m: i64) -> bool {
        self.sim.is_far_side_market(m as usize)
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

    /// The orrery body a market sits at (§21), for click-to-select. `-1` if none.
    #[func]
    fn market_body(&self, market: i64) -> i64 {
        self.sim
            .markets()
            .get(market as usize)
            .map(|m| m.body() as i64)
            .unwrap_or(-1)
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

    /// Hot-reload hull + weapon numbers from a JSON tuning file at `path` (§31):
    /// retune the catalog future ships are fit from. Returns "" on success or a
    /// human-readable error; the live catalog is left untouched on any failure.
    #[func]
    fn reload_ship_data(&mut self, path: GString) -> GString {
        let path = path.to_string();
        let result = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .and_then(|json| self.sim.reload_ship_data(&json));
        GString::from(result.err().unwrap_or_default())
    }

    /// Save the run to `path` in the compact **binary** shipping format (§30).
    /// Returns "" on success or a human-readable error. File I/O lives here in the
    /// shell binding, not the core.
    #[func]
    fn save_game(&self, path: GString) -> GString {
        let path = path.to_string();
        let result = std::fs::write(&path, self.sim.save_bytes())
            .map_err(|e| format!("cannot write {path}: {e}"));
        GString::from(result.err().unwrap_or_default())
    }

    /// Export the run to `path` as a human-readable **JSON** document (§30 dev
    /// export — for inspection/debugging). Returns "" or an error.
    #[func]
    fn export_save_json(&self, path: GString) -> GString {
        let path = path.to_string();
        let result = std::fs::write(&path, self.sim.save_json())
            .map_err(|e| format!("cannot write {path}: {e}"));
        GString::from(result.err().unwrap_or_default())
    }

    /// Load a run from a save file at `path` (§30), replacing the live sim. The
    /// format (binary or JSON) is auto-detected, so old JSON saves still load.
    /// Returns "" on success or a human-readable error; the live sim is left
    /// untouched on any failure (it parses + rebuilds before swapping).
    #[func]
    fn load_game(&mut self, path: GString) -> GString {
        let path = path.to_string();
        match std::fs::read(&path)
            .map_err(|e| format!("cannot read {path}: {e}"))
            .and_then(|bytes| sim::Sim::load_bytes(&bytes))
        {
            Ok(sim) => {
                self.sim = sim;
                GString::new()
            }
            Err(e) => GString::from(e),
        }
    }

    /// Peek a save slot at `path` (§30): the saved tick, or `-1` if the file is
    /// missing or unreadable — for the archive UI's slot summaries. Reads either
    /// format.
    #[func]
    fn save_peek(&self, path: GString) -> i64 {
        let Ok(bytes) = std::fs::read(path.to_string()) else {
            return -1;
        };
        let looks_json = bytes
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .is_some_and(|&b| b == b'{');
        let save = if looks_json {
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(|j| sim::SaveState::from_json(j).ok())
        } else {
            sim::SaveState::from_bincode(&bytes).ok()
        };
        save.map(|s| s.tick as i64).unwrap_or(-1)
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

    /// The destination position of hauler `index` (§7b), for drawing its lane.
    #[func]
    fn hauler_dest_x(&self, index: i64) -> i64 {
        self.sim
            .haulers()
            .get(index as usize)
            .map(|h| h.dest_pos.0)
            .unwrap_or(0)
    }

    #[func]
    fn hauler_dest_y(&self, index: i64) -> i64 {
        self.sim
            .haulers()
            .get(index as usize)
            .map(|h| h.dest_pos.1)
            .unwrap_or(0)
    }

    /// Number of the player's freighters currently flying a standing route (§6),
    /// each a positional asset on the lanes.
    #[func]
    fn freighter_count(&self) -> i64 {
        self.sim.flying_routes().len() as i64
    }

    /// Position of the in-flight freighter at `index` (for the orrery, §6/§21).
    #[func]
    fn freighter_x(&self, index: i64) -> i64 {
        self.sim
            .flying_routes()
            .get(index as usize)
            .map(|&r| self.sim.route_freighter_pos(r).0)
            .unwrap_or(0)
    }

    #[func]
    fn freighter_y(&self, index: i64) -> i64 {
        self.sim
            .flying_routes()
            .get(index as usize)
            .map(|&r| self.sim.route_freighter_pos(r).1)
            .unwrap_or(0)
    }

    /// The destination position of the in-flight freighter at `index`, for its lane.
    #[func]
    fn freighter_dest_x(&self, index: i64) -> i64 {
        self.sim
            .flying_routes()
            .get(index as usize)
            .map(|&r| self.sim.route_dest_pos(r).0)
            .unwrap_or(0)
    }

    #[func]
    fn freighter_dest_y(&self, index: i64) -> i64 {
        self.sim
            .flying_routes()
            .get(index as usize)
            .map(|&r| self.sim.route_dest_pos(r).1)
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

    /// The player's corporation name (§14 expressive identity).
    #[func]
    fn corp_name(&self) -> GString {
        GString::from(self.sim.corp().name())
    }

    /// Adopt corp name preset `i` (cycled in the UI). Returns the new name.
    #[func]
    fn set_corp_name(&mut self, i: i64) -> GString {
        self.sim.set_corp_name_preset(i.max(0) as usize);
        GString::from(self.sim.corp().name())
    }

    /// The fleet livery colour (§14) as a Godot Color (rgb 0..1).
    #[func]
    fn corp_livery_color(&self) -> Color {
        let (r, g, b) = self.sim.corp().livery_rgb();
        Color::from_rgba(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
    }

    /// Cycle the fleet livery; returns the new index (§14).
    #[func]
    fn cycle_livery(&mut self) -> i64 {
        self.sim.cycle_corp_livery() as i64
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

    /// The orrery body sighted wreck `i` drifts near (§15/§21), for placing its
    /// marker on the map. `-1` if no such wreck.
    #[func]
    fn wreck_body(&self, i: i64) -> i64 {
        self.sim
            .wrecks()
            .get(i as usize)
            .map(|w| w.body as i64)
            .unwrap_or(-1)
    }

    /// Strip the first sighted wreck (§15); returns whether one was salvaged.
    #[func]
    fn salvage_wreck(&mut self) -> bool {
        self.sim.salvage_top()
    }

    /// A §13/§17 pressure gauge, `0..=100`: 0 = FactionWar, 1 = Piracy,
    /// 2 = Scarcity, 3 = Incursion (the far-side endgame threat).
    #[func]
    fn pressure_level(&self, kind: i64) -> i64 {
        let k = match kind {
            0 => sim::PressureKind::FactionWar,
            2 => sim::PressureKind::Scarcity,
            3 => sim::PressureKind::Incursion,
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

    /// Whether the player can transit the open ring-gate right now (§0.1/§17) — the
    /// climactic endgame verb is available standing at the gate, not yet through.
    #[func]
    fn can_transit_gate(&self) -> bool {
        self.sim.can_transit_gate()
    }

    /// Whether the player has already transited into the Beyond endgame (§17).
    #[func]
    fn gate_transited(&self) -> bool {
        self.sim.campaign().transited()
    }

    /// Transit the ring-gate into the endgame (§0.1/§17). Returns whether it
    /// happened (only at the open gate). The climax of the whole climb.
    #[func]
    fn transit_gate(&mut self) -> bool {
        self.sim.transit_gate()
    }

    // ---- the far-side bridgehead (§17 endgame, G3) ----

    /// Whether the player's far-side bridgehead has been founded (§17, G3).
    #[func]
    fn bridgehead_founded(&self) -> bool {
        self.sim.bridgehead().is_founded()
    }

    /// The bridgehead's upgrade level (0 if unfounded) (§17, G3).
    #[func]
    fn bridgehead_level(&self) -> i64 {
        self.sim.bridgehead().level() as i64
    }

    /// The bridgehead's current integrity (§17, G3/G4).
    #[func]
    fn bridgehead_integrity(&self) -> i64 {
        self.sim.bridgehead().integrity()
    }

    /// The bridgehead's maximum integrity at its current level (§17, G3).
    #[func]
    fn bridgehead_max_integrity(&self) -> i64 {
        self.sim.bridgehead().max_integrity()
    }

    /// Found the far-side bridgehead (§17, G3). Returns: 0 ok, 1 not in the Beyond,
    /// 2 can't afford, 3 already founded.
    #[func]
    fn found_bridgehead(&mut self) -> i64 {
        use sim::world::BridgeheadError as E;
        match self.sim.found_bridgehead() {
            Ok(()) => 0,
            Err(E::NotBeyond) => 1,
            Err(E::CantAfford) => 2,
            Err(E::AlreadyFounded) => 3,
            Err(E::NotFounded) => 3,
        }
    }

    /// Upgrade the far-side bridgehead (§17, G3). Returns: 0 ok, 2 can't afford,
    /// 3 not founded.
    #[func]
    fn upgrade_bridgehead(&mut self) -> i64 {
        use sim::world::BridgeheadError as E;
        match self.sim.upgrade_bridgehead() {
            Ok(()) => 0,
            Err(E::CantAfford) => 2,
            Err(E::NotFounded) => 3,
            Err(E::NotBeyond) | Err(E::AlreadyFounded) => 3,
        }
    }

    /// The active opening-mission title (§16), or "" once the tutorial is done.
    #[func]
    fn mission_title(&self) -> GString {
        GString::from(self.sim.missions().active().map(|m| m.title).unwrap_or(""))
    }

    /// The active opening-mission hint (§16), or "".
    #[func]
    fn mission_hint(&self) -> GString {
        GString::from(self.sim.missions().active().map(|m| m.hint).unwrap_or(""))
    }

    /// Opening missions completed, and the total (§16).
    #[func]
    fn mission_done_count(&self) -> i64 {
        self.sim.missions().opening_progress().0 as i64
    }

    #[func]
    fn mission_total(&self) -> i64 {
        self.sim.missions().opening_progress().1 as i64
    }

    /// The latest revealed gate-mystery beat (§0.1) — the authored destination pull.
    #[func]
    fn gate_lore(&self) -> GString {
        GString::from(self.sim.missions().latest_gate())
    }

    /// How many gate-mystery beats have been revealed so far (§0.1).
    #[func]
    fn gate_beats(&self) -> i64 {
        self.sim.missions().gate_beats_revealed() as i64
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

    /// The captain of fleet ship `i` (§11), or "".
    #[func]
    fn ship_captain(&self, i: i64) -> GString {
        GString::from(
            self.sim
                .corp()
                .fleet()
                .get(i as usize)
                .map(|s| s.loadout.crew().captain.as_str())
                .unwrap_or(""),
        )
    }

    /// The captain's flavour trait for fleet ship `i` (§11), or "".
    #[func]
    fn ship_trait(&self, i: i64) -> GString {
        GString::from(
            self.sim
                .corp()
                .fleet()
                .get(i as usize)
                .map(|s| sim::ships::captain_trait(&s.loadout.crew().captain))
                .unwrap_or(""),
        )
    }

    /// Rename ship `i`'s call-sign (§14), keeping its class suffix. Returns success.
    #[func]
    fn rename_ship(&mut self, i: i64, call_sign: GString) -> bool {
        self.sim.rename_ship(i as usize, &call_sign.to_string())
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

    /// Where warship `i` is (§6): the dock body name, or "→ Dest" in transit.
    #[func]
    fn ship_location(&self, i: i64) -> GString {
        let fleet = self.sim.corp().fleet();
        let Some(s) = fleet.get(i as usize) else {
            return GString::new();
        };
        let body = |b: usize| self.sim.bodies().get(b).map(|x| x.name).unwrap_or("?");
        if s.nav.in_transit(self.sim.tick()) {
            GString::from(format!("→ {}", body(s.nav.dest)))
        } else {
            GString::from(body(s.nav.location))
        }
    }

    /// Warship `i`'s remass (fuel) as basis points of tankage, 0..=10000 (§6).
    #[func]
    fn ship_fuel_bp(&self, i: i64) -> i64 {
        self.sim
            .corp()
            .fleet()
            .get(i as usize)
            .map(|s| s.nav.fuel_bp())
            .unwrap_or(0)
    }

    /// Whether warship `i` is mid-trajectory (§6).
    #[func]
    fn ship_in_transit(&self, i: i64) -> bool {
        self.sim
            .corp()
            .fleet()
            .get(i as usize)
            .map(|s| s.nav.in_transit(self.sim.tick()))
            .unwrap_or(false)
    }

    /// Absolute orrery position of warship `i` (§6/§21) — for drawing it on the map.
    #[func]
    fn ship_x(&self, i: i64) -> i64 {
        self.sim.ship_position(i as usize).0
    }

    #[func]
    fn ship_y(&self, i: i64) -> i64 {
        self.sim.ship_position(i as usize).1
    }

    /// Order warship `i` to fly to `dest` body at an economical or hard burn (§6).
    /// Returns "" on success, else a short reason (busy / no fuel / already there).
    #[func]
    fn move_ship(&mut self, i: i64, dest: i64, hard_burn: bool) -> GString {
        use sim::world::MoveError::*;
        match self.sim.move_ship(i as usize, dest as usize, hard_burn) {
            Ok(()) => GString::new(),
            Err(Busy) => GString::from("ship is already under way"),
            Err(InsufficientRemass) => GString::from("not enough remass — refuel first"),
            Err(AlreadyThere) => GString::from("already docked there"),
            Err(NoSuchShip) | Err(BadDestination) => GString::from("no such ship/destination"),
        }
    }

    /// Refuel docked warship `i` to a full tank, buying remass (§6). Returns success.
    #[func]
    fn refuel_ship(&mut self, i: i64) -> bool {
        self.sim.refuel_ship(i as usize)
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

    /// Fleet index of the flagship (§14), or -1 if the fleet is empty.
    #[func]
    fn flagship_index(&self) -> i64 {
        self.sim.corp().flagship_index()
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
        self.sim.commission_ship(warship_class(class)).is_ok()
    }

    /// Assemble a warship of `class` from the player's own Assembled-tier component
    /// stock (§7d) — the production-chain payoff. Returns 0 on success, or an error
    /// code: 1 = missing parts, 2 = can't afford the labour fee, 3 = not enough crew.
    #[func]
    fn assemble_ship(&mut self, class: i64) -> i64 {
        match self.sim.assemble_ship(warship_class(class)) {
            Ok(()) => 0,
            Err(sim::CommissionError::MissingParts) => 1,
            Err(sim::CommissionError::CantAfford) => 2,
            Err(sim::CommissionError::NotEnoughCrew) => 3,
        }
    }

    /// A one-line bill of materials for assembling `class` (§7d), e.g.
    /// "2 Machinery, 1 Drives" — for the BUILD view.
    #[func]
    fn ship_bom_desc(&self, class: i64) -> GString {
        let defs = self.sim.markets()[0].defs();
        let parts: Vec<String> = sim::Sim::ship_bom(warship_class(class))
            .iter()
            .map(|&(c, q)| format!("{q} {}", defs.get(c).map(|d| d.name).unwrap_or("?")))
            .collect();
        GString::from(parts.join(", "))
    }

    /// Whether the player currently holds the full bill of materials to assemble
    /// `class` (§7d) — drives the BUILD view's assemble button state.
    #[func]
    fn can_assemble_ship(&self, class: i64) -> bool {
        sim::Sim::ship_bom(warship_class(class))
            .iter()
            .all(|&(c, q)| self.sim.corp().cargo(c) >= q)
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

    /// Warships on station at the home core, ready to answer a raider muster (§6).
    /// Lets the shell tell "no fleet" (commission one) apart from "fleet off
    /// station" (recall it) when an engage finds no defenders.
    #[func]
    fn warships_on_station(&self) -> i64 {
        self.sim.warships_on_station() as i64
    }

    // ---- far-side incursions (§17 endgame, G4) ------------------------------

    /// Whether an incursion is currently bearing on the bridgehead (§17, G4) — the
    /// shell lights the DEFEND verb while this holds.
    #[func]
    fn incursion_pending(&self) -> bool {
        self.sim.incursion_pending()
    }

    /// The severity of the pending incursion (0 if none) (§17, G4).
    #[func]
    fn pending_incursion_severity(&self) -> i64 {
        self.sim.pending_incursion_severity()
    }

    /// How the far-side endgame has resolved (§17, G5): 0 undecided, 1 won, 2 lost.
    #[func]
    fn endgame_outcome(&self) -> i64 {
        match self.sim.endgame_outcome() {
            sim::EndgameOutcome::Undecided => 0,
            sim::EndgameOutcome::Triumph => 1,
            sim::EndgameOutcome::Fallen => 2,
        }
    }

    /// Incursions repelled so far — victory progress (§17, G5).
    #[func]
    fn incursions_survived(&self) -> i64 {
        self.sim.incursions_survived() as i64
    }

    /// The endgame win threshold for the bridgehead level (§17, G5).
    #[func]
    fn endgame_target_level(&self) -> i64 {
        self.sim.endgame_targets().0 as i64
    }

    /// The endgame win threshold for incursions survived (§17, G5).
    #[func]
    fn endgame_target_incursions(&self) -> i64 {
        self.sim.endgame_targets().1 as i64
    }

    /// Defend the bridgehead against the pending incursion at `band` (0 close, 1
    /// medium, 2 long) (§17, G4). Returns 1 if repelled, 0 if the line broke, −1 if
    /// there was no incursion to answer or no warships to answer with.
    #[func]
    fn defend_bridgehead(&mut self, band: i64) -> i64 {
        let band = match band {
            0 => sim::Band::Close,
            2 => sim::Band::Long,
            _ => sim::Band::Medium,
        };
        match self.sim.defend_bridgehead(band) {
            Some(o) if o.winner == Some(0) => 1,
            Some(_) => 0,
            None => -1,
        }
    }

    // ---- combat doctrine + the diorama BattleLog (§9/§22) -------------------

    /// Set the player's target priority (§9): 0 = biggest hull, 1 = most wounded.
    #[func]
    fn set_combat_target(&mut self, t: i64) {
        self.sim.set_combat_target(if t == 1 {
            sim::combat::TargetPriority::Weakest
        } else {
            sim::combat::TargetPriority::Biggest
        });
    }

    /// The player's target priority (0 biggest, 1 weakest).
    #[func]
    fn combat_target(&self) -> i64 {
        match self.sim.combat_doctrine().target {
            sim::combat::TargetPriority::Weakest => 1,
            _ => 0,
        }
    }

    /// Set the player's retreat threshold in percent (§9): break off below this
    /// fraction of the starting fleet. 0 = fight to the death.
    #[func]
    fn set_combat_retreat(&mut self, pct: i64) {
        self.sim.set_combat_retreat(pct.clamp(0, 100) * 100);
    }

    /// The player's retreat threshold in percent (§9).
    #[func]
    fn combat_retreat(&self) -> i64 {
        self.sim.combat_doctrine().retreat_bp / 100
    }

    /// Toggle aggressive (hot) railgun fire (§9 heat): more alpha, but builds heat
    /// that periodically vents.
    #[func]
    fn set_combat_aggressive(&mut self, on: bool) {
        self.sim.set_combat_aggressive(on);
    }

    /// Whether the fleet fires railguns aggressively (§9).
    #[func]
    fn combat_aggressive(&self) -> bool {
        self.sim.combat_doctrine().aggressive_fire
    }

    /// Number of events in the last battle's log (§22 diorama), 0 if none.
    #[func]
    fn battle_log_count(&self) -> i64 {
        self.sim
            .last_battle()
            .map(|b| b.2.log.len() as i64)
            .unwrap_or(0)
    }

    /// Kind of battle-log event `i`: 0 Salvo, 1 Volley, 2 Destroyed, 3 Retreat.
    #[func]
    fn battle_event_kind(&self, i: i64) -> i64 {
        use sim::combat::CombatEvent::*;
        self.sim
            .last_battle()
            .and_then(|b| b.2.log.get(i as usize))
            .map(|e| match e {
                Salvo { .. } => 0,
                Volley { .. } => 1,
                Destroyed { .. } => 2,
                Retreat { .. } => 3,
                Overheat { .. } => 4,
            })
            .unwrap_or(-1)
    }

    /// Which side (0 player, 1 raiders) battle-log event `i` belongs to.
    #[func]
    fn battle_event_side(&self, i: i64) -> i64 {
        use sim::combat::CombatEvent::*;
        self.sim
            .last_battle()
            .and_then(|b| b.2.log.get(i as usize))
            .map(|e| match e {
                Salvo { side, .. }
                | Volley { side, .. }
                | Destroyed { side, .. }
                | Retreat { side }
                | Overheat { side } => *side as i64,
            })
            .unwrap_or(0)
    }

    /// The numeric detail of event `i` (salvo leakers, volley damage; else 0).
    #[func]
    fn battle_event_value(&self, i: i64) -> i64 {
        use sim::combat::CombatEvent::*;
        self.sim
            .last_battle()
            .and_then(|b| b.2.log.get(i as usize))
            .map(|e| match e {
                Salvo { leakers, .. } => *leakers,
                Volley { damage, .. } => *damage,
                _ => 0,
            })
            .unwrap_or(0)
    }

    /// The destroyed ship's name for event `i` (else "").
    #[func]
    fn battle_event_name(&self, i: i64) -> GString {
        use sim::combat::CombatEvent::*;
        GString::from(
            self.sim
                .last_battle()
                .and_then(|b| b.2.log.get(i as usize))
                .and_then(|e| match e {
                    Destroyed { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .unwrap_or(""),
        )
    }

    /// Winner of the last battle: 0 player, 1 raiders, -1 stalemate/none.
    #[func]
    fn battle_winner(&self) -> i64 {
        self.sim
            .last_battle()
            .map(|b| match b.2.winner {
                Some(s) => s as i64,
                None => -1,
            })
            .unwrap_or(-1)
    }

    /// Starting count for `side` (0 player, 1 raiders) in the last battle.
    #[func]
    fn battle_start_count(&self, side: i64) -> i64 {
        self.sim
            .last_battle()
            .map(|b| b.1[(side as usize).min(1)] as i64)
            .unwrap_or(0)
    }

    /// Surviving count for `side` in the last battle.
    #[func]
    fn battle_survivors(&self, side: i64) -> i64 {
        self.sim
            .last_battle()
            .map(|b| b.2.survivors[(side as usize).min(1)] as i64)
            .unwrap_or(0)
    }

    /// The band the last battle was fought at (0 close, 1 medium, 2 long).
    #[func]
    fn battle_band(&self) -> i64 {
        self.sim
            .last_battle()
            .map(|b| match b.0 {
                sim::Band::Close => 0,
                sim::Band::Medium => 1,
                sim::Band::Long => 2,
            })
            .unwrap_or(1)
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

    /// A one-line description of standing route `index` (§4), for the master-table.
    #[func]
    fn route_desc(&self, index: i64) -> GString {
        let Some(r) = self.sim.routes().get(index as usize) else {
            return GString::new();
        };
        let commodity = self.sim.markets()[0]
            .defs()
            .get(r.commodity)
            .map(|d| d.name)
            .unwrap_or("?");
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
                "idle"
            }
        };
        GString::from(format!("{commodity} {origin}→{dest} ×{} [{state}]", r.qty))
    }

    /// A "Origin → Dest" trip label for the in-flight freighter at `index` (§6),
    /// for the FLEET view's real en-route readout.
    #[func]
    fn freighter_trip(&self, index: i64) -> GString {
        match self.sim.flying_routes().get(index as usize) {
            Some(&r) => {
                let rt = &self.sim.routes()[r];
                GString::from(format!(
                    "{} → {}",
                    self.sim.markets()[rt.origin].name(),
                    self.sim.markets()[rt.dest].name()
                ))
            }
            None => GString::new(),
        }
    }

    /// Trip progress (0..=100%) of the in-flight freighter at `index` (§6).
    #[func]
    fn freighter_progress(&self, index: i64) -> i64 {
        match self.sim.flying_routes().get(index as usize) {
            Some(&r) => self.sim.route_progress_bp(r) / 100,
            None => 0,
        }
    }

    /// Remass the in-flight freighter at `index` burns on its trip (§6 fuel cost).
    #[func]
    fn freighter_fuel(&self, index: i64) -> i64 {
        match self.sim.flying_routes().get(index as usize) {
            Some(&r) => self.sim.route_remass_units(r),
            None => 0,
        }
    }

    /// Found a factory refining `input` into the next tier up its production line
    /// (§7d: Raw → Refined → Components → Assembled), sourcing at `buy_market` and
    /// selling surplus at `sell_market` (§3.1). Any non-top-tier commodity works.
    /// Returns whether it was built.
    #[func]
    fn found_refinery(&mut self, input: i64, buy_market: i64, sell_market: i64) -> bool {
        self.sim
            .found_refinery(input as usize, buy_market as usize, sell_market as usize)
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

/// Map a shell class index (0 Frigate, 1 Destroyer, 2 Cruiser, 3 Battleship) to a
/// `ShipClass`, defaulting to Frigate (§8b).
fn warship_class(class: i64) -> sim::ShipClass {
    match class {
        1 => sim::ShipClass::Destroyer,
        2 => sim::ShipClass::Cruiser,
        3 => sim::ShipClass::Battleship,
        _ => sim::ShipClass::Frigate,
    }
}

/// Godot-facing view of the warship catalog and reference fits (§8). Exposes the
/// derived stats of a sensible reference loadout per class so the shell can show
/// the railgun escalation axis; the fitting logic stays in `sim::ships`.
#[derive(GodotClass)]
#[class(base = RefCounted)]
struct TorchShipyard {
    classes: Vec<(GString, sim::ShipStats)>,
    /// Per-class weapon mounts `[pdc, torpedo, railgun, utility]` — the hardpoint
    /// counts the procedural ship forge places weapon models on (§24/§25).
    mounts: Vec<[u32; 4]>,
    _base: Base<RefCounted>,
}

#[godot_api]
impl IRefCounted for TorchShipyard {
    fn init(base: Base<RefCounted>) -> Self {
        use sim::ships::{reference_loadout, ShipClass};
        let mut rng = sim::rng::Pcg32::new(1);
        let mut classes = Vec::new();
        let mut mounts = Vec::new();
        for c in [
            ShipClass::Frigate,
            ShipClass::Destroyer,
            ShipClass::Cruiser,
            ShipClass::Battleship,
        ] {
            let lo = reference_loadout(c, &mut rng);
            let h = lo.hull();
            mounts.push([
                h.pdc_mounts,
                h.torpedo_mounts,
                h.railgun_mounts,
                h.utility_mounts,
            ]);
            classes.push((GString::from(h.name), lo.stats()));
        }
        Self {
            classes,
            mounts,
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

    /// PDC mounts on class `index` — point-defence hardpoints (§8a/§24).
    #[func]
    fn pdc_mounts(&self, index: i64) -> i64 {
        self.mounts
            .get(index as usize)
            .map(|m| m[0] as i64)
            .unwrap_or(0)
    }

    /// Torpedo mounts on class `index` (§8a/§24).
    #[func]
    fn torpedo_mounts(&self, index: i64) -> i64 {
        self.mounts
            .get(index as usize)
            .map(|m| m[1] as i64)
            .unwrap_or(0)
    }

    /// Railgun mounts on class `index` — the capital-defining hardpoints (§8b/§24).
    #[func]
    fn railgun_mounts(&self, index: i64) -> i64 {
        self.mounts
            .get(index as usize)
            .map(|m| m[2] as i64)
            .unwrap_or(0)
    }

    /// Utility mounts on class `index` — radiators/sensors/etc. (§8a/§24).
    #[func]
    fn utility_mounts(&self, index: i64) -> i64 {
        self.mounts
            .get(index as usize)
            .map(|m| m[3] as i64)
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
