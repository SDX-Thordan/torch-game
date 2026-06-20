extends Node3D

## TORCH — minimal shell for the multi-player core rework (iteration 1).
##
## The elaborate 8-view UI was removed; what remains is the 3D orrery framework
## (bodies orbiting the sun) plus a Paradox-style top bar and an escape menu.
## All game logic lives in the deterministic Rust `sim`; this scene drives
## `step()` on a clock and mirrors body positions into 3D nodes.

const UiKit := preload("res://ui/ui_kit.gd")

# 1 AU = 1 world unit; body positions arrive in millionths of an AU.
const SCALE3D := 1.0 / 1_000_000.0
const SPEEDS := [0.0, 1.0, 6.0, 24.0]          # pause · 1× · 6× · 24×
const SPEED_LABELS := ["❚❚", "▶", "▶▶", "▶▶▶"]
const TICKS_PER_SECOND := 4.0                   # real-time-with-pause base rate

var sim: TorchSim
var speed_idx := 1
var _accum := 0.0

# 3D
var _cam: Camera3D
var _orrery_root: Node3D
var _body_nodes: Array[Node3D] = []

# HUD
var _layer: CanvasLayer
var _topbar_labels := {}
var _escape_menu: Control
var _status: Label


func _ready() -> void:
	sim = TorchSim.new()
	sim.reset(7)
	_build_world()
	_build_topbar()
	_build_escape_menu()
	_refresh_topbar()


# ---- 3D orrery -----------------------------------------------------------------

func _build_world() -> void:
	var env := WorldEnvironment.new()
	var e := Environment.new()
	e.background_mode = Environment.BG_COLOR
	e.background_color = Color(0.01, 0.02, 0.04)
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color(0.10, 0.12, 0.16)
	e.ambient_light_energy = 0.6
	env.environment = e
	add_child(env)

	var sun_light := DirectionalLight3D.new()
	sun_light.rotation_degrees = Vector3(-55, -30, 0)
	sun_light.light_energy = 1.1
	add_child(sun_light)

	_cam = Camera3D.new()
	_cam.current = true
	_cam.far = 8000.0
	# A gently-tilted near-top-down view framing the inner system (~±6 AU).
	_cam.position = Vector3(0, 7.0, 2.6)
	_cam.look_at(Vector3.ZERO, Vector3(0, 0, -1))
	add_child(_cam)

	_orrery_root = Node3D.new()
	add_child(_orrery_root)

	for b in sim.body_count():
		var kind: int = sim.body_kind(b)
		# The ring-gate (5) and far-side bodies (6) are not shown.
		if kind == 5 or kind == 6:
			_body_nodes.append(null)
			continue
		var node := _spawn_body(b, kind)
		_orrery_root.add_child(node)
		_body_nodes.append(node)
	_update_world()


func _spawn_body(index: int, kind: int) -> Node3D:
	var holder := Node3D.new()
	var mesh := MeshInstance3D.new()
	var sphere := SphereMesh.new()
	var radius := _display_radius(kind)
	sphere.radius = radius
	sphere.height = radius * 2.0
	mesh.mesh = sphere
	var mat := StandardMaterial3D.new()
	if kind == 0:   # the sun
		mat.emission_enabled = true
		mat.emission = Color(1.0, 0.82, 0.4)
		mat.albedo_color = Color(1.0, 0.85, 0.5)
	else:
		mat.albedo_color = _body_color(kind)
	mesh.material_override = mat
	holder.add_child(mesh)

	var tag := Label3D.new()
	tag.text = String(sim.body_name(index))
	tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	tag.fixed_size = true
	tag.pixel_size = 0.0009
	tag.modulate = Color(0.72, 0.82, 0.92)
	tag.position = Vector3(0, radius + 0.06, 0)
	holder.add_child(tag)
	return holder


func _display_radius(kind: int) -> float:
	match kind:
		0: return 0.45             # star
		1: return 0.10             # planet
		2: return 0.18             # gas giant
		3: return 0.06             # dwarf
		4: return 0.035            # moon
		_: return 0.03             # asteroid / other


func _body_color(kind: int) -> Color:
	match kind:
		1: return Color(0.45, 0.6, 0.85)
		2: return Color(0.8, 0.7, 0.5)
		3: return Color(0.6, 0.65, 0.7)
		4: return Color(0.55, 0.58, 0.62)
		_: return Color(0.5, 0.5, 0.55)


func _update_world() -> void:
	for b in _body_nodes.size():
		var node: Node3D = _body_nodes[b]
		if node == null:
			continue
		var x := float(sim.body_x(b)) * SCALE3D
		var z := float(sim.body_y(b)) * SCALE3D
		node.position = Vector3(x, 0.0, z)


# ---- top bar -------------------------------------------------------------------

func _build_topbar() -> void:
	_layer = CanvasLayer.new()
	add_child(_layer)

	var bar := PanelContainer.new()
	bar.add_theme_stylebox_override("panel", UiKit.panel_box(UiKit.BG_BAR, UiKit.LINE, 0))
	bar.set_anchors_preset(Control.PRESET_TOP_WIDE)
	bar.custom_minimum_size = Vector2(0, 44)
	_layer.add_child(bar)

	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 18)
	row.alignment = BoxContainer.ALIGNMENT_BEGIN
	bar.add_child(row)

	# Left: company logo placeholder.
	var logo := UiKit.label("◆ TORCH", 16, UiKit.ACCENT)
	logo.custom_minimum_size = Vector2(120, 0)
	row.add_child(logo)

	# Center: resource + asset readouts for the human player.
	for key in ["DATE", "CREDITS", "HAULERS", "MINERS", "COMBAT", "COLONIES", "MINING"]:
		var cell := _make_cell(key)
		row.add_child(cell[0])
		_topbar_labels[key] = cell[1]

	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(spacer)

	# Right: speed controls + escape-menu button.
	for i in SPEEDS.size():
		var si := i
		var b := Button.new()
		b.text = SPEED_LABELS[i]
		b.custom_minimum_size = Vector2(34, 30)
		b.focus_mode = Control.FOCUS_NONE
		b.pressed.connect(func() -> void: speed_idx = si)
		row.add_child(b)
	var esc := Button.new()
	esc.text = "☰"
	esc.custom_minimum_size = Vector2(34, 30)
	esc.focus_mode = Control.FOCUS_NONE
	esc.pressed.connect(_toggle_escape_menu)
	row.add_child(esc)

	_status = UiKit.label("", 11, UiKit.TEXT_DIM)
	_status.set_anchors_preset(Control.PRESET_BOTTOM_LEFT)
	_status.position = Vector2(12, -22)
	_status.anchor_top = 1
	_status.anchor_bottom = 1
	_layer.add_child(_status)


func _make_cell(caption: String) -> Array:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 0)
	v.add_child(UiKit.kicker(caption))
	var val := UiKit.label("—", 14, UiKit.TEXT_HI)
	v.add_child(val)
	return [v, val]


func _refresh_topbar() -> void:
	(_topbar_labels["DATE"] as Label).text = _date_string()
	(_topbar_labels["CREDITS"] as Label).text = _commas(sim.credits())
	(_topbar_labels["HAULERS"] as Label).text = str(sim.count_haulers())
	(_topbar_labels["MINERS"] as Label).text = str(sim.count_miners())
	(_topbar_labels["COMBAT"] as Label).text = str(sim.count_combat())
	(_topbar_labels["COLONIES"] as Label).text = str(sim.count_colonies())
	(_topbar_labels["MINING"] as Label).text = str(sim.count_mining_stations())


func _date_string() -> String:
	# 6 ticks = 1 day; show a simple Y-D readout for now.
	var day: int = int(sim.tick()) / 6
	return "Y%d · D%d" % [2142 + day / 360, day % 360]


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


# ---- escape menu ---------------------------------------------------------------

func _build_escape_menu() -> void:
	_escape_menu = Control.new()
	_escape_menu.set_anchors_preset(Control.PRESET_FULL_RECT)
	_escape_menu.visible = false
	_escape_menu.mouse_filter = Control.MOUSE_FILTER_STOP
	_layer.add_child(_escape_menu)

	var scrim := ColorRect.new()
	scrim.color = Color(0, 0, 0, 0.55)
	scrim.set_anchors_preset(Control.PRESET_FULL_RECT)
	_escape_menu.add_child(scrim)

	var panel := UiKit.make_panel(UiKit.BG_PANEL, UiKit.LINE, 10)
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(320, 0)
	_escape_menu.add_child(panel)

	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	v.add_child(UiKit.label("MENU", 18, UiKit.TEXT_HI))
	v.add_child(UiKit.rule())
	v.add_child(_menu_button("Save Game", _save_game))
	v.add_child(_menu_button("Load Game", _load_game))
	v.add_child(_menu_button("Quit", func(): get_tree().quit()))
	v.add_child(UiKit.rule())
	var ver := TorchCore.new().version()
	v.add_child(UiKit.label("TORCH v%s" % String(ver), 10, UiKit.TEXT_DIM))


func _menu_button(text: String, cb: Callable) -> Button:
	var b := Button.new()
	b.text = text
	b.custom_minimum_size = Vector2(0, 36)
	b.focus_mode = Control.FOCUS_NONE
	b.pressed.connect(cb)
	return b


func _toggle_escape_menu() -> void:
	_escape_menu.visible = not _escape_menu.visible
	if _escape_menu.visible:
		speed_idx = 0


func _save_path() -> String:
	# The Rust core writes via std::fs, so hand it a real OS path, not a Godot user:// URI.
	return ProjectSettings.globalize_path("user://torch_save.bin")


func _save_game() -> void:
	var err := String(sim.save_game(_save_path()))
	_status.text = "Saved." if err == "" else "Save failed: %s" % err


func _load_game() -> void:
	var err := String(sim.load_game(_save_path()))
	_status.text = "Loaded." if err == "" else "Load failed: %s" % err
	if err == "":
		_update_world()
		_refresh_topbar()


# ---- loop ----------------------------------------------------------------------

func _process(delta: float) -> void:
	var mult: float = SPEEDS[speed_idx]
	if mult > 0.0:
		_accum += delta * TICKS_PER_SECOND * mult
		while _accum >= 1.0:
			sim.step()
			_accum -= 1.0
	_update_world()
	_refresh_topbar()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		match event.keycode:
			KEY_ESCAPE:
				_toggle_escape_menu()
			KEY_SPACE:
				speed_idx = 0 if speed_idx > 0 else 1
