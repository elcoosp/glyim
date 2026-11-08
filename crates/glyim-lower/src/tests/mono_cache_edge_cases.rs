//! Edge case tests for mono item cache (V24).

use crate::mono::{MonoCtx, MonoItem};
use glyim_core::def_id::{ConstDefId, CrateId, DefId, FnDefId, LocalDefId, StaticDefId};
use glyim_mir::Body;
use glyim_test::test_ty_ctx;
use glyim_type::GenericArg;
use std::sync::Arc;

fn dummy_body() -> Arc<Body> {
    Arc::new(Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(0),
    )))
}

/// Verify `MonoItemId` values are sequential starting from 0.
#[test]
fn sequential_mono_item_ids() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let items: Vec<MonoItem> = (0..5)
        .map(|i| MonoItem::Fn {
            def_id: FnDefId::from_raw(i),
            substs: empty_subst,
        })
        .collect();

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    for (i, item) in items.iter().enumerate() {
        let id = ctx.lookup(item).expect("item should be in cache");
        assert_eq!(
            id.to_raw() as usize,
            i,
            "MonoItemId should be sequential starting from 0"
        );
    }
}

/// Lookup for non-existent item returns None.
#[test]
fn lookup_nonexistent_returns_none() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let item = MonoItem::Fn {
        def_id: FnDefId::from_raw(1),
        substs: empty_subst,
    };
    ctx.collect(
        std::slice::from_ref(&item),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    let other = MonoItem::Fn {
        def_id: FnDefId::from_raw(999),
        substs: empty_subst,
    };
    assert!(
        ctx.lookup(&other).is_none(),
        "non-existent item should return None"
    );
}

/// Same `DefId` with different kinds (Fn vs Const) are distinct items.
#[test]
fn same_def_id_different_kinds() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let fn_item = MonoItem::Fn {
        def_id: FnDefId::from_raw(1),
        substs: empty_subst,
    };
    let const_item = MonoItem::Const {
        def_id: ConstDefId::from_raw(1),
        substs: empty_subst,
    };

    ctx.collect(
        &[fn_item.clone(), const_item.clone()],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        2,
        "same raw DefId index with different kinds should be distinct items"
    );

    let fn_id = ctx.lookup(&fn_item).expect("fn item should be in cache");
    let const_id = ctx
        .lookup(&const_item)
        .expect("const item should be in cache");
    assert_ne!(fn_id, const_id, "different kinds should have different IDs");
}

/// Items data contains correct bodies — verified via Arc pointer identity.
#[test]
fn items_data_has_correct_bodies() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    // Create two distinct bodies
    let body_a = Arc::new(Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(1),
    )));
    let body_b = Arc::new(Body::dummy(DefId::new(
        CrateId::from_raw(0),
        LocalDefId::from_raw(2),
    )));
    let ptr_a = Arc::as_ptr(&body_a);
    let ptr_b = Arc::as_ptr(&body_b);

    let item_a = MonoItem::Fn {
        def_id: FnDefId::from_raw(1),
        substs: empty_subst,
    };
    let item_b = MonoItem::Fn {
        def_id: FnDefId::from_raw(2),
        substs: empty_subst,
    };

    let body_a_clone = body_a.clone();
    let body_b_clone = body_b.clone();
    ctx.collect(
        &[item_a.clone(), item_b.clone()],
        &move |def_id, _substs| {
            // collect() constructs DefId with CrateId(0) and LocalDefId
            // derived from the FnDefId raw value
            if def_id.local_id == LocalDefId::from_raw(1) {
                body_a_clone.clone()
            } else {
                body_b_clone.clone()
            }
        },
        &|_ty| dummy_body(),
    );

    assert_eq!(ctx.item_count(), 2);

    let id_a = ctx.lookup(&item_a).expect("item_a");
    let id_b = ctx.lookup(&item_b).expect("item_b");

    // Verify via Arc pointer identity that each item got the correct body
    assert_eq!(
        Arc::as_ptr(&ctx.items()[id_a.to_raw() as usize].body),
        ptr_a,
        "item_a should have body_a"
    );
    assert_eq!(
        Arc::as_ptr(&ctx.items()[id_b.to_raw() as usize].body),
        ptr_b,
        "item_b should have body_b"
    );
}

/// Symbol names are generated correctly.
#[test]
fn symbol_names_generated() {
    let mut ctx = MonoCtx::new();

    let item = MonoItem::Static {
        def_id: StaticDefId::from_raw(42),
    };
    ctx.collect(
        std::slice::from_ref(&item),
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    let id = ctx.lookup(&item).expect("should find item");
    let symbol = &ctx.items()[id.to_raw() as usize].symbol;
    assert!(!symbol.is_empty(), "symbol should not be empty");
    assert!(
        symbol.contains("42"),
        "symbol should contain the DefId index: got {symbol}"
    );
}

/// Default trait implementation works correctly.
#[test]
fn default_impl() {
    let ctx1 = MonoCtx::new();
    let ctx2 = MonoCtx::default();
    assert_eq!(ctx1.item_count(), ctx2.item_count());
    assert_eq!(ctx1.cache_len(), ctx2.cache_len());
}

/// Collect with all duplicates produces single item.
#[test]
fn all_duplicates_in_start() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let item = MonoItem::Fn {
        def_id: FnDefId::from_raw(1),
        substs: empty_subst,
    };

    ctx.collect(
        &[
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
            item.clone(),
        ],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    assert_eq!(ctx.item_count(), 1, "10 duplicates should produce 1 item");
}

/// Cache remains consistent after many operations.
#[test]
fn cache_consistency_after_many_ops() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    for i in 0u32..20 {
        let item = MonoItem::Fn {
            def_id: FnDefId::from_raw(i),
            substs: empty_subst,
        };
        ctx.collect(&[item], &|_def_id, _substs| dummy_body(), &|_ty| {
            dummy_body()
        });
    }

    assert_eq!(ctx.item_count(), 20);
    assert_eq!(ctx.cache_len(), 20);

    for i in 0u32..20 {
        let item = MonoItem::Fn {
            def_id: FnDefId::from_raw(i),
            substs: empty_subst,
        };
        ctx.collect(&[item], &|_def_id, _substs| dummy_body(), &|_ty| {
            dummy_body()
        });
    }

    assert_eq!(ctx.item_count(), 20, "re-collecting should not add items");
    assert_eq!(ctx.cache_len(), 20);
}

/// Empty substitution vs substitution with one arg are distinct.
#[test]
fn empty_vs_nonempty_substitution() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let one_arg_subst = ty_ctx.intern_substitution(vec![GenericArg::Ty(ty_ctx.bool_ty())]);
    let mut ctx = MonoCtx::new();

    let fn_id = FnDefId::from_raw(1);
    let item_empty = MonoItem::Fn {
        def_id: fn_id,
        substs: empty_subst,
    };
    let item_one = MonoItem::Fn {
        def_id: fn_id,
        substs: one_arg_subst,
    };

    ctx.collect(
        &[item_empty.clone(), item_one.clone()],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    assert_eq!(
        ctx.item_count(),
        2,
        "empty vs non-empty substitution should be distinct"
    );
}

/// Static items with same raw ID as fn items are distinct.
#[test]
fn static_vs_fn_same_raw_id() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let fn_item = MonoItem::Fn {
        def_id: FnDefId::from_raw(7),
        substs: empty_subst,
    };
    let static_item = MonoItem::Static {
        def_id: StaticDefId::from_raw(7),
    };

    ctx.collect(
        &[fn_item.clone(), static_item.clone()],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    assert_eq!(ctx.item_count(), 2);
    assert_ne!(
        ctx.lookup(&fn_item),
        ctx.lookup(&static_item),
        "fn and static with same raw ID should have different MonoItemIds"
    );
}
