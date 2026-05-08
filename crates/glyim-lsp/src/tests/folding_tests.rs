use crate::database::AnalysisDatabase;
use crate::folding::provide_folding_ranges;
use glyim_diag::{FileId, SourceMap};
use std::path::PathBuf;
use lsp_types::*;

#[test]
fn folding_ranges_detected() {
    let db = AnalysisDatabase::new();
    let file_id = FileId(0);
    let path = PathBuf::from("/test/main.g");
    {
        let mut fm = db.file_map.write();
        fm.get_or_create(&path);
    }
    {
        let mut sm = db.source_maps.write();
        sm.insert(file_id, SourceMap::new(path.clone(), file_id, "fn main() {\n    let x = 1;\n}\n".to_string()));
    }
    let params = FoldingRangeParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path("/test/main.g").unwrap(),
        },
        work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
        partial_result_params: PartialResultParams { partial_result_token: None },
    };
    let ranges = provide_folding_ranges(&db, &params).expect("should return ranges");
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0].start_line, 0);
    assert_eq!(ranges[0].end_line, 2);
}
