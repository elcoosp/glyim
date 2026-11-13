//! Determinism self-check bench (plan M1.13, first slice).
//!
//! Dependency-free (`harness = false`), mirroring the `gates` bench:
//! measures a full [`glyim_ledger::verify_determinism`] pass — both
//! independent load/render passes of compute, status, and gates — plus
//! both renderers over the synthetic 500-feature project, so CI logs can
//! track the self-check cost against the gates it composes. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench verify
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::Instant;

/// The synthetic 24-gate declaration (same shape the `gates` bench
/// writes; `verify` re-runs the gates pipeline twice, so the declaration
/// must be present).
const GATES_HEAD: &str = r#"schema = "glyim.ledger-gates-registry/1"

[[gate]]
id = "RG-09"
title = "Feature closure"
criterion = "All core and std features >= L4; core >= L5"
evidence = "levels"

[[gate]]
id = "RG-10"
title = "Gap closure"
criterion = "0 open class-b gaps; every class-a gap has diagnostic + docs; witness debt = 0"
evidence = "gaps"
"#;

/// The synthetic policy (the compute pass inside `verify` loads it).
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

/// Writes a synthetic project under `root` (same shape as the `gates`
/// bench: `count` features, one unwitnessed class-b gap per even index,
/// the spec anchor, the policy, and the 24-gate declaration).
fn write_project(root: &std::path::Path, count: usize) {
    let ledger = root.join("ledger");
    let features = ledger.join("features");
    let gaps = ledger.join("gaps");
    std::fs::create_dir_all(&features).expect("mkdir features");
    std::fs::create_dir_all(&gaps).expect("mkdir gaps");
    std::fs::create_dir_all(root.join("docs/specs")).expect("mkdir specs");
    std::fs::write(root.join("docs/specs/v0.1.0.md"), "# spec fixture\n").expect("write spec");
    std::fs::write(root.join("ledger/policy.toml"), POLICY).expect("write policy");

    let mut gates = String::from(GATES_HEAD);
    for (id, title) in [
        ("RG-01", "No ICE"),
        ("RG-02", "Backend agreement"),
        ("RG-03", "Optimizer validity"),
        ("RG-04", "Determinism"),
        ("RG-05", "Incremental soundness"),
        ("RG-06", "Diagnostics quality"),
        ("RG-07", "Panic/unsafe surface"),
        ("RG-08", "Test strength"),
        ("RG-11", "Compile-time budgets"),
        ("RG-12", "Scaling"),
        ("RG-13", "Memory"),
        ("RG-14", "Incremental latency"),
        ("RG-15", "Generated-code performance"),
        ("RG-16", "IDE latency"),
        ("RG-17", "Reproducible release"),
        ("RG-18", "Supply chain"),
        ("RG-19", "Platform matrix"),
        ("RG-20", "Spec coverage"),
        ("RG-21", "Stdlib"),
        ("RG-22", "Tooling"),
        ("RG-23", "Stability"),
        ("RG-24", "Continuous fuzzing"),
    ] {
        let _ = writeln!(
            gates,
            "\n[[gate]]\n\
             id = \"{id}\"\n\
             title = \"{title}\"\n\
             criterion = \"see the master plan's RG gate table\"\n\
             evidence = \"none\"\n"
        );
    }
    std::fs::write(ledger.join("gates.toml"), gates).expect("write gates.toml");

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

fn main() {
    let dir = tempfile::tempdir().expect("tmp");
    write_project(dir.path(), 500);

    let started = Instant::now();
    let outcome = black_box(glyim_ledger::verify_determinism(dir.path(), None));
    let run = started.elapsed();
    assert!(outcome.is_ok(), "{:?}", outcome.diagnostics);
    assert_eq!(outcome.checks.len(), 6, "all six pipelines must self-check");
    for check in &outcome.checks {
        assert!(check.identical, "check {} drifted", check.name);
    }

    let started = Instant::now();
    let json = black_box(glyim_ledger::format_verify_json(black_box(&outcome)));
    let json_elapsed = started.elapsed();
    assert!(json.contains("\"checks\": 6"));

    let started = Instant::now();
    let text = black_box(glyim_ledger::format_verify_text(black_box(&outcome)));
    let text_elapsed = started.elapsed();
    assert!(text.contains("byte-identical"));

    println!(
        "verify bench: 6 double-run checks over 500 features + gaps\n  verify_determinism : {run:.1?}\n  json               : {json_elapsed:.1?}\n  text               : {text_elapsed:.1?}"
    );
}
