use glyim_core::def_id::{CrateId, DefId, FnDefId, LocalDefId};
use glyim_core::interner::{Interner, Name};
use glyim_core::primitives::{IntTy, Mutability, UintTy};
use glyim_span::Span;
use glyim_test::test_ty_ctx;
use glyim_type::*;
use glyim_typeck::thir::{self, Expr, ExprKind, Literal, Param, Pattern, PatternKind, Stmt};

use crate::lower::{IteratorNextInfo, lower_body};
use crate::tests::support::MockLowerCtx;

fn name(s: &str) -> Name {
    Interner::default().intern(s)
}

fn create_for_loop_body(
    iter_ty: Ty,
    elem_ty: Ty,
    iterable_var: thir::LocalVarId,
    span: Span,
    next: Option<thir::ForIteratorNext>,
) -> thir::Body {
    let pat = Pattern {
        ty: elem_ty,
        kind: PatternKind::Binding {
            var_id: thir::LocalVarId::from_raw(0),
            name: name("x"),
            mutability: Mutability::Not,
            subpattern: None,
        },
        span,
    };
    let iterable = Expr {
        ty: iter_ty,
        kind: ExprKind::VarRef(iterable_var),
        span,
    };
    let body_expr = Expr {
        ty: Ty::UNIT,
        kind: ExprKind::Literal(Literal::Unit),
        span,
    };
    let for_expr = Expr {
        ty: Ty::UNIT,
        kind: ExprKind::For {
            pat: Box::new(pat),
            iterable: Box::new(iterable),
            body: Box::new(body_expr),
            next,
        },
        span,
    };
    let stmt = Stmt::Expr { expr: for_expr };
    thir::Body {
        owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(1)),
        params: vec![Param {
            name: name("iter_var"),
            pat: Pattern {
                ty: iter_ty,
                kind: PatternKind::Binding {
                    var_id: thir::LocalVarId::from_raw(0),
                    name: name("iter_var"),
                    mutability: Mutability::Not,
                    subpattern: None,
                },
                span,
            },
            ty: iter_ty,
            span,
            local: thir::LocalVarId::from_raw(0),
        }],
        stmts: vec![stmt],
        return_ty: Ty::UNIT,
        span,
    }
}

#[test]
fn for_loop_desugaring_uses_iterator_next() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let vec_adt = glyim_core::def_id::AdtId::from_raw(100);
    let option_adt = glyim_core::def_id::AdtId::from_raw(101);
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let iter_ty = ctx_mut.mk_ty(TyKind::Adt(vec_adt, subst));
    let option_subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let option_ty = ctx_mut.mk_ty(TyKind::Adt(option_adt, option_subst));
    let ref_iter_ty = ctx_mut.mk_ref(Region::Erased, iter_ty, Mutability::Mut);
    let discr_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U8));
    let next_fn_id = FnDefId::from_raw(200);
    let next_substs = ctx_mut.intern_substitution(Vec::new());
    let next_fn_ty = ctx_mut.mk_ty(TyKind::FnDef(next_fn_id, next_substs));
    let iter_info = IteratorNextInfo {
        fn_def_id: next_fn_id,
        fn_substs: next_substs,
        fn_ty: next_fn_ty,
        option_ty,
        discr_ty,
        ref_iter_ty,
    };
    let thir_body = create_for_loop_body(
        iter_ty,
        i32_ty,
        thir::LocalVarId::from_raw(0),
        Span::DUMMY,
        None,
    );
    let frozen_ctx = ctx_mut.freeze();
    let mock_ctx =
        MockLowerCtx::new(&frozen_ctx).with_iterator_next(move |_, _| Some(iter_info.clone()));
    let result = lower_body(&mock_ctx, &thir_body);
    assert!(
        result.diagnostics.is_empty(),
        "Lowering produced diagnostics: {:?}",
        result.diagnostics
    );
    let body = &result.body;
    assert!(
        body.basic_blocks.len() >= 4,
        "Expected at least 4 blocks, got {}",
        body.basic_blocks.len()
    );
    let next_fn_id = FnDefId::from_raw(200);
    let mut found_next_call = false;
    for block in body.basic_blocks.iter() {
        if let glyim_mir::TerminatorKind::Call { func, .. } = &block.terminator.kind
            && let glyim_mir::Operand::Constant(c) = func
            && let glyim_mir::MirConstKind::Fn(id, _) = c.kind
            && id == next_fn_id
        {
            found_next_call = true;
            break;
        }
    }
    assert!(
        found_next_call,
        "MIR does not contain call to Iterator::next"
    );
}

#[test]
fn for_loop_fallback_when_no_iterator_info() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let iter_ty = i32_ty;
    let thir_body = create_for_loop_body(
        iter_ty,
        i32_ty,
        thir::LocalVarId::from_raw(0),
        Span::DUMMY,
        None,
    );
    let frozen_ctx = ctx_mut.freeze();
    let mock_ctx = MockLowerCtx::new(&frozen_ctx);
    let result = lower_body(&mock_ctx, &thir_body);
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        result.body.basic_blocks.len(),
        3,
        "Fallback should have 3 blocks: entry, loop, exit"
    );
}

/// Phase 1 (GLYIM_DESTUB_PLAN): the resolved `Iterator::next` is threaded from
/// typeck into the THIR `For` node (the production mechanism). This test proves
/// that lowering takes the real multi-iteration path purely from `For.next`,
/// with NO help from `MockLowerCtx::iterator_next_fn` (which is left at its
/// default `None` here). This is the exact path `PipelineLowerCtx` uses in a
/// real build, where `for x in v.iter()` resolves `Iterator::next` during
/// typeck and the loop runs to completion instead of once-and-break.
#[test]
fn for_loop_desugaring_uses_thir_next_without_ctx_override() {
    let mut ctx_mut = test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(IntTy::I32));
    let vec_adt = glyim_core::def_id::AdtId::from_raw(100);
    let option_adt = glyim_core::def_id::AdtId::from_raw(101);
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let iter_ty = ctx_mut.mk_ty(TyKind::Adt(vec_adt, subst));
    let option_subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let option_ty = ctx_mut.mk_ty(TyKind::Adt(option_adt, option_subst));
    let ref_iter_ty = ctx_mut.mk_ref(Region::Erased, iter_ty, Mutability::Mut);
    let discr_ty = ctx_mut.mk_ty(TyKind::Uint(UintTy::U8));
    let next_fn_id = FnDefId::from_raw(200);
    let next_substs = ctx_mut.intern_substitution(Vec::new());
    let next_fn_ty = ctx_mut.mk_ty(TyKind::FnDef(next_fn_id, next_substs));
    let thir_next = thir::ForIteratorNext {
        fn_def_id: next_fn_id,
        fn_substs: next_substs,
        option_ty,
        discr_ty,
        ref_iter_ty,
        fn_ty: next_fn_ty,
    };
    let thir_body = create_for_loop_body(
        iter_ty,
        i32_ty,
        thir::LocalVarId::from_raw(0),
        Span::DUMMY,
        Some(thir_next),
    );
    let frozen_ctx = ctx_mut.freeze();
    // No `with_iterator_next` override: `MockLowerCtx::iterator_next_fn` returns
    // `None`. The real path must come entirely from the THIR `For.next` field.
    let mock_ctx = MockLowerCtx::new(&frozen_ctx);
    let result = lower_body(&mock_ctx, &thir_body);
    assert!(
        result.diagnostics.is_empty(),
        "Lowering produced diagnostics: {:?}",
        result.diagnostics
    );
    let body = &result.body;
    assert!(
        body.basic_blocks.len() >= 4,
        "Expected at least 4 blocks (real loop), got {}",
        body.basic_blocks.len()
    );
    let mut found_next_call = false;
    for block in body.basic_blocks.iter() {
        if let glyim_mir::TerminatorKind::Call { func, .. } = &block.terminator.kind
            && let glyim_mir::Operand::Constant(c) = func
            && let glyim_mir::MirConstKind::Fn(id, _) = c.kind
            && id == next_fn_id
        {
            found_next_call = true;
            break;
        }
    }
    assert!(
        found_next_call,
        "MIR does not contain call to Iterator::next (THIR-plumbing path failed)"
    );
}

// Note: The real TraitSolver implementation (SimpleTraitSolver) now has a non-stub
// iterator_next_info that uses registered trait methods. To test it, register the Iterator
// trait with a suitable next_def_id and use SimpleTraitSolver in the test.
