//! M2.3 consumer walkthrough: commit a corpus directory and re-verify it —
//! the exact bytes `glyim-bench corpus write` commits and the gate
//! `glyim-bench corpus check` runs in CI ("corpora committed with
//! hashes", plan M2.3 remainder).
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-bench --example corpus_commit
//! ```
//!
//! The example writes the seven generator families (two seeds each, size
//! 32) into a temporary directory through the [`CorpusSink`] seam, prints
//! the manifest table, then re-verifies every committed byte — including
//! a deliberate hand edit that the check must catch. An optional first
//! argument overrides the target directory (use a repo-relative
//! `perf/corpus` to see the conventional committed layout).

use std::path::PathBuf;

use glyim_bench::corpus_dir::{CorpusDirPlan, DirCorpusSink, check_corpus_dir, write_corpus_dir};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("glyim-corpus-commit-example"));

    // Commit: all seven families, two seeds, size 32 (26 programs —
    // small enough for a walkthrough, big enough to show the shape).
    let plan = CorpusDirPlan::from_flags(None, Some("0,1"), 32).expect("plan");
    let manifest = write_corpus_dir(&mut DirCorpusSink::new(&dir), &plan)
        .expect("the corpus must materialize");
    println!(
        "committed {} program(s) into {}\n",
        manifest.programs.len(),
        dir.display()
    );
    print!("{}", manifest.render_text());

    // Gate: every committed byte must still hash to its identity.
    let check = check_corpus_dir(&dir).expect("the manifest must load");
    println!(
        "\ncorpus check: ok = {} ({} entries)",
        check.is_ok(),
        check.entries
    );

    // Demonstrate the drift catch: hand-edit one program and re-check.
    let victim = dir.join("wide-0-32.g");
    std::fs::write(&victim, "// hand edit after commit\n").expect("tamper");
    let check = check_corpus_dir(&dir).expect("the manifest still loads");
    println!(
        "after a hand edit: ok = {}, mismatched = {:?}",
        check.is_ok(),
        check.mismatched,
    );
    println!(
        "restore with: glyim-bench corpus write --dir {}",
        dir.display()
    );
}
