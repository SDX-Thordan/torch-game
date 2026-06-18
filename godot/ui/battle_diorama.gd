class_name BattleDiorama
extends Node3D

## A 3D combat diorama (§22): two small fleets of forged hulls face off and trade
## fire, driven beat-by-beat from the sim's BattleLog playback. Player ships sit on
## the left in their livery, raiders on the right as scavenged Belt hulls; railgun
## volleys throw bright tracers, torpedo salvos throw slower streaks, and a kill
## blooms an explosion as the hull winks out.
##
## Pure shell — no sim dependency beyond the playback calls main.gd already makes.

var _player: Array[Node3D] = []
var _raider: Array[Node3D] = []
var _fx: Array = []            # active effects: {node, ttl, life, kind, base}
var _active := false
var _t := 0.0


func _ready() -> void:
	# Own little world: a dim space environment + key/fill lighting.
	var env := Environment.new()
	env.background_mode = Environment.BG_COLOR
	env.background_color = Color(0.03, 0.04, 0.07)
	env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	env.ambient_light_color = Color(0.40, 0.44, 0.52)
	env.ambient_light_energy = 0.8
	var we := WorldEnvironment.new()
	we.environment = env
	add_child(we)
	var key := DirectionalLight3D.new()
	key.rotation_degrees = Vector3(-46, -30, 0)
	key.light_energy = 1.3
	add_child(key)
	var fill := DirectionalLight3D.new()
	fill.rotation_degrees = Vector3(-12, 130, 0)
	fill.light_energy = 0.4
	add_child(fill)
	var cam := Camera3D.new()
	cam.fov = 46.0
	cam.position = Vector3(0.0, 3.0, 13.0)
	cam.current = true
	add_child(cam)
	cam.look_at(Vector3(0, -0.1, 0), Vector3.UP)


## Spawn the two fleets for a fresh engagement. `player_faction` tints your hulls.
func setup(player_faction: int, n_player: int, n_raider: int) -> void:
	for s in _player:
		s.queue_free()
	for s in _raider:
		s.queue_free()
	for f in _fx:
		(f["node"] as Node).queue_free()
	_player.clear()
	_raider.clear()
	_fx.clear()
	_spawn_fleet(_player, clampi(n_player, 1, 5), player_faction, -1.0, 1100)
	_spawn_fleet(_raider, clampi(n_raider, 1, 5), 2, 1.0, 2200)   # raiders: scavenged Belt
	_active = true


# side: -1 = left (player, prow points +X), +1 = right (raiders, prow points -X).
func _spawn_fleet(into: Array[Node3D], count: int, faction: int, side: float, seed0: int) -> void:
	var rng := RandomNumberGenerator.new()
	rng.seed = seed0
	for i in count:
		# A cruiser flagship leads; the rest are corvettes/destroyers.
		var cls: int = 2 if i == 0 else (1 if i % 2 == 0 else 0)
		var mounts: Array = [[2, 2, 0], [3, 4, 1], [4, 2, 1], [6, 4, 2]][cls]
		# Baked single-mesh hulls — cheap to field a whole fleet (A6/§25).
		var ship: Node3D = ShipForge.build_baked(cls, faction, int(mounts[0]), int(mounts[1]), int(mounts[2]), seed0 + i * 7)
		ship.scale = Vector3.ONE * (0.34 if cls < 2 else 0.4)
		var col: int = i / 2
		var rowy: float = (float(i % 2) - 0.5) * 1.25
		ship.position = Vector3(side * (2.6 + float(col) * 1.05), rowy + rng.randf_range(-0.15, 0.15), rng.randf_range(-1.2, 1.2))
		ship.rotation_degrees = Vector3(0, side * -90.0 + rng.randf_range(-7.0, 7.0), rng.randf_range(-4.0, 4.0))
		add_child(ship)
		into.append(ship)


## Drive a single BattleLog beat. `kind`: 0 salvo, 1 volley, 2 destroyed, 3 retreat,
## 4 overheat. For fire beats `side` is the shooter; for a kill it's the victim.
func on_beat(kind: int, side: int) -> void:
	if not _active:
		return
	match kind:
		1:   # railgun volley — a bright kinetic tracer to an enemy hull
			_fire(side, Color(0.7, 0.85, 1.0) if side == 0 else Color(1.0, 0.55, 0.4), 0.03, 0.16)
		0:   # torpedo salvo — a slower, fatter warm streak
			_fire(side, Color(1.0, 0.7, 0.25), 0.06, 0.34)
		2:   # destroyed — bloom an explosion on the victim side and remove a hull
			_kill(side)
		3:   # retreat — the side peels away
			_retreat(side)
		4:   # overheat — a small vent flash on the shooter
			var fleet0: Array[Node3D] = _player if side == 0 else _raider
			if not fleet0.is_empty():
				_boom(fleet0[0].position + Vector3(0, 0.3, 0), Color(1.0, 0.8, 0.2), 0.18, 0.3)


func _fire(side: int, col: Color, thick: float, life: float) -> void:
	var shooters: Array[Node3D] = _player if side == 0 else _raider
	var targets: Array[Node3D] = _raider if side == 0 else _player
	if shooters.is_empty() or targets.is_empty():
		return
	var a: Vector3 = shooters[randi() % shooters.size()].position
	var b: Vector3 = targets[randi() % targets.size()].position
	a += Vector3(0, 0.1, 0)
	var beam := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = Vector3(thick, thick, a.distance_to(b))
	beam.mesh = bm
	beam.material_override = _glow(col, 6.0)
	beam.look_at_from_position((a + b) * 0.5, b, Vector3.UP)
	add_child(beam)
	_fx.append({"node": beam, "ttl": life, "life": life, "kind": "beam", "base": col})
	_boom(b, col, thick * 4.0, life * 1.4)   # a small impact spark


func _kill(victim_side: int) -> void:
	var fleet: Array[Node3D] = _player if victim_side == 0 else _raider
	if fleet.is_empty():
		return
	var ship: Node3D = fleet.pop_back()
	_boom(ship.position, Color(1.0, 0.6, 0.2), 0.7, 0.6)
	ship.queue_free()


func _retreat(side: int) -> void:
	var fleet: Array[Node3D] = _player if side == 0 else _raider
	for s in fleet:
		_fx.append({"node": s, "ttl": 1.2, "life": 1.2, "kind": "retreat", "base": Vector3(side * 6.0, 0, -4.0)})


func _boom(pos: Vector3, col: Color, r: float, life: float) -> void:
	var mi := MeshInstance3D.new()
	var sm := SphereMesh.new()
	sm.radius = r
	sm.height = r * 2.0
	sm.radial_segments = 10
	sm.rings = 6
	mi.mesh = sm
	mi.position = pos
	mi.material_override = _glow(col, 5.0)
	add_child(mi)
	_fx.append({"node": mi, "ttl": life, "life": life, "kind": "boom", "base": r})


func _glow(col: Color, energy: float) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = col
	m.emission_enabled = true
	m.emission = col
	m.emission_energy_multiplier = energy
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	return m


func _process(delta: float) -> void:
	if not _active:
		return
	_t += delta
	# Gentle idle bob so the fleets feel alive.
	for i in _player.size():
		_player[i].position.y += sin(_t * 1.6 + float(i)) * delta * 0.06
	for i in _raider.size():
		_raider[i].position.y += sin(_t * 1.5 + float(i) * 1.3) * delta * 0.06
	# Advance effects, animating + freeing as their lifetimes run out.
	var keep: Array = []
	for f in _fx:
		f["ttl"] -= delta
		var node: Node3D = f["node"]
		if not is_instance_valid(node):
			continue
		var frac: float = clampf(f["ttl"] / f["life"], 0.0, 1.0)
		match f["kind"]:
			"beam":
				var mat := node.material_override as StandardMaterial3D
				mat.emission_energy_multiplier = 6.0 * frac
				mat.albedo_color.a = frac
			"boom":
				var grow: float = (1.0 - frac) * 1.8 + 0.4
				node.scale = Vector3.ONE * grow
				var bm := node.material_override as StandardMaterial3D
				bm.emission_energy_multiplier = 5.0 * frac
				bm.albedo_color.a = frac
			"retreat":
				node.position += (f["base"] as Vector3) * delta
		if f["ttl"] > 0.0:
			keep.append(f)
		elif f["kind"] != "retreat":
			node.queue_free()
	_fx = keep


func stop() -> void:
	_active = false
