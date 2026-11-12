//! Level-computation engine bench (M1.8).
//!
//! Dependency-free (`harness = false`), mirroring the `gap_report` and
//! `next_queue` benches: measures a full load → compute → render pass over
//! a synthetic registry and evidence store written into a tempdir, so CI
//! logs can track the engine's cost as the registry and the evidence
//! corpus grow. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench compute
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::Path;
use std::time::Instant;

use glyim_ledger::compute_levels;
use ledger_schema::{Level, load_project_registry};

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

/// Writes a synthetic project under `root`: `count` features (one gap per
/// even index, capped at L2), the spec anchor, and the policy.
fn write_project(root: &Path, count: usize) {
    let features = root.join("ledger/features");
    let gaps = root.join("ledger/gaps");
    std::fs::create_dir_all(&features).expect("mkdir features");
    std::fs::create_dir_all(&gaps).expect("mkdir gaps");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");
    std::fs::write(root.join("ledger/policy.toml"), POLICY).expect("write policy");

    let mut feature = String::new();
    let mut gap = String::new();
    for i in 0..count {
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
        if i % 2 == 0 {
            let _ = writeln!(
                gap,
                "[[gap]]\n\
                 id = \"GAP-{i:04}\"\n\
                 class = \"b\"\n\
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

/// Builds `per_feature` evidence records spread over the registry's
/// feature IDs (mode/outcome/backend cycle the same way for every
/// feature, so the level distribution is stable across sizes).
fn write_evidence(root: &Path, count: usize, per_feature: usize) {
    let dir = root.join("ledger/evidence");
    std::fs::create_dir_all(&dir).expect("mkdir evidence");
    let modes = ["compile-pass", "compile-fail", "run-pass", "run-pass"];
    let backends = ["gam", "gam", "gam", "llvm"];
    let mut text = String::new();
    for i in 0..count {
        for j in 0..per_feature {
            let digest = format!("{:0>64}", format!("b{i:04x}{j}"));
            let _ = writeln!(
                text,
                "{{\"schema\":\"glyim.evidence/1\",\"name\":\"bench-{i}-{j}\",\
                 \"fixture\":\"bench/fixture_{i}_{j}.g\",\"fixture_sha256\":\"{digest}\",\
                 \"revision\":\"\",\"mode\":\"{}\",\"outcome\":\"passed\",\"reason\":null,\
                 \"backend\":\"{}\",\"opt\":0,\"features\":[\"LANG-BENCH-{i}\"],\
                 \"declared_backends\":[\"{}\"],\"instructions\":null,\"rss_kb\":null}}",
                modes[j % modes.len()],
                backends[j % backends.len()],
                backends[j % backends.len()],
            );
        }
    }
    std::fs::write(dir.join("bench.jsonl"), text).expect("write evidence");
}

fn main() {
    let dir = tempfile::tempdir().expect("tmp");
    write_project(dir.path(), 500);
    write_evidence(dir.path(), 500, 4);

    // Warm pass: prove the synthetic project computes before timing it.
    let loaded = load_project_registry(dir.path());
    assert!(
        loaded.diagnostics.is_empty(),
        "synthetic registry must parse: {:?}",
        loaded.diagnostics
    );
    let (policy, policy_diagnostics) = ledger_schema::load_policy(dir.path());
    assert!(
        policy_diagnostics.is_empty() && policy.is_some(),
        "synthetic policy must parse: {policy_diagnostics:?}"
    );
    let policy = policy.expect("policy");

    let mut evidence = Vec::new();
    let path = dir.path().join("ledger/evidence/bench.jsonl");
    for line in std::fs::read_to_string(&path)
        .expect("read evidence")
        .lines()
    {
        evidence.push(
            glyim_ledger::LedgerEvidence::from_jsonl_line(line).expect("parse evidence line"),
        );
    }

    // Timed section: pure engine pass over 500 features × 2000 records.
    let started = Instant::now();
    let entries = black_box(compute_levels(
        black_box(&loaded.registry),
        black_box(&policy),
        black_box(&evidence),
    ));
    let engine = started.elapsed();

    let l4 = entries
        .iter()
        .filter(|e| e.computed_level == Level::L4)
        .count();
    let capped = entries.iter().filter(|e| !e.capped_by.is_empty()).count();
    assert_eq!(entries.len(), 500);
    assert!(l4 > 0, "bench evidence must reach L4 for some features");
    assert!(capped > 0, "bench gaps must cap some features");

    // Timed section: full run entry point (registry + policy + evidence).
    let started = Instant::now();
    let outcome = black_box(glyim_ledger::run_compute(dir.path(), None));
    let full = started.elapsed();
    assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);

    println!("compute bench: 500 features, {} records", outcome.records);
    println!("  engine pass:  {engine:?}");
    println!("  full run:     {full:?}");
    println!("  L4: {l4}, capped: {capped}");
}
