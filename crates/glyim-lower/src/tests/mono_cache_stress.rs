//! Stress and scale tests for mono item cache (V24).

use crate::mono::{MonoCtx, MonoItem};
use glyim_core::IntTy;
use glyim_core::def_id::{ConstDefId, CrateId, DefId, FnDefId, LocalDefId, StaticDefId};
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

/// Many distinct fn items with different `DefIds` — all should be collected.
#[test]
fn many_distinct_fn_items() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();
    let count: usize = 50;

    let items: Vec<MonoItem> = (0..count)
        .map(|i| MonoItem::Fn {
            def_id: FnDefId::from_raw(i as u32),
            substs: empty_subst,
        })
        .collect();

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    assert_eq!(
        ctx.item_count(),
        count,
        "all {count} fn items should be collected"
    );
    assert_eq!(ctx.cache_len(), count, "cache should match item count");

    // Every item should be lookable-up
    for item in &items {
        assert!(ctx.lookup(item).is_some(), "every item should be in cache");
    }
}

/// Many distinct substitutions for the same fn `DefId`.
#[test]
fn many_substitutions_same_fn() {
    let mut ty_ctx = test_ty_ctx();
    let mut ctx = MonoCtx::new();
    let fn_id = FnDefId::from_raw(1);
    let count: usize = 20;

    let items: Vec<MonoItem> = (0..count)
        .map(|i| {
            let ty = ty_ctx.mk_ty(TyKind::Int(IntTy::I8));
            let args: Vec<GenericArg> = (0..=i).map(|_| GenericArg::Ty(ty)).collect();
            let subst = ty_ctx.intern_substitution(args);
            MonoItem::Fn {
                def_id: fn_id,
                substs: subst,
            }
        })
        .collect();

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    assert_eq!(
        ctx.item_count(),
        count,
        "all {count} substitutions should produce distinct items"
    );
}

/// Mixed item types: fns, consts, and statics in one collect.
#[test]
fn mixed_item_types() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    let items = vec![
        MonoItem::Fn {
            def_id: FnDefId::from_raw(1),
            substs: empty_subst,
        },
        MonoItem::Const {
            def_id: ConstDefId::from_raw(2),
            substs: empty_subst,
        },
        MonoItem::Static {
            def_id: StaticDefId::from_raw(3),
        },
        MonoItem::Fn {
            def_id: FnDefId::from_raw(4),
            substs: empty_subst,
        },
        MonoItem::Const {
            def_id: ConstDefId::from_raw(5),
            substs: empty_subst,
        },
        MonoItem::Static {
            def_id: StaticDefId::from_raw(6),
        },
    ];

    ctx.collect(&items, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });

    assert_eq!(ctx.item_count(), 6, "all 6 mixed items should be collected");

    for item in &items {
        assert!(
            ctx.lookup(item).is_some(),
            "each mixed item should be in cache"
        );
    }
}

/// Repeated collect calls adding different items each time.
#[test]
fn incremental_collect() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    // First batch: 3 items
    let batch1 = vec![
        MonoItem::Fn {
            def_id: FnDefId::from_raw(1),
            substs: empty_subst,
        },
        MonoItem::Fn {
            def_id: FnDefId::from_raw(2),
            substs: empty_subst,
        },
        MonoItem::Static {
            def_id: StaticDefId::from_raw(3),
        },
    ];
    ctx.collect(&batch1, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });
    assert_eq!(ctx.item_count(), 3);

    // Second batch: 2 new items + 1 duplicate from batch1
    let batch2 = vec![
        MonoItem::Fn {
            def_id: FnDefId::from_raw(4),
            substs: empty_subst,
        },
        MonoItem::Fn {
            def_id: FnDefId::from_raw(1),
            substs: empty_subst,
        }, // duplicate
        MonoItem::Const {
            def_id: ConstDefId::from_raw(5),
            substs: empty_subst,
        },
    ];
    ctx.collect(&batch2, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });
    assert_eq!(
        ctx.item_count(),
        5,
        "only 2 new items added (1 duplicate skipped)"
    );

    // Third batch: 1 new item
    let batch3 = vec![MonoItem::Fn {
        def_id: FnDefId::from_raw(6),
        substs: empty_subst,
    }];
    ctx.collect(&batch3, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });
    assert_eq!(ctx.item_count(), 6);
}

/// Large number of items with overlapping collect calls.
#[test]
fn large_overlapping_collect() {
    let mut ty_ctx = test_ty_ctx();
    let empty_subst = ty_ctx.intern_substitution(Vec::new());
    let mut ctx = MonoCtx::new();

    // Collect 30 items
    let batch1: Vec<MonoItem> = (0..30)
        .map(|i| MonoItem::Fn {
            def_id: FnDefId::from_raw(i),
            substs: empty_subst,
        })
        .collect();
    ctx.collect(&batch1, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });
    assert_eq!(ctx.item_count(), 30);

    // Collect 30 more where half overlap
    let batch2: Vec<MonoItem> = (15..45)
        .map(|i| MonoItem::Fn {
            def_id: FnDefId::from_raw(i),
            substs: empty_subst,
        })
        .collect();
    ctx.collect(&batch2, &|_def_id, _substs| dummy_body(), &|_ty| {
        dummy_body()
    });
    assert_eq!(
        ctx.item_count(),
        45,
        "15 new items from batch2 (15-29 already cached)"
    );
}

/// Empty start list should produce zero items.
#[test]
fn empty_start_list() {
    let mut ctx = MonoCtx::new();
    ctx.collect(&[], &|_def_id, _substs| dummy_body(), &|_ty| dummy_body());
    assert_eq!(ctx.item_count(), 0);
    assert_eq!(ctx.cache_len(), 0);
}

/// Substitution with multiple generic args.
#[test]
fn multi_arg_substitution() {
    let mut ty_ctx = test_ty_ctx();
    let mut ctx = MonoCtx::new();
    let fn_id = FnDefId::from_raw(1);

    let subst_a = ty_ctx.intern_substitution(vec![
        GenericArg::Ty(ty_ctx.bool_ty()),
        GenericArg::Ty(ty_ctx.never_ty()),
    ]);
    let subst_b = ty_ctx.intern_substitution(vec![
        GenericArg::Ty(ty_ctx.bool_ty()),
        GenericArg::Ty(ty_ctx.unit_ty()),
    ]);
    let subst_c = ty_ctx.intern_substitution(vec![
        GenericArg::Ty(ty_ctx.bool_ty()),
        GenericArg::Ty(ty_ctx.never_ty()),
    ]); // same as subst_a

    let item_a = MonoItem::Fn {
        def_id: fn_id,
        substs: subst_a,
    };
    let item_b = MonoItem::Fn {
        def_id: fn_id,
        substs: subst_b,
    };
    let item_c = MonoItem::Fn {
        def_id: fn_id,
        substs: subst_c,
    };

    ctx.collect(
        &[item_a.clone(), item_b, item_c],
        &|_def_id, _substs| dummy_body(),
        &|_ty| dummy_body(),
    );

    // subst_a and subst_c are the same, so only 2 items
    assert_eq!(
        ctx.item_count(),
        if subst_a == subst_c { 2 } else { 3 },
        "multi-arg substitutions should be deduplicated when identical"
    );
}
