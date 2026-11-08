use crate::folding::provide_folding_ranges;
use crate::tests::test_utils::setup_test_db;
use lsp_types::{FoldingRangeParams, TextDocumentIdentifier};

#[test]
fn test_folding_ranges_for_braces() {
    let source = r"fn main() {
    let x = 1;
}
";
    let (db, _file_map, uri, _file_id) = setup_test_db(source, "/test/main.g");
    let params = FoldingRangeParams {
        text_document: TextDocumentIdentifier { uri },
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
        partial_result_params: lsp_types::PartialResultParams::default(),
    };
    let ranges = provide_folding_ranges(&db, &params);
    assert!(ranges.is_some());
    let ranges = ranges.unwrap();
    assert_eq!(ranges.len(), 1);
    let range = &ranges[0];
    assert_eq!(range.start_line, 0);
    assert_eq!(range.start_character, Some(10));
    assert_eq!(range.end_line, 2);
    assert_eq!(range.end_character, Some(0));
    assert_eq!(range.kind, Some(lsp_types::FoldingRangeKind::Region));
}
