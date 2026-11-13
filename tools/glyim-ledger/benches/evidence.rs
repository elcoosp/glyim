//! Evidence-store bench (plan §3.4 curation face).
//!
//! Dependency-free (`harness = false`), mirroring the `status` bench:
//! measures both evidence faces over a synthetic 500-feature project —
//! `record` merging 2000 incoming records into a committed 2000-record
//! store, then `check` validating the resulting 4000-record store — so CI
//! logs can track the curation cost as the store grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench evidence
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

/// The seed policy text (mirrors `ledger/policy.toml`).
const POLICY: &str = r#"schema = "glyim.ledger-policy/1"

[levels.L1]
rules = [{ mode = "compile-pass", outcome = "passed", count = 1 }]

[levels.L2]
rules = [
    { mode = "compile-pass", outcome = "passed", count = 1 },
    { mode = "compile-fail", outcome = "passed", count = 1 },
]

[levels.L3]
rules = [{ mode = "run-pass", outcome = "passed", backend = "gam", count = 1 }]

[levels.L4]
rules = [{ mode = "run-pass", outcome = "passed", backend = "llvm", count = 1 }]

[gap_caps]
enabled = true

[hardening]
fuzz_cpu_hours = 24
mutation_kill_rate_pct = 75
"#;

/// Writes a synthetic project under `root` (same shape as the `status`
/// bench: 500 features over the spec anchor and the seed policy).
fn write_project(root: &Path) {
    let features = root.join("ledger/features");
    std::fs::create_dir_all(&features).expect("mkdir features");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");
    std::fs::write(root.join("ledger/policy.toml"), POLICY).expect("write policy");

    let mut feature = String::new();
    for i in 0..500 {
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
             owner_lane = \"M\"\n\n"
        );
    }
    std::fs::write(features.join("bench.toml"), feature).expect("write features");
}

/// Writes one `glyim.evidence/1` record as a JSONL line.
fn record_line(feature_index: usize, batch: usize, face: usize) -> String {
    let digest = format!("{:0>64}", format!("b{feature_index:04x}{batch}{face}"));
    let modes = ["compile-pass", "compile-fail", "run-pass", "run-pass"];
    let backends = ["gam", "gam", "gam", "llvm"];
    format!(
        "{{\"schema\":\"glyim.evidence/1\",\"name\":\"bench-{feature_index}-{batch}-{face}\",\
         \"fixture\":\"bench/fixture_{feature_index}_{batch}_{face}.g\",\"fixture_sha256\":\"{digest}\",\
         \"revision\":\"\",\"mode\":\"{}\",\"outcome\":\"passed\",\"reason\":null,\
         \"backend\":\"{}\",\"opt\":0,\"features\":[\"LANG-BENCH-{feature_index}\"],\
         \"declared_backends\":[\"{}\"],\"instructions\":null,\"rss_kb\":null}}\n",
        modes[face % modes.len()],
        backends[face % backends.len()],
        backends[face % backends.len()],
    )
}

fn main() {
    let dir = tempfile::tempdir().expect("tmp");
    write_project(dir.path());

    // Seed the committed store: 2000 records (4 faces per feature).
    let store = dir.path().join("ledger/evidence/test.jsonl");
    let mut seed = String::new();
    for i in 0..500 {
        for face in 0..4 {
            seed.push_str(&record_line(i, 0, face));
        }
    }
    std::fs::create_dir_all(store.parent().expect("parent")).expect("mkdir evidence");
    std::fs::write(&store, &seed).expect("write seed store");

    // Incoming run: 2000 fresh records (batch 1, distinct fixtures).
    let incoming = dir.path().join("run.jsonl");
    let mut run = String::new();
    for i in 0..500 {
        for face in 0..4 {
            run.push_str(&record_line(i, 1, face));
        }
    }
    std::fs::write(&incoming, &run).expect("write incoming run");

    let started = Instant::now();
    let outcome = black_box(glyim_ledger::run_evidence_record(
        dir.path(),
        black_box(&[incoming]),
        &store,
        &glyim_ledger::FileEvidenceStore,
    ))
    .expect("record run");
    let record_elapsed = started.elapsed();
    assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
    assert_eq!(outcome.stats.merged, 4000);
    assert_eq!(outcome.stats.duplicates_dropped, 0);

    let started = Instant::now();
    let report = black_box(glyim_ledger::run_evidence_check(black_box(dir.path())));
    let check_elapsed = started.elapsed();
    assert!(report.is_ok(), "{:?}", report.diagnostics);
    assert_eq!(report.records, 4000);

    // Determinism: re-recording the same run is a byte-identical no-op
    // (plan C1/M1.13, measured — the idempotence check IS the bench).
    let committed = std::fs::read_to_string(&store).expect("read store");
    let started = Instant::now();
    let rerun = black_box(glyim_ledger::run_evidence_record(
        dir.path(),
        black_box(&[dir.path().join("run.jsonl")]),
        &store,
        &glyim_ledger::FileEvidenceStore,
    ))
    .expect("re-record run");
    let rerun_elapsed = started.elapsed();
    assert_eq!(rerun.stats.merged, 4000);
    assert_eq!(rerun.stats.duplicates_dropped, 2000);
    assert_eq!(
        std::fs::read_to_string(&store).expect("reread store"),
        committed,
        "idempotent re-record leaves the store byte-identical"
    );

    println!("evidence bench: 500 features, 4000 committed records");
    println!("  record (merge 2000 into 2000): {record_elapsed:?}");
    println!("  check  (validate 4000):        {check_elapsed:?}");
    println!("  re-record (idempotence no-op): {rerun_elapsed:?}");
}
