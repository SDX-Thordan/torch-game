//! `torch-qa` — play TORCH automatically and print a gameplay review.
//!
//! Usage:
//! ```text
//! cargo run -p torch-qa                 # seed 7, 4000 ticks
//! cargo run -p torch-qa -- <seed> <ticks>
//! cargo run -p torch-qa -- 7 4000 > review.md
//! ```
//! Set `TORCH_QA_OUT=<dir>` to also write the report to a file in that dir.

use std::io::Write as _;

fn main() {
    let mut args = std::env::args().skip(1);
    let seed: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(7);
    let ticks: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(4_000);

    let report = torch_qa::render_report(seed, ticks);
    print!("{report}");

    if let Ok(dir) = std::env::var("TORCH_QA_OUT") {
        let path = std::path::Path::new(&dir).join(format!("gameplay-review-seed{seed}.md"));
        match std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &report)) {
            Ok(()) => {
                let _ = writeln!(std::io::stderr(), "wrote {}", path.display());
            }
            Err(e) => {
                let _ = writeln!(std::io::stderr(), "could not write {}: {e}", path.display());
            }
        }
    }
}
