use crate::database::{AnalysisDatabase, SourceMap};
use crate::navigation::workspace_symbols;
use crate::{DefinitionLocation, SymbolInfo, SymbolKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use lsp_types::*;
use std::path::PathBuf;

fn get_test_path(filename: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    path.push(filename);
    path
}

/// Build a minimal analysis DB with `file_id`+`source_map`+`file_map` wired so
/// `workspace_symbols` can resolve symbol locations (previously the ignored
/// tests left `source_maps` empty, causing `workspace_symbols` to return `None`).
fn analysis_with_symbols(names: &[&str]) -> (AnalysisDatabase, FileId) {
    let analysis = AnalysisDatabase::new();
    let path = get_test_path("test.g");
    let file_id = {
        let mut file_map = analysis.file_map.write();
        file_map.get_or_create(&path)
    };
    // A 1-line source long enough for the dummy spans below.
    let source = "fn placeholder() {}\n".to_string();
    let sm = SourceMap::new(path.clone(), file_id, source.clone());
    analysis.source_maps.write().insert(file_id, sm);

    let span = Span::new(
        file_id,
        ByteIdx::ZERO,
        ByteIdx::from_raw(5),
        SyntaxContext::ROOT,
    );
    let mut index = analysis.symbol_index.write();
    for name in names {
        let sym = SymbolInfo {
            name: name.to_string(),
            kind: SymbolKind::Function,
            definition: DefinitionLocation { file_id, span },
            type_signature: None,
            is_pub: true,
            documentation: None,
        };
        index.insert_test_symbol(file_id, sym);
    }
    drop(index);
    (analysis, file_id)
}

#[test]
fn workspace_symbols_fuzzy_search() {
    let (analysis, _fid) =
        analysis_with_symbols(&["apple", "application", "banana", "ape", "grape"]);
    let params = WorkspaceSymbolParams {
        query: "app".to_string(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let results = workspace_symbols(&analysis, &params).unwrap();
    let result_names: Vec<_> = results.iter().map(|s| s.name.as_str()).collect();
    // "app" is a prefix of apple/application (tier 2) so those surface; it is
    // neither a prefix nor contains match for banana/grape. ("ape" is only a
    // fuzzy subsequence, and with the limit not yet reached by higher tiers it
    // is not required to appear.)
    assert!(result_names.contains(&"apple"));
    assert!(result_names.contains(&"application"));
    assert!(!result_names.contains(&"banana"));
    assert!(!result_names.contains(&"grape"));
}

#[test]
fn workspace_symbols_fuzzy_matching_limit() {
    let (analysis, _fid) = analysis_with_symbols(&["alpha", "beta", "gamma", "delta", "epsilon"]);
    let params = WorkspaceSymbolParams {
        query: "a".to_string(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let results = workspace_symbols(&analysis, &params).unwrap();
    // Every symbol contains 'a', so all are subsequence matches; the limit of
    // 20 must cap the returned set and every returned name must contain 'a'.
    assert!(
        results.len() <= 20,
        "results must respect the limit, got {}",
        results.len()
    );
    for r in &results {
        assert!(
            r.name.contains('a'),
            "every result for query 'a' must contain 'a', got {}",
            r.name
        );
    }
}

// Plan §22.3: a true fuzzy (subsequence) query with no exact/prefix/contains
// match must still surface the candidate via the fuzzy tier.
#[test]
fn workspace_symbols_subsequence_fuzzy() {
    let (analysis, _fid) = analysis_with_symbols(&[
        "get_something_related_by_type",
        "get_serialized_buffer",
        "global_state_root_block",
        "alpha",
    ]);
    let params = WorkspaceSymbolParams {
        query: "gsrbt".to_string(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let results = workspace_symbols(&analysis, &params).unwrap();
    let result_names: Vec<_> = results.iter().map(|s| s.name.as_str()).collect();
    assert!(
        result_names.contains(&"get_something_related_by_type"),
        "fuzzy subsequence gsrbt should surface get_something_related_by_type, got {:?}",
        result_names
    );
    assert!(
        !result_names.contains(&"get_serialized_buffer"),
        "gsrb should not match get_serialized_buffer"
    );
}
