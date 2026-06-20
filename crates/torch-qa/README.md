# torch-qa — renderless economy QA

A headless harness that **runs the deterministic TORCH core** (`torch-core`) for many ticks across
several seeds and **assesses the health of the autonomous market economy**, flagging problems. It
depends only on the `rlib` side of the core (like `cargo test`), so it never needs a Godot runtime.

```bash
cargo run -p torch-qa --release -- [TICKS] [SEED_COUNT]   # defaults: 6000 ticks × 8 seeds
```

It prints a written report of findings (✓ good · ⚠ warning · ✗ failure) and exits non-zero on any
failure, so it can gate CI. The aspects it assesses:

- **Determinism** — same seed ⇒ identical end state.
- **Solvency** — no player goes bankrupt.
- **Money supply** — credits stay bounded (no hyperinflation / collapse).
- **Market liveness** — every good's price varies and never pins to a floor/ceiling rail.
- **Trade spreads** — arbitrage keeps inter-market spreads moderate (damped).
- **Food security** — settlements (outpost→capital) don't starve.
- **Hauler utilization** — haulers aren't sitting idle for lack of work.
- **Industry throughput** — facilities are kept fed enough to produce.
- **Reservation integrity** — the anti-stampede never oversubscribes a market.
