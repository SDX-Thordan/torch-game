extends Control
class_name FlowGraph

## The §-market commodity-flow schematic: trading nodes (markets) laid out across
## the panel with arrows for the lanes haulers are actually flying, each tagged
## with the commodity moving. A schematic read of "what's trading where" that
## complements the literal orrery. Fed market names + live flows each frame.

const Ui := preload("res://ui/ui_kit.gd")

var _nodes: PackedStringArray = []      # market names, left→right
var _flows: Array = []                  # Array of {from:int, to:int, label:String}
var _node_pos: Array[Vector2] = []


func set_markets(names: PackedStringArray) -> void:
	_nodes = names
	queue_redraw()


## flows: Array of dictionaries {from, to, label}.
func set_flows(flows: Array) -> void:
	_flows = flows
	queue_redraw()


func _layout() -> void:
	_node_pos.clear()
	var n := _nodes.size()
	if n == 0:
		return
	var midy := size.y * 0.5
	var margin := 70.0
	for i in n:
		var t := float(i) / float(maxi(1, n - 1))
		var x := lerpf(margin, size.x - margin, t)
		# Gentle vertical stagger so arrows between nodes read clearly.
		var y := midy + (sin(t * PI) * -22.0 if n > 2 else 0.0)
		_node_pos.append(Vector2(x, y))


func _draw() -> void:
	_layout()
	if _node_pos.is_empty():
		return
	# Flows first (under the nodes).
	for f in _flows:
		var a: int = f.get("from", -1)
		var b: int = f.get("to", -1)
		if a < 0 or b < 0 or a >= _node_pos.size() or b >= _node_pos.size():
			continue
		_draw_flow(_node_pos[a], _node_pos[b], String(f.get("label", "")))
	# Market nodes.
	for i in _node_pos.size():
		var p := _node_pos[i]
		draw_circle(p, 16.0, Ui.BG_INSET)
		draw_arc(p, 16.0, 0, TAU, 32, Ui.ACCENT, 1.5, true)
		draw_circle(p, 5.0, Ui.ACCENT)
		var nm := _nodes[i]
		var fnt := ThemeDB.fallback_font
		var w := fnt.get_string_size(nm, HORIZONTAL_ALIGNMENT_LEFT, -1, 12).x
		draw_string(fnt, p + Vector2(-w * 0.5, 30), nm, HORIZONTAL_ALIGNMENT_LEFT, -1, 12, Ui.TEXT)


func _draw_flow(a: Vector2, b: Vector2, label: String) -> void:
	var dir := (b - a).normalized()
	var start := a + dir * 18.0
	var end := b - dir * 20.0
	# A gentle bowed line so opposing flows don't overlap.
	var normal := Vector2(-dir.y, dir.x)
	var mid := (start + end) * 0.5 + normal * 14.0
	var pts := PackedVector2Array()
	for i in 13:
		var t := float(i) / 12.0
		pts.append(start.lerp(mid, t).lerp(mid.lerp(end, t), t))
	draw_polyline(pts, Ui.GOLD * Color(1, 1, 1, 0.7), 1.5, true)
	# Arrowhead.
	var tip := end
	var back := dir.rotated(2.7) * 9.0
	var back2 := dir.rotated(-2.7) * 9.0
	draw_colored_polygon(PackedVector2Array([tip, tip + back, tip + back2]), Ui.GOLD)
	if label != "":
		var fnt := ThemeDB.fallback_font
		draw_string(fnt, mid + Vector2(-12, -4), label, HORIZONTAL_ALIGNMENT_LEFT, -1, 10, Ui.GOLD)
