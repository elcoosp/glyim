pub mod codegen;
mod alloc;
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

/// Compile source to LLVM IR in test mode.
pub fn compile_to_ir_tests(source: &str, test_names: &[String]) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() { return Err(format!("parse: {:?}", out.errors)); }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = Codegen::new(&ctx, interner, vec![]);
    cg.generate_for_tests(&hir, test_names, &std::collections::HashSet::new())?;
    Ok(cg.ir_string())
}

#[cfg(test)]
mod test_harness_tests {
    use super::*;

    #[test]
    fn test_compile_to_ir_tests_single() {
        let ir = compile_to_ir_tests(
            "fn check() { 0 }",
            &["check".to_string()],
        ).unwrap();
        assert!(ir.contains("@check"));
        assert!(ir.contains("@main"));
    }

    #[test]
    fn test_harness_skips_user_main() {
        let ir = compile_to_ir_tests(
            "fn main() { 42 }
fn check() { 0 }",
            &["check".to_string()],
        ).unwrap();
        let mains = ir.lines().filter(|l| l.contains("define i32 @main")).count();
        assert_eq!(mains, 1);
    }
}
