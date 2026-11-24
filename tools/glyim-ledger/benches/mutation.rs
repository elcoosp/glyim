//! Policy-mutation bench (plan M1.13 mutation slice).
//!
//! Dependency-free (`harness = false`), mirroring the `verify` bench:
//! measures the catalogue build plus the full mutant-evaluation run —
//! baseline compute + one compute per mutant — over the synthetic
//! 500-feature project, so CI logs can track the mutation-gate cost
//! against the compute pass it multiplies. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench mutation
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

/// The synthetic policy (the same shape the `verify` bench writes).
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

/// Writes a synthetic project under `root`: 500 features with run-pass
/// gam+llvm evidence for even indices and compile-pass evidence for all,
/// so every level rule is load-bearing and every catalogue mutant kills.
fn write_project(root: &std::path::Path, count: usize) {
    let ledger = root.join("ledger");
    let features = ledger.join("features");
    let evidence = ledger.join("evidence");
    std::fs::create_dir_all(&features).expect("mkdir features");
    std::fs::create_dir_all(&evidence).expect("mkdir evidence");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");
    std::fs::write(ledger.join("policy.toml"), POLICY).expect("write policy");

    let mut feature = String::new();
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
    }
    std::fs::write(features.join("bench.toml"), feature).expect("write features");

    let mut records = String::new();
    for i in 0..count {
        // Compile-fail (negative) evidence only for even indices, so the
        // L2 rule set is genuinely load-bearing: odd features stall at L1
        // until the compile-fail rule is satisfied, which is exactly the
        // surface the `drop-l2-negative-rule` mutant removes.
        let variants: &[(usize, &str, &str, &str)] = if i % 2 == 0 {
            &[
                (0, "compile-pass", "gam", "passed"),
                (1, "compile-fail", "gam", "passed"),
                (2, "run-pass", "gam", "passed"),
                (3, "run-pass", "llvm", "passed"),
            ]
        } else {
            &[
                (0, "compile-pass", "gam", "passed"),
                (2, "run-pass", "gam", "passed"),
                (3, "run-pass", "llvm", "passed"),
            ]
        };
        for (variant, mode, backend, outcome) in variants {
            // A deterministic 64-hex digest, unique per (fixture, mode,
            // backend), keeps the distinct-fixture counting set-based and
            // the records valid.
            let digest = format!("{:064x}", i * 4 + variant);
            let _ = writeln!(
                records,
                "{{\"schema\":\"glyim.evidence/1\",\"name\":\"bench-{i}-{mode}-{backend}\",\"fixture\":\"crates/glyim-test/tests/run-pass/bench-{i}.g\",\"fixture_sha256\":\"{digest}\",\"revision\":\"\",\"mode\":\"{mode}\",\"outcome\":\"{outcome}\",\"backend\":\"{backend}\",\"opt\":0,\"features\":[\"LANG-BENCH-{i}\"],\"declared_backends\":[\"gam\",\"llvm\"]}}",
            );
        }
    }
    std::fs::write(evidence.join("bench.jsonl"), records).expect("write evidence");
}

fn main() {
    let dir = tempfile::tempdir().expect("tmp");
    write_project(dir.path(), 500);
    let root: PathBuf = dir.path().to_path_buf();

    let started = Instant::now();
    let outcome = black_box(glyim_ledger::run_policy_mutation(black_box(&root), None));
    let run = started.elapsed();
    assert!(
        outcome.is_ok(),
        "all mutants must kill: {:?}",
        outcome.verdicts
    );
    assert_eq!(outcome.verdicts.len(), 9, "the catalogue is fixed");
    assert_eq!(outcome.features, 500);

    let started = Instant::now();
    let json = black_box(glyim_ledger::format_mutation_json(black_box(&outcome)));
    let json_elapsed = started.elapsed();
    assert!(json.contains("\"survived\": 0"));

    let started = Instant::now();
    let text = black_box(glyim_ledger::format_mutation_text(black_box(&outcome)));
    let text_elapsed = started.elapsed();
    assert!(text.contains("load-bearing"));

    println!(
        "policy-mutation bench: 9 mutants x 500 features (baseline + 9 computes)\n  run_policy_mutation : {run:.1?}\n  json                : {json_elapsed:.1?}\n  text                : {text_elapsed:.1?}"
    );
}
