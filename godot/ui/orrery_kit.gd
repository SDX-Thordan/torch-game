extends RefCounted
class_name OrreryKit

## Stateless 3D factory helpers for the orrery (§17/§21): emissive markers, spheres,
## orbit rings and the station glyph. Pure geometry/material construction — no sim
## logic and no host state, so the shell can build the scene from these like it builds
## 2D chrome from UiKit. The host owns placement; these just mint the nodes.

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
