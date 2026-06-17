class_name ShipForge
extends RefCounted

## Procedural Expanse-style ship generator (GDD §24/§25).
##
## Builds a Node3D warship from primitives following a shape grammar: a modular
## spine of stacked box/cylinder hull sections, an aft drive cluster with a glowing
## plume, a forward bridge, radiator fins, plating greebles, and **named weapon
## hardpoints** that carry their own weapon models (PDC turrets / torpedo launchers /
## railguns). Deterministic from a seed; the faction is a parameter set (Earth/Mars/
## Belt/Independent) so a Martian hull reads differently from an Earther one.
##
## Pure shell art — no sim/determinism dependency. The §25 "bake to optimized mesh"
## step is a later pass; this is the runtime-assembly realization.

# Faction palettes: hull / accent / trim (the §4 visual signatures).
const PALETTE := {
	0: {"hull": Color(0.80, 0.84, 0.88), "accent": Color(0.30, 0.40, 0.55), "name": "Earth"},   # utilitarian blue-grey
	1: {"hull": Color(0.74, 0.66, 0.60), "accent": Color(0.62, 0.26, 0.18), "name": "Mars"},    # rust-red, weapon-forward
	2: {"hull": Color(0.72, 0.62, 0.42), "accent": Color(0.82, 0.48, 0.16), "name": "Belt"},    # ochre, welded/salvaged
	3: {"hull": Color(0.80, 0.80, 0.82), "accent": Color(0.85, 0.49, 0.16), "name": "Indie"},   # grey + classic orange
}
const TRIM := Color(0.11, 0.11, 0.13)
const DARK := Color(0.06, 0.06, 0.07)


static func _mat(col: Color, rough := 0.65, metal := 0.45) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = col
	m.roughness = rough
	m.metallic = metal
	return m


static func _emissive(col: Color, energy := 3.0) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = col
	m.emission_enabled = true
	m.emission = col
	m.emission_energy_multiplier = energy
	return m


# A diagonal hazard-stripe material (the orange/black warning bands on the hulls).
static func _hazard() -> StandardMaterial3D:
	var img := Image.create(32, 32, false, Image.FORMAT_RGB8)
	for y in 32:
		for x in 32:
			var band := int((x + y) / 5.0) % 2
			img.set_pixel(x, y, Color(0.92, 0.66, 0.10) if band == 0 else Color(0.07, 0.07, 0.08))
	var tex := ImageTexture.create_from_image(img)
	var m := StandardMaterial3D.new()
	m.albedo_texture = tex
	m.roughness = 0.6
	m.metallic = 0.2
	return m


static func _box(parent: Node3D, pos: Vector3, size: Vector3, mat: Material) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = size
	mi.mesh = bm
	mi.position = pos
	mi.material_override = mat
	parent.add_child(mi)
	return mi


# A cylinder aligned to an axis ("z" = along the ship's length, "y" = upright).
static func _cyl(parent: Node3D, pos: Vector3, radius: float, height: float, mat: Material, axis := "z") -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var cm := CylinderMesh.new()
	cm.top_radius = radius
	cm.bottom_radius = radius
	cm.height = height
	cm.radial_segments = 12
	mi.mesh = cm
	if axis == "z":
		mi.rotation_degrees = Vector3(90, 0, 0)
	mi.position = pos
	mi.material_override = mat
	parent.add_child(mi)
	return mi


# A thin rib ring around a drum section (the ribbed reactor/engine look).
static func _ring(parent: Node3D, pos: Vector3, radius: float, mat: Material) -> void:
	var mi := MeshInstance3D.new()
	var cm := CylinderMesh.new()
	cm.top_radius = radius
	cm.bottom_radius = radius
	cm.height = 0.035
	cm.radial_segments = 14
	mi.mesh = cm
	mi.rotation_degrees = Vector3(90, 0, 0)
	mi.position = pos
	mi.material_override = mat
	parent.add_child(mi)


# A small sensor dome / dish atop the command tower.
static func _dome(parent: Node3D, pos: Vector3, r: float, mat: Material) -> void:
	var mi := MeshInstance3D.new()
	var sm := SphereMesh.new()
	sm.radius = r
	sm.height = r * 1.4
	sm.radial_segments = 10
	sm.rings = 5
	mi.mesh = sm
	mi.position = pos
	mi.material_override = mat
	parent.add_child(mi)


# ---- weapon models on hardpoints ------------------------------------------

static func _pdc(parent: Node3D, pos: Vector3, mat: Material) -> void:
	_box(parent, pos, Vector3(0.13, 0.05, 0.13), mat)
	_cyl(parent, pos + Vector3(0, 0.05, 0), 0.05, 0.06, mat, "y")
	for bx in [-0.02, 0.02]:
		_cyl(parent, pos + Vector3(bx, 0.09, 0.07), 0.012, 0.16, _mat(DARK, 0.5, 0.7), "z")


static func _torpedo(parent: Node3D, pos: Vector3, mat: Material) -> void:
	_box(parent, pos, Vector3(0.2, 0.14, 0.24), mat)
	var dark := _mat(DARK, 0.5, 0.6)
	for ox in [-0.05, 0.05]:
		for oy in [-0.035, 0.035]:
			_cyl(parent, pos + Vector3(ox, oy, 0.12), 0.022, 0.05, dark, "z")


static func _railgun(parent: Node3D, pos: Vector3, mat: Material, length: float) -> void:
	_box(parent, pos, Vector3(0.17, 0.13, 0.34), mat)          # breech
	_cyl(parent, pos + Vector3(0, 0.02, length * 0.5 + 0.18), 0.035, length, _mat(TRIM, 0.45, 0.7), "z")  # barrel


# A railgun **turret** (Cruiser/Battleship) — a rotatable mount on the dorsal hull,
# vs the Destroyer's fixed spinal gun.
static func _railgun_turret(parent: Node3D, pos: Vector3, mat: Material, length: float) -> void:
	_cyl(parent, pos, 0.1, 0.05, mat, "y")                     # turret ring base
	_box(parent, pos + Vector3(0, 0.05, 0), Vector3(0.16, 0.09, 0.2), mat)   # turret body
	# Twin barrels jutting forward, angled slightly up.
	for bx in [-0.03, 0.03]:
		var barrel := _cyl(parent, pos + Vector3(bx, 0.08, length * 0.5), 0.022, length, _mat(TRIM, 0.45, 0.7), "z")
		barrel.rotation_degrees = Vector3(-8, 0, 0)


# ---- civilian ships (A4) ---------------------------------------------------

## A civilian hull (no weapons): kind 0 = freighter (stacked cargo containers),
## 1 = miner (blunt hull + forward mining rig), 2 = tanker (big fuel drums). Faction
## tints the livery. The trade backbone + prime interdiction targets (§8e).
static func build_civilian(kind: int, faction: int, seed: int) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed
	var root := Node3D.new()
	var pal: Dictionary = PALETTE.get(faction, PALETTE[3])
	var hull_mat := _mat(pal["hull"])
	var trim_mat := _mat(TRIM, 0.8, 0.5)
	var accent_mat := _mat(pal["accent"], 0.55, 0.4)
	var glow := _emissive(Color(0.45, 0.72, 1.0), 4.0)

	# A long thin spine all civilians share.
	var L := 4.2
	_box(root, Vector3(0, -0.18, 0), Vector3(0.16, 0.18, L * 0.96), trim_mat)

	if kind == 2:
		# Tanker: a row of big cylindrical fuel drums.
		var n := 4
		for i in n:
			var z: float = lerpf(-L * 0.4, L * 0.34, float(i) / float(n - 1))
			_cyl(root, Vector3(0, 0.2, z), 0.34, L / float(n) * 0.82, hull_mat, "z")
			_box(root, Vector3(0, 0.2, z), Vector3(0.7, 0.02, 0.06), accent_mat)
	elif kind == 1:
		# Miner: a blunt blocky hull + a forward mining rig (frame + drill).
		for i in 3:
			var z2: float = lerpf(-L * 0.3, L * 0.25, float(i) / 2.0)
			_box(root, Vector3(0, 0.1, z2), Vector3(0.66, 0.5, L / 3.2), hull_mat)
		for sx in [-1.0, 1.0]:
			_box(root, Vector3(sx * 0.28, 0.1, L * 0.46), Vector3(0.06, 0.06, 0.5), trim_mat)
		_cyl(root, Vector3(0, 0.1, L * 0.62), 0.1, 0.5, _mat(Color(0.7, 0.5, 0.2), 0.5, 0.6), "z")
	else:
		# Freighter: a stack of mixed cargo containers on the spine.
		var cols := [Color(0.7, 0.4, 0.2), Color(0.3, 0.5, 0.6), Color(0.6, 0.6, 0.62), Color(0.4, 0.45, 0.3)]
		for i in 7:
			var z3: float = lerpf(-L * 0.4, L * 0.28, float(i) / 6.0)
			var cm: Material = _mat(cols[rng.randi() % cols.size()], 0.75, 0.2)
			_box(root, Vector3(rng.randf_range(-0.05, 0.05), 0.18, z3), Vector3(0.5, 0.34, L / 7.4), cm)
		# Forward bridge pod.
		_box(root, Vector3(0, 0.36, L * 0.42), Vector3(0.34, 0.18, 0.4), hull_mat)

	# Shared aft drive + plume (smaller than a warship's).
	var az := -L * 0.5
	_cyl(root, Vector3(0, 0.0, az), 0.16, 0.2, trim_mat, "z")
	var cone := MeshInstance3D.new()
	var ccm := CylinderMesh.new()
	ccm.top_radius = 0.04
	ccm.bottom_radius = 0.13
	ccm.height = 0.16
	ccm.radial_segments = 10
	cone.mesh = ccm
	cone.rotation_degrees = Vector3(-90, 0, 0)
	cone.position = Vector3(0, 0, az - 0.14)
	cone.material_override = glow
	root.add_child(cone)
	return root


# ---- stations (A4) ---------------------------------------------------------

## A modular station: a central spine of hab/industrial drums, radial solar-panel
## wings, docking arms, and a slow spin. `tier` 0..2 scales it; faction tints it.
static func build_station(faction: int, tier: int, seed: int) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed
	var root := Node3D.new()
	var pal: Dictionary = PALETTE.get(faction, PALETTE[3])
	var hull_mat := _mat(pal["hull"])
	var trim_mat := _mat(TRIM, 0.8, 0.5)
	var accent_mat := _mat(pal["accent"], 0.55, 0.4)
	var solar_mat := _mat(Color(0.10, 0.12, 0.28), 0.3, 0.7)

	# Central stack of habitat drums (the core), upright along Y.
	var drums := 2 + tier
	for i in drums:
		var y: float = (float(i) - float(drums - 1) * 0.5) * 0.6
		var r: float = 0.5 + 0.12 * sin(float(i))
		_cyl(root, Vector3(0, y, 0), r, 0.5, hull_mat if i % 2 == 0 else trim_mat, "y")
		_cyl(root, Vector3(0, y, 0), r + 0.01, 0.06, accent_mat, "y")    # banding
	# Radial docking arms + solar wings around the core.
	var arms := 4 + tier
	for a in arms:
		var ang: float = TAU * float(a) / float(arms)
		var dir := Vector3(cos(ang), 0, sin(ang))
		var arm := _box(root, dir * 0.95, Vector3(1.0, 0.08, 0.08), trim_mat)
		arm.look_at_from_position(dir * 0.95, Vector3.ZERO, Vector3.UP)
		# A solar/radiator panel at the end of every other arm.
		if a % 2 == 0:
			var panel := _box(root, dir * 1.6, Vector3(0.9, 0.02, 0.5), solar_mat)
			panel.look_at_from_position(dir * 1.6, Vector3.ZERO, Vector3.UP)
		else:
			# A docking pod.
			_box(root, dir * 1.45, Vector3(0.26, 0.26, 0.26), hull_mat)
	# A beacon light.
	var beacon := OmniLight3D.new()
	beacon.light_color = pal["accent"]
	beacon.light_energy = 1.5
	beacon.omni_range = 6.0
	root.add_child(beacon)
	return root


# ---- the warship forge -----------------------------------------------------

## Build a ship Node3D. class_idx 0..3 = Frigate..Battleship; faction 0..3; the
## mount counts come from the sim's hull (TorchShipyard); seed makes it deterministic.
static func build(class_idx: int, faction: int, pdc: int, torpedo: int, railgun: int, seed: int) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed
	var root := Node3D.new()

	var pal: Dictionary = PALETTE.get(faction, PALETTE[3])
	var hull_col: Color = pal["hull"]
	var accent_col: Color = pal["accent"]
	# Weathered-industrial palette (the reference look): white-grey plating, a darker
	# mid plate, rust-orange accents, dark metal recesses, dark sensor glass.
	var hull_mat := _mat(hull_col, 0.7, 0.35)
	var plate_mat := _mat(hull_col * 0.7, 0.75, 0.4)
	var accent_mat := _mat(accent_col, 0.6, 0.4)
	var trim_mat := _mat(TRIM, 0.8, 0.5)
	var panel_mat := _mat(DARK, 0.6, 0.6)
	var dome_mat := _emissive(Color(0.25, 0.55, 0.7), 0.6)
	var hazard := _hazard()
	var glow := _emissive(Color(0.45, 0.72, 1.0), 4.0)

	# Class envelope: bigger ships are longer, with more modular sections. Expanse
	# hulls are **tower-like** (built around thrust gravity) — slim for their length,
	# the engine the wide base, the bow the narrow top.
	var t: float = float(class_idx) / 3.0
	var total_len: float = lerpf(3.2, 6.4, t)
	var width: float = lerpf(0.5, 0.92, t)
	var sections: int = 4 + class_idx                 # 4 (corvette) .. 7 (battleship)
	if faction == 1:    # Mars — longer, leaner
		total_len *= 1.12
		width *= 0.9
	elif faction == 2:  # Belt — chunkier
		width *= 1.1

	# Lower keel hull (the wide armored base the modules ride on).
	_box(root, Vector3(0, -width * 0.22, 0), Vector3(width * 0.62, width * 0.34, total_len * 0.9), trim_mat)

	# Stacked hull modules from aft (-Z) to fore (+Z), layered (lower body + upper deck).
	var seg: float = total_len / float(sections)
	var z: float = -total_len * 0.5 + seg * 0.55
	var top_y: Array[float] = []
	var seg_z: Array[float] = []
	for i in sections:
		var frac: float = float(i) / float(sections - 1)        # 0 aft .. 1 fore
		var w: float = width * lerpf(1.0, 0.66, frac) * rng.randf_range(0.94, 1.04)
		var h: float = width * lerpf(0.92, 0.58, frac) * rng.randf_range(0.95, 1.04)
		var is_drum: bool = (i % 3 == 1) and frac < 0.7          # ribbed reactor drums (mid/aft)
		if is_drum:
			var dr: float = maxf(w, h) * 0.52
			_cyl(root, Vector3(0, 0, z), dr, seg * 0.92, hull_mat, "z")
			for k in 3:                                          # rib rings
				_ring(root, Vector3(0, 0, z + lerpf(-seg * 0.3, seg * 0.3, float(k) / 2.0)), dr + 0.01, trim_mat)
		else:
			_box(root, Vector3(0, -h * 0.08, z), Vector3(w, h * 0.7, seg * 0.94), hull_mat)   # lower body
			_box(root, Vector3(0, h * 0.4, z), Vector3(w * 0.74, h * 0.36, seg * 0.78), plate_mat)  # upper deck
		# Rust-orange accent stripe down each flank + a hazard band on alternate decks.
		for sx in [-1.0, 1.0]:
			_box(root, Vector3(sx * w * 0.5, 0, z), Vector3(0.025, h * 0.5, seg * 0.66), accent_mat)
		if i % 2 == 0 and not is_drum:
			_box(root, Vector3(0, h * 0.58 + 0.004, z), Vector3(w * 0.5, 0.012, seg * 0.34), hazard)
		# Plating greebles (panel detail) on the decks and flanks.
		for _g in range(3 + class_idx):
			var gx: float = rng.randf_range(-w * 0.46, w * 0.46)
			var gy: float = (h * 0.58) * (1.0 if rng.randf() > 0.45 else -0.5)
			var gz: float = z + rng.randf_range(-seg * 0.38, seg * 0.38)
			var gs: float = rng.randf_range(0.035, 0.09)
			_box(root, Vector3(gx, gy, gz), Vector3(gs, 0.018, gs * 1.5), panel_mat if rng.randf() > 0.5 else trim_mat)
		top_y.append(h * 0.58)
		seg_z.append(z)
		z += seg

	# Pointed prow: a stepped taper to a nose cap + a thin forward sensor mast.
	var fz: float = seg_z[sections - 1]
	var fy: float = top_y[sections - 1]
	_box(root, Vector3(0, -width * 0.04, fz + seg * 0.55), Vector3(width * 0.34, width * 0.28, seg * 0.5), hull_mat)
	_box(root, Vector3(0, -width * 0.02, fz + seg * 0.86), Vector3(width * 0.16, width * 0.16, seg * 0.34), plate_mat)
	_cyl(root, Vector3(0, 0, fz + seg * 1.15), 0.012, seg * 0.6, trim_mat, "z")    # forward mast
	_dome(root, Vector3(0, 0, fz + seg * 1.42), 0.03, dome_mat)                    # sensor tip

	# Command tower amidships-aft: a stepped superstructure with sensor domes + a dish.
	var tz: float = lerpf(seg_z[0], fz, 0.62)
	var ty: float = width * 0.52
	_box(root, Vector3(0, ty, tz), Vector3(width * 0.4, width * 0.3, seg * 0.9), plate_mat)
	_box(root, Vector3(0, ty + width * 0.22, tz - seg * 0.1), Vector3(width * 0.26, width * 0.18, seg * 0.5), hull_mat)
	_dome(root, Vector3(width * 0.08, ty + width * 0.36, tz - seg * 0.1), 0.04, dome_mat)
	_dome(root, Vector3(-width * 0.07, ty + width * 0.34, tz), 0.03, dome_mat)
	var dish := MeshInstance3D.new()                                              # radar dish
	var dm := CylinderMesh.new()
	dm.top_radius = width * 0.12
	dm.bottom_radius = width * 0.12
	dm.height = 0.02
	dm.radial_segments = 12
	dish.mesh = dm
	dish.rotation_degrees = Vector3(60, 0, 0)
	dish.position = Vector3(0, ty + width * 0.4, tz - seg * 0.1)
	dish.material_override = plate_mat
	root.add_child(dish)

	# Fat aft engine block (the wide "base" of the tower): a ribbed drum + the drive
	# cluster. Epstein-drive count by class — Corvette/Destroyer 1, Cruiser/Battleship 4
	# (a 2×2 cluster, like the Pella/Donnager).
	var az: float = -total_len * 0.5
	var eb: float = width * 0.66
	_cyl(root, Vector3(0, 0, az + 0.02), eb, 0.34, trim_mat, "z")
	for k in 3:
		_ring(root, Vector3(0, 0, az + lerpf(-0.12, 0.12, float(k) / 2.0)), eb + 0.012, panel_mat)
	var bell_pos: Array = []
	var s: float = width * 0.26
	var bsize: float = width * 0.17
	if class_idx >= 2:                                   # Cruiser/Battleship: 4 drives
		bell_pos = [Vector2(-s, -s), Vector2(s, -s), Vector2(-s, s), Vector2(s, s)]
		bsize = width * 0.13
	else:                                                # Corvette/Destroyer: 1 drive
		bell_pos = [Vector2(0, 0)]
	for bp in bell_pos:
		_cyl(root, Vector3(bp.x, bp.y, az - 0.14), bsize, 0.18, panel_mat, "z")
		var cone := MeshInstance3D.new()
		var cm := CylinderMesh.new()
		cm.top_radius = bsize * 0.25
		cm.bottom_radius = bsize * 0.95
		cm.height = 0.18
		cm.radial_segments = 12
		cone.mesh = cm
		cone.rotation_degrees = Vector3(-90, 0, 0)
		cone.position = Vector3(bp.x, bp.y, az - 0.28)
		cone.material_override = glow
		root.add_child(cone)
	var plume := OmniLight3D.new()
	plume.position = Vector3(0, 0, az - 0.32)
	plume.light_color = Color(0.5, 0.75, 1.0)
	plume.light_energy = 2.4
	plume.omni_range = total_len
	root.add_child(plume)

	# Radiator fins (thin angled panels mid-hull).
	for sx in [-1.0, 1.0]:
		var fin := _box(root, Vector3(sx * width * 0.62, 0.0, -total_len * 0.1), Vector3(0.02, width * 0.5, total_len * 0.26), _mat(Color(0.18, 0.18, 0.22), 0.4, 0.7))
		fin.rotation_degrees = Vector3(0, 0, sx * 16.0)

	# ---- weapon hardpoints (weapons are their own models on sockets) ----
	for p in pdc:                                       # PDC turrets ring the upper deck
		var si: int = clampi((p * sections) / maxi(pdc, 1), 0, sections - 1)
		var side: float = -1.0 if p % 2 == 0 else 1.0
		_pdc(root, Vector3(side * width * 0.24, top_y[si] + 0.03, seg_z[si] + rng.randf_range(-0.1, 0.1)), hull_mat)
	for tp in torpedo:                                  # torpedo launchers forward
		var side2: float = -1.0 if tp % 2 == 0 else 1.0
		var fwd: float = seg_z[sections - 1] - 0.1 - float(tp / 2) * 0.18
		_torpedo(root, Vector3(side2 * width * 0.22, top_y[sections - 1] * 0.2, fwd), accent_mat)
	# Railguns by class (the Expanse-ship mapping, §8b): a Destroyer mounts a single
	# **fixed/spinal** gun jutting far forward (like the MCRN heavy frigate); a Cruiser
	# (Pella) and Battleship (Donnager) mount railgun **turrets** on the dorsal hull.
	if class_idx == 1 and railgun > 0:
		_railgun(root, Vector3(0, -width * 0.02, fz + seg * 1.0), hull_mat, lerpf(1.5, 1.8, t))
	elif railgun > 0:
		for rg in railgun:
			# Space turrets along the dorsal spine (fore for one, fore+aft for two).
			var ti: int = clampi(int(round(lerpf(float(sections) * 0.45, float(sections) * 0.8, float(rg) / maxf(float(railgun - 1), 1.0)))), 1, sections - 1)
			_railgun_turret(root, Vector3(0, top_y[ti] + 0.05, seg_z[ti]), hull_mat, lerpf(0.7, 0.95, t))

	return root
