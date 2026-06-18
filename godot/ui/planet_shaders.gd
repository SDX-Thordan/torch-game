class_name PlanetShaders
extends RefCounted

## Procedural celestial-body shaders for the 3D orrery (§17/§21).
##
## "Proper textures" without a texture pipeline: every body is shaded procedurally
## in the fragment, the same way the ship forge builds hulls from primitives. The
## key trick is that **Sol sits at the world origin**, so each shader derives its
## sunlight direction as `normalize(-NODE_POSITION_WORLD)` — giving a real day/night
## terminator (and Earth's night-side city lights) for free as a body orbits and
## spins, with no engine lights involved (all bodies render `unshaded`).
##
## Pure shell art — no sim/determinism dependency.

# Shared GLSL: value noise + fbm + a soft sun-lit term computed from the world
# origin. Prepended to every body shader.
const _HEADER := """
shader_type spatial;
render_mode unshaded, cull_back;

uniform float seed = 0.0;
varying vec3 v_model;   // model-space surface dir (rotates with the body — texture frame)
varying vec3 v_nw;      // world-space normal (for the Sol terminator)
varying vec3 v_sun;     // world-space direction toward Sol (it sits at the origin)

float hash13(vec3 p) {
	p = fract(p * 0.3183099 + 0.1);
	p *= 17.0;
	return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}
float vnoise(vec3 x) {
	vec3 i = floor(x);
	vec3 f = fract(x);
	f = f * f * (3.0 - 2.0 * f);
	return mix(mix(mix(hash13(i + vec3(0,0,0)), hash13(i + vec3(1,0,0)), f.x),
				   mix(hash13(i + vec3(0,1,0)), hash13(i + vec3(1,1,0)), f.x), f.y),
			   mix(mix(hash13(i + vec3(0,0,1)), hash13(i + vec3(1,0,1)), f.x),
				   mix(hash13(i + vec3(0,1,1)), hash13(i + vec3(1,1,1)), f.x), f.y), f.z);
}
float fbm(vec3 p) {
	float a = 0.5;
	float s = 0.0;
	for (int i = 0; i < 5; i++) { s += a * vnoise(p); p *= 2.03; a *= 0.5; }
	return s;
}
float ridged(vec3 p) {
	float a = 0.5;
	float s = 0.0;
	for (int i = 0; i < 4; i++) { s += a * (1.0 - abs(vnoise(p) * 2.0 - 1.0)); p *= 2.07; a *= 0.5; }
	return s;
}
// Sol-lit factor 0 (night) .. 1 (full day) with a soft terminator.
float sun_day(vec3 nw) {
	return smoothstep(-0.12, 0.20, dot(normalize(nw), v_sun));
}
void vertex() {
	v_model = VERTEX;
	v_nw = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
	// Sol is at the world origin, so the node-to-sun direction is just -(node origin).
	v_sun = normalize(-MODEL_MATRIX[3].xyz);
}
"""


static func _make(code: String) -> ShaderMaterial:
	var sh := Shader.new()
	sh.code = _HEADER + code
	var m := ShaderMaterial.new()
	m.shader = sh
	return m


## The star — a turbulent emissive surface that blooms (ALBEDO > 1 → glow).
static func sun() -> ShaderMaterial:
	return _make("""
void fragment() {
	vec3 p = normalize(v_model);
	float t = TIME * 0.025;
	float cells = fbm(p * 4.0 + vec3(t, 0.0, t * 0.6));
	float gran = ridged(p * 18.0 - vec3(t * 1.7));
	float h = cells * 0.65 + gran * 0.35;
	vec3 deep = vec3(0.95, 0.30, 0.04);
	vec3 mid = vec3(1.0, 0.62, 0.12);
	vec3 hot = vec3(1.0, 0.96, 0.72);
	vec3 col = mix(deep, mid, smoothstep(0.25, 0.55, h));
	col = mix(col, hot, smoothstep(0.55, 0.85, h));
	float rim = pow(clamp(1.0 - dot(NORMAL, VIEW), 0.0, 1.0), 2.0);
	col += vec3(1.0, 0.55, 0.2) * rim * 0.7;
	ALBEDO = col * 2.6;
}
""")


## A banded gas giant. `spot` > 0.5 paints a Great-Red-Spot-style storm.
static func gas_giant(band_a: Color, band_b: Color, band_c: Color, pole: Color, spot: float, spot_col: Color) -> ShaderMaterial:
	var m := _make("""
uniform vec3 band_a;
uniform vec3 band_b;
uniform vec3 band_c;
uniform vec3 pole;
uniform float spot;
uniform vec3 spot_col;
void fragment() {
	vec3 p = normalize(v_model);
	float t = TIME * 0.012;
	// Latitude bands, warped by turbulence so they swirl rather than ring cleanly.
	float turb = (fbm(p * 3.0 + vec3(seed, t, 0.0)) - 0.5) * 0.22;
	float lat = p.y + turb;
	float b = 0.5 + 0.5 * sin(lat * 20.0 + sin(lat * 7.0) * 1.5);
	float fine = fbm(vec3(p.x * 6.0, lat * 26.0, p.z * 6.0) + seed);
	b = clamp(mix(b, fine, 0.4), 0.0, 1.0);
	vec3 col = mix(band_a, band_b, b);
	col = mix(col, band_c, smoothstep(0.3, 0.75, abs(lat)) * 0.7);
	col = mix(col, pole, smoothstep(0.72, 0.98, abs(p.y)));
	// Storm spot.
	if (spot > 0.5) {
		float lon = atan(p.z, p.x);
		float dlon = atan(sin(lon - 2.1), cos(lon - 2.1));
		vec2 d = vec2(dlon, (p.y + 0.22) * 2.4);
		float e = length(d / vec2(0.55, 0.4));
		col = mix(spot_col, col, smoothstep(0.55, 1.0, e + fbm(p * 8.0) * 0.15));
	}
	float day = sun_day(v_nw);
	ALBEDO = col * (0.05 + 0.95 * day);
}
""")
	m.set_shader_parameter("band_a", _v(band_a))
	m.set_shader_parameter("band_b", _v(band_b))
	m.set_shader_parameter("band_c", _v(band_c))
	m.set_shader_parameter("pole", _v(pole))
	m.set_shader_parameter("spot", spot)
	m.set_shader_parameter("spot_col", _v(spot_col))
	return m


## A rocky/icy body: cratered noise, optional polar caps. Mercury, Mars, the Moon,
## the dwarfs, asteroids, and the icy outer moons all share this with tuned params.
static func rocky(base: Color, low: Color, crater_amt: float, ice_amt: float, ice_col: Color) -> ShaderMaterial:
	var m := _make("""
uniform vec3 base_col;
uniform vec3 low_col;
uniform float crater_amt;
uniform float ice_amt;
uniform vec3 ice_col;
void fragment() {
	vec3 p = normalize(v_model);
	float h = fbm(p * 4.5 + seed);
	float regio = fbm(p * 1.7 + seed * 1.3);
	vec3 col = mix(low_col, base_col, smoothstep(0.25, 0.75, h));
	col = mix(col, low_col, smoothstep(0.45, 0.62, regio) * 0.5);
	// Cratering: ridged high-frequency darkening + bright rims.
	float c = ridged(p * 12.0 + seed * 2.0);
	col *= (1.0 - crater_amt * 0.45) + crater_amt * 0.6 * c;
	float fine = fbm(p * 22.0 + seed * 3.0);
	col *= 0.85 + 0.3 * fine;
	// Polar ice.
	float lat = abs(p.y);
	float cap = smoothstep(1.0 - ice_amt, 1.0 - ice_amt * 0.35, lat);
	col = mix(col, ice_col, cap * step(0.001, ice_amt));
	float day = sun_day(v_nw);
	ALBEDO = col * (0.045 + 0.955 * day);
}
""")
	m.set_shader_parameter("base_col", _v(base))
	m.set_shader_parameter("low_col", _v(low))
	m.set_shader_parameter("crater_amt", crater_amt)
	m.set_shader_parameter("ice_amt", ice_amt)
	m.set_shader_parameter("ice_col", _v(ice_col))
	return m


## Earth — oceans, continents, ice caps, drifting clouds, and night-side city lights.
static func earth() -> ShaderMaterial:
	return _make("""
void fragment() {
	vec3 p = normalize(v_model);
	float land = fbm(p * 2.6 + vec3(11.0, 4.0, 7.0));
	float coast = smoothstep(0.46, 0.52, land);
	vec3 deep = vec3(0.02, 0.09, 0.27);
	vec3 sea = vec3(0.05, 0.27, 0.5);
	vec3 ocean = mix(deep, sea, smoothstep(0.28, 0.46, land));
	float arid = fbm(p * 5.5 + vec3(5.0, 1.0, 9.0));
	vec3 veg = vec3(0.10, 0.34, 0.13);
	vec3 desert = vec3(0.52, 0.43, 0.24);
	vec3 land_col = mix(veg, desert, smoothstep(0.42, 0.7, arid));
	land_col = mix(land_col, vec3(0.25, 0.4, 0.18), smoothstep(0.7, 0.5, land) * 0.4);
	vec3 surf = mix(ocean, land_col, coast);
	// Ice caps (and a little high-latitude land snow).
	float lat = abs(p.y);
	float ice = smoothstep(0.80, 0.92, lat) + coast * smoothstep(0.62, 0.92, lat) * 0.6;
	surf = mix(surf, vec3(0.92, 0.95, 0.98), clamp(ice, 0.0, 1.0));
	// Clouds, slowly drifting.
	float cl = fbm(p * 3.4 + vec3(TIME * 0.009, 0.0, TIME * 0.006));
	float clouds = smoothstep(0.55, 0.78, cl);
	float day = sun_day(v_nw);
	vec3 lit = surf * (0.03 + 0.97 * day);
	lit = mix(lit, vec3(1.0) * (0.2 + 0.8 * day), clouds * 0.85);
	// Night-side city lights on land.
	float night = 1.0 - day;
	float pop = smoothstep(0.5, 0.72, fbm(p * 38.0 + vec3(3.0))) * coast * (1.0 - clouds);
	vec3 cities = vec3(1.05, 0.82, 0.45) * pop * night * 1.6;
	ALBEDO = lit + cities;
}
""")


## Venus — a smooth, swirling sulphuric cloud deck (no surface visible).
static func venus() -> ShaderMaterial:
	return _make("""
void fragment() {
	vec3 p = normalize(v_model);
	float t = TIME * 0.01;
	float n = fbm(p * 3.5 + vec3(t, seed, 0.0));
	float band = 0.5 + 0.5 * sin(p.y * 8.0 + n * 3.0);
	vec3 col = mix(vec3(0.78, 0.66, 0.42), vec3(0.96, 0.9, 0.72), n);
	col = mix(col, vec3(0.86, 0.78, 0.55), band * 0.4);
	float day = sun_day(v_nw);
	ALBEDO = col * (0.06 + 0.94 * day);
}
""")


## An additive atmospheric rim glow (a slightly larger shell mesh around a body).
## Brightens on the sun-lit limb so atmospheres catch the light.
static func atmosphere(col: Color, intensity: float) -> ShaderMaterial:
	var sh := Shader.new()
	sh.code = """
shader_type spatial;
render_mode unshaded, blend_add, cull_back, depth_draw_never, shadows_disabled;
uniform vec3 atmo_col;
uniform float intensity = 1.0;
varying vec3 v_nw;
varying vec3 v_sun;
void vertex() {
	v_nw = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
	v_sun = normalize(-MODEL_MATRIX[3].xyz);
}
void fragment() {
	float rim = pow(clamp(1.0 - dot(NORMAL, VIEW), 0.0, 1.0), 2.6);
	float lit = smoothstep(-0.45, 0.4, dot(normalize(v_nw), v_sun));
	ALBEDO = atmo_col * rim * (0.2 + 0.8 * lit) * intensity;
}
"""
	var m := ShaderMaterial.new()
	m.shader = sh
	m.set_shader_parameter("atmo_col", _v(col))
	m.set_shader_parameter("intensity", intensity)
	return m


## Flat planetary ring system (a horizontal annulus mesh) — concentric banding with
## a Cassini-style gap, lit by Sol and faintly translucent.
static func rings(inner: float, outer: float, tint: Color) -> ShaderMaterial:
	var sh := Shader.new()
	sh.code = """
shader_type spatial;
render_mode unshaded, blend_mix, cull_disabled, depth_draw_never, shadows_disabled;
uniform float inner;
uniform float outer;
uniform vec3 tint;
varying vec3 v_model;
varying vec3 v_nw;
varying vec3 v_sun;
float h11(float x) { return fract(sin(x * 78.233) * 43758.5453); }
void vertex() {
	v_model = VERTEX;
	v_nw = (MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz;
	v_sun = normalize(-MODEL_MATRIX[3].xyz);
}
void fragment() {
	float r = length(v_model.xz);
	float t = clamp((r - inner) / (outer - inner), 0.0, 1.0);
	float band = 0.5 + 0.5 * sin(t * 140.0);
	float coarse = 0.6 + 0.4 * sin(t * 26.0 + 1.0);
	float speck = h11(floor(t * 220.0));
	float a = (0.3 + 0.5 * band) * coarse * (0.6 + 0.4 * speck);
	// Soft inner/outer edges + a Cassini division near the middle.
	a *= smoothstep(0.0, 0.05, t) * smoothstep(1.0, 0.93, t);
	a *= 1.0 - 0.85 * exp(-pow((t - 0.52) / 0.03, 2.0));
	vec3 col = mix(vec3(0.66, 0.58, 0.42), vec3(0.95, 0.88, 0.68), band) * tint;
	float lit = 0.45 + 0.55 * clamp(abs(dot(normalize(v_nw), v_sun)), 0.0, 1.0);
	ALBEDO = col * lit;
	ALPHA = clamp(a, 0.0, 1.0) * 0.8;
}
"""
	var m := ShaderMaterial.new()
	m.shader = sh
	m.set_shader_parameter("inner", inner)
	m.set_shader_parameter("outer", outer)
	m.set_shader_parameter("tint", _v(tint))
	return m


static func _v(c: Color) -> Vector3:
	return Vector3(c.r, c.g, c.b)
