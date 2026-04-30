mod alloc;
pub mod codegen;
mod debug;
mod runtime_shims;
pub use codegen::Codegen;

pub fn compile_to_ir(source: &str) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
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
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = Codegen::new(&ctx, interner, vec![]);
    cg.generate_for_tests(&hir, test_names, &std::collections::HashSet::new())?;
    Ok(cg.ir_string())
}

/// Compile source to LLVM IR with debug info enabled.
pub fn compile_to_ir_debug(source: &str, enable_debug: bool) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = if enable_debug {
        Codegen::with_debug(&ctx, interner, vec![], source.to_string())?
    } else {
        Codegen::new(&ctx, interner, vec![])
    };
    cg.generate(&hir)?;
    Ok(cg.ir_string())
}

/// Compile source to LLVM IR with line-tables-only debug info.
pub fn compile_to_ir_line_tables(source: &str) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = Codegen::with_line_tables(&ctx, interner, vec![], source.to_string())?;
    cg.generate(&hir)?;
    Ok(cg.ir_string())
}

#[cfg(test)]
mod test_harness_tests {
    use super::*;

    #[test]
    fn test_compile_to_ir_tests_single() {
        let ir = compile_to_ir_tests("fn check() { 0 }", &["check".to_string()]).unwrap();
        assert!(ir.contains("@check"));
        assert!(ir.contains("@main"));
    }

    #[test]
    fn test_harness_skips_user_main() {
        let ir = compile_to_ir_tests(
            "fn main() { 42 }
fn check() { 0 }",
            &["check".to_string()],
        )
        .unwrap();
        let mains = ir
            .lines()
            .filter(|l| l.contains("define i32 @main"))
            .count();
        assert_eq!(mains, 1);
    }
}
#[cfg(test)]
mod line_tables_tests {
    use super::*;

    #[test]
    fn compile_to_ir_line_tables_has_debug_locations() {
        let ir = compile_to_ir_line_tables("main = () => 42").unwrap();
        assert!(
            ir.contains("!dbg"),
            "Line tables mode should have debug locations\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_line_tables_has_line_tables_only() {
        let ir = compile_to_ir_line_tables("main = () => 42").unwrap();
        assert!(
            ir.contains("emissionKind: LineTablesOnly"),
            "Line tables mode should specify LineTablesOnly emission kind\nGot:\n{ir}"
        );
    }
}
#[cfg(test)]
mod debug_ir_tests {
    use super::*;

    #[test]
    fn compile_to_ir_debug_has_subprogram() {
        let ir = compile_to_ir_debug("main = () => 42", true).unwrap();
        assert!(
            ir.contains("DISubprogram"),
            "IR should contain DISubprogram\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_has_debug_locations() {
        let ir = compile_to_ir_debug("main = () => 42", true).unwrap();
        assert!(
            ir.contains("!dbg"),
            "IR should contain debug locations (!dbg)\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_release_no_debug() {
        let ir = compile_to_ir_debug("main = () => 42", false).unwrap();
        assert!(
            !ir.contains("DISubprogram"),
            "Release should have no subprograms\nGot:\n{ir}"
        );
        assert!(
            !ir.contains("!dbg"),
            "Release should have no debug locations\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_multi_function() {
        let ir = compile_to_ir_debug("fn helper() { 0 }\nmain = () => { helper() }", true).unwrap();
        let count = ir.matches("DISubprogram").count();
        assert!(
            count >= 2,
            "Expected >= 2 DISubprogram, got {count}\nIR:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_has_local_variable() {
        let ir = compile_to_ir_debug("fn main() { let x = 42; x }", true).unwrap();
        assert!(
            ir.contains("DILocalVariable"),
            "IR should contain DILocalVariable\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_macro_has_artificial_flag() {
        let src = "@identity fn transform(expr: Expr) -> Expr { return expr }
main = () => @identity(99)";
        let ir = compile_to_ir_debug(src, true).unwrap();
        // Macro-generated function should have DIFlagArtificial
        assert!(
            ir.contains("DISubprogram"),
            "IR should contain DISubprogram
Got:
{ir}"
        );
        // The transform function is macro-generated, so it should be present
        assert!(
            ir.contains("transform"),
            "IR should contain function transform
Got:
{ir}"
        );
    }
}
