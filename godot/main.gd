extends Node3D

## TORCH — minimal shell for the multi-player core rework (iteration 2).
##
## The elaborate 8-view UI was removed; what remains is the 3D orrery (bodies
## orbiting the sun, procedurally shaded, with orbit rings) plus a Paradox-style
## top bar and an escape menu. All game logic lives in the deterministic Rust
## `sim`; this scene drives `step()` on a clock and mirrors body positions into 3D
## nodes. The orrery visuals (shaders / orbit lines / scale / font / zoom) are
## ported from `main`'s richer map.

const UiKit := preload("res://ui/ui_kit.gd")
const PlanetShaders := preload("res://ui/planet_shaders.gd")

# 1 AU = 1 world unit; body positions arrive in millionths of an AU.
const SCALE3D := 1.0 / 1_000_000.0
const SPEEDS := [0.0, 1.0, 6.0, 24.0]          # pause · 1× · 6× · 24×
const SPEED_LABELS := ["❚❚", "▶", "▶▶", "▶▶▶"]
const TICKS_PER_SECOND := 4.0                   # real-time-with-pause base rate
const BAR_H := 30                               # top-bar height (px)

# Camera rig (orbit / pan / zoom around a focus point).
const ZOOM_MIN := 0.05                           # zoom right down onto a single body / its moons
const ZOOM_MAX := 140.0
const ROT_K := 0.008                            # rad per pixel dragged
const PAN_K := 0.0016                           # world units per pixel, per zoom unit

# Orbit-line width: scale the torus tube with the camera distance (zoom) so an orbit ring
# reads as a constant ~hairline at any zoom, instead of a fat band when you zoom in.
const ORBIT_TUBE_K := 0.00060
const ORBIT_TUBE_MIN := 0.00008
const ORBIT_TUBE_MAX := 0.30
# Level-of-detail by zoom (smaller zoom = closer): only reveal moon/station detail once
# zoomed in past this threshold, so the wide system view isn't a clutter of specks.
const MOON_VIS_ZOOM := 1.6
# Labels render at a constant on-screen size (fixed_size) so they're always legible and
# never balloon when you zoom in; this is the per-label scale.
const LABEL_PIXEL := 0.00055
const LABEL_PIXEL_SMALL := 0.00045

var sim: TorchSim
var speed_idx := 1
var _accum := 0.0

# 3D
var _cam: Camera3D
var _orrery_root: Node3D
var _body_nodes: Array[Node3D] = []
var _ship_nodes: Array[Node3D] = []
var _body_spin: Array = []                       # the spinning surface node per body (or null)
var _body_spin_rate: Array[float] = []
var _planet_orbit_rings: Array = []              # [{tm: TorusMesh, r: float}]
var _moon_orbit_rings: Array = []                # [{mi: MeshInstance3D, tm: TorusMesh, r: float}]
var _map_font: Font
var _cam_focus := Vector3.ZERO
var _cam_zoom := 3.8                             # start close on the inner system
var _cam_yaw := 0.0
var _cam_pitch := deg_to_rad(62.0)
var _dragging := false
var _was_drag := false

# HUD
var _layer: CanvasLayer
var _topbar_labels := {}
var _escape_menu: Control
var _status: Label
var _speed_buttons: Array[Button] = []


func _ready() -> void:
	sim = TorchSim.new()
	sim.reset(7)
	_build_world()
	_build_topbar()
	_build_escape_menu()
	_refresh_topbar()


# ---- 3D orrery -----------------------------------------------------------------

func _build_world() -> void:
	# The Protomolecule typeface (The Expanse fan font) for the orrery labels.
	if ResourceLoader.exists("res://assets/fonts/Protomolecule.ttf"):
		_map_font = load("res://assets/fonts/Protomolecule.ttf")

	var env := WorldEnvironment.new()
	var e := Environment.new()
	# Pure-dark backdrop — no starfield. A near-black so the lit bodies and the
	# emissive orbit rings read against it.
	e.background_mode = Environment.BG_COLOR
	e.background_color = Color(0.01, 0.012, 0.02)
	# Bodies are lit in-shader from Sol at the origin, so the scene needs no engine
	# lights; a faint ambient keeps the unlit far rims from going pure black.
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color(0.18, 0.22, 0.32)
	e.ambient_light_energy = 0.18
	# Bloom so the sun, atmospheres and emissive rings glow (HDR ALBEDO > 1 → glow).
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
	_cam.far = 8000.0
	add_child(_cam)
	_update_camera()   # framed close on the inner system; orbit/pan/zoom from input

	_orrery_root = Node3D.new()
	add_child(_orrery_root)

	for b in sim.body_count():
		var kind: int = sim.body_kind(b)
		var bname: String = String(sim.body_name(b))
		var container := _spawn_body(b, bname, kind)
		_orrery_root.add_child(container)
		_body_nodes.append(container)
		# Orbit rings: planets/dwarfs/stations trace their orbit; asteroids and the star
		# carry no ring (the Belt reads as a field, not a wheel).
		var parent: int = sim.body_parent(b)
		if kind != 0 and kind != 7:
			if parent == 0:
				var r := _world3d(sim.body_x(b), sim.body_y(b)).length()
				var pr := _ring(r, Color(0.24, 0.33, 0.48))
				_orrery_root.add_child(pr)
				_planet_orbit_rings.append({"tm": pr.mesh, "r": r})
			elif parent >= 0 and parent < _body_nodes.size():
				var mr: float = float(sim.body_orbit_radius(b)) * SCALE3D
				# Moon/station orbit: a hair-thin, faintly glowing line around its primary.
				var mrm := _emissive_mat(Color(0.3, 0.38, 0.5) * 2.0)
				var mring := _ring_mat(mr, mrm, maxf(0.0022, mr * 0.006))
				_body_nodes[parent].add_child(mring)
				_moon_orbit_rings.append({"mi": mring, "tm": mring.mesh, "r": mr})
		# Billboard label, constant on-screen size, in the map typeface.
		var rad := _display_radius(bname, kind)
		var tag := Label3D.new()
		tag.text = bname
		tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		tag.fixed_size = true
		if _map_font != null:
			tag.font = _map_font
		var small := (kind == 4 or kind == 7 or kind == 8)
		tag.modulate = Color(0.6, 0.7, 0.78) if small else Color(0.72, 0.84, 0.95)
		tag.pixel_size = LABEL_PIXEL_SMALL if small else LABEL_PIXEL
		tag.position = Vector3(0, rad + 0.05, 0)
		container.add_child(tag)

	# Saturn's rings (parented to its tilted surface).
	for b in sim.body_count():
		if String(sim.body_name(b)) == "Saturn" and _body_spin[b] != null:
			_build_saturn_rings(_body_spin[b])
			break
	_build_asteroid_belt()
	_build_ships()
	_update_world(0.0)


# Player-entity marker colours (by owner id): Human, Earth, Mars, OPA, two companies,
# private sector, pirates.
const PLAYER_COL := [
	Color(0.30, 0.84, 0.92), Color(0.40, 0.60, 0.95), Color(0.90, 0.35, 0.30),
	Color(0.92, 0.78, 0.36), Color(0.45, 0.80, 0.50), Color(0.55, 0.85, 0.65),
	Color(0.6, 0.62, 0.66), Color(0.7, 0.25, 0.30),
]


func _build_ships() -> void:
	for i in sim.ship_count():
		var m := MeshInstance3D.new()
		var box := BoxMesh.new()
		var sz := 0.05 if sim.ship_class(i) == 2 else 0.035   # combat a touch bigger
		box.size = Vector3(sz, sz * 0.5, sz)
		m.mesh = box
		var mat := _emissive_mat(PLAYER_COL[clampi(sim.ship_owner(i), 0, PLAYER_COL.size() - 1)])
		m.material_override = mat
		_orrery_root.add_child(m)
		_ship_nodes.append(m)


## Build one celestial body: a positioned container holding a tilted, spinning,
## procedurally-shaded surface sphere plus an atmospheric glow shell where the world
## has an atmosphere. Records the surface + spin rate for the frame loop. Stations
## (kind 8) get a simple emissive marker instead of a shaded planet.
func _spawn_body(b: int, bname: String, kind: int) -> Node3D:
	var container := Node3D.new()
	var rad := _display_radius(bname, kind)
	if kind == 8:
		var marker := _sphere(rad, _emissive_mat(Color(0.45, 0.85, 0.95)))
		container.add_child(marker)
		_body_spin.append(null)
		_body_spin_rate.append(0.0)
		return container
	var surf := _sphere(rad, _make_body_material(bname, kind))
	# Lean the spin axis (axial tilt) — Uranus rolls on its side, Earth a gentle 23°.
	surf.rotation_degrees = Vector3(0.0, 0.0, _axial_tilt(bname))
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
	_body_spin_rate.append(_spin_rate(bname, kind))
	var atmo := _atmosphere_for(bname, kind, rad)
	if atmo != null:
		container.add_child(atmo)
	return container


## A deterministic ring of tumbling rocks between Mars and Jupiter, so the Belt looks
## inhabited, not empty.
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


func _build_saturn_rings(saturn: Node3D) -> void:
	# Ring extent scales with Saturn's display size (~1.2–2.35 planet radii).
	var R := _display_radius("Saturn", 2)
	var r_in := R * 1.2
	var r_out := R * 2.35
	var ring := MeshInstance3D.new()
	ring.mesh = _flat_ring_mesh(r_in, r_out, 120)
	ring.material_override = PlanetShaders.rings(r_in, r_out, Color(1.0, 0.95, 0.85))
	saturn.add_child(ring)
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


# ---- body appearance specs --------------------------------------------------

# Apparent display radii (world units). Not true scale — at 1 AU = 1 unit the real
# bodies would be invisible specks — but the *relative* sizes are honest, so the system
# reads more to scale. Sized so the inner orbits clear the sun (Mercury orbits at 0.387).
const _RADII := {
	"Sol": 0.26,
	"Mercury": 0.026, "Venus": 0.048, "Earth": 0.05, "Mars": 0.034,
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


func _display_radius(bname: String, kind: int) -> float:
	if _RADII.has(bname):
		return float(_RADII[bname])
	match kind:
		0: return 0.26
		1: return 0.038
		2: return 0.14
		3: return 0.02
		4: return 0.013
		7: return 0.013
		8: return 0.012
	return 0.03


func _make_body_material(bname: String, kind: int) -> ShaderMaterial:
	match bname:
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
	if _ICY.has(bname):
		return PlanetShaders.rocky(Color(0.82, 0.86, 0.92), Color(0.5, 0.58, 0.68), 0.55, 0.0, Color.WHITE)
	match kind:
		2:
			return PlanetShaders.gas_giant(Color(0.7, 0.62, 0.5), Color(0.86, 0.8, 0.66),
				Color(0.55, 0.46, 0.36), Color(0.7, 0.66, 0.58), 0.0, Color.WHITE)
		3:
			return PlanetShaders.rocky(Color(0.66, 0.6, 0.54), Color(0.4, 0.36, 0.32), 0.6, 0.15, Color(0.82, 0.82, 0.8))
		7:
			return PlanetShaders.rocky(Color(0.46, 0.4, 0.34), Color(0.22, 0.19, 0.16), 0.95, 0.0, Color.WHITE)
	return PlanetShaders.rocky(Color(0.6, 0.58, 0.55), Color(0.34, 0.33, 0.31), 0.7, 0.0, Color.WHITE)


func _axial_tilt(bname: String) -> float:
	match bname:
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


func _spin_rate(_bname: String, kind: int) -> float:
	match kind:
		0: return 0.0     # the sun's surface is animated in-shader
		2: return 0.5     # gas giants whirl
		1: return 0.13
		3: return 0.10
		4: return 0.08
		7: return 0.35    # rubble-pile asteroids tumble
	return 0.10


## A thin additive atmospheric-glow shell around bodies with an atmosphere, or null.
func _atmosphere_for(bname: String, kind: int, rad: float) -> MeshInstance3D:
	var col := Color.BLACK
	var inten := 0.0
	match bname:
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


# ---- mesh / material helpers ---------------------------------------------------

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


func _ring(radius: float, col: Color) -> MeshInstance3D:
	# A thin orbit line with a faint glow — the emission tips just into the bloom pass.
	return _ring_mat(radius, _emissive_mat(col * 2.4), 0.005)


func _ring_mat(radius: float, mat: StandardMaterial3D, tube: float) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var tm := TorusMesh.new()
	tm.inner_radius = maxf(0.01, radius - tube)
	tm.outer_radius = radius + tube
	# Plenty of segments around the ring so a large orbit reads as a smooth circle, not a
	# stepped polygon; the tube itself needs only a few sides (it's a hairline).
	tm.rings = clampi(int(radius * 48.0), 96, 384)
	tm.ring_segments = 6
	mi.mesh = tm
	mi.material_override = mat
	return mi


func _world3d(wx: float, wy: float) -> Vector3:
	return Vector3(wx * SCALE3D, 0.0, -wy * SCALE3D)


func _screen(p: Vector3) -> Vector2:
	return _cam.unproject_position(p)


func _update_world(delta: float) -> void:
	for b in _body_nodes.size():
		var node: Node3D = _body_nodes[b]
		if node == null:
			continue
		node.position = _world3d(sim.body_x(b), sim.body_y(b))
		# Spin the body on its (tilted) axis — purely cosmetic, so it turns even while
		# the sim is paused.
		if b < _body_spin.size() and _body_spin[b] != null and _body_spin_rate[b] != 0.0:
			(_body_spin[b] as Node3D).rotate_object_local(Vector3.UP, delta * _body_spin_rate[b])
		# Moons / stations drop out at wide zoom (LOD) so the system view isn't cluttered.
		var kind: int = sim.body_kind(b)
		if kind == 4 or kind == 8:
			node.visible = _cam_zoom < MOON_VIS_ZOOM
	# Ships (lerped along flight legs by the core); lift in-flight ones off the ecliptic.
	for i in _ship_nodes.size():
		var sn: Node3D = _ship_nodes[i]
		var p := _world3d(sim.ship_x(i), sim.ship_y(i))
		if sim.ship_in_flight(i):
			p.y = 0.04
		sn.position = p
	_update_orrery_lod()


## Per-frame zoom-constant orbit-line width + moon LOD: keep every orbit ring a hairline
## at any zoom, and reveal moon detail only once the player has zoomed in.
func _update_orrery_lod() -> void:
	var tube := clampf(_cam_zoom * ORBIT_TUBE_K, ORBIT_TUBE_MIN, ORBIT_TUBE_MAX)
	for ring in _planet_orbit_rings:
		var tm: TorusMesh = ring["tm"]
		var r: float = ring["r"]
		tm.inner_radius = maxf(0.001, r - tube)
		tm.outer_radius = r + tube
	var show_moons := _cam_zoom < MOON_VIS_ZOOM
	for ring in _moon_orbit_rings:
		var mtm: TorusMesh = ring["tm"]
		var mr: float = ring["r"]
		var mt := minf(tube, mr * 0.4)
		mtm.inner_radius = maxf(0.001, mr - mt)
		mtm.outer_radius = mr + mt
		(ring["mi"] as MeshInstance3D).visible = show_moons


## Centre the camera on the nearest body to a screen position (click-to-focus).
func _pick_body(pos: Vector2) -> void:
	var best := -1
	var best_d := 40.0
	for b in sim.body_count():
		var d := _screen(_world3d(sim.body_x(b), sim.body_y(b))).distance_to(pos)
		if d < best_d:
			best_d = d
			best = b
	if best < 0:
		return
	_cam_focus = _world3d(sim.body_x(best), sim.body_y(best))
	_update_camera()
	_status.text = "Focus: %s." % String(sim.body_name(best))


## Position the camera from its spherical orbit (yaw/pitch/zoom) around the focus point.
func _update_camera() -> void:
	_cam_pitch = clampf(_cam_pitch, deg_to_rad(15.0), deg_to_rad(85.0))
	var cp := cos(_cam_pitch)
	var dir := Vector3(cp * sin(_cam_yaw), sin(_cam_pitch), cp * cos(_cam_yaw))
	_cam.position = _cam_focus + dir * _cam_zoom
	_cam.look_at(_cam_focus, Vector3.UP)


# ---- top bar -------------------------------------------------------------------

func _build_topbar() -> void:
	_layer = CanvasLayer.new()
	add_child(_layer)

	var bar := PanelContainer.new()
	bar.add_theme_stylebox_override("panel", UiKit.bar_box(UiKit.BG_BAR, UiKit.BG))
	bar.set_anchors_preset(Control.PRESET_TOP_WIDE)
	bar.custom_minimum_size = Vector2(0, BAR_H)
	_layer.add_child(bar)

	# A faint accent top-highlight + crisp bottom hairline + a very faint bloom below the bar.
	var hi := ColorRect.new()
	hi.color = Color(UiKit.ACCENT.r, UiKit.ACCENT.g, UiKit.ACCENT.b, 0.22)
	hi.set_anchors_preset(Control.PRESET_TOP_WIDE)
	hi.offset_bottom = 1.0
	hi.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_layer.add_child(hi)
	var hair := ColorRect.new()
	hair.color = UiKit.LINE_HI
	hair.set_anchors_preset(Control.PRESET_TOP_WIDE)
	hair.offset_top = float(BAR_H)
	hair.offset_bottom = float(BAR_H) + 1.0
	hair.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_layer.add_child(hair)
	var glow := TextureRect.new()
	glow.texture = UiKit.vgrad(
		Color(UiKit.ACCENT.r, UiKit.ACCENT.g, UiKit.ACCENT.b, 0.10),
		Color(UiKit.ACCENT.r, UiKit.ACCENT.g, UiKit.ACCENT.b, 0.0))
	glow.set_anchors_preset(Control.PRESET_TOP_WIDE)
	glow.offset_top = float(BAR_H) + 1.0
	glow.offset_bottom = float(BAR_H) + 13.0
	glow.stretch_mode = TextureRect.STRETCH_SCALE
	glow.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var gm := CanvasItemMaterial.new()
	gm.blend_mode = CanvasItemMaterial.BLEND_MODE_ADD
	glow.material = gm
	_layer.add_child(glow)

	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	row.alignment = BoxContainer.ALIGNMENT_BEGIN
	bar.add_child(row)

	# Left: hexagonal badge — placeholder frame for the player's logo.
	row.add_child(UiKit.hex_badge(float(BAR_H) - 8.0))

	# Center: resource + asset readouts for the human player.
	for key in ["DATE", "CREDITS", "HAULERS", "MINERS", "COMBAT", "COLONIES", "MINING"]:
		var cell := _make_cell(key)
		row.add_child(cell[0])
		_topbar_labels[key] = cell[1]

	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(spacer)

	# Right: speed controls (current speed marked) + escape-menu button.
	_speed_buttons.clear()
	for i in SPEEDS.size():
		var si := i
		var b := UiKit.speed_button(SPEED_LABELS[i])
		b.custom_minimum_size = Vector2(26, 22)
		b.pressed.connect(func() -> void:
			speed_idx = si
			_sync_speed_buttons())
		row.add_child(b)
		_speed_buttons.append(b)
	var esc := Button.new()
	esc.text = "☰"
	esc.custom_minimum_size = Vector2(26, 22)
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
	var val := UiKit.label("—", 11, UiKit.TEXT_HI)
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
	_sync_speed_buttons()


## Mark the active speed button (keeps spacebar / escape-menu speed changes reflected).
func _sync_speed_buttons() -> void:
	for i in _speed_buttons.size():
		_speed_buttons[i].button_pressed = (i == speed_idx)


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
		_update_world(0.0)
		_refresh_topbar()


# ---- loop ----------------------------------------------------------------------

func _process(delta: float) -> void:
	var mult: float = SPEEDS[speed_idx]
	if mult > 0.0:
		_accum += delta * TICKS_PER_SECOND * mult
		while _accum >= 1.0:
			sim.step()
			_accum -= 1.0
	_update_world(delta)
	_refresh_topbar()


func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		match event.keycode:
			KEY_ESCAPE:
				_toggle_escape_menu()
			KEY_SPACE:
				speed_idx = 0 if speed_idx > 0 else 1
		return
	# Camera: wheel = zoom, left-drag = pan, shift+left-drag = orbit, click = focus.
	if event is InputEventMouseButton:
		if event.button_index == MOUSE_BUTTON_WHEEL_UP and event.pressed:
			_cam_zoom = clampf(_cam_zoom * 0.9, ZOOM_MIN, ZOOM_MAX)
			_update_camera()
		elif event.button_index == MOUSE_BUTTON_WHEEL_DOWN and event.pressed:
			_cam_zoom = clampf(_cam_zoom * 1.1, ZOOM_MIN, ZOOM_MAX)
			_update_camera()
		elif event.button_index == MOUSE_BUTTON_LEFT:
			if event.pressed:
				_dragging = true
				_was_drag = false
			else:
				_dragging = false
				if not _was_drag:
					_pick_body(event.position)
	elif event is InputEventMouseMotion and _dragging:
		var rel: Vector2 = event.relative
		if rel.length() > 2.0:
			_was_drag = true
		if event.shift_pressed:
			_cam_yaw -= rel.x * ROT_K
			_cam_pitch += rel.y * ROT_K
		else:
			# Pan along the camera's flattened right/forward axes so the world tracks the cursor.
			var right := _cam.global_transform.basis.x
			var fwd := _cam.global_transform.basis.z
			right.y = 0.0
			fwd.y = 0.0
			right = right.normalized()
			fwd = fwd.normalized()
			var k := _cam_zoom * PAN_K
			_cam_focus += (-right * rel.x + fwd * rel.y) * k
		_update_camera()
