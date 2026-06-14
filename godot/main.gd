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
	lines.append("sim tick=%d   haulers in flight=%d" % [sim.tick(), sim.hauler_count()])
	lines.append("orrery:")
	for b in sim.body_count():
		var ax := sim.body_x(b) / 1_000_000.0
		var ay := sim.body_y(b) / 1_000_000.0
		lines.append("  %-6s (%+6.3f, %+6.3f) AU" % [sim.body_name(b), ax, ay])

	# Two markets side by side: the §7b price spread that drives the haulers.
	var header := "  %-12s" % "commodity"
	for m in sim.market_count():
		header += " %12s" % sim.market_name(m)
	lines.append("market prices (cr):")
	lines.append(header)
	for c in sim.commodity_count():
		var row := "  %-12s" % sim.commodity_name(c)
		for m in sim.market_count():
			row += " %12d" % sim.price(m, c)
		lines.append(row)

	var text := "\n".join(lines)
	print("[TORCH]\n", text)

	var label := Label.new()
	label.anchors_preset = Control.PRESET_FULL_RECT
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	label.text = text
	add_child(label)
