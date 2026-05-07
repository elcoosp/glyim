use crate::codegen::{CodegenBuilder, CoverageMode};
use glyim_hir::lower;

#[test]
fn coverage_instrumentation_emits_counter_increment() {
    let source = "fn main() -> i64 { 42 }";
    let parse_out = glyim_parse::parse(source);
    let mut interner = parse_out.interner;
    let hir = lower(&parse_out.ast, &mut interner);
    let ctx = inkwell::context::Context::create();
    let builder =
        CodegenBuilder::new(&ctx, interner, vec![]).with_coverage_mode(CoverageMode::Function);
    let mut cg = builder.build().expect("codegen build");
    cg.generate(&hir).expect("codegen generate");
    let ir = cg.ir_string();
    assert!(
        ir.contains("__glyim_cov_counts"),
        "Expected global coverage counter array, got:\n{ir}"
    );
    assert!(
        ir.contains("store i64"),
        "Expected counter increment, got:\n{ir}"
    );
}
