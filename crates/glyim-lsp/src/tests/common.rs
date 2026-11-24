use glyim_core::Interner;
use glyim_frontend::parse_to_syntax;
use glyim_hir::pipeline_api::lower_crate_for_pipeline;
use glyim_span::FileId;

pub fn compile_to_hir(
    src: &str,
    file_id: FileId,
    interner: &mut Interner,
) -> (glyim_hir::CrateHir, Vec<glyim_diag::GlyimDiagnostic>) {
    let parse_result = parse_to_syntax(src, file_id);
    let (hir, diags) = lower_crate_for_pipeline(&parse_result.root, interner);
    (hir, diags)
}

pub fn create_test_file_id(raw: u32) -> FileId {
    FileId::from_usize(raw as usize)
}
