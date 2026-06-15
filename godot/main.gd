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

	# §0: the destination always ahead — tier, the now goal, and the ring-gate.
	lines.append("── DESTINATION ──")
	lines.append("  Tier: %s" % sim.tier_name())
	lines.append("  Now:  %s (%d/%d)" % [
		sim.now_goal(), sim.now_goal_progress(), sim.now_goal_target()
	])
	lines.append("  Gate: %d%% — the journey's end" % sim.gate_progress_pct())

	# §5: the player corporation — work the spread, then commission a hull.
	lines.append("── CORPORATION ──")
	var credits0 := sim.credits()
	var cost := sim.buy(1, 5, 20) # buy ReactorFuel cheap at Earth
	var revenue := sim.sell(0, 5, 20) # sell it dear at Ceres
	sim.commission_ship(0) # build a frigate
	lines.append("  arbitrage: spent %d, earned %d (net %+d)" % [cost, revenue, revenue - cost])
	lines.append("  treasury: %d cr (from %d)   crew: %d   fleet: %d" % [
		sim.credits(), credits0, sim.trained_crew(), sim.fleet_size()
	])

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

	# §19: the alert feed — the voiced, ranked exception stream.
	var shown := mini(sim.alert_count(), 4)
	if shown > 0:
		lines.append("alert feed:")
		for a in shown:
			var tag := "[!]" if sim.alert_is_act_now(a) else "   "
			lines.append("  %s %s" % [tag, sim.alert_message(a)])

	# §7b: send a frigate to interdict a hauler from Earth's position.
	if sim.hauler_count() > 0:
		var target_id := sim.hauler_id(0)
		var ex := sim.body_x(1) # Earth
		var ey := sim.body_y(1)
		var outcome := sim.attempt_interdict(target_id, ex, ey, 120_000, 1500)
		var names := ["no solution", "escaped", "interdicted"]
		lines.append("interdiction: hauler %d -> %s" % [target_id, names[outcome]])

	# §10: faction standings — interdicting shipping ripples reputation.
	lines.append("reputation:")
	for fac in sim.faction_count():
		lines.append("  %-13s %+5d (%s)" % [
			sim.faction_name(fac), sim.faction_standing(fac), sim.faction_tier(fac)
		])

	# §10: progression — research / blueprints / CEO skills.
	sim.ceo_gain_xp(3500)
	sim.ceo_choose_branch(2) # Warlord
	sim.research_add_points(400)
	sim.research_tech(0) # Fusion Drives I
	sim.blueprint_discover(0) # generic Belter Frigate
	lines.append("progression:")
	lines.append("  CEO: level %d (%s)" % [sim.ceo_level(), sim.ceo_branch_name()])
	lines.append("  research: %d techs, +%d%% drive" % [
		sim.research_unlocked_count(), sim.research_drive_bonus()
	])
	lines.append("  blueprints known: %d" % sim.blueprint_known_count())

	# §12: run by exception — a separate company set to auto-hunt Earth shipping
	# and auto-invest research, executed by managers with no further input.
	var auto := TorchSim.new()
	auto.reset(1)
	auto.set_interdiction_policy(true, 0, 0) # target Earth
	auto.set_auto_research(true)
	auto.research_add_points(2000)
	for _j in 600:
		auto.step()
	lines.append("automation (managed company):")
	lines.append("  Earth standing %+d, %d techs auto-researched" % [
		auto.faction_standing(0), auto.research_unlocked_count()
	])

	# §8: the warship catalog — railgun count is the escalation axis.
	var yard := TorchShipyard.new()
	lines.append("warships:")
	lines.append("  %-11s %4s %6s %7s %6s" % ["class", "rail", "alpha", "deltaV", "mob"])
	for s in yard.class_count():
		lines.append("  %-11s %4d %6d %7d %6d" % [
			yard.class_name(s), yard.railguns(s), yard.alpha(s),
			yard.delta_v(s), yard.mobility(s)
		])

	# §9: torpedo saturation vs the screen — the band decides.
	lines.append("combat (8 frigates vs battleship):")
	lines.append("  close:  %s" % yard.duel(8, 0))
	lines.append("  long:   %s" % yard.duel(8, 2))

	var text := "\n".join(lines)
	print("[TORCH]\n", text)

	var label := Label.new()
	label.anchors_preset = Control.PRESET_FULL_RECT
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	label.text = text
	add_child(label)
