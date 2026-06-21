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


## A vertical two-stop gradient texture (top → bottom). Shared by the bar fill + its glow.
static func vgrad(top: Color, bottom: Color) -> GradientTexture2D:
	var g := Gradient.new()
	g.set_color(0, top)
	g.set_color(1, bottom)
	var tex := GradientTexture2D.new()
	tex.gradient = g
	tex.fill = GradientTexture2D.FILL_LINEAR
	tex.fill_from = Vector2(0.0, 0.0)
	tex.fill_to = Vector2(0.0, 1.0)
	tex.width = 8
	tex.height = 64
	return tex


## The top-bar fill: a subtle vertical gradient (lighter navy top → darker bottom) rather than a
## flat slab. The crisp bottom hairline + faint glow are drawn by the caller (StyleBoxTexture can't).
static func bar_box(top: Color, bottom: Color) -> StyleBoxTexture:
	var sb := StyleBoxTexture.new()
	sb.texture = vgrad(top, bottom)
	sb.set_content_margin_all(0)
	sb.content_margin_left = 12
	sb.content_margin_right = 10
	sb.content_margin_top = 2
	sb.content_margin_bottom = 2
	return sb


## A hexagonal badge — placeholder frame for the player's logo (flat-top hexagon, fill + outline).
static func hex_badge(size: float, fill: Color = BG_INSET, border: Color = ACCENT) -> Control:
	var holder := Control.new()
	holder.custom_minimum_size = Vector2(size, size)
	holder.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var c := size * 0.5
	var r := size * 0.46
	var pts := PackedVector2Array()
	for i in 6:
		var a := float(i) * PI / 3.0          # flat-top hexagon (vertices at 0°,60°,…)
		pts.append(Vector2(c + r * cos(a), c + r * sin(a)))
	var poly := Polygon2D.new()
	poly.polygon = pts
	poly.color = fill
	holder.add_child(poly)
	var outline := Line2D.new()
	var lpts := pts.duplicate()
	lpts.append(pts[0])
	outline.points = lpts
	outline.width = maxf(1.0, size * 0.07)
	outline.default_color = border
	outline.joint_mode = Line2D.LINE_JOINT_ROUND
	holder.add_child(outline)
	return holder


## A speed-control toggle button: transparent until active, then an accent-washed pill (so the
## current speed reads at a glance). Caller sets `button_pressed` to mark the active one.
static func speed_button(text: String) -> Button:
	var b := Button.new()
	b.text = text
	b.toggle_mode = true
	b.focus_mode = Control.FOCUS_NONE
	b.add_theme_font_size_override("font_size", 12)
	b.add_theme_color_override("font_color", TEXT_DIM)
	b.add_theme_color_override("font_pressed_color", ACCENT)
	b.add_theme_color_override("font_hover_color", TEXT_HI)
	var flat := StyleBoxFlat.new()
	flat.bg_color = Color(0, 0, 0, 0)
	flat.set_corner_radius_all(4)
	var on := StyleBoxFlat.new()
	on.bg_color = ACCENT_SOFT
	on.set_corner_radius_all(4)
	on.set_border_width_all(1)
	on.border_color = ACCENT
	b.add_theme_stylebox_override("normal", flat)
	b.add_theme_stylebox_override("hover", flat)
	b.add_theme_stylebox_override("pressed", on)
	b.add_theme_stylebox_override("hover_pressed", on)
	return b


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
	if width == 0:
		pb.size_flags_horizontal = Control.SIZE_EXPAND_FILL
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


## A view-level header block: large bold title + dim subtitle. Add at the top of each view.
static func view_header(title: String, sub: String = "") -> VBoxContainer:
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 2)
	v.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var tl := label(title, 15, TEXT_HI)
	tl.add_theme_constant_override("line_spacing", 0)
	v.add_child(tl)
	if sub != "":
		v.add_child(label(sub, 11, TEXT_DIM))
	return v


## A stat card for summary strips: caption (kicker) + large value + optional delta.
## Call stat_strip() for the container, then add these as children.
static func stat_card(caption: String, value_str: String,
		delta: String = "", delta_col: Color = GOOD) -> PanelContainer:
	var p := make_panel(BG_INSET, LINE, 6)
	p.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 1)
	v.alignment = BoxContainer.ALIGNMENT_CENTER
	v.mouse_filter = Control.MOUSE_FILTER_IGNORE
	p.add_child(v)
	v.add_child(kicker(caption))
	var val := label(value_str, 16, TEXT_HI)
	val.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	v.add_child(val)
	if delta != "":
		var dl := label(delta, 10, delta_col)
		dl.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
		v.add_child(dl)
	return p


## A uniform horizontal strip container for stat_card()s.
static func stat_strip() -> HBoxContainer:
	var h := HBoxContainer.new()
	h.add_theme_constant_override("separation", 6)
	h.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return h


## A label+value info row (used in summaries, economy breakdowns, etc.).
## lbl_text left-aligned & expands; val_text right-pinned.
static func info_row(lbl_text: String, val_text: String,
		lbl_col: Color = TEXT_DIM, val_col: Color = TEXT_HI,
		size: int = 12) -> HBoxContainer:
	var h := HBoxContainer.new()
	h.add_theme_constant_override("separation", 6)
	h.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var ll := label(lbl_text, size, lbl_col)
	ll.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	h.add_child(ll)
	h.add_child(label(val_text, size, val_col))
	return h


## A thin left-accent panel for named sub-sections (the "card with coloured edge" pattern).
static func section_card(accent_col: Color = ACCENT) -> PanelContainer:
	var p := PanelContainer.new()
	var sb := panel_box(BG_INSET, LINE, 6)
	sb.border_width_left = 2
	sb.border_color = accent_col
	p.add_theme_stylebox_override("panel", sb)
	return p


## A colored status dot (●) used in fleet/relation rows.
static func dot(color: Color) -> Label:
	var l := label("●", 11, color)
	l.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return l
