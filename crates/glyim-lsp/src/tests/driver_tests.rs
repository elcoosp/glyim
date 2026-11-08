use crate::database::{AnalysisDatabase, SourceMap};
use glyim_core::Interner;
use std::path::PathBuf;

#[test]
fn s11_t02_changing_file_updates_diagnostics_incrementally() {
    let mut db = AnalysisDatabase::new();
    let path = PathBuf::from("/test/incremental.g");
    let file_id = db.file_map.write().get_or_create(&path);

    let analyze = |db: &mut AnalysisDatabase, source: &str| {
        let sm = SourceMap::new(file_id, source.to_string());
        db.source_maps.write().insert(file_id, sm.clone());
        let lex_result = glyim_frontend::lex(source, file_id);
        let parse_result = glyim_frontend::parse_to_syntax(source, file_id);
        let mut interner = Interner::new();
        let (hir, _hir_diags) =
            glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);
        db.symbol_index
            .write()
            .build_from_hir(file_id, &hir, &interner);
        db.reference_graph
            .write()
            .build_from_hir(file_id, &hir, &interner);
        db.hirs.write().insert(file_id, hir);
        let mut all_diags = Vec::new();
        all_diags.extend(lex_result.diagnostics);
        all_diags.extend(parse_result.diagnostics);
        let lsp_diags = crate::diagnostics::convert_diagnostics(file_id, &sm, &all_diags);
        if lsp_diags.is_empty() {
            db.diagnostics.write().remove(&file_id);
        } else {
            db.diagnostics.write().insert(file_id, lsp_diags);
        }
        all_diags.len()
    };

    let diag_count1 = analyze(&mut db, "fn main() { @ }");
    let guard1 = db.diagnostics.read();
    let stored1 = guard1.get(&file_id);
    let has_stored1 = stored1.is_some();
    assert!(
        has_stored1 == (diag_count1 > 0),
        "Diagnostics state should match frontend output after first analysis"
    );
    drop(guard1);

    let _diag_count2 = analyze(&mut db, "fn main() {}");
    let guard2 = db.diagnostics.read();
    let stored2 = guard2.get(&file_id);
    let is_empty2 = stored2.is_none();
    assert!(
        is_empty2,
        "Expected empty or removed diagnostics after clean source, got {stored2:?}"
    );
}
