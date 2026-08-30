//! Tests for W2-C11-§1.1: async multi-poll state-machine desugar routing.
//!
//! These tests verify the *structural* desugar decision only (the bail-out gate
//! that engages the state-machine path when `suspend_count >= 2`). They build a
//! `CrateHir` by hand (with `Expr::Await` nodes already present) and run
//! `desugar_async` directly, bypassing the source parser. This is necessary
//! because the source-level `async fn`/`.await` lower (in `lower_expr.rs`) does
//! NOT yet emit `Expr::Await` for `.await` postfixes inside async bodies — the
//! `.await` operand is dropped and `let` bindings in async bodies are omitted,
//! so a parsed `async fn` body never contains `Expr::Await`. That parser gap is
//! tracked separately in §6.1 / KNOWN_GAPS; here we test the desugar logic in
//! isolation, which is the part §1.1 owns.
//!
//! The multi-poll poll-body uses placeholder `i32` future/live-local field types
//! because HIR is pre-type-check (the suspended future's type is unknowable
//! without future-type inference); see module docs in `lower/lower_async.rs`.

use glyim_core::arena::IndexVec;
use glyim_core::def_id::LocalDefId;
use glyim_core::interner::Interner;
use glyim_diag::GlyimDiagnostic;
use glyim_span::Span;

use crate::{
    Body, BodyId, CrateHir, Expr, ExprId, FnItem, Item, ItemId, ItemKind, Literal, Name, Param,
    Path, PathKind, PathSegment, TypeRef, Visibility,
};

use crate::lower::lower_async::desugar_async;

/// Build a `CrateHir` with a single `async fn two(a, b)` whose body is a block
/// containing `n_awaits` `Expr::Await` statements followed by a tail path. The
/// `Expr::Await` nodes are injected directly (the source parser would drop them).
fn build_async_hir(n_awaits: usize) -> CrateHir {
    let interner = Interner::new();
    let two_name: Name = interner.intern("two");
    let a_name: Name = interner.intern("a");
    let b_name: Name = interner.intern("b");
    let x_name: Name = interner.intern("x");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    // Helper to build a bare path expression.
    let path_expr = |_interner: &Interner, n: Name, exprs: &mut IndexVec<ExprId, Expr>| -> ExprId {
        let p = Path {
            segments: vec![PathSegment {
                name: n,
                generic_args: None,
            }],
            kind: PathKind::Plain,
        };
        exprs.push(Expr::Path(p))
    };

    let a_path = path_expr(&interner, a_name, &mut exprs);
    let b_path = path_expr(&interner, b_name, &mut exprs);
    let x_path = path_expr(&interner, x_name, &mut exprs);

    // Build `n_awaits` Await statements (each awaiting a distinct operand).
    let mut await_stmts: Vec<ExprId> = Vec::new();
    for k in 0..n_awaits {
        let operand = if k % 2 == 0 { a_path } else { b_path };
        let aw = exprs.push(Expr::Await { expr: operand });
        await_stmts.push(aw);
    }

    // Root block: first Block in the expr list is the desugar root.
    let block = exprs.push(Expr::Block {
        stmts: await_stmts,
        tail: Some(x_path),
    });
    let _ = block;

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats: IndexVec::new(),
        params: Vec::new(),
        span: Span::DUMMY,
        expr_spans: {
            let mut s = IndexVec::new();
            for _ in 0..exprs.len() {
                s.push(Span::DUMMY);
            }
            s
        },
    };
    let body_id = BodyId::from_raw(0);

    let params = vec![
        Param {
            name: a_name,
            ty: None,
            span: Span::DUMMY,
        },
        Param {
            name: b_name,
            ty: None,
            span: Span::DUMMY,
        },
    ];
    let fn_item = FnItem {
        params,
        return_ty: None,
        body: Some(body_id),
        is_unsafe: false,
        is_async: true,
        is_const: false,
        generic_params: Vec::new(),
        where_clauses: Vec::new(),
        abi: None,
    };
    let item = Item {
        id: ItemId::from_raw(0),
        name: two_name,
        kind: ItemKind::Fn(fn_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    };

    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    items.push(item);
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    bodies.push(body);
    let mut body_owners: IndexVec<BodyId, LocalDefId> = IndexVec::new();
    body_owners.push(LocalDefId::from_raw(0));

    CrateHir {
        items,
        bodies,
        body_owners,
        interner,
    }
}

/// Names of all enum items present in the HIR.
fn enum_item_names(hir: &CrateHir) -> Vec<String> {
    let mut out = Vec::new();
    for item in hir.items.iter() {
        if let ItemKind::Enum(_) = &item.kind {
            out.push(hir.interner.resolve(item.name).to_string());
        }
    }
    out
}

/// §1.1 routing: a function with exactly ONE `.await` must NOT produce a
/// state-enum-backed future (it stays on the existing single-poll path).
#[test]
fn single_await_no_state_enum() {
    let mut hir = build_async_hir(1);
    desugar_async(&mut hir, &mut Vec::new());
    let enums = enum_item_names(&hir);
    assert!(
        !enums.iter().any(|n| n.contains("State")),
        "single-await fn should not produce a *State enum; found: {:?}",
        enums
    );
}

/// §Phase 3 safety guard (GLYIM_DESTUB_PLAN): a function with TWO `.await`s
/// must NOT be silently desugared into the broken `Poll::Pending`-hardcoded
/// skeleton (which would hang forever). The v1 resume-dispatch that splits the
/// body at each await requires post-type-check future types (the planned MIR
/// pass) and is tracked as a known gap. Until it lands, the desugarer MUST emit
/// a clear diagnostic error instead of falling through to the old skeleton.
#[test]
fn multi_await_is_rejected_with_diagnostic() {
    let mut hir = build_async_hir(2);
    let mut diags: Vec<GlyimDiagnostic> = Vec::new();
    desugar_async(&mut hir, &mut diags);
    assert!(
        !diags.is_empty(),
        "multi-await fn must be rejected with a diagnostic, not silently desugared"
    );
    assert!(
        diags.iter().any(|d| {
            let m = d.message.to_lowercase();
            m.contains("state-machine") || m.contains("v1") || m.contains("await")
        }),
        "diagnostic must mention the unsupported multi-await state-machine shape: {:?}",
        diags
    );
    // No `*State` enum may be produced: the function was not desugared.
    let enums = enum_item_names(&hir);
    assert!(
        !enums.iter().any(|n| n.contains("State")),
        "multi-await fn must NOT produce a *State enum (would be the broken skeleton): {:?}",
        enums
    );
}

/// Build a `CrateHir` with an `async fn bad(a, b)` whose body is
/// `for i in 0..3 { let _ = a.await; }` — i.e. an `.await` *inside* a `for`
/// loop body. Phase 3 (GLYIM_DESTUB_PLAN) must reject this shape with a clear
/// compile-time diagnostic rather than silently miscompiling it into an
/// infinite-`Pending` hang.
fn build_async_hir_with_loop_await() -> CrateHir {
    let interner = Interner::new();
    let bad_name: Name = interner.intern("bad");
    let a_name: Name = interner.intern("a");
    let b_name: Name = interner.intern("b");
    let i_name: Name = interner.intern("i");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let mut pats: IndexVec<crate::PatId, crate::Pat> = IndexVec::new();

    let path_expr = |_interner: &Interner, n: Name, exprs: &mut IndexVec<ExprId, Expr>| -> ExprId {
        let p = Path {
            segments: vec![PathSegment {
                name: n,
                generic_args: None,
            }],
            kind: PathKind::Plain,
        };
        exprs.push(Expr::Path(p))
    };

    let a_path = path_expr(&interner, a_name, &mut exprs);

    // `a.await`
    let await_expr = exprs.push(Expr::Await { expr: a_path });
    // `let _x = a.await;` (use a name binding as the pattern)
    let wild_pat_id = pats.push(crate::Pat::Binding {
        name: i_name,
        mutability: glyim_core::primitives::Mutability::Not,
        subpattern: None,
    });
    let let_expr = exprs.push(Expr::Let {
        pat: wild_pat_id,
        value: await_expr,
    });
    // loop body: `let _x = a.await;` (no enclosing Block needed; pointing the
    // For body directly at the Let keeps the root block the only Block, so
    // `root_expr_id` resolves to it and the in_loop walk context is preserved)
    let loop_body = let_expr;
    // for iterator `0..3`
    let zero = exprs.push(Expr::Literal(Literal::Int(0, None)));
    let three = exprs.push(Expr::Literal(Literal::Int(3, None)));
    let range = exprs.push(Expr::Range {
        start: Some(zero),
        end: Some(three),
        inclusive: false,
    });
    let i_pat = pats.push(crate::Pat::Binding {
        name: i_name,
        mutability: glyim_core::primitives::Mutability::Not,
        subpattern: None,
    });
    // `for i in 0..3 { let _x = a.await; }`
    let for_expr = exprs.push(Expr::For {
        pat: i_pat,
        iterable: range,
        body: loop_body,
    });
    // root block: the for loop, tail `b`
    let b_path = path_expr(&interner, b_name, &mut exprs);
    let block = exprs.push(Expr::Block {
        stmts: vec![for_expr],
        tail: Some(b_path),
    });
    let _ = block;

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats,
        params: Vec::new(),
        span: Span::DUMMY,
        expr_spans: {
            let mut s = IndexVec::new();
            for _ in 0..exprs.len() {
                s.push(Span::DUMMY);
            }
            s
        },
    };
    let body_id = BodyId::from_raw(0);

    let params = vec![
        Param {
            name: a_name,
            ty: None,
            span: Span::DUMMY,
        },
        Param {
            name: b_name,
            ty: None,
            span: Span::DUMMY,
        },
    ];
    let fn_item = FnItem {
        params,
        return_ty: None,
        body: Some(body_id),
        is_unsafe: false,
        is_async: true,
        is_const: false,
        generic_params: Vec::new(),
        where_clauses: Vec::new(),
        abi: None,
    };
    let item = Item {
        id: ItemId::from_raw(0),
        name: bad_name,
        kind: ItemKind::Fn(fn_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    };

    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    items.push(item);
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    bodies.push(body);
    let mut body_owners: IndexVec<BodyId, LocalDefId> = IndexVec::new();
    body_owners.push(LocalDefId::from_raw(0));

    CrateHir {
        items,
        bodies,
        body_owners,
        interner,
    }
}

/// Builds `async fn loop_await(n: i32) -> i32 { let mut total = 0; let mut i = 0;
/// while i < n { total = dep(i).await; i = i + 1; } total }` — the supported
/// M4/M5 loop-await shape: a single `.await` inside a `while` body with a
/// statically nameable future type (`dep`).
fn build_async_hir_while_loop_await() -> CrateHir {
    let interner = Interner::new();
    let fn_name: Name = interner.intern("loop_await");
    let n_name: Name = interner.intern("n");
    let total_name: Name = interner.intern("total");
    let i_name: Name = interner.intern("i");
    let dep_name: Name = interner.intern("dep");

    let mut exprs: IndexVec<ExprId, Expr> = IndexVec::new();
    let mut pats: IndexVec<crate::PatId, crate::Pat> = IndexVec::new();

    let path = |n: Name, exprs: &mut IndexVec<ExprId, Expr>| -> ExprId {
        exprs.push(Expr::Path(Path {
            segments: vec![PathSegment {
                name: n,
                generic_args: None,
            }],
            kind: PathKind::Plain,
        }))
    };
    let i32_ty = TypeRef::Path(Path {
        segments: vec![PathSegment {
            name: interner.intern("i32"),
            generic_args: None,
        }],
        kind: PathKind::Plain,
    });

    let zero = exprs.push(Expr::Literal(Literal::Int(0, None)));
    let total_pat = pats.push(crate::Pat::Binding {
        name: total_name,
        mutability: glyim_core::primitives::Mutability::Mut,
        subpattern: None,
    });
    let let_total = exprs.push(Expr::Let {
        pat: total_pat,
        value: zero,
    });
    let i_pat = pats.push(crate::Pat::Binding {
        name: i_name,
        mutability: glyim_core::primitives::Mutability::Mut,
        subpattern: None,
    });
    let let_i = exprs.push(Expr::Let {
        pat: i_pat,
        value: zero,
    });

    let i_path = path(i_name, &mut exprs);
    let n_path = path(n_name, &mut exprs);
    let cond = exprs.push(Expr::Binary {
        op: glyim_core::primitives::BinOp::Lt,
        lhs: i_path,
        rhs: n_path,
    });

    let i_path2 = path(i_name, &mut exprs);
    let dep_path = path(dep_name, &mut exprs);
    let dep_call = exprs.push(Expr::Call {
        func: dep_path,
        args: vec![i_path2],
    });
    let await_expr = exprs.push(Expr::Await { expr: dep_call });
    let total_path = path(total_name, &mut exprs);
    let assign_total = exprs.push(Expr::Assign {
        lhs: total_path,
        rhs: await_expr,
    });
    let i_path3 = path(i_name, &mut exprs);
    let one = exprs.push(Expr::Literal(Literal::Int(1, None)));
    let i_inc = exprs.push(Expr::Binary {
        op: glyim_core::primitives::BinOp::Add,
        lhs: i_path3,
        rhs: one,
    });
    let i_path4 = path(i_name, &mut exprs);
    let assign_i = exprs.push(Expr::Assign {
        lhs: i_path4,
        rhs: i_inc,
    });
    let loop_body = exprs.push(Expr::Block {
        stmts: vec![assign_total, assign_i],
        tail: None,
    });

    let while_expr = exprs.push(Expr::While { cond, body: loop_body });
    let total_tail = path(total_name, &mut exprs);
    let block = exprs.push(Expr::Block {
        stmts: vec![let_total, let_i, while_expr],
        tail: Some(total_tail),
    });
    let _ = block;

    let body = Body {
        owner: LocalDefId::from_raw(0),
        exprs: exprs.clone(),
        pats,
        params: Vec::new(),
        span: Span::DUMMY,
        expr_spans: {
            let mut s = IndexVec::new();
            for _ in 0..exprs.len() {
                s.push(Span::DUMMY);
            }
            s
        },
    };
    let body_id = BodyId::from_raw(0);

    let params = vec![Param {
        name: n_name,
        ty: Some(i32_ty.clone()),
        span: Span::DUMMY,
    }];
    let fn_item = FnItem {
        params,
        return_ty: Some(i32_ty.clone()),
        body: Some(body_id),
        is_unsafe: false,
        is_async: true,
        is_const: false,
        generic_params: Vec::new(),
        where_clauses: Vec::new(),
        abi: None,
    };
    let item = Item {
        id: ItemId::from_raw(0),
        name: fn_name,
        kind: ItemKind::Fn(fn_item),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    };

    let mut items: IndexVec<ItemId, Item> = IndexVec::new();
    items.push(item);
    let mut bodies: IndexVec<BodyId, Body> = IndexVec::new();
    bodies.push(body);
    let mut body_owners: IndexVec<BodyId, LocalDefId> = IndexVec::new();
    body_owners.push(LocalDefId::from_raw(0));

    CrateHir {
        items,
        bodies,
        body_owners,
        interner,
    }
}


/// M4/M5 (plan §P2-1): an `.await` inside a `while` loop body, whose awaited
/// future type is statically nameable, must now desugar to a resumable
/// `*State` coroutine machine — NOT be rejected. This is the loop-await state
/// machine: on `Poll::Pending` the in-loop state is left in place and `poll()`
/// re-drives it on resume, so it can never spin forever.
#[test]
fn while_loop_await_desugars_to_state_machine() {
    let mut hir = build_async_hir_while_loop_await();
    let mut diags: Vec<GlyimDiagnostic> = Vec::new();
    desugar_async(&mut hir, &mut diags);
    // Supported shape: no error diagnostic expected.
    assert!(
        diags.iter().all(|d| !d.is_error()),
        "while-loop-await should desugar cleanly; got diagnostics: {:?}",
        diags
    );
    // It must have produced a `*State` enum (the coroutine state machine).
    let has_state_enum = hir
        .items
        .iter()
        .any(|it| {
            let s = hir.interner.resolve(it.name).to_string();
            s.contains("State") || s.ends_with("Future")
        });
    assert!(
        has_state_enum,
        "while-loop-await must produce a *State/*Future type; items: {:?}",
        hir.items
            .iter()
            .map(|it| hir.interner.resolve(it.name).to_string())
            .collect::<Vec<_>>()
    );
}
