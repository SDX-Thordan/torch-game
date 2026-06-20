extends RefCounted
class_name OrreryKit

## Stateless 3D factory helpers for the orrery (§17/§21): emissive markers, spheres,
## orbit rings and the station glyph. Pure geometry/material construction — no sim
## logic and no host state, so the shell can build the scene from these like it builds
## 2D chrome from UiKit. The host owns placement; these just mint the nodes.

# Icy outer-system moons share a bright frozen-rock shader (§17).
const ICY := ["Europa", "Enceladus", "Tethys", "Dione", "Mimas", "Rhea", "Iapetus",
	"Ganymede", "Callisto", "Triton", "Charon", "Miranda", "Ariel", "Umbriel",
	"Titania", "Oberon", "Hydra", "Nix"]

static func emissive_mat(col: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = col
	m.emission_enabled = true
	m.emission = col
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	return m


static func sphere(radius: float, mat: Material) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	var sm := SphereMesh.new()
	sm.radius = radius
	sm.height = radius * 2.0
	mi.mesh = sm
	mi.material_override = mat
	return mi


static func ring_mat(radius: float, mat: StandardMaterial3D, tube: float) -> MeshInstance3D:
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


static func ring(radius: float, col: Color) -> MeshInstance3D:
	# A thin orbit line with a faint glow — the emission tips just into the bloom pass.
	return ring_mat(radius, emissive_mat(col * 2.4), 0.005)


static func station_glyph(fcol: Color) -> Node3D:
	var root := Node3D.new()
	var mat := emissive_mat(fcol)
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


static func make_body_material(name: String, kind: int) -> ShaderMaterial:
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
	if ICY.has(name):
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


static func axial_tilt(name: String) -> float:
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


static func spin_rate(name: String, kind: int) -> float:
	match kind:
		0: return 0.0     # the sun's surface is animated in-shader
		2: return 0.5     # gas giants whirl
		1: return 0.13
		3: return 0.10
		4: return 0.08
		7: return 0.35    # rubble-pile asteroids tumble
		6: return 0.10
	return 0.10


static func atmosphere_for(name: String, kind: int, rad: float) -> MeshInstance3D:
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
	var shell := sphere(rad * (1.05 if kind == 2 else 1.07), PlanetShaders.atmosphere(col, inten))
	var sm := shell.mesh as SphereMesh
	sm.radial_segments = 28
	sm.rings = 14
	return shell
