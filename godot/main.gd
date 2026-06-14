extends Node

## Toolchain de-risk hello-world (§35.1): prove the Godot shell can call into
## the Rust deterministic core through the GDExtension binding, and that the
## same seed yields the same result across the boundary (§27 determinism).

func _ready() -> void:
	var core := TorchCore.new()

	var greeting: String = core.greeting()
	var fp_a: int = core.fingerprint(42)
	var fp_b: int = core.fingerprint(42)

	print("[TORCH] ", greeting)
	print("[TORCH] core version: ", core.version())
	print("[TORCH] fingerprint(42): ", fp_a, " (deterministic: ", fp_a == fp_b, ")")

	var label := Label.new()
	label.anchors_preset = Control.PRESET_FULL_RECT
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	label.text = "%s\ncore v%s\nfingerprint(42)=%d\ndeterministic=%s" % [
		greeting, core.version(), fp_a, str(fp_a == fp_b)
	]
	add_child(label)
