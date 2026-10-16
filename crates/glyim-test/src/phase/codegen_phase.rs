use std::sync::Arc;

/// CodegenTester.
pub struct CodegenTester;

impl CodegenTester {
    /// Generate an object/executable from a slice of `(symbol, body)` pairs.
    /// The symbol is the mangled LLVM name the pipeline chose for that body
    /// (`glyim_type::mangle_fn_symbol`); tests that don't care about
    /// generics can pass the plain legacy form (`__glyim_fn_<id>`).
    pub fn generate(
        backend: &dyn glyim_codegen::CodegenBackend,
        bodies: &[(String, Arc<glyim_mir::Body>)],
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
