use crate::database::AnalysisDatabase;
use crate::navigation::{goto_definition, find_references, document_symbols};
use crate::reference_graph::{ReferenceGraph, Reference, ReferenceKind};
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
            documentation: None,
        });
    }
    {
        let mut refs = db.reference_graph.write();
        let mut graph = ReferenceGraph::new();
        graph.references.insert("add".into(), vec![
            Reference {
                file_id,
                span: Span::new(0, 3),
                is_definition: true,
                kind: ReferenceKind::Call,
            },
        ]);
        *refs = graph;
    }
    (db, file_id)
}

#[test]
fn goto_definition_works() {
    let (db, file_id) = make_test_db();
    let file_map = db.file_map.read();
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path("/test/main.g").unwrap(),
            },
            position: Position { line: 0, character: 1 },
        },
        work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
        partial_result_params: PartialResultParams { partial_result_token: None },
    };
    let result = goto_definition(&db, &file_map, &params);
    assert!(result.is_some());
}

#[test]
fn document_symbols_works() {
    let (db, _file_id) = make_test_db();
    let file_map = db.file_map.read();
    let params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path("/test/main.g").unwrap(),
        },
        work_done_progress_params: WorkDoneProgressParams { work_done_token: None },
        partial_result_params: PartialResultParams { partial_result_token: None },
    };
    let result = document_symbols(&db, &file_map, &params);
    assert!(result.is_some());
}
