use crate::goto_definition::goto_definition;
use crate::reference_graph::AccessKind;
use crate::{
    AnalysisDatabase, DefinitionLocation, Reference, ReferenceKind, SymbolInfo, SymbolKind,
};
use glyim_span::{ByteIdx, Span, SyntaxContext};
use lsp_types::Uri;
use lsp_types::*;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

fn get_test_path(filename: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(filename);
    path
}

#[test]
fn goto_definition_returns_location() {
    let analysis = AnalysisDatabase::new();
    let path = get_test_path("test.g");
    let file_id = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path)
    };
    let span = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    let sym = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation { file_id, span },
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    analysis
        .symbol_index
        .write()
        .insert_test_symbol(file_id, sym);

    let source_map = crate::SourceMap::new(path.clone(), file_id, "foo".to_string());
    analysis.source_maps.write().insert(file_id, source_map);

    let file_map_guard = analysis.file_map.read();
    let file_map = &*file_map_guard;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::from_str(
                    &Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
                        .unwrap()
                        .to_string(),
                )
                .unwrap(),
            },
            position: Position {
                line: 0,
                character: 0,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let response = goto_definition(&analysis, file_map, &params);
    assert!(response.is_some());
}

#[test]
fn goto_definition_cross_file() {
    let analysis = AnalysisDatabase::new();
    let path1 = get_test_path("main.g");
    let path2 = get_test_path("lib.g");

    let file_id1 = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path1)
    };
    let file_id2 = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path2)
    };

    let span_def = Span::new(
        file_id2,
        ByteIdx::from_raw(5),
        ByteIdx::from_raw(8),
        SyntaxContext::ROOT,
    );
    let sym = SymbolInfo {
        name: "helper".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation {
            file_id: file_id2,
            span: span_def,
        },
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    analysis
        .symbol_index
        .write()
        .insert_test_symbol(file_id2, sym);

    let source_map1 = crate::SourceMap::new(
        path1.clone(),
        file_id1,
        "fn main() { helper(); }".to_string(),
    );
    analysis.source_maps.write().insert(file_id1, source_map1);
    let source_map2 = crate::SourceMap::new(path2.clone(), file_id2, "fn helper() {}".to_string());
    analysis.source_maps.write().insert(file_id2, source_map2);

    let usage_span = Span::new(
        file_id1,
        ByteIdx::from_raw(15),
        ByteIdx::from_raw(21),
        SyntaxContext::ROOT,
    );
    let reference = Reference {
        file_id: file_id1,
        span: usage_span,
        is_definition: false,
        kind: ReferenceKind::Call,
        access: AccessKind::Read,

        def_id: None,

        owner_item_id: None,
            is_item_level: false,
            };
    analysis
        .reference_graph
        .write()
        .insert_test_reference("helper", reference);

    let file_map_guard = analysis.file_map.read();
    let file_map = &*file_map_guard;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::from_str(Url::from_file_path(&path1).unwrap().as_ref()).unwrap(),
            },
            position: Position {
                line: 0,
                character: 17,
            },
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let response = goto_definition(&analysis, file_map, &params);
    assert!(response.is_some());
    if let GotoDefinitionResponse::Scalar(loc) = response.unwrap() {
        assert_eq!(
            loc.uri,
            Uri::from_str(Url::from_file_path(&path2).unwrap().as_ref()).unwrap()
        );
    } else {
        panic!("Expected scalar location");
    }
}
