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


# ---- the forge -------------------------------------------------------------

## Build a ship Node3D. class_idx 0..3 = Frigate..Battleship; faction 0..3; the
## mount counts come from the sim's hull (TorchShipyard); seed makes it deterministic.
static func build(class_idx: int, faction: int, pdc: int, torpedo: int, railgun: int, seed: int) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed
	var root := Node3D.new()

	var pal: Dictionary = PALETTE.get(faction, PALETTE[3])
	var hull_col: Color = pal["hull"]
	var accent_col: Color = pal["accent"]
	var hull_mat := _mat(hull_col)
	var accent_mat := _mat(accent_col, 0.55, 0.4)
	var trim_mat := _mat(TRIM, 0.8, 0.5)
	var hazard := _hazard()
	var glow := _emissive(Color(0.45, 0.72, 1.0), 4.0)

	# Class envelope: bigger ships are longer, wider, with more modular sections.
	var t: float = float(class_idx) / 3.0
	var total_len: float = lerpf(2.6, 5.4, t)
	var width: float = lerpf(0.42, 0.92, t)
	var sections: int = 3 + class_idx                 # 3 (frigate) .. 6 (battleship)
	# Mars runs longer/leaner; the Belt is chunkier; Earth is boxy.
	if faction == 1:
		total_len *= 1.12
		width *= 0.9
	elif faction == 2:
		width *= 1.12

	# Keel spine (a low central beam tying the modules together).
	_box(root, Vector3(0, -width * 0.12, 0), Vector3(width * 0.5, width * 0.28, total_len * 0.96), trim_mat)

	# Stacked hull modules from aft (-Z) to fore (+Z).
	var seg: float = total_len / float(sections)
	var z: float = -total_len * 0.5 + seg * 0.5
	var top_y: Array[float] = []      # remember each module's deck height for hardpoints
	var seg_z: Array[float] = []
	for i in sections:
		var frac: float = float(i) / float(sections - 1)   # 0 aft .. 1 fore
		# The hull tapers toward the bow; the bow module is a wedge nose.
		var w: float = width * lerpf(1.0, 0.62, frac) * rng.randf_range(0.9, 1.06)
		var h: float = width * lerpf(0.95, 0.6, frac) * rng.randf_range(0.92, 1.05)
		var is_drum: bool = (faction != 0) and (i % 2 == 1)   # cylindrical drums (not Earth)
		if is_drum:
			_cyl(root, Vector3(0, 0, z), maxf(w, h) * 0.5, seg * 0.92, hull_mat, "z")
		else:
			_box(root, Vector3(0, 0, z), Vector3(w, h, seg * 0.92), hull_mat)
		# Orange accent side-panels + a hazard band on every other module.
		for sx in [-1.0, 1.0]:
			_box(root, Vector3(sx * w * 0.5, 0, z), Vector3(0.02, h * 0.6, seg * 0.6), accent_mat)
		if i % 2 == 0:
			_box(root, Vector3(0, h * 0.5 + 0.005, z), Vector3(w * 0.7, 0.012, seg * 0.4), hazard)
		# Plating greebles for industrial texture.
		for _g in range(2 + class_idx):
			var gx: float = rng.randf_range(-w * 0.45, w * 0.45)
			var gy: float = h * 0.5 * (1.0 if rng.randf() > 0.5 else -1.0)
			var gz: float = z + rng.randf_range(-seg * 0.35, seg * 0.35)
			var gs: float = rng.randf_range(0.04, 0.1)
			_box(root, Vector3(gx, gy, gz), Vector3(gs, 0.02, gs * 1.4), trim_mat)
		top_y.append(h * 0.5)
		seg_z.append(z)
		z += seg

	# Forward bridge / superstructure (a stepped block atop the fore module).
	var fz: float = seg_z[sections - 1]
	var fy: float = top_y[sections - 1]
	_box(root, Vector3(0, fy + 0.07, fz - 0.05), Vector3(width * 0.42, 0.14, seg * 0.5), hull_mat)
	_box(root, Vector3(0, fy + 0.16, fz - 0.02), Vector3(width * 0.26, 0.06, seg * 0.28), _emissive(accent_col, 0.8))

	# Aft drive cluster: a thrust frame + engine bells with a glowing plume.
	var az: float = -total_len * 0.5 - 0.04
	var bells: int = 1 + class_idx / 2 + class_idx % 2     # 1..3
	_box(root, Vector3(0, 0, az + 0.12), Vector3(width * 0.9, width * 0.7, 0.16), trim_mat)
	var spread: float = width * 0.28
	for bi in bells:
		var bx: float = 0.0 if bells == 1 else lerpf(-spread, spread, float(bi) / float(bells - 1))
		_cyl(root, Vector3(bx, 0, az - 0.02), width * 0.16, 0.22, trim_mat, "z")     # bell housing
		var cone := MeshInstance3D.new()
		var cm := CylinderMesh.new()
		cm.top_radius = width * 0.04
		cm.bottom_radius = width * 0.15
		cm.height = 0.18
		cm.radial_segments = 12
		cone.mesh = cm
		cone.rotation_degrees = Vector3(-90, 0, 0)
		cone.position = Vector3(bx, 0, az - 0.16)
		cone.material_override = glow
		root.add_child(cone)
	var plume := OmniLight3D.new()
	plume.position = Vector3(0, 0, az - 0.2)
	plume.light_color = Color(0.5, 0.75, 1.0)
	plume.light_energy = 2.2
	plume.omni_range = total_len
	root.add_child(plume)

	# Radiator fins (thin angled panels mid-hull) — the §22 heat signature.
	for sx in [-1.0, 1.0]:
		var fin := _box(root, Vector3(sx * width * 0.6, 0.0, -total_len * 0.12), Vector3(0.02, width * 0.5, total_len * 0.28), _mat(Color(0.2, 0.2, 0.23), 0.4, 0.6))
		fin.rotation_degrees = Vector3(0, 0, sx * 18.0)

	# ---- weapon hardpoints (weapons are their own models on sockets) ----
	# PDC turrets ring the upper deck along the hull.
	for p in pdc:
		var si: int = clampi((p * sections) / maxi(pdc, 1), 0, sections - 1)
		var side: float = -1.0 if p % 2 == 0 else 1.0
		var pos := Vector3(side * width * 0.22, top_y[si] + 0.03, seg_z[si] + rng.randf_range(-0.1, 0.1))
		_pdc(root, pos, hull_mat)
	# Torpedo launchers cluster forward (the alpha/equalizer, §8a).
	for tp in torpedo:
		var side2: float = -1.0 if tp % 2 == 0 else 1.0
		var fwd: float = seg_z[sections - 1] - 0.1 - float(tp / 2) * 0.18
		_torpedo(root, Vector3(side2 * width * 0.2, top_y[sections - 1] * 0.2, fwd), accent_mat)
	# Railguns mount spinal at the nose — the capital-defining weapon (§8b).
	for rg in railgun:
		var off: float = (float(rg) - float(railgun - 1) * 0.5) * 0.12
		_railgun(root, Vector3(off, fy + 0.02, fz + seg * 0.4), hull_mat, lerpf(0.6, 1.1, t))

	return root
