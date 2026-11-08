//! Cross-kind and substitution interaction tests for mono item cache (V24).

use crate::mono::{MonoCtx, MonoItem};
use glyim_core::def_id::{ConstDefId, CrateId, DefId, FnDefId, LocalDefId, StaticDefId};
use glyim_core::{IntTy, UintTy};
use glyim_mir::Body;
use glyim_test::test_ty_ctx;
use glyim_type::{GenericArg, TyKind};
use std::sync::Arc;

fn dummy_body() -> Arc<Body> {
    Arc::new(Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(0),
    )))
}

/// Fn and Const with same `DefId` raw index but different substitution lengths.
#[test]
fn fn_and_const_same_raw_different_subst_len() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let one_arg = ty_ctx.intern_substitution(vec![GenericArg::Ty(ty_ctx.bool_ty())]);
    let mut ctx = MonoCtx::new();

    let items = vec![
        MonoItem::Fn {
            def_id: FnDefId::from_raw(1),
            substs: empty_subst,
        },
        MonoItem::Fn {
            def_id: FnDefId::from_raw(1),
            substs: one_arg,
        },
        MonoItem::Const {
            def_id: ConstDefId::from_raw(1),
            substs: ty_ctx.intern_substitution(Vec::new()),
        },
        MonoItem::Const {
            def_id: ConstDefId::from_raw(1),
            substs: ty_ctx.intern_substitution(vec![GenericArg::Ty(ty_ctx.bool_ty())]),
        },
    ];

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    assert_eq!(
        ctx.item_count(),
        4,
        "different kinds and different substs should all be distinct"
    );
}

/// Substitution with Int vs Uint types of same width are distinct.
#[test]
fn int_vs_uint_substitution() {
    let mut ty_ctx = test_ty_ctx();
    let mut ctx = MonoCtx::new();

    let i32_ty = ty_ctx.mk_ty(TyKind::Int(IntTy::I32));
    let u32_ty = ty_ctx.mk_ty(TyKind::Uint(UintTy::U32));

    let subst_i32 = ty_ctx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
    let subst_u32 = ty_ctx.intern_substitution(vec![GenericArg::Ty(u32_ty)]);

    assert_ne!(
        subst_i32, subst_u32,
        "i32 and u32 substitutions should differ"
    );

    let fn_id = FnDefId::from_raw(1);
    let item_i32 = MonoItem::Fn {
        def_id: fn_id,
        substs: subst_i32,
    };
    let item_u32 = MonoItem::Fn {
        def_id: fn_id,
        substs: subst_u32,
    };

    ctx.collect(
        &[item_i32, item_u32],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        2,
        "i32 vs u32 substitutions should produce distinct items"
    );
}

/// Collecting the same static twice across separate collect calls.
#[test]
fn static_dedup_across_collects() {
    let mut ctx = MonoCtx::new();
    let item = MonoItem::Static {
        def_id: StaticDefId::from_raw(1),
    };

    ctx.collect(
        std::slice::from_ref(&item),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );
    assert_eq!(ctx.item_count(), 1);

    ctx.collect(
        std::slice::from_ref(&item),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );
    assert_eq!(
        ctx.item_count(),
        1,
        "static should be deduplicated across collects"
    );

    ctx.collect(
        std::slice::from_ref(&item),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );
    assert_eq!(ctx.item_count(), 1, "static should still be deduplicated");
}

/// Items slice returns items in collection order.
#[test]
fn items_in_collection_order() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let items: Vec<MonoItem> = (1..=5)
        .map(|i| MonoItem::Fn {
            def_id: FnDefId::from_raw(i),
            substs: empty_subst,
        })
        .collect();

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    let collected = ctx.items();
    for (i, item) in items.iter().enumerate() {
        assert_eq!(
            collected[i].item, *item,
            "items should be in collection order"
        );
    }
}

/// `MonoCtx` starts empty and has consistent counts.
#[test]
fn fresh_ctx_is_empty() {
    let ctx = MonoCtx::new();
    assert_eq!(ctx.item_count(), 0);
    assert_eq!(ctx.cache_len(), 0);
    assert!(ctx.items().is_empty());
}

/// Const items with different substitutions are distinct.
#[test]
fn const_different_substs() {
    let mut ty_ctx = test_ty_ctx();
    let mut ctx = MonoCtx::new();

    let subst_a = ty_ctx.intern_substitution(vec![GenericArg::Ty(ty_ctx.bool_ty())]);
    let subst_b = ty_ctx.intern_substitution(vec![GenericArg::Ty(ty_ctx.unit_ty())]);

    let const_id = ConstDefId::from_raw(1);
    let item_a = MonoItem::Const {
        def_id: const_id,
        substs: subst_a,
    };
    let item_b = MonoItem::Const {
        def_id: const_id,
        substs: subst_b,
    };

    ctx.collect(
        &[item_a, item_b],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        2,
        "const with different substs should be distinct"
    );
}

/// Collecting an item, then collecting a different item, then the first again
/// — the first should not be re-added.
#[test]
fn interleaved_collect_no_readd() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let item_a = MonoItem::Fn {
        def_id: FnDefId::from_raw(1),
        substs: empty_subst,
    };
    let item_b = MonoItem::Fn {
        def_id: FnDefId::from_raw(2),
        substs: empty_subst,
    };

    ctx.collect(
        std::slice::from_ref(&item_a),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );
    assert_eq!(ctx.item_count(), 1);

    ctx.collect(
        std::slice::from_ref(&item_b),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );
    assert_eq!(ctx.item_count(), 2);

    ctx.collect(
        std::slice::from_ref(&item_a),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );
    assert_eq!(ctx.item_count(), 2, "item_a should not be re-added");
}

/// Multiple statics with different IDs.
#[test]
fn multiple_statics() {
    let mut ctx = MonoCtx::new();

    let items: Vec<MonoItem> = (0..10)
        .map(|i| MonoItem::Static {
            def_id: StaticDefId::from_raw(i),
        })
        .collect();

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    assert_eq!(ctx.item_count(), 10);

    for item in &items {
        assert!(ctx.lookup(item).is_some());
    }
}
