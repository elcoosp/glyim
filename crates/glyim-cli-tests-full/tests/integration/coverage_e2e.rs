use glyim_compiler::pipeline;

#[test]
fn coverage_jit_run_produces_dump() {
    let dir = tempfile::tempdir().unwrap();
    let cov_path = dir.path().join("glyim-cov.json");
    let source = "fn main() -> i64 { let mut i = 0; while i < 2 { i = i + 1 }; i }";
    let result = pipeline::run_jit_with_coverage(source, &cov_path).unwrap();
    assert_eq!(result, 2);
    let data = std::fs::read_to_string(&cov_path).expect("read cov file");
    let dump: glyim_coverage::data::CoverageDump = serde_json::from_str(&data).unwrap();
    assert!(
        dump.counters.len() >= 2,
        "expected at least 2 counters (function + branch), got {}",
        dump.counters.len()
    );
    let has_branch = dump
        .metadata
        .values()
        .any(|loc| matches!(loc.kind, glyim_coverage::data::LocationKind::Branch));
    assert!(has_branch, "expected at least one branch counter");
}
