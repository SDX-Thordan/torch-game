extends Node3D

## TORCH — the playable shell (§18–§21). A real-time-with-pause game loop over the
## deterministic Rust core, presented as a single cohesive instrument: a rounded
## bezel, a top status bar (logo · date · resources), a left nav rail, and four
## switchable views that share one visual language (see `ui/ui_kit.gd`):
##
##   • SYSTEMS  — the 3D orrery + a station context panel (status / resources /
##                construction queues / standing-order toggles).
##   • FLEET    — the fleet roster as a sortable-feeling table (ALL/IDLE tabs).
##   • BUILD    — the shipyard: hull list → wireframe blueprint → cost → queue.
##   • MARKET   — commodity-flow schematic + market ticker + price-history chart.
##
## All game logic lives in the Rust `sim`; this scene drives `step()` on a clock,
## mirrors the snapshot into the views/3D nodes, and turns input into sim verbs.

const UiKit := preload("res://ui/ui_kit.gd")
const MiniChartS := preload("res://ui/mini_chart.gd")
const FlowGraphS := preload("res://ui/flow_graph.gd")

const TICKS_PER_SECOND := 6.0           # sim ticks per real second at 1× (§28)
const SPEEDS := [0.0, 1.0, 6.0, 24.0]   # pause / 1× / 6× / 24× (§6)
# Opening calm (§0.3): don't surface act-now dilemmas in the first stretch — let the
# player settle in before the world starts hard-pausing them. The sim still runs and
# queues decisions (they may simply time out unshown); this only delays the modal/lock,
# so it's byte-identical to the core. ~10s of real time at 1×.
const EARLY_GRACE_TICKS := 60
const THRESHOLD_NAMES := ["info", "notice", "warning", "critical"]
const BRANCH_NAMES := ["Industrialist", "Trader", "Warlord", "Diplomat"]
const INTENSITY_NAMES := ["Calm", "Normal", "Harsh"]   # §13 pressure difficulty
const QTY_STEP := 5
const QTY_MAX := 500
const SAVE_SLOTS := 3                        # numbered manual save slots (§30)
const IRONMAN_AUTOSAVE_SEC := 20.0           # how often Ironman autosaves

# Views (index = nav-rail order).
const V_SYSTEMS := 0
const V_FLEET := 1
const V_BUILD := 2
const V_MARKET := 3
const V_EMPIRE := 4
const V_RESEARCH := 5
const V_LEDGER := 6
const V_DIPLOMACY := 7
const VIEW_GLYPH := ["◎", "◈", "⛭", "⇄", "✪", "⚛", "▤", "⚑"]
const VIEW_CAP := ["SYSTEMS", "FLEETS", "SHIPYARD", "MARKET", "EMPIRE", "RESEARCH", "LEDGER", "DIPLOMACY"]
const VIEW_TITLE := [
	"Orrery — Sol System",
	"Fleet Management",
	"Orbital Shipyard",
	"Market & Logistics",
	"Empire — Holdings & Expansion",
	"Research — Tech Tree",
	"Ledger — Assets",
	"Diplomacy — Powers & Companies",
]

# 3D orrery framing (§17/§21). Clean mapping: 1 AU = 1 world unit.
const SCALE3D := 1.0 / 1_000_000.0
const VIEW_LERP := 9.0   # orrery marker position smoothing rate (§28 interpolation)
const CAM_DIR := Vector3(0.0, 1.15, 0.9)
const ZOOM_MIN := 0.05   # zoom right down onto a single body / its inner moons (§21)
const ZOOM_MAX := 140.0
# Orbit-line width: scale the torus tube with the camera distance (zoom) so an orbit ring
# reads as a constant ~hairline at any zoom, instead of a fat band when you zoom in.
const ORBIT_TUBE_K := 0.00060
const ORBIT_TUBE_MIN := 0.00008
const ORBIT_TUBE_MAX := 0.30
# Level-of-detail by zoom (smaller zoom = closer): only reveal moon/station detail once
# zoomed in past these thresholds, so the wide system view isn't a clutter of labels.
const STATION_VIS_ZOOM := 5.0
const MOON_VIS_ZOOM := 1.6
# Labels render at a constant on-screen size (fixed_size) so they're always legible and
# never balloon when you zoom in; this is the per-label scale.
const LABEL_PIXEL := 0.00055
const LABEL_PIXEL_SMALL := 0.00045
const ROT_DRAG_SENS := 0.006                  # one-finger drag → yaw (rad per screen px)
const ROT_STEP := 0.40                         # ↺/↻ button + Q/E key rotation step (rad)
const PAN_SENS := 0.0016                        # mouse-drag pan (world units per screen px, ×zoom)
# UI magnification (§33 readability) — the whole HUD scales by this; touch needs it bigger
# than a desktop monitor, and a desktop monitor wants it a touch bigger than 1:1 so the
# dense tables stay legible when the window is maximized/fullscreen. Applied via the
# window's content scale (canvas_items stretch).
const UI_SCALE_TOUCH := 1.35
const UI_SCALE_PC := 1.4
const FACTION_COL := [
	Color(0.4, 0.6, 1.0), Color(0.95, 0.45, 0.4),
	Color(0.95, 0.75, 0.35), Color(0.55, 0.85, 0.6),
]
const HAULER_COL := Color(0.95, 0.7, 0.35)
const SELECT_COL := Color(1.0, 0.4, 0.2)
const CHART_COMMS := 4   # commodities tracked in the price-history chart

var sim: TorchSim
var shipyard: TorchShipyard   # the hull catalog for the BUILD view
var speed_idx := 1
var accum := 0.0
var auto_pause := true
var selected := 0
var view := V_SYSTEMS
var flash := 0.0
var ascend_flash := 0.0
var last_tier := ""
var _last_endgame := 0                       # 0 undecided · 1 won · 2 lost (§17, G5)
var _zoom := 10.0
var _focus_body := 0
var _yaw := 0.0                              # camera orbit angle (rad) — rotate the map
var _pan := Vector3.ZERO                      # ecliptic-plane pan offset from the focus (mouse-drag)
var _fullscreen := true                      # PC starts in true fullscreen (tiling WMs honour it, unlike "maximized"); F11 toggles to a maximized window
var _touches := {}
var _pinch_prev := 0.0
var _pinch_ang_prev := 0.0                   # two-finger twist angle (rad) for rotation
var _was_drag := false                       # a one-finger drag rotated — suppress the tap-focus
var _drag_px := 0.0                           # cumulative drag distance this gesture (click vs drag)
const DRAG_SLOP := 6.0                        # px a press may wander before it counts as a drag, not a click
# PC mode (desktop): mouse-wheel zoom + keyboard, no touch-zoom buttons. Auto-detected
# on desktop, manually toggleable (F8) so it can be forced on either platform.
var pc_mode := false
var _map_controls: Control
var _was_multitouch := false
var _last_chart_tick := -1

# The trade cursor (§5).
var sel_comm := 5
var sel_market := 0
var trade_qty := 20
var ceo_pick := 2
var build_pick := 0     # ship class selected in the BUILD view
var fleet_tab := 0      # 0 ALL · 1 FLEETS(warships) · 2 SINGLE SHIPS(freighters) · 3 IDLE

# Commodity indices resolved by name for the top-bar readouts.
var _idx_ore := 1
var _idx_fuel := 5
var _idx_water := 0

var status := "Welcome, CEO."

# ---- chrome refs ------------------------------------------------------------
var _layer: CanvasLayer
var _content: Control
var _views: Array[Control] = []
var _nav_buttons: Array[Button] = []
var _title: Label
var _date: Label
var _emp_header: Label
var _emp_meters: Label
var _emp_table: RichTextLabel
var _res_credits: Label
var _res_ore: Label
var _res_fuel: Label
var _res_crew: Label
var _res_influence: Label
var _rate_credits: Label
var _rate_ore: Label
var _rate_fuel: Label
var _rate_influence: Label
# Per-day resource rates (sampled from a daily snapshot — the management-sim readout).
var _rate_day := -1
var _rate_snap := {}
var _alert_ticker: Label
var _flash_rect: ColorRect
var _ascend_rect: ColorRect
var _help: Label

# Systems view.
var _sys_title: Label
var _sys_sub: Label
var _sys_object: RichTextLabel
var _sys_status: Label
var _sys_resources: VBoxContainer
var _sys_queues: VBoxContainer
var _sys_now: Label
var _defend_holdings_btn: Button
var _ctx_actions: VBoxContainer            # the contextual-action stack (no persistent buttons)
var _mine_btn: Button
var _withdraw_btn: Button
var _outpost_btn: Button
var _dev_outpost_btn: Button
var _fac_mine_btn: Button
var _fac_storage_btn: Button
var _fac_hangar_btn: Button
var _promote_btn: Button
var _build_btn: Button
var _expand_btn: Button
var _court_btn: Button
var _claim_btn: Button
var _acquire_ctx_btn: Button
var _develop_btn: Button
var _send_btn: Button
var _sys_mission: Label
var _sys_census: Label
var _census_static := ""   # the static body census (planets/moons/asteroids — computed once)
var _tg_patrol: CheckButton
var _tg_research: CheckButton
var _tg_pause: CheckButton
var _feed: RichTextLabel

# Decision panel (Phase A): act-now dilemmas as a menu of trade-off options.
var _dec_layer: CanvasLayer
var _dec_title: Label
var _dec_opts: VBoxContainer
var _dec_shown := ""                          # title currently rendered (rebuild on change)
var _dilemma_lock := false                    # hard pause until a stacked dilemma menu is cleared

# Fleet view.
var _fleet_grid: GridContainer
var _fleet_count: Label
var _corp_lbl: Label
var corp_name_idx := 0
var save_slot := 0                           # active manual slot, 0..SAVE_SLOTS-1 (§30)
var ironman := false                         # Ironman: autosave, no manual reloads
var _autosave_accum := 0.0
var combat_band := 1                         # 0 close · 1 medium · 2 long (§9)
var _combat_lbl: Label                       # doctrine readout in the FLEET view
const DIO_STEP := 0.22                        # seconds between revealed BattleLog beats
# Diorama (§22): plays the last battle's BattleLog beat by beat.
var _diorama: CanvasLayer
var _dio_title: Label
var _dio_sub: Label
var _dio_log: RichTextLabel
var _dio_force_a: RichTextLabel   # player force roster (depletes as kills play)
var _dio_force_b: RichTextLabel   # raider force roster
var _dio_surv := [0, 0]           # live surviving counts during playback
var _dio_start := [0, 0]          # starting counts (pip denominators)
var _dio_idx := 0
var _dio_timer := 0.0
var _dio_playing := false
var _battle3d: BattleDiorama          # the 3D combat scene behind the log (§22)
var _fleet_tabs: Array[Button] = []

# Build view.
var _build_list: VBoxContainer
var _commission_btn: Button                   # greyed when the selected hull can't be sourced
var _build_caption: Label
var _build_stats: Label
var _build_cost: Label
var _bom_lbl: Label
var _build_queue: VBoxContainer
var _ship_pivot: Node3D
# Ship designer (A2): the player's draft loadout for the selected class.
var _des_pdc := 0
var _des_torp := 0
var _des_rail := 0
var _des_burn := 100        # remass load, percent of tankage
var _des_vals := {}         # kind -> value Label (updated each refresh)
var _des_model_i := {0: 0, 1: 0, 2: 0}   # kind -> index into owned models (per-slot pick)
var _des_model_lbl := {}    # kind -> model-name Label
var _refit_target := 0      # fleet index targeted by the refit bay
var _refit_model_i := {0: 0, 1: 0, 2: 0}  # refit-bay model choice per kind
var _refit_lbl := {}        # "target"/0/1/2 -> Label for the refit bay
var _design_lbl: Label
var _arsenal_box: VBoxContainer               # weapon-crafting list (Phase B)
var _scrap_lbl: Label
var _yard_lbl: Label                          # shipyard status (Phase B+)
var _arsenal_sig := ""                         # rebuild the list only when it changes

# Market view.
var _flow: Control
var _chart: Control
var _ticker_grid: GridContainer
var _ticker_title: Label
var _chart_title: Label
var _mkt_sel_lbl: Label
var _mkt_routes_lbl: Label
var _chart_legend: VBoxContainer

# ---- 3D world ---------------------------------------------------------------
var _orrery_root: Node3D
var _cam: Camera3D
var _body_nodes: Array[Node3D] = []
var _body_spin: Array[Node3D] = []          # the spinning surface mesh per body (or null)
var _body_spin_rate: PackedFloat32Array = []  # axial-spin rate (rad/s) per body
var _body_labels: Array[Label3D] = []        # the billboard name tag per body (for LOD)
var _planet_orbit_rings: Array = []          # {tm:TorusMesh, r:float} — zoom-scaled tube
var _moon_orbit_rings: Array = []            # {mi:MeshInstance3D, tm:TorusMesh, r:float}
var _station_markers: Array[Node3D] = []     # colony/station glyphs (LOD-toggled)
var _station_labels: Array[Label3D] = []     # colony/station name tags (LOD-toggled)
var _gate_tm: TorusMesh                       # the gate ring mesh (zoom-scaled tube)
var _gate_r := 40.0
var _map_font: Font                           # the Protomolecule typeface for orrery labels (j)
var _hauler_pool: Array[MeshInstance3D] = []
var _faction_haul_mats: Array[StandardMaterial3D] = []   # per-faction hauler livery (§4)
var _ship_pool: Array[MeshInstance3D] = []     # §6 player warships on the map
var _freighter_pool: Array[MeshInstance3D] = []  # §6 player freighters on the lanes
var _wreck_pool: Array[MeshInstance3D] = []
var _miner_pool: Array[MeshInstance3D] = []   # deployed miners on the orrery
var _gate_ring: MeshInstance3D
var _lane_mesh: ImmediateMesh
var _hauler_mat: StandardMaterial3D
var _ship_mat: StandardMaterial3D
var _freighter_mat: StandardMaterial3D
var _select_mat: StandardMaterial3D
var _wreck_mat: StandardMaterial3D
var _miner_mat: StandardMaterial3D
var _gate_mat: StandardMaterial3D
const FREIGHTER_COL := Color(0.45, 0.78, 0.62)  # muted green — player logistics wing


func _ready() -> void:
	RenderingServer.set_debug_generate_wireframes(true)   # for the BUILD blueprint
	sim = TorchSim.new()
	sim.reset(7)
	shipyard = TorchShipyard.new()
	_resolve_commodity_indices()
	_build_world()
	_build_chrome()
	_build_systems_view()
	_build_fleet_view()
	_build_build_view()
	_build_market_view()
	_build_empire_view()
	_build_research_view()
	_build_ledger_view()
	_build_diplomacy_view()
	_build_diorama()
	_build_decision_panel()
	_select_view(V_SYSTEMS)
	# Open centred + zoomed in on the home station (your starting market), not the wide
	# system view — the player should see *where they are* first (§0.3).
	_focus_body = sim.market_body(0)
	_zoom = 2.2
	# Default to PC mode on desktop, touch on a handheld (§33). Either can be forced
	# with F8 — handy for testing the desktop layout on a dev machine.
	_set_pc_mode(OS.has_feature("pc"))


## Switch between desktop (mouse + keyboard) and touch (pinch + on-screen buttons)
## control schemes. On PC, the window is resizable and the touch-only zoom buttons
## give way to the mouse wheel; on touch they return.
func _set_pc_mode(on: bool) -> void:
	pc_mode = on
	if _map_controls:
		_map_controls.visible = not on
	# Magnify the whole HUD for legibility — bigger on a handheld than a desktop monitor (§33).
	get_window().content_scale_factor = UI_SCALE_PC if on else UI_SCALE_TOUCH
	if on:
		# Desktop: open in TRUE fullscreen by default. A tiling WM (niri/sway) ignores an
		# xdg "maximize" request and leaves the window at its small requested size — which
		# also shrinks the canvas_items stretch below 1.0, making every font tiny. Real
		# fullscreen is honoured everywhere. F11 drops to a maximized window. Cursor shown.
		var win := get_window()
		win.mode = Window.MODE_FULLSCREEN if _fullscreen else Window.MODE_MAXIMIZED
		win.title = "TORCH"
		Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	status = "PC mode — wheel zooms · drag to pan · Shift-drag to rotate · F11 window/fullscreen." if on \
		else "Touch mode — pinch to zoom, tap a world to focus."


## Toggle between true (borderless) fullscreen — the PC default — and a maximized window.
func _toggle_fullscreen() -> void:
	_fullscreen = not _fullscreen
	var win := get_window()
	win.mode = Window.MODE_FULLSCREEN if _fullscreen else Window.MODE_MAXIMIZED


func _resolve_commodity_indices() -> void:
	for c in sim.commodity_count():
		var n := String(sim.commodity_name(c)).to_lower()
		if n.contains("ore"):
			_idx_ore = c
		elif n.contains("fuel"):
			_idx_fuel = c
		elif n.contains("water") or n.contains("ice"):
			_idx_water = c


# ============================================================================
# 3D ORRERY WORLD
# ============================================================================

func _build_world() -> void:
	# The Protomolecule typeface (The Expanse fan font) for the orrery labels (j).
	if ResourceLoader.exists("res://assets/fonts/Protomolecule.ttf"):
		_map_font = load("res://assets/fonts/Protomolecule.ttf")
	var env := WorldEnvironment.new()
	var e := Environment.new()
	# A procedural deep-space sky (stars + Milky Way + nebulae) at infinity (§21).
	e.background_mode = Environment.BG_SKY
	e.sky = PlanetShaders.space_sky()
	# Bodies are lit in-shader from Sol at the origin (§17), so the scene needs no
	# engine lights; a faint ambient just keeps the unlit far rims from going pure black.
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color(0.18, 0.22, 0.32)
	e.ambient_light_energy = 0.18
	# Bloom so the sun, atmospheres and city lights glow (HDR ALBEDO > 1 → glow).
	e.glow_enabled = true
	e.glow_intensity = 0.55
	e.glow_strength = 1.05
	e.glow_bloom = 0.18
	e.glow_hdr_threshold = 1.0
	e.glow_blend_mode = Environment.GLOW_BLEND_MODE_ADDITIVE
	env.environment = e
	add_child(env)

	_cam = Camera3D.new()
	_cam.current = true
	_cam.far = 6000.0
	add_child(_cam)
	_update_camera()

	_orrery_root = Node3D.new()
	add_child(_orrery_root)

	_hauler_mat = _emissive_mat(HAULER_COL)
	# Faction-liveried hauler markers (§4/§24): NPC traffic reads by its owner's colour.
	for fc in FACTION_COL:
		_faction_haul_mats.append(_emissive_mat((fc as Color).lerp(HAULER_COL, 0.35)))
	_ship_mat = _emissive_mat(sim.corp_livery_color())   # player warships fly the livery (§14)
	_freighter_mat = _emissive_mat(FREIGHTER_COL)        # player freighters on the lanes (§6)
	_select_mat = _emissive_mat(SELECT_COL)
	_wreck_mat = _emissive_mat(Color(0.45, 0.85, 0.85))
	_miner_mat = _emissive_mat(Color(0.95, 0.6, 0.18))   # industrial amber

	var gate_r := 40.0
	for b in sim.body_count():
		var kind := sim.body_kind(b)
		var name := String(sim.body_name(b))
		if kind == 5:   # the ring-gate is drawn as a glowing torus, not a sphere
			gate_r = _world3d(sim.body_x(b), sim.body_y(b)).length()
			var ph := Node3D.new()
			_orrery_root.add_child(ph)
			# Label the always-visible ring-gate (§0 destination) so the golden ring out
			# past the Kuiper edge reads as *the gate*, not a mystery orbit. The placeholder
			# node tracks the gate's world position each frame, so the tag rides with it.
			var gtag := Label3D.new()
			gtag.text = "⟁ " + name
			gtag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
			gtag.fixed_size = true
			if _map_font != null:
				gtag.font = _map_font
			gtag.modulate = Color(0.98, 0.86, 0.45)
			gtag.pixel_size = LABEL_PIXEL
			gtag.position = Vector3(0.0, 0.7, 0.0)
			ph.add_child(gtag)
			_body_nodes.append(ph)
			_body_labels.append(gtag)
			_body_spin.append(null)
			_body_spin_rate.append(0.0)
			continue
		var container := _spawn_body(b, name, kind)
		if sim.body_is_far_side(b):
			container.visible = false   # revealed only after the gate is transited (§17)
		_orrery_root.add_child(container)
		_body_nodes.append(container)
		# Orbit rings: planets/dwarfs trace the ecliptic; moons ring their planet.
		# Asteroids and the star carry no ring (the Belt reads as a field, not a wheel).
		var parent := sim.body_parent(b)
		if kind != 0 and kind != 7:
			if parent == 0:
				if not sim.body_is_far_side(b):
					var r := _world3d(sim.body_x(b), sim.body_y(b)).length()
					var pr := _ring(r, Color(0.24, 0.33, 0.48))
					_orrery_root.add_child(pr)
					_planet_orbit_rings.append({"tm": pr.mesh, "r": r})
			else:
				var mr: float = float(sim.body_orbit_radius(b)) * SCALE3D
				# Moon orbit: a hair-thin, faintly glowing line around its planet.
				var mrm := _emissive_mat(Color(0.3, 0.38, 0.5) * 2.0)
				var mring := _ring_mat(mr, mrm, maxf(0.0022, mr * 0.006))
				_body_nodes[parent].add_child(mring)
				_moon_orbit_rings.append({"mi": mring, "tm": mring.mesh, "r": mr})
		var rad := _display_radius(name, kind)
		var tag := Label3D.new()
		tag.text = name
		tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		tag.fixed_size = true   # constant on-screen size — never balloons when zoomed in (i)
		if _map_font != null:
			tag.font = _map_font
		tag.modulate = Color(0.6, 0.7, 0.78) if (kind == 4 or kind == 7) else Color(0.72, 0.84, 0.95)
		tag.pixel_size = LABEL_PIXEL_SMALL if (kind == 4 or kind == 7) else LABEL_PIXEL
		tag.position = Vector3(0, rad + 0.05, 0)
		container.add_child(tag)
		_body_labels.append(tag)

	_gate_mat = _emissive_mat(Color(0.9, 0.78, 0.35))
	_gate_mat.albedo_color = Color(0.95, 0.82, 0.4) * 1.6   # glow through bloom
	_gate_ring = _ring_mat(gate_r, _gate_mat, 0.12)
	_gate_tm = _gate_ring.mesh
	_gate_r = gate_r
	_orrery_root.add_child(_gate_ring)

	for ci in sim.colony_count():
		var cb := sim.colony_body(ci)
		if cb < 0 or cb >= _body_nodes.size():
			continue
		var fcol: Color = FACTION_COL[clampi(sim.colony_faction(ci), 0, 3)]
		var crad := _display_radius(String(sim.body_name(cb)), sim.body_kind(cb))
		var marker := _station_glyph(fcol)
		marker.position = Vector3(crad + 0.03, 0.0, 0.0)
		_body_nodes[cb].add_child(marker)
		_station_markers.append(marker)
		var clbl := Label3D.new()
		clbl.text = sim.colony_name(ci)
		clbl.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		clbl.fixed_size = true
		if _map_font != null:
			clbl.font = _map_font
		clbl.modulate = fcol
		clbl.pixel_size = LABEL_PIXEL_SMALL
		clbl.position = Vector3(0.0, -crad - 0.06, 0.0)
		_body_nodes[cb].add_child(clbl)
		_station_labels.append(clbl)

	for b in sim.body_count():
		if sim.body_name(b) == "Saturn":
			_build_saturn_rings(_body_spin[b])   # parent to the tilted surface
			break

	_build_asteroid_belt()

	_lane_mesh = ImmediateMesh.new()
	var lanes := MeshInstance3D.new()
	lanes.mesh = _lane_mesh
	var lane_mat := _emissive_mat(Color(0.85, 0.6, 0.35))
	lane_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	lane_mat.albedo_color = Color(0.85, 0.6, 0.35, 0.4)
	lanes.material_override = lane_mat
	_orrery_root.add_child(lanes)


## Build one celestial body: a positioned container holding a tilted, spinning,
## procedurally-shaded surface sphere (§17) plus an atmospheric glow shell where
## the world has an atmosphere. Records the surface + spin rate for the frame loop.
func _spawn_body(b: int, name: String, kind: int) -> Node3D:
	var container := Node3D.new()
	var rad := _display_radius(name, kind)
	var surf := _sphere(rad, _make_body_material(name, kind))
	# Lean the spin axis (axial tilt) — Uranus rolls on its side, Earth a gentle 23°.
	surf.rotation_degrees = Vector3(0.0, 0.0, _axial_tilt(name))
	# Tune sphere resolution to apparent size (cheap for tiny moons/rocks).
	var sm := surf.mesh as SphereMesh
	var segs := 12
	if rad > 0.25: segs = 40
	elif rad > 0.09: segs = 28
	elif rad > 0.035: segs = 18
	sm.radial_segments = segs
	sm.rings = maxi(6, segs / 2)
	container.add_child(surf)
	_body_spin.append(surf)
	_body_spin_rate.append(_spin_rate(name, kind))
	var atmo := _atmosphere_for(name, kind, rad)
	if atmo != null:
		container.add_child(atmo)
	return container


## Numerous small bodies filling the asteroid belt (§17) — a deterministic ring of
## tumbling rocks between Mars and Jupiter, so the Belt looks inhabited, not empty.
func _build_asteroid_belt() -> void:
	var amat := PlanetShaders.rocky(Color(0.42, 0.36, 0.3), Color(0.2, 0.17, 0.14), 0.9, 0.0, Color.WHITE)
	var rock := SphereMesh.new()
	rock.radius = 0.012
	rock.height = 0.024
	rock.radial_segments = 6
	rock.rings = 4
	rock.material = amat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = rock
	mm.instance_count = 1400
	var rng := RandomNumberGenerator.new()
	rng.seed = 4242
	for i in mm.instance_count:
		var ang := rng.randf() * TAU
		var r := rng.randf_range(2.05, 3.35)   # AU = world units
		var y := rng.randf_range(-0.06, 0.06) * r
		var pos := Vector3(cos(ang) * r, y, -sin(ang) * r)
		var s := rng.randf_range(0.4, 2.6)
		var basis := Basis(Vector3(rng.randf(), rng.randf(), rng.randf()).normalized(), rng.randf() * TAU).scaled(Vector3.ONE * s)
		mm.set_instance_transform(i, Transform3D(basis, pos))
	var mmi := MultiMeshInstance3D.new()
	mmi.multimesh = mm
	_orrery_root.add_child(mmi)


# ---- body appearance specs (§17/§24) ---------------------------------------

# Apparent display radii (world units). Not true scale — at 1 AU = 1 unit the real
# bodies would be invisible specks — but the *relative* sizes are honest (Jupiter
# dwarfs the terrestrials, moons are tiny), so the system reads more to scale (§21).
# Sized so the inner orbits clear the sun: Mercury orbits at 0.387 units, so the
# sun stays well under that (0.26) while still reading as the largest body.
const _RADII := {
	"Sol": 0.26,
	"Mercury": 0.026, "Venus": 0.048, "Earth": 0.05, "Mars": 0.034,
	# Gas giants scaled down so their innermost moons clear the body instead of clipping
	# inside it (Jupiter's Metis orbits at 0.150, Saturn's Pan at 0.110 world units, etc.).
	"Jupiter": 0.125, "Saturn": 0.09, "Uranus": 0.082, "Neptune": 0.08,
	"Ceres": 0.02, "Pluto": 0.02,
	"Ganymede": 0.026, "Titan": 0.026, "Callisto": 0.024, "Io": 0.02, "Europa": 0.018,
	"Luna": 0.022, "Triton": 0.02, "Titania": 0.016, "Oberon": 0.015,
	"Rhea": 0.014, "Iapetus": 0.014, "Vesta": 0.014, "Pallas": 0.013,
}
# Icy/bright moons (and Pluto-likes) — share the rocky shader tuned pale and ice-rich.
const _ICY := ["Europa", "Enceladus", "Tethys", "Dione", "Mimas", "Rhea", "Iapetus",
	"Ganymede", "Callisto", "Triton", "Charon", "Miranda", "Ariel", "Umbriel",
	"Titania", "Oberon", "Hydra", "Nix"]


func _display_radius(name: String, kind: int) -> float:
	if _RADII.has(name):
		return float(_RADII[name])
	match kind:
		0: return 0.26
		1: return 0.038
		2: return 0.14
		3: return 0.02
		4: return 0.013
		6: return 0.12
		7: return 0.013
	return 0.03


func _make_body_material(name: String, kind: int) -> ShaderMaterial:
	match name:
		"Sol":
			return PlanetShaders.sun()
		"Earth":
			return PlanetShaders.earth()
		"Venus":
			return PlanetShaders.venus()
		"Jupiter":
			return PlanetShaders.gas_giant(Color(0.74, 0.58, 0.4), Color(0.92, 0.84, 0.68),
				Color(0.58, 0.4, 0.28), Color(0.72, 0.66, 0.56), 1.0, Color(0.8, 0.32, 0.22))
		"Saturn":
			return PlanetShaders.gas_giant(Color(0.84, 0.74, 0.52), Color(0.94, 0.88, 0.68),
				Color(0.76, 0.64, 0.44), Color(0.82, 0.76, 0.6), 0.0, Color.WHITE)
		"Uranus":
			return PlanetShaders.gas_giant(Color(0.52, 0.82, 0.83), Color(0.72, 0.92, 0.92),
				Color(0.42, 0.7, 0.76), Color(0.6, 0.82, 0.84), 0.0, Color.WHITE)
		"Neptune":
			return PlanetShaders.gas_giant(Color(0.18, 0.34, 0.78), Color(0.34, 0.52, 0.92),
				Color(0.13, 0.26, 0.6), Color(0.28, 0.44, 0.8), 1.0, Color(0.16, 0.28, 0.66))
		"Mars":
			return PlanetShaders.rocky(Color(0.74, 0.36, 0.18), Color(0.44, 0.2, 0.12), 0.5, 0.11, Color(0.93, 0.94, 0.96))
		"Mercury":
			return PlanetShaders.rocky(Color(0.55, 0.5, 0.46), Color(0.3, 0.28, 0.26), 0.95, 0.0, Color.WHITE)
		"Luna":
			return PlanetShaders.rocky(Color(0.62, 0.62, 0.63), Color(0.3, 0.3, 0.32), 0.9, 0.0, Color.WHITE)
		"Titan":
			return PlanetShaders.rocky(Color(0.82, 0.56, 0.2), Color(0.52, 0.34, 0.12), 0.15, 0.0, Color.WHITE)
		"Pluto":
			return PlanetShaders.rocky(Color(0.72, 0.62, 0.52), Color(0.45, 0.38, 0.32), 0.45, 0.28, Color(0.86, 0.83, 0.78))
		"Ceres":
			return PlanetShaders.rocky(Color(0.5, 0.48, 0.46), Color(0.29, 0.28, 0.27), 0.75, 0.06, Color(0.72, 0.74, 0.76))
	if _ICY.has(name):
		return PlanetShaders.rocky(Color(0.82, 0.86, 0.92), Color(0.5, 0.58, 0.68), 0.55, 0.0, Color.WHITE)
	match kind:
		2:
			return PlanetShaders.gas_giant(Color(0.7, 0.62, 0.5), Color(0.86, 0.8, 0.66),
				Color(0.55, 0.46, 0.36), Color(0.7, 0.66, 0.58), 0.0, Color.WHITE)
		3:
			return PlanetShaders.rocky(Color(0.66, 0.6, 0.54), Color(0.4, 0.36, 0.32), 0.6, 0.15, Color(0.82, 0.82, 0.8))
		6:
			return PlanetShaders.rocky(Color(0.5, 0.4, 0.62), Color(0.24, 0.18, 0.34), 0.7, 0.0, Color.WHITE)
		7:
			return PlanetShaders.rocky(Color(0.46, 0.4, 0.34), Color(0.22, 0.19, 0.16), 0.95, 0.0, Color.WHITE)
	return PlanetShaders.rocky(Color(0.6, 0.58, 0.55), Color(0.34, 0.33, 0.31), 0.7, 0.0, Color.WHITE)


func _axial_tilt(name: String) -> float:
	match name:
		"Earth": return 23.0
		"Mars": return 25.0
		"Saturn": return 27.0
		"Neptune": return 28.0
		"Jupiter": return 3.0
		"Uranus": return 92.0   # rolls on its side
		"Pluto": return 57.0
		"Mercury": return 2.0
		"Venus": return 3.0
	return 8.0


func _spin_rate(name: String, kind: int) -> float:
	match kind:
		0: return 0.0     # the sun's surface is animated in-shader
		2: return 0.5     # gas giants whirl
		1: return 0.13
		3: return 0.10
		4: return 0.08
		7: return 0.35    # rubble-pile asteroids tumble
		6: return 0.10
	return 0.10


## A thin additive atmospheric-glow shell around bodies with an atmosphere, or null.
func _atmosphere_for(name: String, kind: int, rad: float) -> MeshInstance3D:
	var col := Color.BLACK
	var inten := 0.0
	match name:
		"Earth": col = Color(0.35, 0.6, 1.0); inten = 1.5
		"Venus": col = Color(0.96, 0.86, 0.56); inten = 1.7
		"Mars": col = Color(0.86, 0.55, 0.4); inten = 0.5
		"Titan": col = Color(0.86, 0.56, 0.2); inten = 1.3
		"Jupiter": col = Color(0.86, 0.72, 0.56); inten = 0.9
		"Saturn": col = Color(0.92, 0.84, 0.62); inten = 0.8
		"Uranus": col = Color(0.6, 0.86, 0.9); inten = 0.9
		"Neptune": col = Color(0.32, 0.52, 0.96); inten = 1.0
		"Pluto": col = Color(0.72, 0.76, 0.86); inten = 0.35
		_:
			if kind == 2:
				col = Color(0.72, 0.76, 0.86); inten = 0.7
			else:
				return null
	var shell := _sphere(rad * (1.05 if kind == 2 else 1.07), PlanetShaders.atmosphere(col, inten))
	var sm := shell.mesh as SphereMesh
	sm.radial_segments = 28
	sm.rings = 14
	return shell


func _build_saturn_rings(saturn: Node3D) -> void:
	# Ring extent scales with Saturn's display size (~1.2–2.35 planet radii).
	var R := _display_radius("Saturn", 2)
	var r_in := R * 1.2
	var r_out := R * 2.35
	# The main ring sheet — a flat, banded, Sol-lit annulus (parented to the tilted
	# planet so the rings sit on Saturn's equator).
	var ring := MeshInstance3D.new()
	ring.mesh = _flat_ring_mesh(r_in, r_out, 120)
	ring.material_override = PlanetShaders.rings(r_in, r_out, Color(1.0, 0.95, 0.85))
	saturn.add_child(ring)
	# A scatter of ring particles for a touch of depth/sparkle.
	var amat := PlanetShaders.rocky(Color(0.82, 0.76, 0.62), Color(0.5, 0.46, 0.38), 0.4, 0.0, Color.WHITE)
	var rock := SphereMesh.new()
	rock.radius = 0.004
	rock.height = 0.008
	rock.radial_segments = 5
	rock.rings = 3
	rock.material = amat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = rock
	mm.instance_count = 300
	var rng := RandomNumberGenerator.new()
	rng.seed = 17
	for i in mm.instance_count:
		var ang := rng.randf() * TAU
		var rad := rng.randf_range(r_in, r_out)
		var pos := Vector3(cos(ang) * rad, rng.randf_range(-0.005, 0.005), sin(ang) * rad)
		var s := rng.randf_range(0.5, 2.0)
		var basis := Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * s)
		mm.set_instance_transform(i, Transform3D(basis, pos))
	var ast := MultiMeshInstance3D.new()
	ast.multimesh = mm
	saturn.add_child(ast)


## A flat horizontal annulus (ring of triangles in the XZ plane) for planetary rings.
func _flat_ring_mesh(inner: float, outer: float, segs: int) -> ArrayMesh:
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	for i in segs:
		var a0 := TAU * float(i) / float(segs)
		var a1 := TAU * float(i + 1) / float(segs)
		var ci := Vector3(cos(a0) * inner, 0.0, sin(a0) * inner)
		var co := Vector3(cos(a0) * outer, 0.0, sin(a0) * outer)
		var ni := Vector3(cos(a1) * inner, 0.0, sin(a1) * inner)
		var no := Vector3(cos(a1) * outer, 0.0, sin(a1) * outer)
		for v in [ci, co, no, ci, no, ni]:
			st.set_normal(Vector3.UP)
			st.add_vertex(v)
	return st.commit()


# ============================================================================
# CHROME (bezel · top bar · nav rail · content host)
# ============================================================================

func _build_chrome() -> void:
	_layer = CanvasLayer.new()
	add_child(_layer)

	# Outer bezel — border only, so the orrery shows through inside it.
	var bezel := Panel.new()
	bezel.set_anchors_preset(Control.PRESET_FULL_RECT)
	var bsb := UiKit.panel_box(Color(0, 0, 0, 0), UiKit.LINE_HI, 14, 1)
	bezel.add_theme_stylebox_override("panel", bsb)
	bezel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_layer.add_child(bezel)

	# Left nav rail (solid chrome).
	var rail := Panel.new()
	rail.add_theme_stylebox_override("panel", UiKit.panel_box(UiKit.BG_BAR, UiKit.LINE, 12))
	_fill(rail, 8, 8, -1, 8)
	rail.anchor_right = 0
	rail.offset_right = 70
	rail.mouse_filter = Control.MOUSE_FILTER_STOP
	_layer.add_child(rail)
	var rail_v := VBoxContainer.new()
	rail_v.add_theme_constant_override("separation", 6)
	_fill(rail_v, 4, 8, 4, 8)
	rail.add_child(rail_v)
	# Brand flame mark at the top of the rail.
	var mark := UiKit.label("◆", 22, UiKit.ACCENT)
	mark.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	rail_v.add_child(mark)
	rail_v.add_child(UiKit.rule())
	for v in VIEW_CAP.size():
		var b := UiKit.nav_button(VIEW_GLYPH[v], VIEW_CAP[v], v == view)
		var vi := v
		b.pressed.connect(func() -> void: _select_view(vi))
		rail_v.add_child(b)
		_nav_buttons.append(b)

	# Top status bar.
	var bar := Panel.new()
	bar.add_theme_stylebox_override("panel", UiKit.panel_box(UiKit.BG_BAR, UiKit.LINE, 12))
	bar.set_anchors_preset(Control.PRESET_FULL_RECT)
	bar.anchor_bottom = 0
	bar.offset_left = 86
	bar.offset_top = 8
	bar.offset_right = -8
	bar.offset_bottom = 48
	_layer.add_child(bar)
	# Left: brand + view title.
	var brand := UiKit.label("TORCH", 18, UiKit.TEXT_HI)
	brand.position = Vector2(14, 8)
	bar.add_child(brand)
	# Pause / play controls (touch-first — the handheld replacement for the spacebar).
	var pp := HBoxContainer.new()
	pp.add_theme_constant_override("separation", 4)
	pp.position = Vector2(96, 0)
	bar.add_child(pp)
	pp.add_child(_make_map_button("⏸", func(): speed_idx = 0))
	pp.add_child(_make_map_button("▶", _play_step))
	_title = UiKit.label("", 13, UiKit.TEXT_DIM)
	_title.position = Vector2(190, 12)
	bar.add_child(_title)
	# Centre: an alert ticker (latest act-now) — visible in every view.
	_alert_ticker = UiKit.label("", 12, UiKit.BAD)
	_alert_ticker.position = Vector2(320, 12)
	bar.add_child(_alert_ticker)
	# Right: resource readouts, pinned to the right edge.
	var res := HBoxContainer.new()
	res.add_theme_constant_override("separation", 18)
	res.alignment = BoxContainer.ALIGNMENT_END
	res.set_anchors_preset(Control.PRESET_FULL_RECT)
	res.offset_left = 360
	res.offset_right = -14
	res.offset_top = 0
	res.offset_bottom = 0
	res.mouse_filter = Control.MOUSE_FILTER_IGNORE
	bar.add_child(res)
	_date = UiKit.label("", 13, UiKit.TEXT_DIM)
	res.add_child(_make_res_cell("DATE", _date))
	var cc := _make_rated_cell("CREDITS", UiKit.GOLD)
	res.add_child(cc[0]); _res_credits = cc[1]; _rate_credits = cc[2]
	var co := _make_rated_cell("ORE", UiKit.TEXT)
	res.add_child(co[0]); _res_ore = co[1]; _rate_ore = co[2]
	var cf := _make_rated_cell("FUEL", UiKit.TEXT)
	res.add_child(cf[0]); _res_fuel = cf[1]; _rate_fuel = cf[2]
	var ce := _make_rated_cell("CREW", UiKit.TEXT)
	res.add_child(ce[0]); _res_crew = ce[1]
	var ci := _make_rated_cell("INFLUENCE", Color(0.85, 0.72, 0.4))
	res.add_child(ci[0]); _res_influence = ci[1]; _rate_influence = ci[2]

	# Content host (between the rail and the screen edge, below the bar). IGNORE so that
	# taps/drags/pinches over the *map* (the 3D orrery behind it) reach `_unhandled_input`;
	# the interactive panels inside still catch their own input (they default to STOP).
	_content = Control.new()
	_content.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_fill(_content, 86, 54, 8, 8)
	_layer.add_child(_content)

	# Full-screen juice washes + paused banner sit above the content.
	_flash_rect = _make_wash(Color(1.0, 0.3, 0.22, 0.0))
	_ascend_rect = _make_wash(Color(1.0, 0.82, 0.3, 0.0))
	_layer.add_child(_flash_rect)
	_layer.add_child(_ascend_rect)
	_help = UiKit.label("", 9, UiKit.TEXT_DIM)
	_help.set_anchors_preset(Control.PRESET_FULL_RECT)
	_help.anchor_top = 1
	_help.offset_top = -16
	_help.offset_left = 92
	_help.offset_bottom = -2
	_layer.add_child(_help)


## A resource cell with a caption, a big value, and a small per-day rate sublabel
## (Stellaris-style). Returns [cell, value_label, rate_label].
func _make_rated_cell(caption: String, vcolor: Color) -> Array:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 0)
	v.alignment = BoxContainer.ALIGNMENT_CENTER
	v.add_child(UiKit.kicker(caption))
	var val := UiKit.label("", 14, vcolor)
	v.add_child(val)
	var rate := UiKit.label("", 9, UiKit.TEXT_DIM)
	v.add_child(rate)
	return [v, val, rate]


func _make_res_cell(caption: String, value: Label) -> VBoxContainer:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 0)
	v.alignment = BoxContainer.ALIGNMENT_CENTER
	v.add_child(UiKit.kicker(caption))
	v.add_child(value)
	return v


## Anchor a control to fill its parent with edge insets (responsive for the
## `canvas_items`/expand stretch the project uses, §17 mobile).
func _fill(c: Control, l: float, t: float, r: float, b: float) -> void:
	c.set_anchors_preset(Control.PRESET_FULL_RECT)
	c.offset_left = l
	c.offset_top = t
	c.offset_right = -r
	c.offset_bottom = -b


func _make_wash(col: Color) -> ColorRect:
	var cr := ColorRect.new()
	cr.color = col
	cr.set_anchors_preset(Control.PRESET_FULL_RECT)
	cr.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return cr


func _select_view(v: int) -> void:
	view = v
	for i in _views.size():
		_views[i].visible = i == v
	for i in _nav_buttons.size():
		_nav_buttons[i].set_pressed_no_signal(i == v)
	# The orrery only renders behind the SYSTEMS view.
	_orrery_root.visible = v == V_SYSTEMS


# ============================================================================
# SYSTEMS VIEW (orrery context panel + goal/feed overlay + map controls)
# ============================================================================

func _build_systems_view() -> void:
	var root := Control.new()
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_content.add_child(root)
	_views.append(root)

	# Right context panel (station detail), pinned to the right of the content.
	var ctx := UiKit.make_panel()
	ctx.set_anchors_preset(Control.PRESET_FULL_RECT)
	ctx.anchor_left = 1
	ctx.offset_left = -312
	ctx.offset_right = 0
	ctx.offset_top = 0
	ctx.offset_bottom = -132
	root.add_child(ctx)
	var col := VBoxContainer.new()
	col.add_theme_constant_override("separation", 6)
	ctx.add_child(col)
	_sys_title = UiKit.label("", 16, UiKit.TEXT_HI)
	col.add_child(_sys_title)
	_sys_sub = UiKit.label("", 11, UiKit.TEXT_DIM)
	col.add_child(_sys_sub)
	# The tapped object's contextual detail (mineral yield / miner / contested influence /
	# colony development) — the object is the centre of the panel, not just the market.
	_sys_object = RichTextLabel.new()
	_sys_object.bbcode_enabled = true
	_sys_object.fit_content = true
	_sys_object.scroll_active = false
	_sys_object.add_theme_font_size_override("normal_font_size", 12)
	_sys_object.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_sys_object.visible = false
	col.add_child(_sys_object)
	col.add_child(UiKit.rule())
	_sys_status = UiKit.label("", 12, UiKit.TEXT)
	col.add_child(_sys_status)
	col.add_child(UiKit.kicker("Resources"))
	_sys_resources = VBoxContainer.new()
	_sys_resources.add_theme_constant_override("separation", 3)
	col.add_child(_sys_resources)
	col.add_child(UiKit.kicker("Active Construction Queues"))
	_sys_queues = VBoxContainer.new()
	_sys_queues.add_theme_constant_override("separation", 4)
	col.add_child(_sys_queues)
	col.add_child(UiKit.kicker("Standing Orders"))
	_tg_patrol = _add_toggle(col, "Interdiction patrol", func(on): sim.toggle_patrol())
	_tg_research = _add_toggle(col, "Auto-research", func(on): sim.toggle_auto_research())
	_tg_pause = _add_toggle(col, "Auto-pause on alert", func(on): auto_pause = on)
	_add_toggle(col, "Ironman (autosave, no reloads)", _toggle_ironman)
	_make_draggable(ctx, col)

	# Bottom-left overlay panel: NOW goal + the active mission + the gate mystery +
	# the always-visible gate progress (§0.1 — the authored destination pull, §16).
	var goal := UiKit.make_panel(UiKit.BG_PANEL, UiKit.LINE, 8)
	goal.set_anchors_preset(Control.PRESET_FULL_RECT)
	goal.anchor_top = 1
	goal.offset_top = -214
	goal.offset_left = 0
	goal.offset_right = 360
	goal.offset_bottom = 0
	root.add_child(goal)
	var gv := VBoxContainer.new()
	gv.add_theme_constant_override("separation", 4)
	goal.add_child(gv)
	# System summary (mockup's "Sol System" card) — a one-line census of what's out there.
	gv.add_child(UiKit.kicker("Sol System  ·  Home System"))
	_sys_census = UiKit.label("", 11, UiKit.TEXT_DIM)
	gv.add_child(_sys_census)
	gv.add_child(UiKit.rule())
	gv.add_child(UiKit.kicker("Objective"))
	_sys_mission = UiKit.label("", 12, UiKit.ACCENT)
	_sys_mission.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_sys_mission.custom_minimum_size = Vector2(338, 0)
	gv.add_child(_sys_mission)
	_sys_now = UiKit.label("", 11, UiKit.TEXT_DIM)
	_sys_now.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_sys_now.custom_minimum_size = Vector2(338, 0)
	gv.add_child(_sys_now)
	_make_draggable(goal, gv)

	# Alert feed panel, bottom-centre over the orrery.
	var feedp := UiKit.make_panel(UiKit.BG_PANEL, UiKit.LINE, 8)
	feedp.set_anchors_preset(Control.PRESET_FULL_RECT)
	feedp.anchor_top = 1
	feedp.offset_top = -124
	feedp.offset_left = 370
	feedp.offset_right = -322
	feedp.offset_bottom = 0
	root.add_child(feedp)
	var fv := VBoxContainer.new()
	feedp.add_child(fv)
	fv.add_child(UiKit.kicker("Alert Feed"))
	_feed = RichTextLabel.new()
	_feed.bbcode_enabled = true
	_feed.scroll_active = false
	_feed.fit_content = true
	_feed.add_theme_font_size_override("normal_font_size", 12)
	_feed.add_theme_font_size_override("bold_font_size", 12)
	_feed.mouse_filter = Control.MOUSE_FILTER_IGNORE
	fv.add_child(_feed)
	_make_draggable(feedp, fv)

	# Contextual actions — no persistent button grid; only the verbs relevant to what you
	# tapped appear here (and the conditional endgame verbs), bottom-right over the orrery.
	# Map zoom/rotate is fingers-only now (pinch / drag / twist); save is autosave (Ironman).
	var fo := VBoxContainer.new()
	fo.add_theme_constant_override("separation", 5)
	fo.set_anchors_preset(Control.PRESET_FULL_RECT)
	fo.anchor_left = 1
	fo.anchor_top = 1
	fo.offset_left = -300
	fo.offset_top = -210
	fo.offset_right = -8
	fo.offset_bottom = -140
	fo.alignment = BoxContainer.ALIGNMENT_END
	root.add_child(fo)
	_ctx_actions = fo
	# Every verb here is keyed to what you *tapped* (set visible in `_refresh_systems`):
	# the object is the centre, and only the actions it affords appear.
	# — A belt/outer-moon site: send an autonomous miner, or recall the one working it.
	_mine_btn = _make_op_button("⛏ Send Miner", _deploy_miner)
	_mine_btn.visible = false
	fo.add_child(_mine_btn)
	_withdraw_btn = _make_op_button("⤴ Withdraw Miner", _withdraw_miner_here)
	_withdraw_btn.visible = false
	fo.add_child(_withdraw_btn)
	# — An uninhabited body: plant an outpost (the station that develops into a base), or
	#   develop the one you already have here.
	_outpost_btn = _make_op_button("⚑ Build Outpost", _found_outpost_here)
	_outpost_btn.visible = false
	fo.add_child(_outpost_btn)
	_dev_outpost_btn = _make_op_button("⬆ Develop Outpost", _develop_outpost_here)
	_dev_outpost_btn.visible = false
	fo.add_child(_dev_outpost_btn)
	# — Your operational outpost: build facilities (Mine to produce raw goods, then Storage/Hangar).
	_fac_mine_btn = _make_op_button("⛏ Build Mine", func(): _build_facility(0))
	_fac_mine_btn.visible = false
	fo.add_child(_fac_mine_btn)
	_fac_storage_btn = _make_op_button("▣ Build Storage", func(): _build_facility(1))
	_fac_storage_btn.visible = false
	fo.add_child(_fac_storage_btn)
	_fac_hangar_btn = _make_op_button("⊓ Build Hangar", func(): _build_facility(2))
	_fac_hangar_btn.visible = false
	fo.add_child(_fac_hangar_btn)
	# — A fully-built outpost: promote it to a colony (triples its yield).
	_promote_btn = _make_op_button("★ Promote to Colony", _promote_outpost_here)
	_promote_btn.visible = false
	fo.add_child(_promote_btn)
	# — An uninhabited body: plant your shipyard (the warship facility).
	_build_btn = _make_op_button("⚓ Build Shipyard", _found_shipyard_here)
	_build_btn.visible = false
	fo.add_child(_build_btn)
	# — Your shipyard's body: develop it further.
	_expand_btn = _make_op_button("⬆ Expand Shipyard", _expand_shipyard)
	_expand_btn.visible = false
	fo.add_child(_expand_btn)
	# — A contested colony: build influence over it, then claim it.
	_court_btn = _make_op_button("◎ Court", _court_contested)
	_court_btn.visible = false
	fo.add_child(_court_btn)
	_claim_btn = _make_op_button("◎ Claim", _claim_contested)
	_claim_btn.visible = false
	fo.add_child(_claim_btn)
	# — An independent colony: buy it out (a mid-game goal — much pricier than an outpost).
	_acquire_ctx_btn = _make_op_button("⊕ Acquire Colony", _acquire_focused_colony)
	_acquire_ctx_btn.visible = false
	fo.add_child(_acquire_ctx_btn)
	# — A colony you own: develop it (the tall growth axis).
	_develop_btn = _make_op_button("⬆ Develop", _develop_focused_colony)
	_develop_btn.visible = false
	fo.add_child(_develop_btn)
	# — Any world: send the docked fleet there.
	_send_btn = _make_op_button("🚀 Send Fleet", _dispatch_fleet_to_focus)
	_send_btn.visible = false
	fo.add_child(_send_btn)
	# The empire layer (E3): answer a great-power coalition strike on your holdings —
	# lit only while the inners are moving on you.
	_defend_holdings_btn = _make_op_button("⚔ Defend Holdings", _defend_holdings)
	_defend_holdings_btn.visible = false
	fo.add_child(_defend_holdings_btn)
	# (The gate-transit + far-side bridgehead/defend verbs were removed — no stake-less
	#  "click to win" endgame; the early-game trade/management sim is the focus.)



func _add_toggle(col: VBoxContainer, text: String, cb: Callable) -> CheckButton:
	var row := UiKit.toggle_row(text, false)
	col.add_child(row)
	var cbtn: CheckButton = row.get_node("toggle")
	cbtn.toggled.connect(cb)
	return cbtn


func _make_map_button(label: String, cb: Callable) -> Button:
	var btn := Button.new()
	btn.text = label
	btn.custom_minimum_size = Vector2(40, 40)
	btn.add_theme_font_size_override("font_size", 20)
	btn.focus_mode = Control.FOCUS_NONE
	btn.add_theme_stylebox_override("normal", UiKit.panel_box(UiKit.BG_BAR, UiKit.LINE, 6))
	btn.add_theme_stylebox_override("hover", UiKit.panel_box(UiKit.ACCENT_SOFT, UiKit.ACCENT, 6))
	btn.add_theme_stylebox_override("pressed", UiKit.panel_box(UiKit.ACCENT_SOFT, UiKit.ACCENT, 6))
	btn.add_theme_color_override("font_color", UiKit.ACCENT)
	btn.pressed.connect(cb)
	return btn


## Make a HUD panel draggable with a finger (or mouse) — translate it by the drag delta,
## which works regardless of its anchors (shift all four offsets together). Empty panel
## areas drag it; interactive children (toggles/buttons, which stay STOP) keep working,
## because the panel's content container is set IGNORE so empty regions fall through to it.
func _make_draggable(panel: Control, content: Control) -> void:
	content.mouse_filter = Control.MOUSE_FILTER_IGNORE
	panel.mouse_filter = Control.MOUSE_FILTER_STOP
	panel.gui_input.connect(func(e: InputEvent) -> void:
		# Touch uses ScreenDrag; the mouse uses MouseMotion. Gate by mode so a touch
		# device's emulated mouse events don't double-translate the panel.
		var rel := Vector2.ZERO
		if e is InputEventScreenDrag and not pc_mode:
			rel = e.relative
		elif e is InputEventMouseMotion and pc_mode and (e.button_mask & MOUSE_BUTTON_MASK_LEFT) != 0:
			rel = e.relative
		else:
			return
		panel.offset_left += rel.x
		panel.offset_right += rel.x
		panel.offset_top += rel.y
		panel.offset_bottom += rel.y
		panel.accept_event()
	)


## A wider labelled touch button for fleet ops (§6).
func _make_op_button(label: String, cb: Callable) -> Button:
	var btn := Button.new()
	btn.text = label
	btn.custom_minimum_size = Vector2(104, 32)
	btn.add_theme_font_size_override("font_size", 12)
	btn.focus_mode = Control.FOCUS_NONE
	btn.add_theme_stylebox_override("normal", UiKit.panel_box(UiKit.BG_BAR, UiKit.LINE, 6))
	btn.add_theme_stylebox_override("hover", UiKit.panel_box(UiKit.ACCENT_SOFT, UiKit.ACCENT, 6))
	btn.add_theme_stylebox_override("pressed", UiKit.panel_box(UiKit.ACCENT_SOFT, UiKit.ACCENT, 6))
	btn.add_theme_color_override("font_color", UiKit.TEXT_HI)
	btn.pressed.connect(cb)
	return btn


## ---- Phase A: the act-now decision panel (a menu of trade-off options) --------

## Build the bottom-centre dilemma panel — hidden until an act-now exception offers
## the player a choice (speculate / profiteer / relief, etc.). Each option is a tap
## target with its own benefit/risk line, so answering the feed is a *decision*.
func _build_decision_panel() -> void:
	_dec_layer = CanvasLayer.new()
	_dec_layer.layer = 50
	_dec_layer.visible = false
	add_child(_dec_layer)
	# A dim full-screen scrim behind the popup so it reads as a modal "stop and decide"
	# moment (the game is hard-paused while it's up), and so stray taps don't hit the map.
	var scrim := ColorRect.new()
	scrim.color = Color(0.0, 0.0, 0.0, 0.55)
	scrim.set_anchors_preset(Control.PRESET_FULL_RECT)
	scrim.mouse_filter = Control.MOUSE_FILTER_STOP
	_dec_layer.add_child(scrim)
	var panel := PanelContainer.new()
	panel.add_theme_stylebox_override("panel", UiKit.panel_box(Color(0.05, 0.07, 0.11, 0.98), UiKit.ACCENT, 12, 2))
	# A large centred card — the popup is the main event, not a corner toast.
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.offset_left = -420
	panel.offset_right = 420
	panel.offset_top = -230
	panel.offset_bottom = 230
	_dec_layer.add_child(panel)
	var box := VBoxContainer.new()
	box.add_theme_constant_override("separation", 12)
	panel.add_child(box)
	box.add_child(UiKit.kicker("⚠ Act Now — Your Call"))
	_dec_title = UiKit.label("", 24, UiKit.TEXT_HI)
	_dec_title.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	box.add_child(_dec_title)
	box.add_child(UiKit.rule())
	_dec_opts = VBoxContainer.new()
	_dec_opts.add_theme_constant_override("separation", 10)
	box.add_child(_dec_opts)


## Show/populate the dilemma panel for the top pending decision (rebuilt on change).
func _refresh_decisions() -> void:
	# Hold the modal during the opening grace window (§0.3) — see EARLY_GRACE_TICKS.
	if sim.decision_count() <= 0 or sim.tick() < EARLY_GRACE_TICKS:
		_dec_layer.visible = false
		_dec_shown = ""
		return
	_dec_layer.visible = true
	var title := String(sim.decision_title(0))
	if title == _dec_shown:
		return                                  # already rendered this dilemma
	_dec_shown = title
	_dec_title.text = title
	for c in _dec_opts.get_children():
		c.queue_free()
	var n := sim.decision_option_count(0)
	for opt in n:
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 8)
		var risky := sim.decision_option_risky(0, opt)
		var label := String(sim.decision_option_label(0, opt))
		var btn := _make_op_button(("⚠ " if risky else "") + label, _resolve_decision.bind(opt))
		btn.custom_minimum_size = Vector2(190, 56)
		btn.add_theme_font_size_override("font_size", 16)
		row.add_child(btn)
		var desc := UiKit.label(String(sim.decision_option_desc(0, opt)), 15, UiKit.TEXT)
		desc.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		desc.custom_minimum_size = Vector2(560, 0)
		desc.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		desc.size_flags_vertical = Control.SIZE_EXPAND_FILL
		row.add_child(desc)
		_dec_opts.add_child(row)


## Resolve the top dilemma with the chosen option, surface the outcome, repaint.
func _resolve_decision(opt: int) -> void:
	var msg := String(sim.resolve_decision(0, opt))
	status = msg if msg != "" else "Couldn't act on that — nothing to source."
	_dec_shown = ""                             # force a rebuild for any next dilemma
	_refresh_decisions()


## Buy + deploy a miner at the focused body (early-game industry) — it mines that body's
## raw into your warehouse. Tap a body first to choose where (and what) to mine.
func _deploy_miner() -> void:
	if _focus_body <= 0 or sim.body_kind(_focus_body) == 5:
		status = "Tap a body first (an asteroid/moon), then ⛏ MINE its mineral."
		return
	if not sim.can_mine_body(_focus_body):
		status = "Off-limits — miners work only the belts & outer moons/rings, not the Earth/Mars AO."
		return
	var msg := String(sim.buy_miner(_focus_body))
	status = msg if msg != "" else "Can't deploy a miner — need 9,000 cr (or the miner cap is reached)."


## Recall the miner working the focused body (the "until withdrawn" half of the loop).
func _withdraw_miner_here() -> void:
	var msg := String(sim.withdraw_miner(_focus_body))
	status = msg if msg != "" else "No miner here to withdraw."


## Found the player's shipyard on the tapped (uninhabited) body — your first body-built station.
func _found_shipyard_here() -> void:
	var msg := String(sim.found_shipyard_at(_focus_body))
	status = msg if msg != "" else "Can't build here — need 60,000 cr (a shipyard's a major undertaking)."


## Found an outpost on the tapped (uninhabited) body — the station that develops into a base.
func _found_outpost_here() -> void:
	var msg := String(sim.found_outpost(_focus_body))
	status = msg if msg != "" else "Can't build an outpost here — need 18,000 cr (or it's an occupied/invalid site)."


## Develop the outpost on the focused body a level (raises its tribute).
func _develop_outpost_here() -> void:
	var msg := String(sim.develop_outpost(_focus_body))
	status = msg if msg != "" else "Can't develop this outpost — it's maxed, or you're short on credits."


## Build a facility (0 Mine · 1 Storage · 2 Hangar) at the focused outpost.
func _build_facility(kind: int) -> void:
	var msg := String(sim.build_outpost_facility(_focus_body, kind))
	status = msg if msg != "" else "Can't build that facility — need 12,000 cr, and the outpost must be operational."


## Promote the focused (fully-built) outpost to a colony.
func _promote_outpost_here() -> void:
	var next := String(sim.outpost_next_rank_name(_focus_body))
	var msg := String(sim.promote_outpost(_focus_body))
	if msg != "":
		ascend_flash = 1.0
		status = msg
	else:
		status = "Can't promote to %s — it must be maxed, fully facilitated, hold enough population, and you need the credits." % next


## Buy out the independent colony on the focused body — a mid-game goal (much pricier than
## an outpost), bought by clicking the colony itself.
func _acquire_focused_colony() -> void:
	var ci := _colony_index_for_body(_focus_body)
	if ci < 0 or not sim.colony_acquirable(ci):
		status = "Tap an independent colony to buy it."
		return
	var cost: int = sim.colony_acquire_cost(ci)
	var code: int = sim.acquire_colony(ci)
	if code == 0:
		ascend_flash = 1.0
		status = "⊕ %s acquired — it joins your holdings." % String(sim.colony_name(ci))
	elif code == 3:
		status = "Can't afford %s — it costs %s cr (acquiring a colony is a mid-game goal)." % [String(sim.colony_name(ci)), _commas(cost)]
	else:
		status = "Can't acquire %s." % String(sim.colony_name(ci))


## Develop the colony sitting on the focused body (the tall growth axis).
func _develop_focused_colony() -> void:
	var ci := _colony_index_for_body(_focus_body)
	if ci < 0:
		status = "Tap one of your colonies to develop it."
		return
	var msg := String(sim.develop_colony(ci))
	status = msg if msg != "" else "Can't develop %s — it's maxed, or you're short on credits." % String(sim.colony_name(ci))


## The colony index sitting on `body`, or -1 (object→index lookup for contextual actions).
func _colony_index_for_body(body: int) -> int:
	if body <= 0:
		return -1
	for i in sim.colony_count():
		if sim.colony_body(i) == body:
			return i
	return -1


## The contested-hub index sitting on `body`, or -1.
func _contested_index_for_body(body: int) -> int:
	if body <= 0:
		return -1
	for i in sim.contested_count():
		if sim.contested_body(i) == body:
			return i
	return -1


## Send every docked warship on a committed trajectory to the focused world (§6).
func _dispatch_fleet_to_focus() -> void:
	if _focus_body <= 0 or sim.body_kind(_focus_body) == 5:
		status = "Tap a world first, then SEND FLEET."
		return
	var sent := 0
	var blocked := ""
	for i in sim.fleet_size():
		if sim.ship_in_transit(i):
			continue
		var err := String(sim.move_ship(i, _focus_body, false))
		if err == "":
			sent += 1
		elif err != "already docked there":
			blocked = err
	if sent > 0:
		status = "%d ship(s) burning for %s." % [sent, String(sim.body_name(_focus_body))]
	elif blocked != "":
		status = "Can't dispatch: %s." % blocked
	else:
		status = "No docked warships to send (or already there)."


## ---- save slots + Ironman (§13/§30) -----------------------------------------

func _slot_path(i: int) -> String:
	return ProjectSettings.globalize_path("user://torch_slot_%d.sav" % i)


func _ironman_path() -> String:
	return ProjectSettings.globalize_path("user://torch_ironman.sav")


func _active_save_path() -> String:
	return _ironman_path() if ironman else _slot_path(save_slot)


func _do_save() -> void:
	var err := String(sim.save_game(_active_save_path()))
	if err == "":
		status = "Saved — %s." % ("Ironman" if ironman else "slot %d" % (save_slot + 1))
	else:
		status = "Save failed: %s" % err


func _do_load() -> void:
	if ironman:
		status = "Ironman — no manual reload. Your choices stand."
		return
	var err := String(sim.load_game(_slot_path(save_slot)))
	if err == "":
		speed_idx = 0
		selected = 0
		status = "Loaded slot %d — paused. [1] to resume." % (save_slot + 1)
	else:
		status = "Load failed: %s" % err


func _cycle_slot() -> void:
	save_slot = (save_slot + 1) % SAVE_SLOTS
	var t := sim.save_peek(_slot_path(save_slot))
	status = "Slot %d — %s." % [save_slot + 1, ("saved day %d" % (t / 24)) if t >= 0 else "empty"]


func _toggle_ironman(on: bool) -> void:
	ironman = on
	if ironman:
		sim.save_game(_ironman_path())   # commit a baseline immediately
		_autosave_accum = 0.0
		status = "IRONMAN engaged — autosaves, no reloads. Live with your choices."
	else:
		status = "Ironman off — manual slots restored."


## Cycle the corporation name through the presets (§14 self-expression).
func _cycle_corp_name() -> void:
	corp_name_idx += 1
	var nm := String(sim.set_corp_name(corp_name_idx))
	status = "Corporation renamed: %s." % nm


## Cycle the fleet livery and repaint the ships (§14).
func _cycle_livery_btn() -> void:
	sim.cycle_livery()
	_apply_livery()
	status = "Fleet livery updated."


## Repaint the shared warship material to the current livery (§14).
func _apply_livery() -> void:
	var c := sim.corp_livery_color()
	_ship_mat.albedo_color = c
	_ship_mat.emission = c


## Refuel every docked warship to a full tank (§6).
func _refuel_fleet() -> void:
	var n := 0
	for i in sim.fleet_size():
		if sim.refuel_ship(i):
			n += 1
	status = "Refuelled %d ship(s)." % n if n > 0 else "Nothing to refuel."


## Refit every docked hull to your best-owned weapons (Phase B) — a yard fee + time
## per ship; refitting hulls can't fight until they're out of the yard.
func _refit_fleet() -> void:
	var n := 0
	for i in sim.fleet_size():
		if String(sim.refit_ship(i, -1, -1, -1)) != "":   # -1 = best-owned
			n += 1
	if n > 0:
		status = "Refitting %d hull(s) to your latest weapons — they're in the yard." % n
	else:
		status = "No hull to refit (need docked ships + a better weapon in service)."


## Refit one ship to the refit bay's chosen models (Phase B per-model refit).
func _refit_selected_ship() -> void:
	if sim.fleet_size() <= 0:
		status = "No ship to refit."
		return
	var tgt: int = clampi(_refit_target, 0, sim.fleet_size() - 1)
	var msg := String(sim.refit_ship(
		tgt, _refit_model_id(0), _refit_model_id(1), _refit_model_id(2)))
	status = msg if msg != "" else "Can't refit — dock the ship at the yard, or own the models."


## The model id chosen in the refit bay for `kind` (0 PDC, 1 TORP, 2 RAIL).
func _refit_model_id(kind: int) -> int:
	var nn := sim.owned_model_count(kind)
	if nn <= 0:
		return -1
	return sim.owned_model_id(kind, clampi(int(_refit_model_i[kind]), 0, nn - 1))


func _cycle_refit_target(delta: int) -> void:
	var n := sim.fleet_size()
	if n > 0:
		_refit_target = (_refit_target + delta + n) % n


func _cycle_refit_model(kind: int, delta: int) -> void:
	var n := sim.owned_model_count(kind)
	if n > 0:
		_refit_model_i[kind] = (int(_refit_model_i[kind]) + delta + n) % n


## Transit the open ring-gate into the endgame (§0.1/§17) — the climax of the climb.
func _transit_gate() -> void:
	if sim.transit_gate():
		ascend_flash = 1.0
		status = "⟁ You transited the ring. There is no coming back the same."
		# Jump the camera through to the far side — its first revealed world (§17).
		for b in sim.body_count():
			if sim.body_is_far_side(b):
				_focus_body = b
				_zoom = 8.0
				break


## Buy out the cheapest independent frontier colony (the empire layer, E1). One-press
## expansion — but each acquisition angers the inner powers, so grow with care.
## Develop the least-developed holding (Phase C, the tall growth axis) — invest credits
## to scale its output, with no coalition alarm (unlike acquiring a new colony).
func _develop_colony() -> void:
	var msg := String(sim.develop_best())
	status = msg if msg != "" else "Nothing to develop — acquire a colony first, or you're maxed/short on credits."


## Cycle the empire-wide development doctrine (Phase C) — Industry / Trade / Growth tilt.
func _cycle_doctrine() -> void:
	status = String(sim.cycle_dev_doctrine())


func _acquire_colony() -> void:
	var best := -1
	var best_cost := 1 << 30
	for i in sim.colony_count():
		if sim.colony_acquirable(i):
			var cost: int = sim.colony_acquire_cost(i)
			if cost >= 0 and cost < best_cost:
				best_cost = cost
				best = i
	if best < 0:
		status = "No independent colonies left to acquire."
		return
	var name := String(sim.colony_name(best))
	var code: int = sim.acquire_colony(best)
	match code:
		0:
			ascend_flash = 1.0
			status = "⊕ %s joins the company — the inners are watching." % name
			_focus_body = sim.colony_body(best)
		3:
			status = "Not enough credits to acquire %s (needs %d cr)." % [name, best_cost]
		_:
			status = "%s can't be acquired." % name


## Diplomatically annex an independent colony (the empire layer, E4) — spends
## Influence and needs good standing with the Independents, but angers the inners less.
func _annex_colony() -> void:
	var target := -1
	for i in sim.colony_count():
		if sim.colony_annexable(i):
			target = i
			break
	if target < 0:
		# Tell the player why nothing is annexable (the common gates).
		if sim.influence() < 300:
			status = "Annexation needs Influence (have %d/300) and Independent goodwill." % sim.influence()
		else:
			status = "No colony to annex — raise standing with the Independents first."
		return
	var name := String(sim.colony_name(target))
	var code: int = sim.annex_colony(target)
	match code:
		0:
			ascend_flash = 1.0
			status = "⊕ %s joins us by treaty — cleaner than coin." % name
			_focus_body = sim.colony_body(target)
		3:
			status = "The Independents don't trust us enough to annex %s yet." % name
		4:
			status = "Not enough Influence to annex %s (need 300)." % name
		_:
			status = "%s can't be annexed." % name


## Seize a colony by force (the empire layer, E5) — assault the weakest-garrisoned
## target with the fleet. The harshest political price of the three paths.
func _seize_colony() -> void:
	# Pick the lightest-garrisoned colony we don't already hold.
	var target := -1
	var best_garrison := 1 << 30
	for i in sim.colony_count():
		if not sim.colony_controlled(i):
			var g: int = sim.colony_garrison(i)
			if g < best_garrison:
				best_garrison = g
				target = i
	if target < 0:
		status = "No colony left to seize."
		return
	var name := String(sim.colony_name(target))
	var code: int = sim.seize_colony(target, combat_band)
	match code:
		1:
			ascend_flash = 1.0
			status = "⚔ %s taken by force — the owner will not forget this." % name
			_focus_body = sim.colony_body(target)
			_open_diorama()
		0:
			flash = 1.0
			status = "⚔ The assault on %s failed — we lost ships for nothing." % name
			_open_diorama()
		-3:
			status = "No fleet to mount an assault — commission warships first."
		_:
			status = "%s can't be seized." % name


## Court an independent company (the diplomacy layer, E8) — invest Influence to deepen
## your best non-allied relationship toward alliance. Macro: a standing investment, not
## a per-event choice. Allies' colonies join you free and their ships screen your trade.
func _court_company() -> void:
	# Advance the most-promising relationship that isn't already an ally.
	var target := -1
	var best_rel := -2000000000
	for i in sim.company_count():
		var st: int = sim.company_stance(i)
		if st < 4:  # not yet Ally
			var rel: int = sim.company_relation(i)
			if rel > best_rel:
				best_rel = rel
				target = i
	if target < 0:
		status = "Every independent company is already allied with us."
		return
	var name := String(sim.company_name(target))
	var code: int = sim.court_company(target)
	match code:
		0:
			var sn: int = sim.company_stance(target)
			var stance: String = ["Rival", "Cold", "Neutral", "Partner", "Ally"][sn]
			status = "🤝 Courted %s — now %s." % [name, stance]
		2:
			status = "Not enough Influence to court %s (need 100)." % name
		_:
			status = "Can't court %s." % name


## The contested hub the player has the focused body on, else the one they've courted
## most (so the COURT/CLAIM verbs act on something sensible without a separate selector).
func _focus_contested() -> int:
	var n := sim.contested_count()
	if n == 0:
		return -1
	if _focus_body > 0:
		for i in n:
			if sim.contested_body(i) == _focus_body:
				return i
	var best := 0
	var best_pi := -1
	for i in n:
		var pi: int = sim.contested_player_influence(i)
		if pi > best_pi:
			best_pi = pi
			best = i
	return best


## Court the focused contested hub — spend Influence to build standing toward claiming it.
func _court_contested() -> void:
	var i := _focus_contested()
	if i < 0:
		status = "No contested hubs."
		return
	var msg := String(sim.court_contested_colony(i))
	if msg != "":
		status = msg
	else:
		status = "Can't court %s — need Influence (it accrues over time)." % String(sim.contested_name(i))


## Claim the focused contested hub once your standing clears the threshold.
func _claim_contested() -> void:
	var i := _focus_contested()
	if i < 0:
		status = "No contested hubs."
		return
	if not sim.contested_claimable(i):
		status = "%s: standing %d/%d — court it more before claiming." % [String(sim.contested_name(i)), sim.contested_player_influence(i), sim.contested_claim_threshold()]
		return
	var msg := String(sim.claim_contested_colony(i))
	if msg != "":
		ascend_flash = 1.0
		status = msg
	else:
		status = "Can't claim %s." % String(sim.contested_name(i))


## Defend the holdings against a great-power coalition strike (the empire layer, E3).
func _defend_holdings() -> void:
	var result: int = sim.defend_holdings(combat_band)
	match result:
		1:
			ascend_flash = 1.0
			status = "⚔ Coalition strike repelled — the holdings stand."
			_open_diorama()
		0:
			flash = 1.0
			status = "⚔ The line broke — the inners pried a holding loose."
			_open_diorama()
		-1:
			status = "No fleet to answer the coalition. Commission warships."


## Found the far-side bridgehead (§17, G3) — the first foothold beyond the ring.
func _found_bridgehead() -> void:
	var code: int = sim.found_bridgehead()
	match code:
		0:
			ascend_flash = 1.0
			status = "⛓ The bridgehead stands. We hold ground beyond the ring."
		1:
			status = "Transit the gate before you can plant a foothold."
		2:
			status = "Not enough credits to found the bridgehead."
		3:
			status = "The bridgehead already stands."


## Reinforce the far-side bridgehead a level (§17, G3) — more integrity to weather
## the incursions to come.
func _upgrade_bridgehead() -> void:
	var code: int = sim.upgrade_bridgehead()
	match code:
		0:
			var lvl: int = sim.bridgehead_level()
			status = "⛓ Bridgehead reinforced — now level %d." % lvl
		2:
			status = "Not enough credits to reinforce the bridgehead."
		3:
			status = "Found a bridgehead before you can reinforce it."


## Defend the bridgehead against the pending incursion (§17, G4) — rally the fleet
## and resolve the fight. The far side answers; you answer back.
func _defend_bridgehead() -> void:
	var result: int = sim.defend_bridgehead(combat_band)
	match result:
		1:
			ascend_flash = 1.0
			status = "⚔ Incursion repelled — the bridgehead holds the line."
			_open_diorama()
		0:
			flash = 1.0
			status = "⚔ The line broke — the bridgehead is hit."
			_open_diorama()
		-1:
			status = "No fleet to answer the incursion. Commission warships."


## Rename the flagship, cycling an evocative call-sign pool (§14, mobile-friendly —
## no text entry). The hero ship gets a player-chosen identity (the Rocinante effect).
const _SHIP_CALLSIGNS := [
	"Valkyrie", "Tarrasque", "Sundancer", "Black Mesa", "Wayfarer", "Roci",
	"Nemesis", "Firebrand", "Pale Horse", "Daybreak", "Old Faithful", "Specter",
]
var _callsign_idx := 0
func _rename_flagship() -> void:
	var fi: int = sim.flagship_index()
	if fi < 0:
		status = "No ship to rename — commission a hull first."
		return
	_callsign_idx = (_callsign_idx + 1) % _SHIP_CALLSIGNS.size()
	var nm: String = _SHIP_CALLSIGNS[_callsign_idx]
	if sim.rename_ship(fi, nm):
		status = "Flagship renamed: %s." % String(sim.ship_name(fi))


## ---- combat command + the §22 diorama --------------------------------------

## Cycle the engagement range band (§9): close ⇄ medium ⇄ long.
func _cycle_band() -> void:
	combat_band = (combat_band + 1) % 3
	status = "Engagement range: %s." % ["close", "medium", "long"][combat_band]


## Flip the fleet's target priority (§9): biggest hull ⇄ most wounded.
func _cycle_target() -> void:
	sim.set_combat_target(1 - sim.combat_target())
	status = "Target priority: %s." % ("most wounded" if sim.combat_target() == 1 else "biggest hull")


## Step the retreat threshold (§9): fight-to-death → 25 → 50 → 75 → death.
func _cycle_retreat() -> void:
	var steps := [0, 25, 50, 75]
	var idx := 0
	for s in steps.size():
		if steps[s] == sim.combat_retreat():
			idx = s
	sim.set_combat_retreat(steps[(idx + 1) % steps.size()])
	var rt := sim.combat_retreat()
	status = "Retreat threshold: %s." % ("never (fight to the death)" if rt == 0 else "%d%%" % rt)


## Toggle hot vs disciplined railgun fire (§9 heat): more alpha, but it vents.
func _cycle_fire() -> void:
	sim.set_combat_aggressive(not sim.combat_aggressive())
	status = "Railgun fire: %s." % ("AGGRESSIVE — more alpha, builds heat" if sim.combat_aggressive() else "disciplined — steady, no heat")


## Throw the on-station fleet at a raider pack and play back the result (§9/§22).
func _engage_raiders() -> void:
	var r := sim.engage(combat_band)
	if r == -1:
		# Distinguish "no fleet" from "fleet is off defending elsewhere" (§6).
		if sim.fleet_size() > 0 and sim.warships_on_station() == 0:
			status = "Fleet is off-station — recall warships to the core to engage."
		else:
			status = "No warships to send — commission a hull in BUILD first."
		return
	_open_diorama()


## Build the full-screen BattleLog playback overlay (§22), hidden until a fight.
func _build_diorama() -> void:
	_diorama = CanvasLayer.new()
	_diorama.layer = 60
	_diorama.visible = false
	add_child(_diorama)
	var dim := ColorRect.new()
	dim.color = Color(0.02, 0.03, 0.06, 0.93)
	dim.set_anchors_preset(Control.PRESET_FULL_RECT)
	dim.mouse_filter = Control.MOUSE_FILTER_STOP
	dim.gui_input.connect(func(e):
		if (e is InputEventMouseButton and e.pressed) or (e is InputEventScreenTouch and e.pressed):
			_close_diorama())
	_diorama.add_child(dim)
	var box := VBoxContainer.new()
	box.set_anchors_preset(Control.PRESET_FULL_RECT)
	box.offset_left = 90
	box.offset_right = -90
	box.offset_top = 44
	box.offset_bottom = -44
	box.add_theme_constant_override("separation", 8)
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_diorama.add_child(box)
	box.add_child(UiKit.kicker("Engagement Report"))
	_dio_title = UiKit.label("", 22, UiKit.ACCENT)
	box.add_child(_dio_title)
	_dio_sub = UiKit.label("", 14, UiKit.TEXT_DIM)
	box.add_child(_dio_sub)
	# Live force rosters — two pip bars that deplete as kills play (§22 juice).
	var forces := HBoxContainer.new()
	forces.add_theme_constant_override("separation", 40)
	box.add_child(forces)
	_dio_force_a = _dio_force_label()
	_dio_force_b = _dio_force_label()
	forces.add_child(_dio_force_a)
	forces.add_child(_dio_force_b)
	box.add_child(UiKit.rule())
	# The 3D battle scene (forged fleets trading fire) fills the upper area.
	var vpc := SubViewportContainer.new()
	vpc.stretch = true
	vpc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	vpc.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	vpc.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_child(vpc)
	var vp := SubViewport.new()
	vp.own_world_3d = true
	vp.transparent_bg = false
	vp.msaa_3d = Viewport.MSAA_DISABLED
	vpc.add_child(vp)
	_battle3d = BattleDiorama.new()
	vp.add_child(_battle3d)
	# A shorter play-by-play log beneath the battle.
	var sc := ScrollContainer.new()
	sc.custom_minimum_size = Vector2(0, 132)
	sc.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_child(sc)
	_dio_log = RichTextLabel.new()
	_dio_log.bbcode_enabled = true
	_dio_log.fit_content = true
	_dio_log.scroll_following = true
	_dio_log.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_dio_log.add_theme_font_size_override("normal_font_size", 14)
	_dio_log.mouse_filter = Control.MOUSE_FILTER_IGNORE
	sc.add_child(_dio_log)
	box.add_child(UiKit.label("tap anywhere to dismiss", 11, UiKit.TEXT_DIM))


## A roster label (bbcode) for one side's depleting force pips.
func _dio_force_label() -> RichTextLabel:
	var r := RichTextLabel.new()
	r.bbcode_enabled = true
	r.fit_content = true
	r.scroll_active = false
	r.custom_minimum_size = Vector2(360, 0)
	r.add_theme_font_size_override("normal_font_size", 16)
	r.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return r


## Render one side's force as filled/spent pips + a tally (§22 juice).
func _dio_set_force(label: RichTextLabel, who: String, alive: int, total: int, col: Color) -> void:
	var pips := ""
	for i in mini(total, 16):
		pips += "▰" if i < alive else "▱"
	label.text = "[color=#%s]%s[/color]\n[color=#%s]%s[/color]  [color=#%s]%d/%d[/color]" % [
		UiKit.TEXT_HI.to_html(false), who,
		col.to_html(false), pips,
		UiKit.TEXT.to_html(false), alive, total]


## Pause the world and begin replaying the just-resolved battle.
func _open_diorama() -> void:
	speed_idx = 0
	accum = 0.0
	_dio_log.text = ""
	_dio_idx = 0
	_dio_timer = 0.0
	_dio_playing = true
	var bands := ["CLOSE", "MEDIUM", "LONG"]
	_dio_title.text = "ENGAGEMENT · %s RANGE" % bands[clampi(sim.battle_band(), 0, 2)]
	_dio_sub.text = "%s — %d hulls    vs    Raiders — %d hulls" % [
		String(sim.corp_name()), sim.battle_start_count(0), sim.battle_start_count(1)]
	# Rosters start at full strength and deplete as kills reveal.
	_dio_start = [sim.battle_start_count(0), sim.battle_start_count(1)]
	_dio_surv = [_dio_start[0], _dio_start[1]]
	_dio_refresh_forces()
	# Spawn the 3D fleets (player in Independent livery, raiders as scavenged Belt hulls).
	_battle3d.setup(3, _dio_start[0], _dio_start[1])
	_diorama.visible = true


## Repaint both force rosters from the live surviving counts.
func _dio_refresh_forces() -> void:
	_dio_set_force(_dio_force_a, String(sim.corp_name()), _dio_surv[0], _dio_start[0], UiKit.GOOD)
	_dio_set_force(_dio_force_b, "Raiders", _dio_surv[1], _dio_start[1], UiKit.BAD)


func _close_diorama() -> void:
	_dio_playing = false
	_diorama.visible = false
	if _battle3d:
		_battle3d.stop()


## Reveal one BattleLog beat per DIO_STEP, then the outcome (called each frame).
func _play_diorama(delta: float) -> void:
	if not _dio_playing:
		return
	_dio_timer += delta
	var total := sim.battle_log_count()
	while _dio_timer >= DIO_STEP and _dio_idx < total:
		_dio_timer -= DIO_STEP
		_dio_log.append_text(_dio_event_line(_dio_idx) + "\n")
		var kind := sim.battle_event_kind(_dio_idx)
		var side := sim.battle_event_side(_dio_idx)
		# Drive the 3D scene's fire/explosion FX from the same beat.
		_battle3d.on_beat(kind, side)
		# A kill depletes the victim side's roster live (§22 juice).
		if kind == 2:
			_dio_surv[side] = maxi(0, _dio_surv[side] - 1)
			_dio_refresh_forces()
		_dio_idx += 1
		if _dio_idx >= total:
			_dio_log.append_text("\n" + _dio_outcome_line())
			_dio_playing = false


## One BattleLog beat rendered as a bbcode line, coloured by side (§19/§22).
func _dio_event_line(i: int) -> String:
	var side := sim.battle_event_side(i)
	var who := String(sim.corp_name()) if side == 0 else "Raiders"
	var col := UiKit.GOOD.to_html(false) if side == 0 else UiKit.BAD.to_html(false)
	match sim.battle_event_kind(i):
		0:   # Salvo
			return "[color=#%s]%s[/color] torpedo salvo — %d leaker(s) breach the screen" % [
				col, who, sim.battle_event_value(i)]
		1:   # Volley
			return "[color=#%s]%s[/color] railgun volley — %d damage" % [
				col, who, sim.battle_event_value(i)]
		2:   # Destroyed
			return "    [color=#%s]✖ %s destroyed[/color]" % [
				UiKit.BAD.to_html(false), String(sim.battle_event_name(i))]
		3:   # Retreat
			return "[color=#%s]%s breaks off and retreats[/color]" % [
				UiKit.ACCENT.to_html(false), who]
		4:   # Overheat
			return "[color=#%s]%s vents heat — railguns hold fire[/color]" % [
				UiKit.GOLD.to_html(false), who]
	return ""


## The closing verdict + survivor tally for the diorama.
func _dio_outcome_line() -> String:
	var head := ""
	match sim.battle_winner():
		0:
			head = "[color=#%s]◆ FIELD HELD — the raiders break and run.[/color]" % UiKit.GOOD.to_html(false)
		1:
			head = "[color=#%s]✖ FLEET BROKEN — the raiders hold the field.[/color]" % UiKit.BAD.to_html(false)
		_:
			head = "[color=#%s]— STALEMATE — both sides withdraw.[/color]" % UiKit.TEXT_DIM.to_html(false)
	var bounty := sim.battle_bounty()
	var bline := ""
	if sim.battle_winner() == 0 and bounty > 0:
		bline = "\n[color=#%s]✚ Bounty +%d cr — the lanes are calmer.[/color]" % [UiKit.GOOD.to_html(false), bounty]
	return "%s\n[color=#%s]Survivors — %s %d/%d  ·  Raiders %d/%d[/color]%s" % [
		head, UiKit.TEXT.to_html(false),
		String(sim.corp_name()), sim.battle_survivors(0), sim.battle_start_count(0),
		sim.battle_survivors(1), sim.battle_start_count(1), bline]


# ============================================================================
# FLEET VIEW (roster table)
# ============================================================================

func _build_fleet_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	panel.add_child(v)
	# Tabs.
	var tabs := HBoxContainer.new()
	tabs.add_theme_constant_override("separation", 4)
	v.add_child(tabs)
	var names := ["ALL", "FLEETS", "SINGLE SHIPS", "IDLE"]
	for i in names.size():
		var b := UiKit.tab_button(names[i], i == 0)
		var ti := i
		b.pressed.connect(func(): _set_fleet_tab(ti))
		tabs.add_child(b)
		_fleet_tabs.append(b)
	# Expressive identity (§14): corp name + cycle name / livery.
	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	tabs.add_child(spacer)
	_corp_lbl = UiKit.label("", 13, UiKit.TEXT_HI)
	tabs.add_child(_corp_lbl)
	tabs.add_child(_make_op_button("RENAME", _cycle_corp_name))
	tabs.add_child(_make_op_button("LIVERY", _cycle_livery_btn))
	tabs.add_child(_make_op_button("FLAGSHIP", _rename_flagship))
	_fleet_count = UiKit.label("", 11, UiKit.TEXT_DIM)
	v.add_child(_fleet_count)
	# Combat command (§9): doctrine knobs + the engage verb that opens the diorama.
	var cmd := HBoxContainer.new()
	cmd.add_theme_constant_override("separation", 6)
	v.add_child(cmd)
	cmd.add_child(UiKit.kicker("Doctrine"))
	_combat_lbl = UiKit.label("", 12, UiKit.TEXT)
	_combat_lbl.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	cmd.add_child(_combat_lbl)
	cmd.add_child(_make_op_button("RANGE", _cycle_band))
	cmd.add_child(_make_op_button("TARGET", _cycle_target))
	cmd.add_child(_make_op_button("RETREAT", _cycle_retreat))
	cmd.add_child(_make_op_button("FIRE", _cycle_fire))
	cmd.add_child(_make_op_button("◆ ENGAGE", _engage_raiders))
	# Refit bay (Phase B): pick a docked hull + a model per slot, then refit it (fee +
	# time in the yard). Batch "REFIT FLEET" (best-owned) lives on the op deck below.
	var rb := HBoxContainer.new()
	rb.add_theme_constant_override("separation", 5)
	v.add_child(rb)
	rb.add_child(UiKit.kicker("Refit bay"))
	rb.add_child(_tiny_btn("◂", func() -> void: _cycle_refit_target(-1)))
	var rt := UiKit.label("—", 11, UiKit.TEXT_HI)
	rt.custom_minimum_size = Vector2(112, 0)
	rt.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	rb.add_child(rt)
	_refit_lbl["target"] = rt
	rb.add_child(_tiny_btn("▸", func() -> void: _cycle_refit_target(1)))
	rb.add_child(_make_refit_picker(0, "P"))
	rb.add_child(_make_refit_picker(1, "T"))
	rb.add_child(_make_refit_picker(2, "R"))
	rb.add_child(_make_op_button("⚒ REFIT", _refit_selected_ship))
	v.add_child(UiKit.rule())
	# Header row + grid.
	var sc := ScrollContainer.new()
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	v.add_child(sc)
	_fleet_grid = GridContainer.new()
	_fleet_grid.columns = 6
	_fleet_grid.add_theme_constant_override("h_separation", 22)
	_fleet_grid.add_theme_constant_override("v_separation", 7)
	_fleet_grid.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	sc.add_child(_fleet_grid)


func _set_fleet_tab(i: int) -> void:
	fleet_tab = i
	for t in _fleet_tabs.size():
		_fleet_tabs[t].set_pressed_no_signal(t == i)


# ============================================================================
# BUILD VIEW (hull list · blueprint · cost · queue)
# ============================================================================

func _build_build_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var hb := HBoxContainer.new()
	hb.add_theme_constant_override("separation", 12)
	panel.add_child(hb)

	# Left: hull list.
	var left := VBoxContainer.new()
	left.custom_minimum_size = Vector2(220, 0)
	left.add_theme_constant_override("separation", 5)
	hb.add_child(left)
	left.add_child(UiKit.kicker("Hull Types"))
	_build_list = VBoxContainer.new()
	_build_list.add_theme_constant_override("separation", 4)
	left.add_child(_build_list)
	for i in shipyard.class_count():
		var b := Button.new()
		b.toggle_mode = true
		b.focus_mode = Control.FOCUS_NONE
		b.button_pressed = i == build_pick
		b.alignment = HORIZONTAL_ALIGNMENT_LEFT
		b.add_theme_font_size_override("font_size", 13)
		b.add_theme_color_override("font_color", UiKit.TEXT)
		b.add_theme_color_override("font_pressed_color", UiKit.ACCENT)
		b.add_theme_stylebox_override("normal", UiKit.panel_box(UiKit.BG_INSET, UiKit.LINE, 6))
		b.add_theme_stylebox_override("pressed", UiKit.panel_box(UiKit.ACCENT_SOFT, UiKit.ACCENT, 6))
		b.add_theme_stylebox_override("hover", UiKit.panel_box(UiKit.BG_INSET, UiKit.LINE_HI, 6))
		b.text = "  %s" % String(shipyard.class_name(i))
		var bi := i
		b.pressed.connect(func(): _pick_build(bi))
		_build_list.add_child(b)

	# Centre: wireframe blueprint viewport + caption + cost.
	var centre := VBoxContainer.new()
	centre.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	centre.add_theme_constant_override("separation", 6)
	hb.add_child(centre)
	var vp_panel := UiKit.make_panel(UiKit.BG_INSET, UiKit.LINE, 8)
	vp_panel.size_flags_vertical = Control.SIZE_EXPAND_FILL
	centre.add_child(vp_panel)
	var svc := SubViewportContainer.new()
	svc.stretch = true
	svc.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	svc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	svc.mouse_filter = Control.MOUSE_FILTER_IGNORE
	vp_panel.add_child(svc)
	var sv := SubViewport.new()
	sv.own_world_3d = true
	sv.transparent_bg = true
	svc.add_child(sv)
	# Light the bench so the procedural hull reads: a key directional + a warm fill +
	# ambient from a WorldEnvironment (the SubViewport owns its own world).
	var env := WorldEnvironment.new()
	var e := Environment.new()
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color(0.42, 0.46, 0.55)
	e.ambient_light_energy = 1.15
	env.environment = e
	sv.add_child(env)
	var scam := Camera3D.new()
	scam.position = Vector3(0.4, 1.5, 4.8)
	sv.add_child(scam)
	scam.look_at(Vector3.ZERO, Vector3.UP)   # after entering the tree (uses global xform)
	var key := DirectionalLight3D.new()
	key.rotation_degrees = Vector3(-40, 125, 0)
	key.light_energy = 1.7
	sv.add_child(key)
	# A camera-side fill so faces toward the viewer don't fall to shadow.
	var fill := DirectionalLight3D.new()
	fill.rotation_degrees = Vector3(-15, -35, 0)
	fill.light_color = Color(0.85, 0.9, 1.0)
	fill.light_energy = 0.7
	sv.add_child(fill)
	_ship_pivot = Node3D.new()
	sv.add_child(_ship_pivot)
	_build_caption = UiKit.label("", 15, UiKit.TEXT_HI)
	centre.add_child(_build_caption)
	# Ship designer (A2): arm weapons on the hull's slots + set the burn profile;
	# the stats + the 3D hull update live, and COMMISSION builds your design.
	centre.add_child(UiKit.kicker("Loadout — arm the slots"))
	var drow1 := HBoxContainer.new()
	drow1.add_theme_constant_override("separation", 16)
	centre.add_child(drow1)
	drow1.add_child(_make_stepper("pdc", "PDC"))
	drow1.add_child(_make_stepper("torp", "TORP"))
	var drow2 := HBoxContainer.new()
	drow2.add_theme_constant_override("separation", 16)
	centre.add_child(drow2)
	drow2.add_child(_make_stepper("rail", "RAIL"))
	drow2.add_child(_make_stepper("burn", "BURN%"))
	# Per-slot model pickers (Phase B): choose which in-service weapon model arms each
	# kind's slots — your fleet loadout is the macro decision.
	centre.add_child(UiKit.kicker("Fit models (from your foundry)"))
	centre.add_child(_make_model_picker(0, "PDC"))
	centre.add_child(_make_model_picker(1, "TORP"))
	centre.add_child(_make_model_picker(2, "RAIL"))
	_design_lbl = UiKit.label("", 12, UiKit.TEXT)
	centre.add_child(_design_lbl)
	_reset_design()
	_forge_ship()
	_build_stats = UiKit.label("", 12, UiKit.TEXT_DIM)
	centre.add_child(_build_stats)
	_build_cost = UiKit.label("", 12, UiKit.TEXT)
	centre.add_child(_build_cost)
	_bom_lbl = UiKit.label("", 11, UiKit.TEXT_DIM)
	centre.add_child(_bom_lbl)
	_commission_btn = UiKit.action_button("◆  COMMISSION HULL")
	_commission_btn.pressed.connect(_commission_selected)
	centre.add_child(_commission_btn)
	var assemble := UiKit.action_button("⚙  ASSEMBLE FROM PARTS")
	assemble.pressed.connect(_assemble_selected)
	centre.add_child(assemble)
	# Civilian trader (freighter) — buildable from the start (no shipyard needed); the
	# logistics hull you assign to trade routes. The early-game money-maker.
	var freighter := UiKit.action_button("⛟  BUY TRADER (FREIGHTER)")
	freighter.pressed.connect(_buy_freighter)
	centre.add_child(freighter)

	# Right: construction queue.
	var right := VBoxContainer.new()
	right.custom_minimum_size = Vector2(230, 0)
	right.add_theme_constant_override("separation", 5)
	hb.add_child(right)
	# Shipyard (Phase B+): warships need your own yard (Tycho sells only civilians +
	# corvettes). Build it (very expensive) and expand it to lay down bigger hulls.
	right.add_child(UiKit.kicker("Shipyard"))
	_yard_lbl = UiKit.label("", 12, UiKit.TEXT)
	right.add_child(_yard_lbl)
	var yrow := HBoxContainer.new()
	yrow.add_theme_constant_override("separation", 5)
	right.add_child(yrow)
	yrow.add_child(_make_op_button("⚓ FOUND", _found_shipyard))
	yrow.add_child(_make_op_button("⬆ EXPAND", _expand_shipyard))

	right.add_child(UiKit.kicker("Construction Queue"))
	_build_queue = VBoxContainer.new()
	_build_queue.add_theme_constant_override("separation", 6)
	right.add_child(_build_queue)

	# Weapon arsenal / crafting (Phase B): scrap from combat crafts better weapons,
	# which newly built ships fit. Advanced/faction designs antagonise the powers.
	right.add_child(UiKit.kicker("Weapon Arsenal"))
	_scrap_lbl = UiKit.label("", 12, UiKit.ACCENT)
	right.add_child(_scrap_lbl)
	var asc := ScrollContainer.new()
	asc.custom_minimum_size = Vector2(0, 230)
	asc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	right.add_child(asc)
	_arsenal_box = VBoxContainer.new()
	_arsenal_box.add_theme_constant_override("separation", 3)
	asc.add_child(_arsenal_box)


## Build (or rebuild) the procedural ship in the BUILD bench for the selected class,
## using the sim's hull slot counts + the player's faction livery (§24/§25).
func _forge_ship() -> void:
	if _ship_pivot == null:
		return
	for c in _ship_pivot.get_children():
		c.queue_free()
	# The player flies the Belt livery by default (home turf); the forge shows the
	# player's chosen loadout (A2), so weapon models track the steppers.
	var fac := 3
	var ship := ShipForge.build(build_pick, fac, _des_pdc, _des_torp, _des_rail, 1000 + build_pick)
	_ship_pivot.add_child(ship)


## The weapon foundry list (Phase B): schematics you've earned → production lines you
## tool up (time + scrap + credits). You can't buy advanced weapons. Rebuilt on change.
func _refresh_arsenal() -> void:
	if _arsenal_box == null:
		return
	var scrap := sim.scrap()
	_scrap_lbl.text = "⚙ Scrap parts: %d" % scrap
	# Signature folds in owned + schematic + producing counts so the list repaints as
	# production lines finish.
	var owned := 0
	var known := 0
	var building := 0
	var wc := sim.weapon_count()
	for i in wc:
		if sim.weapon_owned(i):
			owned += 1
		if sim.weapon_known(i):
			known += 1
		if sim.weapon_producing(i) > 0:
			building += 1
	var sig := "%d|%d|%d|%d" % [scrap, owned, known, building]
	if sig == _arsenal_sig:
		return
	_arsenal_sig = sig
	for c in _arsenal_box.get_children():
		c.queue_free()
	var kinds := ["PDC", "TORPEDO", "RAILGUN"]
	var last_kind := -1
	for i in wc:
		var k := sim.weapon_kind(i)
		if k != last_kind:
			last_kind = k
			_arsenal_box.add_child(UiKit.kicker(kinds[clampi(k, 0, 2)]))
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 5)
		var ownd := sim.weapon_owned(i)
		var nm := UiKit.label(String(sim.weapon_name(i)), 11, UiKit.TEXT_HI if ownd else UiKit.TEXT)
		nm.custom_minimum_size = Vector2(118, 0)
		row.add_child(nm)
		if ownd:
			row.add_child(UiKit.label("✓ in service", 10, UiKit.GOOD))
		elif sim.weapon_producing(i) > 0:
			row.add_child(UiKit.label("⏳ building", 10, UiKit.ACCENT))
		elif sim.weapon_can_produce(i):
			var b := _make_op_button("BUILD", _produce_weapon.bind(i))
			b.custom_minimum_size = Vector2(64, 24)
			row.add_child(b)
		elif sim.weapon_known(i):
			row.add_child(UiKit.label("need parts", 10, UiKit.TEXT_DIM))
		else:
			row.add_child(UiKit.label("🔒 schematic", 10, UiKit.TEXT_DIM))
		_arsenal_box.add_child(row)
		var d := UiKit.label(String(sim.weapon_desc(i)), 10, UiKit.TEXT_DIM)
		d.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		d.custom_minimum_size = Vector2(200, 0)
		_arsenal_box.add_child(d)


## Found a shipyard at home (Phase B+) — very expensive; unlocks warship building.
func _found_shipyard() -> void:
	var msg := String(sim.found_shipyard_home())
	status = msg if msg != "" else "Can't found a shipyard — need 60,000 cr (or you already have one)."


## Expand the shipyard a tier (unlocks the next hull class).
func _expand_shipyard() -> void:
	var msg := String(sim.expand_shipyard())
	status = msg if msg != "" else "Can't expand — build a yard first, it's maxed, or short on credits."


## Start producing weapon `i` (needs the schematic + scrap + credits + time).
func _produce_weapon(i: int) -> void:
	var msg := String(sim.produce_weapon(i))
	status = msg if msg != "" else "Can't build that — need the schematic, scrap, or credits."
	_arsenal_sig = ""
	_refresh_arsenal()


## A weapon/burn stepper for the designer (A2): [label] [−] value [+].
func _make_stepper(kind: String, label: String) -> Control:
	var hb := HBoxContainer.new()
	hb.add_theme_constant_override("separation", 2)
	hb.add_child(UiKit.label(label, 11, UiKit.TEXT_DIM))
	hb.add_child(_make_op_button("−", func() -> void: _adjust_design(kind, -1)))
	var val := UiKit.label("0", 13, UiKit.TEXT_HI)
	val.custom_minimum_size = Vector2(40, 0)
	val.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	hb.add_child(val)
	_des_vals[kind] = val
	hb.add_child(_make_op_button("+", func() -> void: _adjust_design(kind, 1)))
	return hb


## A per-kind weapon-model picker (Phase B): [LABEL] [<] model-name [>] — cycles the
## in-service models of that kind. The chosen model arms that kind's slots.
func _make_model_picker(kind: int, label: String) -> Control:
	var hb := HBoxContainer.new()
	hb.add_theme_constant_override("separation", 4)
	var l := UiKit.label(label, 11, UiKit.TEXT_DIM)
	l.custom_minimum_size = Vector2(40, 0)
	hb.add_child(l)
	hb.add_child(_make_op_button("<", func() -> void: _cycle_model(kind, -1)))
	var nm := UiKit.label("—", 11, UiKit.TEXT_HI)
	nm.custom_minimum_size = Vector2(150, 0)
	nm.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	hb.add_child(nm)
	_des_model_lbl[kind] = nm
	hb.add_child(_make_op_button(">", func() -> void: _cycle_model(kind, 1)))
	return hb


## A small single-char cycle button (compact, for the refit bay row).
func _tiny_btn(txt: String, cb: Callable) -> Button:
	var b := _make_op_button(txt, cb)
	b.custom_minimum_size = Vector2(26, 26)
	return b


## A compact refit-bay model picker for `kind`: [label] ‹ name › — cycles owned models.
func _make_refit_picker(kind: int, label: String) -> Control:
	var hb := HBoxContainer.new()
	hb.add_theme_constant_override("separation", 2)
	hb.add_child(UiKit.label(label, 10, UiKit.TEXT_DIM))
	hb.add_child(_tiny_btn("‹", func() -> void: _cycle_refit_model(kind, -1)))
	var nm := UiKit.label("—", 10, UiKit.TEXT_HI)
	nm.custom_minimum_size = Vector2(86, 0)
	nm.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	hb.add_child(nm)
	_refit_lbl[kind] = nm
	hb.add_child(_tiny_btn("›", func() -> void: _cycle_refit_model(kind, 1)))
	return hb


## The model id chosen for `kind` (0 PDC, 1 TORP, 2 RAIL), from the in-service models.
func _des_model_id(kind: int) -> int:
	var n := sim.owned_model_count(kind)
	if n <= 0:
		return -1
	var idx: int = clampi(int(_des_model_i[kind]), 0, n - 1)
	return sim.owned_model_id(kind, idx)


## Cycle the chosen model for a kind (wrapping through the in-service models).
func _cycle_model(kind: int, delta: int) -> void:
	var n := sim.owned_model_count(kind)
	if n <= 0:
		return
	_des_model_i[kind] = (int(_des_model_i[kind]) + delta + n) % n


## Reset the draft loadout to the hull's full reference fit (every slot armed).
func _reset_design() -> void:
	_des_pdc = shipyard.pdc_mounts(build_pick)
	_des_torp = shipyard.torpedo_mounts(build_pick)
	_des_rail = shipyard.railgun_mounts(build_pick)
	_des_burn = 100


## Change a designer value (clamped to the hull's slots / burn range), then re-forge.
func _adjust_design(kind: String, delta: int) -> void:
	match kind:
		"pdc": _des_pdc = clampi(_des_pdc + delta, 0, shipyard.pdc_mounts(build_pick))
		"torp": _des_torp = clampi(_des_torp + delta, 0, shipyard.torpedo_mounts(build_pick))
		"rail": _des_rail = clampi(_des_rail + delta, 0, shipyard.railgun_mounts(build_pick))
		"burn": _des_burn = clampi(_des_burn + delta * 10, 10, 100)
	_forge_ship()


func _pick_build(i: int) -> void:
	build_pick = i
	for c in _build_list.get_child_count():
		(_build_list.get_child(c) as Button).set_pressed_no_signal(c == i)
	_reset_design()
	_forge_ship()


func _commission_selected() -> void:
	# Build the player's custom design (A2/Phase B): chosen weapon models per slot.
	var code: int = sim.commission_designed(
		build_pick,
		_des_model_id(0), _des_pdc,
		_des_model_id(1), _des_torp,
		_des_model_id(2), _des_rail,
		_des_burn)
	match code:
		0: status = "%s commissioned to your design." % String(shipyard.class_name(build_pick))
		1: status = "Can't build — short on credits."
		2: status = "Can't build — not enough trained crew."
		4: status = "Need your own shipyard for this hull (Tycho sells only civilians + corvettes)."
		_: status = "That design won't fit the hull."


## Buy a civilian trader (freighter) — no shipyard needed; the hull you put on trade routes.
func _buy_freighter() -> void:
	status = "Trader commissioned — assign it a route in MARKET." if sim.commission_freighter() else "Can't build a trader — short on credits or crew."


## Assemble the selected hull from the player's own component stock (§7d payoff).
func _assemble_selected() -> void:
	var cls := String(shipyard.class_name(build_pick))
	match sim.assemble_ship(build_pick):
		0:
			status = "%s assembled from parts — the chain pays off." % cls
		1:
			status = "Missing components — need %s (build them up the chain)." % String(sim.ship_bom_desc(build_pick))
		3:
			status = "Not enough trained crew to assemble a %s." % cls
		_:
			status = "Can't assemble — short on the labour fee."


# ============================================================================
# MARKET VIEW (flow schematic · ticker · price history)
# ============================================================================

func _build_market_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	panel.add_child(v)

	# Market + route controls (a trade-management header — no longer keyboard-only).
	var ctl := HBoxContainer.new()
	ctl.add_theme_constant_override("separation", 6)
	v.add_child(ctl)
	ctl.add_child(_make_op_button("◀", func(): _cycle_market(-1)))
	_mkt_sel_lbl = UiKit.label("", 13, UiKit.TEXT_HI)
	_mkt_sel_lbl.custom_minimum_size = Vector2(180, 0)
	_mkt_sel_lbl.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	ctl.add_child(_mkt_sel_lbl)
	ctl.add_child(_make_op_button("▶", func(): _cycle_market(1)))
	ctl.add_child(_make_op_button("＋ Create Route (to best market)", _create_route_from_sel))
	ctl.add_child(_make_op_button("✕ Clear Routes", func(): sim.clear_trade_route(); status = "Trade routes cleared."))
	_mkt_routes_lbl = UiKit.label("", 11, UiKit.TEXT_DIM)
	v.add_child(_mkt_routes_lbl)

	# Top: commodity-flow schematic.
	v.add_child(UiKit.kicker("Trade Flow"))
	var flowp := UiKit.make_panel(UiKit.BG_INSET, UiKit.LINE, 8)
	flowp.size_flags_vertical = Control.SIZE_EXPAND_FILL
	flowp.custom_minimum_size = Vector2(0, 250)
	v.add_child(flowp)
	_flow = FlowGraphS.new()
	_flow.mouse_filter = Control.MOUSE_FILTER_IGNORE
	flowp.add_child(_flow)
	var names := PackedStringArray()
	for m in _visible_market_count():
		names.append(String(sim.market_name(m)))
	_flow.set_markets(names)

	# Bottom row: ticker (left) + price history (right).
	var bottom := HBoxContainer.new()
	bottom.add_theme_constant_override("separation", 10)
	bottom.custom_minimum_size = Vector2(0, 250)
	v.add_child(bottom)

	var tickerp := UiKit.make_panel(UiKit.BG_INSET, UiKit.LINE, 8)
	tickerp.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	bottom.add_child(tickerp)
	var tv := VBoxContainer.new()
	tickerp.add_child(tv)
	_ticker_title = UiKit.kicker("Market Ticker")
	tv.add_child(_ticker_title)
	tv.add_child(UiKit.rule())
	var tsc := ScrollContainer.new()
	tsc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	tsc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	tv.add_child(tsc)
	_ticker_grid = GridContainer.new()
	_ticker_grid.columns = 5
	_ticker_grid.add_theme_constant_override("h_separation", 18)
	_ticker_grid.add_theme_constant_override("v_separation", 6)
	tsc.add_child(_ticker_grid)

	var chartp := UiKit.make_panel(UiKit.BG_INSET, UiKit.LINE, 8)
	chartp.custom_minimum_size = Vector2(380, 0)
	bottom.add_child(chartp)
	var cv := VBoxContainer.new()
	chartp.add_child(cv)
	_chart_title = UiKit.kicker("Price History")
	cv.add_child(_chart_title)
	var chb := HBoxContainer.new()
	chb.size_flags_vertical = Control.SIZE_EXPAND_FILL
	cv.add_child(chb)
	_chart = MiniChartS.new()
	_chart.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_chart.size_flags_vertical = Control.SIZE_EXPAND_FILL
	_chart.mouse_filter = Control.MOUSE_FILTER_IGNORE
	chb.add_child(_chart)
	_chart_legend = VBoxContainer.new()
	_chart_legend.custom_minimum_size = Vector2(96, 0)
	chb.add_child(_chart_legend)
	# Track the first CHART_COMMS commodities.
	var cols := _chart_colors()
	_chart.setup(cols)
	for i in mini(CHART_COMMS, sim.commodity_count()):
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 5)
		var sw := ColorRect.new()
		sw.color = cols[i]
		sw.custom_minimum_size = Vector2(10, 10)
		row.add_child(sw)
		row.add_child(UiKit.label(String(sim.commodity_name(i)), 10, UiKit.TEXT_DIM))
		_chart_legend.add_child(row)


func _chart_colors() -> Array[Color]:
	return [UiKit.ACCENT, UiKit.GOLD, UiKit.GOOD, Color(0.7, 0.55, 0.95)]


# ============================================================================
# CAMERA + WORLD↔SCREEN
# ============================================================================

func _update_camera() -> void:
	var f := _focus_pos()
	# Shift the look target so the focus sits left-of-centre, clear of the right
	# context panel that overlays the orrery in the SYSTEMS view.
	var look := f + Vector3(0.18 * _zoom, 0.0, 0.0)
	# Orbit the view around the focus by the player's yaw (finger-twist / drag / Q-E / buttons).
	var dir := CAM_DIR.normalized().rotated(Vector3.UP, _yaw)
	_cam.position = look + dir * _zoom
	_cam.look_at(look, Vector3.UP)


func _focus_pos() -> Vector3:
	# The focused body (or Sol at the origin) plus any free-pan offset the player has
	# dragged the map by. Panning is on the ecliptic plane so the orrery stays flat.
	var base := Vector3.ZERO
	if _focus_body > 0 and _focus_body < sim.body_count():
		base = _world3d(sim.body_x(_focus_body), sim.body_y(_focus_body))
	return base + _pan


func _world3d(wx: float, wy: float) -> Vector3:
	return Vector3(wx * SCALE3D, 0.0, -wy * SCALE3D)


func _screen(p: Vector3) -> Vector2:
	return _cam.unproject_position(p)


## Input-event positions in `_unhandled_input` and `Camera3D.unproject_position` are **both**
## in the viewport's canvas space (Godot pre-transforms input by the stretch/screen transform),
## so map picking needs **no** content-scale conversion — even when the HUD is magnified
## (`content_scale_factor` ≠ 1). (An earlier ×factor here mis-scaled every click; verified via
## the viewport screen-transform + `Input.parse_input_event` that identity is correct.)
func _to_view(pos: Vector2) -> Vector2:
	return pos


func _zoom_by(factor: float) -> void:
	_zoom = clampf(_zoom * factor, ZOOM_MIN, ZOOM_MAX)


## Orbit the map view by `d` radians (wrapped). Drives both the ↺/↻ buttons + Q/E keys
## and the finger gestures (one-finger drag, two-finger twist).
func _rotate_by(d: float) -> void:
	_yaw = wrapf(_yaw + d, -PI, PI)


## Pan the map by a screen-space drag delta (PC mouse-drag). Convert the screen delta
## into a move on the ecliptic plane along the camera's flattened right/forward axes, so
## dragging feels like grabbing the map; scaled by zoom so it tracks the cursor at any scale.
func _pan_by(screen_delta: Vector2) -> void:
	var basis := _cam.global_transform.basis
	var right := Vector3(basis.x.x, 0.0, basis.x.z).normalized()
	var fwd := Vector3(-basis.z.x, 0.0, -basis.z.z).normalized()
	var k := _zoom * PAN_SENS
	_pan += right * (-screen_delta.x * k) + fwd * (screen_delta.y * k)


## The ▶ button: resume from pause to 1×, then step the time-compression up (1→2→3).
## (A pending dilemma re-pauses every frame, so this can't override "answer the popup".)
func _play_step() -> void:
	speed_idx = clampi(speed_idx + 1, 1, SPEEDS.size() - 1)


func _two_finger_dist() -> float:
	var pts := _touches.values()
	if pts.size() < 2:
		return 0.0
	return pts[0].distance_to(pts[1])


## The angle (rad) of the line between the two active touches — its change is the twist.
func _two_finger_angle() -> float:
	var pts := _touches.values()
	if pts.size() < 2:
		return 0.0
	var v: Vector2 = pts[1] - pts[0]
	return v.angle()


func _reset_view() -> void:
	_focus_body = 0
	_zoom = 10.0
	_yaw = 0.0
	_pan = Vector3.ZERO
	status = "View: inner system (drag to pan · Shift-drag / twist to rotate · wheel/pinch to zoom)."


# ============================================================================
# FRAME LOOP
# ============================================================================

func _process(delta: float) -> void:
	# Hard-pause on *every* act-now dilemma (§0.4): time can't resume until the popup is
	# answered. Each decision is a deliberate stop — the player resolves it, then plays on.
	if sim.decision_count() >= 1 and sim.tick() >= EARLY_GRACE_TICKS:
		if not _dilemma_lock:
			status = "A decision needs you — answer the popup to resume."
		_dilemma_lock = true
	else:
		_dilemma_lock = false
	if _dilemma_lock:
		speed_idx = 0
		accum = 0.0
	var mult: float = SPEEDS[speed_idx]
	if mult > 0.0:
		accum += delta * TICKS_PER_SECOND * mult
		while accum >= 1.0:
			sim.step()
			accum -= 1.0
			if sim.just_alerted():
				flash = 1.0
				if auto_pause:
					speed_idx = 0
					accum = 0.0
					status = "Auto-paused — act-now shortage. [E] exploit, then resume."
					break
	var tier := sim.tier_name()
	if last_tier != "" and tier != last_tier:
		ascend_flash = 1.0
		status = "Ascended to %s — the ring-gate draws closer." % tier
	last_tier = tier
	# The endgame finale (§17, G5): the journey completes, in triumph or defeat.
	var endgame: int = sim.endgame_outcome()
	if endgame != _last_endgame and endgame != 0:
		if endgame == 1:
			ascend_flash = 1.0
			status = "★ Victory — the far side is yours. The journey is complete."
		else:
			flash = 1.0
			status = "✝ The bridgehead has fallen. The far side is lost."
	_last_endgame = endgame
	flash = maxf(0.0, flash - delta * 2.0)
	ascend_flash = maxf(0.0, ascend_flash - delta)
	# Ironman (§13): the world saves itself, so there's no scumming a bad call.
	if ironman:
		_autosave_accum += delta
		if _autosave_accum >= IRONMAN_AUTOSAVE_SEC:
			_autosave_accum = 0.0
			sim.save_game(_ironman_path())
	if view == V_SYSTEMS:
		_update_world(delta)
	if view == V_BUILD and _ship_pivot:
		_ship_pivot.rotate_y(delta * 0.6)
	_play_diorama(delta)
	_sample_prices()
	_refresh()


## Record one price sample per sim tick into the history chart, regardless of the
## active view — so the MARKET chart already has a curve when you open it.
func _sample_prices() -> void:
	if _chart == null or sim.tick() == _last_chart_tick:
		return
	_last_chart_tick = sim.tick()
	var vals := PackedFloat32Array()
	for i in mini(CHART_COMMS, sim.commodity_count()):
		vals.append(float(sim.price(sel_market, i)))
	_chart.push(vals)


## Smoothly move a marker toward its latest sim position (§28 view interpolation),
## so traffic glides between sim ticks instead of snapping. Snaps on a big jump
## (a respawn or a pooled slot reused by a different entity).
func _smooth_to(node: Node3D, target: Vector3, delta: float, fresh: bool) -> void:
	if fresh or node.position.distance_to(target) > 8.0:
		node.position = target
	else:
		node.position = node.position.lerp(target, clampf(delta * VIEW_LERP, 0.0, 1.0))
	# Orient the hull marker along its heading (toward the target), so it points down
	# its lane instead of facing a fixed way (A5). Negligible when nearly stationary.
	var d := target - node.position
	if d.length() > 0.015:
		node.look_at(node.position + d.normalized(), Vector3.UP)


## Per-frame orrery level-of-detail + zoom-constant orbit-line width (a/c/i): keep every
## orbit ring a hairline at any zoom, and reveal moon + station detail only once the player
## has zoomed in past each tier — so the wide system view stays uncluttered.
func _update_orrery_lod() -> void:
	var tube := clampf(_zoom * ORBIT_TUBE_K, ORBIT_TUBE_MIN, ORBIT_TUBE_MAX)
	for ring in _planet_orbit_rings:
		var tm: TorusMesh = ring["tm"]
		var r: float = ring["r"]
		tm.inner_radius = maxf(0.001, r - tube)
		tm.outer_radius = r + tube
	var show_moons := _zoom < MOON_VIS_ZOOM
	for ring in _moon_orbit_rings:
		var mtm: TorusMesh = ring["tm"]
		var mr: float = ring["r"]
		var mt := minf(tube, mr * 0.4)
		mtm.inner_radius = maxf(0.001, mr - mt)
		mtm.outer_radius = mr + mt
		(ring["mi"] as MeshInstance3D).visible = show_moons
	if _gate_tm != null:
		var gt := clampf(tube * 3.0, 0.04, 1.0)   # the gate stays a touch bolder — it's the beacon
		_gate_tm.inner_radius = maxf(0.01, _gate_r - gt)
		_gate_tm.outer_radius = _gate_r + gt
	# Stations / colonies: glyph + tag only when zoomed in past the station tier.
	var show_st := _zoom < STATION_VIS_ZOOM
	for m in _station_markers:
		m.visible = show_st
	for l in _station_labels:
		l.visible = show_st


func _update_world(delta: float) -> void:
	_update_camera()
	var beyond: bool = sim.far_side_revealed()
	for b in sim.body_count():
		if b < _body_nodes.size():
			# Glide toward the latest per-tick sim position (§28 interpolation) instead of
			# snapping each tick — at 1× a body steps 6×/s, which reads as stutter. Snap on a
			# big jump (a far-side reveal / a large time-compression step) so it never lags.
			var node: Node3D = _body_nodes[b]
			var target := _world3d(sim.body_x(b), sim.body_y(b))
			if node.position.distance_to(target) > 8.0:
				node.position = target
			else:
				node.position = node.position.lerp(target, clampf(delta * VIEW_LERP, 0.0, 1.0))
			# Spin the body on its (tilted) axis — purely cosmetic view interpolation
			# like §28 marker smoothing, so it turns even while the sim is paused.
			if b < _body_spin.size() and _body_spin[b] != null and _body_spin_rate[b] != 0.0:
				_body_spin[b].rotate_object_local(Vector3.UP, delta * _body_spin_rate[b])
			# Visibility: far-side bodies stay hidden until the gate is transited (§17), and
			# moons drop out at wide zoom (LOD) so the system view isn't a clutter of specks.
			var vis := true
			if sim.body_is_far_side(b):
				vis = beyond
			if sim.body_kind(b) == 4 and _zoom >= MOON_VIS_ZOOM:
				vis = false
			node.visible = vis
	_update_orrery_lod()
	var n := sim.hauler_count()
	while _hauler_pool.size() < n:
		var mi := _hull_marker(_hauler_mat)
		_orrery_root.add_child(mi)
		_hauler_pool.append(mi)
	for i in _hauler_pool.size():
		var node := _hauler_pool[i]
		if i < n:
			var fresh := not node.visible
			node.visible = true
			_smooth_to(node, _world3d(sim.hauler_x(i), sim.hauler_y(i)), delta, fresh)
			var sel := i == selected
			var fi := sim.hauler_faction(i)
			var livery: StandardMaterial3D = _hauler_mat if fi < 0 else _faction_haul_mats[clampi(fi, 0, 3)]
			node.material_override = _select_mat if sel else livery
			node.scale = Vector3.ONE * (1.6 if sel else 1.0)
		else:
			node.visible = false
	# Player warships — positional now (§6); a moving one swells slightly.
	var sn := sim.fleet_size()
	while _ship_pool.size() < sn:
		var sm := _hull_marker(_ship_mat)
		_orrery_root.add_child(sm)
		_ship_pool.append(sm)
	for si in _ship_pool.size():
		var sship := _ship_pool[si]
		if si < sn:
			var fresh := not sship.visible
			sship.visible = true
			_smooth_to(sship, _world3d(sim.ship_x(si), sim.ship_y(si)), delta, fresh)
			sship.scale = Vector3.ONE * (1.4 if sim.ship_in_transit(si) else 1.0)
		else:
			sship.visible = false
	# Player freighters — positional on their standing-route lanes now (§6).
	var fn_ := sim.freighter_count()
	while _freighter_pool.size() < fn_:
		var fm := _hull_marker(_freighter_mat)
		_orrery_root.add_child(fm)
		_freighter_pool.append(fm)
	for fi in _freighter_pool.size():
		var fnode := _freighter_pool[fi]
		if fi < fn_:
			var fresh := not fnode.visible
			fnode.visible = true
			_smooth_to(fnode, _world3d(sim.freighter_x(fi), sim.freighter_y(fi)), delta, fresh)
		else:
			fnode.visible = false
	_lane_mesh.clear_surfaces()
	if n > 0 or fn_ > 0:
		_lane_mesh.surface_begin(Mesh.PRIMITIVE_LINES)
		# Draw lanes from the *smoothed* marker positions so trail + marker agree.
		for i in n:
			_lane_mesh.surface_add_vertex(_hauler_pool[i].position)
			_lane_mesh.surface_add_vertex(_world3d(sim.hauler_dest_x(i), sim.hauler_dest_y(i)))
		for i in fn_:
			_lane_mesh.surface_add_vertex(_freighter_pool[i].position)
			_lane_mesh.surface_add_vertex(_world3d(sim.freighter_dest_x(i), sim.freighter_dest_y(i)))
		_lane_mesh.surface_end()
	var wn := sim.wreck_count()
	while _wreck_pool.size() < wn:
		var wm := _sphere(0.03, _wreck_mat)
		_orrery_root.add_child(wm)
		_wreck_pool.append(wm)
	for wi in _wreck_pool.size():
		var wnode := _wreck_pool[wi]
		var wb := sim.wreck_body(wi) if wi < wn else -1
		if wb >= 0:
			wnode.visible = true
			wnode.position = _world3d(sim.body_x(wb), sim.body_y(wb)) + Vector3(0.12 + 0.08 * wi, 0.14, 0)
		else:
			wnode.visible = false
	# Deployed miners — a small amber rig riding above the body it works (early industry).
	var mn := sim.miner_count()
	while _miner_pool.size() < mn:
		var mm := _miner_marker()
		_orrery_root.add_child(mm)
		_miner_pool.append(mm)
	for mi in _miner_pool.size():
		var mnode := _miner_pool[mi]
		var mb := sim.miner_body(mi) if mi < mn else -1
		if mb >= 0:
			mnode.visible = true
			mnode.position = _world3d(sim.body_x(mb), sim.body_y(mb)) + Vector3(-0.12 - 0.07 * mi, 0.12, 0.05)
		else:
			mnode.visible = false
	var g: float = clampf(float(sim.gate_progress_pct()) / 100.0, 0.0, 1.0)
	_gate_mat.emission_energy_multiplier = 0.2 + 1.6 * g


# ============================================================================
# EMPIRE VIEW (holdings master-table + acquisition verbs — the empire layer E6)
# ============================================================================

func _build_empire_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	panel.add_child(v)

	# Headline: the empire rank + the next rung (the expansion spine, E6).
	v.add_child(UiKit.kicker("The Company"))
	_emp_header = UiKit.label("", 16, UiKit.TEXT_HI)
	v.add_child(_emp_header)
	# Meters: capacity / efficiency / coalition alarm / influence.
	_emp_meters = UiKit.label("", 12, UiKit.TEXT)
	v.add_child(_emp_meters)
	v.add_child(UiKit.rule())

	# Acquisition verbs — the three pathways + the defense (the empire master-deck).
	v.add_child(UiKit.kicker("Acquire / Defend"))
	var ops := HBoxContainer.new()
	ops.add_theme_constant_override("separation", 6)
	v.add_child(ops)
	ops.add_child(_make_op_button("⊕ BUY", _acquire_colony))
	ops.add_child(_make_op_button("⊕ ANNEX", _annex_colony))
	ops.add_child(_make_op_button("⚔ SEIZE", _seize_colony))
	ops.add_child(_make_op_button("⬆ DEVELOP", _develop_colony))
	ops.add_child(_make_op_button("⚙ DOCTRINE", _cycle_doctrine))
	ops.add_child(_make_op_button("⛨ DEFEND", _defend_holdings))
	ops.add_child(_make_op_button("🤝 COURT", _court_company))
	# Contested hubs (early game): gather influence over a fought-over colony, then claim it.
	ops.add_child(_make_op_button("◎ COURT HUB", _court_contested))
	ops.add_child(_make_op_button("◎ CLAIM HUB", _claim_contested))
	v.add_child(UiKit.rule())

	# The master-table: holdings + acquirable targets.
	v.add_child(UiKit.kicker("Holdings & Targets"))
	var sc := ScrollContainer.new()
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	v.add_child(sc)
	_emp_table = RichTextLabel.new()
	_emp_table.bbcode_enabled = true
	_emp_table.fit_content = true
	_emp_table.scroll_active = false
	_emp_table.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_emp_table.add_theme_font_size_override("normal_font_size", 12)
	sc.add_child(_emp_table)


func _refresh_empire() -> void:
	var holdings: int = sim.holding_count()
	var cap: int = sim.admin_capacity()
	var rank := String(sim.empire_rank())
	var next_name := String(sim.next_empire_rank_name())
	var head := "%s   ·   %d holding(s)" % [rank, holdings]
	if holdings > 0:
		head += "   ·   tallest L%d" % sim.peak_dev()   # Phase C: the tall axis
	if next_name != "":
		head += "   →   %s at %d" % [next_name, sim.next_empire_rank_at()]
	_emp_header.text = head

	var strain: int = sim.admin_strain()
	var meters := "Admin %d/%d" % [holdings, cap]
	if strain > 0:
		meters += " ⚠ strained (%d%% efficiency)" % sim.holdings_efficiency_pct()
	meters += "      Influence %d" % sim.influence()
	meters += "      Doctrine: %s" % String(sim.dev_doctrine_name())
	# Security (EP3): escorts on station vs. what the empire's shipping needs.
	if holdings > 0:
		var need: int = sim.escorts_needed()
		var have: int = sim.warships_on_station()
		if sim.empire_secure():
			meters += "      Escorts %d/%d ✓" % [have, need]
		else:
			meters += "      ⚠ Escorts %d/%d — piracy bleeds you" % [have, need]
		# Enforcement (EP4): a soured great power taxes/inspects your shipping.
		if sim.worst_standing() <= -200:
			meters += "      ⚠ customs sweeps (mend fences)"
	# Per-faction alarm (E7): show whose sphere you've provoked, not a single gauge.
	if holdings > 0:
		var names := ["Earth", "Mars", "Belt"]
		var parts := PackedStringArray()
		for fi in 3:
			parts.append("%s %d" % [names[fi], sim.faction_alarm(fi)])
		var prefix := "Alarm  "
		if sim.coalition_active():
			prefix = "⚠ COALITION (led by %s)  " % names[sim.coalition_leader()]
		meters += "\n%s%s" % [prefix, "  ·  ".join(parts)]
	if sim.coalition_strike_pending():
		meters += "  —  STRIKE INBOUND, DEFEND"
	_emp_meters.text = meters

	# Build the holdings + targets table.
	var t := ""
	# Contested hubs first — the early-game focus: the major colonies the powers fight
	# over, a gauge of each power's grip + your standing toward claiming it (the Ganymede
	# conflict). Eros/Pallas/Vesta/Tycho (belt) + Europa/Ganymede/Titan (jovian/cronian).
	if sim.contested_count() > 0:
		t += "[color=#9fb0c0]── CONTESTED HUBS  (the powers fight over these) ──[/color]\n"
		var sel := _focus_contested()
		for i in sim.contested_count():
			var cb := sim.contested_body(i)
			# Skip ones you've already claimed (they show under YOUR HOLDINGS).
			var claimed := false
			for j in sim.colony_count():
				if sim.colony_controlled(j) and sim.colony_body(j) == cb:
					claimed = true
					break
			if claimed:
				continue
			var marker := "▸ " if i == sel else "  "
			var lead: int = sim.contested_leader(i)
			t += "[color=#cfd8e0]%s%s[/color]  —  led by [color=%s]%s[/color]\n" % [marker, String(sim.contested_name(i)), _FAC_COL[lead], _faction_name(lead)]
			t += "     " + _influence_bar(i) + "\n"
			var pi: int = sim.contested_player_influence(i)
			var thr: int = sim.contested_claim_threshold()
			var pcol := "#78e68c" if pi >= thr else "#e6c860"
			var claim := "  ·  [color=#78e68c]CLAIMABLE[/color]" if pi >= thr else ""
			t += "     [color=#7a8696]your standing[/color] [color=%s]%d/%d[/color]%s\n" % [pcol, pi, thr, claim]
		t += "\n"
	t += "[color=#9fb0c0]── YOUR HOLDINGS ──[/color]\n"
	var any_held := false
	for i in sim.colony_count():
		if sim.colony_controlled(i):
			any_held = true
			var fac := _faction_name(sim.colony_faction(i))
			var good := String(sim.commodity_name(sim.colony_specialty(i)))
			var dev: int = sim.colony_dev(i)
			var devtxt := "[color=#e6c860]dev L%d[/color]" % dev
			var dcost: int = sim.develop_cost(i)
			if dcost >= 0:
				devtxt += " [color=#7a8696](→L%d: %s cr)[/color]" % [dev + 1, _commas(dcost)]
			else:
				devtxt += " [color=#7a8696](max)[/color]"
			var line := "[color=#78e68c]✦ %s[/color]  (%s)  ·  %s  ·  supplies [color=#cfd8e0]%s[/color]" % [String(sim.colony_name(i)), fac, devtxt, good]
			# EP2: flag the ones that are markets you now own (fee-reduced + NPC tariff).
			var body := sim.colony_body(i)
			for m in sim.market_count():
				if sim.market_body(m) == body and sim.market_is_owned(m):
					line += "  ·  [color=#9fd8ff]your market[/color]"
					break
			t += line + "\n"
	if sim.station_count() > 0:
		t += "[color=#78e68c]✦ %d production station(s)[/color]\n" % sim.station_count()
		any_held = true
	if not any_held:
		t += "[color=#6f8a93](none yet — acquire a frontier colony below)[/color]\n"
	t += "\n[color=#9fb0c0]── ACQUIRABLE (independents) ──[/color]\n"
	var any_target := false
	for i in sim.colony_count():
		if sim.colony_controlled(i):
			continue
		var fac_i: int = sim.colony_faction(i)
		var name := String(sim.colony_name(i))
		var garrison: int = sim.colony_garrison(i)
		if fac_i == 3:  # Independents — buyable / annexable
			any_target = true
			var cost: int = sim.colony_acquire_cost(i)
			var annex := "  ·  annex" if sim.colony_annexable(i) else ""
			t += "[color=#cfd8e0]· %s[/color]  buy %d cr%s  ·  garrison %d\n" % [name, cost, annex, garrison]
	if not any_target:
		t += "[color=#6f8a93](the independent frontier is yours — seize a great power's colony for more)[/color]\n"
	t += "\n[color=#9fb0c0]── SEIZABLE (by force) ──[/color]\n"
	for i in sim.colony_count():
		if sim.colony_controlled(i) or sim.colony_faction(i) == 3:
			continue
		var name2 := String(sim.colony_name(i))
		var fac2 := _faction_name(sim.colony_faction(i))
		t += "[color=#e0b0b0]⚔ %s[/color]  (%s)  ·  garrison %d\n" % [name2, fac2, sim.colony_garrison(i)]
	# Independent companies — the negotiable actors (E8). Macro diplomacy.
	if sim.company_count() > 0:
		t += "\n[color=#9fb0c0]── INDEPENDENT RELATIONS  (Influence %d) ──[/color]\n" % sim.influence()
		var stance_names := ["Rival", "Cold", "Neutral", "Partner", "Ally"]
		var stance_cols := ["#e0708a", "#9fb0c0", "#cfd8e0", "#9fd8ff", "#78e68c"]
		for i in sim.company_count():
			var s: int = sim.company_stance(i)
			t += "[color=%s]🤝 %s — %s[/color]  (rel %d)\n" % [stance_cols[s], String(sim.company_name(i)), stance_names[s], sim.company_relation(i)]
	_emp_table.text = t


func _faction_name(f: int) -> String:
	match f:
		0: return "Earth"
		1: return "Mars"
		2: return "Belt/OPA"
		_: return "Independent"


# ============================================================================
# LEDGER VIEW — the EU4-style sortable overview of every asset, where it is, and
# what it's doing. "I like numbers": click a column header to sort, tap a tab to
# switch asset class. (V_LEDGER)
# ============================================================================

const _LED_TABS := ["Fleet", "Miners", "Outposts", "Colonies", "Markets", "Mysteries"]
const _LED_MYSTERIES := 5
var _led_tab := 0
var _led_sort := 0
var _led_asc := true
var _led_grid: GridContainer
var _led_tab_row: HBoxContainer
var _led_summary: Label


# ============================================================================
# RESEARCH VIEW — the tech tree + your accumulated points + spend them (V_RESEARCH).
# ============================================================================

var _res_points_lbl: Label
var _res_tree: VBoxContainer


# ============================================================================
# DIPLOMACY VIEW — the great powers' standings + the independent companies you can
# court (V_DIPLOMACY). Surfaces the existing faction/company diplomacy as its own view.
# ============================================================================

var _dip_powers: VBoxContainer
var _dip_companies: VBoxContainer


func _build_diplomacy_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	panel.add_child(v)
	v.add_child(UiKit.kicker("The Great Powers — your standing"))
	_dip_powers = VBoxContainer.new()
	_dip_powers.add_theme_constant_override("separation", 4)
	v.add_child(_dip_powers)
	v.add_child(UiKit.rule())
	v.add_child(UiKit.kicker("Independent Companies — court them with Influence"))
	var sc := ScrollContainer.new()
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	v.add_child(sc)
	_dip_companies = VBoxContainer.new()
	_dip_companies.add_theme_constant_override("separation", 6)
	_dip_companies.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	sc.add_child(_dip_companies)


func _refresh_diplomacy() -> void:
	for c in _dip_powers.get_children():
		c.queue_free()
	for f in 4:
		var st: int = sim.faction_standing(f)
		var col := UiKit.GOOD if st > 50 else (UiKit.BAD if st < -50 else UiKit.TEXT)
		_dip_powers.add_child(UiKit.label("[%s] %s — standing %d (%s)" % [
			_FAC_COL_NAME(f), String(sim.faction_name(f)), st, String(sim.faction_tier(f))], 13, col))
	for c in _dip_companies.get_children():
		c.queue_free()
	if sim.company_count() == 0:
		_dip_companies.add_child(UiKit.label("(no independent companies in range)", 12, UiKit.TEXT_DIM))
	var stance_names := ["Rival", "Cold", "Neutral", "Partner", "Ally"]
	var stance_cols := [UiKit.BAD, UiKit.TEXT_DIM, UiKit.TEXT, Color(0.62, 0.85, 1.0), UiKit.GOOD]
	for i in sim.company_count():
		var row := UiKit.make_panel(UiKit.BG_INSET, UiKit.LINE, 6)
		var hb := HBoxContainer.new()
		hb.add_theme_constant_override("separation", 10)
		row.add_child(hb)
		var info := VBoxContainer.new()
		info.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		hb.add_child(info)
		var s: int = sim.company_stance(i)
		info.add_child(UiKit.label("🤝 %s" % String(sim.company_name(i)), 14, UiKit.TEXT_HI))
		info.add_child(UiKit.label("%s  ·  relation %d" % [stance_names[s], sim.company_relation(i)], 11, stance_cols[s]))
		if s < 4:
			hb.add_child(_make_op_button("Court", _court_company_idx.bind(i)))
		else:
			hb.add_child(UiKit.label("Allied ✓", 12, UiKit.GOOD))
		_dip_companies.add_child(row)


func _FAC_COL_NAME(f: int) -> String:
	return ["Earth", "Mars", "Belt", "Indep"][clampi(f, 0, 3)]


func _court_company_idx(i: int) -> void:
	var code: int = sim.court_company(i)
	if code == 0:
		status = "🤝 Courted %s." % String(sim.company_name(i))
	elif code == 2:
		status = "Not enough Influence to court %s." % String(sim.company_name(i))
	else:
		status = "Can't court %s." % String(sim.company_name(i))


func _build_research_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	panel.add_child(v)
	v.add_child(UiKit.kicker("Research — spend points earned through operations"))
	_res_points_lbl = UiKit.label("", 16, UiKit.ACCENT)
	v.add_child(_res_points_lbl)
	v.add_child(UiKit.rule())
	var sc := ScrollContainer.new()
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	v.add_child(sc)
	_res_tree = VBoxContainer.new()
	_res_tree.add_theme_constant_override("separation", 6)
	_res_tree.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	sc.add_child(_res_tree)


func _do_research(i: int) -> void:
	if sim.research_tech(i):
		status = "Researched %s." % String(sim.tech_name(i))
	else:
		status = "Can't research %s yet — need points or its prerequisite." % String(sim.tech_name(i))


func _refresh_research() -> void:
	_res_points_lbl.text = "⚛ %d research points  ·  %d/%d techs unlocked" % [
		sim.research_points(), sim.research_unlocked_count(), sim.tech_count()]
	for c in _res_tree.get_children():
		c.queue_free()
	for i in sim.tech_count():
		var row := UiKit.make_panel(UiKit.BG_INSET, UiKit.LINE, 6)
		var hb := HBoxContainer.new()
		hb.add_theme_constant_override("separation", 10)
		row.add_child(hb)
		var info := VBoxContainer.new()
		info.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		hb.add_child(info)
		var unlocked: bool = sim.tech_unlocked(i)
		var avail: bool = sim.tech_can_research(i)
		var name_col := UiKit.GOOD if unlocked else (UiKit.TEXT_HI if avail else UiKit.TEXT_DIM)
		var title := UiKit.label("%s%s" % ["✓ " if unlocked else "", String(sim.tech_name(i))], 14, name_col)
		info.add_child(title)
		var pr: int = sim.tech_prereq(i)
		var sub := "Cost %d pts" % sim.tech_cost(i)
		if pr >= 0:
			sub += "   ·   needs %s" % String(sim.tech_name(pr))
		info.add_child(UiKit.label(sub, 11, UiKit.TEXT_DIM))
		# Action: a Research button when available; status text otherwise.
		if unlocked:
			hb.add_child(UiKit.label("Researched", 12, UiKit.GOOD))
		elif avail:
			var b := _make_op_button("Research", _do_research.bind(i))
			hb.add_child(b)
		else:
			hb.add_child(UiKit.label("Locked", 12, UiKit.TEXT_DIM))
		_res_tree.add_child(row)


func _build_ledger_view() -> void:
	var panel := UiKit.make_panel()
	panel.visible = false
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_content.add_child(panel)
	_views.append(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 8)
	panel.add_child(v)

	v.add_child(UiKit.kicker("Ledger — your assets at a glance"))
	_led_summary = UiKit.label("", 13, UiKit.TEXT_HI)
	v.add_child(_led_summary)
	# Asset-class tabs.
	_led_tab_row = HBoxContainer.new()
	_led_tab_row.add_theme_constant_override("separation", 6)
	v.add_child(_led_tab_row)
	for t in _LED_TABS.size():
		_led_tab_row.add_child(UiKit.tab_button(_LED_TABS[t], t == _led_tab))
	for t in _led_tab_row.get_child_count():
		var b: Button = _led_tab_row.get_child(t)
		b.pressed.connect(_ledger_set_tab.bind(t))
	v.add_child(UiKit.rule())
	# The sortable table (header buttons + cells), rebuilt each refresh.
	var sc := ScrollContainer.new()
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
	sc.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	v.add_child(sc)
	_led_grid = GridContainer.new()
	_led_grid.add_theme_constant_override("h_separation", 18)
	_led_grid.add_theme_constant_override("v_separation", 5)
	_led_grid.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	sc.add_child(_led_grid)


func _ledger_set_tab(t: int) -> void:
	_led_tab = t
	_led_sort = 0
	_led_asc = true
	for i in _led_tab_row.get_child_count():
		(_led_tab_row.get_child(i) as Button).set_pressed_no_signal(i == t)


func _ledger_sort_by(col: int) -> void:
	if col == _led_sort:
		_led_asc = not _led_asc
	else:
		_led_sort = col
		_led_asc = true


## Column titles for ledger tab `t`.
func _ledger_columns(t: int) -> Array:
	match t:
		0: return ["Ship", "Location", "Status", "Battles", "Won", "Age (d)"]
		1: return ["Body", "Mineral", "Output/t", "On outpost"]
		2: return ["Body", "Level", "Status", "Tribute/t"]
		3: return ["Colony", "Faction", "Control", "Dev", "Supplies"]
		_: return ["Market", "Body", "Yours"]


## The rows (each an Array of cells; ints sort numerically, strings lexically) for tab `t`.
func _ledger_rows(t: int) -> Array:
	var rows: Array = []
	match t:
		0:
			for i in sim.fleet_size():
				rows.append([String(sim.ship_name(i)), String(sim.ship_location(i)),
					("In transit" if sim.ship_in_transit(i) else "Docked"),
					sim.ship_battles(i), sim.ship_battles_won(i), sim.ship_age(i)])
		1:
			for i in sim.miner_count():
				var mb := sim.miner_body(i)
				var boosted: bool = sim.outpost_level_at(mb) > 0
				rows.append([String(sim.body_name(mb)), String(sim.body_mineral_name(mb)),
					(3 if boosted else 2), ("✦ +50%" if boosted else "—")])
		2:
			for i in sim.outpost_count():
				var ob := sim.outpost_body(i)
				var lvl: int = sim.outpost_level(i)
				var bdays: int = sim.outpost_build_days(ob)
				var st := "Building (%dd)" % bdays if bdays >= 0 else "Operational"
				var trib: int = 0 if bdays >= 0 else lvl * 30
				rows.append([String(sim.body_name(ob)), lvl, st, trib])
		3:
			for i in sim.colony_count():
				var ctl := "Owned" if sim.colony_controlled(i) else _faction_name(sim.colony_faction(i))
				rows.append([String(sim.colony_name(i)), _faction_name(sim.colony_faction(i)),
					ctl, sim.colony_dev(i), String(sim.commodity_name(sim.colony_specialty(i)))])
		_:
			for i in _visible_market_count():
				rows.append([String(sim.market_name(i)), String(sim.body_name(sim.market_body(i))),
					("Yours" if sim.market_is_owned(i) else "—")])
	return rows


## Type-aware less-than for sorting a ledger column (numbers numerically, else lexically).
func _ledger_less(a: Variant, b: Variant, asc: bool) -> bool:
	var r := 0
	if (a is int or a is float) and (b is int or b is float):
		r = -1 if a < b else (1 if a > b else 0)
	else:
		var sa := str(a)
		var sb := str(b)
		r = -1 if sa < sb else (1 if sa > sb else 0)
	return r < 0 if asc else r > 0


## The Mysteries tab — the slow-burn authored threads, *discovered through play* (the Expanse
## model: at the start nobody knows of the ring or the protomolecule). Stated as N/7, escalating.
func _refresh_mysteries() -> void:
	for c in _led_grid.get_children():
		c.queue_free()
	_led_grid.columns = 1
	var beats: int = sim.gate_beats()
	if beats <= 0:
		var none := UiKit.label("The system holds its secrets.\n\nYour operations have turned up nothing yet beyond the ordinary trade of Sol — no one speaks of a ring beyond Pluto, and no one has ever heard the word \"protomolecule.\"\n\nKeep working the system. What's out there does not stay hidden forever.", 14, UiKit.TEXT_DIM)
		none.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		none.custom_minimum_size = Vector2(720, 0)
		_led_grid.add_child(none)
		return
	var head := UiKit.label("🜲  The Ring-Gate  —  %d / 7 fragments uncovered" % beats, 17, UiKit.GOLD)
	_led_grid.add_child(head)
	_led_grid.add_child(UiKit.gauge(float(beats) / 7.0, UiKit.GOLD, 360, 9))
	var beat := UiKit.label("✦ %s" % String(sim.gate_lore()), 14, UiKit.TEXT)
	beat.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	beat.custom_minimum_size = Vector2(720, 0)
	_led_grid.add_child(beat)
	var foot_text := "The picture is whole. Whatever waits beyond the ring, you know now that it is there — and that it is waking." if beats >= 7 else "Each operation, each salvaged wreck, each rung you climb turns up another fragment. The mystery deepens the higher you reach."
	var foot := UiKit.label(foot_text, 12, UiKit.ACCENT if beats >= 7 else UiKit.TEXT_DIM)
	foot.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	foot.custom_minimum_size = Vector2(720, 0)
	_led_grid.add_child(foot)


func _refresh_ledger() -> void:
	_led_summary.text = "💰 %s cr   ·   ◈ %d ships   ·   ⛏ %d miners   ·   ⚑ %d outposts   ·   ✦ %d holdings" % [
		_commas(sim.credits()), sim.fleet_size(), sim.miner_count(), sim.outpost_count(), sim.holding_count()]
	if _led_tab == _LED_MYSTERIES:
		_refresh_mysteries()
		return
	var cols := _ledger_columns(_led_tab)
	var rows := _ledger_rows(_led_tab)
	var sortc := clampi(_led_sort, 0, cols.size() - 1)
	rows.sort_custom(func(x, y): return _ledger_less(x[sortc], y[sortc], _led_asc))
	for c in _led_grid.get_children():
		c.queue_free()
	_led_grid.columns = cols.size()
	# Header row — clickable sort buttons.
	for i in cols.size():
		var arrow := ""
		if i == sortc:
			arrow = "  ▲" if _led_asc else "  ▼"
		var hb := UiKit.tab_button(cols[i] + arrow, i == sortc)
		hb.pressed.connect(_ledger_sort_by.bind(i))
		_led_grid.add_child(hb)
	# Data rows.
	for row in rows:
		for ci in row.size():
			var cell: Variant = row[ci]
			var txt := _commas(int(cell)) if (cell is int and absi(int(cell)) >= 1000) else str(cell)
			var lbl := UiKit.label(txt, 12, UiKit.TEXT if ci == 0 else UiKit.TEXT_DIM)
			_led_grid.add_child(lbl)
	if rows.is_empty():
		_led_grid.add_child(UiKit.label("(none yet)", 12, UiKit.TEXT_DIM))


# Per-faction bbcode colours (Earth blue · Mars red · Belt ochre · Independent grey).
const _FAC_COL := ["#5b8fd6", "#d6604f", "#d6a24f", "#9fb0c0"]


## A compact influence gauge for contested colony `i`: a 20-cell bar coloured by which
## power holds each slice (so a glance shows whose grip is tightening — Earth vs Mars).
func _influence_bar(i: int) -> String:
	var total := 0
	for f in 4:
		total += sim.contested_influence(i, f)
	if total <= 0:
		return ""
	var cells := 20
	var bar := ""
	var pcts := PackedStringArray()
	for f in 4:
		var inf: int = sim.contested_influence(i, f)
		var n: int = int(round(float(inf) * cells / float(total)))
		for _k in n:
			bar += "[color=%s]▰[/color]" % _FAC_COL[f]
		pcts.append("[color=%s]%s %d%%[/color]" % [_FAC_COL[f], _faction_name(f), inf * 100 / total])
	return bar + "   " + "  ".join(pcts)


func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_PAUSED or what == NOTIFICATION_WM_WINDOW_FOCUS_OUT:
		speed_idx = 0


func _refresh() -> void:
	_refresh_chrome()
	_refresh_decisions()
	match view:
		V_SYSTEMS:
			_refresh_systems()
		V_FLEET:
			_refresh_fleet()
		V_BUILD:
			_refresh_build()
		V_MARKET:
			_refresh_market()
		V_EMPIRE:
			_refresh_empire()
		V_RESEARCH:
			_refresh_research()
		V_LEDGER:
			_refresh_ledger()
		V_DIPLOMACY:
			_refresh_diplomacy()
	_flash_rect.color.a = flash * 0.5
	_ascend_rect.color.a = ascend_flash * 0.5
	if pc_mode:
		_help.text = "PC ·  [Space/1/2/3] time   [F1–F4/F6] views   wheel: zoom · drag: pan · Shift-drag/[,.]: rotate · click: focus   [↑↓] commodity [←→] market   [B]uy [S]ell   [Tab] target [I]nterdict [E]xploit   [N]ew ship   [F5]/[F9] save·load   [F11] fullscreen   [F8] touch mode"
	else:
		_help.text = "Touch ·  [Space/1/2/3] time   pinch: zoom · drag/twist: rotate · tap: focus   [B]uy [S]ell   [I]nterdict [E]xploit   [F8] PC mode"


## Per-day resource rates for the top bar — sampled once per in-game day (6 ticks) as the
## delta from the previous day's snapshot, so you can see your economy's pulse at a glance.
func _update_resource_rates() -> void:
	var day: int = int(sim.tick()) / 6
	var cur := {"credits": sim.credits(), "ore": sim.cargo(_idx_ore),
		"fuel": sim.cargo(_idx_fuel), "influence": sim.influence()}
	if _rate_snap.is_empty():
		_rate_snap = cur
		_rate_day = day
	elif day != _rate_day:
		var days: int = maxi(1, day - _rate_day)   # at fast-forward many days pass per frame
		for key in cur:
			var d: int = (int(cur[key]) - int(_rate_snap[key])) / days
			var lbl: Label = {"credits": _rate_credits, "ore": _rate_ore,
				"fuel": _rate_fuel, "influence": _rate_influence}[key]
			lbl.text = ("+%s/d" % _commas(d)) if d >= 0 else ("%s/d" % _commas(d))
			lbl.add_theme_color_override("font_color", UiKit.GOOD if d > 0 else (UiKit.BAD if d < 0 else UiKit.TEXT_DIM))
		_rate_snap = cur
		_rate_day = day


func _refresh_chrome() -> void:
	var sp := "‖ PAUSED" if speed_idx == 0 else "▶ %d×" % int(SPEEDS[speed_idx])
	_title.text = "%s      %s" % [VIEW_TITLE[view], sp]
	_title.add_theme_color_override("font_color", UiKit.GOLD if speed_idx == 0 else UiKit.TEXT_DIM)
	_date.text = _date_string()
	_res_credits.text = _commas(sim.credits())
	_res_ore.text = _commas(sim.cargo(_idx_ore))
	_res_fuel.text = _commas(sim.cargo(_idx_fuel))
	_res_crew.text = str(sim.trained_crew())
	_res_influence.text = str(sim.influence())
	_update_resource_rates()
	# Alert ticker: the most recent act-now shortage, if any.
	var ticker := ""
	for a in sim.alert_count():
		if sim.alert_is_act_now(a):
			ticker = "[!] %s" % String(sim.alert_message(a))
			break
	_alert_ticker.text = ticker


func _date_string() -> String:
	var day := 15 + sim.tick() / 6
	var year := 2142 + day / 360
	var doy := day % 360
	return "%04d.%02d.%02d" % [year, doy / 30 + 1, doy % 30 + 1]


func _commas(n: int) -> String:
	var s := str(absi(n))
	var out := ""
	var c := 0
	for i in range(s.length() - 1, -1, -1):
		out = s[i] + out
		c += 1
		if c % 3 == 0 and i > 0:
			out = "," + out
	return ("-" if n < 0 else "") + out


## The display name for a body kind (0 Star … 7 Asteroid).
func _body_kind_name(k: int) -> String:
	match k:
		0: return "Star"
		1: return "Planet"
		2: return "Gas Giant"
		3: return "Dwarf Planet"
		4: return "Moon"
		5: return "Ring-Gate"
		6: return "Far-Side World"
		7: return "Asteroid"
		_: return "Body"


## Re-centre the right panel on the tapped object: its identity + a contextual detail block.
func _refresh_object_panel() -> void:
	var fb := _focus_body
	if fb <= 0:
		_sys_title.text = String(sim.market_name(sel_market))
		_sys_sub.text = "Trading Node  ·  Sol System"
		_sys_object.visible = false
		return
	_sys_title.text = String(sim.body_name(fb))
	var kind := _body_kind_name(sim.body_kind(fb))
	var ci := _colony_index_for_body(fb)
	var coni := _contested_index_for_body(fb)
	var sub := kind
	var detail := ""
	if coni >= 0:
		# A contested hub — show the powers' grip (the influence gauge) + your standing.
		sub = "Contested hub  ·  led by %s" % _faction_name(sim.contested_leader(coni))
		detail = "[color=#cfd8e0]The great powers fight over this hub.[/color]\n" + _influence_bar(coni)
		var pi: int = sim.contested_player_influence(coni)
		var thr: int = sim.contested_claim_threshold()
		var pcol := "#78e68c" if pi >= thr else "#e6c860"
		detail += "\n[color=#7a8696]your standing[/color] [color=%s]%d/%d[/color]" % [pcol, pi, thr]
	elif ci >= 0 and sim.colony_controlled(ci):
		# Your colony — development state.
		sub = "Your colony  ·  %s" % kind
		var dev: int = sim.colony_dev(ci)
		var cbdays: int = sim.colony_build_days(ci)
		detail = "[color=#78e68c]✦ Owned colony[/color] — development [color=#e6c860]L%d[/color]" % dev
		if cbdays > 0:
			detail += "  [color=#e6c860](⚙ developing → L%d: %d days)[/color]" % [dev, cbdays]
		else:
			var dcost: int = sim.develop_cost(ci)
			detail += "  [color=#7a8696](→L%d: %s cr, ~180 days)[/color]" % [dev + 1, _commas(dcost)] if dcost >= 0 else "  [color=#7a8696](max)[/color]"
	elif ci >= 0:
		sub = "%s colony  ·  %s" % [_faction_name(sim.colony_faction(ci)), kind]
		if sim.colony_acquirable(ci):
			detail = "[color=#cfd8e0]Independent[/color] — buy out for [color=#e6c860]%s cr[/color] (a mid-game goal)." % _commas(sim.colony_acquire_cost(ci))
	elif sim.shipyard_tier() > 0 and fb == sim.shipyard_body():
		sub = "Your shipyard  ·  %s" % kind
		detail = "[color=#9fd8ff]Shipyard[/color] — builds up to [color=#cfd8e0]%s[/color]" % String(sim.shipyard_max_hull())
	elif sim.outpost_level_at(fb) > 0:
		# Your outpost — the body-built station base.
		sub = "Your outpost  ·  %s" % kind
		var lvl: int = sim.outpost_level_at(fb)
		var bdays: int = sim.outpost_build_days(fb)
		if bdays >= 0:
			# Still under construction — the slow "set it and wait" build (~180 days).
			detail = "[color=#e6c860]⚙ Under construction[/color] — [color=#cfd8e0]%d days[/color] until it comes online (L%d)." % [bdays, lvl]
		else:
			var rank: int = sim.outpost_rank(fb)
			var rank_name := String(sim.outpost_rank_name(fb))
			var glyphs: Array = ["⚑", "★", "✦", "♔"]
			var glyph: String = glyphs[clampi(rank, 0, 3)]
			sub = "Your %s  ·  %s" % [rank_name.to_lower(), kind]
			detail = "[color=#78e68c]%s %s[/color] — level [color=#e6c860]L%d[/color]" % [glyph, rank_name, lvl]
			var ocost: int = sim.outpost_develop_cost(fb)
			detail += "  [color=#7a8696](→L%d: %s cr, ~120 days)[/color]" % [lvl + 1, _commas(ocost)] if ocost >= 0 else "  [color=#7a8696](max)[/color]"
			# Facilities — the progression rungs (Mine = produces raw goods).
			var facs := PackedStringArray()
			for fk in [[0, "Mine"], [1, "Storage"], [2, "Hangar"]]:
				if sim.outpost_has_facility(fb, fk[0]):
					facs.append("[color=#78e68c]✓ %s[/color]" % fk[1])
				else:
					facs.append("[color=#7a8696]%s[/color]" % fk[1])
			detail += "\nFacilities: %s" % "  ".join(facs)
			# Per-asset inventory: the outpost's local store of the body's mineral (§10).
			if sim.outpost_has_facility(fb, 0):
				var stored: int = sim.outpost_stored(fb)
				var scap: int = sim.outpost_store_cap(fb)
				var ship := "[color=#78e68c]→ shipped to your warehouse[/color]" if sim.outpost_has_facility(fb, 2) else "[color=#e6a060]⚠ no Hangar — stuck on-site[/color]"
				detail += "\nStored %s: [color=#cfd8e0]%s/%s[/color]  %s" % [String(sim.body_mineral_name(fb)), _commas(stored), _commas(scap), ship]
			var pop: int = sim.outpost_population(fb)
			var pop_need: int = sim.outpost_promote_population(fb)
			var next_rank := String(sim.outpost_next_rank_name(fb))
			var has_ice: bool = sim.cargo(_idx_water) >= 1
			detail += "\nPopulation: [color=#cfd8e0]%d[/color] %s" % [pop, ("[color=#78e68c](↑ growing — fed Ice)[/color]" if has_ice else "[color=#e6a060](stalled — supply Ice to grow)[/color]")]
			detail += "  ·  yield ×%d" % int(([1, 3, 6, 12] as Array)[clampi(rank, 0, 3)])
			if rank >= 3:
				detail += "\n[color=#9fd8ff]♔ Your Capital — the seat of your power.[/color]"
			elif not sim.outpost_has_facility(fb, 0):
				detail += "\n[color=#e6a060]⚠ No Mine — produces no raw goods yet (only tribute).[/color]"
			elif sim.can_promote_outpost(fb):
				detail += "\n[color=#78e68c]★ Ready to promote to a %s (★ verb below).[/color]" % next_rank
			elif pop < pop_need:
				detail += "\n[color=#7a8696]To %s: max level + all facilities + %d/%d population.[/color]" % [next_rank, pop, pop_need]
			if sim.miner_at(fb):
				detail += "\n[color=#f0a030]⛏ miner here gets +50% (hauls to the outpost)[/color]"
	elif sim.can_mine_body(fb):
		if sim.miner_at(fb):
			detail = "[color=#f0a030]⛏ Miner working here[/color] — extracting [color=#cfd8e0]%s[/color]" % String(sim.body_mineral_name(fb))
		else:
			detail = "Mineable %s — yields [color=#cfd8e0]%s[/color]" % [kind.to_lower(), String(sim.body_mineral_name(fb))]
	_sys_sub.text = sub
	_sys_object.text = detail
	_sys_object.visible = detail != ""


func _refresh_systems() -> void:
	# The panel re-centres on whatever you tapped — the object is the subject, not just the
	# market. Identity + a contextual detail block (yield / miner / influence / development).
	_refresh_object_panel()
	# System census (mockup card): static body counts (cached) + your live holdings.
	if _census_static == "":
		var planets := 0
		var asteroids := 0
		var moons := 0
		for b in sim.body_count():
			match sim.body_kind(b):
				1, 2, 3: planets += 1   # planet / gas giant / dwarf
				4: moons += 1
				7: asteroids += 1
		_census_static = "%d worlds · %d moons · %d asteroids" % [planets, moons, asteroids]
	_sys_census.text = "Star G2V  ·  %s  ·  your: %d outpost(s), %d holding(s)" % [
		_census_static, sim.outpost_count(), sim.holding_count()]
	var holdings: int = sim.holding_count()
	var cap: int = sim.admin_capacity()
	# The expansion spine (E6): lead with the empire rank, then the holdings/cap.
	var hold_txt := "%s · Holdings %d/%d" % [String(sim.empire_rank()), holdings, cap]
	if sim.admin_strain() > 0:
		# Overextended (E2): flag the strain + the income hit.
		hold_txt = "⚠ Holdings %d/%d (strained · %d%%)" % [holdings, cap, sim.holdings_efficiency_pct()]
	# Influence (E4): the statecraft resource for diplomatic annexation.
	hold_txt += "   ·   Influence %d" % sim.influence()
	# Coalition alarm (E3): warn as the great powers turn against your expansion.
	if sim.coalition_active():
		hold_txt += "   ·   ⚠ COALITION (alarm %d)" % sim.coalition_alarm()
	elif sim.coalition_alarm() >= 300:
		hold_txt += "   ·   inners wary (%d)" % sim.coalition_alarm()
	# The DEFEND HOLDINGS verb lights only while a coalition strike presses (E3).
	if _defend_holdings_btn:
		_defend_holdings_btn.visible = sim.coalition_strike_pending()
	# Object-contextual verbs — the tapped body is the centre; only what it affords appears.
	if _mine_btn:
		var fb := _focus_body
		var ci := _colony_index_for_body(fb)
		var coni := _contested_index_for_body(fb)
		var owned: bool = ci >= 0 and sim.colony_controlled(ci)
		var has_outpost: bool = fb > 0 and sim.outpost_level_at(fb) > 0
		_mine_btn.visible = fb > 0 and sim.can_mine_body(fb) and not sim.miner_at(fb)
		_withdraw_btn.visible = fb > 0 and sim.miner_at(fb)
		var outpost_building: bool = has_outpost and sim.outpost_build_days(fb) >= 0
		var outpost_ready: bool = has_outpost and not outpost_building
		_outpost_btn.visible = fb > 0 and ci < 0 and sim.can_found_outpost(fb)
		_dev_outpost_btn.visible = has_outpost and not outpost_building  # can't develop mid-build
		# Facilities — only on an operational *outpost* (rank 0) that lacks each.
		var is_plain_outpost: bool = outpost_ready and sim.outpost_rank(fb) == 0
		_fac_mine_btn.visible = is_plain_outpost and not sim.outpost_has_facility(fb, 0)
		_fac_storage_btn.visible = is_plain_outpost and not sim.outpost_has_facility(fb, 1)
		_fac_hangar_btn.visible = is_plain_outpost and not sim.outpost_has_facility(fb, 2)
		_promote_btn.visible = outpost_ready and sim.can_promote_outpost(fb)
		if _promote_btn.visible:
			_promote_btn.text = "★ Promote to %s" % String(sim.outpost_next_rank_name(fb))
		_build_btn.visible = fb > 0 and ci < 0 and not has_outpost and sim.can_found_shipyard_at(fb)
		_expand_btn.visible = sim.shipyard_tier() > 0 and fb == sim.shipyard_body()
		_court_btn.visible = coni >= 0 and not owned
		_claim_btn.visible = coni >= 0 and not owned and sim.contested_claimable(coni)
		# Buy out an independent colony by clicking it (not a contested hub — those use Claim).
		_acquire_ctx_btn.visible = ci >= 0 and coni < 0 and sim.colony_acquirable(ci)
		_develop_btn.visible = owned and sim.colony_build_days(ci) == 0  # not mid-development
		_send_btn.visible = fb > 0 and sim.fleet_size() > 0
	var mtxt := ""
	if sim.miner_count() > 0:
		mtxt = "   ·   ⛏ %d miner(s)" % sim.miner_count()
	if _focus_body > 0 and sim.body_kind(_focus_body) != 5:
		if sim.can_mine_body(_focus_body):
			mtxt += "   ·   mine %s here yields %s" % [String(sim.body_name(_focus_body)), String(sim.body_mineral_name(_focus_body))]
		else:
			mtxt += "   ·   %s: off-limits to miners (Earth/Mars AO)" % String(sim.body_name(_focus_body))
	_sys_status.text = "Status: Online   ·   %s   ·   %s%s" % [sim.tier_name(), hold_txt, mtxt]
	# Resources — the station's on-hand stock (what this node holds to trade).
	for c in _sys_resources.get_children():
		c.queue_free()
	for ci in [_idx_water, _idx_ore, _idx_fuel]:
		var row := HBoxContainer.new()
		row.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var nm := UiKit.label(String(sim.commodity_name(ci)), 12, UiKit.TEXT_DIM)
		nm.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		row.add_child(nm)
		row.add_child(UiKit.label(_commas(sim.stock(sel_market, ci)), 12, UiKit.TEXT))
		_sys_resources.add_child(row)
	# Active queues — standing routes/stations as live "projects".
	for c in _sys_queues.get_children():
		c.queue_free()
	var any := false
	for i in mini(sim.route_count(), 3):
		_sys_queues.add_child(_queue_row("Route", String(sim.route_desc(i))))
		any = true
	for i in mini(sim.station_count(), 2):
		_sys_queues.add_child(_queue_row("Station", String(sim.station_desc(i))))
		any = true
	if not any:
		_sys_queues.add_child(UiKit.label("— no active orders —", 11, UiKit.TEXT_DIM))
	# Standing-order toggle state (sync without re-emitting).
	_tg_patrol.set_pressed_no_signal(sim.patrol_enabled())
	_tg_research.set_pressed_no_signal(sim.auto_research_enabled())
	_tg_pause.set_pressed_no_signal(auto_pause)
	# Active opening mission (§16), the NOW goal, and the gate mystery (§0.1).
	var mt := String(sim.mission_title())
	if mt != "":
		_sys_mission.text = "%s  ·  %s" % [mt, String(sim.mission_hint())]
	else:
		_sys_mission.text = "Tutorial complete — the company is yours to run."
	_sys_now.text = "NOW: %s (%d/%d)" % [
		sim.now_goal(), sim.now_goal_progress(), sim.now_goal_target()]
	# (The gate-transit endgame has been removed — the focus is the early-game trade/
	#  management sim; the ring stays a slow-burn mystery in the LEDGER's Mysteries tab.)
	# Feed.
	var feed := ""
	for a in mini(sim.alert_count(), 3):
		var msg := String(sim.alert_message(a))
		if sim.alert_is_act_now(a):
			feed += "[color=#ff6a4d][!] %s[/color]\n" % msg
		else:
			feed += "[color=#9fb0c0]· %s[/color]\n" % msg
	if feed == "":
		feed = "[color=#6f8a93]All quiet.[/color]"
	_feed.text = feed


func _queue_row(kind: String, desc: String) -> Control:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 1)
	v.add_child(UiKit.label("%s — %s" % [kind, desc], 11, UiKit.TEXT))
	# A lively pseudo-progress so the queue reads as "working" (transit/loading).
	var p := UiKit.gauge(0.35 + 0.5 * absf(sin(float(sim.tick()) * 0.05 + desc.length())), UiKit.ACCENT, 280, 6)
	v.add_child(p)
	return v


func _refresh_fleet() -> void:
	# Refit bay readout (Phase B): target ship + chosen model per kind.
	if _refit_lbl.has("target"):
		var fsz0 := sim.fleet_size()
		if fsz0 > 0:
			_refit_target = clampi(_refit_target, 0, fsz0 - 1)
			var nm := String(sim.ship_name(_refit_target))
			if sim.ship_refitting(_refit_target):
				nm += " ⏳"
			(_refit_lbl["target"] as Label).text = nm
		else:
			(_refit_lbl["target"] as Label).text = "—"
		for kind in [0, 1, 2]:
			if _refit_lbl.has(kind):
				var mid := _refit_model_id(kind)
				(_refit_lbl[kind] as Label).text = "—" if mid < 0 else String(sim.weapon_name(mid))
	for c in _fleet_grid.get_children():
		c.queue_free()
	for h in ["SHIP", "STATUS", "TYPE", "LOCATION", "ASSIGNMENT", "FUEL/AMMO"]:
		_fleet_grid.add_child(UiKit.label(h, 10, UiKit.ACCENT))
	var shown := 0
	var fsz := sim.fleet_size()
	# Warships — real position + fuel from the sim (§6).
	if fleet_tab != 2:   # not "single ships only"
		for i in fsz:
			var moving := sim.ship_in_transit(i)
			if fleet_tab == 3 and moving:   # IDLE tab: only docked ships
				continue
			var loc := String(sim.ship_location(i))
			var fuel := float(sim.ship_fuel_bp(i)) / 10000.0
			var assign := "En route" if moving else ("Refuel needed" if fuel < 0.12 else "Docked")
			var fcol: Color = UiKit.GOOD if fuel > 0.35 else (UiKit.ACCENT if fuel > 0.12 else UiKit.BAD)
			var crew := "Capt. %s · %s" % [String(sim.ship_captain(i)), String(sim.ship_trait(i))]
			_fleet_row(String(sim.ship_name(i)), fuel > 0.05, crew, loc, assign, fuel, fcol)
			shown += 1
	# Freighters run the standing routes (§4) — positional on their lanes now (§6).
	if fleet_tab == 0 or fleet_tab == 2 or fleet_tab == 3:
		var fr := sim.freighters()
		var flying := sim.freighter_count()
		for i in fr:
			var en_route := i < flying
			if fleet_tab == 3 and en_route:   # IDLE tab: only docked freighters
				continue
			var loc := String(sim.freighter_trip(i)) if en_route else "Ceres Yards"
			var assign := ("In transit %d%%  ·  %d fuel" % [sim.freighter_progress(i), sim.freighter_fuel(i)]) if en_route else (
				String(sim.route_status()) if sim.route_count() > 0 else "Idle")
			_fleet_row("Logistics Wing %d" % (i + 1), true, "Freighter", loc, assign,
				1.0, UiKit.GOOD if en_route else UiKit.ACCENT)
			shown += 1
	if shown == 0:
		for _i in 6:
			_fleet_grid.add_child(UiKit.label("—" if _i == 0 else "", 12, UiKit.TEXT_DIM))
	var flag_capt := ""
	if fsz > 0:
		var fi: int = sim.flagship_index()
		if fi >= 0:
			flag_capt = "  ·  Capt. %s (%s)" % [String(sim.ship_captain(fi)), String(sim.ship_trait(fi))]
	_fleet_count.text = "%d ships  ·  flagship: %s%s" % [
		fsz + sim.freighters(), String(sim.flagship_name()) if fsz > 0 else "—", flag_capt]
	if _corp_lbl:
		_corp_lbl.text = String(sim.corp_name())
		_corp_lbl.add_theme_color_override("font_color", sim.corp_livery_color())
	if _combat_lbl:
		var bands := ["close", "medium", "long"]
		var tgt := "wounded" if sim.combat_target() == 1 else "biggest"
		var rt := sim.combat_retreat()
		var on_station: int = sim.warships_on_station()
		var fire := "hot" if sim.combat_aggressive() else "disciplined"
		_combat_lbl.text = "range %s · target %s · retreat %s · fire %s · %d on station" % [
			bands[combat_band], tgt, ("never" if rt == 0 else "%d%%" % rt), fire, on_station]


func _fleet_row(ship: String, ok: bool, type: String, loc: String, assign: String, fuel: float, fuelcol: Color) -> void:
	_fleet_grid.add_child(UiKit.label(ship, 12, UiKit.TEXT_HI))
	_fleet_grid.add_child(UiKit.label("✓" if ok else "•", 12, UiKit.GOOD if ok else UiKit.TEXT_DIM))
	_fleet_grid.add_child(UiKit.label(type, 12, UiKit.TEXT_DIM))
	_fleet_grid.add_child(UiKit.label(loc, 12, UiKit.TEXT))
	_fleet_grid.add_child(UiKit.label(assign, 12, UiKit.TEXT))
	_fleet_grid.add_child(UiKit.gauge(fuel, fuelcol, 90, 8))


func _refresh_build() -> void:
	# Slow turntable so the procedural hull shows off (§24).
	if _ship_pivot:
		_ship_pivot.rotate_y(get_process_delta_time() * 0.5)
	_refresh_arsenal()
	# Shipyard status (Phase B+): tier + what it can build, or the Tycho fallback.
	if _yard_lbl:
		var tier := sim.shipyard_tier()
		if tier <= 0:
			var corv := "corvettes ✓" if sim.can_buy_corvettes() else "corvettes (need OPA standing)"
			_yard_lbl.text = "None — Tycho sells civilians + %s.\nFound a yard (60,000 cr) to build warships." % corv
		elif sim.shipyard_build_days() > 0:
			_yard_lbl.text = "⚙ Tier %d under construction — %d days until it can lay down hulls." % [tier, sim.shipyard_build_days()]
		else:
			var ec := sim.expand_shipyard_cost()
			var more := "  ·  expand → %s cr" % _commas(ec) if ec >= 0 else "  ·  max tier"
			_yard_lbl.text = "Tier %d — builds up to %s%s" % [tier, String(sim.shipyard_max_hull()), more]
	# Designer (A2): step values + live fit stats.
	if _des_vals.has("pdc"):
		(_des_vals["pdc"] as Label).text = "%d/%d" % [_des_pdc, shipyard.pdc_mounts(build_pick)]
		(_des_vals["torp"] as Label).text = "%d/%d" % [_des_torp, shipyard.torpedo_mounts(build_pick)]
		(_des_vals["rail"] as Label).text = "%d/%d" % [_des_rail, shipyard.railgun_mounts(build_pick)]
		(_des_vals["burn"] as Label).text = "%d" % _des_burn
	# Per-slot model picks: show the chosen model's name (or "—" if none owned).
	for kind in [0, 1, 2]:
		if _des_model_lbl.has(kind):
			var mid := _des_model_id(kind)
			(_des_model_lbl[kind] as Label).text = "—" if mid < 0 else String(sim.weapon_name(mid))
	if _design_lbl:
		var fit := shipyard.evaluate_fit(
			build_pick,
			_des_model_id(0), _des_pdc,
			_des_model_id(1), _des_torp,
			_des_model_id(2), _des_rail,
			_des_burn)
		var ok: bool = fit.get("ok", true)
		_design_lbl.text = "Design:  alpha %d   ·   Δv %d   ·   mobility %d   ·   power %d/%d" % [
			int(fit.get("alpha", 0)), int(fit.get("delta_v", 0)), int(fit.get("mobility", 0)),
			int(fit.get("power_used", 0)), int(fit.get("power_cap", 0))]
		_design_lbl.add_theme_color_override("font_color", UiKit.TEXT if ok else UiKit.BAD)
	# Grey out hulls you can't source yet (no shipyard / OPA standing), and the COMMISSION
	# button for the selected one — civilians (the trader) are always buildable below.
	if _build_list:
		for i in _build_list.get_child_count():
			var hb2: Button = _build_list.get_child(i)
			var buildable: bool = sim.can_build_hull(i)
			hb2.disabled = not buildable
			hb2.add_theme_color_override("font_color", UiKit.TEXT if buildable else UiKit.TEXT_DIM)
	if _commission_btn:
		_commission_btn.disabled = not sim.can_build_hull(build_pick)
	var nm := String(shipyard.class_name(build_pick))
	_build_caption.text = nm
	_build_stats.text = "railguns %d   ·   alpha %d   ·   Δv %d   ·   mobility %d" % [
		shipyard.railguns(build_pick), shipyard.alpha(build_pick),
		shipyard.delta_v(build_pick), shipyard.mobility(build_pick)]
	# Synthesised build cost scaling with class (the sim charges credits + crew).
	var tier := build_pick + 1
	_build_cost.text = "Metal %s   ·   Electronics %s   ·   Crew %d   ·   ~%d days" % [
		_commas(2000 * tier), _commas(800 * tier), 12 * tier, 6 * tier]
	# §7d bill of materials for the assemble-from-parts path, lit green if held.
	if _bom_lbl:
		var have: bool = sim.can_assemble_ship(build_pick)
		_bom_lbl.text = "Assemble from: %s%s" % [
			String(sim.ship_bom_desc(build_pick)), "  ✓ in stock" if have else ""]
		_bom_lbl.add_theme_color_override("font_color", UiKit.GOOD if have else UiKit.TEXT_DIM)
	# Queue = active standing orders, presented as "projects".
	for c in _build_queue.get_children():
		c.queue_free()
	var rows := 0
	for i in mini(sim.route_count(), 3):
		_build_queue.add_child(_queue_row("Route", String(sim.route_desc(i))))
		rows += 1
	for i in mini(sim.station_count(), 2):
		_build_queue.add_child(_queue_row("Refinery", String(sim.station_desc(i))))
		rows += 1
	if rows == 0:
		_build_queue.add_child(UiKit.label("Queue empty.", 11, UiKit.TEXT_DIM))


func _visible_market_count() -> int:
	# Far-side markets (§17) are contiguous at the end and hidden until the gate is
	# transited; the board, ticker, and selection cycle only the visible ones.
	if sim.far_side_revealed():
		return sim.market_count()
	var n := 0
	for m in sim.market_count():
		if not sim.market_is_far_side(m):
			n += 1
	return n


## Cycle the selected market (the trade cursor) — the MARKET view's ◀ ▶ controls.
func _cycle_market(d: int) -> void:
	var n := _visible_market_count()
	sel_market = (sel_market + d + n) % n


## Create a standing trade route from the selected market: sell the selected commodity into
## the market where it's dearest (with room). The route then auto-runs against the freighter pool.
func _create_route_from_sel() -> void:
	var best := -1
	var best_price := sim.price(sel_market, sel_comm)
	for m in _visible_market_count():
		if m == sel_market:
			continue
		var p := sim.price(m, sel_comm)
		if p > best_price:
			best_price = p
			best = m
	if best < 0:
		status = "%s is already dearest here — no profitable route." % String(sim.commodity_name(sel_comm))
		return
	sim.set_trade_route(sel_comm, sel_market, best, trade_qty, 1)
	status = "Route created: %s  %s → %s (auto-runs when a trader is free)." % [
		String(sim.commodity_name(sel_comm)), String(sim.market_name(sel_market)), String(sim.market_name(best))]


func _refresh_market() -> void:
	# Dynamic titles + the trade-route summary (the management readout).
	if _ticker_title:
		_ticker_title.text = ("Market Ticker  ·  %s" % String(sim.market_name(sel_market))).to_upper()
		_chart_title.text = ("Price History  ·  %s" % String(sim.market_name(sel_market))).to_upper()
		_mkt_sel_lbl.text = "%s  ·  %s" % [String(sim.market_name(sel_market)), String(sim.commodity_name(sel_comm))]
		var rc := sim.route_count()
		if rc > 0:
			var parts := PackedStringArray()
			for i in rc:
				parts.append(String(sim.route_desc(i)))
			_mkt_routes_lbl.text = "Routes (%d/%d): %s" % [rc, sim.route_cap(), "   ·   ".join(parts)]
		else:
			_mkt_routes_lbl.text = "No trade routes yet — pick a market + commodity (↑↓), then ＋ Create Route. Traders run them automatically."
	# Ticker grid.
	for c in _ticker_grid.get_children():
		c.queue_free()
	for h in ["COMMODITY", "PRICE", "STOCK", "BEST SPREAD", "TREND"]:
		_ticker_grid.add_child(UiKit.label(h, 10, UiKit.ACCENT))
	for ci in sim.commodity_count():
		var price := sim.price(sel_market, ci)
		# Best spread = the dearest other market vs. here (the arbitrage you'd work).
		var best := 0
		var best_m := sel_market
		for m in _visible_market_count():
			if m == sel_market:
				continue
			var d := sim.price(m, ci) - price
			if d > best:
				best = d
				best_m = m
		var spread_pct := 0
		if price > 0:
			spread_pct = best * 100 / price
		_ticker_grid.add_child(UiKit.label(String(sim.commodity_name(ci)), 12,
			UiKit.ACCENT if ci == sel_comm else UiKit.TEXT_HI))
		_ticker_grid.add_child(UiKit.label(_commas(price), 12, UiKit.GOLD))
		_ticker_grid.add_child(UiKit.label(_commas(sim.stock(sel_market, ci)), 12, UiKit.TEXT))
		var spread_lbl := ("+%d%% → %s" % [spread_pct, sim.market_name(best_m)]) if best > 0 else "—"
		_ticker_grid.add_child(UiKit.label(spread_lbl, 12, UiKit.GOOD if best > 0 else UiKit.TEXT_DIM))
		_ticker_grid.add_child(UiKit.label("▲" if best > 0 else "▼", 12,
			UiKit.GOOD if best > 0 else UiKit.BAD))
	# Flow graph — one arrow per in-flight hauler, tagged by destination market.
	var flows: Array = []
	for h in mini(sim.hauler_count(), 8):
		# Map the hauler's endpoints to the nearest markets for the schematic.
		var src := _nearest_market(sim.hauler_x(h), sim.hauler_y(h))
		var dst := _nearest_market(sim.hauler_dest_x(h), sim.hauler_dest_y(h))
		if src != dst:
			flows.append({"from": src, "to": dst, "label": ""})
	_flow.set_flows(flows)


func _nearest_market(x: int, y: int) -> int:
	var best := 0
	var best_d := INF
	for m in _visible_market_count():
		var bb := sim.market_body(m)
		var d := Vector2(sim.body_x(bb) - x, sim.body_y(bb) - y).length()
		if d < best_d:
			best_d = d
			best = m
	return best


# ============================================================================
# INPUT
# ============================================================================

func _pick_hauler(pos: Vector2) -> bool:
	var best := -1
	var best_d := 22.0
	for hi in sim.hauler_count():
		# Pick against the *rendered* (smoothed) marker so tap + visual agree (§28).
		var mp: Vector3 = _hauler_pool[hi].position if hi < _hauler_pool.size() else _world3d(sim.hauler_x(hi), sim.hauler_y(hi))
		var d := _screen(mp).distance_to(pos)
		if d < best_d:
			best_d = d
			best = hi
	if best >= 0:
		selected = best
		status = "Targeted hauler %d — [I] to interdict." % best
		return true
	return false


func _pick_body(pos: Vector2) -> void:
	var best := -1
	var best_d := 40.0
	for b in sim.body_count():
		if sim.body_kind(b) == 5:
			continue
		var d := _screen(_world3d(sim.body_x(b), sim.body_y(b))).distance_to(pos)
		if d < best_d:
			best_d = d
			best = b
	if best < 0:
		return
	_focus_body = best
	_pan = Vector3.ZERO   # re-centre on the tapped body (clear any free-pan offset)
	_zoom = clampf(_zoom, ZOOM_MIN, 8.0) if sim.body_kind(best) == 2 else _zoom
	var note := ""
	for m in _visible_market_count():
		if sim.market_body(m) == best:
			sel_market = m
			note = " — trade cursor here"
	status = "Focus: %s%s." % [sim.body_name(best), note]


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventScreenTouch:
		if event.pressed:
			_touches[event.index] = event.position
			if _touches.size() == 1:
				_was_drag = false
			if _touches.size() >= 2:
				_was_multitouch = true
				_pinch_prev = _two_finger_dist()
				_pinch_ang_prev = _two_finger_angle()
		else:
			_touches.erase(event.index)
			if _touches.size() < 2:
				_pinch_prev = 0.0
		return
	if event is InputEventScreenDrag:
		if _touches.has(event.index):
			_touches[event.index] = event.position
		if _touches.size() >= 2:
			# Two fingers: pinch distance → zoom, twist angle → rotate (map gestures).
			var d := _two_finger_dist()
			if _pinch_prev > 0.0 and d > 0.0:
				_zoom = clampf(_zoom * (_pinch_prev / d), ZOOM_MIN, ZOOM_MAX)
			_pinch_prev = d
			var a := _two_finger_angle()
			_rotate_by(a - _pinch_ang_prev)
			_pinch_ang_prev = a
		elif view == V_SYSTEMS:
			# One finger dragging the map: orbit the view; mark it so the release isn't a tap.
			_rotate_by(-event.relative.x * ROT_DRAG_SENS)
			if absf(event.relative.x) + absf(event.relative.y) > 1.0:
				_was_drag = true
		return
	if event is InputEventMagnifyGesture:
		_zoom = clampf(_zoom / event.factor, ZOOM_MIN, ZOOM_MAX)
		return
	if event is InputEventMouseMotion and pc_mode and view == V_SYSTEMS:
		if (event.button_mask & MOUSE_BUTTON_MASK_LEFT) != 0:
			# Left-drag pans the map; Shift-left-drag rotates it (yaw). Past a small
			# threshold, mark _was_drag so the release isn't read as a click-to-focus.
			if event.shift_pressed:
				_rotate_by(-event.relative.x * ROT_DRAG_SENS)
			else:
				_pan_by(event.relative)
			# Only count it as a drag (suppressing click-to-focus) once the pointer has
			# travelled past the slop — a normal click jitters a pixel or two and must still focus.
			_drag_px += absf(event.relative.x) + absf(event.relative.y)
			if _drag_px > DRAG_SLOP:
				_was_drag = true
			return
	if event is InputEventMouseButton:
		match event.button_index:
			MOUSE_BUTTON_WHEEL_UP:
				if event.pressed and view == V_SYSTEMS:
					_zoom_by(0.85)
				return
			MOUSE_BUTTON_WHEEL_DOWN:
				if event.pressed and view == V_SYSTEMS:
					_zoom_by(1.18)
				return
			MOUSE_BUTTON_LEFT:
				if view == V_SYSTEMS:
					if event.pressed:
						_was_drag = false   # arm: a click that doesn't drag will focus
						_drag_px = 0.0
					elif _was_multitouch or _was_drag:
						_was_multitouch = false
						_was_drag = false
					else:
						var vp := _to_view(event.position)
						if not _pick_hauler(vp):
							_pick_body(vp)
				return
			MOUSE_BUTTON_RIGHT:
				if event.pressed and view == V_SYSTEMS:
					_reset_view()
				return
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	# While dilemmas are stacked, time is locked — ignore speed changes until cleared.
	if _dilemma_lock and event.keycode in [KEY_SPACE, KEY_1, KEY_2, KEY_3]:
		status = "Decisions pending — resolve them all to resume."
		return
	match event.keycode:
		KEY_SPACE:
			speed_idx = 0 if speed_idx != 0 else 1
		KEY_1:
			speed_idx = 1
		KEY_2:
			speed_idx = 2
		KEY_3:
			speed_idx = 3
		KEY_F1:
			_select_view(V_SYSTEMS)
		KEY_F2:
			_select_view(V_FLEET)
		KEY_F3:
			_select_view(V_BUILD)
		KEY_F4:
			_select_view(V_MARKET)
		KEY_F6:
			_select_view(V_EMPIRE)
		KEY_F8:
			_set_pc_mode(not pc_mode)
		KEY_F11:
			_toggle_fullscreen()
		KEY_UP:
			sel_comm = (sel_comm - 1 + sim.commodity_count()) % sim.commodity_count()
		KEY_DOWN:
			sel_comm = (sel_comm + 1) % sim.commodity_count()
		KEY_LEFT, KEY_RIGHT:
			sel_market = (sel_market + 1) % _visible_market_count()
		KEY_BRACKETLEFT:
			trade_qty = maxi(QTY_STEP, trade_qty - QTY_STEP)
		KEY_BRACKETRIGHT:
			trade_qty = mini(QTY_MAX, trade_qty + QTY_STEP)
		KEY_COMMA:
			_rotate_by(-ROT_STEP)
		KEY_PERIOD:
			_rotate_by(ROT_STEP)
		KEY_B:
			_do_buy()
		KEY_S:
			_do_sell()
		KEY_TAB:
			if sim.hauler_count() > 0:
				selected = (selected + 1) % sim.hauler_count()
		KEY_I:
			_do_interdict()
		KEY_E:
			status = "Exploited the shortage — sourced cheap, sold into the spike." if sim.answer_shortage() else "No open shortage to exploit."
		KEY_Y:
			auto_pause = not auto_pause
			status = "Auto-pause %s." % ("on" if auto_pause else "off")
		KEY_N:
			status = "Frigate commissioned." if sim.commission_ship(0) else "Can't build: short on crew or credits."
		KEY_P:
			sim.toggle_patrol()
			status = "Interdiction patrol %s." % ("engaged" if sim.patrol_enabled() else "stood down")
		KEY_O:
			sim.cycle_patrol_target()
			status = "Patrol now hunts: %s." % sim.patrol_target_name()
		KEY_R:
			sim.toggle_auto_research()
			status = "Auto-research %s." % ("on" if sim.auto_research_enabled() else "off")
		KEY_V:
			status = "Researched a new tech." if sim.research_next() else "Not enough research points yet."
		KEY_A:
			sim.nudge_alert_threshold(1)
		KEY_Z:
			sim.nudge_alert_threshold(-1)
		KEY_C:
			ceo_pick = (ceo_pick + 1) % BRANCH_NAMES.size()
		KEY_X:
			status = "CEO committed to %s." % BRANCH_NAMES[ceo_pick] if sim.ceo_choose_branch(ceo_pick) else "Branch already chosen."
		KEY_F:
			status = "Freighter commissioned." if sim.commission_freighter() else "Can't afford a freighter / no crew."
		KEY_D:
			var dest := (sel_market + 1) % _visible_market_count()
			sim.set_trade_route(sel_comm, sel_market, dest, trade_qty, 1)
			status = "Trade route set: %s %s→%s ×%d." % [
				sim.commodity_name(sel_comm), sim.market_name(sel_market), sim.market_name(dest), trade_qty]
		KEY_G:
			sim.clear_trade_route()
			status = "Trade route cleared."
		KEY_M:
			if sim.found_refinery(sel_comm, sel_market, sel_market):
				status = "Factory founded: %s → next tier @ %s." % [sim.commodity_name(sel_comm), sim.market_name(sel_market)]
			else:
				status = "Can't found factory — pick a non-top-tier commodity, or short on capital/slots."
		KEY_K:
			status = "Contract accepted — deliver the goods before it lapses." if sim.accept_first_contract() else "No open contract to accept."
		KEY_J:
			status = "Contract delivered — paid and reputation lifted." if sim.fulfill_ready_contract() else "No contract you can fill from the warehouse."
		KEY_L:
			# Hot-reload both tuning overlays (§31): commodities + ship catalog.
			var cerr: String = sim.reload_commodity_data(ProjectSettings.globalize_path("user://commodities.json"))
			var serr: String = sim.reload_ship_data(ProjectSettings.globalize_path("user://ships.json"))
			if cerr == "" and serr == "":
				status = "Tuning data reloaded (commodities + ships)."
			else:
				status = "Reload failed: %s" % (cerr if cerr != "" else serr)
		KEY_U:
			var nxt := (sim.intensity() + 1) % 3
			sim.set_intensity(nxt)
			status = "Pressure intensity: %s." % INTENSITY_NAMES[nxt]
		KEY_H:
			status = "Wreck stripped — haul aboard." if sim.salvage_wreck() else "No derelict in range to salvage."
		KEY_W:
			_engage_raiders()
		KEY_F5:
			_do_save()
		KEY_F9:
			_do_load()


func _do_buy() -> void:
	var cost := sim.buy(sel_market, sel_comm, trade_qty)
	if cost < 0:
		status = "Buy failed — short on credits, or the market's tapped out."
	else:
		status = "Bought %d %s at %s for %d cr." % [trade_qty, sim.commodity_name(sel_comm), sim.market_name(sel_market), cost]


func _do_sell() -> void:
	var revenue := sim.sell(sel_market, sel_comm, trade_qty)
	if revenue < 0:
		status = "Sell failed — you don't hold that much %s." % sim.commodity_name(sel_comm)
	else:
		status = "Sold %d %s at %s for %d cr." % [trade_qty, sim.commodity_name(sel_comm), sim.market_name(sel_market), revenue]


func _do_interdict() -> void:
	if sim.hauler_count() == 0:
		status = "No haulers in flight to interdict."
		return
	selected = clampi(selected, 0, sim.hauler_count() - 1)
	var id := sim.hauler_id(selected)
	var outcome := sim.attempt_interdict(id, sim.body_x(1), sim.body_y(1), 120_000, 1500)
	status = ["No firing solution — reposition.", "The hauler ran the gap (escaped).", "Hauler interdicted — a shortage blooms."][outcome]


# ============================================================================
# 3D PRIMITIVES
# ============================================================================

func _emissive_mat(col: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = col
	m.emission_enabled = true
	m.emission = col
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	return m


func _sphere(radius: float, mat: Material) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var sm := SphereMesh.new()
	sm.radius = radius
	sm.height = radius * 2.0
	mi.mesh = sm
	mi.material_override = mat
	return mi


## A small amber mining-rig glyph (a stubby drum) for the orrery.
func _miner_marker() -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var cm := CylinderMesh.new()
	cm.top_radius = 0.05
	cm.bottom_radius = 0.07
	cm.height = 0.11
	cm.radial_segments = 8
	mi.mesh = cm
	mi.material_override = _miner_mat
	return mi


## A small directional **hull** marker for the orrery (A5) — a long thin body that
## points down its lane (oriented in _smooth_to), so ships read as ships, not dots.
## Single mesh so picking/selection (material_override + scale) stay unchanged.
func _hull_marker(mat: StandardMaterial3D) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = Vector3(0.05, 0.04, 0.18)   # long, thin — a hull along its heading (+Z/-Z)
	mi.mesh = bm
	mi.material_override = mat
	return mi


## A tiny station glyph for a colony/holding on the orrery (A5) — a hab drum + cross
## arms, faction-tinted. Parented to a (static) body node, so a multi-part node is fine.
func _station_glyph(fcol: Color) -> Node3D:
	var root := Node3D.new()
	var mat := _emissive_mat(fcol)
	var drum := MeshInstance3D.new()
	var cm := CylinderMesh.new()
	cm.top_radius = 0.02
	cm.bottom_radius = 0.02
	cm.height = 0.05
	cm.radial_segments = 8
	drum.mesh = cm
	drum.material_override = mat
	root.add_child(drum)
	for ang in [0.0, 90.0]:
		var arm := MeshInstance3D.new()
		var b := BoxMesh.new()
		b.size = Vector3(0.08, 0.006, 0.006)
		arm.mesh = b
		arm.rotation_degrees = Vector3(0, ang, 0)
		arm.material_override = mat
		root.add_child(arm)
	return root


func _ring(radius: float, col: Color) -> MeshInstance3D:
	# A thin orbit line with a faint glow — the emission tips just into the bloom pass.
	return _ring_mat(radius, _emissive_mat(col * 2.4), 0.005)


func _ring_mat(radius: float, mat: StandardMaterial3D, tube: float) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var tm := TorusMesh.new()
	tm.inner_radius = maxf(0.01, radius - tube)
	tm.outer_radius = radius + tube
	# Plenty of segments around the ring so a large orbit reads as a smooth circle, not a
	# stepped polygon; the tube itself needs only a few sides (it's a hairline). Scaled with
	# radius and capped so the inner moons stay cheap and the wide planet orbits stay round.
	tm.rings = clampi(int(radius * 48.0), 96, 384)
	tm.ring_segments = 6
	mi.mesh = tm
	mi.material_override = mat
	return mi


