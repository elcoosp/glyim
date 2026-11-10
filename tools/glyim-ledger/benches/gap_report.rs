//! Gap-registry report bench (M1.7).
//!
//! Dependency-free (`harness = false`), mirroring the `glyim-metrics`
//! `scan_bench` and the `glyim-test` benches: measures a full
//! load → validate → render pass of the `gaps` report over a synthetic
//! registry written into a tempdir, so CI logs can track the report's
//! cost as the registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench gap_report
//! ```

use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

/// Writes a synthetic registry under `root`: `count` feature entries and
/// `count` gap entries, alternating witnessed (with a fixture carrying
/// the `//@ known-gap:` marker) and unwitnessed, so both report shapes
/// are measured.
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
        feature.push_str(&format!(
            "[[feature]]\n\
             id = \"LANG-BENCH-{i}\"\n\
             title = \"bench feature {i}\"\n\
             area = \"lang.control-flow\"\n\
             tier = \"core\"\n\
             spec = \"docs/specs/v0.1.0.md\"\n\
             target_level = \"L5\"\n\
             backends = [\"gam\", \"llvm\"]\n\
             owner_lane = \"M\"\n\n"
        ));
        if i % 2 == 0 {
            std::fs::write(
                root.join(format!("witness-{i}.g")),
                format!("//@ known-gap: GAP-{i:04}\nfn main() {{}}\n"),
            )
            .expect("write witness fixture");
            gap.push_str(&format!(
                "[[gap]]\n\
                 id = \"GAP-{i:04}\"\n\
                 class = \"c\"\n\
                 title = \"bench gap {i}\"\n\
                 affects = [\"LANG-BENCH-{i}\"]\n\
                 caps = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n\
                 status = \"witnessed\"\n\
                 witness = \"witness-{i}.g\"\n\n"
            ));
        } else {
            gap.push_str(&format!(
                "[[gap]]\n\
                 id = \"GAP-{i:04}\"\n\
                 class = \"c\"\n\
                 title = \"bench gap {i}\"\n\
                 affects = [\"LANG-BENCH-{i}\"]\n\
                 caps = \"L2\"\n\
                 opened_in = \"v0.3.0\"\n\
                 status = \"unwitnessed\"\n\n"
            ));
        }
    }
    std::fs::write(features.join("bench.toml"), feature).expect("write features");
    std::fs::write(gaps.join("bench.toml"), gap).expect("write gaps");
}

fn main() {
    const RUNS: usize = 50;
    const GAPS: usize = 64;

    let dir = tempfile::tempdir().expect("tempdir");
    write_registry(dir.path(), GAPS);

    // Warmup + sanity: the synthetic registry must validate cleanly, or
    // the bench is measuring a failure path.
    let warm = glyim_ledger::run_gaps(dir.path());
    assert!(
        warm.is_ok(),
        "synthetic registry must validate: {:?}",
        warm.diagnostics
    );
    assert_eq!(warm.gaps.len(), GAPS);
    assert_eq!(warm.unwitnessed.len(), GAPS / 2);

    let start = Instant::now();
    let mut rendered = 0usize;
    for _ in 0..RUNS {
        let outcome = glyim_ledger::run_gaps(black_box(dir.path()));
        let text = glyim_ledger::format_gap_report(black_box(&outcome));
        rendered += text.len();
    }
    let elapsed = start.elapsed();
    println!(
        "gaps report: {GAPS} gaps x {RUNS} runs in {elapsed:?} ({} µs/run, {rendered} bytes rendered)",
        elapsed.as_micros() / RUNS as u128,
    );
}
