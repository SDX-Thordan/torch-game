# torch-qa — automated gameplay QA

A headless framework that **plays TORCH automatically** and writes a **gameplay
review**. Because the sim core (`torch-core`) is pure and deterministic (§27),
it can be driven by a program; this crate is that program.

It is the QA counterpart to `cargo test`. Tests assert that systems *work*;
`torch-qa` asserts (and critiques) how the game *plays* — pacing, agency,
economy bounds, alert engagement, reputation tradeoffs, and the cross-cutting
design gaps only a full playthrough surfaces. Same seed ⇒ same review, so a
regression in *feel* shows up as a diff.

## Run it

```bash
cargo run -p torch-qa                 # seed 7, 4000 ticks → review on stdout
cargo run -p torch-qa -- 42 8000      # a different seed / longer run
cargo run -p torch-qa -- 7 4000 > review.md
TORCH_QA_OUT=qa-reports cargo run -p torch-qa   # also write a file
```

See `docs/SAMPLE_GAMEPLAY_REVIEW.md` for example output.

## How it works

```
Strategy (persona)  ──act()──►  Sim  ──step()──►  events + state
        ▲                                                │
        └───────────  Harness samples a Transcript  ◄────┘
                                   │
                            Review engine ──► findings (Markdown)
```

- **`strategy`** — a `Strategy` trait and a roster of autoplayer personas, each a
  play style that presses the same verbs a human would:
  - **Spectator** — touches nothing; measures whether the world is alive and
    watchable on its own (§28).
  - **Arbitrageur** — hand-trades the spread every tick; stress-tests the
    economy's bounds (§5/§7a).
  - **Logistician** — sets one standing trade route and walks away; tests the
    parameterized standing-order heart (§4).
  - **Privateer** — raids the lanes; the only style that climbs the retention
    spine, and pays for it in reputation (§7b/§0).
  - **Tycoon** — the intended full-loop operator: trade, route, raid, research,
    answer shortages (§0–§19).
- **`harness`** — drives a persona through the sim for thousands of ticks,
  tallies the event stream, and samples world state into a `Transcript`.
- **`review`** — correctness/balance heuristics that turn a `Transcript` into
  ranked `Finding`s, plus a cross-cutting `design_review` that compares personas.
- **`engagement`** — the *second lens*: scores six **structural proxies of
  engagement** per play style (Direction, Flow, Agency, Reward rhythm, Stakes,
  Variety) and synthesises a cross-cutting "is it fun?" read — which styles are
  engaging (dominant-strategy check), the weakest dimension to invest in, and
  whether the hands-off world is watchable. It can't measure subjective fun; it
  flags the anti-patterns that reliably kill it (aimlessness, dead air, flat
  stakes, starved rewards, a single dominant approach).

- **`ui`** — the *usability lens*: a **static** audit of the shell's contract
  with the sim (the gdext `#[func]` binding and how `godot/*.gd` wires it). It
  can't see pixels — that's the GUT view tests and the manual render pass — but,
  deterministically and headlessly, it catches the affordance gaps that quietly
  hurt usability: phantom calls that would break at runtime, capability the player
  can't reach, exceptions with no one-press answer, unsurfaced status, missing
  controls legend, and platform fit (a keyboard-scale control surface on an
  Android-first game).
- **`early`** — the *onboarding lens*: drives a **Newcomer** (a reasonable new
  player following the opening-mission beats) through the first 720 ticks and
  audits the opening loop — a first objective + the gate carrot from tick 0,
  time-to-first-reward, whether the opening-mission chain actually completes (and
  *which* one doesn't), the first industrial step's affordability, opening
  pacing/dead-air, and whether the runway stays calm before the world bites. A
  game lives or dies in its first session; the other lenses average over a whole
  run.

## Four questions, four lenses

- **Does it work / is it balanced?** → `review` + `design_review`.
- **Is it engaging to play?** → `engagement` (read the scores as "where is fun at
  *risk*?", not "how fun is it?").
- **Can the player see and reach it all?** → `ui` (a static affordance audit of
  the binding ⟷ shell wiring).
- **Does the *opening* land?** → `early` (an explicit early-game-loop audit).

## Extending it

Add a play style by implementing `Strategy` and dropping it into
`strategy::roster()`. Add a correctness lens with a heuristic in `review.rs`, or
an engagement facet in `engagement.rs`, both reading `Transcript` fields. All
keep the determinism guarantee, so new checks are reproducible in CI.
