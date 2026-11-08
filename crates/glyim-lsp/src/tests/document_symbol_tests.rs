use crate::navigation::document_symbols;
use crate::{AnalysisDatabase, DefinitionLocation, SymbolInfo, SymbolKind, TypeSignature};
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
fn document_symbols_returns_hierarchy() {
    let analysis = AnalysisDatabase::new();
    let path = get_test_path("test.g");
    let file_id = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path)
    };
    let source_map =
        crate::SourceMap::new(file_id, "fn foo() {}\nstruct Bar { x: i32 }".to_string());
    analysis.source_maps.write().insert(file_id, source_map);

    let span_foo = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(7),
        SyntaxContext::ROOT,
    );
    let span_bar = Span::new(
        file_id,
        ByteIdx::from_raw(10),
        ByteIdx::from_raw(20),
        SyntaxContext::ROOT,
    );
    let foo_sym = SymbolInfo {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        definition: DefinitionLocation {
            file_id,
            span: span_foo,
        },
        type_signature: Some(TypeSignature {
            params: vec![],
            receiver_type: None,
            return_type: Some("()".to_string()),
            generic_params: vec![],
        }),
        is_pub: true,
        documentation: None,
    };
    let bar_sym = SymbolInfo {
        name: "Bar".to_string(),
        kind: SymbolKind::Struct,
        definition: DefinitionLocation {
            file_id,
            span: span_bar,
        },
        type_signature: None,
        is_pub: true,
        documentation: None,
    };
    {
        let mut sym_idx = analysis.symbol_index.write();
        sym_idx.insert_test_symbol(file_id, foo_sym);
        sym_idx.insert_test_symbol(file_id, bar_sym);
    }

    let file_map_guard = analysis.file_map.read();
    let file_map = &*file_map_guard;

    let params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Uri::from_str(
                &Uri::from_str(Url::from_file_path(&path).unwrap().as_ref())
                    .unwrap()
                    .to_string(),
            )
            .unwrap(),
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };

    let response = document_symbols(&analysis, file_map, &params);
    assert!(response.is_some());
    if let Some(DocumentSymbolResponse::Nested(symbols)) = response {
        let names: Vec<_> = symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"Bar"));
    } else {
        panic!("Expected nested document symbols");
    }
}
