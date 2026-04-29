pub mod codegen;
mod runtime_shims;
pub use codegen::Codegen;

pub fn compile_to_ir(source: &str) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() { return Err(format!("parse: {:?}", out.errors)); }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = Codegen::new(&ctx, interner, vec![]);
    cg.generate(&hir)?;
    Ok(cg.ir_string())
}
