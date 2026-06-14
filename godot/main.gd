extends Node

## Hello-world (§35.1) + a live demo of the deterministic sim↔view contract
## (§29): drive the Rust core's fixed-tick sim and render the orrery snapshot.
## All game logic is in Rust; this shell only steps the sim and reads positions.

func _ready() -> void:
	var core := TorchCore.new()

	var sim := TorchSim.new()
	sim.reset(42)
	for _i in 240: # advance ~10 days at 1 tick ≈ 1 hour
		sim.step()

	var lines: Array[String] = []
	lines.append(core.greeting())
	lines.append("sim tick=%d  bodies=%d" % [sim.tick(), sim.body_count()])
	for b in sim.body_count():
		var ax := sim.body_x(b) / 1_000_000.0
		var ay := sim.body_y(b) / 1_000_000.0
		lines.append("  %-6s (%+6.3f, %+6.3f) AU" % [sim.body_name(b), ax, ay])

	var text := "\n".join(lines)
	print("[TORCH]\n", text)

	var label := Label.new()
	label.anchors_preset = Control.PRESET_FULL_RECT
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	label.text = text
	add_child(label)
