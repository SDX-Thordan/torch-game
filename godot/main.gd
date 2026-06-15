extends Node2D

## TORCH — the playable shell (§18–§21). A real-time-with-pause game loop over the
## deterministic Rust core: the orrery owns the screen, panels read the snapshot,
## the alert feed carries the voice, and the player presses the verbs (§0.4).
##
## All game logic lives in the Rust `sim`; this scene only drives `step()` on a
## clock, renders the snapshot, and turns input into sim verbs.

const TICKS_PER_SECOND := 6.0           # sim ticks per real second at 1× (§28)
const SPEEDS := [0.0, 1.0, 6.0, 24.0]   # pause / 1× / 6× / 24× (§6)
const ORRERY_CENTRE := Vector2(910, 360)
const ORRERY_RADIUS := 300.0            # px for the outermost body
const THRESHOLD_NAMES := ["info", "notice", "warning", "critical"]
const BRANCH_NAMES := ["Industrialist", "Trader", "Warlord", "Diplomat"]
const AU := 1_000_000.0
const MAX_AU := 2.9                     # Ceres orbit, for scaling
const QTY_STEP := 5
const QTY_MAX := 500

var sim: TorchSim
var speed_idx := 1
var accum := 0.0
var selected := 0                       # index of the selected in-flight hauler

# The trade cursor — granular control over what/where/how much you deal (§5).
var sel_comm := 5                       # commodity (ReactorFuel by default)
var sel_market := 0                     # market (Ceres)
var trade_qty := 20
var ceo_pick := 2                       # CEO branch under consideration (Warlord)

var status := "Welcome, CEO."

var _top: Label
var _assets: Label
var _deck: Label
var _feed: Label
var _help: Label
var _font: Font


func _ready() -> void:
	_font = ThemeDB.fallback_font
	sim = TorchSim.new()
	sim.reset(7)
	_top = _make_label(Vector2(12, 8), 18)
	_assets = _make_label(Vector2(12, 44), 15)
	_deck = _make_label(Vector2(12, 318), 14)
	_feed = _make_label(Vector2(12, 560), 14)
	_help = _make_label(Vector2(12, 678), 12)


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
	# Market board: a marker on the selected market column + selected commodity row.
	var head := "   %-12s" % "MARKET"
	for m in sim.market_count():
		var nm := sim.market_name(m)
		head += ("[%-8s]" % nm) if m == sel_market else (" %-8s  " % nm)
	head += "   you"
	lines.append(head)
	for c in sim.commodity_count():
		var cursor := ">" if c == sel_comm else " "
		var row := "%s  %-12s" % [cursor, sim.commodity_name(c)]
		for m in sim.market_count():
			row += " %9d " % sim.price(m, c)
		row += "  %d" % sim.cargo(c)
		lines.append(row)
	lines.append("")
	# The live trade preview — what this deal would cost / earn right now.
	var price := sim.price(sel_market, sel_comm)
	lines.append("TRADE  %s @ %s  ×%d   →  buy %d cr / sell %d cr" % [
		sim.commodity_name(sel_comm), sim.market_name(sel_market),
		trade_qty, price * trade_qty, price * trade_qty,
	])
	lines.append("")
	lines.append("haulers in flight: %d   (selected %d)" % [sim.hauler_count(), selected])
	lines.append("» " + status)
	_assets.text = "\n".join(lines)

	# Command deck — the policy a CEO sets and the company she grows (§10/§12).
	var deck: Array[String] = ["── COMMAND DECK ──"]
	var rep := "  "
	for f in sim.faction_count():
		rep += "%s %+d %s   " % [sim.faction_name(f), sim.faction_standing(f), sim.faction_tier(f)]
		if f == 1:
			deck.append(rep)
			rep = "  "
	if rep.strip_edges() != "":
		deck.append(rep)
	var branch := sim.ceo_branch_name()
	var branch_str := branch if branch != "(none)" else "(pick %s: C cycle, X commit)" % BRANCH_NAMES[ceo_pick]
	deck.append("CEO Lv %d %s    research %d techs (+%d%% drive), %d pts" % [
		sim.ceo_level(), branch_str, sim.research_unlocked_count(),
		sim.research_drive_bonus(), sim.research_points()
	])
	deck.append("patrol: %s (%s)    auto-research: %s    alerts ≥ %s" % [
		"ON" if sim.patrol_enabled() else "off", sim.patrol_target_name(),
		"ON" if sim.auto_research_enabled() else "off", THRESHOLD_NAMES[sim.alert_threshold()]
	])
	_deck.text = "\n".join(deck)

	var feed_lines: Array[String] = ["── ALERT FEED ──"]
	for a in mini(sim.alert_count(), 3):
		var tag := "[!]" if sim.alert_is_act_now(a) else "   "
		feed_lines.append("%s %s" % [tag, sim.alert_message(a)])
	_feed.text = "\n".join(feed_lines)

	_help.text = "[Space/1/2/3]time  [↑↓]commodity [←→]market [ [ ] ]qty [B]uy [S]ell  [Tab][I]nterdict  [N]ew ship\n[P]atrol [O]target [R]auto-research [V]invest [A/Z]alerts [C]CEO-pick [X]commit"


func _draw() -> void:
	var px_per_unit := ORRERY_RADIUS / (MAX_AU * AU)
	for b in sim.body_count():
		var r := Vector2(sim.body_x(b), sim.body_y(b)).length() * px_per_unit
		if r > 1.0:
			draw_arc(ORRERY_CENTRE, r, 0, TAU, 96, Color(0.25, 0.3, 0.35), 1.0)
	for b in sim.body_count():
		var p := ORRERY_CENTRE + Vector2(sim.body_x(b), -sim.body_y(b)) * px_per_unit
		var is_sun := b == 0
		draw_circle(p, 9.0 if is_sun else 5.0, Color(1, 0.8, 0.3) if is_sun else Color(0.6, 0.8, 1.0))
		draw_string(_font, p + Vector2(8, -8), sim.body_name(b), HORIZONTAL_ALIGNMENT_LEFT, -1, 13, Color(0.7, 0.8, 0.9))
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
		KEY_UP:
			sel_comm = (sel_comm - 1 + sim.commodity_count()) % sim.commodity_count()
		KEY_DOWN:
			sel_comm = (sel_comm + 1) % sim.commodity_count()
		KEY_LEFT, KEY_RIGHT:
			sel_market = (sel_market + 1) % sim.market_count()
		KEY_BRACKETLEFT:
			trade_qty = maxi(QTY_STEP, trade_qty - QTY_STEP)
		KEY_BRACKETRIGHT:
			trade_qty = mini(QTY_MAX, trade_qty + QTY_STEP)
		KEY_B:
			_do_buy()
		KEY_S:
			_do_sell()
		KEY_TAB:
			if sim.hauler_count() > 0:
				selected = (selected + 1) % sim.hauler_count()
		KEY_I:
			_do_interdict()
		KEY_N:
			status = "Frigate commissioned." if sim.commission_ship(0) else "Can't build: short on crew or credits."
		KEY_P:
			sim.toggle_patrol()
			status = "Interdiction patrol %s." % ("engaged" if sim.patrol_enabled() else "stood down")
		KEY_O:
			sim.cycle_patrol_target()
			status = "Patrol now hunts: %s." % sim.patrol_target_name()
		KEY_R:
			sim.toggle_auto_research()
			status = "Auto-research %s." % ("on" if sim.auto_research_enabled() else "off")
		KEY_V:
			status = "Researched a new tech." if sim.research_next() else "Not enough research points yet."
		KEY_A:
			sim.nudge_alert_threshold(1)
		KEY_Z:
			sim.nudge_alert_threshold(-1)
		KEY_C:
			ceo_pick = (ceo_pick + 1) % BRANCH_NAMES.size()
		KEY_X:
			status = "CEO committed to %s." % BRANCH_NAMES[ceo_pick] if sim.ceo_choose_branch(ceo_pick) else "Branch already chosen."


func _do_buy() -> void:
	var cost := sim.buy(sel_market, sel_comm, trade_qty)
	if cost < 0:
		status = "Buy failed — short on credits, or the market's tapped out."
	else:
		status = "Bought %d %s at %s for %d cr." % [trade_qty, sim.commodity_name(sel_comm), sim.market_name(sel_market), cost]


func _do_sell() -> void:
	var revenue := sim.sell(sel_market, sel_comm, trade_qty)
	if revenue < 0:
		status = "Sell failed — you don't hold that much %s." % sim.commodity_name(sel_comm)
	else:
		status = "Sold %d %s at %s for %d cr." % [trade_qty, sim.commodity_name(sel_comm), sim.market_name(sel_market), revenue]


## The featured verb (§7b): send a frigate from Earth to cut the selected hauler.
func _do_interdict() -> void:
	if sim.hauler_count() == 0:
		status = "No haulers in flight to interdict."
		return
	selected = clampi(selected, 0, sim.hauler_count() - 1)
	var id := sim.hauler_id(selected)
	var outcome := sim.attempt_interdict(id, sim.body_x(1), sim.body_y(1), 120_000, 1500)
	status = ["No firing solution — reposition.", "The hauler ran the gap (escaped).", "Hauler interdicted — a shortage blooms."][outcome]
