//! Status-renderer bench (M1.9 first slice).
//!
//! Dependency-free (`harness = false`), mirroring the `compute` bench:
//! measures both renderers over a synthetic 500-feature project with one
//! gap per even index and 2000 evidence records, so CI logs can track the
//! render cost as the registry grows. Run with:
//!
//! ```text
//! cargo bench -p glyim-ledger --bench status
//! ```

use std::fmt::Write as _;
use std::hint::black_box;
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

/// Writes a synthetic project under `root` (same shape as the `compute`
/// bench: `count` features, one L2-capped gap per even index, the spec
/// anchor, and the policy).
fn write_project(root: &std::path::Path, count: usize) {
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

/// Writes `per_feature` evidence records for each of `count` features.
fn write_evidence(root: &std::path::Path, count: usize, per_feature: usize) {
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

    let started = Instant::now();
    let outcome = black_box(glyim_ledger::run_status(dir.path(), None));
    let run = started.elapsed();
    assert!(outcome.is_ok(), "{:?}", outcome.compute.diagnostics);

    let started = Instant::now();
    let json = black_box(glyim_ledger::format_status_json(black_box(
        &outcome.compute,
    )));
    let json_elapsed = started.elapsed();

    let started = Instant::now();
    let markdown = black_box(glyim_ledger::format_status_markdown(black_box(
        &outcome.compute,
    )));
    let markdown_elapsed = started.elapsed();

    assert!(json.contains("\"schema\": \"glyim.status/1\""));
    assert!(markdown.starts_with(glyim_ledger::STATUS_MD_START));
    // Determinism: two renders are byte-identical (plan C1/M1.13).
    assert_eq!(json, glyim_ledger::format_status_json(&outcome.compute));
    assert_eq!(
        markdown,
        glyim_ledger::format_status_markdown(&outcome.compute)
    );

    println!(
        "status bench: 500 features, {} records",
        outcome.compute.records
    );
    println!("  run:      {run:?}");
    println!("  json:     {json_elapsed:?}");
    println!("  markdown: {markdown_elapsed:?}");
}
