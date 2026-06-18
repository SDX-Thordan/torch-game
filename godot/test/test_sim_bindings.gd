# Integration tests for the sim↔view binding contract (§32) — the surface main.gd
# drives. These run headless under GUT and load the real gdext core, so they catch
# binding regressions a Rust unit test can't see (wrong arg mapping, missing #[func]).
extends GutTest

var sim


func before_each() -> void:
	sim = TorchSim.new()
	sim.reset(7)


func test_world_has_markets_commodities_and_bodies() -> void:
	assert_eq(sim.commodity_count(), 12, "the four-tier chain is 12 commodities")
	assert_gt(sim.market_count(), 2, "several trading nodes exist")
	assert_gt(sim.body_count(), 4, "the solar system has many bodies")


func test_time_advances_on_step() -> void:
	var t0: int = sim.tick()
	sim.step()
	assert_eq(sim.tick(), t0 + 1, "one step advances the clock one tick")


func test_commission_grows_the_fleet_and_spends_credits() -> void:
	# Warships need your own shipyard now (Phase B+): a fresh hull is gated until one's up.
	assert_false(sim.commission_ship(1), "a destroyer needs a shipyard you don't have yet")
	sim.dev_grant_shipyard()
	var credits0: int = sim.credits()
	var fleet0: int = sim.fleet_size()
	assert_true(sim.commission_ship(0), "with a yard, a frigate builds")
	assert_eq(sim.fleet_size(), fleet0 + 1, "the hull joined the fleet")
	assert_lt(sim.credits(), credits0, "commissioning spent credits")


func test_engage_reports_no_warships_when_fleet_empty() -> void:
	assert_eq(sim.engage(1), -1, "no warships ⇒ -1 (nothing to send)")


func test_freighters_become_positional_on_a_route() -> void:
	assert_true(sim.commission_freighter(), "a freighter is affordable")
	sim.set_trade_route(5, 1, 0, 20, 1)  # ReactorFuel, market 1 → 0
	var flying := 0
	for i in 2000:
		sim.step()
		if sim.freighter_count() > 0:
			flying = sim.freighter_count()
			break
	assert_gt(flying, 0, "the route dispatches a positional freighter")
	# A flying freighter has a real (non-origin) map position.
	var pos := Vector2(sim.freighter_x(0), sim.freighter_y(0))
	assert_ne(pos, Vector2.ZERO, "the freighter has a live position")


func test_bill_of_materials_describes_assembled_parts() -> void:
	var bom := String(sim.ship_bom_desc(0))  # Frigate
	assert_string_contains(bom, "Machinery", "the frigate BOM lists Machinery")
	assert_false(sim.can_assemble_ship(0), "an empty warehouse can't assemble")


func test_binary_save_round_trips_through_the_binding() -> void:
	# §30: the shipping save is binary (bincode). Round-trip it through the gdext
	# save/load bindings and confirm the run state restores.
	sim.commission_ship(0)
	sim.step()
	sim.step()
	var tick: int = sim.tick()
	var fleet: int = sim.fleet_size()
	var path: String = ProjectSettings.globalize_path("user://gut_test.sav")
	assert_eq(String(sim.save_game(path)), "", "binary save writes cleanly")
	# Advance, then load the save back — state should rewind to the saved point.
	sim.step()
	sim.step()
	assert_eq(String(sim.load_game(path)), "", "binary save loads cleanly")
	assert_eq(sim.tick(), tick, "tick restored from the binary save")
	assert_eq(sim.fleet_size(), fleet, "fleet restored from the binary save")
