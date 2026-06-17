# TORCH — Art Track: procedural ships, the designer, stations & civilians (§24/§25)

**Why this doc.** A player call (2026-06-17): *tackle art now — at least the ships and
the ship designer (swap weapons on slots, drives, reactors). Build an engine to
procedurally generate Expanse-like ships (with slots for weapons as their own models).
Also stations, civilian ships, and distinct Martian/Earther ships.*

This is the GDD §24/§25 art track — "the biggest single art lift." It's realized as a
**pure-shell procedural forge** (Godot/GDScript building from primitives + materials),
**no sim/determinism dependency** — the §7c gate and the QA body stay untouched. The
§25 "bake to an optimized mesh" step is a later pass; this is runtime assembly.

## Sequenced rungs (small focused PRs)

### A1 — The procedural ship forge + the BUILD bench 🛠️ ✅ DONE
**Goal:** a recognizable Expanse-style warship from primitives, weapons on hardpoints,
shown solid (not wireframe) in the BUILD designer.
- **Built:** `godot/ui/ship_forge.gd` (`class_name ShipForge`) — a shape-grammar
  assembler: a **modular spine** of stacked box/cylinder hull sections (cylindrical
  drums for non-Earth hulls), **orange accent panels + diagonal hazard stripes** (a
  procedural Image texture), an **aft drive cluster** (engine bells + emissive plume +
  an OmniLight), a **forward bridge**, **radiator fins**, **plating greebles**, and
  **named weapon hardpoints** carrying their own models — PDC turrets (base + barrels),
  torpedo launchers (tube cluster), and spinal **railguns** (breech + long barrel). The
  hull envelope (length/width/section count/drive bells) scales with class
  (Frigate→Battleship); the **faction is a parameter set** (Earth boxy blue-grey, Mars
  lean rust-red, Belt chunky ochre, Indie grey+orange). Deterministic from a seed. The
  forge reads the sim's per-class slot counts via 4 new read-only `TorchShipyard`
  bindings (`pdc_mounts`/`torpedo_mounts`/`railgun_mounts`/`utility_mounts`). The BUILD
  view renders it solid in a lit `SubViewport` (key + fill + ambient) on a slow
  turntable, rebuilding on class change. Render-verified across all four classes.
- *Gameplay-neutral:* pure shell + read-only bindings → §7c gate + QA body unchanged.

### A2 — The interactive ship designer 🔧 ✅ DONE
**Goal:** the fitting bench is *interactive*. Per-slot weapon pickers (swap a PDC ↔
torpedo ↔ railgun where the hull allows), plus drive/reactor choices; re-validate the
fit through the sim's `ships::Loadout` fitting (slots/power/tankage/crew), and
**re-forge** the ship live so the chosen weapons appear on the sockets. The sim already
has the fitting model + `FitError`; expose mutable-loadout bindings.

### A3 — Faction-distinct hull shapes 🛰️ ✅ DONE
**Goal:** deepen the §4 visual signatures from palette-only into *shape* (Earth boxy &
bilateral; Mars elongated, angular, weapon-forward; Belt asymmetric, welded, salvaged).
Different section grammars + greeble styles + weapon placement per faction. Raider hulls
read as scavenged.
- **Built:** a per-faction `SHAPE` profile in `ship_forge.gd` driving the hull grammar
  (len/width/height multipliers, taper, drums, sponsons, lateral asymmetry, deck bevel,
  bolt-ons, prow kind). **Earth (UNN):** wide, boxy, low, gentle taper, symmetric
  **sponson pods** on the flanks, a **blunt squared nose** — institutional/bilateral.
  **Mars (MCRN):** long, narrow, **faceted/beveled decks**, a hard taper to a pointed
  **forward spear-lance** — sleek, weapon-forward. **Belt/OPA:** chunky, mixed
  **drums + boxes welded off-centre** (lateral asymmetry), **salvaged bolt-on tanks**
  strutted to one flank, a **ragged off-centre prow** — scavenged. **Independent:** the
  modular stepped baseline. Render-verified all four at a common fixed scale (broadside
  + top-down). Pure shell → §7c gate + QA body byte-identical.

### A4 — Stations & the civilian fleet 🏭 ✅ DONE (models; orrery wiring in A5)
**Goal:** a **station** kit (modular hab drums + solar/radiator + docking arms, by the
same forge) for holdings/markets on the orrery, and **civilian classes** (freighter,
miner, tanker, Q-ship) — the trade backbone and prime interdiction targets.

### A5 — Forge the orrery fleet 🚀 ✅ DONE
**Goal:** replace the orrery's placeholder markers (cyan spheres / orange dots) with
small forged hulls (LOD'd/instanced) so the live map shows real ships — your fleet,
NPC haulers, raiders — colored by livery/faction.

### A6 — Bake & optimize (later) 📦
**Goal:** the §25 bake step — merge a forged ship's primitives into one optimized mesh
(+ atlas) so a fleet of them is cheap. Premature while hulls are primitive-light;
revisit when ships populate the orrery at scale (A5) or detail grows.

### A7 — The 3D combat diorama 🎆 ✅ DONE
**Goal:** turn the §22 engagement report from a text BattleLog playback into a real
**3D battle**. Built `godot/ui/battle_diorama.gd` (`class_name BattleDiorama`) — a
self-contained Node3D (own camera/lights/env) hosted in a `SubViewport` above the
play-by-play log. On `_open_diorama` it spawns two small forged fleets (player in
livery left, raiders as scavenged Belt hulls right, facing off); each BattleLog beat
drives FX via `on_beat(kind, side)` — railgun volleys throw bright kinetic tracers,
torpedo salvos slower warm streaks, a kill blooms an expanding explosion as the hull
winks out, a retreat peels the side away. Pure shell — no sim dependency beyond the
playback calls main.gd already made; the §7c gate + QA body stay byte-identical.

## Order & risk
A1 (forge) → A2 (designer interactivity) are the player's core ask; A3 deepens the
look; A4/A5 spread the forge to stations/civilians/orrery; A6 is performance, deferred.
All pure-shell (no determinism risk). The realistic ceiling with Godot primitives is
"believable industrial silhouette + livery + snap-on weapons," not SE-voxel detail —
good enough to read, and far past the old wireframe; true voxel meshes would be the
offline-tool path (§25) if ever wanted.
