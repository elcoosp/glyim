//! Pipeline-level proof that a constant defined in the value namespace is
//! referenceable through a value-namespace path (`CONST` and `mod::CONST`).
//!
//! This exercises: HIR `ConstItem` type registration in `typeck_crate`,
//! `check_path` resolving the const to a `thir::ExprKind::ConstRef` carrying
//! the constant's value type, and MIR lowering to `MirConstKind::ConstRef`.
use glyim_span::FileId;
use glyim_test::assert_no_errors;
use glyim_test::harness::compiler::{CompileOutput, PipelineCompiler, TestCompiler};
use std::sync::Arc;

use glyim_test::mock::MockCodegen;

fn compile(src: &str) -> CompileOutput {
    let backend = Arc::new(MockCodegen::new());
    let compiler = PipelineCompiler::new(backend);
    compiler.compile(src, FileId::from_raw(1), &[])
}

#[test]
fn top_level_const_referenced_by_path() {
    let src = r#"
        const ANSWER: i32 = 42;
        fn main() {
            let _ = ANSWER;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

#[test]
fn module_const_referenced_by_multi_segment_path() {
    let src = r#"
        mod config {
            const MAX: i32 = 100;
        }
        fn main() {
            let _ = config::MAX;
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);
}

/// Aggregate const materialization (unstub-2 plan Phase 7.1 + Part C): a tuple
/// or array constant must fold at MIR-lower into a concrete `MirConstKind::
/// Aggregate([...])`, NOT fall back to a `MirConstKind::ConstRef` zero-init
/// global. This exercises `PipelineLowerCtx::cv_const` for aggregate values.
#[test]
fn aggregate_const_folds_to_aggregate_mir_const() {
    let src = r#"
        const PAIR: (i32, i32) = (1, 2);
        fn use_pair(p: (i32, i32)) -> i32 { p.0 + p.1 }
        fn main() -> i32 {
            use_pair(PAIR)
        }
    "#;
    let output = compile(src);
    assert_no_errors(&output.diagnostics);

    // Count `Aggregate` and `ConstRef` constants across all lowered bodies.
    let mut aggregate_count = 0usize;
    let mut constref_count = 0usize;
    for body in &output.mir_bodies {
        for bb in body.basic_blocks.iter() {
            for stmt in &bb.statements {
                if let glyim_mir::StatementKind::Assign(_, rvalue) = &stmt.kind
                    && let glyim_mir::Rvalue::Use(glyim_mir::Operand::Constant(c)) = rvalue
                {
                    match c.kind {
                        glyim_mir::MirConstKind::Aggregate(_) => aggregate_count += 1,
                        glyim_mir::MirConstKind::ConstRef(..) => constref_count += 1,
                        _ => {}
                    }
                }
            }
            if let glyim_mir::TerminatorKind::Call { args, .. } = &bb.terminator.kind {
                for arg in args {
                    if let glyim_mir::Operand::Constant(c) = arg {
                        match c.kind {
                            glyim_mir::MirConstKind::Aggregate(_) => aggregate_count += 1,
                            glyim_mir::MirConstKind::ConstRef(..) => constref_count += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    assert!(
        aggregate_count >= 1,
        "expected at least 1 folded Aggregate constant (PAIR), found {}",
        aggregate_count
    );
    assert_eq!(
        constref_count, 0,
        "aggregate consts must fold to Aggregate, not fall back to ConstRef"
    );
}
