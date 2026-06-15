extends Node2D

## TORCH — the playable shell (§18–§21). A real-time-with-pause game loop over the
## deterministic Rust core: the orrery owns the screen, panels read the snapshot,
## the alert feed carries the voice, and the player presses the verbs (§0.4).
##
## All game logic lives in the Rust `sim`; this scene only drives `step()` on a
## clock, renders the snapshot, and turns input into sim verbs.

const TICKS_PER_SECOND := 6.0           # sim ticks per real second at 1× (§28)
const SPEEDS := [0.0, 1.0, 6.0, 24.0]   # pause / 1× / 6× / 24× (§6)
const ORRERY_CENTRE := Vector2(840, 384)
const ORRERY_RADIUS := 320.0            # px for the outermost body
const AU := 1_000_000.0
const MAX_AU := 2.9                     # Ceres orbit, for scaling

var sim: TorchSim
var speed_idx := 1
var accum := 0.0
var selected := 0                       # index of the selected in-flight hauler
var status := "Welcome, CEO. [Space] pause · [1/2/3] speed · [Tab] select · [I]nterdict · [T]rade · [B]uild"

var _top: Label
var _assets: Label
var _feed: Label
var _help: Label
var _font: Font


func _ready() -> void:
	_font = ThemeDB.fallback_font
	sim = TorchSim.new()
	sim.reset(7)
	_top = _make_label(Vector2(12, 8), 18)
	_assets = _make_label(Vector2(12, 44), 15)
	_feed = _make_label(Vector2(12, 612), 15)
	_help = _make_label(Vector2(12, 700), 13)
	_help.text = status


func _make_label(pos: Vector2, size: int) -> Label:
	var l := Label.new()
	l.position = pos
	l.add_theme_font_size_override("font_size", size)
	add_child(l)
	return l


func _process(delta: float) -> void:
	var mult: float = SPEEDS[speed_idx]
	if mult > 0.0:
		accum += delta * TICKS_PER_SECOND * mult
		while accum >= 1.0:
			sim.step()
			accum -= 1.0
	_refresh()
	queue_redraw()


## Backgrounding pauses the clock (§6/§28).
func _notification(what: int) -> void:
	if what == NOTIFICATION_APPLICATION_PAUSED or what == NOTIFICATION_WM_WINDOW_FOCUS_OUT:
		speed_idx = 0


func _refresh() -> void:
	var speed_label := "paused" if speed_idx == 0 else "%d×" % int(SPEEDS[speed_idx])
	_top.text = "TORCH  ·  T+%d  ·  %s        Tier: %s   Gate %d%%        %d cr   crew %d   fleet %d" % [
		sim.tick(), speed_label, sim.tier_name(), sim.gate_progress_pct(),
		sim.credits(), sim.trained_crew(), sim.fleet_size(),
	]

	var lines: Array[String] = []
	lines.append("NOW: %s (%d/%d)" % [sim.now_goal(), sim.now_goal_progress(), sim.now_goal_target()])
	lines.append("")
	lines.append("%-12s %8s %8s   you" % ["MARKET", sim.market_name(0), sim.market_name(1)])
	for c in sim.commodity_count():
		lines.append("%-12s %8d %8d   %d" % [
			sim.commodity_name(c), sim.price(0, c), sim.price(1, c), sim.cargo(c)
		])
	lines.append("")
	lines.append("haulers in flight: %d   (selected %d)" % [sim.hauler_count(), selected])
	_assets.text = "\n".join(lines)

	var feed_lines: Array[String] = ["── ALERT FEED ──"]
	for a in mini(sim.alert_count(), 3):
		var tag := "[!]" if sim.alert_is_act_now(a) else "   "
		feed_lines.append("%s %s" % [tag, sim.alert_message(a)])
	_feed.text = "\n".join(feed_lines)
	_help.text = status


func _draw() -> void:
	var px_per_unit := ORRERY_RADIUS / (MAX_AU * AU)
	# Orbit rings.
	for b in sim.body_count():
		var r := Vector2(sim.body_x(b), sim.body_y(b)).length() * px_per_unit
		if r > 1.0:
			draw_arc(ORRERY_CENTRE, r, 0, TAU, 96, Color(0.25, 0.3, 0.35), 1.0)
	# Bodies (Sol at the centre).
	for b in sim.body_count():
		var p := ORRERY_CENTRE + Vector2(sim.body_x(b), -sim.body_y(b)) * px_per_unit
		var is_sun := b == 0
		draw_circle(p, 9.0 if is_sun else 5.0, Color(1, 0.8, 0.3) if is_sun else Color(0.6, 0.8, 1.0))
		draw_string(_font, p + Vector2(8, -8), sim.body_name(b), HORIZONTAL_ALIGNMENT_LEFT, -1, 13, Color(0.7, 0.8, 0.9))
	# Haulers — the §7b traffic, the thing you hunt.
	for h in sim.hauler_count():
		var hp := ORRERY_CENTRE + Vector2(sim.hauler_x(h), -sim.hauler_y(h)) * px_per_unit
		var col := Color(1.0, 0.5, 0.2) if h == selected else Color(0.9, 0.7, 0.4)
		draw_circle(hp, 3.0, col)


func _unhandled_input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	match event.keycode:
		KEY_SPACE:
			speed_idx = 0 if speed_idx != 0 else 1
		KEY_1:
			speed_idx = 1
		KEY_2:
			speed_idx = 2
		KEY_3:
			speed_idx = 3
		KEY_TAB:
			if sim.hauler_count() > 0:
				selected = (selected + 1) % sim.hauler_count()
		KEY_I:
			_do_interdict()
		KEY_T:
			_do_trade()
		KEY_B:
			status = "Frigate commissioned." if sim.commission_ship(0) else "Can't build: short on crew or credits."


## The featured verb (§7b): send a frigate from Earth to cut the selected hauler.
func _do_interdict() -> void:
	if sim.hauler_count() == 0:
		status = "No haulers in flight to interdict."
		return
	selected = clampi(selected, 0, sim.hauler_count() - 1)
	var id := sim.hauler_id(selected)
	var outcome := sim.attempt_interdict(id, sim.body_x(1), sim.body_y(1), 120_000, 1500)
	status = ["No firing solution — reposition.", "The hauler ran the gap (escaped).", "Hauler interdicted — a shortage blooms."][outcome]


## A quick arbitrage round trip on ReactorFuel (buy Earth, sell Ceres).
func _do_trade() -> void:
	var cost := sim.buy(1, 5, 20)
	if cost < 0:
		status = "Trade failed — short on credits or stock."
		return
	var revenue := sim.sell(0, 5, 20)
	status = "Arbitrage: spent %d, earned %d (net %+d)." % [cost, revenue, revenue - cost]
