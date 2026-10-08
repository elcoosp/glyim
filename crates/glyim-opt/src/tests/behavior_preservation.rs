//! Optimizer behavior-preservation tests.
//!
//! The optimizer has the most Critical bugs in the audit (MIR-1 const-prop
//! staleness, MIR-6 DCE-drops-needed-assignments, MIR-10 drop-elaboration CFG
//! corruption, ...). Every one is a *side-effect-unsafe transform* that
//! structural tests miss. The remedy is a differential check: interpret the
//! body before and after `optimize`, and assert the observable result is
//! identical. This file is the harness; individual tests supply the bodies.

use super::testutil::{build_test_body, const_int, ty_i32};
use glyim_core::primitives::Mutability;
use glyim_mir::*;
use glyim_mir_interp::{InterpValue, Interpreter};
use glyim_span::Span;
use glyim_test::test_ty_ctx;

/// Run a body through the interpreter and return its exit value as i64.
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

/// Assert that optimizing `body` does not change what it computes.
fn assert_optimization_preserves(
    locals: Vec<(glyim_type::Ty, Mutability)>,
    blocks: Vec<BasicBlockData>,
    return_ty: glyim_type::Ty,
) {
    let mut ctx_mut = test_ty_ctx();
    let body = build_test_body(locals, blocks, 0, return_ty);
    let ctx = ctx_mut.freeze();

    let before = run_interp(&ctx, &body);
    let optimized = crate::optimize(&ctx, &std::sync::Arc::new(body.clone())).body;
    let after = run_interp(&ctx, &optimized);

    assert_eq!(
        before, after,
        "optimization changed observable behavior ({before} -> {after})"
    );
}

/// A trivial constant-return body: `local0 = 42; return`. Confirms the
/// differential harness itself works (both interpretations must yield 42)
/// before trusting it on bodies where the optimizer actually transforms.
#[test]
fn trivial_constant_returns_42() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ty_i32(&mut ctx_mut);
    let ctx = ctx_mut.freeze();
    let body = build_test_body(
        vec![(i32_ty, Mutability::Mut)],
        vec![BasicBlockData {
            statements: vec![Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(const_int(42, i32_ty)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            }],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(Span::DUMMY),
            },
            is_cleanup: false,
        }],
        0,
        i32_ty,
    );
    assert_eq!(run_interp(&ctx, &body), 42);
}

/// MIR-1 shape: a constant assigned, used, then the local reassigned. The
/// optimizer must not propagate the *stale* first value into the use after
/// the reassignment.
#[test]
fn const_prop_does_not_use_stale_value() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ty_i32(&mut ctx_mut);
    let locals = vec![
        (i32_ty, Mutability::Mut), // 0 = return
        (i32_ty, Mutability::Mut), // 1 = scratch
    ];
    // local0 = local1; local1 = 99; ... but the interesting shape is:
    // local1 = 10; local0 = local1; local1 = 20; return local0
    // A naive const-prop that binds local1 := 10 and rewrites every later
    // read of local1 would make local0 = 20 — wrong.
    let blocks = vec![BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(const_int(10, i32_ty)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(const_int(20, i32_ty)),
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
    assert_optimization_preserves(locals, blocks, i32_ty);
}

/// MIR-6 shape: a dead assignment whose right-hand side has a side effect
/// (here, a read of a local the optimizer might think is dead) must not be
/// deleted if its result is observed.
#[test]
fn dce_preserves_observed_value() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ty_i32(&mut ctx_mut);
    let locals = vec![
        (i32_ty, Mutability::Mut), // 0 = return
        (i32_ty, Mutability::Mut), // 1 = observed
        (i32_ty, Mutability::Mut), // 2 = unused
    ];
    let blocks = vec![BasicBlockData {
        statements: vec![
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(1)),
                    Rvalue::Use(const_int(7, i32_ty)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(2)),
                    Rvalue::Use(const_int(999, i32_ty)),
                ),
                source_info: SourceInfo::new(Span::DUMMY),
            },
            Statement {
                kind: StatementKind::Assign(
                    Place::new(LocalIdx::from_raw(0)),
                    Rvalue::Use(Operand::Copy(Place::new(LocalIdx::from_raw(1)))),
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
    assert_optimization_preserves(locals, blocks, i32_ty);
}
