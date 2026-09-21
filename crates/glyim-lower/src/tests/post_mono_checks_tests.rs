use glyim_core::arena::IndexVec;
use glyim_core::def_id::{CrateId, DefId, FnDefId, LocalDefId};
use glyim_core::primitives::Mutability;
use glyim_diag::DiagSeverity;
use glyim_mir::{Body, LocalDecl, SourceInfo, Terminator, TerminatorKind};
use glyim_span::Span;
use glyim_test::{
    assert_diag_contains, assert_error_count, assert_has_errors, assert_has_severity,
    assert_no_errors,
};
use glyim_type::{GenericArg, Region, Substitution, Ty, TyKind};
use std::sync::Arc;

use crate::mono::{MonoItem, MonoItemData};
use crate::post_mono_checks::{
    check_large_mono_set, check_unsized_locals, check_unused_generic_params,
};

/// Helper to create a minimal MIR body with given locals and a single return block.
fn make_body(owner_def_id: DefId, return_ty: Ty, locals_ty: Vec<Ty>) -> Body {
    let mut locals = IndexVec::new();
    // _0 is return place
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Mut,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    for ty in locals_ty {
        locals.push(LocalDecl {
            ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(Span::DUMMY),
        });
    }

    let mut basic_blocks = IndexVec::new();
    let return_bb = basic_blocks.push(glyim_mir::BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });
    let _entry_bb = basic_blocks.push(glyim_mir::BasicBlockData {
        statements: vec![],
        terminator: Terminator {
            kind: TerminatorKind::Goto { target: return_bb },
            source_info: SourceInfo::new(Span::DUMMY),
        },
        is_cleanup: false,
    });

    Body {
        owner: owner_def_id,
        basic_blocks,
        locals,
        arg_count: 0,
        return_ty,
        span: Span::DUMMY,
        var_debug_info: vec![],
    }
}

fn make_fn_def_id(raw: u32) -> FnDefId {
    FnDefId::from_raw(raw)
}

fn make_def_id(raw: u32) -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(raw))
}

/// Create a MonoItemData for a function with the given substitution.
fn make_mono_fn_with_subst(def_id_raw: u32, subst: Substitution, body: Body) -> MonoItemData {
    MonoItemData {
        item: MonoItem::Fn {
            def_id: make_fn_def_id(def_id_raw),
            substs: subst,
        },
        body: Arc::new(body),
        symbol: format!("fn_{}", def_id_raw),
        source_module: 0,
    }
}

// ==================== V26-T02: Unsized locals ====================

#[test]
fn unsized_local_errors() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(Ty::UNIT));
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(1), Ty::UNIT, vec![slice_ty]);
    let item = make_mono_fn_with_subst(1, Substitution::empty(), body);
    let items = vec![item];

    let diags = check_unsized_locals(&items, &frozen);
    assert_has_errors(&diags);
    assert_error_count(&diags, 1);
    assert_diag_contains(&diags, "unsized local");
}

// ==================== V26-T03: Large mono item set ====================

#[test]
fn large_mono_set_warns() {
    let body = make_body(make_def_id(0), Ty::UNIT, vec![]);
    let mut items = Vec::new();
    for i in 0..50 {
        items.push(make_mono_fn_with_subst(
            i,
            Substitution::empty(),
            body.clone(),
        ));
    }
    let threshold = 30;
    let diags = check_large_mono_set(&items, threshold);
    assert_has_severity(&diags, DiagSeverity::Warning);
    assert_diag_contains(&diags, "large number of mono items");
}

#[test]
fn small_mono_set_no_warn() {
    let body = make_body(make_def_id(0), Ty::UNIT, vec![]);
    let items: Vec<MonoItemData> = (0..5)
        .map(|i| make_mono_fn_with_subst(i, Substitution::empty(), body.clone()))
        .collect();
    let diags = check_large_mono_set(&items, 100);
    assert_no_errors(&diags);
}

// ==================== V26-T01: Unused generic params ====================

#[test]
fn unused_generic_param_warns() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(Ty::UNIT)]);
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(10), Ty::UNIT, vec![]);
    let item = make_mono_fn_with_subst(10, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_has_severity(&diags, DiagSeverity::Warning);
    assert_diag_contains(&diags, "unused generic parameter");
}

#[test]
fn no_unused_when_body_uses_generic() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let name = ctx_mut.resolver().intern("T");
    let used_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy { index: 0, name }));
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(used_ty)]);
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(11), Ty::UNIT, vec![used_ty]);
    let item = make_mono_fn_with_subst(11, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

// ==================== Extended V26-T02: Unsized locals ====================

#[test]
fn unsized_dynamic_errors() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    // Create a Dynamic (trait object) type
    let predicates: Vec<glyim_type::Predicate> = vec![glyim_type::Predicate::WellFormed(Ty::UNIT)];
    let binder = glyim_type::Binder::bind(predicates.into_boxed_slice(), vec![].into_boxed_slice());
    let dyn_ty = ctx_mut.mk_ty(TyKind::Dynamic(binder, Region::Erased));
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(2), Ty::UNIT, vec![dyn_ty]);
    let item = make_mono_fn_with_subst(2, Substitution::empty(), body);
    let items = vec![item];

    let diags = check_unsized_locals(&items, &frozen);
    assert_has_errors(&diags);
    assert_error_count(&diags, 1);
    assert_diag_contains(&diags, "unsized local");
}

#[test]
fn multiple_unsized_locals_multiple_errors() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let slice_ty1 = ctx_mut.mk_ty(TyKind::Slice(Ty::UNIT));
    let slice_ty2 = ctx_mut.mk_ty(TyKind::Slice(Ty::BOOL));
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(3), Ty::UNIT, vec![slice_ty1, slice_ty2]);
    let item = make_mono_fn_with_subst(3, Substitution::empty(), body);
    let items = vec![item];

    let diags = check_unsized_locals(&items, &frozen);
    assert_has_errors(&diags);
    assert_error_count(&diags, 2);
}

#[test]
fn sized_locals_no_error() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let bool_ty = Ty::BOOL;
    let unit_ty = Ty::UNIT;
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(4), Ty::UNIT, vec![i32_ty, bool_ty, unit_ty]);
    let item = make_mono_fn_with_subst(4, Substitution::empty(), body);
    let items = vec![item];

    let diags = check_unsized_locals(&items, &frozen);
    assert_no_errors(&diags);
}

// ==================== Extended V26-T03: Large mono item set ====================

#[test]
fn large_mono_set_exactly_at_threshold_no_warn() {
    let body = make_body(make_def_id(0), Ty::UNIT, vec![]);
    let items: Vec<MonoItemData> = (0..30)
        .map(|i| make_mono_fn_with_subst(i, Substitution::empty(), body.clone()))
        .collect();
    let threshold = 30;
    let diags = check_large_mono_set(&items, threshold);
    assert_no_errors(&diags);
}

#[test]
fn large_mono_set_one_above_threshold_warns() {
    let body = make_body(make_def_id(0), Ty::UNIT, vec![]);
    let items: Vec<MonoItemData> = (0..31)
        .map(|i| make_mono_fn_with_subst(i, Substitution::empty(), body.clone()))
        .collect();
    let threshold = 30;
    let diags = check_large_mono_set(&items, threshold);
    assert_has_severity(&diags, DiagSeverity::Warning);
}

#[test]
fn empty_mono_set_no_warn() {
    let items: Vec<MonoItemData> = vec![];
    let diags = check_large_mono_set(&items, 1);
    assert_no_errors(&diags);
}

// ==================== Extended V26-T01: Unused generic params ====================

#[test]
fn unused_generic_param_multiple_substs_warns() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let subst =
        ctx_mut.intern_substitution(vec![GenericArg::Ty(Ty::UNIT), GenericArg::Ty(Ty::BOOL)]);
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(12), Ty::UNIT, vec![]);
    let item = make_mono_fn_with_subst(12, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_has_severity(&diags, DiagSeverity::Warning);
    assert_diag_contains(&diags, "unused generic parameter");
}

#[test]
fn empty_substitution_no_warn() {
    let ctx_mut = glyim_test::test_ty_ctx();
    let subst = Substitution::empty();
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(13), Ty::UNIT, vec![]);
    let item = make_mono_fn_with_subst(13, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

#[test]
fn generic_param_used_in_rvalue_constant_no_warn() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let param_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: ctx_mut.resolver().intern("T"),
    }));
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let frozen = ctx_mut.freeze();

    // Body uses the param type in a local
    let body = make_body(make_def_id(14), Ty::UNIT, vec![param_ty]);
    let item = make_mono_fn_with_subst(14, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

// ==================== Edge case: unsized return type ====================

#[test]
fn unsized_return_errors() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let slice_ret = ctx_mut.mk_ty(TyKind::Slice(Ty::UNIT));
    let frozen = ctx_mut.freeze();

    // return_ty = slice_ret, placed in local _0
    let body = make_body(make_def_id(5), slice_ret, vec![]);
    let item = make_mono_fn_with_subst(5, Substitution::empty(), body);
    let items = vec![item];

    let diags = check_unsized_locals(&items, &frozen);
    assert_has_errors(&diags);
    assert_error_count(&diags, 1);
    assert_diag_contains(&diags, "unsized local");
}

// ==================== Edge case: large set with very high threshold ====================

#[test]
fn large_set_below_high_threshold_no_warn() {
    let body = make_body(make_def_id(0), Ty::UNIT, vec![]);
    let items: Vec<MonoItemData> = (0..500)
        .map(|i| make_mono_fn_with_subst(i, Substitution::empty(), body.clone()))
        .collect();
    let diags = check_large_mono_set(&items, 1000);
    assert_no_errors(&diags);
}

// ==================== Edge case: Const items (not Fn) ignored ====================

#[test]
fn const_item_ignored_for_unused_params() {
    use crate::mono::MonoItem;
    use glyim_core::def_id::ConstDefId;
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let param_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: ctx_mut.resolver().intern("T"),
    }));
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(15), Ty::UNIT, vec![]);
    let item = MonoItemData {
        item: MonoItem::Const {
            def_id: ConstDefId::from_raw(15),
            substs: subst,
        },
        body: Arc::new(body),
        symbol: "const_15".to_string(),
        source_module: 0,
    };
    let items = vec![item];

    // Const items should not be checked for unused generic params
    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

// ==================== Edge case: unaligned threshold values ====================

#[test]
fn threshold_zero_warns_for_any_items() {
    let body = make_body(make_def_id(0), Ty::UNIT, vec![]);
    let items = vec![make_mono_fn_with_subst(0, Substitution::empty(), body)];
    let diags = check_large_mono_set(&items, 0);
    assert_has_severity(&diags, DiagSeverity::Warning);
}

// ==================== Unused generic param in Ref local ====================

#[test]
fn generic_param_used_in_ref_local_no_warn() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let name = ctx_mut.resolver().intern("T");
    let param_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy { index: 0, name }));
    let ref_ty = ctx_mut.mk_ref(
        Region::Erased,
        param_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(16), Ty::UNIT, vec![ref_ty]);
    let item = make_mono_fn_with_subst(16, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

// ==================== Unused generic param in Tuple local ====================

#[test]
fn generic_param_used_in_tuple_local_no_warn() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let name = ctx_mut.resolver().intern("T");
    let param_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy { index: 0, name }));
    let tuple_subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let tuple_ty = ctx_mut.mk_ty(TyKind::Tuple(tuple_subst));
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(17), Ty::UNIT, vec![tuple_ty]);
    let item = make_mono_fn_with_subst(17, subst, body);
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

// ==================== Static item ignored for unused params ====================

#[test]
fn static_item_ignored_for_unused_params() {
    use crate::mono::MonoItem;
    use glyim_core::def_id::StaticDefId;
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let _param_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: ctx_mut.resolver().intern("T"),
    }));
    let frozen = ctx_mut.freeze(); // not needed, but consistent

    let body = make_body(make_def_id(18), Ty::UNIT, vec![]);
    let item = MonoItemData {
        item: MonoItem::Static {
            def_id: StaticDefId::from_raw(18),
        },
        body: Arc::new(body),
        symbol: "static_18".to_string(),
        source_module: 0,
    };
    let items = vec![item];

    let diags = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags);
}

// ==================== Multiple functions with mixed unsized/sized ====================

#[test]
fn multiple_functions_mixed_unsized() {
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(Ty::UNIT));
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let frozen = ctx_mut.freeze();

    let body_unsized = make_body(make_def_id(19), Ty::UNIT, vec![slice_ty]);
    let body_sized = make_body(make_def_id(20), Ty::UNIT, vec![i32_ty]);

    let items = vec![
        make_mono_fn_with_subst(19, Substitution::empty(), body_unsized),
        make_mono_fn_with_subst(20, Substitution::empty(), body_sized),
    ];

    let diags = check_unsized_locals(&items, &frozen);
    assert_has_errors(&diags);
    assert_error_count(&diags, 1); // only the unsized one
}

// ==================== Combined checks integration (optional convenience) ====================

#[test]
fn combined_post_mono_checks() {
    use crate::post_mono_checks::{
        check_large_mono_set, check_unsized_locals, check_unused_generic_params,
    };
    let mut ctx_mut = glyim_test::test_ty_ctx();
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(Ty::UNIT));
    let param_ty = ctx_mut.mk_ty(TyKind::Param(glyim_type::ParamTy {
        index: 0,
        name: ctx_mut.resolver().intern("T"),
    }));
    let ref_ty = ctx_mut.mk_ref(
        Region::Erased,
        param_ty,
        glyim_core::primitives::Mutability::Not,
    );
    let subst = ctx_mut.intern_substitution(vec![GenericArg::Ty(param_ty)]);
    let frozen = ctx_mut.freeze();

    let body_unsized = make_body(make_def_id(21), Ty::UNIT, vec![slice_ty]);
    let body_used_param = make_body(make_def_id(22), Ty::UNIT, vec![ref_ty]);
    let body_empty = make_body(make_def_id(23), Ty::UNIT, vec![]);

    let items = vec![
        make_mono_fn_with_subst(21, Substitution::empty(), body_unsized),
        make_mono_fn_with_subst(22, subst, body_used_param), // param used
        make_mono_fn_with_subst(23, Substitution::empty(), body_empty),
    ];

    let diags_unsized = check_unsized_locals(&items, &frozen);
    assert_has_errors(&diags_unsized);
    assert_error_count(&diags_unsized, 1);

    let diags_large = check_large_mono_set(&items, 2);
    assert_has_severity(&diags_large, DiagSeverity::Warning);

    let diags_unused = check_unused_generic_params(&items, None, &frozen);
    assert_no_errors(&diags_unused);
}

// ==================== §8.12: checks wired through MonoCtx::collect ====================
// Proves the post-monomorphization checks run on the *collected* mono items
// (the exact path the pipeline uses after `mono_ctx.collect(...)`), not just on
// hand-built `Vec<MonoItemData>`.

#[test]
fn collect_then_check_unsized_local_fires() {
    use crate::mono::MonoCtx;
    use glyim_core::def_id::{CrateId, DefId, LocalDefId};
    use glyim_type::{Ty, TyKind};

    let mut ctx_mut = glyim_test::test_ty_ctx();
    let slice_ty = ctx_mut.mk_ty(TyKind::Slice(Ty::UNIT));
    let frozen = ctx_mut.freeze();

    // A function body whose local _1 is an unsized slice.
    let body = make_body(make_def_id(90), Ty::UNIT, vec![slice_ty]);
    let def_id = make_fn_def_id(90);

    let mir_bodies = |d: DefId, _s: &glyim_type::Substitution| -> Arc<glyim_mir::Body> {
        assert_eq!(
            d,
            DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(90))
        );
        Arc::new(body.clone())
    };
    let drop_glue =
        |_t: Ty| -> Arc<glyim_mir::Body> { Arc::new(make_body(make_def_id(0), Ty::UNIT, vec![])) };

    let root = crate::mono::MonoItem::Fn {
        def_id,
        substs: glyim_type::Substitution::empty(),
    };

    let mut mono_ctx = MonoCtx::new();
    mono_ctx.collect(std::slice::from_ref(&root), &mir_bodies, &drop_glue);

    // This mirrors `glyim-pipeline/src/lib.rs`'s post-mono check block.
    let diags = check_unsized_locals(mono_ctx.items(), &frozen);
    assert_has_errors(&diags);
    assert_diag_contains(&diags, "unsized local");
}

#[test]
fn collect_then_check_sized_local_clean() {
    use crate::mono::MonoCtx;
    use glyim_core::def_id::{CrateId, DefId, LocalDefId};
    use glyim_type::{Ty, TyKind};

    let mut ctx_mut = glyim_test::test_ty_ctx();
    let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
    let frozen = ctx_mut.freeze();

    let body = make_body(make_def_id(91), Ty::UNIT, vec![i32_ty]);
    let def_id = make_fn_def_id(91);
    let mir_bodies = |d: DefId, _s: &glyim_type::Substitution| -> Arc<glyim_mir::Body> {
        assert_eq!(
            d,
            DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(91))
        );
        Arc::new(body.clone())
    };
    let drop_glue =
        |_t: Ty| -> Arc<glyim_mir::Body> { Arc::new(make_body(make_def_id(0), Ty::UNIT, vec![])) };

    let root = crate::mono::MonoItem::Fn {
        def_id,
        substs: glyim_type::Substitution::empty(),
    };
    let mut mono_ctx = MonoCtx::new();
    mono_ctx.collect(std::slice::from_ref(&root), &mir_bodies, &drop_glue);

    let diags = check_unsized_locals(mono_ctx.items(), &frozen);
    assert_no_errors(&diags);
}
