extends Node3D

## TORCH — the playable shell (§18–§21). A real-time-with-pause game loop over the
## deterministic Rust core, now with a **3D orrery**: lit bodies orbit the sun on
## the ecliptic, haulers run the lanes between them, and an always-visible ring
## marks the gate (§0.1). The HUD (panels + alert feed + the voice) rides on a 2D
## CanvasLayer over the 3D world; the player presses the verbs (§0.4).
##
## All game logic lives in the Rust `sim`; this scene drives `step()` on a clock,
## mirrors the snapshot into 3D nodes, and turns input into sim verbs.

const TICKS_PER_SECOND := 6.0           # sim ticks per real second at 1× (§28)
const SPEEDS := [0.0, 1.0, 6.0, 24.0]   # pause / 1× / 6× / 24× (§6)
const THRESHOLD_NAMES := ["info", "notice", "warning", "critical"]
const BRANCH_NAMES := ["Industrialist", "Trader", "Warlord", "Diplomat"]
const INTENSITY_NAMES := ["Calm", "Normal", "Harsh"]   # §13 pressure difficulty
const QTY_STEP := 5
const QTY_MAX := 500
const SAVE_PATH := "user://savegame.json"   # where [F5]/[F9] persist the run (§30)

# 3D orrery framing (§21). Sim distances are in ~10^6 units; scale to a few dozen
# world units and look down at an angle so the ecliptic reads as a plane.
const SCALE3D := 1.0 / 320000.0         # sim units → world units (Ceres ≈ 9 units)
const CAM_POS := Vector3(0, 16, 13)
# Look left of the sun so the whole system sits in the clear right half of the
# screen, not behind the left-hand HUD column (§20).
const LOOK_TARGET := Vector3(-5.5, 0, 0)
const SPACE_BG := Color(0.02, 0.03, 0.06)
const PANEL_BG := Color(0.04, 0.05, 0.07, 0.82)   # left info-column backdrop (§20)
const HAULER_COL := Color(0.95, 0.7, 0.35)
const SELECT_COL := Color(1.0, 0.4, 0.2)

var sim: TorchSim
var speed_idx := 1
var accum := 0.0
var auto_pause := true                   # pause when an act-now alert fires (§28)
var selected := 0                       # index of the selected in-flight hauler
var flash := 0.0                         # act-now alert juice: a fading screen tint (§23)
var ascend_flash := 0.0                  # tier-ascension fanfare: a fading gold glow (§0.3)
var last_tier := ""                      # to detect a tier ascent across frames

# The trade cursor — granular control over what/where/how much you deal (§5).
var sel_comm := 5                       # commodity (ReactorFuel by default)
var sel_market := 0                     # market (Ceres)
var trade_qty := 20
var ceo_pick := 2                       # CEO branch under consideration (Warlord)

var status := "Welcome, CEO."

# HUD (2D overlay).
var _top: Label
var _assets: Label
var _deck: Label
var _feed: RichTextLabel              # bbcode so alerts colour by priority (§19)
var _help: Label
var _paused: Label
var _flash_rect: ColorRect
var _ascend_rect: ColorRect

# 3D world.
var _cam: Camera3D
var _body_nodes: Array[Node3D] = []      # one per sim body (index-aligned; sun at 0)
var _hauler_pool: Array[MeshInstance3D] = []
var _wreck_pool: Array[MeshInstance3D] = []   # §15 derelict markers on the map
var _gate_ring: MeshInstance3D
var _lane_mesh: ImmediateMesh                 # faint trails: each hauler → its dest (§7b)
var _hauler_mat: StandardMaterial3D
var _select_mat: StandardMaterial3D
var _wreck_mat: StandardMaterial3D
var _gate_mat: StandardMaterial3D


func _ready() -> void:
	sim = TorchSim.new()
	sim.reset(7)
	_build_world()
	_build_hud()


# ---- scene construction -----------------------------------------------------

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
	_cam.look_at_from_position(CAM_POS, LOOK_TARGET, Vector3.UP)
	_cam.current = true
	add_child(_cam)

	# The sun lights the system from the centre.
	var sun_light := OmniLight3D.new()
	sun_light.omni_range = 200.0
	sun_light.light_energy = 1.6
	add_child(sun_light)
	var key := DirectionalLight3D.new()
	key.rotation_degrees = Vector3(-60, -30, 0)
	key.light_energy = 0.4
	add_child(key)

	# Shared hauler/wreck materials (created once; reused across the pools).
	_hauler_mat = _emissive_mat(HAULER_COL)
	_select_mat = _emissive_mat(SELECT_COL)
	_wreck_mat = _emissive_mat(Color(0.45, 0.85, 0.85))   # teal: a derelict to strip

	var max_r := 1.0
	for b in sim.body_count():
		var pos := _world3d(sim.body_x(b), sim.body_y(b))
		if b == 0:
			# The sun: a bright emissive core at the centre.
			var sun := _sphere(0.9, _emissive_mat(Color(1.0, 0.85, 0.3)))
			sun.position = pos
			add_child(sun)
			_body_nodes.append(sun)
			continue
		var r := pos.length()
		max_r = maxf(max_r, r)
		# Orbit ring on the ecliptic.
		add_child(_ring(r, Color(0.25, 0.35, 0.45)))
		# The planet/station body, lit by the sun.
		var body := _sphere(0.5, _lit_mat(_body_colour(b)))
		body.position = pos
		add_child(body)
		# A billboarded name tag floating above it (§21 legibility).
		var tag := Label3D.new()
		tag.text = sim.body_name(b)
		tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		tag.modulate = Color(0.75, 0.85, 0.95)
		tag.pixel_size = 0.006
		tag.position = Vector3(0, 0.6, 0)
		body.add_child(tag)
		_body_nodes.append(body)

	# The always-visible ring-gate (§0.1): a faint outer ring that brightens as you
	# approach. Updated each frame from gate_progress_pct.
	_gate_mat = _emissive_mat(Color(0.9, 0.78, 0.35))
	_gate_ring = _ring_mat(max_r + 1.8, _gate_mat, 0.05)
	add_child(_gate_ring)

	# Hauler lane trails (§7b): faint lines from each hauler to its destination,
	# rebuilt every frame, so the interdiction decision ("which one?") is spatial.
	_lane_mesh = ImmediateMesh.new()
	var lanes := MeshInstance3D.new()
	lanes.mesh = _lane_mesh
	var lane_mat := _emissive_mat(Color(0.85, 0.6, 0.35))
	lane_mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	lane_mat.albedo_color = Color(0.85, 0.6, 0.35, 0.4)
	lanes.material_override = lane_mat
	add_child(lanes)

	_build_starfield()


## A deterministic starfield shell behind the system — the §21 "felt vastness",
## so the dark space reads as depth rather than emptiness. A single MultiMesh of
## billboarded points (cheap, static).
func _build_starfield() -> void:
	var n := 600
	var mat := StandardMaterial3D.new()
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	mat.billboard_mode = BaseMaterial3D.BILLBOARD_ENABLED
	mat.albedo_color = Color(0.85, 0.88, 1.0)
	var quad := QuadMesh.new()
	quad.size = Vector2(0.18, 0.18)
	quad.material = mat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = quad
	mm.instance_count = n
	var rng := RandomNumberGenerator.new()
	rng.seed = 7              # deterministic placement (§27 in spirit)
	for i in n:
		var dir := Vector3(rng.randfn(), rng.randfn(), rng.randfn())
		if dir.length() < 0.001:
			dir = Vector3.UP
		var pos := dir.normalized() * rng.randf_range(55.0, 80.0)
		var s := rng.randf_range(0.5, 1.6)   # varied star sizes
		mm.set_instance_transform(i, Transform3D(Basis().scaled(Vector3.ONE * s), pos))
	var mmi := MultiMeshInstance3D.new()
	mmi.multimesh = mm
	add_child(mmi)


func _build_hud() -> void:
	var layer := CanvasLayer.new()
	add_child(layer)

	# Left info-column backdrop so panels stay legible over the orrery (§20).
	var bg := ColorRect.new()
	bg.color = PANEL_BG
	bg.position = Vector2(0, 0)
	bg.size = Vector2(720, 720)
	bg.mouse_filter = Control.MOUSE_FILTER_IGNORE
	layer.add_child(bg)

	# Full-screen flash washes (juice). Kept transparent until an event fires.
	_flash_rect = _make_wash(Color(1.0, 0.3, 0.22, 0.0))
	_ascend_rect = _make_wash(Color(1.0, 0.82, 0.3, 0.0))
	layer.add_child(_flash_rect)
	layer.add_child(_ascend_rect)

	# Stacked, non-overlapping panels down the left column (sizes/gaps tuned from
	# rendered captures so the dense market board never runs into the deck).
	_top = _make_label(layer, Vector2(12, 8), 17)
	_assets = _make_label(layer, Vector2(12, 38), 12)
	_deck = _make_label(layer, Vector2(12, 366), 10)
	_help = _make_label(layer, Vector2(12, 700), 10)
	# The alert feed is a bbcode RichTextLabel so each line can colour by priority.
	_feed = RichTextLabel.new()
	_feed.bbcode_enabled = true
	_feed.scroll_active = false
	_feed.position = Vector2(12, 636)
	_feed.size = Vector2(700, 110)
	_feed.add_theme_font_size_override("normal_font_size", 12)
	_feed.add_theme_font_size_override("bold_font_size", 12)
	_feed.mouse_filter = Control.MOUSE_FILTER_IGNORE
	layer.add_child(_feed)
	# The paused banner sits over the orrery (right side), clear of the top bar.
	_paused = _make_label(layer, Vector2(900, 70), 22)
	_paused.modulate = Color(1.0, 0.8, 0.3)


func _make_label(parent: CanvasLayer, pos: Vector2, size: int) -> Label:
	var l := Label.new()
	l.position = pos
	l.add_theme_font_size_override("font_size", size)
	l.mouse_filter = Control.MOUSE_FILTER_IGNORE   # clicks fall through to picking
	parent.add_child(l)
	return l


func _make_wash(col: Color) -> ColorRect:
	var cr := ColorRect.new()
	cr.color = col
	cr.position = Vector2.ZERO
	cr.size = Vector2(1280, 720)
	cr.set_anchors_preset(Control.PRESET_FULL_RECT)
	cr.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return cr


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
	var tm := TorusMesh.new()             # lies flat on the XZ plane (hole up Y)
	tm.inner_radius = maxf(0.01, radius - tube)
	tm.outer_radius = radius + tube
	mi.mesh = tm
	mi.material_override = mat
	return mi


func _body_colour(b: int) -> Color:
	var palette := [
		Color(0.55, 0.75, 1.0),   # cool
		Color(0.8, 0.85, 0.9),    # pale
		Color(0.9, 0.6, 0.45),    # rust
		Color(0.6, 0.85, 0.7),    # green
	]
	return palette[(b - 1) % palette.size()]


# ---- world → screen ---------------------------------------------------------

## Sim coords (orbital plane) → 3D world position on the ecliptic.
func _world3d(wx: float, wy: float) -> Vector3:
	return Vector3(wx * SCALE3D, 0.0, -wy * SCALE3D)


## A 3D position projected to a screen point (for mouse picking).
func _screen(p: Vector3) -> Vector2:
	return _cam.unproject_position(p)


# ---- frame loop -------------------------------------------------------------

func _process(delta: float) -> void:
	var mult: float = SPEEDS[speed_idx]
	if mult > 0.0:
		accum += delta * TICKS_PER_SECOND * mult
		while accum >= 1.0:
			sim.step()
			accum -= 1.0
			# An act-now exception flashes the screen (§23 juice) so it's never
			# missed, and — if auto-pause is on (§28/§0.4) — stops the clock the
			# instant it fires so you never idle through a decision.
			if sim.just_alerted():
				flash = 1.0
				if auto_pause:
					speed_idx = 0
					accum = 0.0
					status = "Auto-paused — act-now shortage. [E] exploit, then resume."
					break
	# Tier ascent (§0.3): catch the climb and fire a celebratory gold fanfare.
	var tier := sim.tier_name()
	if last_tier != "" and tier != last_tier:
		ascend_flash = 1.0
		status = "Ascended to %s — the ring-gate draws closer." % tier
	last_tier = tier
	flash = maxf(0.0, flash - delta * 2.0)           # ~0.5 s fade
	ascend_flash = maxf(0.0, ascend_flash - delta)   # ~1 s celebratory fade
	_update_world()
	_refresh()


## Mirror the sim snapshot into the 3D scene each frame.
func _update_world() -> void:
	for b in sim.body_count():
		if b < _body_nodes.size():
			_body_nodes[b].position = _world3d(sim.body_x(b), sim.body_y(b))
	# Haulers: grow the pool to the current count, place them, hide the rest.
	var n := sim.hauler_count()
	while _hauler_pool.size() < n:
		var mi := _sphere(0.22, _hauler_mat)
		add_child(mi)
		_hauler_pool.append(mi)
	for i in _hauler_pool.size():
		var node := _hauler_pool[i]
		if i < n:
			node.visible = true
			node.position = _world3d(sim.hauler_x(i), sim.hauler_y(i))
			# The targeted hauler glows red and swells — the one you'd interdict.
			var sel := i == selected
			node.material_override = _select_mat if sel else _hauler_mat
			node.scale = Vector3.ONE * (1.6 if sel else 1.0)
		else:
			node.visible = false
	# Rebuild the hauler lane trails each frame (§7b).
	_lane_mesh.clear_surfaces()
	if n > 0:
		_lane_mesh.surface_begin(Mesh.PRIMITIVE_LINES)
		for i in n:
			_lane_mesh.surface_add_vertex(_world3d(sim.hauler_x(i), sim.hauler_y(i)))
			_lane_mesh.surface_add_vertex(_world3d(sim.hauler_dest_x(i), sim.hauler_dest_y(i)))
		_lane_mesh.surface_end()
	# Sighted derelicts (§15): a teal marker floating above the body each drifts
	# near, so discovery is visible on the map, not just in the HUD line.
	var wn := sim.wreck_count()
	while _wreck_pool.size() < wn:
		var wm := _sphere(0.18, _wreck_mat)
		add_child(wm)
		_wreck_pool.append(wm)
	for wi in _wreck_pool.size():
		var wnode := _wreck_pool[wi]
		var wb := sim.wreck_body(wi) if wi < wn else -1
		if wb >= 0:
			wnode.visible = true
			wnode.position = _world3d(sim.body_x(wb), sim.body_y(wb)) + Vector3(0.5 + 0.35 * wi, 0.7, 0)
		else:
			wnode.visible = false
	# The gate ring brightens with approach (§0.1).
	var g: float = clampf(float(sim.gate_progress_pct()) / 100.0, 0.0, 1.0)
	_gate_mat.emission_energy_multiplier = 0.2 + 1.6 * g
	# Flash washes track the fading juice values.
	_flash_rect.color.a = flash * 0.5
	_ascend_rect.color.a = ascend_flash * 0.5
	_paused.visible = speed_idx == 0
	_paused.text = "‖ PAUSED"


## Backgrounding pauses the clock (§6/§28).
func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_PAUSED or what == NOTIFICATION_WM_WINDOW_FOCUS_OUT:
		speed_idx = 0


func _refresh() -> void:
	var speed_label := "paused" if speed_idx == 0 else "%d×" % int(SPEEDS[speed_idx])
	_top.text = "TORCH  ·  T+%d  ·  %s        Tier: %s   Gate %d%%        %d cr   crew %d   fleet %d" % [
		sim.tick(), speed_label, sim.tier_name(), sim.gate_progress_pct(),
		sim.credits(), sim.trained_crew(), sim.fleet_size(),
	]

	var lines: Array[String] = []
	lines.append("NOW: %s (%d/%d)" % [sim.now_goal(), sim.now_goal_progress(), sim.now_goal_target()])
	# The §0.3 tier briefing + the scope it unlocks (stations/routes widen as you climb).
	lines.append("   %s" % sim.tier_briefing())
	lines.append("   scope: up to %d stations, %d routes" % [sim.station_cap(), sim.route_cap()])
	lines.append("")
	# Market board: a marker on the selected market column + selected commodity row.
	var head := "   %-12s" % "MARKET"
	for m in sim.market_count():
		var nm := sim.market_name(m)
		head += ("[%-8s]" % nm) if m == sel_market else (" %-8s  " % nm)
	head += "   you"
	lines.append(head)
	for c in sim.commodity_count():
		var cursor := ">" if c == sel_comm else " "
		var row := "%s  %-12s" % [cursor, sim.commodity_name(c)]
		for m in sim.market_count():
			row += " %9d " % sim.price(m, c)
		row += "  %d" % sim.cargo(c)
		lines.append(row)
	lines.append("")
	# The live trade preview — what this deal would cost / earn right now.
	var price := sim.price(sel_market, sel_comm)
	lines.append("TRADE  %s @ %s  ×%d   →  buy %d cr / sell %d cr" % [
		sim.commodity_name(sel_comm), sim.market_name(sel_market),
		trade_qty, price * trade_qty, price * trade_qty,
	])
	lines.append("")
	lines.append("haulers in flight: %d   (selected %d)" % [sim.hauler_count(), selected])
	lines.append("» " + status)
	_assets.text = "\n".join(lines)

	# Command deck — the policy a CEO sets and the company she grows (§10/§12).
	var deck: Array[String] = ["── COMMAND DECK ──"]
	var rep := "  "
	for f in sim.faction_count():
		rep += "%s %+d %s   " % [sim.faction_name(f), sim.faction_standing(f), sim.faction_tier(f)]
		if f == 1:
			deck.append(rep)
			rep = "  "
	if rep.strip_edges() != "":
		deck.append(rep)
	var branch := sim.ceo_branch_name()
	var branch_str := branch if branch != "(none)" else "(pick %s: C cycle, X commit)" % BRANCH_NAMES[ceo_pick]
	deck.append("CEO Lv %d %s    research %d techs (+%d%% drive), %d pts" % [
		sim.ceo_level(), branch_str, sim.research_unlocked_count(),
		sim.research_drive_bonus(), sim.research_points()
	])
	deck.append("patrol: %s (%s)    auto-research: %s    alerts ≥ %s    auto-pause: %s" % [
		"ON" if sim.patrol_enabled() else "off", sim.patrol_target_name(),
		"ON" if sim.auto_research_enabled() else "off", THRESHOLD_NAMES[sim.alert_threshold()],
		"ON" if auto_pause else "off"
	])
	# Fleet roster (§14): size, and the flagship — the hero ship you come to care about.
	var fleet_line := "fleet %d" % sim.fleet_size()
	if sim.fleet_size() > 0:
		fleet_line += "    flagship: %s" % sim.flagship_name()
	deck.append(fleet_line)
	# Standing-order master-tables (§4): every route/station/contract on its own
	# row (the "master-tables" half of the map+tables control model), each capped
	# so the panel stays bounded.
	deck.append("ROUTES %d/%d   freighters %d" % [sim.route_count(), sim.route_cap(), sim.freighters()])
	_append_table(deck, sim.route_count(), 3, func(i): return sim.route_desc(i), "(none — [D] sets one)")
	deck.append("STATIONS %d/%d    CONTRACTS %d open" % [sim.station_count(), sim.station_cap(), sim.open_contract_count()])
	_append_table(deck, sim.station_count(), 2, func(i): return sim.station_desc(i), "")
	_append_table(deck, sim.contract_count(), 1, func(i): return sim.contract_desc(i), "")
	# §13 pressure: the three gauges, the next-raid telegraph, and the difficulty.
	deck.append("pressure  war %d  piracy %d  scarcity %d    raid ETA ~%dt    intensity: %s" % [
		sim.pressure_level(0), sim.pressure_level(1), sim.pressure_level(2),
		sim.raid_eta(), INTENSITY_NAMES[sim.intensity()]
	])
	# §15 discovery: derelicts sighted, ripe to strip ([H] to salvage the nearest).
	var wrecks := "wrecks %d sighted" % sim.wreck_count()
	if sim.wreck_count() > 0:
		wrecks += "    nearest: %s  [H] salvage" % sim.wreck_name(0)
	deck.append(wrecks)
	_deck.text = "\n".join(deck)

	# Alert feed, coloured by priority (§19): act-now shortages glow warm and
	# carry a [!], FYI notices stay cool and quiet.
	var feed := "[b]── ALERT FEED ──[/b]\n"
	for a in mini(sim.alert_count(), 2):
		var msg := sim.alert_message(a)
		if sim.alert_is_act_now(a):
			feed += "[color=#ff6a4d][!] %s[/color]\n" % msg
		else:
			feed += "[color=#9fb0c0]    %s[/color]\n" % msg
	_feed.text = feed

	_help.text = "[Space/1/2/3]time  [↑↓]commodity [←→]market [ [ ] ]qty [B]uy [S]ell  [Tab]/[click]target [I]nterdict [E]xploit  [N]ew ship  [F]reighter [D]route [G]clear [M]refinery [K]accept [J]fill-contract\n[P]atrol [O]target [R]auto-research [V]invest [A/Z]alerts [C]CEO-pick [X]commit [Y]auto-pause [U]intensity [H]salvage  [F5]save [F9]load"


## Append up to `cap` rows of a standing-order table to the deck, with an overflow
## tally and an optional empty-state line (the §4 master-tables).
func _append_table(rows: Array, count: int, cap: int, getter: Callable, empty: String) -> void:
	if count == 0:
		if empty != "":
			rows.append("   " + empty)
		return
	for i in mini(count, cap):
		rows.append("   • " + str(getter.call(i)))
	if count > cap:
		rows.append("   …(+%d more)" % (count - cap))


# ---- input ------------------------------------------------------------------

## Select the in-flight hauler nearest a screen point, if one is within reach.
## Returns whether a hauler was picked.
func _pick_hauler(pos: Vector2) -> bool:
	var best := -1
	var best_d := 22.0   # px pick radius
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


## Select the market whose body is nearest a screen point (sets the trade cursor).
func _pick_market(pos: Vector2) -> void:
	var best := -1
	var best_d := 36.0   # px pick radius around the body
	for m in sim.market_count():
		var b := sim.market_body(m)
		if b < 0:
			continue
		var d := _screen(_world3d(sim.body_x(b), sim.body_y(b))).distance_to(pos)
		if d < best_d:
			best_d = d
			best = m
	if best >= 0:
		sel_market = best
		status = "Market: %s — trade cursor here." % sim.market_name(best)


func _unhandled_input(event: InputEvent) -> void:
	# Click an in-flight hauler in the orrery to target it for interdiction (§21);
	# failing that, select the market at the clicked body — the orrery is the
	# control surface.
	if event is InputEventMouseButton and event.pressed and event.button_index == MOUSE_BUTTON_LEFT:
		if not _pick_hauler(event.position):
			_pick_market(event.position)
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
			# A standing Trade Route from the cursor: buy here, sell at the other market.
			var dest := (sel_market + 1) % sim.market_count()
			sim.set_trade_route(sel_comm, sel_market, dest, trade_qty, 1)
			status = "Trade route set: %s %s→%s ×%d." % [
				sim.commodity_name(sel_comm), sim.market_name(sel_market), sim.market_name(dest), trade_qty
			]
		KEY_G:
			sim.clear_trade_route()
			status = "Trade route cleared."
		KEY_M:
			# Found a refinery for the selected raw commodity at the selected market.
			if sim.found_refinery(sel_comm, sel_market, sel_market):
				status = "Refinery founded: %s → refined @ %s." % [sim.commodity_name(sel_comm), sim.market_name(sel_market)]
			else:
				status = "Can't found refinery — pick a RAW commodity, or short on capital/slots."
		KEY_K:
			status = "Contract accepted — deliver the goods before it lapses." if sim.accept_first_contract() else "No open contract to accept."
		KEY_J:
			status = "Contract delivered — paid and reputation lifted." if sim.fulfill_ready_contract() else "No contract you can fill from the warehouse."
		KEY_L:
			# Hot-reload commodity tuning (§31) from a designer-droppable override.
			var err := sim.reload_commodity_data(ProjectSettings.globalize_path("user://commodities.json"))
			status = "Commodity data reloaded." if err == "" else "Reload failed: %s" % err
		KEY_U:
			# Cycle the §13 pressure-intensity difficulty (Calm/Normal/Harsh).
			var next := (sim.intensity() + 1) % 3
			sim.set_intensity(next)
			status = "Pressure intensity: %s." % INTENSITY_NAMES[next]
		KEY_H:
			# Salvage a sighted derelict (§15 discovery & wonder).
			status = "Wreck stripped — haul aboard." if sim.salvage_wreck() else "No derelict in range to salvage."
		KEY_F5:
			# Save the run to disk (§30).
			var serr := sim.save_game(ProjectSettings.globalize_path(SAVE_PATH))
			status = "Game saved." if serr == "" else "Save failed: %s" % serr
		KEY_F9:
			# Load the run from disk (§30); resets selection cursors into range.
			var lerr := sim.load_game(ProjectSettings.globalize_path(SAVE_PATH))
			if lerr == "":
				speed_idx = 0
				selected = 0
				status = "Game loaded — paused. Press [1] to resume."
			else:
				status = "Load failed: %s" % lerr


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


## The featured verb (§7b): send a frigate from Earth to cut the selected hauler.
func _do_interdict() -> void:
	if sim.hauler_count() == 0:
		status = "No haulers in flight to interdict."
		return
	selected = clampi(selected, 0, sim.hauler_count() - 1)
	var id := sim.hauler_id(selected)
	var outcome := sim.attempt_interdict(id, sim.body_x(1), sim.body_y(1), 120_000, 1500)
	status = ["No firing solution — reposition.", "The hauler ran the gap (escaped).", "Hauler interdicted — a shortage blooms."][outcome]
