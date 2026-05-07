use glyim_compiler::pipeline;

#[test]
fn coverage_jit_run_produces_dump() {
    let dir = tempfile::tempdir().unwrap();
    let cov_path = dir.path().join("glyim-cov.json");
    let source = "fn main() -> i64 { let x = 42; x }";
    let result = pipeline::run_jit_with_coverage(source, &cov_path).unwrap();
    assert_eq!(result, 42);
    let data = std::fs::read_to_string(&cov_path).expect("read cov file");
    let dump: glyim_coverage::data::CoverageDump = serde_json::from_str(&data).unwrap();
    assert!(!dump.counters.is_empty(), "counters should not be empty");
}
