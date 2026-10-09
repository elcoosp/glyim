//! DCE must not delete a store to a local that is only read *through a
//! projection*.
//!
//! Bug: `collect_operand_uses` marked `place.local` used but ignored the
//! projection list. So for `arr[i]` (MIR `arr[Deref, Index(i)]`) only `arr`
//! was marked used, `i` was not, and DCE deleted `i = 1` — the later index
//! read then hit an uninitialized local (`index local N not initialized`).
//! Observed via `s.as_bytes()[0]`, which made all byte-indexed str/array code
//! panic under the pipeline that runs `optimize()`.

use super::testutil::{build_test_body, const_int, ty_i32};
use glyim_core::primitives::Mutability;
use glyim_mir::*;
use glyim_mir_interp::{InterpValue, Interpreter};
use glyim_span::Span;
use glyim_test::test_ty_ctx;

fn run_interp(ctx: &glyim_type::TyCtx, body: &Body) -> i64 {
    let mut interp = Interpreter::new(ctx);
    interp.run_body(body).expect("interpreter must run clean");
    match interp.get_return_value() {
        Some(InterpValue::Int(n)) => n as i64,
        Some(InterpValue::Uint(n)) => n as i64,
        Some(InterpValue::Bool(b)) => b as i64,
        _ => 0,
    }
}

/// `local2 = 5; local0 = local2;` — the index local (`local2`) is only ever
/// *read* via a projection (`local1[Index(local2)]`), never as a bare place.
/// DCE must keep its store.
#[test]
fn dce_keeps_local_read_only_through_index_projection() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ty_i32(&mut ctx_mut);
    let ctx = ctx_mut.freeze();

    // locals: 0 = return, 1 = array (as an aggregate of 3), 2 = index.
    let arr_elem = |n: i128| {
        Operand::Constant(MirConst {
            kind: MirConstKind::Int(n),
            ty: i32_ty,
            span: Span::DUMMY,
        })
    };
    let blocks = vec![BasicBlockData {
        statements: vec![
            // local1 = [10, 20, 30]
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Aggregate(
                        AggregateKind::Tuple,
                        vec![arr_elem(10), arr_elem(20), arr_elem(30)],
                    ),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            // local2 = 1   <-- DCE used to delete this
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(const_int(1, i32_ty)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            // local0 = local1[Index(local2)]
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Copy(Place {
                        local: LocalIdx::from_raw(1),
                        projection: vec![ProjectionElem::Index(LocalIdx::from_raw(2))]
                            .into_boxed_slice(),
                    })),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
        ],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    }];

    let body = build_test_body(
        vec![
            (i32_ty, Mutability::Mut),
            (i32_ty, Mutability::Mut),
            (i32_ty, Mutability::Mut),
        ],
        blocks,
        0,
        i32_ty,
    );

    // Sanity: unoptimized gives 20.
    assert_eq!(run_interp(&ctx, &body), 20, "unoptimized");

    // Optimized must give the same — DCE must NOT have removed `local2 = 1`.
    let optimized = crate::optimize(&ctx, &std::sync::Arc::new(body.clone())).body;
    assert_eq!(
        run_interp(&ctx, &optimized),
        20,
        "optimize() broke an index read — DCE removed the index local's store"
    );
}
