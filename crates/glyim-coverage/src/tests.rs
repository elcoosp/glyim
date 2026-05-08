use crate::data::CoverageDump;

#[test]
fn empty_dump_has_no_files() {
    let json = r#"{"files":{},"counters":{},"metadata":{},"version":1}"#;
    let dump: CoverageDump = serde_json::from_str(json).unwrap();
    assert!(dump.files.is_empty());
    assert!(dump.counters.is_empty());
}
