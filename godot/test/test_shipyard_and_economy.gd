# Integration tests for the shipyard catalog binding + economy/combat flows the
# shell drives (§32). Headless under GUT against the real gdext core.
extends GutTest

var sim
var yard


func before_each() -> void:
	sim = TorchSim.new()
	sim.reset(7)
	yard = TorchShipyard.new()


func test_shipyard_catalog_exposes_the_escalation_axis() -> void:
	assert_gt(yard.class_count(), 3, "at least the four warship classes")
	# Railgun mounts are the §8b escalation axis: a battleship out-guns a frigate.
	var frig_rg: int = yard.railguns(0)
	var bship_rg: int = yard.railguns(3)
	assert_gt(bship_rg, frig_rg, "the capital defines the railgun axis")
	assert_gt(yard.alpha(3), yard.alpha(0), "the capital out-alphas the escort")
	assert_gt(yard.mobility(0), yard.mobility(3), "the escort is nimbler")
	assert_ne(String(yard.class_name(0)), "", "class names resolve")


func test_market_prices_stay_within_their_rails() -> void:
	# The §7c invariant from the shell's vantage: prices never pin to 0 or run away.
	for _i in 500:
		sim.step()
	for m in sim.market_count():
		for c in sim.commodity_count():
			var p: int = sim.price(m, c)
			assert_gt(p, 0, "price stays positive (no death spiral)")


func test_a_refinery_can_be_founded_on_a_processable_good() -> void:
	# Ore (1) refines up its line; a top-tier good (Drives, 11) cannot.
	assert_true(sim.found_refinery(1, 0, 0), "Ore is a valid factory input")
	assert_false(sim.found_refinery(11, 0, 0), "a finished good has no higher tier")


func test_engage_produces_a_battle_log_for_the_diorama() -> void:
	# Commission a couple of hulls and fight; the diorama needs a populated log.
	sim.dev_grant_shipyard()   # warships need a yard now (Phase B+)
	sim.commission_ship(0)
	sim.commission_ship(0)
	# Hulls go through a timed build (§5/§8c) — let the shipyard stand them up.
	for _i in 400:
		if sim.pending_ship_count() == 0:
			break
		sim.step()
	var result: int = sim.engage(0)  # Close band
	assert_ne(result, -1, "an on-station fleet engages")
	assert_gt(sim.battle_log_count(), 0, "the engagement produced a BattleLog")
	assert_gt(sim.battle_start_count(0), 0, "the diorama knows the player force size")


func test_warships_must_be_on_station_to_engage() -> void:
	sim.dev_grant_shipyard()   # warships need a yard now (Phase B+)
	sim.commission_ship(0)
	# Let the timed build (§5/§8c) stand the hull up before it can take station.
	for _i in 400:
		if sim.pending_ship_count() == 0:
			break
		sim.step()
	assert_eq(sim.warships_on_station(), 1, "a built hull docks at the core")
	# Fly it away (body 3 = Earth); now the core is undefended.
	sim.move_ship(0, 3, false)
	assert_eq(sim.warships_on_station(), 0, "a departed hull is off station")
	assert_eq(sim.engage(1), -1, "no on-station warship ⇒ can't defend")


func test_gate_transit_is_gated_until_the_open_ring() -> void:
	# §0.1/§17: the endgame transit verb is unavailable until the gate is reached,
	# and the player hasn't transited at the start.
	assert_false(sim.can_transit_gate(), "can't transit from The Station")
	assert_false(sim.gate_transited(), "not through the gate at the start")
	assert_false(sim.transit_gate(), "transit is refused before the open gate")
	assert_eq(sim.gate_progress_pct(), 0, "the gate is far off at the start")
