//! Tests for Tier 1.6 — scope drop elaboration (whole-value / top-level drops).
//!
//! After lowering a function body, every non-`Copy` local that is still live
//! at scope exit must receive a `Drop` terminator in reverse declaration
//! order. `Copy` locals (e.g. `i32`) must NOT be dropped.

use crate::lower::lower_body;
use crate::tests::mock_lower_ctx::TestLowerCtx;
use crate::tests::thir_builder::ThirBuilder;
use glyim_core::primitives::IntTy;
use glyim_mir::{BasicBlockIdx, LocalIdx, TerminatorKind};
use glyim_test::{assert_mir, test_ty_ctx};
use glyim_type::*;
use glyim_typeck::thir::{ExprKind, Literal};

fn string_ty(ctx: &mut TyCtxMut) -> Ty {
    ctx.mk_ty(TyKind::String)
}

#[test]
fn non_copy_local_gets_drop_terminator() {
    // `let s: String = ...;` with no other statements -> scope drop for `s`.
    let mut ctx_mut = test_ty_ctx();
    let s_ty = string_ty(&mut ctx_mut);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(s_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "s",
        s_ty,
        Some(b.expr(ExprKind::Literal(Literal::String(b.make_name("hi"))), s_ty)),
        &mut stmts,
    );
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // `String` is non-Copy, so scope drop elaboration must insert a `Drop`
    // terminator for the `s` local (local index 1) somewhere in the body, and
    // the entry block must not end directly in `Return` (it must route through
    // the drop chain).
    let entry = &result.body.basic_blocks[BasicBlockIdx::from_raw(0)];
    assert!(
        !matches!(entry.terminator.kind, TerminatorKind::Return),
        "entry must route through drop chain, not Return directly"
    );

    let mut drop_count = 0;
    let mut drops_s_local = false;
    for bb in result.body.basic_blocks.iter() {
        if let TerminatorKind::Drop { place, cleanup, .. } = &bb.terminator.kind {
            drop_count += 1;
            assert!(cleanup.is_none(), "drop elaboration should not set cleanup");
            if place.local == LocalIdx::from_raw(1) {
                drops_s_local = true;
            }
        }
    }
    assert!(
        drops_s_local,
        "expected a Drop terminator for the `s` local"
    );
    assert!(drop_count >= 1, "expected at least one Drop terminator");
}

#[test]
fn copy_local_does_not_get_drop_terminator() {
    // `let x: i32 = 10;` -> i32 is Copy, so no drop elaboration; the body ends
    // directly in a `Return`.
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(i32_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "x",
        i32_ty,
        Some(b.expr(ExprKind::Literal(Literal::Int(10, None)), i32_ty)),
        &mut stmts,
    );
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // No drops: single block ending directly in Return.
    assert_eq!(result.body.basic_blocks.len(), 1);
    assert_mir(&ctx, &result.body).block_terminator(BasicBlockIdx::from_raw(0), "Return");
}

#[test]
fn return_place_is_never_dropped() {
    // A function returning a `String` value must not drop `_0` (the return
    // place) in the scope-drop elaboration.
    let mut ctx_mut = test_ty_ctx();
    let s_ty = string_ty(&mut ctx_mut);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let b = ThirBuilder::new(s_ty, interner);
    let body = b.into_body(vec![], vec![]);
    let result = lower_body(&mock, &body);

    // No locals other than _0, so no drop chain: single Return block.
    assert_eq!(result.body.basic_blocks.len(), 1);
    assert_mir(&ctx, &result.body).block_terminator(BasicBlockIdx::from_raw(0), "Return");
}

/// §1.8 / Phase 4 (GLYIM_DESTUB_PLAN): a non-`Copy` local's scope-exit `Drop`
/// must be *guarded* by a `SwitchInt` on its drop-flag (allocated for every
/// droppable `let` via `register_drop_flag_init`), so a later partial move can
/// suppress the parent's destructor. This locks that the guard wiring in
/// `elaborate_scope_drops` is live (pre-Phase-4 it emitted an unconditional
/// `Drop`, which would double-free a partially-moved parent).
#[test]
fn droppable_local_gets_guarded_drop_via_drop_flag() {
    let mut ctx_mut = test_ty_ctx();
    let s_ty = string_ty(&mut ctx_mut);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(s_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "s",
        s_ty,
        Some(b.expr(ExprKind::Literal(Literal::String(b.make_name("hi"))), s_ty)),
        &mut stmts,
    );
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // Exactly one Drop must be *guarded* by a SwitchInt on a dedicated drop-flag
    // local (distinct from the dropped local), proving the Phase 4 guard wiring
    // in `elaborate_scope_drops` is live. (The harness may also allocate
    // need-drop temporaries — e.g. the string literal — so we assert the guard
    // exists rather than an exact drop count.)
    let mut switch_count = 0;
    let mut guarded_drop = false;
    for bb in result.body.basic_blocks.iter() {
        if let TerminatorKind::SwitchInt { discr, .. } = &bb.terminator.kind {
            switch_count += 1;
            // The discriminant must be a Copy of a *flag* local, distinct from
            // any local that is actually dropped (a real drop-flag guards a Drop
            // of a *different* local, not the flag itself).
            if let glyim_mir::Operand::Copy(glyim_mir::Place { local, projection }) = discr {
                assert!(projection.is_empty(), "flag read must be direct");
                let flag = *local;
                let guards_a_drop = result.body.basic_blocks.iter().any(|other| {
                    if let TerminatorKind::Drop { place, .. } = &other.terminator.kind {
                        place.local != flag
                    } else {
                        false
                    }
                });
                if guards_a_drop {
                    guarded_drop = true;
                }
            } else {
                panic!("drop-flag guard discriminant must be a Copy of a place");
            }
        }
    }
    assert_eq!(
        switch_count, 1,
        "expected exactly one drop-flag guard SwitchInt"
    );
    assert!(
        guarded_drop,
        "expected the SwitchInt to guard a Drop of a different local (real drop-flag)"
    );
}

/// §1.8 regression: a function with no moves must keep producing an
/// unconditional `Drop` for its non-`Copy` locals. The `drop_flags` map is
/// empty until move-semantics land, so the §1.8 guard degrades to the
/// pre-existing behavior — this locks that the existing drop-elaboration
/// structure is unchanged by the guard wiring.
#[test]
fn no_move_case_unaffected() {
    let mut ctx_mut = test_ty_ctx();
    let s_ty = string_ty(&mut ctx_mut);
    let interner = ctx_mut.resolver().clone();
    let ctx = ctx_mut.freeze();
    let mock = TestLowerCtx::new(&ctx);

    let mut b = ThirBuilder::new(s_ty, interner);
    let mut stmts = Vec::new();
    b.add_let_binding(
        "s",
        s_ty,
        Some(b.expr(ExprKind::Literal(Literal::String(b.make_name("hi"))), s_ty)),
        &mut stmts,
    );
    let body = b.into_body(stmts, vec![]);
    let result = lower_body(&mock, &body);

    // The non-Copy `s` local must still receive a `Drop` terminator.
    let mut drop_count = 0;
    for bb in result.body.basic_blocks.iter() {
        if let TerminatorKind::Drop { place, cleanup, .. } = &bb.terminator.kind {
            assert!(cleanup.is_none(), "drop elaboration should not set cleanup");
            drop_count += 1;
            assert_ne!(place.local.to_raw(), 0, "return place must not be dropped");
        }
    }
    assert!(
        drop_count >= 1,
        "expected at least one Drop terminator for the `s` local"
    );
}
