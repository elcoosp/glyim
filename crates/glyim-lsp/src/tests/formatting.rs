use crate::formatting::format_document;
use crate::tests::test_utils::setup_test_db;
use lsp_types::{DocumentFormattingParams, FormattingOptions, TextDocumentIdentifier};

#[test]
fn test_format_document() {
    let source = "fn main(){let x=1;}";
    let expected = "fn main(){\n    let x=1;\n}\n";
    let (db, _file_map, uri, _file_id) = setup_test_db(source, "/test/main.g");
    let params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        options: FormattingOptions::default(),
        work_done_progress_params: lsp_types::WorkDoneProgressParams::default(),
    };
    let edits = format_document(&db, &params);
    assert!(edits.is_some());
    let edits = edits.unwrap();
    assert_eq!(edits.len(), 1);
    let edit = &edits[0];
    assert_eq!(edit.new_text, expected);
}
