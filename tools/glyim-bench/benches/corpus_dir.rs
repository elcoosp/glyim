//! Committed-corpora bench (plan M2.3 remainder).
//!
//! Dependency-free (`harness = false`), mirroring the `corpus_gen` bench:
//! measures a full [`glyim_bench::corpus_dir::write_corpus_dir`]
//! materialization (all seven families × four seeds at size 64, through
//! the [`glyim_bench::corpus_dir::CorpusSink`] seam into a temp dir) plus
//! the [`glyim_bench::corpus_dir::check_corpus_dir`] re-verification pass,
//! so CI logs can track the commit/verify cost as the corpus grows. Run
//! with:
//!
//! ```text
//! cargo bench -p glyim-bench --bench corpus_dir
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_bench::corpus_dir::{CorpusDirPlan, DirCorpusSink, check_corpus_dir, write_corpus_dir};

fn main() {
    let dir = tempfile::tempdir().expect("tmp");
    let plan = CorpusDirPlan::from_flags(None, Some("0,1,2,3"), 64).expect("plan");

    let started = Instant::now();
    let manifest = black_box(write_corpus_dir(
        black_box(&mut DirCorpusSink::new(dir.path())),
        black_box(&plan),
    ))
    .expect("write pass");
    let write_elapsed = started.elapsed();
    assert_eq!(manifest.programs.len(), 28, "7 families x 4 seeds");

    let started = Instant::now();
    let check = black_box(check_corpus_dir(black_box(dir.path()))).expect("check pass");
    let check_elapsed = started.elapsed();
    assert!(check.is_ok(), "{check:?}");
    assert_eq!(check.entries, 28);

    println!(
        "corpus_dir bench: 28 programs (7 families x 4 seeds, size 64)\n  write_corpus_dir : {write_elapsed:.1?}\n  check_corpus_dir : {check_elapsed:.1?}"
    );
}
