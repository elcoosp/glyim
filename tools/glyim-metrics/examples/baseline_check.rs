//! M0.6 (baseline ratchet) consumer walkthrough: scan a workspace with the
//! same counters CI's `glyim-metrics --check` gate uses, then run the
//! `Ratchet` against a baseline — first inline, then from the committed
//! `metrics/baseline.json`.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-metrics --example baseline_check
//! ```
//!
//! An optional first argument scans a different directory (the library
//! face takes any root; the binary form `glyim-metrics [ROOT]
//! [--check BASELINE]` is the CI gate over this repository).

use std::path::{Path, PathBuf};

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    let metrics = glyim_metrics::scan_tree(&root);
    println!(
        "scan: {} rust file(s), {} line(s), {} documented item(s), {} inline allow(s)",
        metrics.rust_files, metrics.code_lines, metrics.documented_items, metrics.inline_allows,
    );

    // The ratchet is a pure function: current metrics against a baseline,
    // one `Regression` per counter that moved the wrong way.
    let baseline = glyim_metrics::RepoMetrics::from_json(&serde_json::json!({
        "rust_files": metrics.rust_files,
        "code_lines": metrics.code_lines,
        "documented_items": metrics.documented_items,
        "inline_allows": metrics.inline_allows,
        "public_items": metrics.public_items,
        "test_functions": metrics.test_functions,
        "workspace_members": metrics.workspace_members,
    }))
    .expect("inline baseline is a valid RepoMetrics document");
    let regressions = glyim_metrics::Ratchet::check(&metrics, &baseline);
    println!(
        "inline baseline: {} regression(s) (an equal baseline never regresses)",
        regressions.len()
    );

    // The committed baseline is the honest one — the same document CI's
    // metrics-ratchet job checks.
    let committed = Path::new("metrics/baseline.json");
    match std::fs::read_to_string(committed) {
        Ok(text) => {
            let value: serde_json::Value =
                serde_json::from_str(&text).expect("metrics/baseline.json is valid JSON");
            let baseline = glyim_metrics::RepoMetrics::from_json(&value)
                .expect("metrics/baseline.json is a valid RepoMetrics document");
            let regressions = glyim_metrics::Ratchet::check(&metrics, &baseline);
            if regressions.is_empty() {
                println!("committed baseline {}: OK — no counter regressed", committed.display());
            } else {
                for regression in &regressions {
                    println!("{regression:?}");
                }
                std::process::exit(1);
            }
        }
        // Run outside the repository root: skip the committed face.
        Err(_) => println!("no committed baseline at {} — skipped"),
    }
}
