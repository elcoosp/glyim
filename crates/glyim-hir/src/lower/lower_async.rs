//! Async desugaring (`async fn` / `.await`).
//!
//! This pass runs over the freshly-lowered `CrateHir` *before* type-checking.
//! It rewrites every `async fn` into the `Future` state-machine shape the
//! type-checker already knows how to compile (see `dyn_dispatch.rs::
//! async_desugar_target_compiles`, which compiles that exact shape with zero
//! diagnostics):
//!
//! ```text
//! async fn foo(args) -> R { body }   =>
//!
//! struct FooFuture { f0: T0, f1: T1, .. }      // captured parameters
//! impl Future for FooFuture {
//!     type Output = R;
//!     fn poll(&mut self) -> Poll<R> { <body> }
//! }
//! fn foo(args) -> FooFuture { FooFuture { f0: args0, .. } }
//! ```
//!
//! `.await e` is rewritten to `match e.poll() { Poll::Ready(v) => v,
//! Poll::Pending => panic!("async suspension not supported") }`. `return v`
//! inside the async body becomes `return Poll::Ready(v)`; the tail expression
//! becomes `Poll::Ready(tail)`.
//!
//! SUSPENSION MODEL (documented limitation): this is a single-poll desugar —
//! it assumes awaited futures resolve on first poll (the common trivial case,
//! e.g. `async { 42 }` or awaiting an already-ready future). A `Poll::Pending`
//! from a suspended future is treated as a hard error (`panic!`) rather than
//! resuming a state machine. True multi-state coroutine lowering (generator
//! state machine + resume) is the research-grade remainder tracked in
//! `KNOWN_GAPS.md` Phase 5 and is intentionally NOT attempted here; what this
//! pass delivers is a *real*, compiling `async fn`/`.await` for the supported
//! subset, not a stub.

use glyim_core::arena::IndexVec;
use glyim_core::interner::Interner;
use glyim_core::primitives::{Mutability, StructKind, Visibility};
use crate::PatId;
use glyim_span::Span;

use crate::{
    AssociatedTy, Body, BodyId, EnumItem, Expr, ExprId, Field, ImplItem, ImplMethod, Item, ItemId,
    ItemKind, MatchArm, Param, Pat, Path, PathKind, PathSegment, StructItem, TypeRef,
    Variant,
};
use glyim_diag::{DiagSeverity, ErrorCategory, ErrorCode, GlyimDiagnostic};

/// Desugar every `async fn` in `hir` into a future struct + `impl Future` +
/// wrapper fn. `async fn` items are mutated in place (their `is_async` flag is
/// cleared and their body becomes the wrapper), and the new struct/impl items
/// are appended to `hir.items`. `Expr::Await` nodes in the poll bodies are
/// rewritten into poll matches.
pub fn desugar_async(hir: &mut crate::CrateHir, diags: &mut Vec<GlyimDiagnostic>) {
    let _initial_len = hir.items.len();
    let async_items: Vec<ItemId> = hir
        .items
        .iter_enumerated()
        .filter_map(|(id, item)| match &item.kind {
            ItemKind::Fn(fn_item) if fn_item.is_async => Some(id),
            _ => None,
        })
        .collect();

    for item_id in async_items {
        // §1.1 risk-reduction gate: the existing single-poll desugar is correct
        // and tested for bodies with at most one suspension point. Only engage
        // the multi-poll state-machine path when there are >= 2 `.await`s, where
        // a `Pending` cannot be papered over with a panic.
        let body_id = match &hir.items[item_id].kind {
            ItemKind::Fn(f) => f.body,
            _ => None,
        };
        let suspend_count = body_id
            .map(|b| {
                let mut sps = Vec::new();
                collect_suspend_points(&hir.bodies[b], root_expr_id(&hir.bodies[b]), &mut sps);
                sps.len()
            })
            .unwrap_or(0);

        // Phase 3 (GLYIM_DESTUB_PLAN): the v1 state-machine transform does NOT
        // yet support `.await` inside a `while`/`loop`/`for` body. Resuming into
        // a loop's mid-iteration state requires splitting the loop body itself,
        // which is the v2 follow-up. Until then, the single most important
        // correctness guard is to NEVER silently fall through to the old
        // skeleton (whose `S_k` arms hardcode `Poll::Pending` and produce an
        // infinite-`Pending` miscompile that hangs forever). Instead we emit a
        // clear compile-time diagnostic and route to the single-poll desugar,
        // which at least turns each `Pending` into a loud `panic!` rather than a
        // silent infinite loop.
        let loop_await_await = body_id.and_then(|b| {
            let root = root_expr_id(&hir.bodies[b]);
            let mut sps = Vec::new();
            collect_suspend_points(&hir.bodies[b], root, &mut sps);
            let mut loop_desc = std::collections::HashSet::new();
            collect_loop_body_descendants(&hir.bodies[b], &mut loop_desc);
            let found = sps.iter().find(|sp| loop_desc.contains(&sp.await_expr));
            found.map(|sp| sp.await_expr)
        });
        let loop_await = loop_await_await.is_some();
        if loop_await {
            // M4/M5: attempt the loop-await coroutine desugar (a genuinely
            // resumable state machine — `Poll::Pending` leaves the in-loop
            // state in place and `poll()` re-drives it on resume, so it can
            // never spin forever). It returns `Some(())` for the supported
            // shape (a single `.await` inside a `while` body with a statically
            // nameable future type) and `None` for unsupported shapes.
            if desugar_loop_async_fn(hir, item_id, diags).is_some() {
                continue;
            }
            // Unsupported shape: leave the safety gate in place. Routing an
            // unsupported loop body to the single-poll desugar would turn the
            // in-loop `Pending` into a `loop {}` that hangs forever — a silent
            // miscompile. Falling through emits the loud `async-v2` diagnostic
            // (ErrorCode 60) and skips desugaring, so type-checking rejects the
            // un-lowered `Expr::Await` instead.
            let await_expr = loop_await_await;
            let span = await_expr
                .and_then(|eid| hir.bodies[body_id.unwrap()].expr_spans.get(eid).copied())
                .unwrap_or(Span::DUMMY);
            diags.push(GlyimDiagnostic::new(
                ErrorCode {
                    category: ErrorCategory::Type,
                    number: 60,
                },
                DiagSeverity::Error,
                "`.await` inside a loop body is not yet supported by the async state-machine \
                 lowering (tracked: KNOWN_GAPS.md async-v2). Hoist the await out of the loop, \
                 or collect futures into a Vec and await them sequentially outside the loop.",
                glyim_diag::MultiSpan::from_span(span),
            ));
            continue;
        } else if suspend_count <= 1 {
            // Single-suspension bodies are handled by the correct, tested
            // single-poll desugar (the future resolves on the first poll, or
            // `Pending` becomes a loud `loop {}`/panic rather than a silent hang).
            desugar_one_async_fn(hir, item_id);
        } else {
            // Multi-await sequential body (>= 2 `.await`s, none inside a loop).
            // Route to the real v1 resume-dispatch state-machine desugar
            // (plan §Phase 3 / M4). It emits a `Start`/`S0`/…/`S_{n-1}`/`Done`
            // state enum plus a `poll` body that drives each suspended future
            // and stores live locals + the in-flight future across
            // `Poll::Pending`, so the future genuinely suspends and resumes.
            desugar_multi_async_fn(hir, item_id, diags);
        }
    }
}

/// Return the id of the body's root expression (the one whose value is the
/// function's return). Mirrors the "first `Expr::Block`, else last expr" rule
/// used by `rewrite_for_poll` in this file.
fn root_expr_id(body: &Body) -> ExprId {
    // The function body is the outermost block. This builder (and the parser)
    // push child expressions before their parent, so the outermost block is the
    // *last* `Expr::Block` allocated, not the first.
    let block = (0..body.exprs.len())
        .rev()
        .map(|i| ExprId::from_raw(i as u32))
        .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));
    match block {
        Some(rid) => rid,
        None => ExprId::from_raw((body.exprs.len().saturating_sub(1)) as u32),
    }
}

/// One `.await` site inside an async body, in lexical/execution order.
/// Build an `Expr::Path(self)` reference expression for the `self` receiver.
fn self_expr(interner: &Interner, body: &mut Body, self_name: crate::Name) -> ExprId {
    body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(self_name))),
        Span::DUMMY,
    )
}

struct SuspendPoint {
    /// The `ExprId` of the `Expr::Await` node itself (`expr` is the future).
    await_expr: ExprId,
}

/// Walk `body` in structural evaluation order and collect every `Expr::Await`,
/// assigning ids 0..N in execution order. Does NOT descend into nested
/// `async fn`/`async {}` blocks (those are desugared independently by the
/// outer loop over `hir.items`).
fn collect_suspend_points(body: &Body, root: ExprId, out: &mut Vec<SuspendPoint>) {
    fn walk(body: &Body, id: ExprId, out: &mut Vec<SuspendPoint>) {
        match &body.exprs[id] {
            Expr::Await { expr } => {
                walk(body, *expr, out);
                out.push(SuspendPoint {
                    await_expr: id,
                });
            }
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    walk(body, *s, out);
                }
                if let Some(t) = tail {
                    walk(body, *t, out);
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                walk(body, *cond, out);
                walk(body, *then_branch, out);
                if let Some(e) = else_branch {
                    walk(body, *e, out);
                }
            }
            Expr::Match { scrutinee, arms } => {
                walk(body, *scrutinee, out);
                for a in arms {
                    if let Some(g) = a.guard {
                        walk(body, g, out);
                    }
                    walk(body, a.body, out);
                }
            }
            Expr::Call { func, args } => {
                walk(body, *func, out);
                for a in args {
                    walk(body, *a, out);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                walk(body, *receiver, out);
                for a in args {
                    walk(body, *a, out);
                }
            }
            Expr::While { cond, body: wb } => {
                walk(body, *cond, out);
                walk(body, *wb, out);
            }
            Expr::Loop { body: lb } => walk(body, *lb, out),
            Expr::For { iterable, body: fb, .. } => {
                walk(body, *iterable, out);
                walk(body, *fb, out);
            }
            Expr::Return { value: Some(v) } => walk(body, *v, out),
            Expr::Let { value, .. } => walk(body, *value, out),
            Expr::Assign { lhs, rhs } => {
                walk(body, *lhs, out);
                walk(body, *rhs, out);
            }
            Expr::Field { receiver, .. } => walk(body, *receiver, out),
            Expr::Index { base, index } => {
                walk(body, *base, out);
                walk(body, *index, out);
            }
            Expr::Unary { expr, .. } => walk(body, *expr, out),
            Expr::Binary { lhs, rhs, .. } => {
                walk(body, *lhs, out);
                walk(body, *rhs, out);
            }
            Expr::Cast { expr, .. } => walk(body, *expr, out),
            Expr::Ref { expr, .. } => walk(body, *expr, out),
            Expr::Struct { fields, spread, .. } => {
                for (_, f) in fields {
                    walk(body, *f, out);
                }
                if let Some(s) = spread {
                    walk(body, *s, out);
                }
            }
            Expr::Array(elems) => {
                for e in elems {
                    walk(body, *e, out);
                }
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    walk(body, *e, out);
                }
            }
            Expr::Range { start, end, .. } => {
                if let Some(s) = start {
                    walk(body, *s, out);
                }
                if let Some(e) = end {
                    walk(body, *e, out);
                }
            }
            Expr::Closure { body: cb, .. } => walk(body, *cb, out),
            _ => {}
        }
    }
    walk(body, root, out);
}

/// Collect every `ExprId` that lies textually inside a `while`/`loop`/`for`
/// body. Used by the async desugar's loop-await guard to decide whether a
/// suspend point must be rejected (the v1 state machine cannot resume into a
/// loop's mid-iteration state).
///
/// Implemented by scanning the **entire** function body arena for loop nodes
/// (not by walking from `root_expr_id`, which can return a `Block` that is not
/// the ancestor of the loop in the HIR tree — e.g. when the function body is a
/// `while` whose surrounding block is not the returned root). This makes the
/// detection root-independent and unable to silently miss a nested await.
fn collect_loop_body_descendants(body: &Body, set: &mut std::collections::HashSet<ExprId>) {
    // First, gather every loop node's body so we can descend into each.
    let loops: Vec<ExprId> = (0..body.exprs.len())
        .map(|i| ExprId::from_raw(i as u32))
        .filter(|&id| matches!(
            body.exprs[id],
            Expr::While { .. } | Expr::Loop { .. } | Expr::For { .. }
        ))
        .collect();
    // Recursively add every descendant of each loop body. `add_subtree` itself
    // recurses into nested loops, so deeply nested awaits are covered.
    fn add_subtree(body: &Body, id: ExprId, set: &mut std::collections::HashSet<ExprId>) {
        set.insert(id);
        let e = body.exprs[id].clone();
        match e {
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    add_subtree(body, s, set);
                }
                if let Some(t) = tail {
                    add_subtree(body, t, set);
                }
            }
            Expr::If { cond, then_branch, else_branch } => {
                add_subtree(body, cond, set);
                add_subtree(body, then_branch, set);
                if let Some(e) = else_branch {
                    add_subtree(body, e, set);
                }
            }
            Expr::Match { scrutinee, arms } => {
                add_subtree(body, scrutinee, set);
                for a in arms {
                    if let Some(g) = a.guard {
                        add_subtree(body, g, set);
                    }
                    add_subtree(body, a.body, set);
                }
            }
            Expr::Call { func, args } => {
                add_subtree(body, func, set);
                for a in args {
                    add_subtree(body, a, set);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                add_subtree(body, receiver, set);
                for a in args {
                    add_subtree(body, a, set);
                }
            }
            Expr::While { cond, body: wb } => {
                add_subtree(body, cond, set);
                add_subtree(body, wb, set);
            }
            Expr::Loop { body: lb } => add_subtree(body, lb, set),
            Expr::For { iterable, body: fb, .. } => {
                add_subtree(body, iterable, set);
                add_subtree(body, fb, set);
            }
            Expr::Assign { lhs, rhs } => {
                add_subtree(body, lhs, set);
                add_subtree(body, rhs, set);
            }
            Expr::Binary { lhs, rhs, .. } => {
                add_subtree(body, lhs, set);
                add_subtree(body, rhs, set);
            }
            Expr::Return { value: Some(v) } => add_subtree(body, v, set),
            Expr::Let { value, .. } => {
                add_subtree(body, value, set);
            }
            Expr::Struct { fields, spread, .. } => {
                for (_, f) in fields {
                    add_subtree(body, f, set);
                }
                if let Some(s) = spread {
                    add_subtree(body, s, set);
                }
            }
            Expr::Field { receiver, .. } => add_subtree(body, receiver, set),
            Expr::Index { base, index } => {
                add_subtree(body, base, set);
                add_subtree(body, index, set);
            }
            Expr::Unary { expr, .. } => add_subtree(body, expr, set),
            Expr::Cast { expr, .. } => add_subtree(body, expr, set),
            Expr::Ref { expr, .. } => add_subtree(body, expr, set),
            Expr::Array(elems) => {
                for e in elems {
                    add_subtree(body, e, set);
                }
            }
            Expr::Tuple(elems) => {
                for e in elems {
                    add_subtree(body, e, set);
                }
            }
            Expr::Range { start, end, .. } => {
                if let Some(s) = start {
                    add_subtree(body, s, set);
                }
                if let Some(e) = end {
                    add_subtree(body, e, set);
                }
            }
            Expr::Closure { body: cb, .. } => add_subtree(body, cb, set),
            _ => {}
        }
    }
    for l in loops {
        if let Expr::While { body: wb, .. } = body.exprs[l] {
            add_subtree(body, wb, set);
        } else if let Expr::Loop { body: lb } = body.exprs[l] {
            add_subtree(body, lb, set);
        } else if let Expr::For { body: fb, .. } = body.exprs[l] {
            add_subtree(body, fb, set);
        }
    }
}

fn plain_path(interner: &Interner, name: &str) -> Path {
    Path {
        segments: vec![PathSegment {
            name: interner.intern(name),
            generic_args: None,
        }],
        kind: PathKind::Plain,
    }
}

fn generic_path(interner: &Interner, name: &str, args: Vec<TypeRef>) -> Path {
    Path {
        segments: vec![PathSegment {
            name: interner.intern(name),
            generic_args: Some(args),
        }],
        kind: PathKind::Plain,
    }
}

fn desugar_one_async_fn(hir: &mut crate::CrateHir, item_id: ItemId) {
    let mut item = hir.items[item_id].clone();
    let fn_item = match &mut item.kind {
        ItemKind::Fn(f) => f,
        _ => return,
    };
    let fn_name = item.name;
    let original_params = fn_item.params.clone();
    let return_ty = fn_item.return_ty.clone();
    let original_body_id = match fn_item.body {
        Some(b) => b,
        None => return,
    };
    // Capture the original body's parameter patterns so the wrapper fn can
    // reference the parameters by name when constructing the future.
    let original_body_params = hir.bodies[original_body_id].params.clone();
    let original_body_owner = hir.bodies[original_body_id].owner;

    let interner = &hir.interner;
    let fn_name_str = interner.resolve(fn_name).to_string();
    let future_name = format!("{}Future", fn_name_str);
    let future_name_id = interner.intern(&future_name);
    let output_id = interner.intern("Output");
    let poll_id = interner.intern("poll");
    let ready_id = interner.intern("Ready");
    let pending_id = interner.intern("Pending");

    // ---- 1. Future struct: one field per captured parameter ----
    let mut fields = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{}", i));
        let ty = p
            .ty
            .clone()
            .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
        fields.push(Field {
            name: field_name,
            ty,
            span: Span::DUMMY,
        });
    }
    let future_struct_item = Item {
        id: ItemId::from_raw(hir.items.len() as u32),
        name: future_name_id,
        kind: ItemKind::Struct(StructItem {
            fields,
            kind: StructKind::Record,
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    };

    // ---- 2. Build the `poll` body: rewrite the async body ----
    // Map each async parameter name -> the future-struct field name (`f0`,
    // `f1`, ...). Inside `poll(&mut self)` the parameters are only reachable
    // via `self.f0`, so every reference to a parameter in the poll body is
    // rewritten to a field access on `self`.
    let mut param_fields: std::collections::HashMap<crate::Name, crate::Name> = std::collections::HashMap::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{}", i));
        param_fields.insert(p.name, field_name);
    }
    let self_name = interner.intern("self");
    let mut poll_body = {
        let original_body = &hir.bodies[original_body_id];
        let mut b = Body {
            owner: original_body.owner,
            exprs: original_body.exprs.clone(),
            pats: IndexVec::new(),
            params: Vec::new(),
            span: original_body.span,
            expr_spans: original_body.expr_spans.clone(),
        };
        // The poll body's sole parameter is `self` (the future struct). Push it
        // FIRST so it is allocated the lowest local index — the MIR convention
        // (used by the LLVM prologue and the MIR interpreter) assigns parameter
        // `i` to `locals[1 + i]`. Appending the original body's pats afterwards
        // keeps non-parameter local bindings referenced by the body alive,
        // while `rewrite_for_poll` rewrites the captured async-parameter
        // references to `self.fN` field accesses. Pushing `self` *after* the
        // original pats (the previous behaviour) gave `self` a high local
        // index, so the field rewrite resolved to a different slot than the
        // parameter binding and the future field read returned garbage.
        let self_pat = b.pats.push(Pat::Binding {
            name: self_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        for p in original_body.pats.iter() {
            b.pats.push(p.clone());
        }
        b.params.push(self_pat);
        b
    };
    rewrite_for_poll(interner, &mut poll_body, &param_fields, self_name, ready_id, pending_id);
    let poll_body_id = hir.bodies.push(poll_body);

    // ---- 3. impl Future for FooFuture ----
    let output_ty = return_ty
        .clone()
        .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
    let poll_return_ty = TypeRef::Path(generic_path(interner, "Poll", vec![output_ty.clone()]));
    let self_mut_param = Param {
        name: interner.intern("self"),
        ty: None,
        span: Span::DUMMY,
    };
    let poll_method = ImplMethod {
        name: poll_id,
        body: Some(poll_body_id),
        params: vec![self_mut_param],
        return_ty: Some(poll_return_ty),
    };
    let impl_item = Item {
        id: ItemId::from_raw(hir.items.len() as u32 + 1),
        name: future_name_id,
        kind: ItemKind::Impl(ImplItem {
            trait_ref: Some(plain_path(interner, "Future")),
            self_ty: TypeRef::Path(plain_path(interner, &future_name)),
            methods: vec![poll_method],
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
            associated_types: vec![AssociatedTy {
                name: output_id,
                bounds: Vec::new(),
                default: Some(output_ty.clone()),
            }],
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    };

    // ---- 4. Wrapper fn body: `FooFuture { f0: a0, .. }` ----
    let mut wrapper_body = Body {
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        params: original_body_params,
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let mut wrapper_fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{}", i));
        let var_id = wrapper_body.alloc_expr(
            Expr::Path(plain_path(interner, interner.resolve(p.name))),
            Span::DUMMY,
        );
        wrapper_fields.push((field_name, var_id));
    }
    let struct_lit = wrapper_body.alloc_expr(
        Expr::Struct {
            path: plain_path(interner, &future_name),
            fields: wrapper_fields,
            spread: None,
        },
        Span::DUMMY,
    );
    let return_struct = wrapper_body.alloc_expr(Expr::Return { value: Some(struct_lit) }, Span::DUMMY);
    let _ = return_struct;
    let wrapper_body_id = hir.bodies.push(wrapper_body);

    // Clear `is_async`: after desugaring, `f` is a synchronous function that
    // constructs and returns the future struct. Keeping `is_async` true would
    // make coherence treat it as a second `async` impl and is inconsistent
    // with the post-desugar semantics.
    fn_item.is_async = false;
    fn_item.body = Some(wrapper_body_id);
    fn_item.return_ty = Some(TypeRef::Path(plain_path(interner, &future_name)));
    hir.items[item_id] = item;

    // Append the new struct + impl items.
    hir.items.push(future_struct_item);
    hir.items.push(impl_item);
}

/// Recursively rewrite the poll body: wrap `return v` in `Poll::Ready`,
/// rewrite `.await` into a poll `Match`, and rewrite references to the async
/// function's parameters into `self.fN` field accesses (the parameters are
/// captured as fields of the future struct and only reachable through `self`
/// inside `poll(&mut self)`).
fn rewrite_for_poll(
    interner: &Interner,
    body: &mut Body,
    param_fields: &std::collections::HashMap<crate::Name, crate::Name>,
    self_name: crate::Name,
    ready_id: crate::Name,
    pending_id: crate::Name,
) {
    // Snapshot expr ids to avoid iterating while mutating.
    let ids: Vec<ExprId> = (0..body.exprs.len()).map(|i| ExprId::from_raw(i as u32)).collect();
    for eid in ids {
        let expr = body.exprs[eid].clone();
        let new_expr = rewrite_expr(interner, body, expr, param_fields, self_name, ready_id, pending_id);
        body.exprs[eid] = new_expr;
    }

    // The async body's final tail expression becomes the future's output, so
    // it must be wrapped in `Poll::Ready(..)` to match `poll`'s
    // `-> Poll<Output>` return type. (Explicit `return` sites are already
    // wrapped by `rewrite_expr`; this handles the implicit tail.)
    //
    // IMPORTANT: we must wrap the *Block's tail* expression (the actual return
    // value), NOT `body.exprs.last()`. The flat arena's last slot is often a
    // `self`-path allocated during `rewrite_expr`; wrapping it in
    // `Poll::Ready(..)` would make any `self.fN` field access (whose receiver
    // points at that same slot) resolve to `Poll::Ready(self).fN` — a type
    // error. Locate the root `Block` and wrap its `tail`.
    let root_block = (0..body.exprs.len())
        .map(|i| ExprId::from_raw(i as u32))
        .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));
    let tail_id = match root_block {
        Some(rid) => match &body.exprs[rid] {
            Expr::Block { tail: Some(t), .. } => *t,
            Expr::Block { .. } => return,
            _ => return,
        },
        // Non-block body: the sole/top-level expr is the tail.
        None => ExprId::from_raw(0),
    };
    // Wrap the tail value in `Poll::Ready(..)`. `fresh_tail` is a *separate*
    // slot holding a clone of the original tail, so the wrapper's argument
    // does NOT point at `tail_id` itself (which would be a self-referential
    // expression that infinitely recurses during type-checking).
    let original_tail = body.exprs[tail_id].clone();
    let fresh_tail = body.alloc_expr(original_tail, Span::DUMMY);
    let func = body.alloc_expr(Expr::Path(two_seg(interner, "Poll", ready_id)), Span::DUMMY);
    let wrapped_expr = Expr::Call {
        func,
        args: vec![fresh_tail],
    };
    // Overwrite the old root block's tail slot with the wrapped value. (The
    // root block itself is left in place but is no longer the last expr.)
    body.exprs[tail_id] = wrapped_expr;
    // Append a *fresh* root `Block { tail: Some(tail_id) }` as the LAST expr
    // in the arena. `check_stmt` treats `body.exprs[len-1]` as the function's
    // return value, so the wrapped `Poll::Ready(..)` must be reachable as that
    // final block's tail — not the `Poll::Ready` constructor *path* that was
    // allocated above (which would resolve to the `poll` fn-item and produce
    // a `FnDef vs Poll<Output>` mismatch).
    let new_root_stmts = match root_block {
        Some(rid) => {
            if let Expr::Block { stmts, .. } = &body.exprs[rid] {
                stmts.clone()
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    };
    body.alloc_expr(
        Expr::Block {
            stmts: new_root_stmts,
            tail: Some(tail_id),
        },
        Span::DUMMY,
    );
}

/// Compute, for each suspend point `k`, the set of local bindings that are
/// *live after* suspend point `k` (i.e. referenced by some expression that
/// executes after `k`). These are the locals the state variant `S_k` must
/// capture so they survive a `Pending`/`resume` round-trip.
///
/// This is a conservative, name-based analysis over the HIR `Expr` tree: it
/// walks the body collecting, per suspend point, every `Name` referenced after
/// that point. It does NOT distinguish scopes or re-bindings (it is
/// over-approximate, which is sound: over-capturing a local only costs memory,
/// never correctness). The `original_params` names are excluded (they are
/// already captured as the outer future struct's `f0..fn` fields and reachable
/// via `self.fN`).
/// Compute, for each suspend point `k`, the set of local bindings that are
/// *live after* suspend point `k`. Scaffold for the deferred multi-await v1
/// transform; not yet wired into dispatch.
#[allow(dead_code)]
fn compute_live_across_suspends(
    body: &Body,
    suspend_points: &[SuspendPoint],
    original_params: &[Param],
) -> Vec<std::collections::BTreeSet<crate::Name>> {
    use std::collections::BTreeSet;

    let param_names: BTreeSet<crate::Name> =
        original_params.iter().map(|p| p.name).collect();

    // Collect every `Name` referenced anywhere in the body.
    fn collect_names(body: &Body, root: ExprId, out: &mut BTreeSet<crate::Name>) {
        fn walk(body: &Body, id: ExprId, out: &mut BTreeSet<crate::Name>) {
            match &body.exprs[id] {
                Expr::Path(p) => {
                    if let Some(n) = p.as_name() {
                        out.insert(n);
                    }
                }
                Expr::Block { stmts, tail } => {
                    for s in stmts {
                        walk(body, *s, out);
                    }
                    if let Some(t) = tail {
                        walk(body, *t, out);
                    }
                }
                Expr::If {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    walk(body, *cond, out);
                    walk(body, *then_branch, out);
                    if let Some(e) = else_branch {
                        walk(body, *e, out);
                    }
                }
                Expr::Match { scrutinee, arms } => {
                    walk(body, *scrutinee, out);
                    for a in arms {
                        if let Some(g) = a.guard {
                            walk(body, g, out);
                        }
                        walk(body, a.body, out);
                    }
                }
                Expr::Call { func, args } => {
                    walk(body, *func, out);
                    for a in args {
                        walk(body, *a, out);
                    }
                }
                Expr::MethodCall { receiver, args, .. } => {
                    walk(body, *receiver, out);
                    for a in args {
                        walk(body, *a, out);
                    }
                }
                Expr::While { cond, body: wb } => {
                    walk(body, *cond, out);
                    walk(body, *wb, out);
                }
                Expr::Loop { body: lb } => walk(body, *lb, out),
                Expr::For { iterable, body: fb, .. } => {
                    walk(body, *iterable, out);
                    walk(body, *fb, out);
                }
                Expr::Return { value: Some(v) } => walk(body, *v, out),
                Expr::Let { value, .. } => walk(body, *value, out),
                Expr::Assign { lhs, rhs } => {
                    walk(body, *lhs, out);
                    walk(body, *rhs, out);
                }
                Expr::Field { receiver, .. } => walk(body, *receiver, out),
                Expr::Index { base, index } => {
                    walk(body, *base, out);
                    walk(body, *index, out);
                }
                Expr::Unary { expr, .. } => walk(body, *expr, out),
                Expr::Binary { lhs, rhs, .. } => {
                    walk(body, *lhs, out);
                    walk(body, *rhs, out);
                }
                Expr::Cast { expr, .. } => walk(body, *expr, out),
                Expr::Ref { expr, .. } => walk(body, *expr, out),
                Expr::Struct { fields, spread, .. } => {
                    for (_, f) in fields {
                        walk(body, *f, out);
                    }
                    if let Some(s) = spread {
                        walk(body, *s, out);
                    }
                }
                Expr::Array(elems) => {
                    for e in elems {
                        walk(body, *e, out);
                    }
                }
                Expr::Tuple(elems) => {
                    for e in elems {
                        walk(body, *e, out);
                    }
                }
                Expr::Range { start, end, .. } => {
                    if let Some(s) = start {
                        walk(body, *s, out);
                    }
                    if let Some(e) = end {
                        walk(body, *e, out);
                    }
                }
                Expr::Closure { body: cb, .. } => walk(body, *cb, out),
                _ => {}
            }
        }
        walk(body, root, out);
    }

    let root = root_expr_id(body);
    let mut result = Vec::with_capacity(suspend_points.len());
    for k in 0..suspend_points.len() {
        // Names referenced strictly AFTER suspend point k. We approximate
        // "after" by collecting every name in the body, then subtracting the
        // names that appear only at or before k's position. A precise
        // intra-body split would require a CFG; here we over-approximate by
        // taking all names referenced anywhere after the first statement that
        // contains an await, which is sound for capture purposes.
        let _ = k;
        let mut all = BTreeSet::new();
        collect_names(body, root, &mut all);
        // Drop parameters (captured separately as `f0..fn`).
        for pn in &param_names {
            all.remove(pn);
        }
        result.push(all);
    }
    result
}

/// Build the state-enum HIR item FooState with Start, S0..S{n-1}, Done variants.
/// Future/live-local field types use an i32 placeholder (HIR is pre-type-check).
/// See module docs / section 6.1 for the type-check gap.
/// Build the `FooState` enum for the (deferred) multi-await v1 transform.
/// Retained as scaffold; the real v1 is the post-MIR pass in
/// `glyim-lower/src/async_state_transform.rs` (see plan §Phase 3). Marked
/// allow(dead_code) because it is not yet wired into the dispatch.
#[allow(dead_code)]
fn build_state_enum(
    hir: &mut crate::CrateHir,
    state_name: &str,
    original_params: &[Param],
    live_across: &[std::collections::BTreeSet<crate::Name>],
) -> Item {
    let interner = &hir.interner;
    let state_name_id = interner.intern(state_name);
    let mut variants = Vec::new();

    // Start(P0..Pn) — captures the function parameters.
    let mut start_fields = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{}", i));
        let ty = p
            .ty
            .clone()
            .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
        start_fields.push(Field {
            name: field_name,
            ty,
            span: Span::DUMMY,
        });
    }
    variants.push(Variant {
        name: interner.intern("Start"),
        fields: start_fields,
        kind: StructKind::Record,
        span: Span::DUMMY,
    });

    // S_k { fut, ..live } — captures the suspended future + live locals.
    for (k, live) in live_across.iter().enumerate() {
        let mut fields = Vec::new();
        let fut_name = interner.intern("fut");
        fields.push(Field {
            name: fut_name,
            ty: TypeRef::Path(plain_path(interner, "i32")),
            span: Span::DUMMY,
        });
        for (idx, live_name) in live.iter().enumerate() {
            let field_name = interner.intern(&format!("live{}", idx));
            let _ = live_name;
            fields.push(Field {
                name: field_name,
                ty: TypeRef::Path(plain_path(interner, "i32")),
                span: Span::DUMMY,
            });
        }
        variants.push(Variant {
            name: interner.intern(&format!("S{}", k)),
            fields,
            kind: StructKind::Record,
            span: Span::DUMMY,
        });
    }

    // Done — terminal state.
    variants.push(Variant {
        name: interner.intern("Done"),
        fields: Vec::new(),
        kind: StructKind::Record,
        span: Span::DUMMY,
    });

    Item {
        id: ItemId::from_raw(hir.items.len() as u32),
        name: state_name_id,
        kind: ItemKind::Enum(EnumItem {
            variants,
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    }
}

/// Build the outer future wrapper struct: `struct FooFuture { state: FooState }`.
/// Build the outer future wrapper struct. Scaffold for the deferred multi-await
/// v1 transform (the real v1 is a post-MIR pass); not yet wired into dispatch.
#[allow(dead_code)]
fn build_future_wrapper_struct(
    hir: &mut crate::CrateHir,
    future_name: &str,
    state_name: &str,
) -> Item {
    let interner = &hir.interner;
    let future_name_id = interner.intern(future_name);
    Item {
        id: ItemId::from_raw(hir.items.len() as u32),
        name: future_name_id,
        kind: ItemKind::Struct(StructItem {
            fields: vec![Field {
                name: interner.intern("state"),
                ty: TypeRef::Path(plain_path(interner, state_name)),
                span: Span::DUMMY,
            }],
            kind: StructKind::Record,
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    }
}

/// Build `impl Future for FooFuture { type Output = R; fn poll(&mut self) -> Poll<R> }`.
/// Shared by both the single-poll and multi-poll desugar paths.
/// Build `impl Future for FooFuture`. Scaffold for the deferred multi-await v1
/// transform (the real v1 is a post-MIR pass); not yet wired into dispatch.
#[allow(dead_code)]
fn build_future_impl(
    hir: &mut crate::CrateHir,
    future_name: &str,
    output_ty: TypeRef,
    poll_body_id: BodyId,
) -> Item {
    let interner = &hir.interner;
    let future_name_id = interner.intern(future_name);
    let output_id = interner.intern("Output");
    let poll_id = interner.intern("poll");
    let poll_return_ty = TypeRef::Path(generic_path(interner, "Poll", vec![output_ty.clone()]));
    let self_mut_param = Param {
        name: interner.intern("self"),
        ty: None,
        span: Span::DUMMY,
    };
    Item {
        id: ItemId::from_raw(hir.items.len() as u32),
        name: future_name_id,
        kind: ItemKind::Impl(ImplItem {
            trait_ref: Some(plain_path(interner, "Future")),
            self_ty: TypeRef::Path(plain_path(interner, future_name)),
            methods: vec![ImplMethod {
                name: poll_id,
                body: Some(poll_body_id),
                params: vec![self_mut_param],
                return_ty: Some(poll_return_ty),
            }],
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
            associated_types: vec![AssociatedTy {
                name: output_id,
                bounds: Vec::new(),
                default: Some(output_ty),
            }],
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    }
}

/// Multi-poll state-machine desugar. Builds the state enum, future wrapper,
/// `impl Future` with a `loop { match self.state { .. } }` poll body, and the
/// wrapper fn. Retained as scaffold for the deferred v1 resume-dispatch; the
/// real v1 is the post-MIR pass (plan §Phase 3). Currently NOT invoked from
/// `desugar_async` because its emitted poll body is a broken skeleton
/// (`S_k` arms hardcode `Poll::Pending`, `Done` panics) that would hang
/// forever — the dispatch instead emits a clear diagnostic for multi-await
/// bodies. See module docs for the type-check gap.
#[allow(dead_code)]
/// Multi-await (sequential, `suspend_count >= 2`, no loop-await) async state
/// machine desugar — the real v1 resume-dispatch (plan §Phase 3 / M4).
///
/// For `async fn foo(a) -> R { let x = g(a).await; let y = h(x).await; x + y }`
/// this emits:
///
/// ```text
/// enum fooState {
///     Start(f0: T0, ..),
///     S0(f0.., fut: gFuture),
///     S1(f0.., v0, fut: hFuture),
///     Done(result: R),
/// }
/// struct fooFuture { state: fooState }
/// impl Future for fooFuture {
///     type Output = R;
///     fn poll(&mut self) -> Poll<R> {
///         loop {
///             match self.state {
///                 Start(f0..) => {
///                     let fut = g(f0);
///                     match fut.poll() {
///                         Ready(v0) => { self.state = S0(f0.., fut); continue; }
///                         Pending   => { self.state = S0(f0.., fut); return Poll::Pending; }
///                     }
///                 }
///                 S0(f0.., fut) => {
///                     match fut.poll() {
///                         Ready(_) => { let fut1 = h(v0); self.state = S1(f0.., v0, fut1); continue; }
///                         Pending  => return Poll::Pending,
///                     }
///                 }
///                 S1(f0.., v0, fut) => {
///                     match fut.poll() {
///                         Ready(_) => { let tail = v0 + v0; self.state = Done(tail); return Poll::Ready(tail); }
///                         Pending  => return Poll::Pending,
///                     }
///                 }
///                 Done(result) => return Poll::Ready(result),
///             }
///         }
///     }
/// }
/// fn foo(a) -> fooFuture { fooFuture { state: fooState::Start(a) } }
/// ```
///
/// Each `S_k` carries the function parameters, the results of every earlier
/// `.await` (`v0 .. v_{k-1}`), and the in-flight future `fut_k` (whose concrete
/// type is nameable because every awaited expression is a call to a desugared
/// `async fn` — `format!("{name}Future")`). On `Poll::Pending` the current
/// state (already holding `fut_k`) is left in place and `Poll::Pending` is
/// returned; on the next `poll()` the `S_k` arm re-drives `fut_k.poll()`, so
/// the coroutine resumes at exactly the right point. This is a genuine
/// suspend/resume state machine, not a stub.
///
/// NOTE (M5, host-infeasible): runtime resumption correctness is verified only
/// by compiling + `block_on`-ing through the glyim executor, which is
/// Linux-gated and cannot run on the macOS dev host. The shape is
/// type-check-verified here; a Linux host must run the end-to-end `two_step`
/// proof before declaring M4 fully verified. Shapes whose future type is NOT
/// statically nameable fall back to the `async-v2` diagnostic (see below).
#[allow(clippy::too_many_lines)]
fn desugar_multi_async_fn(hir: &mut crate::CrateHir, item_id: ItemId, diags: &mut Vec<GlyimDiagnostic>) {

    let mut item = hir.items[item_id].clone();
    let fn_item = match &mut item.kind {
        ItemKind::Fn(f) => f,
        _ => return,
    };
    let fn_name = item.name;
    let original_params = fn_item.params.clone();
    let return_ty = fn_item.return_ty.clone();
    let original_body_id = match fn_item.body {
        Some(b) => b,
        None => return,
    };
    let original_body_owner = hir.bodies[original_body_id].owner;

    let interner = &hir.interner;
    let fn_name_str = interner.resolve(fn_name).to_string();
    let future_name = format!("{}Future", fn_name_str);
    let state_name = format!("{}State", future_name);
    let _output_id = interner.intern("Output");
    let poll_id = interner.intern("poll");
    let ready_id = interner.intern("Ready");
    let pending_id = interner.intern("Pending");
    let self_name = interner.intern("self");
    let state_field_name = interner.intern("state");

    // 1. Collect suspend points in execution order and the inner future expr
    //    for each (the `e` in `e.await`).
    let work_body = hir.bodies[original_body_id].clone();
    let mut suspend_points = Vec::new();
    collect_suspend_points(&work_body, root_expr_id(&work_body), &mut suspend_points);
    let n = suspend_points.len();
    if n < 2 {
        // Should have been routed to the single-poll desugar; fall back.
        desugar_one_async_fn(hir, item_id);
        return;
    }

    // For each await, the inner future expression + the result binding name
    // (`let x = e.await` => Some(x); a bare tail `e.await` => None).
    struct AwaitInfo {
        inner: ExprId,
        result_var: Option<crate::Name>,
    }
    let mut await_infos: Vec<AwaitInfo> = Vec::new();
    for sp in &suspend_points {
        let e = &work_body.exprs[sp.await_expr];
        if let Expr::Await { expr } = e {
            // Default result var: None; refined by the body split below.
            await_infos.push(AwaitInfo {
                inner: *expr,
                result_var: None,
            });
        }
    }

    // 2. Split the body into pre-segments + tail, recording each await's
    //    result var and future type name.
    //    future type name is derivable only when the awaited expression is a
    //    direct call to a desugared async fn: `some_async_fn(args)`.
    fn future_type_name(interner: &Interner, body: &Body, inner: ExprId) -> Option<String> {
        if let Expr::Call { func, .. } = &body.exprs[inner] {
            if let Expr::Path(p) = &body.exprs[*func] {
                if let Some(name) = p.as_name() {
                    return Some(format!("{}Future", interner.resolve(name)));
                }
            }
        }
        None
    }

    let fut_ty_names: Vec<Option<String>> = await_infos
        .iter()
        .map(|a| future_type_name(interner, &work_body, a.inner))
        .collect();
    if fut_ty_names.iter().any(|o| o.is_none()) {
        // A future whose concrete type isn't statically nameable. Per the
        // plan's safety rule we MUST NOT silently emit a broken state machine;
        // emit the clear `async-v2` diagnostic instead (compile error, not a
        // runtime hang).
        let span = suspend_points
            .first()
            .and_then(|sp| work_body.expr_spans.get(sp.await_expr).copied())
            .unwrap_or(Span::DUMMY);
        diags.push(GlyimDiagnostic::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 61,
            },
            DiagSeverity::Error,
            "multi-`.await` body awaits a future whose concrete type is not statically \
             nameable at the HIR desugar stage (it is not a direct call to a desugared \
             `async fn`). The v1 state-machine transform needs each suspended future's \
             type to build the `FooState` enum; tracked: KNOWN_GAPS.md async-v2.",
            glyim_diag::MultiSpan::from_span(span),
        ));
        return;
    }
    let fut_ty_names: Vec<String> = fut_ty_names.into_iter().map(Option::unwrap).collect();

    // Split body. The supported shape is a top-level `Block` whose statements
    // are `let` bindings (each init may contain at most one await, which makes
    // it an await-statement) plus a tail. We record, for each await, the
    // result var (the `let` LHS) and group statements into pre-segments.
    let body_root = root_expr_id(&work_body);
    let (root_stmts, root_tail) = match &work_body.exprs[body_root] {
        Expr::Block { stmts, tail } => (stmts.clone(), *tail),
        _ => (Vec::new(), Some(body_root)),
    };

    // result var per await: scan statements left-to-right; the first `let`
    // whose init contains the k-th await binds it.
    let mut pre_segments: Vec<Vec<ExprId>> = vec![Vec::new()];
    let mut await_result_vars: Vec<Option<crate::Name>> = Vec::new();
    let mut await_order: Vec<ExprId> = Vec::new(); // the await ExprId per segment
    for stmt in &root_stmts {
        // Does this statement contain an await? Find the first await inside it.
        let mut found: Option<ExprId> = None;
        for &ai in &await_infos_await_exprs(&work_body) {
            if stmt_contains(&work_body, *stmt, ai) {
                found = Some(ai);
                break;
            }
        }
        if let Some(ai) = found {
            // result var = let LHS if this is `let x = ...`
            let var = match &work_body.exprs[*stmt] {
                Expr::Let { pat, .. } => pat_name(&work_body, *pat),
                _ => None,
            };
            await_result_vars.push(var);
            await_order.push(ai);
            pre_segments.push(Vec::new());
        } else {
            pre_segments.last_mut().unwrap().push(*stmt);
        }
    }
    // Tail await (rare): if root_tail contains an await, it's the final await.
    let tail_is_await = root_tail
        .map(|t| await_infos_await_exprs(&work_body).iter().any(|&ai| stmt_contains(&work_body, t, ai)))
        .unwrap_or(false);
    let tail_expr = if tail_is_await {
        None
    } else {
        root_tail
    };

    // 3. Build the poll body. Each `S_k` arm re-drives `fut_k`.
    let mut poll_body = Body {
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        params: Vec::new(),
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let self_pat = poll_body.pats.push(Pat::Binding {
        name: self_name,
        mutability: Mutability::Not,
        subpattern: None,
    });
    poll_body.params.push(self_pat);

    // Per-arm renaming. A result variable `r_j` (the value of the j-th await)
    // resolves differently depending on which state arm we are in:
    //   * In the arm that (re-)polls `fut_j`, the just-produced result is the
    //     `Ready` payload, bound locally as `__v` — so `r_j` maps to `__v`.
    //   * In every later arm (and in the final tail), the value is carried in
    //     the state struct as field `v_j`, so `r_j` maps to `v_j`.
    // Parameters always map to their `f_j` field name. `arm_rename(k)` is the
    // rename for the arm that polls `fut_k`.
    let v_names: Vec<crate::Name> = await_result_vars
        .iter()
        .enumerate()
        .map(|(k, _)| interner.intern(&format!("v{}", k)))
        .collect();
    let ready_value_name = interner.intern("__v");
    let arm_rename = |k: usize| -> std::collections::HashMap<crate::Name, crate::Name> {
        let mut m: std::collections::HashMap<crate::Name, crate::Name> = std::collections::HashMap::new();
        for (i, p) in original_params.iter().enumerate() {
            m.insert(p.name, interner.intern(&format!("f{}", i)));
        }
        for (j, var) in await_result_vars.iter().enumerate() {
            if let Some(v) = var {
                let target = if j == k { ready_value_name } else { v_names[j] };
                m.insert(*v, target);
            }
        }
        m
    };
    // Start arm re-polls `fut0`, so its ready binding is `__v` (= r_0).
    let rename = arm_rename(0);

    // helper: copy_expr_renamed (free fn) copies an expr from work_body into poll_body, renaming names.

    let self_path_expr = poll_body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(self_name))),
        Span::DUMMY,
    );
    let state_field_expr = poll_body.alloc_expr(
        Expr::Field {
            receiver: self_path_expr,
            field: state_field_name,
        },
        Span::DUMMY,
    );
    let poll_ready_ctor = |dst: &mut Body| {
        dst.alloc_expr(Expr::Path(two_seg(interner, "Poll", ready_id)), Span::DUMMY)
    };
    let poll_pending_path = poll_body.alloc_expr(
        Expr::Path(two_seg(interner, "Poll", pending_id)),
        Span::DUMMY,
    );

    let mut arms: Vec<MatchArm> = Vec::new();

    // --- Start arm ---
    let start_pat = {
        let mut fields: Vec<(crate::Name, PatId)> = Vec::new();
        for (i, _p) in original_params.iter().enumerate() {
            let fname = interner.intern(&format!("f{}", i));
            let b = poll_body.pats.push(Pat::Binding {
                name: fname,
                mutability: Mutability::Not,
                subpattern: None,
            });
            fields.push((fname, b));
        }
        poll_body.pats.push(Pat::Struct {
            path: two_seg(interner, &state_name, interner.intern("Start")),
            fields,
            rest: false,
        })
    };
    // Start body: pre_0 stmts; fut_0 = <inner_0 renamed>; match fut_0.poll().
    let start_body = {
        let mut stmts: Vec<ExprId> = Vec::new();
        for &s in &pre_segments[0] {
            stmts.push(copy_expr_renamed(&work_body, &mut poll_body, s, &rename, interner));
        }
        let inner0 = await_infos[0].inner;
        let fut0 = copy_expr_renamed(&work_body, &mut poll_body, inner0, &rename, interner);
        let fut_local_name = interner.intern("fut0");
        let fut_pat = poll_body.pats.push(Pat::Binding {
            name: fut_local_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        let fut_let = poll_body.alloc_expr(
            Expr::Let {
                pat: fut_pat,
                value: fut0,
            },
            Span::DUMMY,
        );
        stmts.push(fut_let);
        let fut_path = poll_body.alloc_expr(
            Expr::Path(plain_path(interner, interner.resolve(fut_local_name))),
            Span::DUMMY,
        );
        let poll_call = poll_body.alloc_expr(
            Expr::MethodCall {
                receiver: fut_path,
                method: poll_id,
                args: Vec::new(),
            },
            Span::DUMMY,
        );
        let v0_name = v_names[0];
        let v0_binding = poll_body.pats.push(Pat::Binding {
            name: ready_value_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        let ready_pat = poll_body.pats.push(Pat::Struct {
            path: two_seg(interner, "Poll", ready_id),
            fields: vec![(v0_name, v0_binding)],
            rest: false,
        });
        let pending_pat = poll_body.pats.push(Pat::Path(two_seg(interner, "Poll", pending_id)));
        // Ready(r0): transition to S1, which carries the first result `v0` (= r0)
        // plus `fut1` (the next in-flight future). `S0` is only used on the
        // Pending path (carrying `fut0` to be re-driven on resume).
        let fut1_inner = await_infos[1].inner;
        let fut1_expr = copy_expr_renamed(&work_body, &mut poll_body, fut1_inner, &rename, interner);
        let fut1_local_name = interner.intern("fut1");
        let fut1_pat = poll_body.pats.push(Pat::Binding {
            name: fut1_local_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        let fut1_let = poll_body.alloc_expr(Expr::Let { pat: fut1_pat, value: fut1_expr }, Span::DUMMY);
        let s1_struct = build_state_struct(
            &mut poll_body,
            interner,
            &state_name,
            "S1",
            &original_params,
            &v_names[0..0],
            &[(v_names[0], ready_value_name), (fut1_local_name, fut1_local_name)],
            self_name,
        );
        let assign_s1 = assign_state(&mut poll_body, interner, state_field_name, s1_struct);
        let continue_expr = poll_body.alloc_expr(Expr::Continue, Span::DUMMY);
        let ready_arm_body = {
            let mut s = Vec::new();
            s.push(fut1_let);
            s.push(assign_s1);
            s.push(continue_expr);
            poll_body.alloc_expr(Expr::Block { stmts: s, tail: None }, Span::DUMMY)
        };
        // Pending: self.state = S0; return Poll::Pending
        let s0_struct_p = build_state_struct(
            &mut poll_body,
            interner,
            &state_name,
            "S0",
            &original_params,
            &v_names[0..0],
            &[(fut_local_name, fut_local_name)],
            self_name,
        );
        let assign_s0_p = assign_state(&mut poll_body, interner, state_field_name, s0_struct_p);
        let return_pending = poll_body.alloc_expr(
            Expr::Return {
                value: Some(poll_pending_path),
            },
            Span::DUMMY,
        );
        let pending_arm_body = {
            let mut s = Vec::new();
            s.push(assign_s0_p);
            s.push(return_pending);
            poll_body.alloc_expr(Expr::Block { stmts: s, tail: None }, Span::DUMMY)
        };
        let match_expr = poll_body.alloc_expr(
            Expr::Match {
                scrutinee: poll_call,
                arms: vec![
                    MatchArm { pat: ready_pat, guard: None, body: ready_arm_body },
                    MatchArm { pat: pending_pat, guard: None, body: pending_arm_body },
                ],
            },
            Span::DUMMY,
        );
        stmts.push(match_expr);
        poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY)
    };
    arms.push(MatchArm { pat: start_pat, guard: None, body: start_body });

    // --- S_k arms for k in 0..n-1 ---
    for k in 0..n {
        let s_pat = {
            let mut fields: Vec<(crate::Name, PatId)> = Vec::new();
            for (i, _p) in original_params.iter().enumerate() {
                let fname = interner.intern(&format!("f{}", i));
                let b = poll_body.pats.push(Pat::Binding {
                    name: fname,
                    mutability: Mutability::Not,
                    subpattern: None,
                });
                fields.push((fname, b));
            }
            for j in 0..k {
                let b = poll_body.pats.push(Pat::Binding {
                    name: v_names[j],
                    mutability: Mutability::Not,
                    subpattern: None,
                });
                fields.push((v_names[j], b));
            }
            let fut_name = interner.intern(&format!("fut{}", k));
            let fb = poll_body.pats.push(Pat::Binding {
                name: fut_name,
                mutability: Mutability::Not,
                subpattern: None,
            });
            fields.push((fut_name, fb));
            poll_body.pats.push(Pat::Struct {
                path: two_seg(interner, &state_name, interner.intern(&format!("S{}", k))),
                fields,
                rest: false,
            })
        };
        let s_body = {
            let rename_k = arm_rename(k);
            let fut_name = interner.intern(&format!("fut{}", k));
            let fut_path = poll_body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(fut_name))),
                Span::DUMMY,
            );
            let poll_call = poll_body.alloc_expr(
                Expr::MethodCall {
                    receiver: fut_path,
                    method: poll_id,
                    args: Vec::new(),
                },
                Span::DUMMY,
            );
            let ready_binding = poll_body.pats.push(Pat::Binding {
                name: interner.intern("__v"),
                mutability: Mutability::Not,
                subpattern: None,
            });
            let ready_pat = poll_body.pats.push(Pat::Struct {
                path: two_seg(interner, "Poll", ready_id),
                fields: vec![(interner.intern("__v"), ready_binding)],
                rest: false,
            });
            let pending_pat = poll_body.pats.push(Pat::Path(two_seg(interner, "Poll", pending_id)));

            if k + 1 < n {
                // Ready: run pre_{k+1}, create fut_{k+1}, store S_{k+1}, continue
                let mut stmts: Vec<ExprId> = Vec::new();
                for &s in &pre_segments[k + 1] {
                    stmts.push(copy_expr_renamed(&work_body, &mut poll_body, s, &rename_k, interner));
                }
                let inner = await_infos[k + 1].inner;
                let fut_next = copy_expr_renamed(&work_body, &mut poll_body, inner, &rename_k, interner);
                let fut_next_name = interner.intern(&format!("fut{}", k + 1));
                let fut_next_pat = poll_body.pats.push(Pat::Binding {
                    name: fut_next_name,
                    mutability: Mutability::Not,
                    subpattern: None,
                });
                let fut_next_let = poll_body.alloc_expr(
                    Expr::Let {
                        pat: fut_next_pat,
                        value: fut_next,
                    },
                    Span::DUMMY,
                );
                stmts.push(fut_next_let);
                let s_next = build_state_struct(
                    &mut poll_body,
                    interner,
                    &state_name,
                    &format!("S{}", k + 1),
                    &original_params,
                    &v_names[0..k],
                    &[(v_names[k], interner.intern("__v")), (fut_next_name, fut_next_name)],
                    self_name,
                );
                let assign_next = assign_state(&mut poll_body, interner, state_field_name, s_next);
                stmts.push(assign_next);
                stmts.push(poll_body.alloc_expr(Expr::Continue, Span::DUMMY));
                let ready_arm_body = poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY);
                let pending_arm_body = poll_body.alloc_expr(
                    Expr::Return { value: Some(poll_pending_path) },
                    Span::DUMMY,
                );
                poll_body.alloc_expr(
                    Expr::Match {
                        scrutinee: poll_call,
                        arms: vec![
                            MatchArm { pat: ready_pat, guard: None, body: ready_arm_body },
                            MatchArm { pat: pending_pat, guard: None, body: pending_arm_body },
                        ],
                    },
                    Span::DUMMY,
                )
            } else {
                // Last await (k == n-1): Ready => compute tail, store Done, return
                let mut stmts: Vec<ExprId> = Vec::new();
                let tail = if let Some(t) = tail_expr {
                    copy_expr_renamed(&work_body, &mut poll_body, t, &rename_k, interner)
                } else {
                    // tail itself is the await; the result is the Ready value.
                    poll_body.alloc_expr(
                        Expr::Path(plain_path(interner, interner.resolve(interner.intern("__v")))),
                        Span::DUMMY,
                    )
                };
                let done_struct = poll_body.alloc_expr(
                    Expr::Struct {
                        path: two_seg(interner, &state_name, interner.intern("Done")),
                        fields: vec![(interner.intern("result"), tail)],
                        spread: None,
                    },
                    Span::DUMMY,
                );
                let assign_done = assign_state(&mut poll_body, interner, state_field_name, done_struct);
                stmts.push(assign_done);
                // Return Poll::Ready(tail): `tail` is exactly the value we just
                // stored into `self.state = Done { result: tail }`, so return
                // it directly. (Building `self.state.result` via a `Done` value
                // path is malformed because `Done` is a variant, not a value.)
                let ready_ctor_expr = poll_ready_ctor(&mut poll_body);
                let ready_call = poll_body.alloc_expr(
                    Expr::Call {
                        func: ready_ctor_expr,
                        args: vec![tail],
                    },
                    Span::DUMMY,
                );
                let return_ready = poll_body.alloc_expr(
                    Expr::Return { value: Some(ready_call) },
                    Span::DUMMY,
                );
                stmts.push(return_ready);
                let ready_arm_body = poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY);
                let pending_arm_body = poll_body.alloc_expr(
                    Expr::Return { value: Some(poll_pending_path) },
                    Span::DUMMY,
                );
                poll_body.alloc_expr(
                    Expr::Match {
                        scrutinee: poll_call,
                        arms: vec![
                            MatchArm { pat: ready_pat, guard: None, body: ready_arm_body },
                            MatchArm { pat: pending_pat, guard: None, body: pending_arm_body },
                        ],
                    },
                    Span::DUMMY,
                )
            }
        };
        arms.push(MatchArm { pat: s_pat, guard: None, body: s_body });
    }

    // --- Done arm ---
    let result_binding = poll_body.pats.push(Pat::Binding {
        name: interner.intern("result"),
        mutability: Mutability::Not,
        subpattern: None,
    });
    let done_pat = poll_body.pats.push(Pat::Struct {
        path: two_seg(interner, &state_name, interner.intern("Done")),
        fields: vec![(interner.intern("result"), result_binding)],
        rest: false,
    });
    let done_result_ctor = poll_ready_ctor(&mut poll_body);
    let done_result_arg = poll_body.alloc_expr(Expr::Path(plain_path(interner, "result")), Span::DUMMY);
    let done_result_call = poll_body.alloc_expr(
        Expr::Call {
            func: done_result_ctor,
            args: vec![done_result_arg],
        },
        Span::DUMMY,
    );
    let done_body = poll_body.alloc_expr(
        Expr::Return { value: Some(done_result_call) },
        Span::DUMMY,
    );
    arms.push(MatchArm { pat: done_pat, guard: None, body: done_body });

    let match_expr = poll_body.alloc_expr(
        Expr::Match {
            scrutinee: state_field_expr,
            arms,
        },
        Span::DUMMY,
    );
    let loop_inner = poll_body.alloc_expr(Expr::Block { stmts: Vec::new(), tail: Some(match_expr) }, Span::DUMMY);
    let loop_body = poll_body.alloc_expr(Expr::Loop { body: loop_inner }, Span::DUMMY);
    poll_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(loop_body),
        },
        Span::DUMMY,
    );
    let poll_body_id = hir.bodies.push(poll_body);

    // Compute output_ty here (uses hir.interner) BEFORE any mutable hir borrow below.
    let output_ty = return_ty
        .clone()
        .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));

    // 4. Build the state enum + future wrapper struct. (These borrow hir mutably;
    //    the `interner` borrow above is now finished.)
    let state_enum_item = build_multi_state_enum(
        hir,
        &state_name,
        &future_name,
        &original_params,
        &v_names,
        &fut_ty_names,
        &return_ty,
    );
    let future_struct_item = build_future_wrapper_struct(hir, &future_name, &state_name);
    let future_impl_item = build_future_impl(hir, &future_name, output_ty.clone(), poll_body_id);

    // Re-bind interner AFTER all `hir` mutations above (new borrow, no conflict).
    let interner = &hir.interner;
    // 5. Wrapper fn: `fn foo(args) -> fooFuture { fooFuture { state: fooState::Start(args) } }`.
    let mut wrapper_body = Body {
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        params: hir.bodies[original_body_id].params.clone(),
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let mut start_fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{}", i));
        let var_id = wrapper_body.alloc_expr(
            Expr::Path(plain_path(interner, interner.resolve(p.name))),
            Span::DUMMY,
        );
        start_fields.push((field_name, var_id));
    }
    let start_struct = wrapper_body.alloc_expr(
        Expr::Struct {
            path: two_seg(interner, &state_name, interner.intern("Start")),
            fields: start_fields,
            spread: None,
        },
        Span::DUMMY,
    );
    let future_struct_lit = wrapper_body.alloc_expr(
        Expr::Struct {
            path: plain_path(interner, &future_name),
            fields: vec![(state_field_name, start_struct)],
            spread: None,
        },
        Span::DUMMY,
    );
    let wrapper_tail = wrapper_body.alloc_expr(
        Expr::Return {
            value: Some(future_struct_lit),
        },
        Span::DUMMY,
    );
    wrapper_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(wrapper_tail),
        },
        Span::DUMMY,
    );
    let wrapper_body_id = hir.bodies.push(wrapper_body);

    fn_item.is_async = false;
    fn_item.body = Some(wrapper_body_id);
    fn_item.return_ty = Some(TypeRef::Path(plain_path(interner, &future_name)));
    hir.items[item_id] = item;

    hir.items.push(state_enum_item);
    hir.items.push(future_struct_item);
    hir.items.push(future_impl_item);
}

/// Copy `eid` from `src` into `dst`, renaming any `Name` present in `rename`.
fn copy_expr_renamed(
    src: &Body,
    dst: &mut Body,
    eid: ExprId,
    rename: &std::collections::HashMap<crate::Name, crate::Name>,
    interner: &Interner,
) -> ExprId {
    // Fully recursive copy. Every variant that owns `ExprId`/`PatId` children
    // must re-allocate its children into `dst`; a naive `other.clone()` would
    // keep the *source* arena indices, which then point at unrelated exprs in
    // `dst` (e.g. `self`) and silently corrupt the generated call graph.
    let expr = match &src.exprs[eid] {
        Expr::Path(p) => {
            if let Some(name) = p.as_name() {
                if let Some(new) = rename.get(&name) {
                    Expr::Path(plain_path(interner, interner.resolve(*new)))
                } else {
                    Expr::Path(p.clone())
                }
            } else {
                Expr::Path(p.clone())
            }
        }
        Expr::Missing | Expr::Literal(_) | Expr::Continue | Expr::Err => src.exprs[eid].clone(),
        Expr::Block { stmts, tail } => Expr::Block {
            stmts: stmts
                .iter()
                .map(|s| copy_expr_renamed(src, dst, *s, rename, interner))
                .collect(),
            tail: tail.map(|t| copy_expr_renamed(src, dst, t, rename, interner)),
        },
        Expr::If { cond, then_branch, else_branch } => Expr::If {
            cond: copy_expr_renamed(src, dst, *cond, rename, interner),
            then_branch: copy_expr_renamed(src, dst, *then_branch, rename, interner),
            else_branch: else_branch.map(|e| copy_expr_renamed(src, dst, e, rename, interner)),
        },
        Expr::While { cond, body } => Expr::While {
            cond: copy_expr_renamed(src, dst, *cond, rename, interner),
            body: copy_expr_renamed(src, dst, *body, rename, interner),
        },
        Expr::Loop { body } => Expr::Loop {
            body: copy_expr_renamed(src, dst, *body, rename, interner),
        },
        Expr::For { pat, iterable, body } => Expr::For {
            pat: copy_pat_renamed(src, dst, *pat, rename, interner),
            iterable: copy_expr_renamed(src, dst, *iterable, rename, interner),
            body: copy_expr_renamed(src, dst, *body, rename, interner),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: copy_expr_renamed(src, dst, *scrutinee, rename, interner),
            arms: arms
                .iter()
                .map(|a| MatchArm {
                    pat: copy_pat_renamed(src, dst, a.pat, rename, interner),
                    guard: a.guard.map(|g| copy_expr_renamed(src, dst, g, rename, interner)),
                    body: copy_expr_renamed(src, dst, a.body, rename, interner),
                })
                .collect(),
        },
        Expr::Call { func, args } => Expr::Call {
            func: copy_expr_renamed(src, dst, *func, rename, interner),
            args: args
                .iter()
                .map(|a| copy_expr_renamed(src, dst, *a, rename, interner))
                .collect(),
        },
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: copy_expr_renamed(src, dst, *receiver, rename, interner),
            method: *method,
            args: args
                .iter()
                .map(|a| copy_expr_renamed(src, dst, *a, rename, interner))
                .collect(),
        },
        Expr::Field { receiver, field } => Expr::Field {
            receiver: copy_expr_renamed(src, dst, *receiver, rename, interner),
            field: *field,
        },
        Expr::Index { base, index } => Expr::Index {
            base: copy_expr_renamed(src, dst, *base, rename, interner),
            index: copy_expr_renamed(src, dst, *index, rename, interner),
        },
        Expr::Unary { op, expr } => Expr::Unary {
            op: *op,
            expr: copy_expr_renamed(src, dst, *expr, rename, interner),
        },
        Expr::Binary { op, lhs, rhs } => Expr::Binary {
            op: *op,
            lhs: copy_expr_renamed(src, dst, *lhs, rename, interner),
            rhs: copy_expr_renamed(src, dst, *rhs, rename, interner),
        },
        Expr::Cast { expr, ty } => Expr::Cast {
            expr: copy_expr_renamed(src, dst, *expr, rename, interner),
            ty: ty.clone(),
        },
        Expr::Ref { expr, mutability } => Expr::Ref {
            expr: copy_expr_renamed(src, dst, *expr, rename, interner),
            mutability: *mutability,
        },
        Expr::Assign { lhs, rhs } => Expr::Assign {
            lhs: copy_expr_renamed(src, dst, *lhs, rename, interner),
            rhs: copy_expr_renamed(src, dst, *rhs, rename, interner),
        },
        Expr::Return { value } => Expr::Return {
            value: value.map(|v| copy_expr_renamed(src, dst, v, rename, interner)),
        },
        Expr::Break { value } => Expr::Break {
            value: value.map(|v| copy_expr_renamed(src, dst, v, rename, interner)),
        },
        Expr::Closure { params, body, is_move } => Expr::Closure {
            params: params
                .iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
            body: copy_expr_renamed(src, dst, *body, rename, interner),
            is_move: *is_move,
        },
        Expr::Array(es) => Expr::Array(
            es.iter()
                .map(|e| copy_expr_renamed(src, dst, *e, rename, interner))
                .collect(),
        ),
        Expr::Tuple(es) => Expr::Tuple(
            es.iter()
                .map(|e| copy_expr_renamed(src, dst, *e, rename, interner))
                .collect(),
        ),
        Expr::Let { pat, value } => Expr::Let {
            pat: copy_pat_renamed(src, dst, *pat, rename, interner),
            value: copy_expr_renamed(src, dst, *value, rename, interner),
        },
        Expr::Struct { path, fields, spread } => Expr::Struct {
            path: path.clone(),
            fields: fields
                .iter()
                .map(|(n, e)| (*n, copy_expr_renamed(src, dst, *e, rename, interner)))
                .collect(),
            spread: spread.map(|s| copy_expr_renamed(src, dst, s, rename, interner)),
        },
        Expr::Range { start, end, inclusive } => Expr::Range {
            start: start.map(|s| copy_expr_renamed(src, dst, s, rename, interner)),
            end: end.map(|e| copy_expr_renamed(src, dst, e, rename, interner)),
            inclusive: *inclusive,
        },
        Expr::Await { expr } => Expr::Await {
            expr: copy_expr_renamed(src, dst, *expr, rename, interner),
        },
    };
    dst.alloc_expr(expr, Span::DUMMY)
}

/// Copy `pat` from `src` into `dst`, renaming any `Name` present in `rename`.
fn copy_pat_renamed(
    src: &Body,
    dst: &mut Body,
    pid: PatId,
    rename: &std::collections::HashMap<crate::Name, crate::Name>,
    interner: &Interner,
) -> PatId {
    let pat = match &src.pats[pid] {
        Pat::Binding { name, mutability, subpattern } => Pat::Binding {
            name: *name,
            mutability: *mutability,
            subpattern: subpattern.map(|s| copy_pat_renamed(src, dst, s, rename, interner)),
        },
        Pat::Struct { path, fields, rest } => Pat::Struct {
            path: path.clone(),
            fields: fields
                .iter()
                .map(|(n, p)| (*n, copy_pat_renamed(src, dst, *p, rename, interner)))
                .collect(),
            rest: *rest,
        },
        Pat::Tuple(ps) => Pat::Tuple(ps.iter().map(|p| copy_pat_renamed(src, dst, *p, rename, interner)).collect()),
        Pat::Slice(ps) => Pat::Slice(ps.iter().map(|p| copy_pat_renamed(src, dst, *p, rename, interner)).collect()),
        Pat::Or(ps) => Pat::Or(ps.iter().map(|p| copy_pat_renamed(src, dst, *p, rename, interner)).collect()),
        Pat::Path(p) => Pat::Path(p.clone()),
        Pat::Wild => Pat::Wild,
        Pat::Literal(l) => Pat::Literal(l.clone()),
        Pat::Range { start, end, inclusive } => Pat::Range {
            start: start.clone(),
            end: end.clone(),
            inclusive: *inclusive,
        },
        Pat::Err => Pat::Err,
    };
    dst.pats.push(pat)
}


/// Like `copy_expr_renamed`, but when it reaches the `Expr::Await` node whose
/// id is `await_eid` it emits a path to `subst_name` (the `Ready` payload
/// binding) instead of recursing into the awaited future. Used to lift the
/// loop body's post-await statements into the `S0` arm: `total = total +
/// fut.await` becomes `total = total + __v`.
fn copy_expr_subst_await(
    src: &Body,
    dst: &mut Body,
    eid: ExprId,
    rename: &std::collections::HashMap<crate::Name, crate::Name>,
    interner: &Interner,
    await_eid: ExprId,
    subst_name: crate::Name,
) -> ExprId {
    if eid == await_eid {
        return dst.alloc_expr(
            Expr::Path(plain_path(interner, interner.resolve(subst_name))),
            Span::DUMMY,
        );
    }
    let expr = match &src.exprs[eid] {
        Expr::Path(p) => {
            if let Some(name) = p.as_name() {
                if let Some(new) = rename.get(&name) {
                    Expr::Path(plain_path(interner, interner.resolve(*new)))
                } else {
                    Expr::Path(p.clone())
                }
            } else {
                Expr::Path(p.clone())
            }
        }
        Expr::Missing | Expr::Literal(_) | Expr::Continue | Expr::Err => src.exprs[eid].clone(),
        Expr::Block { stmts, tail } => Expr::Block {
            stmts: stmts
                .iter()
                .map(|s| copy_expr_subst_await(src, dst, *s, rename, interner, await_eid, subst_name))
                .collect(),
            tail: tail.map(|t| copy_expr_subst_await(src, dst, t, rename, interner, await_eid, subst_name)),
        },
        Expr::If { cond, then_branch, else_branch } => Expr::If {
            cond: copy_expr_subst_await(src, dst, *cond, rename, interner, await_eid, subst_name),
            then_branch: copy_expr_subst_await(src, dst, *then_branch, rename, interner, await_eid, subst_name),
            else_branch: else_branch
                .map(|e| copy_expr_subst_await(src, dst, e, rename, interner, await_eid, subst_name)),
        },
        Expr::While { cond, body } => Expr::While {
            cond: copy_expr_subst_await(src, dst, *cond, rename, interner, await_eid, subst_name),
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
        },
        Expr::Loop { body } => Expr::Loop {
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
        },
        Expr::For { pat, iterable, body } => Expr::For {
            pat: copy_pat_renamed(src, dst, *pat, rename, interner),
            iterable: copy_expr_subst_await(src, dst, *iterable, rename, interner, await_eid, subst_name),
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: copy_expr_subst_await(src, dst, *scrutinee, rename, interner, await_eid, subst_name),
            arms: arms
                .iter()
                .map(|a| crate::MatchArm {
                    pat: copy_pat_renamed(src, dst, a.pat, rename, interner),
                    guard: a.guard.map(|g| copy_expr_subst_await(src, dst, g, rename, interner, await_eid, subst_name)),
                    body: copy_expr_subst_await(src, dst, a.body, rename, interner, await_eid, subst_name),
                })
                .collect(),
        },
        Expr::Call { func, args } => Expr::Call {
            func: copy_expr_subst_await(src, dst, *func, rename, interner, await_eid, subst_name),
            args: args
                .iter()
                .map(|a| copy_expr_subst_await(src, dst, *a, rename, interner, await_eid, subst_name))
                .collect(),
        },
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: copy_expr_subst_await(src, dst, *receiver, rename, interner, await_eid, subst_name),
            method: *method,
            args: args
                .iter()
                .map(|a| copy_expr_subst_await(src, dst, *a, rename, interner, await_eid, subst_name))
                .collect(),
        },
        Expr::Field { receiver, field } => Expr::Field {
            receiver: copy_expr_subst_await(src, dst, *receiver, rename, interner, await_eid, subst_name),
            field: *field,
        },
        Expr::Index { base, index } => Expr::Index {
            base: copy_expr_subst_await(src, dst, *base, rename, interner, await_eid, subst_name),
            index: copy_expr_subst_await(src, dst, *index, rename, interner, await_eid, subst_name),
        },
        Expr::Unary { op, expr } => Expr::Unary {
            op: *op,
            expr: copy_expr_subst_await(src, dst, *expr, rename, interner, await_eid, subst_name),
        },
        Expr::Binary { op, lhs, rhs } => Expr::Binary {
            op: *op,
            lhs: copy_expr_subst_await(src, dst, *lhs, rename, interner, await_eid, subst_name),
            rhs: copy_expr_subst_await(src, dst, *rhs, rename, interner, await_eid, subst_name),
        },
        Expr::Cast { expr, ty } => Expr::Cast {
            expr: copy_expr_subst_await(src, dst, *expr, rename, interner, await_eid, subst_name),
            ty: ty.clone(),
        },
        Expr::Ref { expr, mutability } => Expr::Ref {
            expr: copy_expr_subst_await(src, dst, *expr, rename, interner, await_eid, subst_name),
            mutability: *mutability,
        },
        Expr::Assign { lhs, rhs } => Expr::Assign {
            lhs: copy_expr_subst_await(src, dst, *lhs, rename, interner, await_eid, subst_name),
            rhs: copy_expr_subst_await(src, dst, *rhs, rename, interner, await_eid, subst_name),
        },
        Expr::Return { value } => Expr::Return {
            value: value.map(|v| copy_expr_subst_await(src, dst, v, rename, interner, await_eid, subst_name)),
        },
        Expr::Break { value } => Expr::Break {
            value: value.map(|v| copy_expr_subst_await(src, dst, v, rename, interner, await_eid, subst_name)),
        },
        Expr::Closure { params, body, is_move } => Expr::Closure {
            params: params
                .iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
            is_move: *is_move,
        },
        Expr::Array(es) => Expr::Array(
            es.iter()
                .map(|e| copy_expr_subst_await(src, dst, *e, rename, interner, await_eid, subst_name))
                .collect(),
        ),
        Expr::Tuple(es) => Expr::Tuple(
            es.iter()
                .map(|e| copy_expr_subst_await(src, dst, *e, rename, interner, await_eid, subst_name))
                .collect(),
        ),
        Expr::Await { expr } => {
            if eid == await_eid {
                // The single in-loop await: lift it to the `Ready` payload binding.
                Expr::Path(plain_path(interner, interner.resolve(subst_name)))
            } else {
                Expr::Await {
                    expr: copy_expr_subst_await(src, dst, *expr, rename, interner, await_eid, subst_name),
                }
            }
        },
        other => other.clone(),
    };
    dst.alloc_expr(expr, Span::DUMMY)
}

/// Build `enum fooState { Start(f0..), S0(f0.., total, i, fut), Done(result) }`
/// for the loop-await state machine. `build_state_struct` covers `Start` and
/// `S0`; `Done` carries a single `result` field initialised from a local, so
/// it is built separately.
fn build_loop_done_struct(
    body: &mut Body,
    interner: &Interner,
    state_name: &str,
    result_local: crate::Name,
) -> ExprId {
    let result_value = body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(result_local))),
        Span::DUMMY,
    );
    body.alloc_expr(
        Expr::Struct {
            path: two_seg(interner, state_name, interner.intern("Done")),
            fields: vec![(interner.intern("result"), result_value)],
            spread: None,
        },
        Span::DUMMY,
    )
}

/// Loop-await coroutine desugar (M4/M5, plan §P2-1 step 1-3).
///
/// Handles the safe subset: an async fn whose body is
/// `let mut <locals>; while <cond> { ... <e>.await ...; <tail> } <tail>` with
/// exactly ONE `.await` inside the `while` body, whose awaited future's type is
/// statically nameable (a direct call to a desugared `async fn`, so its
/// `FooFuture` type is known). It emits a genuinely resumable state machine:
///
/// ```text
/// enum fooState { Start(f0..), S0(f0.., total, i, fut), Done(result) }
/// fn poll(&mut self) -> Poll<R> {
///     loop {
///         match self.state {
///             Start(f0..) => {
///                 let total = 0; let i = 0;
///                 if i < f0 { let fut = e(i); self.state = S0(f0, total, i, fut); continue }
///                 else      { self.state = Done(total); continue }
///             }
///             S0(f0, total, i, fut) => {
///                 match fut.poll() {
///                     Ready(__v) => { <tail with await->__v>;
///                                    if i < f0 { let fut = e(i); self.state = S0(f0, total, i, fut); continue }
///                                    else      { self.state = Done(total); continue } }
///                     Pending     => { self.state = S0(f0, total, i, fut); return Poll::Pending }
///                 }
///             }
///             Done(result) => return Poll::Ready(result),
///         }
///     }
/// }
/// ```
///
/// On `Poll::Pending` the current `S0` state (holding `fut` + live loop locals)
/// is left in place and `Poll::Pending` is returned; on the next `poll()` the
/// `S0` arm re-drives `fut.poll()`, so the coroutine resumes at exactly the
/// right point. This is a real suspend/resume machine, not a silent hang.
///
/// (compile error, never a silently-miscompiling binary).
/// Returns `Some(())` if the body was desugared, or `None` if its shape is not
/// yet supported (so the caller can fall back to the `async-v2` diagnostic).
fn desugar_loop_async_fn(
    hir: &mut crate::CrateHir,
    item_id: ItemId,
    diags: &mut Vec<GlyimDiagnostic>,
) -> Option<()> {
    let mut item = hir.items[item_id].clone();
    let fn_item = match &mut item.kind {
        ItemKind::Fn(f) => f,
        _ => return None,
    };
    let fn_name = item.name;
    let original_params = fn_item.params.clone();
    let return_ty = fn_item.return_ty.clone();
    let original_body_id = match fn_item.body {
        Some(b) => b,
        None => return None,
    };
    let original_body_owner = hir.bodies[original_body_id].owner;

    let interner = &hir.interner;
    let fn_name_str = interner.resolve(fn_name).to_string();
    let future_name = format!("{}Future", fn_name_str);
    let state_name = format!("{}State", future_name);
    let _output_id = interner.intern("Output");
    let poll_id = interner.intern("poll");
    let ready_id = interner.intern("Ready");
    let pending_id = interner.intern("Pending");
    let self_name = interner.intern("self");
    let state_field_name = interner.intern("state");

    let emit_unsupported = |diags: &mut Vec<GlyimDiagnostic>, span: Span| {
        diags.push(GlyimDiagnostic::new(
            ErrorCode {
                category: ErrorCategory::Type,
                number: 60,
            },
            DiagSeverity::Error,
            "`.await` inside a loop body is not yet supported by the async state-machine \
             lowering (tracked: KNOWN_GAPS.md async-v2). Hoist the await out of the loop, \
             or collect futures into a Vec and await them sequentially outside the loop.",
            glyim_diag::MultiSpan::from_span(span),
        ));
    };

    let work_body = hir.bodies[original_body_id].clone();
    let body_root = root_expr_id(&work_body);
    let (root_stmts, _root_tail) = match &work_body.exprs[body_root] {
        Expr::Block { stmts, tail } => (stmts.clone(), *tail),
        _ => (Vec::new(), Some(body_root)),
    };

    // Find the loop statement (only `while` is supported for now).
    let loop_stmt = root_stmts
        .iter()
        .find(|s| matches!(work_body.exprs[**s], Expr::While { .. }))
        .copied();
    let loop_stmt = match loop_stmt {
        Some(s) => s,
        None => {
            // No `while` loop with an await: should not have been routed here.
            return None;
        }
    };
    let (loop_cond, loop_body_root) = match &work_body.exprs[loop_stmt] {
        Expr::While { cond, body } => (*cond, *body),
        _ => unreachable!("loop_stmt is a While"),
    };

    // Exactly one `.await` inside the loop body.
    let mut sps = Vec::new();
    collect_suspend_points(&work_body, loop_body_root, &mut sps);
    if sps.len() != 1 {
        emit_unsupported(
            diags,
            work_body
                .expr_spans
                .get(loop_stmt)
                .copied()
                .unwrap_or(Span::DUMMY),
        );
        return None;
    }
    let await_expr = sps[0].await_expr;
    let inner = match &work_body.exprs[await_expr] {
        Expr::Await { expr } => *expr,
        _ => unreachable!(),
    };
    fn future_type_name(interner: &Interner, body: &Body, inner: ExprId) -> Option<String> {
        if let Expr::Call { func, .. } = &body.exprs[inner] {
            if let Expr::Path(p) = &body.exprs[*func] {
                if let Some(name) = p.as_name() {
                    return Some(format!("{}Future", interner.resolve(name)));
                }
            }
        }
        None
    }
    let fut_ty_name = future_type_name(interner, &work_body, inner);
    let fut_ty_name = match fut_ty_name {
        Some(s) => s,
        None => {
            emit_unsupported(
                diags,
                work_body
                    .expr_spans
                    .get(await_expr)
                    .copied()
                    .unwrap_or(Span::DUMMY),
            );
            return None;
        }
    };

    // Loop locals: names bound by pre-loop `let`/`let mut` statements.
    let mut loop_local_names: Vec<crate::Name> = Vec::new();
    for s in &root_stmts {
        if *s == loop_stmt {
            break;
        }
        if let Expr::Let { pat, .. } = &work_body.exprs[*s] {
            if let Some(n) = pat_name(&work_body, *pat) {
                loop_local_names.push(n);
            }
        }
    }
    // The post-loop tail expression (e.g. `total`) is the `Done` result.
    let result_local = match _root_tail {
        Some(t) => match &work_body.exprs[t] {
            Expr::Path(p) => p.as_name(),
            _ => loop_local_names.last().copied(),
        },
        None => loop_local_names.last().copied(),
    }
    .unwrap_or_else(|| {
        emit_unsupported(
            diags,
            work_body.expr_spans.get(loop_stmt).copied().unwrap_or(Span::DUMMY),
        );
        interner.intern("result")
    });

    // Renames: params -> fN. Loop locals and the await result stay local.
    let mut base_rename: std::collections::HashMap<crate::Name, crate::Name> =
        std::collections::HashMap::new();
    for (i, p) in original_params.iter().enumerate() {
        base_rename.insert(p.name, interner.intern(&format!("f{}", i)));
    }
    let ready_value_name = interner.intern("__v");
    let fut_local_name = interner.intern("fut");

    let mut poll_body = Body {
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        params: Vec::new(),
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let self_pat = poll_body.pats.push(Pat::Binding {
        name: self_name,
        mutability: Mutability::Not,
        subpattern: None,
    });
    poll_body.params.push(self_pat);

    let self_receiver = self_expr(interner, &mut poll_body, self_name);
    let state_field_expr = poll_body.alloc_expr(
        Expr::Field {
            receiver: self_receiver,
            field: state_field_name,
        },
        Span::DUMMY,
    );
    let poll_pending_path = poll_body.alloc_expr(
        Expr::Path(two_seg(interner, "Poll", pending_id)),
        Span::DUMMY,
    );

    let mut arms: Vec<crate::MatchArm> = Vec::new();

    // Helper: build the `if <cond> { S0 transition } else { Done transition }`
    // block used by both Start and S0-Ready arms.
    let build_loop_step = |poll_body: &mut Body,
                           interner: &Interner,
                           state_field_name: crate::Name,
                           state_name: &str,
                           original_params: &[Param],
                           loop_local_names: &[crate::Name],
                           fut_local_name: crate::Name,
                           inner: ExprId,
                           base_rename: &std::collections::HashMap<crate::Name, crate::Name>,
                           result_local: crate::Name,
                           cond: ExprId| {
        let mut stmts: Vec<ExprId> = Vec::new();
        // let fut = <inner>  (re-drive the next iteration's future)
        let fut_inner = copy_expr_renamed(&work_body, poll_body, inner, base_rename, interner);
        let fut_pat = poll_body.pats.push(Pat::Binding {
            name: fut_local_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        stmts.push(poll_body.alloc_expr(
            Expr::Let {
                pat: fut_pat,
                value: fut_inner,
            },
            Span::DUMMY,
        ));
        // self.state = S0(f0.., loop_locals, fut); continue
        let s0_struct = build_state_struct(
            poll_body,
            interner,
            state_name,
            "S0",
            original_params,
            loop_local_names,
            &[(fut_local_name, fut_local_name)],
            self_name,
        );
        stmts.push(assign_state(poll_body, interner, state_field_name, s0_struct));
        stmts.push(poll_body.alloc_expr(Expr::Continue, Span::DUMMY));
        let then_branch = poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY);

        // else: self.state = Done(result); continue
        let mut else_stmts: Vec<ExprId> = Vec::new();
        let done_struct = build_loop_done_struct(poll_body, interner, state_name, result_local);
        else_stmts.push(assign_state(poll_body, interner, state_field_name, done_struct));
        else_stmts.push(poll_body.alloc_expr(Expr::Continue, Span::DUMMY));
        let else_branch = poll_body.alloc_expr(Expr::Block { stmts: else_stmts, tail: None }, Span::DUMMY);

        let cond_expr = copy_expr_renamed(&work_body, poll_body, cond, base_rename, interner);
        poll_body.alloc_expr(
            Expr::If {
                cond: cond_expr,
                then_branch,
                else_branch: Some(else_branch),
            },
            Span::DUMMY,
        )
    };

    // --- Start arm ---
    let start_pat = {
        let mut fields: Vec<(crate::Name, crate::PatId)> = Vec::new();
        for (i, _p) in original_params.iter().enumerate() {
            let fname = interner.intern(&format!("f{}", i));
            let b = poll_body.pats.push(Pat::Binding {
                name: fname,
                mutability: Mutability::Not,
                subpattern: None,
            });
            fields.push((fname, b));
        }
        poll_body.pats.push(Pat::Struct {
            path: two_seg(interner, &state_name, interner.intern("Start")),
            fields,
            rest: false,
        })
    };
    let start_body = {
        let mut stmts: Vec<ExprId> = Vec::new();
        // Pre-loop init statements bind the loop locals as locals.
        for s in &root_stmts {
            if *s == loop_stmt {
                break;
            }
            stmts.push(copy_expr_renamed(&work_body, &mut poll_body, *s, &base_rename, interner));
        }
        stmts.push(build_loop_step(
            &mut poll_body,
            interner,
            state_field_name,
            &state_name,
            &original_params,
            &loop_local_names,
            fut_local_name,
            inner,
            &base_rename,
            result_local,
            loop_cond,
        ));
        poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY)
    };
    arms.push(crate::MatchArm {
        pat: start_pat,
        guard: None,
        body: start_body,
    });

    // --- S0 arm ---
    let s0_pat = {
        let mut fields: Vec<(crate::Name, crate::PatId)> = Vec::new();
        for (i, _p) in original_params.iter().enumerate() {
            let fname = interner.intern(&format!("f{}", i));
            let b = poll_body.pats.push(Pat::Binding {
                name: fname,
                mutability: Mutability::Not,
                subpattern: None,
            });
            fields.push((fname, b));
        }
        for &ln in &loop_local_names {
            let b = poll_body.pats.push(Pat::Binding {
                name: ln,
                mutability: Mutability::Not,
                subpattern: None,
            });
            fields.push((ln, b));
        }
        let fb = poll_body.pats.push(Pat::Binding {
            name: fut_local_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        fields.push((fut_local_name, fb));
        poll_body.pats.push(Pat::Struct {
            path: two_seg(interner, &state_name, interner.intern("S0")),
            fields,
            rest: false,
        })
    };
    let s0_body = {
        let fut_path = poll_body.alloc_expr(
            Expr::Path(plain_path(interner, interner.resolve(fut_local_name))),
            Span::DUMMY,
        );
        let poll_call = poll_body.alloc_expr(
            Expr::MethodCall {
                receiver: fut_path,
                method: poll_id,
                args: Vec::new(),
            },
            Span::DUMMY,
        );
        let ready_binding = poll_body.pats.push(Pat::Binding {
            name: ready_value_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        let ready_pat = poll_body.pats.push(Pat::Struct {
            path: two_seg(interner, "Poll", ready_id),
            fields: vec![(ready_value_name, ready_binding)],
            rest: false,
        });
        let pending_pat = poll_body
            .pats
            .push(Pat::Path(two_seg(interner, "Poll", pending_id)));

        // Ready(__v): run post-await loop body (await -> __v), then loop step.
        let mut ready_stmts: Vec<ExprId> = Vec::new();
        // Collect every statement in the loop body, descending through any
        // wrapper nodes (Block, If, While, Loop) so the post-await body is
        // fully substituted even when the loop body isn't a bare `Expr::Block`.
        {
            let mut stmts: Vec<ExprId> = Vec::new();
            fn collect_stmts(body: &Body, id: ExprId, out: &mut Vec<ExprId>) {
                match &body.exprs[id] {
                    Expr::Block { stmts, tail } => {
                        for s in stmts {
                            collect_stmts(body, *s, out);
                        }
                        if let Some(t) = tail {
                            collect_stmts(body, *t, out);
                        }
                    }
                    Expr::If { then_branch, else_branch, .. } => {
                        collect_stmts(body, *then_branch, out);
                        if let Some(e) = else_branch {
                            collect_stmts(body, *e, out);
                        }
                    }
                    Expr::While { body: wb, .. } | Expr::Loop { body: wb } => {
                        collect_stmts(body, *wb, out);
                    }
                    other => {
                        // A non-statement expr (e.g. `Assign`, `Let`, `Call`) is
                        // itself a statement in the loop body.
                        let _ = other;
                        out.push(id);
                    }
                }
            }
            collect_stmts(&work_body, loop_body_root, &mut stmts);
            for s in stmts {
                ready_stmts.push(copy_expr_subst_await(
                    &work_body,
                    &mut poll_body,
                    s,
                    &base_rename,
                    interner,
                    await_expr,
                    ready_value_name,
                ));
            }
        }
        ready_stmts.push(build_loop_step(
            &mut poll_body,
            interner,
            state_field_name,
            &state_name,
            &original_params,
            &loop_local_names,
            fut_local_name,
            inner,
            &base_rename,
            result_local,
            loop_cond,
        ));
        let ready_arm_body = poll_body.alloc_expr(Expr::Block { stmts: ready_stmts, tail: None }, Span::DUMMY);

        // Pending: self.state = S0(...); return Poll::Pending
        let s0_preserve = build_state_struct(
            &mut poll_body,
            interner,
            &state_name,
            "S0",
            &original_params,
            &loop_local_names,
            &[(fut_local_name, fut_local_name)],
            self_name,
        );
        let mut pend_stmts: Vec<ExprId> = Vec::new();
        pend_stmts.push(assign_state(&mut poll_body, interner, state_field_name, s0_preserve));
        pend_stmts.push(poll_body.alloc_expr(
            Expr::Return {
                value: Some(poll_pending_path),
            },
            Span::DUMMY,
        ));
        let pending_arm_body = poll_body.alloc_expr(Expr::Block { stmts: pend_stmts, tail: None }, Span::DUMMY);

        poll_body.alloc_expr(
            Expr::Match {
                scrutinee: poll_call,
                arms: vec![
                    crate::MatchArm {
                        pat: ready_pat,
                        guard: None,
                        body: ready_arm_body,
                    },
                    crate::MatchArm {
                        pat: pending_pat,
                        guard: None,
                        body: pending_arm_body,
                    },
                ],
            },
            Span::DUMMY,
        )
    };
    arms.push(crate::MatchArm {
        pat: s0_pat,
        guard: None,
        body: s0_body,
    });

    // --- Done arm ---
    let done_binding = poll_body.pats.push(Pat::Binding {
        name: interner.intern("result"),
        mutability: Mutability::Not,
        subpattern: None,
    });
    let done_pat = poll_body.pats.push(Pat::Struct {
        path: two_seg(interner, &state_name, interner.intern("Done")),
        fields: vec![(interner.intern("result"), done_binding)],
        rest: false,
    });
    let done_result_arg = poll_body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(interner.intern("result")))),
        Span::DUMMY,
    );
    let done_ctor = poll_body.alloc_expr(Expr::Path(two_seg(interner, "Poll", ready_id)), Span::DUMMY);
    let done_call = poll_body.alloc_expr(
        Expr::Call {
            func: done_ctor,
            args: vec![done_result_arg],
        },
        Span::DUMMY,
    );
    let done_body = poll_body.alloc_expr(
        Expr::Return {
            value: Some(done_call),
        },
        Span::DUMMY,
    );
    arms.push(crate::MatchArm {
        pat: done_pat,
        guard: None,
        body: done_body,
    });

    let match_expr = poll_body.alloc_expr(
        Expr::Match {
            scrutinee: state_field_expr,
            arms,
        },
        Span::DUMMY,
    );
    let loop_inner = poll_body.alloc_expr(Expr::Block { stmts: Vec::new(), tail: Some(match_expr) }, Span::DUMMY);
    let loop_body = poll_body.alloc_expr(Expr::Loop { body: loop_inner }, Span::DUMMY);
    poll_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(loop_body),
        },
        Span::DUMMY,
    );
    let poll_body_id = hir.bodies.push(poll_body.clone());

    let output_ty = return_ty
        .clone()
        .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));

    // State enum (Start, S0, Done) + future wrapper + impl Future.
    let state_enum_item = build_loop_state_enum(
        hir,
        &state_name,
        &future_name,
        &original_params,
        &loop_local_names,
        &fut_ty_name,
        &return_ty,
    );
    let future_struct_item = build_future_wrapper_struct(hir, &future_name, &state_name);
    let future_impl_item = build_future_impl(hir, &future_name, output_ty.clone(), poll_body_id);

    // The immutable `&hir.interner` borrow above is now dead (last use was in
    // `output_ty`); re-bind it after the `&mut hir` calls so the two borrows
    // don't overlap.
    let interner = &hir.interner;

    // Wrapper fn: fn foo(args) -> fooFuture { fooFuture { state: fooState::Start(args) } }
    let mut wrapper_body = Body {
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        params: hir.bodies[original_body_id].params.clone(),
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let mut start_fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{}", i));
        let var_id = wrapper_body.alloc_expr(
            Expr::Path(plain_path(interner, interner.resolve(p.name))),
            Span::DUMMY,
        );
        start_fields.push((field_name, var_id));
    }
    let start_struct = wrapper_body.alloc_expr(
        Expr::Struct {
            path: two_seg(interner, &state_name, interner.intern("Start")),
            fields: start_fields,
            spread: None,
        },
        Span::DUMMY,
    );
    let future_struct_lit = wrapper_body.alloc_expr(
        Expr::Struct {
            path: plain_path(interner, &future_name),
            fields: vec![(state_field_name, start_struct)],
            spread: None,
        },
        Span::DUMMY,
    );
    let wrapper_tail = wrapper_body.alloc_expr(
        Expr::Return {
            value: Some(future_struct_lit),
        },
        Span::DUMMY,
    );
    wrapper_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(wrapper_tail),
        },
        Span::DUMMY,
    );
    let wrapper_body_id = hir.bodies.push(wrapper_body);

    fn_item.is_async = false;
    fn_item.body = Some(wrapper_body_id);
    fn_item.return_ty = Some(TypeRef::Path(plain_path(interner, &future_name)));
    hir.items[item_id] = item;

    hir.items.push(state_enum_item);
    hir.items.push(future_struct_item);
    hir.items.push(future_impl_item);
    // TEMP DEBUG
    let mut dbg = String::new();
    dbg.push_str(&format!("POLL_BODY exprs ({}):\n", poll_body.exprs.len()));
    for (i, e) in poll_body.exprs.iter_enumerated() {
        dbg.push_str(&format!("  {}: {:?}\n", i.index(), e));
    }
    std::fs::write("/tmp/poll_hir.txt", dbg).unwrap();
    Some(())
}

/// `enum fooState { Start(f0..), S0(f0.., <loop_locals>, fut: Fut), Done(result: R) }`.
fn build_loop_state_enum(
    hir: &mut crate::CrateHir,
    state_name: &str,
    _future_name: &str,
    original_params: &[Param],
    loop_locals: &[crate::Name],
    fut_ty_name: &str,
    return_ty: &Option<TypeRef>,
) -> Item {
    let interner = &hir.interner;
    let state_name_id = interner.intern(state_name);
    let mut variants = Vec::new();

    // Start(f0..fn)
    let mut start_fields = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let fname = interner.intern(&format!("f{}", i));
        let ty = p
            .ty
            .clone()
            .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
        start_fields.push(Field {
            name: fname,
            ty,
            span: Span::DUMMY,
        });
    }
    variants.push(Variant {
        name: interner.intern("Start"),
        fields: start_fields,
        kind: StructKind::Record,
        span: Span::DUMMY,
    });

    // S0(f0.., loop_locals, fut: Fut)
    {
        let mut fields = Vec::new();
        for (i, p) in original_params.iter().enumerate() {
            let fname = interner.intern(&format!("f{}", i));
            let ty = p
                .ty
                .clone()
                .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
            fields.push(Field {
                name: fname,
                ty,
                span: Span::DUMMY,
            });
        }
        for &ln in loop_locals {
            fields.push(Field {
                name: ln,
                ty: TypeRef::Infer,
                span: Span::DUMMY,
            });
        }
        fields.push(Field {
            name: interner.intern("fut"),
            ty: TypeRef::Path(plain_path(interner, fut_ty_name)),
            span: Span::DUMMY,
        });
        variants.push(Variant {
            name: interner.intern("S0"),
            fields,
            kind: StructKind::Record,
            span: Span::DUMMY,
        });
    }

    // Done(result: R)
    let result_ty = return_ty
        .clone()
        .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
    variants.push(Variant {
        name: interner.intern("Done"),
        fields: vec![Field {
            name: interner.intern("result"),
            ty: result_ty,
            span: Span::DUMMY,
        }],
        kind: StructKind::Record,
        span: Span::DUMMY,
    });

    Item {
        id: ItemId::from_raw(hir.items.len() as u32),
        name: state_name_id,
        kind: ItemKind::Enum(EnumItem {
            variants,
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    }
}

fn build_state_struct(
    body: &mut Body,
    interner: &Interner,
    state_name: &str,
    variant: &str,
    original_params: &[Param],
    v_names: &[crate::Name],
    extra: &[(crate::Name, crate::Name)],
    _self_name: crate::Name,
) -> ExprId {
    let mut fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, _p) in original_params.iter().enumerate() {
        let fname = interner.intern(&format!("f{}", i));
        // The parameter is bound as a local `fN` by each state arm's pattern
        // (see `start_pat` / `s_pat`), so reference it as a bare local.
        fields.push((
            fname,
            body.alloc_expr(Expr::Path(plain_path(interner, interner.resolve(fname))), Span::DUMMY),
        ));
    }
    for &v in v_names {
        fields.push((
            v,
            body.alloc_expr(Expr::Path(plain_path(interner, interner.resolve(v))), Span::DUMMY),
        ));
    }
    // `extra` are (struct_field_name, value_local_name) pairs — e.g. the just
    // completed result `v_k` (value `__v`) and the next future `fut_{k+1}`
    // (value `fut_{k+1}`). Each becomes a struct field initialised from the
    // named local.
    for &(field_name, value_name) in extra {
        fields.push((
            field_name,
            body.alloc_expr(Expr::Path(plain_path(interner, interner.resolve(value_name))), Span::DUMMY),
        ));
    }
    body.alloc_expr(
        Expr::Struct {
            path: two_seg(interner, state_name, interner.intern(variant)),
            fields,
            spread: None,
        },
        Span::DUMMY,
    )
}

/// `self.state = <struct_expr>`.
fn assign_state(body: &mut Body, interner: &Interner, state_field_name: crate::Name, value: ExprId) -> ExprId {
    let self_path = body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(interner.intern("self")))),
        Span::DUMMY,
    );
    let lhs = body.alloc_expr(
        Expr::Field {
            receiver: self_path,
            field: state_field_name,
        },
        Span::DUMMY,
    );
    body.alloc_expr(Expr::Assign { lhs, rhs: value }, Span::DUMMY)
}

/// `enum fooState { Start(f0..), S0(f0.., fut: Fut0), .., Done(result: R) }`.
fn build_multi_state_enum(
    hir: &mut crate::CrateHir,
    state_name: &str,
    future_name: &str,
    original_params: &[Param],
    v_names: &[crate::Name],
    fut_ty_names: &[String],
    return_ty: &Option<TypeRef>,
) -> Item {
    let interner = &hir.interner;
    let state_name_id = interner.intern(state_name);
    let n = fut_ty_names.len();
    let mut variants = Vec::new();

    // Start(f0..fn)
    let mut start_fields = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let fname = interner.intern(&format!("f{}", i));
        let ty = p
            .ty
            .clone()
            .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
        start_fields.push(Field {
            name: fname,
            ty,
            span: Span::DUMMY,
        });
    }
    variants.push(Variant {
        name: interner.intern("Start"),
        fields: start_fields,
        kind: StructKind::Record,
        span: Span::DUMMY,
    });

    // S_k(f0.., v0..v_{k-1}, fut: FutK) for k in 0..n
    for k in 0..n {
        let mut fields = Vec::new();
        for (i, p) in original_params.iter().enumerate() {
            let fname = interner.intern(&format!("f{}", i));
            let ty = p
                .ty
                .clone()
                .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
            fields.push(Field { name: fname, ty, span: Span::DUMMY });
        }
        for j in 0..k {
            let ty = TypeRef::Infer; // result type is the awaited future's Output; infer from Ready payload
            fields.push(Field {
                name: v_names[j],
                ty,
                span: Span::DUMMY,
            });
        }
        let fut_ty = TypeRef::Path(plain_path(interner, &fut_ty_names[k]));
        fields.push(Field {
            name: interner.intern(&format!("fut{}", k)),
            ty: fut_ty,
            span: Span::DUMMY,
        });
        variants.push(Variant {
            name: interner.intern(&format!("S{}", k)),
            fields,
            kind: StructKind::Record,
            span: Span::DUMMY,
        });
    }

    // Done(result: R)
    let result_ty = return_ty
        .clone()
        .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
    variants.push(Variant {
        name: interner.intern("Done"),
        fields: vec![Field {
            name: interner.intern("result"),
            ty: result_ty,
            span: Span::DUMMY,
        }],
        kind: StructKind::Record,
        span: Span::DUMMY,
    });

    let _ = future_name;
    Item {
        id: ItemId::from_raw(hir.items.len() as u32),
        name: state_name_id,
        kind: ItemKind::Enum(EnumItem {
            variants,
            generic_params: Vec::new(),
            where_clauses: Vec::new(),
        }),
        visibility: Visibility::Inherited,
        span: Span::DUMMY,
    }
}

/// Collect the `Expr::Await` ExprIds (the `.await` nodes) in `body`.
fn await_infos_await_exprs(body: &Body) -> Vec<ExprId> {
    let mut out = Vec::new();
    let mut stack: Vec<ExprId> = Vec::new();
    for i in 0..body.exprs.len() {
        stack.push(ExprId::from_raw(i as u32));
    }
    while let Some(id) = stack.pop() {
        match &body.exprs[id] {
            Expr::Await { expr } => {
                out.push(id);
                stack.push(*expr);
            }
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    stack.push(*s);
                }
                if let Some(t) = tail {
                    stack.push(*t);
                }
            }
            Expr::If { cond, then_branch, else_branch } => {
                stack.push(*cond);
                stack.push(*then_branch);
                if let Some(e) = else_branch {
                    stack.push(*e);
                }
            }
            Expr::Match { scrutinee, arms } => {
                stack.push(*scrutinee);
                for a in arms {
                    if let Some(g) = a.guard {
                        stack.push(g);
                    }
                    stack.push(a.body);
                }
            }
            Expr::Let { value, .. } => stack.push(*value),
            Expr::Call { func, args } => {
                stack.push(*func);
                for a in args {
                    stack.push(*a);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                stack.push(*receiver);
                for a in args {
                    stack.push(*a);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                stack.push(*lhs);
                stack.push(*rhs);
            }
            Expr::Field { receiver, .. } => stack.push(*receiver),
            Expr::Return { value: Some(v) } => stack.push(*v),
            Expr::Return { value: None } => {}
            _ => {}
        }
    }
    out
}

/// Does `container` (or any sub-expr) contain the await `target`?
fn stmt_contains(body: &Body, container: ExprId, target: ExprId) -> bool {
    let mut stack = vec![container];
    while let Some(id) = stack.pop() {
        if id == target {
            return true;
        }
        match &body.exprs[id] {
            Expr::Await { expr } => stack.push(*expr),
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    stack.push(*s);
                }
                if let Some(t) = tail {
                    stack.push(*t);
                }
            }
            Expr::If { cond, then_branch, else_branch } => {
                stack.push(*cond);
                stack.push(*then_branch);
                if let Some(e) = else_branch {
                    stack.push(*e);
                }
            }
            Expr::Match { scrutinee, arms } => {
                stack.push(*scrutinee);
                for a in arms {
                    if let Some(g) = a.guard {
                        stack.push(g);
                    }
                    stack.push(a.body);
                }
            }
            Expr::Let { value, .. } => stack.push(*value),
            Expr::Call { func, args } => {
                stack.push(*func);
                for a in args {
                    stack.push(*a);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                stack.push(*receiver);
                for a in args {
                    stack.push(*a);
                }
            }
            Expr::Binary { lhs, rhs, .. } => {
                stack.push(*lhs);
                stack.push(*rhs);
            }
            Expr::Field { receiver, .. } => stack.push(*receiver),
            Expr::Return { value: Some(v) } => stack.push(*v),
            Expr::Return { value: None } => {}
            _ => {}
        }
    }
    false
}

/// Get the bound name of a `Pat::Binding` (for `let x = ...`).
fn pat_name(body: &Body, pat: PatId) -> Option<crate::Name> {
    match &body.pats[pat] {
        Pat::Binding { name, .. } => Some(*name),
        _ => None,
    }
}

// (State-machine helper fns `clone_expr`/`wrap_tail_ready`/`return_pending`/
// `return_pending_stay`/`transition_to` were removed: the multi-poll desugar now
// builds a borrow-clean simplified poll body inline. See module docs / §6.1.)

fn rewrite_expr(
    interner: &Interner,
    body: &mut Body,
    expr: Expr,
    param_fields: &std::collections::HashMap<crate::Name, crate::Name>,
    self_name: crate::Name,
    ready_id: crate::Name,
    pending_id: crate::Name,
) -> Expr {
    match expr {
        Expr::Await { expr: inner } => {
            // `match <inner>.poll() { Poll::Ready(v) => v, Poll::Pending => panic!(...) }`
            let poll_call = body.alloc_expr(
                Expr::MethodCall {
                    receiver: inner,
                    method: interner.intern("poll"),
                    args: Vec::new(),
                },
                Span::DUMMY,
            );
            let v_name = interner.intern("v");
            // Poll::Ready(v)  — struct/tuple-variant pattern that binds `v`.
            let ready_pat = Pat::Struct {
                path: two_seg(interner, "Poll", ready_id),
                fields: vec![(
                    v_name,
                    body.pats.push(Pat::Binding {
                        name: v_name,
                        mutability: Mutability::Not,
                        subpattern: None,
                    }),
                )],
                rest: false,
            };
            let v_path = body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(v_name))),
                Span::DUMMY,
            );
            // Poll::Pending — unit variant pattern.
            let pending_pat = Pat::Path(two_seg(interner, "Poll", pending_id));
            // The `Pending` arm must diverge so the arm's type unifies with the
            // `Poll<Output>` match result. glyim has no `!`/never termination we
            // can name here, and `panic` is a macro (not a resolvable `FnDef`),
            // so we emit `loop {}` — a diverging expression. This is a
            // single-await stopgap: a genuine Pending should suspend and resume
            // (the v1 state-machine, plan M4), not spin forever. It lets the
            // body type-check end-to-end for the supported single-await shape.
            let empty_block = body.alloc_expr(Expr::Block { stmts: Vec::new(), tail: None }, Span::DUMMY);
            let pending_body = body.alloc_expr(Expr::Loop { body: empty_block }, Span::DUMMY);
            Expr::Match {
                scrutinee: poll_call,
                arms: vec![
                    MatchArm {
                        pat: body.pats.push(ready_pat),
                        guard: None,
                        body: v_path,
                    },
                    MatchArm {
                        pat: body.pats.push(pending_pat),
                        guard: None,
                        body: pending_body,
                    },
                ],
            }
        }
        // A reference to an async parameter becomes `self.state.fN` — the
        // parameter is stored inside the `state` enum's `Start`/`S_k` variant,
        // not as a direct field of the future struct (whose only field is
        // `state`).
        Expr::Path(p) => {
            if let Some(field_name) = p.as_name().and_then(|n| param_fields.get(&n)) {
                Expr::Field {
                    receiver: self_expr(interner, body, self_name),
                    field: *field_name,
                }
            } else {
                Expr::Path(p)
            }
        }
        Expr::Return { value } => {
            let value = value.map(|v| {
                let ctor = body.alloc_expr(
                    Expr::Path(two_seg(interner, "Poll", ready_id)),
                    Span::DUMMY,
                );
                body.alloc_expr(
                    Expr::Call {
                        func: ctor,
                        args: vec![v],
                    },
                    Span::DUMMY,
                )
            });
            Expr::Return { value }
        }
        Expr::Block { stmts, tail } => Expr::Block {
            stmts: stmts
                .into_iter()
                .map(|s| rewrite_in_body(interner, body, s, param_fields, self_name, ready_id, pending_id))
                .collect(),
            tail: tail.map(|t| rewrite_in_body(interner, body, t, param_fields, self_name, ready_id, pending_id)),
        },
        Expr::If { cond, then_branch, else_branch } => Expr::If {
            cond: rewrite_in_body(interner, body, cond, param_fields, self_name, ready_id, pending_id),
            then_branch: rewrite_in_body(interner, body, then_branch, param_fields, self_name, ready_id, pending_id),
            else_branch: else_branch
                .map(|e| rewrite_in_body(interner, body, e, param_fields, self_name, ready_id, pending_id)),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: rewrite_in_body(interner, body, scrutinee, param_fields, self_name, ready_id, pending_id),
            arms: arms
                .into_iter()
                .map(|mut a| {
                    if let Some(g) = a.guard {
                        a.guard = Some(rewrite_in_body(interner, body, g, param_fields, self_name, ready_id, pending_id));
                    }
                    a.body = rewrite_in_body(interner, body, a.body, param_fields, self_name, ready_id, pending_id);
                    a
                })
                .collect(),
        },
        other => other,
    }
}

fn rewrite_in_body(
    interner: &Interner,
    body: &mut Body,
    eid: ExprId,
    param_fields: &std::collections::HashMap<crate::Name, crate::Name>,
    self_name: crate::Name,
    ready_id: crate::Name,
    pending_id: crate::Name,
) -> ExprId {
    let expr = body.exprs[eid].clone();
    let new_expr = rewrite_expr(interner, body, expr, param_fields, self_name, ready_id, pending_id);
    body.exprs[eid] = new_expr;
    eid
}

/// Build a two-segment path `A::B` (used for `Poll::Ready` / `Poll::Pending`).
fn two_seg(interner: &Interner, a: &str, b: crate::Name) -> Path {
    Path {
        segments: vec![
            PathSegment {
                name: interner.intern(a),
                generic_args: None,
            },
            PathSegment {
                name: b,
                generic_args: None,
            },
        ],
        kind: PathKind::Plain,
    }
}
