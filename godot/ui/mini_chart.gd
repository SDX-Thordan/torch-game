extends Control
class_name MiniChart

## A compact multi-series line chart (the §-market PRICE HISTORY panel). Series are
## pushed in over time by the shell; we draw the last N samples as polylines with a
## faint grid. Pure presentation — fed a rolling window of integer prices.

const Ui := preload("res://ui/ui_kit.gd")
const CAP := 96   # samples kept per series

var _series: Array = []          # Array of PackedFloat32Array
var _colors: Array[Color] = []


func setup(colors: Array[Color]) -> void:
	_colors = colors
	_series = []
	for _c in colors:
		_series.append(PackedFloat32Array())


## Append one fresh sample per series (call once per refresh tick).
func push(values: PackedFloat32Array) -> void:
	for i in mini(values.size(), _series.size()):
		var s: PackedFloat32Array = _series[i]
		s.append(values[i])
		if s.size() > CAP:
			s.remove_at(0)
		_series[i] = s
	queue_redraw()


func _draw() -> void:
	var r := Rect2(Vector2.ZERO, size)
	# Faint grid.
	for gy in range(1, 4):
		var y := r.size.y * gy / 4.0
		draw_line(Vector2(0, y), Vector2(r.size.x, y), Ui.LINE * Color(1, 1, 1, 0.5), 1.0)
	# Find a shared vertical range across all series so they're comparable.
	var lo := INF
	var hi := -INF
	for s in _series:
		for v in s:
			lo = minf(lo, v)
			hi = maxf(hi, v)
	if lo == INF or hi <= lo:
		return
	var pad := (hi - lo) * 0.12 + 1.0
	lo -= pad
	hi += pad
	for si in _series.size():
		var s: PackedFloat32Array = _series[si]
		if s.size() < 2:
			continue
		var col: Color = _colors[si] if si < _colors.size() else Ui.ACCENT
		var pts := PackedVector2Array()
		for i in s.size():
			var x := r.size.x * float(i) / float(CAP - 1)
			var y := r.size.y * (1.0 - (s[i] - lo) / (hi - lo))
			pts.append(Vector2(x, y))
		draw_polyline(pts, col, 1.5, true)
		# A dot on the latest sample.
		draw_circle(pts[pts.size() - 1], 2.5, col)
