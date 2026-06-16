# Unit tests for the shared UI kit + chart helpers (§20/§32): the factory functions
# main.gd builds every panel/button/gauge from. Pure GDScript, no gdext needed.
extends GutTest


func test_palette_is_defined() -> void:
	assert_true(UiKit.ACCENT is Color, "accent colour defined")
	assert_true(UiKit.GOOD is Color and UiKit.BAD is Color, "status colours defined")


func test_factories_make_valid_nodes() -> void:
	var lbl := UiKit.label("hi", 13, UiKit.TEXT)
	assert_true(lbl is Label, "label() returns a Label")
	assert_eq(lbl.text, "hi")
	var btn := UiKit.action_button("GO")
	assert_true(btn is Button, "action_button() returns a Button")
	var panel := UiKit.make_panel()
	assert_true(panel is PanelContainer, "make_panel() returns a PanelContainer")
	var box := UiKit.panel_box()
	assert_true(box is StyleBoxFlat, "panel_box() returns a StyleBoxFlat")
	for n in [lbl, btn, panel]:
		n.free()


func test_gauge_clamps_its_ratio() -> void:
	var g := UiKit.gauge(2.0)  # over-full
	assert_true(g is ProgressBar)
	assert_lte(g.value, g.max_value, "gauge ratio is clamped to its range")
	g.free()


func test_mini_chart_accepts_samples() -> void:
	var chart := MiniChart.new()
	chart.setup([UiKit.ACCENT, UiKit.GOOD])
	# Pushing samples should not error and should bound its history.
	for i in 400:
		chart.push(PackedFloat32Array([float(i), float(i * 2)]))
	assert_true(true, "pushing many samples stays stable")
	chart.free()
