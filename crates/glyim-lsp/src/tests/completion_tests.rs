use crate::database::AnalysisDatabase;
use crate::completion::provide_completions;
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
        sm.insert(file_id, SourceMap::new(path.clone(), file_id, "fn add(a: i64, b: i64) -> i64 { a + b }".to_string()));
    }
    {
        let mut idx = db.symbol_index.write();
        idx.insert_test_symbol(file_id, SymbolInfo {
            name: "add".into(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation { file_id, span: Span::new(0, 3) },
            type_signature: Some(TypeSignature {
                params: vec![("a".into(), glyim_hir::types::HirType::Int), ("b".into(), glyim_hir::types::HirType::Int)],
                return_type: Some(glyim_hir::types::HirType::Int),
            }),
            is_pub: false,
            documentation: Some("Adds two integers.".into()),
        });
        idx.insert_test_symbol(file_id, SymbolInfo {
            name: "main".into(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation { file_id, span: Span::new(23, 27) },
            type_signature: Some(TypeSignature {
                params: vec![],
                return_type: Some(glyim_hir::types::HirType::Int),
            }),
            is_pub: false,
            documentation: None,
        });
    }
    (db, file_id)
}

fn completion_params() -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path("/test/main.g").unwrap(),
            },
            position: Position { line: 0, character: 0 },
        },
        work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
        partial_result_params: PartialResultParams { partial_result_token: None },
        context: None,
    }
}

#[test]
fn completion_provides_items() {
    let (db, _file_id) = make_test_db();
    let file_map = db.file_map.read();
    let params = completion_params();
    let response = provide_completions(&db, &file_map, &params).expect("should return completions");
    match response {
        CompletionResponse::List(list) => {
            assert_eq!(list.items.len(), 2);
            let names: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
            assert!(names.contains(&"add"));
            assert!(names.contains(&"main"));
        }
        _ => panic!("expected list"),
    }
}

#[test]
fn completion_has_type_signatures() {
    let (db, _file_id) = make_test_db();
    let file_map = db.file_map.read();
    let params = completion_params();
    let response = provide_completions(&db, &file_map, &params).unwrap();
    if let CompletionResponse::List(list) = response {
        let add_item = list.items.iter().find(|i| i.label == "add").unwrap();
        assert_eq!(add_item.kind, Some(CompletionItemKind::FUNCTION));
        assert!(add_item.detail.as_ref().unwrap().contains("a: Int, b: Int"));
        assert!(add_item.documentation.is_some());
        let insert_text = add_item.insert_text.as_ref().unwrap();
        assert!(insert_text.contains("${1:a"), "expected snippet placeholder, got {}", insert_text);
    }
}

#[test]
fn completion_snippet_without_params() {
    let (db, _file_id) = make_test_db();
    let file_map = db.file_map.read();
    let params = completion_params();
    let response = provide_completions(&db, &file_map, &params).unwrap();
    if let CompletionResponse::List(list) = response {
        let main_item = list.items.iter().find(|i| i.label == "main").unwrap();
        assert_eq!(main_item.insert_text.as_ref().unwrap(), "main()");
    }
}
