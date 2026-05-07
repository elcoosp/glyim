use crate::database::AnalysisDatabase;
use crate::hover::provide_hover;
use crate::symbol_index::{SymbolInfo, SymbolKind, DefinitionLocation, TypeSignature};
use glyim_diag::{FileId, Span, SourceMap};
use std::path::PathBuf;
use lsp_types::*;

fn make_test_db() -> (AnalysisDatabase, FileId) {
    let db = AnalysisDatabase::new();
    let file_id = FileId(0);
    let path = PathBuf::from("/test/main.g");
    {
        let mut fm = db.file_map.write();
        fm.get_or_create(&path);
    }
    {
        let mut sm = db.source_maps.write();
        sm.insert(file_id, SourceMap::new(path.clone(), file_id, "fn add(a: i64) -> i64 { a }\n".to_string()));
    }
    {
        let mut idx = db.symbol_index.write();
        idx.insert_test_symbol(file_id, SymbolInfo {
            name: "add".into(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation { file_id, span: Span::new(0, 3) },
            type_signature: Some(TypeSignature {
                params: vec![("a".into(), glyim_hir::types::HirType::Int)],
                return_type: Some(glyim_hir::types::HirType::Int),
            }),
            is_pub: false,
            documentation: Some("Adds one.".into()),
        });
    }
    (db, file_id)
}

fn hover_params() -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path("/test/main.g").unwrap(),
            },
            position: Position { line: 0, character: 0 }, // within "fn"
        },
        work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
    }
}

#[test]
fn hover_returns_markdown() {
    let (db, _file_id) = make_test_db();
    let file_map = db.file_map.read();
    let params = hover_params();
    let hover = provide_hover(&db, &file_map, &params).expect("should return hover");
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(markup.value.contains("add"));
        assert!(markup.value.contains("a: Int"));
        assert!(markup.value.contains("Adds one"));
    } else {
        panic!("expected markdown");
    }
}
