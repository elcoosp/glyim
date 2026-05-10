mod alloc;
pub mod codegen;
mod debug;
pub mod dispatch;
mod hash_shims;
pub mod helpers;
pub mod live;
pub mod micro_module;
pub mod orc;
pub mod runtime_shims;
pub mod tiered;
pub use codegen::Codegen;
pub use codegen::CodegenBuilder;

/// Compile Glyim source to a WebAssembly binary (.wasm) for use as a macro.
pub fn compile_to_wasm(source: &str, target_triple: &str) -> Result<Vec<u8>, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    // Initialize all targets including WebAssembly
    inkwell::targets::Target::initialize_webassembly(
        &inkwell::targets::InitializationConfig::default(),
    );
    let mut cg = CodegenBuilder::new(&ctx, interner, vec![]).build()?;
    cg.set_target(target_triple);
    cg.generate(&hir)?;
    let tmp_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let obj_path = tmp_dir.path().join("output.o");
    cg.write_object_file(&obj_path)?;
    std::fs::read(&obj_path).map_err(|e| e.to_string())
}

pub fn compile_to_ir(source: &str) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = CodegenBuilder::new(&ctx, interner, vec![]).build()?;
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
    let mut cg = CodegenBuilder::new(&ctx, interner, vec![]).build()?;
    cg.generate_for_tests(&hir, test_names, &std::collections::HashSet::new())?;
    Ok(cg.ir_string())
}

/// Compile source to LLVM IR with debug info enabled.
/// Compile a list of HIR items (by index) into per-function object code blobs.
/// Each function is compiled in its own module.
pub fn compile_items_to_objects(
    _hir: &glyim_hir::Hir,
    mono_result: &glyim_hir::monomorphize::MonoResult,
    interner: &glyim_interner::Interner,
    item_indices: &[usize],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    use glyim_hir::item::HirItem;
    let ctx = inkwell::context::Context::create();
    let mut results = Vec::new();
    for &idx in item_indices {
        if idx >= mono_result.hir.items.len() { continue; }
        let item = &mono_result.hir.items[idx];
        let name = match item { HirItem::Fn(f) => interner.resolve(f.name).to_string(), _ => continue };
        let mini_hir = glyim_hir::Hir { items: vec![item.clone()] };
        let mut cg = CodegenBuilder::new(&ctx, interner.clone(), mono_result.expr_types.clone()).build()?;
        cg.generate(&mini_hir)?;
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let obj_path = tmp.path().join("out.o");
        cg.write_object_file(&obj_path)?;
        let bytes = std::fs::read(&obj_path).map_err(|e| e.to_string())?;
        results.push((name, bytes));
    }
    Ok(results)
}

pub fn compile_to_ir_debug(
    source: &str,
    enable_debug: bool,
    file_name: &str,
) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = if enable_debug {
        Codegen::with_debug(&ctx, interner, vec![], source.to_string(), file_name)?
    } else {
        CodegenBuilder::new(&ctx, interner, vec![]).build()?
    };
    cg.generate(&hir)?;
    Ok(cg.ir_string())
}

/// Compile source to LLVM IR with line-tables-only debug info.
pub fn compile_to_ir_line_tables(source: &str, file_name: &str) -> Result<String, String> {
    let out = glyim_parse::parse(source);
    if !out.errors.is_empty() {
        return Err(format!("parse: {:?}", out.errors));
    }
    let mut interner = out.interner;
    let hir = glyim_hir::lower(&out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let mut cg = Codegen::with_line_tables(&ctx, interner, vec![], source.to_string(), file_name)?;
    eprintln!("=== HIR BEFORE CODEGEN ===\n{:#?}\n=== END HIR ===", hir);
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
        let ir = compile_to_ir_line_tables("main = () => 42", "test.g").unwrap();
        assert!(
            ir.contains("!dbg"),
            "Line tables mode should have debug locations\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_line_tables_has_line_tables_only() {
        let ir = compile_to_ir_line_tables("main = () => 42", "test.g").unwrap();
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
        let ir = compile_to_ir_debug("main = () => 42", true, "test.g").unwrap();
        assert!(
            ir.contains("DISubprogram"),
            "IR should contain DISubprogram\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_has_debug_locations() {
        let ir = compile_to_ir_debug("main = () => 42", true, "test.g").unwrap();
        assert!(
            ir.contains("!dbg"),
            "IR should contain debug locations (!dbg)\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_release_no_debug() {
        let ir = compile_to_ir_debug("main = () => 42", false, "test.g").unwrap();
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
        let ir = compile_to_ir_debug(
            "fn helper() { 0 }\nmain = () => { helper() }",
            true,
            "test.g",
        )
        .unwrap();
        let count = ir.matches("DISubprogram").count();
        assert!(
            count >= 2,
            "Expected >= 2 DISubprogram, got {count}\nIR:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_has_local_variable() {
        let ir = compile_to_ir_debug("fn main() { let x = 42; x }", true, "test.g").unwrap();
        assert!(
            ir.contains("DILocalVariable"),
            "IR should contain DILocalVariable\nGot:\n{ir}"
        );
    }

    #[test]
    fn compile_to_ir_debug_macro_has_artificial_flag() {
        let src = "@identity fn transform(expr: Expr) -> Expr { return expr }
main = () => @identity(99)";
        let ir = compile_to_ir_debug(src, true, "test.g").unwrap();
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
pub mod wasm_abi;

#[cfg(test)]
mod tests;
