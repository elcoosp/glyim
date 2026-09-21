use std::sync::Arc;

/// CodegenTester.
pub struct CodegenTester;

impl CodegenTester {
    /// generate.
    pub fn generate(
        backend: &dyn glyim_codegen::CodegenBackend,
        bodies: &[Arc<glyim_mir::Body>],
        output: &std::path::Path,
    ) -> glyim_diag::CompResult<()> {
        backend.generate(bodies, output)
    }
    /// generate_function.
    pub fn generate_function(
        backend: &dyn glyim_codegen::CodegenBackend,
        body: &Arc<glyim_mir::Body>,
    ) -> glyim_diag::CompResult<Vec<u8>> {
        backend.generate_function(body)
    }
}
