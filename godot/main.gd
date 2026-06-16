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
const VIEW_GLYPH := ["◎", "◈", "⛭", "⇄"]
const VIEW_CAP := ["SYSTEMS", "FLEET", "BUILD", "MARKET"]
const VIEW_TITLE := [
	"Orrery — Sol System",
	"Fleet Management",
	"Orbital Shipyard",
	"Market & Logistics",
]

# 3D orrery framing (§17/§21). Clean mapping: 1 AU = 1 world unit.
const SCALE3D := 1.0 / 1_000_000.0
const CAM_DIR := Vector3(0.0, 1.15, 0.9)
const ZOOM_MIN := 1.2
const ZOOM_MAX := 140.0
# 0 Star, 1 Planet, 2 GasGiant, 3 Dwarf, 4 Moon, 5 Gate.
const BODY_RADIUS := [0.45, 0.13, 0.32, 0.09, 0.06, 0.0]
const FACTION_COL := [
	Color(0.4, 0.6, 1.0), Color(0.95, 0.45, 0.4),
	Color(0.95, 0.75, 0.35), Color(0.55, 0.85, 0.6),
]
const SPACE_BG := Color(0.02, 0.03, 0.06)
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
var _zoom := 10.0
var _focus_body := 0
var _touches := {}
var _pinch_prev := 0.0
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
var _res_credits: Label
var _res_ore: Label
var _res_fuel: ProgressBar
var _res_crew: Label
var _alert_ticker: Label
var _flash_rect: ColorRect
var _ascend_rect: ColorRect
var _help: Label

# Systems view.
var _sys_title: Label
var _sys_sub: Label
var _sys_status: Label
var _sys_resources: VBoxContainer
var _sys_queues: VBoxContainer
var _sys_now: Label
var _sys_gate: ProgressBar
var _sys_gate_lbl: Label
var _sys_mission: Label
var _sys_lore: Label
var _tg_patrol: CheckButton
var _tg_research: CheckButton
var _tg_pause: CheckButton
var _feed: RichTextLabel

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
var _fleet_tabs: Array[Button] = []

# Build view.
var _build_list: VBoxContainer
var _build_caption: Label
var _build_stats: Label
var _build_cost: Label
var _bom_lbl: Label
var _build_queue: VBoxContainer
var _ship_pivot: Node3D

# Market view.
var _flow: Control
var _chart: Control
var _ticker_grid: GridContainer
var _chart_legend: VBoxContainer

# ---- 3D world ---------------------------------------------------------------
var _orrery_root: Node3D
var _cam: Camera3D
var _body_nodes: Array[Node3D] = []
var _hauler_pool: Array[MeshInstance3D] = []
var _ship_pool: Array[MeshInstance3D] = []     # §6 player warships on the map
var _freighter_pool: Array[MeshInstance3D] = []  # §6 player freighters on the lanes
var _wreck_pool: Array[MeshInstance3D] = []
var _gate_ring: MeshInstance3D
var _lane_mesh: ImmediateMesh
var _hauler_mat: StandardMaterial3D
var _ship_mat: StandardMaterial3D
var _freighter_mat: StandardMaterial3D
var _select_mat: StandardMaterial3D
var _wreck_mat: StandardMaterial3D
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
	_build_diorama()
	_select_view(V_SYSTEMS)


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
	var env := WorldEnvironment.new()
	var e := Environment.new()
	e.background_mode = Environment.BG_COLOR
	e.background_color = SPACE_BG
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color(0.35, 0.4, 0.5)
	e.ambient_light_energy = 0.35
	env.environment = e
	add_child(env)

	_cam = Camera3D.new()
	_cam.current = true
	_cam.far = 6000.0
	add_child(_cam)
	_update_camera()

	_orrery_root = Node3D.new()
	add_child(_orrery_root)

	var sun_light := OmniLight3D.new()
	sun_light.omni_range = 6000.0
	sun_light.light_energy = 1.2
	_orrery_root.add_child(sun_light)
	var key := DirectionalLight3D.new()
	key.rotation_degrees = Vector3(-60, -30, 0)
	key.light_energy = 0.7
	_orrery_root.add_child(key)

	_hauler_mat = _emissive_mat(HAULER_COL)
	_ship_mat = _emissive_mat(sim.corp_livery_color())   # player warships fly the livery (§14)
	_freighter_mat = _emissive_mat(FREIGHTER_COL)        # player freighters on the lanes (§6)
	_select_mat = _emissive_mat(SELECT_COL)
	_wreck_mat = _emissive_mat(Color(0.45, 0.85, 0.85))

	var gate_r := 40.0
	for b in sim.body_count():
		var kind := sim.body_kind(b)
		if kind == 0:
			var sun := _sphere(BODY_RADIUS[0], _emissive_mat(Color(1.0, 0.85, 0.3)))
			_orrery_root.add_child(sun)
			_body_nodes.append(sun)
			continue
		if kind == 5:
			gate_r = _world3d(sim.body_x(b), sim.body_y(b)).length()
			var ph := Node3D.new()
			_orrery_root.add_child(ph)
			_body_nodes.append(ph)
			continue
		var body := _sphere(BODY_RADIUS[kind], _lit_mat(_body_colour_kind(b, kind)))
		_orrery_root.add_child(body)
		_body_nodes.append(body)
		var parent := sim.body_parent(b)
		if parent == 0:
			var r := _world3d(sim.body_x(b), sim.body_y(b)).length()
			_orrery_root.add_child(_ring(r, Color(0.20, 0.28, 0.38)))
		else:
			var mr: float = float(sim.body_orbit_radius(b)) * SCALE3D
			var mrm := _emissive_mat(Color(0.32, 0.36, 0.42))
			_body_nodes[parent].add_child(_ring_mat(mr, mrm, maxf(0.004, mr * 0.012)))
		var tag := Label3D.new()
		tag.text = sim.body_name(b)
		tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		tag.modulate = Color(0.6, 0.7, 0.78) if kind == 4 else Color(0.72, 0.84, 0.95)
		tag.pixel_size = 0.0026 if kind == 4 else 0.006
		tag.position = Vector3(0, BODY_RADIUS[kind] + 0.06, 0)
		body.add_child(tag)

	_gate_mat = _emissive_mat(Color(0.9, 0.78, 0.35))
	_gate_ring = _ring_mat(gate_r, _gate_mat, 0.12)
	_orrery_root.add_child(_gate_ring)

	for ci in sim.colony_count():
		var cb := sim.colony_body(ci)
		if cb < 0 or cb >= _body_nodes.size():
			continue
		var fcol: Color = FACTION_COL[clampi(sim.colony_faction(ci), 0, 3)]
		var marker := _sphere(0.03, _emissive_mat(fcol))
		marker.position = Vector3(BODY_RADIUS[sim.body_kind(cb)] + 0.03, 0.0, 0.0)
		_body_nodes[cb].add_child(marker)
		var clbl := Label3D.new()
		clbl.text = sim.colony_name(ci)
		clbl.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		clbl.modulate = fcol
		clbl.pixel_size = 0.0026
		clbl.position = Vector3(0.0, -BODY_RADIUS[sim.body_kind(cb)] - 0.07, 0.0)
		_body_nodes[cb].add_child(clbl)

	for b in sim.body_count():
		if sim.body_name(b) == "Saturn":
			_build_saturn_rings(_body_nodes[b])
			break

	_lane_mesh = ImmediateMesh.new()
	var lanes := MeshInstance3D.new()
	lanes.mesh = _lane_mesh
	var lane_mat := _emissive_mat(Color(0.85, 0.6, 0.35))
	lane_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	lane_mat.albedo_color = Color(0.85, 0.6, 0.35, 0.4)
	lanes.material_override = lane_mat
	_orrery_root.add_child(lanes)

	_build_starfield()


func _build_starfield() -> void:
	var n := 600
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.billboard_mode = BaseMaterial3D.BILLBOARD_ENABLED
	mat.albedo_color = Color(0.85, 0.88, 1.0)
	var quad := QuadMesh.new()
	quad.size = Vector2(1.4, 1.4)
	quad.material = mat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = quad
	mm.instance_count = n
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	for i in n:
		var dir := Vector3(rng.randfn(), rng.randfn(), rng.randfn())
		if dir.length() < 0.001:
			dir = Vector3.UP
		var pos := dir.normalized() * rng.randf_range(260.0, 420.0)
		var s := rng.randf_range(0.6, 2.0)
		mm.set_instance_transform(i, Transform3D(Basis().scaled(Vector3.ONE * s), pos))
	var mmi := MultiMeshInstance3D.new()
	mmi.multimesh = mm
	_orrery_root.add_child(mmi)


func _build_saturn_rings(saturn: Node3D) -> void:
	for rr in [0.42, 0.48, 0.54, 0.60, 0.66, 0.72]:
		var rm := _emissive_mat(Color(0.85, 0.78, 0.55, 0.45))
		rm.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		saturn.add_child(_ring_mat(rr, rm, 0.018))
	var amat := StandardMaterial3D.new()
	amat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	amat.albedo_color = Color(0.78, 0.74, 0.66)
	var rock := BoxMesh.new()
	rock.size = Vector3(0.012, 0.012, 0.012)
	rock.material = amat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = rock
	mm.instance_count = 220
	var rng := RandomNumberGenerator.new()
	rng.seed = 17
	for i in mm.instance_count:
		var ang := rng.randf() * TAU
		var rad := rng.randf_range(0.40, 0.74)
		var pos := Vector3(cos(ang) * rad, rng.randf_range(-0.012, 0.012), sin(ang) * rad)
		var s := rng.randf_range(0.5, 2.2)
		var basis := Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * s)
		mm.set_instance_transform(i, Transform3D(basis, pos))
	var ast := MultiMeshInstance3D.new()
	ast.multimesh = mm
	saturn.add_child(ast)


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
	_title = UiKit.label("", 13, UiKit.TEXT_DIM)
	_title.position = Vector2(92, 12)
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
	_res_credits = UiKit.label("", 14, UiKit.GOLD)
	res.add_child(_make_res_cell("CREDITS", _res_credits))
	_res_ore = UiKit.label("", 14, UiKit.TEXT)
	res.add_child(_make_res_cell("ORE", _res_ore))
	# Fuel as a gauge cell.
	var fuel_cell := VBoxContainer.new()
	fuel_cell.add_theme_constant_override("separation", 2)
	fuel_cell.add_child(UiKit.kicker("FUEL"))
	_res_fuel = UiKit.gauge(0.5, UiKit.ACCENT, 70, 9)
	fuel_cell.add_child(_res_fuel)
	res.add_child(fuel_cell)
	_res_crew = UiKit.label("", 14, UiKit.TEXT)
	res.add_child(_make_res_cell("CREW", _res_crew))

	# Content host (between the rail and the screen edge, below the bar).
	_content = Control.new()
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
	gv.add_child(UiKit.kicker("Objective"))
	_sys_mission = UiKit.label("", 12, UiKit.ACCENT)
	_sys_mission.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_sys_mission.custom_minimum_size = Vector2(338, 0)
	gv.add_child(_sys_mission)
	_sys_now = UiKit.label("", 11, UiKit.TEXT_DIM)
	_sys_now.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_sys_now.custom_minimum_size = Vector2(338, 0)
	gv.add_child(_sys_now)
	# The gate mystery — the one authored thread (§0.1).
	_sys_lore = UiKit.label("", 10, UiKit.GOLD)
	_sys_lore.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_sys_lore.custom_minimum_size = Vector2(338, 0)
	gv.add_child(_sys_lore)
	_sys_gate_lbl = UiKit.label("", 10, UiKit.GOLD)
	gv.add_child(_sys_gate_lbl)
	_sys_gate = UiKit.gauge(0.0, UiKit.GOLD, 338, 8)
	gv.add_child(_sys_gate)

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

	# Map controls (mobile-friendly), bottom-right corner of the orrery region.
	var mc := HBoxContainer.new()
	mc.add_theme_constant_override("separation", 6)
	mc.set_anchors_preset(Control.PRESET_FULL_RECT)
	mc.anchor_left = 1
	mc.anchor_top = 1
	mc.offset_left = -470
	mc.offset_top = -176
	mc.offset_right = -322
	mc.offset_bottom = -132
	root.add_child(mc)
	mc.add_child(_make_map_button("+", func(): _zoom_by(0.8)))
	mc.add_child(_make_map_button("–", func(): _zoom_by(1.25)))
	mc.add_child(_make_map_button("◉", _reset_view))

	# Fleet ops (§6): send the docked warships to the focused world, or refuel them.
	var fo := HBoxContainer.new()
	fo.add_theme_constant_override("separation", 6)
	fo.set_anchors_preset(Control.PRESET_FULL_RECT)
	fo.anchor_left = 1
	fo.anchor_top = 1
	fo.offset_left = -470
	fo.offset_top = -126
	fo.offset_right = -250
	fo.offset_bottom = -94
	root.add_child(fo)
	fo.add_child(_make_op_button("SEND FLEET", _dispatch_fleet_to_focus))
	fo.add_child(_make_op_button("REFUEL", _refuel_fleet))

	# Archive (§30): numbered save slots + load. (Ironman toggle lives in settings.)
	var ar := HBoxContainer.new()
	ar.add_theme_constant_override("separation", 6)
	ar.set_anchors_preset(Control.PRESET_FULL_RECT)
	ar.anchor_left = 1
	ar.anchor_top = 1
	ar.offset_left = -470
	ar.offset_top = -88
	ar.offset_right = -120
	ar.offset_bottom = -56
	root.add_child(ar)
	ar.add_child(_make_op_button("SLOT", _cycle_slot))
	ar.add_child(_make_op_button("SAVE", _do_save))
	ar.add_child(_make_op_button("LOAD", _do_load))


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
	return ProjectSettings.globalize_path("user://torch_slot_%d.json" % i)


func _ironman_path() -> String:
	return ProjectSettings.globalize_path("user://torch_ironman.json")


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
	var sc := ScrollContainer.new()
	sc.size_flags_vertical = Control.SIZE_EXPAND_FILL
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
	_diorama.visible = true


## Repaint both force rosters from the live surviving counts.
func _dio_refresh_forces() -> void:
	_dio_set_force(_dio_force_a, String(sim.corp_name()), _dio_surv[0], _dio_start[0], UiKit.GOOD)
	_dio_set_force(_dio_force_b, "Raiders", _dio_surv[1], _dio_start[1], UiKit.BAD)


func _close_diorama() -> void:
	_dio_playing = false
	_diorama.visible = false


## Reveal one BattleLog beat per DIO_STEP, then the outcome (called each frame).
func _play_diorama(delta: float) -> void:
	if not _dio_playing:
		return
	_dio_timer += delta
	var total := sim.battle_log_count()
	while _dio_timer >= DIO_STEP and _dio_idx < total:
		_dio_timer -= DIO_STEP
		_dio_log.append_text(_dio_event_line(_dio_idx) + "\n")
		# A kill depletes the victim side's roster live (§22 juice).
		if sim.battle_event_kind(_dio_idx) == 2:
			var side := sim.battle_event_side(_dio_idx)
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
	return "%s\n[color=#%s]Survivors — %s %d/%d  ·  Raiders %d/%d[/color]" % [
		head, UiKit.TEXT.to_html(false),
		String(sim.corp_name()), sim.battle_survivors(0), sim.battle_start_count(0),
		sim.battle_survivors(1), sim.battle_start_count(1)]


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
	sv.debug_draw = Viewport.DEBUG_DRAW_WIREFRAME
	svc.add_child(sv)
	var scam := Camera3D.new()
	scam.position = Vector3(0, 1.1, 4.2)
	sv.add_child(scam)
	scam.look_at(Vector3.ZERO, Vector3.UP)   # after entering the tree (uses global xform)
	var slight := OmniLight3D.new()
	slight.position = Vector3(2, 3, 4)
	slight.omni_range = 30
	sv.add_child(slight)
	_ship_pivot = Node3D.new()
	sv.add_child(_ship_pivot)
	_build_blueprint_ship(_ship_pivot)
	_build_caption = UiKit.label("", 15, UiKit.TEXT_HI)
	centre.add_child(_build_caption)
	_build_stats = UiKit.label("", 12, UiKit.TEXT_DIM)
	centre.add_child(_build_stats)
	_build_cost = UiKit.label("", 12, UiKit.TEXT)
	centre.add_child(_build_cost)
	_bom_lbl = UiKit.label("", 11, UiKit.TEXT_DIM)
	centre.add_child(_bom_lbl)
	var commission := UiKit.action_button("◆  COMMISSION HULL")
	commission.pressed.connect(_commission_selected)
	centre.add_child(commission)
	var assemble := UiKit.action_button("⚙  ASSEMBLE FROM PARTS")
	assemble.pressed.connect(_assemble_selected)
	centre.add_child(assemble)

	# Right: construction queue.
	var right := VBoxContainer.new()
	right.custom_minimum_size = Vector2(230, 0)
	right.add_theme_constant_override("separation", 5)
	hb.add_child(right)
	right.add_child(UiKit.kicker("Construction Queue"))
	_build_queue = VBoxContainer.new()
	_build_queue.add_theme_constant_override("separation", 6)
	right.add_child(_build_queue)


func _build_blueprint_ship(pivot: Node3D) -> void:
	var mat := _emissive_mat(UiKit.ACCENT)
	mat.emission_energy_multiplier = 0.6
	# Hull.
	var hull := MeshInstance3D.new()
	var cap := CapsuleMesh.new()
	cap.radius = 0.32
	cap.height = 2.4
	hull.mesh = cap
	hull.rotation_degrees = Vector3(90, 0, 0)
	hull.material_override = mat
	pivot.add_child(hull)
	# Bridge.
	var bridge := MeshInstance3D.new()
	var bx := BoxMesh.new()
	bx.size = Vector3(0.4, 0.3, 0.6)
	bridge.mesh = bx
	bridge.position = Vector3(0, 0.32, 0.5)
	bridge.material_override = mat
	pivot.add_child(bridge)
	# Nacelles.
	for sx in [-1.0, 1.0]:
		var nac := MeshInstance3D.new()
		var nb := BoxMesh.new()
		nb.size = Vector3(0.22, 0.22, 1.4)
		nac.mesh = nb
		nac.position = Vector3(0.5 * sx, -0.1, -0.3)
		nac.material_override = mat
		pivot.add_child(nac)


func _pick_build(i: int) -> void:
	build_pick = i
	for c in _build_list.get_child_count():
		(_build_list.get_child(c) as Button).set_pressed_no_signal(c == i)


func _commission_selected() -> void:
	if sim.commission_ship(build_pick):
		status = "%s commissioned into the fleet." % String(shipyard.class_name(build_pick))
	else:
		status = "Can't build — short on crew or credits."


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
	for m in sim.market_count():
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
	tv.add_child(UiKit.kicker("Market Ticker  ·  %s" % String(sim.market_name(sel_market))))
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
	cv.add_child(UiKit.kicker("Price History  ·  %s" % String(sim.market_name(sel_market))))
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
	_cam.position = look + CAM_DIR.normalized() * _zoom
	_cam.look_at(look, Vector3.UP)


func _focus_pos() -> Vector3:
	if _focus_body > 0 and _focus_body < sim.body_count():
		return _world3d(sim.body_x(_focus_body), sim.body_y(_focus_body))
	return Vector3.ZERO


func _world3d(wx: float, wy: float) -> Vector3:
	return Vector3(wx * SCALE3D, 0.0, -wy * SCALE3D)


func _screen(p: Vector3) -> Vector2:
	return _cam.unproject_position(p)


func _zoom_by(factor: float) -> void:
	_zoom = clampf(_zoom * factor, ZOOM_MIN, ZOOM_MAX)


func _two_finger_dist() -> float:
	var pts := _touches.values()
	if pts.size() < 2:
		return 0.0
	return pts[0].distance_to(pts[1])


func _reset_view() -> void:
	_focus_body = 0
	_zoom = 10.0
	status = "View: inner system (pinch / +– to zoom, tap a world to focus)."


# ============================================================================
# FRAME LOOP
# ============================================================================

func _process(delta: float) -> void:
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
	flash = maxf(0.0, flash - delta * 2.0)
	ascend_flash = maxf(0.0, ascend_flash - delta)
	# Ironman (§13): the world saves itself, so there's no scumming a bad call.
	if ironman:
		_autosave_accum += delta
		if _autosave_accum >= IRONMAN_AUTOSAVE_SEC:
			_autosave_accum = 0.0
			sim.save_game(_ironman_path())
	if view == V_SYSTEMS:
		_update_world()
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


func _update_world() -> void:
	_update_camera()
	for b in sim.body_count():
		if b < _body_nodes.size():
			_body_nodes[b].position = _world3d(sim.body_x(b), sim.body_y(b))
	var n := sim.hauler_count()
	while _hauler_pool.size() < n:
		var mi := _sphere(0.06, _hauler_mat)
		_orrery_root.add_child(mi)
		_hauler_pool.append(mi)
	for i in _hauler_pool.size():
		var node := _hauler_pool[i]
		if i < n:
			node.visible = true
			node.position = _world3d(sim.hauler_x(i), sim.hauler_y(i))
			var sel := i == selected
			node.material_override = _select_mat if sel else _hauler_mat
			node.scale = Vector3.ONE * (1.6 if sel else 1.0)
		else:
			node.visible = false
	# Player warships — positional now (§6); a moving one swells slightly.
	var sn := sim.fleet_size()
	while _ship_pool.size() < sn:
		var sm := _sphere(0.08, _ship_mat)
		_orrery_root.add_child(sm)
		_ship_pool.append(sm)
	for si in _ship_pool.size():
		var sship := _ship_pool[si]
		if si < sn:
			sship.visible = true
			sship.position = _world3d(sim.ship_x(si), sim.ship_y(si))
			sship.scale = Vector3.ONE * (1.4 if sim.ship_in_transit(si) else 1.0)
		else:
			sship.visible = false
	# Player freighters — positional on their standing-route lanes now (§6).
	var fn_ := sim.freighter_count()
	while _freighter_pool.size() < fn_:
		var fm := _sphere(0.07, _freighter_mat)
		_orrery_root.add_child(fm)
		_freighter_pool.append(fm)
	for fi in _freighter_pool.size():
		var fnode := _freighter_pool[fi]
		if fi < fn_:
			fnode.visible = true
			fnode.position = _world3d(sim.freighter_x(fi), sim.freighter_y(fi))
		else:
			fnode.visible = false
	_lane_mesh.clear_surfaces()
	if n > 0 or fn_ > 0:
		_lane_mesh.surface_begin(Mesh.PRIMITIVE_LINES)
		for i in n:
			_lane_mesh.surface_add_vertex(_world3d(sim.hauler_x(i), sim.hauler_y(i)))
			_lane_mesh.surface_add_vertex(_world3d(sim.hauler_dest_x(i), sim.hauler_dest_y(i)))
		for i in fn_:
			_lane_mesh.surface_add_vertex(_world3d(sim.freighter_x(i), sim.freighter_y(i)))
			_lane_mesh.surface_add_vertex(_world3d(sim.freighter_dest_x(i), sim.freighter_dest_y(i)))
		_lane_mesh.surface_end()
	var wn := sim.wreck_count()
	while _wreck_pool.size() < wn:
		var wm := _sphere(0.06, _wreck_mat)
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
	var g: float = clampf(float(sim.gate_progress_pct()) / 100.0, 0.0, 1.0)
	_gate_mat.emission_energy_multiplier = 0.2 + 1.6 * g


func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_PAUSED or what == NOTIFICATION_WM_WINDOW_FOCUS_OUT:
		speed_idx = 0


func _refresh() -> void:
	_refresh_chrome()
	match view:
		V_SYSTEMS:
			_refresh_systems()
		V_FLEET:
			_refresh_fleet()
		V_BUILD:
			_refresh_build()
		V_MARKET:
			_refresh_market()
	_flash_rect.color.a = flash * 0.5
	_ascend_rect.color.a = ascend_flash * 0.5
	_help.text = "[Space/1/2/3] time   [↑↓] commodity   [←→] market   [ [ / ] ] qty   [B]uy [S]ell   [Tab] target   [I]nterdict [E]xploit   [N]ew ship   [F]reighter [D]route [M]refinery   [K]/[J] contract   [H]salvage   [F5]/[F9] save·load"


func _refresh_chrome() -> void:
	var sp := "‖ PAUSED" if speed_idx == 0 else "▶ %d×" % int(SPEEDS[speed_idx])
	_title.text = "%s      %s" % [VIEW_TITLE[view], sp]
	_title.add_theme_color_override("font_color", UiKit.GOLD if speed_idx == 0 else UiKit.TEXT_DIM)
	_date.text = _date_string()
	_res_credits.text = _commas(sim.credits())
	_res_ore.text = _commas(sim.cargo(_idx_ore))
	var fuel := sim.cargo(_idx_fuel)
	_res_fuel.value = clampf(float(fuel) / 400.0, 0.05, 1.0)
	_res_crew.text = str(sim.trained_crew())
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


func _refresh_systems() -> void:
	# Title = the focused body if it's a market, else the trade-cursor market.
	_sys_title.text = String(sim.market_name(sel_market))
	_sys_sub.text = "Trading Node  ·  Sol System"
	_sys_status.text = "Status: Online   ·   %s   ·   Gate %d%%" % [sim.tier_name(), sim.gate_progress_pct()]
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
	_sys_lore.text = "✦ %s" % String(sim.gate_lore())
	_sys_gate_lbl.text = "RING-GATE  %d%%   ·   mystery %d/7" % [sim.gate_progress_pct(), sim.gate_beats()]
	_sys_gate.value = clampf(float(sim.gate_progress_pct()) / 100.0, 0.0, 1.0)
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


func _refresh_market() -> void:
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
		for m in sim.market_count():
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
	for m in sim.market_count():
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
		var d := _screen(_world3d(sim.hauler_x(hi), sim.hauler_y(hi))).distance_to(pos)
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
	_zoom = clampf(_zoom, ZOOM_MIN, 8.0) if sim.body_kind(best) == 2 else _zoom
	var note := ""
	for m in sim.market_count():
		if sim.market_body(m) == best:
			sel_market = m
			note = " — trade cursor here"
	status = "Focus: %s%s." % [sim.body_name(best), note]


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventScreenTouch:
		if event.pressed:
			_touches[event.index] = event.position
			if _touches.size() >= 2:
				_was_multitouch = true
				_pinch_prev = _two_finger_dist()
		else:
			_touches.erase(event.index)
			if _touches.size() < 2:
				_pinch_prev = 0.0
		return
	if event is InputEventScreenDrag:
		if _touches.has(event.index):
			_touches[event.index] = event.position
		if _touches.size() >= 2:
			var d := _two_finger_dist()
			if _pinch_prev > 0.0 and d > 0.0:
				_zoom = clampf(_zoom * (_pinch_prev / d), ZOOM_MIN, ZOOM_MAX)
			_pinch_prev = d
		return
	if event is InputEventMagnifyGesture:
		_zoom = clampf(_zoom / event.factor, ZOOM_MIN, ZOOM_MAX)
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
				if not event.pressed and view == V_SYSTEMS:
					if _was_multitouch:
						_was_multitouch = false
					elif not _pick_hauler(event.position):
						_pick_body(event.position)
				return
			MOUSE_BUTTON_RIGHT:
				if event.pressed and view == V_SYSTEMS:
					_reset_view()
				return
	if not (event is InputEventKey) or not event.pressed or event.echo:
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
		KEY_UP:
			sel_comm = (sel_comm - 1 + sim.commodity_count()) % sim.commodity_count()
		KEY_DOWN:
			sel_comm = (sel_comm + 1) % sim.commodity_count()
		KEY_LEFT, KEY_RIGHT:
			sel_market = (sel_market + 1) % sim.market_count()
		KEY_BRACKETLEFT:
			trade_qty = maxi(QTY_STEP, trade_qty - QTY_STEP)
		KEY_BRACKETRIGHT:
			trade_qty = mini(QTY_MAX, trade_qty + QTY_STEP)
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
			var dest := (sel_market + 1) % sim.market_count()
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


func _lit_mat(col: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = col
	return m


func _sphere(radius: float, mat: StandardMaterial3D) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var sm := SphereMesh.new()
	sm.radius = radius
	sm.height = radius * 2.0
	mi.mesh = sm
	mi.material_override = mat
	return mi


func _ring(radius: float, col: Color) -> MeshInstance3D:
	return _ring_mat(radius, _emissive_mat(col), 0.02)


func _ring_mat(radius: float, mat: StandardMaterial3D, tube: float) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var tm := TorusMesh.new()
	tm.inner_radius = maxf(0.01, radius - tube)
	tm.outer_radius = radius + tube
	mi.mesh = tm
	mi.material_override = mat
	return mi


func _body_colour(b: int) -> Color:
	var palette := [
		Color(0.55, 0.75, 1.0), Color(0.8, 0.85, 0.9),
		Color(0.9, 0.6, 0.45), Color(0.6, 0.85, 0.7),
	]
	return palette[(b - 1) % palette.size()]


func _body_colour_kind(b: int, kind: int) -> Color:
	match kind:
		2:
			var giants := [
				Color(0.85, 0.72, 0.5), Color(0.92, 0.85, 0.6),
				Color(0.6, 0.88, 0.88), Color(0.45, 0.6, 0.95),
			]
			return giants[((b - 6) % giants.size() + giants.size()) % giants.size()]
		3:
			return Color(0.72, 0.66, 0.6)
		4:
			return Color(0.7, 0.72, 0.76)
		_:
			return _body_colour(b)
