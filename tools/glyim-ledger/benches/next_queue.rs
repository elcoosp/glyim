//! Work-queue bench (plan §3.9).
//!
//! Dependency-free (`harness = false`), mirroring the `gap_report` bench:
//! measures a full load → validate → rank → JSON-render pass of the
//! `next` queue over a synthetic registry written into a tempdir (64 gaps
//! plus 64 features chained through `requires`), so CI logs can track the
//! queue's cost as the registry grows. Also asserts the §3.9
//! determinism contract on the synthetic registry: two renders of the
//! same state are byte-identical. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench next_queue
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

/// Writes a synthetic registry under `root`: `count` feature entries
/// chained in one `requires` path (so depths span 0..count-1) and `count`
/// gap entries alternating class and witness status, so every rank of
/// the queue is exercised.
fn write_registry(root: &Path, count: usize) {
    let features = root.join("ledger/features/lang");
    let gaps = root.join("ledger/gaps/known");
    std::fs::create_dir_all(&features).expect("mkdir features");
    std::fs::create_dir_all(&gaps).expect("mkdir gaps");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");

    let mut feature = String::new();
    let mut gap = String::new();
    for i in 0..count {
        let requires = if i == 0 {
            String::new()
        } else {
            format!("requires = [\"LANG-BENCH-{}\"]\n", i - 1)
        };
        let _ = writeln!(
            feature,
            "[[feature]]\n\
             id = \"LANG-BENCH-{i}\"\n\
             title = \"bench feature {i}\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             {requires}\n\
             owner_lane = \"M\"\n\n"
        );
        let class = ["b", "a", "c"][i % 3];
        if i % 2 == 0 {
            std::fs::write(
                root.join(format!("witness-{i}.g")),
                format!("//@ known-gap: GAP-{i:04}\nfn main() {{}}\n"),
            )
            .expect("write witness fixture");
            let _ = writeln!(
                gap,
                "[[gap]]\n\
                 id = \"GAP-{i:04}\"\n\
                 class = \"{class}\"\n\
                 title = \"bench gap {i}\"\n\
                 affects = [\"LANG-BENCH-{i}\"]\n\
                 caps = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n\
                 status = \"witnessed\"\n\
                 witness = \"witness-{i}.g\"\n\n"
            );
        } else {
            let _ = writeln!(
                gap,
                "[[gap]]\n\
                 id = \"GAP-{i:04}\"\n\
                 class = \"{class}\"\n\
                 title = \"bench gap {i}\"\n\
                 affects = [\"LANG-BENCH-{i}\"]\n\
                 caps = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n\
                 status = \"unwitnessed\"\n\n"
            );
        }
    }
    std::fs::write(features.join("bench.toml"), feature).expect("write features");
    std::fs::write(gaps.join("bench.toml"), gap).expect("write gaps");
}

fn main() {
    const RUNS: usize = 50;
    const ENTRIES: usize = 64;

    let dir = tempfile::tempdir().expect("tempdir");
    write_registry(dir.path(), ENTRIES);

    // Warmup + sanity: the synthetic registry must validate cleanly, or
    // the bench is measuring a failure path.
    let warm = glyim_ledger::run_next(dir.path());
    assert!(
        warm.is_ok(),
        "synthetic registry must validate: {:?}",
        warm.diagnostics
    );
    assert_eq!(warm.total, ENTRIES * 2);

    let start = Instant::now();
    let mut rendered = 0usize;
    for _ in 0..RUNS {
        let outcome = glyim_ledger::run_next(black_box(dir.path()));
        let json = glyim_ledger::format_next_json(black_box(&outcome));
        rendered += json.len();
    }
    let elapsed = start.elapsed();

    // §3.9 determinism spot-check on the measured registry state.
    let first = glyim_ledger::run_next(dir.path());
    let second = glyim_ledger::run_next(dir.path());
    assert_eq!(
        glyim_ledger::format_next_json(&first),
        glyim_ledger::format_next_json(&second),
        "the next queue must be byte-deterministic for a fixed registry"
    );

    println!(
        "next queue: {ENTRIES} gaps + {ENTRIES} features x {RUNS} runs in {elapsed:?} \
         ({} µs/run, {rendered} bytes rendered)",
        elapsed.as_micros() / RUNS as u128,
    );
}
