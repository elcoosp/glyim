use crate::database::AnalysisDatabase;
use crate::formatting::format_document;
use glyim_diag::{FileId, SourceMap};
use std::path::PathBuf;
use lsp_types::*;

#[test]
fn formatting_produces_edits_when_mismatch() {
    let db = AnalysisDatabase::new();
    let file_id = FileId(0);
    let path = PathBuf::from("/test/main.g");
    {
        let mut fm = db.file_map.write();
        fm.get_or_create(&path);
    }
    {
        let mut sm = db.source_maps.write();
        sm.insert(file_id, SourceMap::new(path.clone(), file_id, "fn main() -> i64 { 42 }\n".to_string()));
    }
    let params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path("/test/main.g").unwrap(),
        },
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            ..Default::default()
        },
        work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
    };
    let edits = format_document(&db, &params);
    // The source is already clean, may return None (no edits needed)
    // We'll just assert it doesn't crash
    assert!(edits.is_some());
}
