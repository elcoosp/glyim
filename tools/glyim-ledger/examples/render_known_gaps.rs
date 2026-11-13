//! M1.9 consumer walkthrough: generate `KNOWN_GAPS.md` from the gap
//! registry — the exact bytes `glyim-ledger render-gaps [ROOT]` renders —
//! then verify the committed document with the `--check` drift gate.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-ledger --example render_known_gaps
//! ```
//!
//! The example prints the Markdown face to stdout, then runs the same
//! `--check` byte-comparison CI runs against the committed `KNOWN_GAPS.md`
//! (plan M1.9: outputs deterministic (golden) — the file must be a pure
//! function of `ledger/gaps/known/*.toml`). An optional first argument
//! overrides the project root.
//!
//! Try regenerating the committed document after editing a registry entry:
//!
//! ```text
//! cargo run -p glyim-ledger -- render-gaps --out KNOWN_GAPS.md
//! ```

use std::path::PathBuf;

use glyim_ledger::{JsonGapRenderer, KnownGapsRenderer, MarkdownGapRenderer, run_render_gaps};

fn main() {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    // Face 1: the committed Markdown document (stdout here).
    let markdown = run_render_gaps(&root, glyim_ledger::RenderGapsFormat::Markdown, None, false)
        .expect("the registry at the repository root must load");
    print!("{}", markdown.rendered);

    // Face 2: the glyim.known-gaps/1 JSON contract (the Astro readiness
    // page's input — the next M1.9 consumer of the same seam).
    let json = JsonGapRenderer.render(&markdown.outcome);
    println!();
    println!("-- glyim.known-gaps/1 (first 5 lines) --");
    for line in json.lines().take(5) {
        println!("{line}");
    }

    // The gate: the committed KNOWN_GAPS.md must match the render.
    let committed = run_render_gaps(&root, glyim_ledger::RenderGapsFormat::Markdown, None, true)
        .expect("the committed document must be readable");
    println!();
    println!(
        "render-gaps --check: committed KNOWN_GAPS.md matches the registry ({} gap(s), {} witnessed, {} debt)",
        committed.outcome.gaps.len(),
        committed.outcome.gaps.len() - committed.outcome.unwitnessed.len(),
        committed.outcome.unwitnessed.len(),
    );
    println!(
        "hint: MarkdownGapRenderer rendered {} bytes; the JSON face {} bytes",
        MarkdownGapRenderer.render(&committed.outcome).len(),
        json.len(),
    );
}
