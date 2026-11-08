use crate::database::{AnalysisDatabase, SourceMap};
use glyim_core::Interner;

#[test]
fn diagnostics_are_emitted_on_change() {
    let db = AnalysisDatabase::new();
    let path = std::path::PathBuf::from("/test/diag.g");
    let file_id = db.file_map.write().get_or_create(&path);

    let sm = SourceMap::new(file_id, "fn main() { @ }".to_string());
    db.source_maps.write().insert(file_id, sm.clone());

    let lex_result = glyim_frontend::lex("fn main() { @ }", file_id);
    let parse_result = glyim_frontend::parse_to_syntax("fn main() { @ }", file_id);
    let mut interner = Interner::new();
    let (_hir, _hir_diags) =
        glyim_hir::pipeline_api::lower_crate_for_pipeline(&parse_result.root, &mut interner);

    let mut all_diags = Vec::new();
    all_diags.extend(lex_result.diagnostics);
    all_diags.extend(parse_result.diagnostics);

    let lsp_diags = crate::diagnostics::convert_diagnostics(file_id, &sm, &all_diags);
    if !lsp_diags.is_empty() {
        db.diagnostics.write().insert(file_id, lsp_diags);
    }

    let guard = db.diagnostics.read();
    let stored = guard.get(&file_id);
    assert!(
        stored.is_some(),
        "File with parse error should produce diagnostics"
    );
}
