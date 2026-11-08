use glyim_core::arena::IndexVec;
use glyim_core::def_id::{ConstDefId, CrateId, DefId, FnDefId, LocalDefId, StaticDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::{Abi, IntTy, Mutability, Safety, UintTy};
use glyim_core::{AdtId, ClosureId, OpaqueTyId};
use glyim_mir::*;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_test::test_ty_ctx;
use glyim_type::*;
use std::sync::Arc;

use crate::mono::{MonoItem, MonoItemData};
use crate::polymorphize::*;

/// Helper: intern a Name from a string via the `TyCtxMut` resolver.
fn intern_name(ctx: &mut TyCtxMut, s: &str) -> Name {
    ctx.resolver().intern(s)
}

fn dummy_span() -> Span {
    Span::new(
        FileId::BOGUS,
        ByteIdx::ZERO,
        ByteIdx::ZERO,
        SyntaxContext::ROOT,
    )
}

fn dummy_source_info() -> SourceInfo {
    SourceInfo::new(dummy_span())
}

fn dummy_def_id() -> DefId {
    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0))
}

/// Build a simple MIR body with the given local types.
/// Local 0 is the return place. The first `arg_count` locals
/// after the return place are arguments.
fn build_body(local_tys: Vec<Ty>, arg_count: usize) -> Body {
    let owner = dummy_def_id();
    let return_ty = local_tys.first().copied().unwrap_or(Ty::UNIT);

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    for ty in local_tys {
        locals.push(LocalDecl {
            ty,
            mutability: Mutability::Not,
            source_info: dummy_source_info(),
        });
    }

    let entry = BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    };

    let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    blocks.push(entry);

    Body {
        owner,
        basic_blocks: blocks,
        locals,
        arg_count,
        return_ty,
        span: dummy_span(),
        var_debug_info: Vec::new(),
    }
}

/// Build a body that uses a parameter type in a call operand.
fn build_body_with_fn_call(
    callee_def_id: FnDefId,
    callee_substs: Substitution,
    return_ty: Ty,
) -> Body {
    let owner = dummy_def_id();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: return_ty,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });
    let dest_local = LocalIdx::from_raw(0);

    let func_const = MirConst {
        kind: MirConstKind::Fn(callee_def_id, callee_substs),
        ty: Ty::UNIT,
        span: dummy_span(),
    };
    let func_op = Operand::Constant(func_const);

    let next_bb = BasicBlockIdx::from_raw(1);
    let term = Terminator {
        kind: TerminatorKind::Call {
            func: func_op,
            args: Vec::new(),
            destination: Place::new(dest_local),
            target: Some(next_bb),
            cleanup: None,
        },
        source_info: dummy_source_info(),
    };

    let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: term,
        is_cleanup: false,
    });
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks: blocks,
        locals,
        arg_count: 0,
        return_ty,
        span: dummy_span(),
        var_debug_info: Vec::new(),
    }
}

/// Build a body that contains a local of a parameter type.
fn build_body_with_local_of_type(local_ty: Ty) -> Body {
    let owner = dummy_def_id();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });
    locals.push(LocalDecl {
        ty: local_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    });

    Body {
        owner,
        basic_blocks: blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: dummy_span(),
        var_debug_info: Vec::new(),
    }
}

// ============================================================
// V31-T01: fn foo<T>(x: i32) where T unused -> not monomorphized over T
// ============================================================

#[test]
fn t01_unused_type_param_not_monomorphized() {
    let mut ctx = test_ty_ctx();

    // Build a body where T (ParamTy index 0) does NOT appear
    // fn foo<T>(x: i32) -> i32 { x }
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    // Substitution has one type parameter T
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);

    let fn_def_id = FnDefId::from_raw(1);
    let item = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    // Analyze which params are used
    let used = analyze_used_params(&body, &ctx, substs);
    assert_eq!(used.len(), 1, "should have one param slot");
    assert!(!used[0], "T is unused in the body, should be marked false");

    // Compute polymorphized item
    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    match poly_item {
        MonoItem::Fn {
            substs: poly_substs,
            ..
        } => {
            let args = ctx.substitution_args(poly_substs);
            assert_eq!(
                args.len(),
                1,
                "polymorphized substs should still have 1 arg"
            );
            if let GenericArg::Ty(ty) = args[0] {
                assert!(
                    matches!(ctx.ty_kind(ty), TyKind::Unit),
                    "unused param T should be replaced with unit type, got {}",
                    PrintTy::new(ty, &ctx)
                );
            } else {
                panic!("expected Ty generic arg");
            }
        }
        _ => panic!("expected Fn mono item"),
    }

    // Now test deduplication: two different instantiations of the same unused T
    // should deduplicate to one item
    let bool_ty = ctx.bool_ty();
    let substs_i32 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let substs_bool = ctx.intern_substitution(vec![GenericArg::Ty(bool_ty)]);

    let item_i32 = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_def_id,
            substs: substs_i32,
        },
        body: Arc::new(body.clone()),
        symbol: "foo::<i32>".to_string(),
        source_module: 0,
    };
    let item_bool = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_def_id,
            substs: substs_bool,
        },
        body: Arc::new(body.clone()),
        symbol: "foo::<bool>".to_string(),
        source_module: 0,
    };

    let deduped = deduplicate(&mut ctx, &[item_i32, item_bool]);
    assert_eq!(
        deduped.len(),
        1,
        "foo::<i32> and foo::<bool> with unused T should deduplicate to 1 item, got {}",
        deduped.len()
    );
}

// ============================================================
// V31-T02: fn bar<T>(x: T) where T used -> monomorphized
// ============================================================

#[test]
fn t02_used_type_param_is_monomorphized() {
    let mut ctx = test_ty_ctx();

    // Build a body where T (ParamTy index 0) DOES appear as a local type
    // fn bar<T>(x: T) -> T { x }
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let body = build_body_with_local_of_type(param_t);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(2);
    let item = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    // Analyze which params are used
    let used = analyze_used_params(&body, &ctx, substs);
    assert_eq!(used.len(), 1, "should have one param slot");
    assert!(
        used[0],
        "T is used in the body (as a local type), should be marked true"
    );

    // Compute polymorphized item — T is used so substs should remain unchanged
    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    match poly_item {
        MonoItem::Fn {
            substs: poly_substs,
            ..
        } => {
            let args = ctx.substitution_args(poly_substs);
            assert_eq!(args.len(), 1);
            if let GenericArg::Ty(ty) = args[0] {
                assert!(
                    matches!(ctx.ty_kind(ty), TyKind::Param(_)),
                    "used param T should remain as Param, got {}",
                    PrintTy::new(ty, &ctx)
                );
            }
        }
        _ => panic!("expected Fn mono item"),
    }

    // Two different instantiations should NOT deduplicate when T is used
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let substs_i32 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let substs_bool = ctx.intern_substitution(vec![GenericArg::Ty(bool_ty)]);

    let item_i32 = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_def_id,
            substs: substs_i32,
        },
        body: Arc::new(body.clone()),
        symbol: "bar::<i32>".to_string(),
        source_module: 0,
    };
    let item_bool = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_def_id,
            substs: substs_bool,
        },
        body: Arc::new(body.clone()),
        symbol: "bar::<bool>".to_string(),
        source_module: 0,
    };

    let deduped = deduplicate(&mut ctx, &[item_i32, item_bool]);
    assert_eq!(
        deduped.len(),
        2,
        "bar::<i32> and bar::<bool> with used T should NOT deduplicate, got {} items",
        deduped.len()
    );
}

// ============================================================
// V31-T03: Polymorphization across multiple calls keeps consistency
// ============================================================

#[test]
fn t03_multiple_calls_consistent_deduplication() {
    let mut ctx = test_ty_ctx();

    // Scenario: three instantiations of the same function with unused T
    // All three should deduplicate to a single item.

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let u32_ty = ctx.mk_ty(TyKind::Uint(UintTy::U32));

    // Body where T (ParamTy index 0) is unused
    let body = build_body(vec![i32_ty, i32_ty], 1);

    let fn_def_id = FnDefId::from_raw(3);

    let substs_i32 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let substs_bool = ctx.intern_substitution(vec![GenericArg::Ty(bool_ty)]);
    let substs_u32 = ctx.intern_substitution(vec![GenericArg::Ty(u32_ty)]);

    let items = vec![
        MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_def_id,
                substs: substs_i32,
            },
            body: Arc::new(body.clone()),
            symbol: "baz::<i32>".to_string(),
            source_module: 0,
        },
        MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_def_id,
                substs: substs_bool,
            },
            body: Arc::new(body.clone()),
            symbol: "baz::<bool>".to_string(),
            source_module: 0,
        },
        MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_def_id,
                substs: substs_u32,
            },
            body: Arc::new(body.clone()),
            symbol: "baz::<u32>".to_string(),
            source_module: 0,
        },
    ];

    let deduped = deduplicate(&mut ctx, &items);
    assert_eq!(
        deduped.len(),
        1,
        "three instantiations with unused T should deduplicate to 1, got {}",
        deduped.len()
    );

    // The surviving item should have unit type for the unused param
    match deduped[0].item {
        MonoItem::Fn { substs, .. } => {
            let args = ctx.substitution_args(substs);
            if let GenericArg::Ty(ty) = args[0] {
                assert!(
                    matches!(ctx.ty_kind(ty), TyKind::Unit),
                    "deduplicated item should have unit type for unused param"
                );
            }
        }
        _ => panic!("expected Fn item"),
    }
}

// ============================================================
// V31-T04: Correctness with drop glue (T unused but may need drop)
// ============================================================

#[test]
fn t04_drop_glue_unused_param_still_deduplicates() {
    // In rustc's polymorphization, unused type parameters are replaced even
    // if the type has drop glue. The rationale is that if T doesn't appear
    // in the body at all, there's nothing of type T to drop.
    let mut ctx = test_ty_ctx();

    // fn qux<T>(x: i32) -> i32 { x }  — T is unused
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(4);

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(!used[0], "T is unused even considering potential drop glue");

    // Two instantiations with types that have different drop behavior
    // should still deduplicate since T is unused
    let string_ty = ctx.mk_ty(TyKind::String);
    let substs_string = ctx.intern_substitution(vec![GenericArg::Ty(string_ty)]);
    let substs_i32 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);

    let item_string = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_def_id,
            substs: substs_string,
        },
        body: Arc::new(body.clone()),
        symbol: "qux::<String>".to_string(),
        source_module: 0,
    };
    let item_i32 = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_def_id,
            substs: substs_i32,
        },
        body: Arc::new(body.clone()),
        symbol: "qux::<i32>".to_string(),
        source_module: 0,
    };

    let deduped = deduplicate(&mut ctx, &[item_string, item_i32]);
    assert_eq!(
        deduped.len(),
        1,
        "String and i32 instantiations with unused T should deduplicate (no drop glue for T in body)"
    );
}

// ============================================================
// Additional edge case tests
// ============================================================

#[test]
fn empty_substitution_no_polymorphization() {
    let mut ctx = test_ty_ctx();

    // Non-generic function: empty substitution → no polymorphization
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    let substs = ctx.intern_substitution(Vec::new());
    let fn_def_id = FnDefId::from_raw(10);
    let item = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    assert_eq!(poly_item, item, "non-generic item should be unchanged");
}

#[test]
fn multiple_params_some_used_some_not() {
    let mut ctx = test_ty_ctx();

    // fn f<A, B>(x: A) -> A { x }
    // A is used, B is not
    let name_a = intern_name(&mut ctx, "A");
    let name_b = intern_name(&mut ctx, "B");
    let param_a = ParamTy {
        index: 0,
        name: name_a,
    };
    let param_b = ParamTy {
        index: 1,
        name: name_b,
    };
    let ty_a = ctx.mk_ty(TyKind::Param(param_a));
    let ty_b = ctx.mk_ty(TyKind::Param(param_b));

    // Body uses A as a local type but not B
    let body = build_body_with_local_of_type(ty_a);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(ty_a), GenericArg::Ty(ty_b)]);
    let fn_def_id = FnDefId::from_raw(11);
    let item = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "A is used");
    assert!(!used[1], "B is not used");

    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    match poly_item {
        MonoItem::Fn {
            substs: poly_substs,
            ..
        } => {
            let args = ctx.substitution_args(poly_substs);
            assert_eq!(args.len(), 2);
            // A should remain as Param
            if let GenericArg::Ty(ty) = args[0] {
                assert!(
                    matches!(ctx.ty_kind(ty), TyKind::Param(_)),
                    "A should remain Param"
                );
            }
            // B should be replaced with unit
            if let GenericArg::Ty(ty) = args[1] {
                assert!(
                    matches!(ctx.ty_kind(ty), TyKind::Unit),
                    "B should be replaced with unit"
                );
            }
        }
        _ => panic!("expected Fn item"),
    }
}

#[test]
fn param_used_via_fn_call_substitution() {
    let mut ctx = test_ty_ctx();

    // A function body that calls another generic function with T
    // This means T is used via the Fn const's substitution
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    let callee_substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let callee_def_id = FnDefId::from_raw(100);

    let body = build_body_with_fn_call(callee_def_id, callee_substs, Ty::UNIT);

    let caller_substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(12);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs: caller_substs,
    };

    let used = analyze_used_params(&body, &ctx, caller_substs);
    assert!(
        used[0],
        "T is used via the callee's substitution in the Fn const"
    );
}

#[test]
fn static_item_unchanged_by_polymorphization() {
    let mut ctx = test_ty_ctx();

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty], 0);

    let static_def_id = StaticDefId::from_raw(20);
    let item = MonoItem::Static {
        def_id: static_def_id,
    };

    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    assert_eq!(poly_item, item, "static items should be unchanged");
}

#[test]
fn deduplicate_preserves_order() {
    let mut ctx = test_ty_ctx();

    // First item with used param, second with unused param (different functions)
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    // Function with used T
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let body_used = build_body_with_local_of_type(param_t);

    // Function with unused T
    let body_unused = build_body(vec![i32_ty, i32_ty], 1);

    let substs_i32 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let substs_bool = ctx.intern_substitution(vec![GenericArg::Ty(ctx.bool_ty())]);

    let fn_def_used = FnDefId::from_raw(30);
    let fn_def_unused = FnDefId::from_raw(31);

    let items = vec![
        MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_def_used,
                substs: substs_i32,
            },
            body: Arc::new(body_used.clone()),
            symbol: "used_fn::<i32>".to_string(),
            source_module: 0,
        },
        MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_def_unused,
                substs: substs_i32,
            },
            body: Arc::new(body_unused.clone()),
            symbol: "unused_fn::<i32>".to_string(),
            source_module: 0,
        },
        MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_def_unused,
                substs: substs_bool,
            },
            body: Arc::new(body_unused.clone()),
            symbol: "unused_fn::<bool>".to_string(),
            source_module: 0,
        },
    ];

    let deduped = deduplicate(&mut ctx, &items);
    // used_fn::<i32> → kept (unique)
    // unused_fn::<i32> → polymorphized to unused_fn::<()>, kept
    // unused_fn::<bool> → polymorphized to unused_fn::<()>, deduplicated with previous
    assert_eq!(
        deduped.len(),
        2,
        "should have 2 items: used_fn::<i32> and unused_fn::<()>, got {}",
        deduped.len()
    );
}

// ============================================================
// Additional test: Param used in reference type
// ============================================================

#[test]
fn param_used_in_ref_local() {
    let mut ctx = test_ty_ctx();

    // fn f<T>(x: &T) — T is used via Ref(Region, T, Not)
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let ref_ty = ctx.mk_ref(Region::Erased, param_t, Mutability::Not);

    let body = build_body_with_local_of_type(ref_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(50);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside a reference type");
}

// ============================================================
// Additional test: Param used in raw pointer type
// ============================================================

#[test]
fn param_used_in_raw_ptr_local() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let ptr_ty = ctx.mk_ty(TyKind::RawPtr(param_t, Mutability::Not));

    let body = build_body_with_local_of_type(ptr_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(51);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside a raw pointer type");
}

// ============================================================
// Additional test: Param used in slice type
// ============================================================

#[test]
fn param_used_in_slice_local() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let slice_ty = ctx.mk_ty(TyKind::Slice(param_t));

    let body = build_body_with_local_of_type(slice_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(52);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside a slice type");
}

// ============================================================
// Additional test: Param used in array type
// ============================================================

#[test]
fn param_used_in_array_local() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let array_const = Const {
        kind: ConstKind::Uint(3),
        ty: ctx.mk_ty(TyKind::Uint(UintTy::Usize)),
    };
    let array_ty = ctx.mk_ty(TyKind::Array(param_t, array_const));

    let body = build_body_with_local_of_type(array_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(53);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside an array type");
}

// ============================================================
// Additional test: Param used in tuple type
// ============================================================

#[test]
fn param_used_in_tuple_local() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let tuple_subst =
        ctx.intern_substitution(vec![GenericArg::Ty(param_t), GenericArg::Ty(i32_ty)]);
    let tuple_ty = ctx.mk_ty(TyKind::Tuple(tuple_subst));

    let body = build_body_with_local_of_type(tuple_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(54);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside a tuple type");
}

// ============================================================
// Additional test: Param used in ADT type
// ============================================================

#[test]
fn param_used_in_adt_local() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let adt_id = AdtId::from_raw(1);
    let adt_subst = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let adt_ty = ctx.mk_ty(TyKind::Adt(adt_id, adt_subst));

    let body = build_body_with_local_of_type(adt_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(55);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside an ADT type");
}

// ============================================================
// Additional test: Param used in function pointer type
// ============================================================

#[test]
fn param_used_in_fn_ptr_local() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    let fn_sig = FnSig {
        inputs: ctx.intern_substitution(vec![GenericArg::Ty(param_t)]),
        output: Ty::UNIT,
        c_variadic: false,
        unsafety: Safety::Safe,
        abi: Abi::Glyim,
    };
    let fn_ptr_ty = ctx.mk_ty(TyKind::FnPtr(fn_sig));

    let body = build_body_with_local_of_type(fn_ptr_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(56);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside a function pointer type");
}

// ============================================================
// Additional test: All params unused
// ============================================================

#[test]
fn all_params_unused() {
    let mut ctx = test_ty_ctx();

    // fn f<A, B, C>(x: i32) — none used
    let name_a = intern_name(&mut ctx, "A");
    let name_b = intern_name(&mut ctx, "B");
    let name_c = intern_name(&mut ctx, "C");
    let ty_a = ctx.mk_ty(TyKind::Param(ParamTy {
        index: 0,
        name: name_a,
    }));
    let ty_b = ctx.mk_ty(TyKind::Param(ParamTy {
        index: 1,
        name: name_b,
    }));
    let ty_c = ctx.mk_ty(TyKind::Param(ParamTy {
        index: 2,
        name: name_c,
    }));

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    let substs = ctx.intern_substitution(vec![
        GenericArg::Ty(ty_a),
        GenericArg::Ty(ty_b),
        GenericArg::Ty(ty_c),
    ]);
    let fn_def_id = FnDefId::from_raw(60);
    let item = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert_eq!(used.len(), 3);
    assert!(!used[0], "A is unused");
    assert!(!used[1], "B is unused");
    assert!(!used[2], "C is unused");

    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    match poly_item {
        MonoItem::Fn {
            substs: poly_substs,
            ..
        } => {
            let args = ctx.substitution_args(poly_substs);
            assert_eq!(args.len(), 3);
            for (i, arg) in args.iter().enumerate() {
                if let GenericArg::Ty(ty) = arg {
                    assert!(
                        matches!(ctx.ty_kind(*ty), TyKind::Unit),
                        "param {i} should be replaced with unit"
                    );
                }
            }
        }
        _ => panic!("expected Fn item"),
    }
}

// ============================================================
// Additional test: Deduplication with const item
// ============================================================

#[test]
fn const_item_deduplication() {
    let mut ctx = test_ty_ctx();

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    let bool_ty = ctx.bool_ty();
    let substs_i32 = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let substs_bool = ctx.intern_substitution(vec![GenericArg::Ty(bool_ty)]);

    let const_def_id = ConstDefId::from_raw(70);

    let item_i32 = MonoItemData {
        item: MonoItem::Const {
            def_id: const_def_id,
            substs: substs_i32,
        },
        body: Arc::new(body.clone()),
        symbol: "MY_CONST::<i32>".to_string(),
        source_module: 0,
    };
    let item_bool = MonoItemData {
        item: MonoItem::Const {
            def_id: const_def_id,
            substs: substs_bool,
        },
        body: Arc::new(body.clone()),
        symbol: "MY_CONST::<bool>".to_string(),
        source_module: 0,
    };

    let deduped = deduplicate(&mut ctx, &[item_i32, item_bool]);
    assert_eq!(
        deduped.len(),
        1,
        "const items with unused type param should deduplicate"
    );
}

// ============================================================
// Additional test: Different def_ids never deduplicate
// ============================================================

#[test]
fn different_def_ids_never_deduplicate() {
    let mut ctx = test_ty_ctx();

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);

    let fn_a = FnDefId::from_raw(80);
    let fn_b = FnDefId::from_raw(81);

    let item_a = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_a,
            substs,
        },
        body: Arc::new(body.clone()),
        symbol: "a::<i32>".to_string(),
        source_module: 0,
    };
    let item_b = MonoItemData {
        item: MonoItem::Fn {
            def_id: fn_b,
            substs,
        },
        body: Arc::new(body.clone()),
        symbol: "b::<i32>".to_string(),
        source_module: 0,
    };

    let deduped = deduplicate(&mut ctx, &[item_a, item_b]);
    assert_eq!(
        deduped.len(),
        2,
        "different functions should never deduplicate even if both have unused params"
    );
}

// ============================================================
// Additional test: Param used via FnDef type (not just Fn const)
// ============================================================

#[test]
fn param_used_in_fn_def_local_type() {
    let mut ctx = test_ty_ctx();

    // fn f<T>() where a local has type fn() -> T (FnDef)
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    let fn_def_id = FnDefId::from_raw(90);
    let fn_def_substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_ty = ctx.mk_ty(TyKind::FnDef(fn_def_id, fn_def_substs));

    let body = build_body_with_local_of_type(fn_def_ty);

    let caller_substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let caller_fn_id = FnDefId::from_raw(91);
    let _ = MonoItem::Fn {
        def_id: caller_fn_id,
        substs: caller_substs,
    };

    let used = analyze_used_params(&body, &ctx, caller_substs);
    assert!(used[0], "T is used via FnDef type in a local");
}

// ============================================================
// Additional test: Param used in return type only
// ============================================================

#[test]
fn param_used_in_return_type() {
    let mut ctx = test_ty_ctx();

    // fn f<T>() -> T — the return place (local 0) has type T
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    // build_body puts first type as return place (local 0)
    let body = build_body(vec![param_t], 0);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(92);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used as the return type (local 0)");
}

// ============================================================
// Additional test: Param used only in SwitchInt switch_ty
// ============================================================

#[test]
fn param_used_in_switch_int_switch_ty() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    // Build a body with SwitchInt terminator where switch_ty is Param(T)
    let owner = dummy_def_id();
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });
    // discr local
    let discr_local = LocalIdx::from_raw(1);
    locals.push(LocalDecl {
        ty: i32_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    let discr_op = Operand::Copy(Place::new(discr_local));
    let targets = SwitchTargets::new(Box::new([]), BasicBlockIdx::from_raw(1));

    let term = Terminator {
        kind: TerminatorKind::SwitchInt {
            discr: discr_op,
            switch_ty: param_t, // T used here!
            targets,
        },
        source_info: dummy_source_info(),
    };

    let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: term,
        is_cleanup: false,
    });
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks: blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: dummy_span(),
        var_debug_info: Vec::new(),
    };

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(93);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used in SwitchInt switch_ty");
}

// ============================================================
// Additional test: Deduplication reduces mono item count correctly
// ============================================================

#[test]
fn dedup_reduces_item_count() {
    let mut ctx = test_ty_ctx();

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let bool_ty = ctx.bool_ty();
    let u8_ty = ctx.mk_ty(TyKind::Uint(UintTy::U8));
    let u16_ty = ctx.mk_ty(TyKind::Uint(UintTy::U16));

    // Function where T is unused
    let body_unused = build_body(vec![i32_ty, i32_ty], 1);
    let fn_unused = FnDefId::from_raw(100);

    // Function where T is used
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let body_used = build_body_with_local_of_type(param_t);
    let fn_used = FnDefId::from_raw(101);

    let types = [i32_ty, bool_ty, u8_ty, u16_ty];
    let mut items = Vec::new();

    // 4 instantiations of unused-T function
    for (i, ty) in types.iter().enumerate() {
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(*ty)]);
        items.push(MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_unused,
                substs,
            },
            body: Arc::new(body_unused.clone()),
            symbol: format!("unused_fn::<{i}>"),
            source_module: 0,
        });
    }

    // 4 instantiations of used-T function
    for (i, ty) in types.iter().enumerate() {
        let substs = ctx.intern_substitution(vec![GenericArg::Ty(*ty)]);
        items.push(MonoItemData {
            item: MonoItem::Fn {
                def_id: fn_used,
                substs,
            },
            body: Arc::new(body_used.clone()),
            symbol: format!("used_fn::<{i}>"),
            source_module: 0,
        });
    }

    let deduped = deduplicate(&mut ctx, &items);
    // 4 unused-T instantiations → 1 polymorphized item
    // 4 used-T instantiations → 4 separate items
    // Total: 5
    assert_eq!(
        deduped.len(),
        5,
        "expected 1 unused-T + 4 used-T = 5 items, got {}",
        deduped.len()
    );
}

// ============================================================
// Additional test: Mixed type and lifetime params
// ============================================================

#[test]
fn lifetime_params_unchanged() {
    let mut ctx = test_ty_ctx();

    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let body = build_body(vec![i32_ty, i32_ty], 1);

    // Substitution with both a type param and a lifetime
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let lifetime = Region::Erased;

    let substs = ctx.intern_substitution(vec![
        GenericArg::Ty(param_t),                // index 0: unused type param
        GenericArg::Lifetime(lifetime.clone()), // index 1: lifetime (should be kept as-is)
    ]);

    let fn_def_id = FnDefId::from_raw(110);
    let item = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(!used[0], "T is unused");

    let poly_item = compute_poly_item(&mut ctx, &item, &body);
    match poly_item {
        MonoItem::Fn {
            substs: poly_substs,
            ..
        } => {
            let args = ctx.substitution_args(poly_substs);
            assert_eq!(args.len(), 2);
            // Type param should be replaced with unit
            if let GenericArg::Ty(ty) = args[0] {
                assert!(matches!(ctx.ty_kind(ty), TyKind::Unit));
            }
            // Lifetime should remain unchanged
            if let GenericArg::Lifetime(r) = &args[1] {
                assert_eq!(
                    r.clone(),
                    lifetime,
                    "lifetime should be preserved unchanged"
                );
            }
        }
        _ => panic!("expected Fn item"),
    }
}

// ============================================================
// Additional test: Deeply nested type usage (Ref<Slice<Param>>)
// ============================================================

#[test]
fn deeply_nested_type_usage() {
    let mut ctx = test_ty_ctx();

    // fn f<T>(x: &[T]) — T used via Ref → Slice → Param
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let slice_ty = ctx.mk_ty(TyKind::Slice(param_t));
    let ref_slice_ty = ctx.mk_ref(Region::Erased, slice_ty, Mutability::Not);

    let body = build_body_with_local_of_type(ref_slice_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(120);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used deep inside Ref<Slice<T>>");
}

// ============================================================
// Additional test: Param used in Assert condition operand
// ============================================================

#[test]
fn param_used_via_assert_condition() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    // Build a body where a constant of type T appears in an Assert condition
    let owner = dummy_def_id();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });

    // Create a constant of type T (bool-like) used in assert
    let cond_const = MirConst {
        kind: MirConstKind::Bool(true),
        ty: param_t,
        span: dummy_span(),
    };
    let cond_op = Operand::Constant(cond_const);

    let target_bb = BasicBlockIdx::from_raw(1);
    let term = Terminator {
        kind: TerminatorKind::Assert {
            cond: cond_op,
            expected: true,
            target: target_bb,
            cleanup: None,
            msg: AssertMessage::DivisionByZero,
        },
        source_info: dummy_source_info(),
    };

    let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: term,
        is_cleanup: false,
    });
    blocks.push(BasicBlockData {
        statements: Vec::new(),
        terminator: Terminator {
            kind: TerminatorKind::Return,
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    });

    let body = Body {
        owner,
        basic_blocks: blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: dummy_span(),
        var_debug_info: Vec::new(),
    };

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(121);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(
        used[0],
        "T is used via constant type in Assert condition operand"
    );
}

// ============================================================
// Additional test: Opaque type with param
// ============================================================

#[test]
fn param_used_in_opaque_type() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    let opaque_id = OpaqueTyId::from_raw(1);
    let opaque_subst = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let opaque_ty = ctx.mk_ty(TyKind::Opaque(opaque_id, opaque_subst));

    let body = build_body_with_local_of_type(opaque_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(122);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside an opaque type");
}

// ============================================================
// §8.11: analyze_used_params must traverse place projections
// (Copy/Move of a projected place, Ref/Discriminant/Len of a
// place, Drop/Call destination places) and the Index operand
// local, recording generic parameters that appear there.
// ============================================================

/// Build a body with the given local types (local 0 = return), plus a single
/// statement `Assign(_0, Rvalue::Use(Copy(Place { local: arr_local,
/// projection: [Index(idx_local)] })))` and a `Drop { place: arr_local }`
/// terminator. This exercises the Copy-operand projection path, the Index
/// operand local path, and the Drop-destination place path.
fn build_body_with_indexed_copy_and_drop(arr_ty: Ty, idx_ty: Ty) -> Body {
    let owner = dummy_def_id();

    let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
    // _0: return
    locals.push(LocalDecl {
        ty: Ty::UNIT,
        mutability: Mutability::Mut,
        source_info: dummy_source_info(),
    });
    // _1: array (base of the indexed place)
    let arr_local = LocalIdx::from_raw(1);
    locals.push(LocalDecl {
        ty: arr_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });
    // _2: index operand local
    let idx_local = LocalIdx::from_raw(2);
    locals.push(LocalDecl {
        ty: idx_ty,
        mutability: Mutability::Not,
        source_info: dummy_source_info(),
    });

    let indexed_place = Place {
        local: arr_local,
        projection: Box::new([ProjectionElem::Index(idx_local)]),
    };
    let copy_op = Operand::Copy(indexed_place);

    let entry = BasicBlockData {
        statements: vec![Statement {
            kind: StatementKind::Assign(Place::new(LocalIdx::from_raw(0)), Rvalue::Use(copy_op)),
            source_info: dummy_source_info(),
        }],
        terminator: Terminator {
            kind: TerminatorKind::Drop {
                place: Place::new(arr_local),
                target: BasicBlockIdx::from_raw(0),
                cleanup: None,
            },
            source_info: dummy_source_info(),
        },
        is_cleanup: false,
    };

    let mut blocks: IndexVec<BasicBlockIdx, BasicBlockData> = IndexVec::new();
    blocks.push(entry);

    Body {
        owner,
        basic_blocks: blocks,
        locals,
        arg_count: 0,
        return_ty: Ty::UNIT,
        span: dummy_span(),
        var_debug_info: Vec::new(),
    }
}

#[test]
fn param_used_via_indexed_copy_and_drop_projection() {
    let mut ctx = test_ty_ctx();

    // T appears ONLY in the index-operand local's type (local _2). The array
    // itself (_1) is `[i32; 3]`, so T is not reachable from any local decl
    // *except* the index local — which the §8.11 Index path must still mark.
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
    let three_const = Const {
        kind: ConstKind::Uint(3),
        ty: usize_ty,
    };
    let i32_ty = ctx.mk_ty(TyKind::Int(IntTy::I32));
    let arr_ty = ctx.mk_ty(TyKind::Array(i32_ty, three_const));

    let body = build_body_with_indexed_copy_and_drop(arr_ty, param_t);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let used = analyze_used_params(&body, &ctx, substs);
    assert!(
        used[0],
        "T is used via the Index operand local of a Copy projection"
    );
}

#[test]
fn param_used_via_drop_destination_projection() {
    let mut ctx = test_ty_ctx();

    // T appears only in the array element type of a `Drop` destination place.
    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));
    let usize_ty = ctx.mk_ty(TyKind::Uint(UintTy::Usize));
    let three_const = Const {
        kind: ConstKind::Uint(3),
        ty: usize_ty,
    };
    let arr_ty = ctx.mk_ty(TyKind::Array(param_t, three_const));

    let body = build_body_with_indexed_copy_and_drop(arr_ty, ctx.mk_ty(TyKind::Int(IntTy::I32)));

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let used = analyze_used_params(&body, &ctx, substs);
    assert!(
        used[0],
        "T is used via the Drop destination place projection"
    );
}

// ============================================================
// Additional test: Closure type with param
// ============================================================

#[test]
fn param_used_in_closure_type() {
    let mut ctx = test_ty_ctx();

    let name_t = intern_name(&mut ctx, "T");
    let param_ty = ParamTy {
        index: 0,
        name: name_t,
    };
    let param_t = ctx.mk_ty(TyKind::Param(param_ty));

    let closure_id = ClosureId::from_raw(1);
    let closure_subst = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let closure_ty = ctx.mk_ty(TyKind::Closure(closure_id, closure_subst));

    let body = build_body_with_local_of_type(closure_ty);

    let substs = ctx.intern_substitution(vec![GenericArg::Ty(param_t)]);
    let fn_def_id = FnDefId::from_raw(123);
    let _ = MonoItem::Fn {
        def_id: fn_def_id,
        substs,
    };

    let used = analyze_used_params(&body, &ctx, substs);
    assert!(used[0], "T is used inside a closure type");
}
