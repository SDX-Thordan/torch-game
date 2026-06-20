//! `torch-qa` — run the TORCH economy assessment renderless and print the report.
//!
//! Usage: `cargo run -p torch-qa [--release] -- [TICKS] [SEED_COUNT]`
//! Defaults: 6000 ticks × 8 seeds (0..8). Exits non-zero if any finding is a failure, so it can
//! gate CI.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let ticks: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6_000);
    let seed_count: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    let seeds: Vec<u64> = (0..seed_count).collect();

    let report = torch_qa::assess(&seeds, ticks);
    print!("{}", report.render());

    match report.worst() {
        torch_qa::Severity::Fail => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}
