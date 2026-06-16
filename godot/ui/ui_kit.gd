extends RefCounted
class_name UiKit

## Shared visual design language for the TORCH shell (§18–§20). One place for the
## palette + styled-widget factories so every view (orrery, fleet, production,
## market) reads as the same instrument: deep-navy panels, cyan accents, thin
## hairlines, rounded chrome. Pure presentation — no sim logic lives here.

# ---- palette ----------------------------------------------------------------
const BG          := Color(0.027, 0.043, 0.063)          # app background
const BG_PANEL    := Color(0.055, 0.085, 0.110, 0.92)    # standard panel fill
const BG_INSET    := Color(0.030, 0.050, 0.070, 0.94)    # inset / table fill
const BG_BAR      := Color(0.10, 0.16, 0.20, 1.0)         # top + nav chrome
const LINE        := Color(0.13, 0.30, 0.36)             # hairline / panel border
const LINE_HI     := Color(0.27, 0.58, 0.66)             # brighter edge
const ACCENT      := Color(0.30, 0.84, 0.92)             # cyan — the brand accent
const ACCENT_SOFT := Color(0.30, 0.84, 0.92, 0.18)       # accent wash (selection)
const TEXT        := Color(0.80, 0.90, 0.93)
const TEXT_DIM    := Color(0.46, 0.61, 0.67)
const TEXT_HI     := Color(0.93, 0.98, 1.0)
const GOOD        := Color(0.42, 0.83, 0.52)             # profit / positive
const BAD         := Color(0.93, 0.45, 0.32)             # loss / alert
const GOLD        := Color(0.91, 0.78, 0.36)             # credits / ascent


# ---- styleboxes -------------------------------------------------------------

static func panel_box(fill: Color = BG_PANEL, border: Color = LINE, radius: int = 8, bw: int = 1) -> StyleBoxFlat:
	var sb := StyleBoxFlat.new()
	sb.bg_color = fill
	sb.set_border_width_all(bw)
	sb.border_color = border
	sb.set_corner_radius_all(radius)
	sb.set_content_margin_all(10)
	return sb


static func make_panel(fill: Color = BG_PANEL, border: Color = LINE, radius: int = 8) -> PanelContainer:
	var p := PanelContainer.new()
	p.add_theme_stylebox_override("panel", panel_box(fill, border, radius))
	return p


# ---- text -------------------------------------------------------------------

static func label(text: String, size: int = 13, color: Color = TEXT) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	l.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return l


## A small, dim, all-caps section header (the kicker over each panel block).
static func kicker(text: String) -> Label:
	var l := label(text.to_upper(), 10, ACCENT)
	l.add_theme_constant_override("line_spacing", 2)
	return l


## A 1px hairline rule (horizontal).
static func rule(color: Color = LINE) -> Control:
	var c := ColorRect.new()
	c.color = color
	c.custom_minimum_size = Vector2(0, 1)
	c.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	c.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return c


# ---- widgets ----------------------------------------------------------------

## A thin progress/gauge bar (fuel, construction %, pressure …).
static func gauge(ratio: float, color: Color = ACCENT, width: int = 90, height: int = 8) -> ProgressBar:
	var pb := ProgressBar.new()
	pb.custom_minimum_size = Vector2(width, height)
	pb.min_value = 0.0
	pb.max_value = 1.0
	pb.value = clampf(ratio, 0.0, 1.0)
	pb.show_percentage = false
	var bg := StyleBoxFlat.new()
	bg.bg_color = BG_INSET
	bg.set_corner_radius_all(3)
	bg.set_border_width_all(1)
	bg.border_color = LINE
	var fg := StyleBoxFlat.new()
	fg.bg_color = color
	fg.set_corner_radius_all(3)
	pb.add_theme_stylebox_override("background", bg)
	pb.add_theme_stylebox_override("fill", fg)
	return pb


## A labelled toggle switch (standing-order row). Returns the CheckButton so the
## caller can wire `toggled` and read state; the row label sits to its left.
static func toggle_row(text: String, on: bool) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var l := label(text, 12, TEXT)
	l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	l.mouse_filter = Control.MOUSE_FILTER_IGNORE
	row.add_child(l)
	var cb := CheckButton.new()
	cb.button_pressed = on
	cb.focus_mode = Control.FOCUS_NONE
	cb.add_theme_color_override("font_color", TEXT_DIM)
	cb.name = "toggle"
	row.add_child(cb)
	return row


## A left nav-rail button: stacked glyph + caption, highlighted when active.
static func nav_button(glyph: String, caption: String, active: bool) -> Button:
	var b := Button.new()
	b.toggle_mode = true
	b.button_pressed = active
	b.focus_mode = Control.FOCUS_NONE
	b.custom_minimum_size = Vector2(58, 56)
	b.text = "%s\n%s" % [glyph, caption]
	b.add_theme_font_size_override("font_size", 10)
	b.autowrap_mode = TextServer.AUTOWRAP_OFF
	b.add_theme_color_override("font_color", TEXT_DIM)
	b.add_theme_color_override("font_hover_color", TEXT_HI)
	b.add_theme_color_override("font_pressed_color", ACCENT)
	b.add_theme_color_override("font_focus_color", ACCENT)
	var flat := StyleBoxFlat.new()
	flat.bg_color = Color(0, 0, 0, 0)
	flat.set_corner_radius_all(6)
	var on := StyleBoxFlat.new()
	on.bg_color = ACCENT_SOFT
	on.set_corner_radius_all(6)
	on.border_width_left = 2
	on.border_color = ACCENT
	b.add_theme_stylebox_override("normal", flat)
	b.add_theme_stylebox_override("hover", panel_box(Color(1, 1, 1, 0.05), Color(0, 0, 0, 0), 6, 0))
	b.add_theme_stylebox_override("pressed", on)
	return b


## A small pill button used for tabs / actions inside views.
static func tab_button(text: String, active: bool) -> Button:
	var b := Button.new()
	b.text = text
	b.toggle_mode = true
	b.button_pressed = active
	b.focus_mode = Control.FOCUS_NONE
	b.add_theme_font_size_override("font_size", 12)
	b.add_theme_color_override("font_color", TEXT_DIM)
	b.add_theme_color_override("font_pressed_color", ACCENT)
	b.add_theme_color_override("font_hover_color", TEXT_HI)
	var flat := StyleBoxFlat.new()
	flat.bg_color = Color(0, 0, 0, 0)
	flat.content_margin_left = 12
	flat.content_margin_right = 12
	flat.content_margin_top = 4
	flat.content_margin_bottom = 4
	var on := flat.duplicate()
	on.border_width_bottom = 2
	on.border_color = ACCENT
	b.add_theme_stylebox_override("normal", flat)
	b.add_theme_stylebox_override("pressed", on)
	b.add_theme_stylebox_override("hover", flat)
	return b


## A primary action button (e.g. COMMISSION) in brand cyan.
static func action_button(text: String) -> Button:
	var b := Button.new()
	b.text = text
	b.focus_mode = Control.FOCUS_NONE
	b.add_theme_font_size_override("font_size", 13)
	b.add_theme_color_override("font_color", BG)
	b.add_theme_color_override("font_hover_color", BG)
	var sb := StyleBoxFlat.new()
	sb.bg_color = ACCENT
	sb.set_corner_radius_all(5)
	sb.set_content_margin_all(8)
	var hv := sb.duplicate()
	hv.bg_color = TEXT_HI
	b.add_theme_stylebox_override("normal", sb)
	b.add_theme_stylebox_override("pressed", sb)
	b.add_theme_stylebox_override("hover", hv)
	return b
