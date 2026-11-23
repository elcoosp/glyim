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

use crate::{GsxAttrHIR, GsxAttrValue, GsxChildHIR, GsxExpr, PatId};
use glyim_core::arena::IndexVec;
use glyim_core::interner::Interner;
use glyim_core::primitives::{Mutability, StructKind, Visibility};
use glyim_span::Span;

use crate::{
    AssociatedTy, Body, BodyId, EnumItem, Expr, ExprId, Field, ImplItem, ImplMethod, Item, ItemId,
    ItemKind, Literal, MatchArm, Param, Pat, Path, PathKind, PathSegment, StructItem, TypeRef,
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
        let suspend_count = body_id.map_or(0, |b| {
            let mut sps = Vec::new();
            collect_suspend_points(&hir.bodies[b], root_expr_id(&hir.bodies[b]), &mut sps);
            sps.len()
        });

        // Phase 3 (GLYIM_DESTUB_PLAN): the v1 state-machine transform supports
        // `.await` inside a `while` body, inside a `for i in a..b` body
        // (normalized into the while shape by `find_and_normalize_loop_stmt`,
        // the GAP-0001 closure) and — since the GAP-0006 `loop`-shape closure —
        // inside an unbounded `loop` body (normalized into `while true` in
        // place). The remaining loop shapes (`for` over a non-range iterable,
        // destructuring loop patterns) cannot be re-driven across a
        // `Poll::Pending` yet, and the single most important correctness guard
        // is to NEVER silently fall through to the old skeleton (whose `S_k`
        // arms hardcode `Poll::Pending` and produce an infinite-`Pending`
        // miscompile that hangs forever). Instead we emit a clear compile-time
        // diagnostic and route to the single-poll desugar, which at least turns
        // each `Pending` into a loud `panic!` rather than a silent infinite
        // loop.
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
            // shapes (a single `.await` inside a `while` body, inside a `for`
            // body over a range, or inside an unbounded `loop` body, with a
            // statically nameable future type) and `None` for unsupported
            // shapes.
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
            let span = body_id
                .and_then(|bid| {
                    await_expr.and_then(|eid| hir.bodies[bid].expr_spans.get(eid).copied())
                })
                .unwrap_or(Span::DUMMY);
            diags.push(GlyimDiagnostic::new(
                ErrorCode::from_registered(ErrorCategory::Type, 60),
                DiagSeverity::Error,
                "`.await` inside a loop body is not yet supported by the async state-machine \
                 lowering for this shape (`for` over a non-range iterable, destructuring \
                 loop patterns; tracked: KNOWN_GAPS.md async-v2). Hoist the \
                 await out of the loop, or collect futures into a Vec and await them \
                 sequentially outside the loop.",
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
                out.push(SuspendPoint { await_expr: id });
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
            Expr::For {
                iterable, body: fb, ..
            } => {
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
        .filter(|&id| {
            matches!(
                body.exprs[id],
                Expr::While { .. } | Expr::Loop { .. } | Expr::For { .. }
            )
        })
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
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
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
            Expr::For {
                iterable, body: fb, ..
            } => {
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
    let future_name = format!("{fn_name_str}Future");
    let future_name_id = interner.intern(&future_name);
    let output_id = interner.intern("Output");
    let poll_id = interner.intern("poll");
    let ready_id = interner.intern("Ready");
    let pending_id = interner.intern("Pending");

    // ---- 1. Future struct: one field per captured parameter ----
    let mut fields = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{i}"));
        let ty =
            p.ty.clone()
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
    let mut param_fields: std::collections::HashMap<crate::Name, crate::Name> =
        std::collections::HashMap::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{i}"));
        param_fields.insert(p.name, field_name);
    }
    let self_name = interner.intern("self");
    let mut poll_body = {
        let original_body = &hir.bodies[original_body_id];
        let mut b = Body {
            unsafe_blocks: std::collections::HashSet::new(),
            owner: original_body.owner,
            exprs: original_body.exprs.clone(),
            pats: IndexVec::new(),
            pat_spans: IndexVec::new(),
            params: Vec::new(),
            span: original_body.span,
            expr_spans: original_body.expr_spans.clone(),
        };
        // The cloned `exprs` arena still contains `Expr::Let { pat: PatId(k) }`
        // nodes that reference the original body's `pats` by index. Pushing the
        // `self` PatId *before* the original pats shifts all those indices,
        // causing `let x = ...` to bind `self` and every later `x` reference to
        // be unresolved. Instead, preserve the original pat ordering verbatim,
        // then append the synthesized `self` binding at a fresh index and
        // register it as the sole poll parameter.
        for p in original_body.pats.iter() {
            b.pats.push(p.clone());
        }
        let self_pat = b.pats.push(Pat::Binding {
            name: self_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        b.params.push(self_pat);
        b
    };
    rewrite_for_poll(
        interner,
        &mut poll_body,
        &param_fields,
        self_name,
        ready_id,
        pending_id,
    );
    let poll_body_owner = poll_body.owner;
    let poll_body_id = hir.bodies.push(poll_body);
    // Keep `body_owners` aligned with `bodies` (indexed by BodyId).
    // The desugar allocates bodies beyond the parser-lowered range,
    // so typeck's `body_owners[bid]` lookups would otherwise OOB.
    hir.body_owners.push(poll_body_owner);

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
        generic_params: Vec::new(),
        return_ty: Some(poll_return_ty),
        is_unsafe: false,
        attrs: Vec::new(),
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
        unsafe_blocks: std::collections::HashSet::new(),
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        pat_spans: IndexVec::new(),
        params: original_body_params,
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let mut wrapper_fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{i}"));
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
    let return_struct = wrapper_body.alloc_expr(
        Expr::Return {
            value: Some(struct_lit),
        },
        Span::DUMMY,
    );
    let _ = return_struct;
    let wrapper_body_owner = wrapper_body.owner;
    let wrapper_body_id = hir.bodies.push(wrapper_body);
    // Keep `body_owners` aligned with `bodies` (indexed by BodyId).
    // The desugar allocates bodies beyond the parser-lowered range,
    // so typeck's `body_owners[bid]` lookups would otherwise OOB.
    hir.body_owners.push(wrapper_body_owner);

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
    // T100-PATCHED-ROOTBLOCK: capture the function's root Block *before*
    // rewriting. `rewrite_expr` allocates new `Expr::Block` nodes (the
    // `Pending => loop {}` arm allocates an empty Block as the loop body),
    // so a post-rewrite reverse search for "the last Block in the arena"
    // finds that empty inner Block instead of the fn-body root. Its
    // `tail` is `None`, so the wrapping code below took the
    // `Expr::Block { .. } => return` bail-out branch and left the poll
    // body without its `Poll::Ready(...)` wrapper — producing a
    // practically-empty MIR body (`StorageLive(self); Return`) and the
    // "read from uninitialized local 0" failure at runtime.
    //
    // `root_expr_id` uses the same reverse search as the original code
    // here, so capturing its result before any allocation preserves the
    // exact intended root.
    let pre_rewrite_root_block: Option<ExprId> = (0..body.exprs.len())
        .rev()
        .map(|i| ExprId::from_raw(i as u32))
        .find(|&rid| matches!(body.exprs[rid], Expr::Block { .. }));

    // Snapshot expr ids to avoid iterating while mutating.
    let ids: Vec<ExprId> = (0..body.exprs.len())
        .map(|i| ExprId::from_raw(i as u32))
        .collect();
    for eid in ids {
        let expr = body.exprs[eid].clone();
        let new_expr = rewrite_expr(
            interner,
            body,
            expr,
            param_fields,
            self_name,
            ready_id,
            pending_id,
        );
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
    // T004-PATCHED [HIRX-1]: `lower_block_to_expr` allocates children
    // before the block, so the outermost fn-body Block is the *last*
    // Block in the arena (matching `root_expr_id` above). Searching
    // forward finds a *nested* block (if/match/loop body) and collapses
    // the poll body onto it — every other statement of the async fn
    // including the `.await` polling becomes unreachable.
    // T100-PATCHED-ROOTBLOCK: use the captured pre-rewrite root; a fresh
    // reverse search would find a rewrite-allocated inner Block.
    let root_block = pre_rewrite_root_block;
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

/// Build the outer future wrapper struct: `struct FooFuture { state: FooState }`.
///
/// Shared by both desugar paths: `rewrite_for_poll` (single-poll) and
/// `desugar_multi_async_fn` (multi-await resume-dispatch) both call this.
/// Historical note: an earlier revision of this doc called the helper an
/// unwired scaffold carrying a nonexistent `allow(dead_code)`; the
/// multi-await v1 resume-dispatch has since landed (plan §Phase 3 / M4)
/// and this helper is on its live path.
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
/// Shared by both the single-poll and multi-poll desugar paths (the same
/// "scaffold, not wired in" note that used to sit here was stale — see
/// `build_future_wrapper_struct`).
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
                generic_params: Vec::new(),
                return_ty: Some(poll_return_ty),
                is_unsafe: false,
                attrs: Vec::new(),
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

/// Multi-poll state-machine desugar: builds the state enum, future wrapper,
/// `impl Future` with a `loop { match self.state { .. } }` poll body, and the
/// wrapper fn. This IS the real v1 resume-dispatch for multi-await
/// sequential bodies (plan §Phase 3 / M4): `desugar_async` routes every
/// body with `suspend_count >= 2` and no loop-await here. An earlier
/// revision of this doc called the function an uninvoked "broken
/// skeleton" scaffold — that was true before the v1 dispatch landed and
/// is false today; see the `desugar_async` call site and the module docs
/// for the type-check gap.
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
fn desugar_multi_async_fn(
    hir: &mut crate::CrateHir,
    item_id: ItemId,
    diags: &mut Vec<GlyimDiagnostic>,
) {
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
    let future_name = format!("{fn_name_str}Future");
    let state_name = format!("{future_name}State");
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
    }
    let mut await_infos: Vec<AwaitInfo> = Vec::new();
    for sp in &suspend_points {
        let e = &work_body.exprs[sp.await_expr];
        if let Expr::Await { expr } = e {
            // Default result var: None; refined by the body split below.
            await_infos.push(AwaitInfo { inner: *expr });
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
    if fut_ty_names.iter().any(std::option::Option::is_none) {
        // A future whose concrete type isn't statically nameable. Per the
        // plan's safety rule we MUST NOT silently emit a broken state machine;
        // emit the clear `async-v2` diagnostic instead (compile error, not a
        // runtime hang).
        let span = suspend_points
            .first()
            .and_then(|sp| work_body.expr_spans.get(sp.await_expr).copied())
            .unwrap_or(Span::DUMMY);
        diags.push(GlyimDiagnostic::new(
            ErrorCode::from_registered(ErrorCategory::Type, 61),
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
        } else if let Some(seg) = pre_segments.last_mut() {
            seg.push(*stmt);
        }
    }
    // Tail await (rare): if root_tail contains an await, it's the final await.
    let tail_is_await = root_tail.is_some_and(|t| {
        await_infos_await_exprs(&work_body)
            .iter()
            .any(|&ai| stmt_contains(&work_body, t, ai))
    });
    let tail_expr = if tail_is_await { None } else { root_tail };

    // 3. Build the poll body. Each `S_k` arm re-drives `fut_k`.
    let mut poll_body = Body {
        unsafe_blocks: std::collections::HashSet::new(),
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        pat_spans: IndexVec::new(),
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
        .map(|(k, _)| interner.intern(&format!("v{k}")))
        .collect();
    let ready_value_name = interner.intern("__v");
    let arm_rename = |k: usize| -> std::collections::HashMap<crate::Name, crate::Name> {
        let mut m: std::collections::HashMap<crate::Name, crate::Name> =
            std::collections::HashMap::new();
        for (i, p) in original_params.iter().enumerate() {
            m.insert(p.name, interner.intern(&format!("f{i}")));
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
            let fname = interner.intern(&format!("f{i}"));
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
            stmts.push(copy_expr_renamed(
                &work_body,
                &mut poll_body,
                s,
                &rename,
                interner,
            ));
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
                pat_span: Span::DUMMY,
                value: fut0,
                ty: None,
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
                generic_args: None,
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
        let pending_pat = poll_body
            .pats
            .push(Pat::Path(two_seg(interner, "Poll", pending_id)));
        // Ready(r0): transition to S1, which carries the first result `v0` (= r0)
        // plus `fut1` (the next in-flight future). `S0` is only used on the
        // Pending path (carrying `fut0` to be re-driven on resume).
        let fut1_inner = await_infos[1].inner;
        let fut1_expr =
            copy_expr_renamed(&work_body, &mut poll_body, fut1_inner, &rename, interner);
        let fut1_local_name = interner.intern("fut1");
        let fut1_pat = poll_body.pats.push(Pat::Binding {
            name: fut1_local_name,
            mutability: Mutability::Not,
            subpattern: None,
        });
        let fut1_let = poll_body.alloc_expr(
            Expr::Let {
                pat: fut1_pat,
                pat_span: Span::DUMMY,
                value: fut1_expr,
                ty: None,
            },
            Span::DUMMY,
        );
        let s1_struct = build_state_struct(
            &mut poll_body,
            interner,
            &state_name,
            "S1",
            &original_params,
            &v_names[0..0],
            &[
                (v_names[0], ready_value_name),
                (fut1_local_name, fut1_local_name),
            ],
            self_name,
        );
        let assign_s1 = assign_state(&mut poll_body, interner, state_field_name, s1_struct);
        let continue_expr = poll_body.alloc_expr(Expr::Continue, Span::DUMMY);
        let ready_arm_body = {
            let mut s = Vec::new();
            // HIR-11(a): statements between await 0 and await 1 must run on the
            // Ready path *before* the next future is built. Previously they were
            // dropped entirely, so a `let mid = x + 1;` between the first and
            // second await left `mid` unbound in the tail (`unresolved name`).
            for &st in &pre_segments[1] {
                s.push(copy_expr_renamed(
                    &work_body,
                    &mut poll_body,
                    st,
                    &rename,
                    interner,
                ));
            }
            s.push(fut1_let);
            s.push(assign_s1);
            s.push(continue_expr);
            poll_body.alloc_expr(
                Expr::Block {
                    stmts: s,
                    tail: None,
                },
                Span::DUMMY,
            )
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
            let mut s = vec![assign_s0_p];
            s.push(return_pending);
            poll_body.alloc_expr(
                Expr::Block {
                    stmts: s,
                    tail: None,
                },
                Span::DUMMY,
            )
        };
        let match_expr = poll_body.alloc_expr(
            Expr::Match {
                scrutinee: poll_call,
                arms: vec![
                    MatchArm {
                        pat: ready_pat,
                        guard: None,
                        body: ready_arm_body,
                    },
                    MatchArm {
                        pat: pending_pat,
                        guard: None,
                        body: pending_arm_body,
                    },
                ],
            },
            Span::DUMMY,
        );
        stmts.push(match_expr);
        poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY)
    };
    arms.push(MatchArm {
        pat: start_pat,
        guard: None,
        body: start_body,
    });

    // --- S_k arms for k in 0..n-1 ---
    for k in 0..n {
        let s_pat = {
            let mut fields: Vec<(crate::Name, PatId)> = Vec::new();
            for (i, _p) in original_params.iter().enumerate() {
                let fname = interner.intern(&format!("f{i}"));
                let b = poll_body.pats.push(Pat::Binding {
                    name: fname,
                    mutability: Mutability::Not,
                    subpattern: None,
                });
                fields.push((fname, b));
            }
            for name in v_names.iter().take(k).copied() {
                let b = poll_body.pats.push(Pat::Binding {
                    name,
                    mutability: Mutability::Not,
                    subpattern: None,
                });
                fields.push((name, b));
            }
            let fut_name = interner.intern(&format!("fut{k}"));
            let fb = poll_body.pats.push(Pat::Binding {
                name: fut_name,
                mutability: Mutability::Not,
                subpattern: None,
            });
            fields.push((fut_name, fb));
            poll_body.pats.push(Pat::Struct {
                path: two_seg(interner, &state_name, interner.intern(&format!("S{k}"))),
                fields,
                rest: false,
            })
        };
        let s_body = {
            let rename_k = arm_rename(k);
            let fut_name = interner.intern(&format!("fut{k}"));
            let fut_path = poll_body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(fut_name))),
                Span::DUMMY,
            );
            let poll_call = poll_body.alloc_expr(
                Expr::MethodCall {
                    receiver: fut_path,
                    method: poll_id,
                    args: Vec::new(),
                    generic_args: None,
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
            let pending_pat = poll_body
                .pats
                .push(Pat::Path(two_seg(interner, "Poll", pending_id)));

            if k + 1 < n {
                // Ready: run pre_{k+1}, create fut_{k+1}, store S_{k+1}, continue
                let mut stmts: Vec<ExprId> = Vec::new();
                for &s in &pre_segments[k + 1] {
                    stmts.push(copy_expr_renamed(
                        &work_body,
                        &mut poll_body,
                        s,
                        &rename_k,
                        interner,
                    ));
                }
                let inner = await_infos[k + 1].inner;
                let fut_next =
                    copy_expr_renamed(&work_body, &mut poll_body, inner, &rename_k, interner);
                let fut_next_name = interner.intern(&format!("fut{}", k + 1));
                let fut_next_pat = poll_body.pats.push(Pat::Binding {
                    name: fut_next_name,
                    mutability: Mutability::Not,
                    subpattern: None,
                });
                let fut_next_let = poll_body.alloc_expr(
                    Expr::Let {
                        pat: fut_next_pat,
                        pat_span: Span::DUMMY,
                        value: fut_next,
                        ty: None,
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
                    &[
                        (v_names[k], interner.intern("__v")),
                        (fut_next_name, fut_next_name),
                    ],
                    self_name,
                );
                let assign_next = assign_state(&mut poll_body, interner, state_field_name, s_next);
                stmts.push(assign_next);
                stmts.push(poll_body.alloc_expr(Expr::Continue, Span::DUMMY));
                let ready_arm_body =
                    poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY);
                let pending_arm_body = poll_body.alloc_expr(
                    Expr::Return {
                        value: Some(poll_pending_path),
                    },
                    Span::DUMMY,
                );
                poll_body.alloc_expr(
                    Expr::Match {
                        scrutinee: poll_call,
                        arms: vec![
                            MatchArm {
                                pat: ready_pat,
                                guard: None,
                                body: ready_arm_body,
                            },
                            MatchArm {
                                pat: pending_pat,
                                guard: None,
                                body: pending_arm_body,
                            },
                        ],
                    },
                    Span::DUMMY,
                )
            } else {
                // Last await (k == n-1): Ready => compute tail, store Done, return
                let mut stmts: Vec<ExprId> = Vec::new();
                // HIR-11(b): statements between the last await and the tail
                // (`pre_segments[n]`). They were never emitted anywhere, so
                // `let d = k(c); d` after the final await lost `d`.
                for &st in &pre_segments[n] {
                    stmts.push(copy_expr_renamed(
                        &work_body,
                        &mut poll_body,
                        st,
                        &rename_k,
                        interner,
                    ));
                }
                let tail = if let Some(t) = tail_expr {
                    copy_expr_renamed(&work_body, &mut poll_body, t, &rename_k, interner)
                } else {
                    // tail itself is the await; the result is the Ready value.
                    poll_body.alloc_expr(
                        Expr::Path(plain_path(
                            interner,
                            interner.resolve(interner.intern("__v")),
                        )),
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
                let assign_done =
                    assign_state(&mut poll_body, interner, state_field_name, done_struct);
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
                    Expr::Return {
                        value: Some(ready_call),
                    },
                    Span::DUMMY,
                );
                stmts.push(return_ready);
                let ready_arm_body =
                    poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY);
                let pending_arm_body = poll_body.alloc_expr(
                    Expr::Return {
                        value: Some(poll_pending_path),
                    },
                    Span::DUMMY,
                );
                poll_body.alloc_expr(
                    Expr::Match {
                        scrutinee: poll_call,
                        arms: vec![
                            MatchArm {
                                pat: ready_pat,
                                guard: None,
                                body: ready_arm_body,
                            },
                            MatchArm {
                                pat: pending_pat,
                                guard: None,
                                body: pending_arm_body,
                            },
                        ],
                    },
                    Span::DUMMY,
                )
            }
        };
        arms.push(MatchArm {
            pat: s_pat,
            guard: None,
            body: s_body,
        });
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
    let done_result_arg =
        poll_body.alloc_expr(Expr::Path(plain_path(interner, "result")), Span::DUMMY);
    let done_result_call = poll_body.alloc_expr(
        Expr::Call {
            func: done_result_ctor,
            args: vec![done_result_arg],
        },
        Span::DUMMY,
    );
    let done_body = poll_body.alloc_expr(
        Expr::Return {
            value: Some(done_result_call),
        },
        Span::DUMMY,
    );
    arms.push(MatchArm {
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
    let loop_inner = poll_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(match_expr),
        },
        Span::DUMMY,
    );
    let loop_body = poll_body.alloc_expr(Expr::Loop { body: loop_inner }, Span::DUMMY);
    poll_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(loop_body),
        },
        Span::DUMMY,
    );
    let poll_body_owner = poll_body.owner;
    let poll_body_id = hir.bodies.push(poll_body);
    // Keep `body_owners` aligned with `bodies` (indexed by BodyId).
    // The desugar allocates bodies beyond the parser-lowered range,
    // so typeck's `body_owners[bid]` lookups would otherwise OOB.
    hir.body_owners.push(poll_body_owner);

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
        unsafe_blocks: std::collections::HashSet::new(),
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        pat_spans: IndexVec::new(),
        params: hir.bodies[original_body_id].params.clone(),
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let mut start_fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{i}"));
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
    let wrapper_body_owner = wrapper_body.owner;
    let wrapper_body_id = hir.bodies.push(wrapper_body);
    // Keep `body_owners` aligned with `bodies` (indexed by BodyId).
    // The desugar allocates bodies beyond the parser-lowered range,
    // so typeck's `body_owners[bid]` lookups would otherwise OOB.
    hir.body_owners.push(wrapper_body_owner);

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
        // GSX view expressions (finding F7): attribute/child hole exprs are
        // body-owned ExprIds — re-allocate them into `dst` like any other
        // child, keeping the element structure intact.
        Expr::Gsx(g) => {
            let mut g2 = (**g).clone();
            for attr in &mut g2.attrs {
                let value = match attr {
                    GsxAttrHIR::Plain { value, .. } | GsxAttrHIR::Directive { value, .. } => value,
                };
                if let GsxAttrValue::Expr(e) = value {
                    *e = copy_expr_renamed(src, dst, *e, rename, interner);
                }
            }
            for child in &mut g2.children {
                match child {
                    GsxChildHIR::Expr(e) => {
                        *e = copy_expr_renamed(src, dst, *e, rename, interner);
                    }
                    GsxChildHIR::Node(inner) => {
                        // Recursive structure copy: holes re-allocated in place.
                        copy_gsx_holes(src, dst, inner, rename, interner);
                    }
                    GsxChildHIR::Text(_) => {}
                }
            }
            Expr::Gsx(Box::new(g2))
        }
        Expr::Block { stmts, tail } => Expr::Block {
            stmts: stmts
                .iter()
                .map(|s| copy_expr_renamed(src, dst, *s, rename, interner))
                .collect(),
            tail: tail.map(|t| copy_expr_renamed(src, dst, t, rename, interner)),
        },
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Expr::If {
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
        Expr::For {
            pat,
            iterable,
            body,
        } => Expr::For {
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
                    guard: a
                        .guard
                        .map(|g| copy_expr_renamed(src, dst, g, rename, interner)),
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
        Expr::MethodCall {
            receiver,
            method,
            args,
            generic_args,
        } => Expr::MethodCall {
            receiver: copy_expr_renamed(src, dst, *receiver, rename, interner),
            method: *method,
            args: args
                .iter()
                .map(|a| copy_expr_renamed(src, dst, *a, rename, interner))
                .collect(),
            generic_args: generic_args.clone(),
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
        Expr::Closure {
            params,
            param_tys,
            body,
            is_move,
        } => Expr::Closure {
            params: params
                .iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
            param_tys: param_tys.clone(),
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
        Expr::Let {
            pat,
            pat_span,
            value,
            ty,
        } => Expr::Let {
            pat: copy_pat_renamed(src, dst, *pat, rename, interner),
            pat_span: *pat_span,
            value: copy_expr_renamed(src, dst, *value, rename, interner),
            ty: ty.clone(),
        },
        Expr::Struct {
            path,
            fields,
            spread,
        } => Expr::Struct {
            path: path.clone(),
            fields: fields
                .iter()
                .map(|(n, e)| (*n, copy_expr_renamed(src, dst, *e, rename, interner)))
                .collect(),
            spread: spread.map(|s| copy_expr_renamed(src, dst, s, rename, interner)),
        },
        Expr::Range {
            start,
            end,
            inclusive,
        } => Expr::Range {
            start: start.map(|s| copy_expr_renamed(src, dst, s, rename, interner)),
            end: end.map(|e| copy_expr_renamed(src, dst, e, rename, interner)),
            inclusive: *inclusive,
        },
        Expr::Await { expr } => Expr::Await {
            expr: copy_expr_renamed(src, dst, *expr, rename, interner),
        },
        Expr::Try { expr } => Expr::Try {
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
        Pat::Binding {
            name,
            mutability,
            subpattern,
        } => Pat::Binding {
            name: *name,
            mutability: *mutability,
            subpattern: subpattern.map(|s| copy_pat_renamed(src, dst, s, rename, interner)),
        },
        Pat::RefBinding {
            name,
            mutability,
            ref_mutability,
            subpattern,
        } => Pat::RefBinding {
            name: *name,
            mutability: *mutability,
            ref_mutability: *ref_mutability,
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
        Pat::Tuple(ps) => Pat::Tuple(
            ps.iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
        ),
        Pat::Slice(ps) => Pat::Slice(
            ps.iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
        ),
        Pat::Or(ps) => Pat::Or(
            ps.iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
        ),
        Pat::Path(p) => Pat::Path(p.clone()),
        Pat::Wild => Pat::Wild,
        Pat::Literal(l) => Pat::Literal(l.clone()),
        Pat::Range {
            start,
            end,
            inclusive,
        } => Pat::Range {
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
        // GSX view expressions (finding F7): attribute/child hole exprs are
        // body-owned ExprIds — re-allocate them into `dst` like any other
        // child, keeping the element structure intact.
        Expr::Gsx(g) => {
            let mut g2 = (**g).clone();
            for attr in &mut g2.attrs {
                let value = match attr {
                    GsxAttrHIR::Plain { value, .. } | GsxAttrHIR::Directive { value, .. } => value,
                };
                if let GsxAttrValue::Expr(e) = value {
                    *e = copy_expr_renamed(src, dst, *e, rename, interner);
                }
            }
            for child in &mut g2.children {
                match child {
                    GsxChildHIR::Expr(e) => {
                        *e = copy_expr_renamed(src, dst, *e, rename, interner);
                    }
                    GsxChildHIR::Node(inner) => {
                        // Recursive structure copy: holes re-allocated in place.
                        copy_gsx_holes(src, dst, inner, rename, interner);
                    }
                    GsxChildHIR::Text(_) => {}
                }
            }
            Expr::Gsx(Box::new(g2))
        }
        Expr::Block { stmts, tail } => Expr::Block {
            stmts: stmts
                .iter()
                .map(|s| {
                    copy_expr_subst_await(src, dst, *s, rename, interner, await_eid, subst_name)
                })
                .collect(),
            tail: tail.map(|t| {
                copy_expr_subst_await(src, dst, t, rename, interner, await_eid, subst_name)
            }),
        },
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Expr::If {
            cond: copy_expr_subst_await(src, dst, *cond, rename, interner, await_eid, subst_name),
            then_branch: copy_expr_subst_await(
                src,
                dst,
                *then_branch,
                rename,
                interner,
                await_eid,
                subst_name,
            ),
            else_branch: else_branch.map(|e| {
                copy_expr_subst_await(src, dst, e, rename, interner, await_eid, subst_name)
            }),
        },
        Expr::While { cond, body } => Expr::While {
            cond: copy_expr_subst_await(src, dst, *cond, rename, interner, await_eid, subst_name),
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
        },
        Expr::Loop { body } => Expr::Loop {
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
        },
        Expr::For {
            pat,
            iterable,
            body,
        } => Expr::For {
            pat: copy_pat_renamed(src, dst, *pat, rename, interner),
            iterable: copy_expr_subst_await(
                src, dst, *iterable, rename, interner, await_eid, subst_name,
            ),
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: copy_expr_subst_await(
                src, dst, *scrutinee, rename, interner, await_eid, subst_name,
            ),
            arms: arms
                .iter()
                .map(|a| crate::MatchArm {
                    pat: copy_pat_renamed(src, dst, a.pat, rename, interner),
                    guard: a.guard.map(|g| {
                        copy_expr_subst_await(src, dst, g, rename, interner, await_eid, subst_name)
                    }),
                    body: copy_expr_subst_await(
                        src, dst, a.body, rename, interner, await_eid, subst_name,
                    ),
                })
                .collect(),
        },
        Expr::Call { func, args } => Expr::Call {
            func: copy_expr_subst_await(src, dst, *func, rename, interner, await_eid, subst_name),
            args: args
                .iter()
                .map(|a| {
                    copy_expr_subst_await(src, dst, *a, rename, interner, await_eid, subst_name)
                })
                .collect(),
        },
        Expr::MethodCall {
            receiver,
            method,
            args,
            generic_args,
        } => Expr::MethodCall {
            receiver: copy_expr_subst_await(
                src, dst, *receiver, rename, interner, await_eid, subst_name,
            ),
            method: *method,
            args: args
                .iter()
                .map(|a| {
                    copy_expr_subst_await(src, dst, *a, rename, interner, await_eid, subst_name)
                })
                .collect(),
            generic_args: generic_args.clone(),
        },
        Expr::Field { receiver, field } => Expr::Field {
            receiver: copy_expr_subst_await(
                src, dst, *receiver, rename, interner, await_eid, subst_name,
            ),
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
            value: value.map(|v| {
                copy_expr_subst_await(src, dst, v, rename, interner, await_eid, subst_name)
            }),
        },
        Expr::Break { value } => Expr::Break {
            value: value.map(|v| {
                copy_expr_subst_await(src, dst, v, rename, interner, await_eid, subst_name)
            }),
        },
        Expr::Closure {
            params,
            param_tys,
            body,
            is_move,
        } => Expr::Closure {
            params: params
                .iter()
                .map(|p| copy_pat_renamed(src, dst, *p, rename, interner))
                .collect(),
            param_tys: param_tys.clone(),
            body: copy_expr_subst_await(src, dst, *body, rename, interner, await_eid, subst_name),
            is_move: *is_move,
        },
        Expr::Array(es) => Expr::Array(
            es.iter()
                .map(|e| {
                    copy_expr_subst_await(src, dst, *e, rename, interner, await_eid, subst_name)
                })
                .collect(),
        ),
        Expr::Tuple(es) => Expr::Tuple(
            es.iter()
                .map(|e| {
                    copy_expr_subst_await(src, dst, *e, rename, interner, await_eid, subst_name)
                })
                .collect(),
        ),
        Expr::Await { expr } => {
            if eid == await_eid {
                // The single in-loop await: lift it to the `Ready` payload binding.
                Expr::Path(plain_path(interner, interner.resolve(subst_name)))
            } else {
                Expr::Await {
                    expr: copy_expr_subst_await(
                        src, dst, *expr, rename, interner, await_eid, subst_name,
                    ),
                }
            }
        }
        Expr::Try { expr } => Expr::Try {
            expr: copy_expr_subst_await(src, dst, *expr, rename, interner, await_eid, subst_name),
        },
        other => other.clone(),
    };
    dst.alloc_expr(expr, Span::DUMMY)
}

/// Build `enum fooState { Start(f0..), S0(f0.., total, i, fut), Done(result) }`
/// for the loop-await state machine. `build_state_struct` covers `Start` and
/// `S0`; `Done` carries a single `result` field initialised from a local, so
/// it is built separately.
/// Loop-await coroutine desugar (M4/M5, plan §P2-1 step 1-3).
///
/// Handles the safe subset: an async fn whose body is
/// `let mut <locals>; while <cond> { ... <e>.await ...; <tail> } <tail>` or
/// `let mut <locals>; for <pat> in <a>..[=]<b> { ... <e>.await ...; <tail> } <tail>`
/// (`for` is normalized into the `while` shape — the GAP-0001 closure) with
/// exactly ONE `.await` inside the loop body, whose awaited future's type is
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
/// Locate the async fn's loop statement and normalize supported `for`
/// loops into the `while` shape the loop-await state machine re-drives.
///
/// Returns the `ExprId` of the (possibly synthesized) `Expr::While` root
/// statement plus the synthesized iterator-increment statement (`None` for
/// a native `while` or a normalized `loop`), or `None` when the shape is
/// unsupported (a `for` over a non-range iterable, a destructuring loop
/// pattern, or a range bound that is a general expression — the machine
/// re-evaluates the condition on every re-drive, but a `for` loop evaluates
/// its bounds once, so only side-effect-free bounds (literal / path) are
/// safe to normalize).
///
/// Normalization (GAP-0001 closure) rewrites, in the scratch `work_body`:
///
/// ```text
/// for i in a..b { BODY(i) }
/// ```
///
/// into
///
/// ```text
/// let mut __for_i = a;
/// while __for_i < b { BODY(__for_i); __for_i = __for_i + 1; }
/// ```
///
/// (`<=` for `..=`), with the pattern binding renamed to the fresh
/// synthetic name inside the loop body. `root_stmts` and the root block are
/// spliced so the machine's `Start` arm copies the init binding as a
/// pre-loop statement and the state enum carries `__for_i` as a loop local
/// across `Poll::Pending`.
///
/// Normalization (GAP-0006 `loop`-shape closure) rewrites `loop { B }` into
/// `while true { B }` in place — the machine's re-evaluated condition is a
/// fresh literal `true`, the body needs no rename, and the exit path is the
/// depth-0 `break` translation's `Done` transition.
fn find_and_normalize_loop_stmt(
    work_body: &mut Body,
    body_root: ExprId,
    root_stmts: &mut Vec<ExprId>,
    interner: &Interner,
) -> Option<(ExprId, Option<ExprId>)> {
    let idx = root_stmts.iter().position(|s| {
        matches!(
            work_body.exprs[*s],
            Expr::While { .. } | Expr::For { .. } | Expr::Loop { .. }
        )
    })?;
    let stmt = root_stmts[idx];

    // `loop { B }` normalizes into the `while true { B }` shape the machine
    // re-drives, in place: the condition is a fresh literal `true` (the
    // machine re-evaluates the condition on every re-drive, and a literal is
    // both correct and side-effect-free), and the loop body is carried over
    // untouched — no pattern rename, no statement extraction, no increment
    // step — so every depth-0 `break`/`continue` inside keeps its
    // translation. The exit path is the machine's own `Done` transition,
    // which the depth-0 `break` translation (`translate_loop_ctrl`) already
    // emits; the loop's value comes from the `break v` operand when present.
    // Reassigning the slot re-initializes the moved-out place.
    if matches!(work_body.exprs[stmt], Expr::Loop { .. }) {
        let loop_body = match work_body.exprs[stmt].clone() {
            Expr::Loop { body } => body,
            _ => unreachable!("matches! above"),
        };
        let cond = work_body.alloc_expr(Expr::Literal(Literal::Bool(true)), Span::DUMMY);
        work_body.exprs[stmt] = Expr::While {
            cond,
            body: loop_body,
        };
        return Some((stmt, None));
    }

    if matches!(work_body.exprs[stmt], Expr::While { .. }) {
        return Some((stmt, None));
    }

    // `Expr::For`: normalize into the while shape if the shape is supported.
    let (pat, iterable, for_body) = match &work_body.exprs[stmt] {
        Expr::For {
            pat,
            iterable,
            body,
        } => (*pat, *iterable, *body),
        _ => unreachable!("stmt matched Expr::For"),
    };

    // Supported iterable: `a..b` / `a..=b` with side-effect-free bounds.
    let (start, end, inclusive) = match &work_body.exprs[iterable] {
        Expr::Range {
            start,
            end,
            inclusive,
        } => match (start, end) {
            (Some(s), Some(e)) => {
                let bound_ok =
                    |id: ExprId| matches!(work_body.exprs[id], Expr::Literal(_) | Expr::Path(_));
                if bound_ok(*s) && bound_ok(*e) {
                    (*s, *e, *inclusive)
                } else {
                    return None;
                }
            }
            _ => return None,
        },
        _ => return None,
    };

    // Supported pattern: a simple binding (the machine renames the binding
    // name inside the body). Destructuring patterns stay on the diagnostic.
    let pat_binding = match &work_body.pats[pat] {
        Pat::Binding {
            name,
            mutability: _,
            subpattern: None,
        } => *name,
        _ => return None,
    };

    // The for body must be a plain statement block (no tail value) — the
    // shape the machine can re-drive statement by statement.
    let for_body_shape_ok = matches!(&work_body.exprs[for_body], Expr::Block { tail: None, .. });
    if !for_body_shape_ok {
        return None;
    }

    // Fresh synthetic iterator name: never collide with a binding already
    // present in the body.
    let existing: std::collections::HashSet<crate::Name> = work_body
        .pats
        .iter()
        .filter_map(|p| binding_name(p))
        .collect();
    let mut fresh = interner.intern("__for_i");
    let mut k = 0u32;
    while existing.contains(&fresh) {
        if k >= 1_000 {
            // Absurd collision count (the body binds a thousand __for_i*
            // names): treat the shape as unsupported.
            return None;
        }
        k += 1;
        fresh = interner.intern(&format!("__for_i{k}"));
    }

    // Snapshot copy: `copy_expr_renamed` needs distinct src/dst arenas, so
    // clone the scratch body once and copy out of the snapshot.
    let snap = work_body.clone();
    let no_rename = std::collections::HashMap::new();
    let rename: std::collections::HashMap<crate::Name, crate::Name> =
        [(pat_binding, fresh)].into_iter().collect();

    let start_copy = copy_expr_renamed(&snap, work_body, start, &no_rename, interner);
    let end_copy = copy_expr_renamed(&snap, work_body, end, &no_rename, interner);
    let renamed_body = copy_expr_renamed(&snap, work_body, for_body, &rename, interner);
    let mut body_stmts = match &work_body.exprs[renamed_body] {
        Expr::Block { stmts, tail: None } => stmts.clone(),
        _ => return None,
    };

    // `__for_i = __for_i + 1;`
    let i_path_inc = work_body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(fresh))),
        Span::DUMMY,
    );
    let one = work_body.alloc_expr(Expr::Literal(Literal::Int(1, None)), Span::DUMMY);
    let inc_rhs = work_body.alloc_expr(
        Expr::Binary {
            op: glyim_core::primitives::BinOp::Add,
            lhs: i_path_inc,
            rhs: one,
        },
        Span::DUMMY,
    );
    let inc = work_body.alloc_expr(
        Expr::Assign {
            lhs: i_path_inc,
            rhs: inc_rhs,
        },
        Span::DUMMY,
    );
    body_stmts.push(inc);
    let new_body = work_body.alloc_expr(
        Expr::Block {
            stmts: body_stmts,
            tail: None,
        },
        Span::DUMMY,
    ); // `__for_i < end` (or `<=` for `..=`).
    let i_path = work_body.alloc_expr(
        Expr::Path(plain_path(interner, interner.resolve(fresh))),
        Span::DUMMY,
    );
    let cond = work_body.alloc_expr(
        Expr::Binary {
            op: if inclusive {
                glyim_core::primitives::BinOp::LtEq
            } else {
                glyim_core::primitives::BinOp::Lt
            },
            lhs: i_path,
            rhs: end_copy,
        },
        Span::DUMMY,
    );
    let while_stmt = work_body.alloc_expr(
        Expr::While {
            cond,
            body: new_body,
        },
        Span::DUMMY,
    );

    // `let mut __for_i = start;`
    let init_pat = work_body.pats.push(Pat::Binding {
        name: fresh,
        mutability: glyim_core::primitives::Mutability::Mut,
        subpattern: None,
    });
    let init_stmt = work_body.alloc_expr(
        Expr::Let {
            pat: init_pat,
            pat_span: Span::DUMMY,
            value: start_copy,
            ty: None,
        },
        Span::DUMMY,
    );

    // Splice the root block: the init binding before the loop, the
    // synthesized while in the for's slot.
    root_stmts[idx] = while_stmt;
    root_stmts.insert(idx, init_stmt);
    if let Expr::Block { stmts, .. } = &mut work_body.exprs[body_root] {
        stmts.clone_from(root_stmts);
    }
    Some((while_stmt, Some(inc)))
}

/// The `Name` a pattern binds directly, if any.
fn binding_name(pat: &Pat) -> Option<crate::Name> {
    match pat {
        Pat::Binding { name, .. } | Pat::RefBinding { name, .. } => Some(*name),
        _ => None,
    }
}

/// Rewrite the depth-0 `break`/`continue` of a copied loop-await body —
/// the jumps targeting the desugared loop itself — into the machine's own
/// state transitions, editing `body`'s arena in place.
///
/// Inside the poll body these jumps would otherwise target the poll
/// dispatch loop: a `break` would exit it (a type error against the
/// `Poll<_>` return), a `continue` would re-enter the dispatch with the
/// same `S0` state and re-poll the same future forever. So:
///
/// * `continue` → the loop step (`on_continue`), optionally preceded by the
///   `for` normalization's iterator increment, so the loop variable still
///   advances before the next iteration's condition is re-checked;
/// * `break` / `break v` → the `Done { result: <result local | v> }`
///   transition (`on_break`), which the dispatch loop returns as
///   `Poll::Ready`.
///
/// Jumps nested inside an inner `while`/`loop`/`for` (depth > 0) or inside
/// a closure body keep their real inner loop and stay verbatim. Each
/// translation site is freshly allocated by the callbacks, and the walker
/// never descends into a replaced node, so a translated jump's embedded
/// dispatch `continue` is never itself re-translated.
fn translate_loop_ctrl(
    body: &mut Body,
    root: ExprId,
    state_name: &str,
    state_field_name: crate::Name,
    on_continue: &mut impl FnMut(&mut Body) -> ExprId,
    on_break: &mut impl FnMut(&mut Body, Option<ExprId>) -> ExprId,
) {
    fn walk(
        body: &mut Body,
        id: ExprId,
        depth: u32,
        state_name: &str,
        state_field_name: crate::Name,
        on_continue: &mut impl FnMut(&mut Body) -> ExprId,
        on_break: &mut impl FnMut(&mut Body, Option<ExprId>) -> ExprId,
    ) {
        // Clone the expr to decide the case (the clone carries the same
        // child ids); mutation below writes fresh structs into the slot.
        let expr = body.exprs[id].clone();
        match expr {
            // Terminal jumps: translate at depth 0, keep verbatim deeper.
            Expr::Continue => {
                if depth == 0 {
                    let step = on_continue(body);
                    let step_root = body.exprs[step].clone();
                    body.exprs[id] = step_root;
                }
            }
            Expr::Break { value } => {
                if depth == 0 {
                    let replacement = on_break(body, value);
                    let block_root = body.exprs[replacement].clone();
                    body.exprs[id] = block_root;
                } else if let Some(v) = value {
                    walk(
                        body,
                        v,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Block { stmts, tail } => {
                for s in stmts {
                    walk(
                        body,
                        s,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
                if let Some(t) = tail {
                    walk(
                        body,
                        t,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                walk(
                    body,
                    cond,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                walk(
                    body,
                    then_branch,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                if let Some(e) = else_branch {
                    walk(
                        body,
                        e,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            // A nested loop increments the jump depth: its own
            // break/continue target it, not the desugared loop.
            Expr::While { cond, body: wb } => {
                walk(
                    body,
                    cond,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                walk(
                    body,
                    wb,
                    depth + 1,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Loop { body: lb } => {
                walk(
                    body,
                    lb,
                    depth + 1,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::For {
                pat: _,
                iterable,
                body: fb,
            } => {
                walk(
                    body,
                    iterable,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                walk(
                    body,
                    fb,
                    depth + 1,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            // A closure body is its own control-flow scope: nothing inside
            // it can target the desugared loop.
            Expr::Closure {
                params: _,
                param_tys: _,
                body: cb,
                is_move: _,
            } => {
                walk(
                    body,
                    cb,
                    depth + 1,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Match { scrutinee, arms } => {
                walk(
                    body,
                    scrutinee,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                for arm in arms {
                    if let Some(g) = arm.guard {
                        walk(
                            body,
                            g,
                            depth,
                            state_name,
                            state_field_name,
                            on_continue,
                            on_break,
                        );
                    }
                    walk(
                        body,
                        arm.body,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Call { func, args } => {
                walk(
                    body,
                    func,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                for a in args {
                    walk(
                        body,
                        a,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                walk(
                    body,
                    receiver,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                for a in args {
                    walk(
                        body,
                        a,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Field { receiver, .. } => {
                walk(
                    body,
                    receiver,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Index { base, index } => {
                walk(
                    body,
                    base,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                walk(
                    body,
                    index,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Unary { expr, .. } => {
                walk(
                    body,
                    expr,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Binary { lhs, rhs, .. } => {
                walk(
                    body,
                    lhs,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                walk(
                    body,
                    rhs,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Cast { expr, .. } => {
                walk(
                    body,
                    expr,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Ref { expr, .. } => {
                walk(
                    body,
                    expr,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Assign { lhs, rhs } => {
                walk(
                    body,
                    lhs,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
                walk(
                    body,
                    rhs,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Return { value } => {
                if let Some(v) = value {
                    walk(
                        body,
                        v,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Let { value, .. } => {
                walk(
                    body,
                    value,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            Expr::Struct { fields, spread, .. } => {
                for (_, e) in fields {
                    walk(
                        body,
                        e,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
                if let Some(s) = spread {
                    walk(
                        body,
                        s,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Array(es) | Expr::Tuple(es) => {
                for e in es {
                    walk(
                        body,
                        e,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Range { start, end, .. } => {
                if let Some(s) = start {
                    walk(
                        body,
                        s,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
                if let Some(e) = end {
                    walk(
                        body,
                        e,
                        depth,
                        state_name,
                        state_field_name,
                        on_continue,
                        on_break,
                    );
                }
            }
            Expr::Await { expr } | Expr::Try { expr } => {
                walk(
                    body,
                    expr,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            // GSX view expressions: attribute/child holes are body-owned
            // exprs — walk them like any other child (no loop constructs
            // exist inside a view, but holes may embed arbitrary exprs).
            Expr::Gsx(g) => {
                fn walk_gsx(
                    body: &mut Body,
                    g: &mut GsxExpr,
                    depth: u32,
                    state_name: &str,
                    state_field_name: crate::Name,
                    on_continue: &mut impl FnMut(&mut Body) -> ExprId,
                    on_break: &mut impl FnMut(&mut Body, Option<ExprId>) -> ExprId,
                ) {
                    for attr in &mut g.attrs {
                        let value = match attr {
                            GsxAttrHIR::Plain { value, .. }
                            | GsxAttrHIR::Directive { value, .. } => value,
                        };
                        if let GsxAttrValue::Expr(e) = value {
                            walk(
                                body,
                                *e,
                                depth,
                                state_name,
                                state_field_name,
                                on_continue,
                                on_break,
                            );
                        }
                    }
                    for child in &mut g.children {
                        match child {
                            GsxChildHIR::Expr(e) => {
                                walk(
                                    body,
                                    *e,
                                    depth,
                                    state_name,
                                    state_field_name,
                                    on_continue,
                                    on_break,
                                );
                            }
                            GsxChildHIR::Node(inner) => {
                                walk_gsx(
                                    body,
                                    inner,
                                    depth,
                                    state_name,
                                    state_field_name,
                                    on_continue,
                                    on_break,
                                );
                            }
                            GsxChildHIR::Text(_) => {}
                        }
                    }
                }
                let mut g2 = (*g).clone();
                walk_gsx(
                    body,
                    &mut g2,
                    depth,
                    state_name,
                    state_field_name,
                    on_continue,
                    on_break,
                );
            }
            // Leaves (paths, literals, missing/err sentinels).
            Expr::Path(_) | Expr::Literal(_) | Expr::Missing | Expr::Err => {}
        }
    }
    walk(
        body,
        root,
        0,
        state_name,
        state_field_name,
        on_continue,
        on_break,
    );
}

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
    let original_body_id = fn_item.body?;
    let original_body_owner = hir.bodies[original_body_id].owner;

    let interner = &hir.interner;
    let fn_name_str = interner.resolve(fn_name).to_string();
    let future_name = format!("{fn_name_str}Future");
    let state_name = format!("{future_name}State");
    let _output_id = interner.intern("Output");
    let poll_id = interner.intern("poll");
    let ready_id = interner.intern("Ready");
    let pending_id = interner.intern("Pending");
    let self_name = interner.intern("self");
    let state_field_name = interner.intern("state");

    let emit_unsupported = |diags: &mut Vec<GlyimDiagnostic>, span: Span| {
        diags.push(GlyimDiagnostic::new(
            ErrorCode::from_registered(ErrorCategory::Type, 60),
            DiagSeverity::Error,
            "`.await` inside a loop body is not yet supported by the async state-machine \
             lowering for this shape (`for` over a non-range iterable, destructuring \
             loop patterns; tracked: KNOWN_GAPS.md async-v2). Hoist the \
             await out of the loop, or collect futures into a Vec and await them \
             sequentially outside the loop.",
            glyim_diag::MultiSpan::from_span(span),
        ));
    };

    let mut work_body = hir.bodies[original_body_id].clone();
    let body_root = root_expr_id(&work_body);
    let (mut root_stmts, mut root_tail) = match &work_body.exprs[body_root] {
        Expr::Block { stmts, tail } => (stmts.clone(), *tail),
        _ => (Vec::new(), Some(body_root)),
    };

    // A loop written as the body's last expression is the root block's
    // TAIL, not a statement: the parser lowers a trailing `loop { .. }`,
    // `while { .. }`, or `for { .. }` with no following statement as the
    // block's tail expression, so the statement scan below would miss it.
    // The machine consumes a *statement* loop, so splice a tail-position
    // loop into the statement list first. After the splice the `Done`
    // result comes from the loop locals (or the `break v` operand), not
    // from a (now consumed) tail expression, which is exactly the
    // tail-less fallback the machine already implements.
    if let Some(t) = root_tail {
        if matches!(
            work_body.exprs[t],
            Expr::While { .. } | Expr::For { .. } | Expr::Loop { .. }
        ) {
            root_stmts.push(t);
            root_tail = None;
        }
    }

    // Find the loop statement. `while` is natively supported; the GAP-0001
    // witness shape — `for <pat> in <start>..<end>` with a single `.await`
    // in the body — is normalized into the same while shape first (a
    // synthetic `let mut __for_i = <start>;` before the loop, the pattern
    // binding renamed to `__for_i` inside the body, and an explicit
    // `__for_i = __for_i + 1;` step after it), so the state machine below
    // re-drives both shapes identically; the GAP-0006 `loop`-shape witness —
    // `loop { .. }` with a single `.await` in the body — is normalized into
    // `while true { .. }` in place, so the machine re-drives it too (the
    // exit path is the depth-0 `break` translation's `Done` transition).
    // Other shapes keep the async-v2 diagnostic (the caller emits it when we
    // return `None`). The second element is the normalization's
    // iterator-increment statement, which a translated `continue` must
    // re-run so the loop variable advances (`None` for native `while` and
    // normalized `loop` bodies).
    let (loop_stmt, for_step_increment) =
        match find_and_normalize_loop_stmt(&mut work_body, body_root, &mut root_stmts, interner) {
            Some(found) => found,
            None => {
                // No supported loop statement here (a `for` over a non-range
                // iterable, or a destructuring loop pattern): the caller
                // emits the async-v2 diagnostic for the shape.
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
    let fut_ty_name = if let Some(s) = fut_ty_name {
        s
    } else {
        emit_unsupported(
            diags,
            work_body
                .expr_spans
                .get(await_expr)
                .copied()
                .unwrap_or(Span::DUMMY),
        );
        return None;
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
    // A plain path tail is read directly from the carried loop local; any
    // other tail shape (a computed expression) is re-evaluated at the
    // `Done` transitions, where every loop local it can reference is in
    // scope (the `Start` arm's copied pre-loop bindings and the `S0` arm's
    // pattern bindings). `None` (tail-less body) falls back to the last
    // loop local.
    let result_local = match root_tail {
        Some(t) => match &work_body.exprs[t] {
            Expr::Path(p) => p.as_name(),
            _ => loop_local_names.last().copied(),
        },
        None => loop_local_names.last().copied(),
    }
    .unwrap_or_else(|| {
        emit_unsupported(
            diags,
            work_body
                .expr_spans
                .get(loop_stmt)
                .copied()
                .unwrap_or(Span::DUMMY),
        );
        interner.intern("result")
    });

    // Renames: params -> fN. Loop locals and the await result stay local.
    let mut base_rename: std::collections::HashMap<crate::Name, crate::Name> =
        std::collections::HashMap::new();
    for (i, p) in original_params.iter().enumerate() {
        base_rename.insert(p.name, interner.intern(&format!("f{i}")));
    }
    let ready_value_name = interner.intern("__v");
    let fut_local_name = interner.intern("fut");
    // The tail expression to evaluate at `Done`, fresh per transition site.
    let result_value_src = root_tail;
    let build_result_value = |body: &mut Body| -> ExprId {
        match result_value_src {
            Some(t) => copy_expr_renamed(&work_body, body, t, &base_rename, interner),
            None => body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(result_local))),
                Span::DUMMY,
            ),
        }
    };

    let mut poll_body = Body {
        unsafe_blocks: std::collections::HashSet::new(),
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        pat_spans: IndexVec::new(),
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
                pat_span: Span::DUMMY,
                value: fut_inner,
                ty: None,
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
        stmts.push(assign_state(
            poll_body,
            interner,
            state_field_name,
            s0_struct,
        ));
        stmts.push(poll_body.alloc_expr(Expr::Continue, Span::DUMMY));
        let then_branch = poll_body.alloc_expr(Expr::Block { stmts, tail: None }, Span::DUMMY);

        // else: self.state = Done(result); continue
        let mut else_stmts: Vec<ExprId> = Vec::new();
        let done_result_value = build_result_value(poll_body);
        let done_struct = poll_body.alloc_expr(
            Expr::Struct {
                path: two_seg(interner, state_name, interner.intern("Done")),
                fields: vec![(interner.intern("result"), done_result_value)],
                spread: None,
            },
            Span::DUMMY,
        );
        else_stmts.push(assign_state(
            poll_body,
            interner,
            state_field_name,
            done_struct,
        ));
        else_stmts.push(poll_body.alloc_expr(Expr::Continue, Span::DUMMY));
        let else_branch = poll_body.alloc_expr(
            Expr::Block {
                stmts: else_stmts,
                tail: None,
            },
            Span::DUMMY,
        );

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
            let fname = interner.intern(&format!("f{i}"));
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
            stmts.push(copy_expr_renamed(
                &work_body,
                &mut poll_body,
                *s,
                &base_rename,
                interner,
            ));
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
            let fname = interner.intern(&format!("f{i}"));
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
                generic_args: None,
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
        // Copy the WHOLE loop body with the await substituted by the polled
        // value, preserving wrapper structure: an `If`/`While`/`Loop` around
        // the await keeps its guard and its `break`/`continue` targets
        // (flattening the wrappers here would run guarded statements
        // unconditionally and orphan their jumps). `copy_expr_subst_await`
        // recurses through every variant, so a non-`Block` body root is
        // copied just as faithfully.
        let copied_body = copy_expr_subst_await(
            &work_body,
            &mut poll_body,
            loop_body_root,
            &base_rename,
            interner,
            await_expr,
            ready_value_name,
        );
        // Depth-0 `break`/`continue` (jumps targeting the desugared loop
        // itself) cannot survive verbatim inside the dispatch loop: a
        // `break` would exit the poll dispatch loop and a `continue` would
        // re-enter it with the same `S0` state (an unbounded re-poll).
        // Translate them into the machine's own transitions: `continue`
        // becomes the loop step (optionally preceded by the `for`
        // normalization's iterator increment, so the loop variable still
        // advances), `break [v]` becomes the `Done { result }` transition.
        // Inner-loop jumps (depth > 0) keep their real inner loop and stay
        // verbatim.
        translate_loop_ctrl(
            &mut poll_body,
            copied_body,
            &state_name,
            state_field_name,
            // Fresh loop step per site (the step ends in the machine's own
            // dispatch `continue`, which must never be re-translated).
            &mut |body: &mut Body| {
                let step = build_loop_step(
                    body,
                    interner,
                    state_field_name,
                    &state_name,
                    &original_params,
                    &loop_local_names,
                    fut_local_name,
                    inner,
                    &base_rename,
                    loop_cond,
                );
                match for_step_increment {
                    Some(inc) => {
                        // `for i in a..b`: a `continue` must still advance
                        // the iterator state before the next loop step.
                        // The step itself ends in the machine's dispatch
                        // `continue`, so nothing after it is reachable.
                        let inc_copy =
                            copy_expr_renamed(&work_body, body, inc, &base_rename, interner);
                        body.alloc_expr(
                            Expr::Block {
                                stmts: vec![inc_copy, step],
                                tail: None,
                            },
                            Span::DUMMY,
                        )
                    }
                    None => step,
                }
            },
            &mut |body: &mut Body, value: Option<ExprId>| {
                // `break [v]`: transition to `Done { result: v }` and
                // re-enter the dispatch loop, which returns `Poll::Ready`.
                let result_value = match value {
                    Some(v) => v,
                    None => build_result_value(body),
                };
                let done = body.alloc_expr(
                    Expr::Struct {
                        path: two_seg(interner, &state_name, interner.intern("Done")),
                        fields: vec![(interner.intern("result"), result_value)],
                        spread: None,
                    },
                    Span::DUMMY,
                );
                let assign = assign_state(body, interner, state_field_name, done);
                let cont = body.alloc_expr(Expr::Continue, Span::DUMMY);
                body.alloc_expr(
                    Expr::Block {
                        stmts: vec![assign, cont],
                        tail: None,
                    },
                    Span::DUMMY,
                )
            },
        );
        ready_stmts.push(copied_body);
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
            loop_cond,
        ));
        let ready_arm_body = poll_body.alloc_expr(
            Expr::Block {
                stmts: ready_stmts,
                tail: None,
            },
            Span::DUMMY,
        );

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
        let mut pend_stmts: Vec<ExprId> = vec![assign_state(
            &mut poll_body,
            interner,
            state_field_name,
            s0_preserve,
        )];
        pend_stmts.push(poll_body.alloc_expr(
            Expr::Return {
                value: Some(poll_pending_path),
            },
            Span::DUMMY,
        ));
        let pending_arm_body = poll_body.alloc_expr(
            Expr::Block {
                stmts: pend_stmts,
                tail: None,
            },
            Span::DUMMY,
        );

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
        Expr::Path(plain_path(
            interner,
            interner.resolve(interner.intern("result")),
        )),
        Span::DUMMY,
    );
    let done_ctor =
        poll_body.alloc_expr(Expr::Path(two_seg(interner, "Poll", ready_id)), Span::DUMMY);
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
    let loop_inner = poll_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(match_expr),
        },
        Span::DUMMY,
    );
    let loop_body = poll_body.alloc_expr(Expr::Loop { body: loop_inner }, Span::DUMMY);
    poll_body.alloc_expr(
        Expr::Block {
            stmts: Vec::new(),
            tail: Some(loop_body),
        },
        Span::DUMMY,
    );
    let poll_body_owner = poll_body.owner;
    let poll_body_id = hir.bodies.push(poll_body.clone());
    // Keep `body_owners` aligned with `bodies` (indexed by BodyId). The
    // desugar allocates bodies beyond the parser-lowered range, so typeck's
    // `body_owners[bid]` lookups would otherwise OOB.
    hir.body_owners.push(poll_body_owner);

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
        unsafe_blocks: std::collections::HashSet::new(),
        owner: original_body_owner,
        exprs: IndexVec::new(),
        pats: IndexVec::new(),
        pat_spans: IndexVec::new(),
        params: hir.bodies[original_body_id].params.clone(),
        span: Span::DUMMY,
        expr_spans: IndexVec::new(),
    };
    let mut start_fields: Vec<(crate::Name, ExprId)> = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let field_name = interner.intern(&format!("f{i}"));
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
    let wrapper_body_owner = wrapper_body.owner;
    let wrapper_body_id = hir.bodies.push(wrapper_body);
    // Keep `body_owners` aligned with `bodies` (indexed by BodyId).
    // The desugar allocates bodies beyond the parser-lowered range,
    // so typeck's `body_owners[bid]` lookups would otherwise OOB.
    hir.body_owners.push(wrapper_body_owner);

    fn_item.is_async = false;
    fn_item.body = Some(wrapper_body_id);
    fn_item.return_ty = Some(TypeRef::Path(plain_path(interner, &future_name)));
    hir.items[item_id] = item;

    hir.items.push(state_enum_item);
    hir.items.push(future_struct_item);
    hir.items.push(future_impl_item);
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
        let fname = interner.intern(&format!("f{i}"));
        let ty =
            p.ty.clone()
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
            let fname = interner.intern(&format!("f{i}"));
            let ty =
                p.ty.clone()
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
        let fname = interner.intern(&format!("f{i}"));
        // The parameter is bound as a local `fN` by each state arm's pattern
        // (see `start_pat` / `s_pat`), so reference it as a bare local.
        fields.push((
            fname,
            body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(fname))),
                Span::DUMMY,
            ),
        ));
    }
    for &v in v_names {
        fields.push((
            v,
            body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(v))),
                Span::DUMMY,
            ),
        ));
    }
    // `extra` are (struct_field_name, value_local_name) pairs — e.g. the just
    // completed result `v_k` (value `__v`) and the next future `fut_{k+1}`
    // (value `fut_{k+1}`). Each becomes a struct field initialised from the
    // named local.
    for &(field_name, value_name) in extra {
        fields.push((
            field_name,
            body.alloc_expr(
                Expr::Path(plain_path(interner, interner.resolve(value_name))),
                Span::DUMMY,
            ),
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
fn assign_state(
    body: &mut Body,
    interner: &Interner,
    state_field_name: crate::Name,
    value: ExprId,
) -> ExprId {
    let self_path = body.alloc_expr(
        Expr::Path(plain_path(
            interner,
            interner.resolve(interner.intern("self")),
        )),
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
    let mut variants = Vec::new();

    // Start(f0..fn)
    let mut start_fields = Vec::new();
    for (i, p) in original_params.iter().enumerate() {
        let fname = interner.intern(&format!("f{i}"));
        let ty =
            p.ty.clone()
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
    for (k, fut_name) in fut_ty_names.iter().enumerate() {
        let mut fields = Vec::new();
        for (i, p) in original_params.iter().enumerate() {
            let fname = interner.intern(&format!("f{i}"));
            let ty =
                p.ty.clone()
                    .unwrap_or_else(|| TypeRef::Path(plain_path(interner, "i32")));
            fields.push(Field {
                name: fname,
                ty,
                span: Span::DUMMY,
            });
        }
        for name in v_names.iter().take(k).copied() {
            let ty = TypeRef::Infer; // result type is the awaited future's Output; infer from Ready payload
            fields.push(Field {
                name,
                ty,
                span: Span::DUMMY,
            });
        }
        let fut_ty = TypeRef::Path(plain_path(interner, fut_name));
        fields.push(Field {
            name: interner.intern(&format!("fut{k}")),
            ty: fut_ty,
            span: Span::DUMMY,
        });
        variants.push(Variant {
            name: interner.intern(&format!("S{k}")),
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

/// Collect the `Expr::Await` `ExprIds` (the `.await` nodes) in `body`.
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
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
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
            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
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
        Pat::Binding { name, .. } | Pat::RefBinding { name, .. } => Some(*name),
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
                    generic_args: None,
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
            let empty_block = body.alloc_expr(
                Expr::Block {
                    stmts: Vec::new(),
                    tail: None,
                },
                Span::DUMMY,
            );
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
                let ctor =
                    body.alloc_expr(Expr::Path(two_seg(interner, "Poll", ready_id)), Span::DUMMY);
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
                .map(|s| {
                    rewrite_in_body(
                        interner,
                        body,
                        s,
                        param_fields,
                        self_name,
                        ready_id,
                        pending_id,
                    )
                })
                .collect(),
            tail: tail.map(|t| {
                rewrite_in_body(
                    interner,
                    body,
                    t,
                    param_fields,
                    self_name,
                    ready_id,
                    pending_id,
                )
            }),
        },
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => Expr::If {
            cond: rewrite_in_body(
                interner,
                body,
                cond,
                param_fields,
                self_name,
                ready_id,
                pending_id,
            ),
            then_branch: rewrite_in_body(
                interner,
                body,
                then_branch,
                param_fields,
                self_name,
                ready_id,
                pending_id,
            ),
            else_branch: else_branch.map(|e| {
                rewrite_in_body(
                    interner,
                    body,
                    e,
                    param_fields,
                    self_name,
                    ready_id,
                    pending_id,
                )
            }),
        },
        Expr::Match { scrutinee, arms } => Expr::Match {
            scrutinee: rewrite_in_body(
                interner,
                body,
                scrutinee,
                param_fields,
                self_name,
                ready_id,
                pending_id,
            ),
            arms: arms
                .into_iter()
                .map(|mut a| {
                    if let Some(g) = a.guard {
                        a.guard = Some(rewrite_in_body(
                            interner,
                            body,
                            g,
                            param_fields,
                            self_name,
                            ready_id,
                            pending_id,
                        ));
                    }
                    a.body = rewrite_in_body(
                        interner,
                        body,
                        a.body,
                        param_fields,
                        self_name,
                        ready_id,
                        pending_id,
                    );
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
    let new_expr = rewrite_expr(
        interner,
        body,
        expr,
        param_fields,
        self_name,
        ready_id,
        pending_id,
    );
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

/// Re-allocate the hole expressions of a nested GSX node into `dst` in place.
fn copy_gsx_holes(
    src: &Body,
    dst: &mut Body,
    g: &mut GsxExpr,
    rename: &std::collections::HashMap<crate::Name, crate::Name>,
    interner: &Interner,
) {
    for attr in &mut g.attrs {
        let value = match attr {
            GsxAttrHIR::Plain { value, .. } | GsxAttrHIR::Directive { value, .. } => value,
        };
        if let GsxAttrValue::Expr(e) = value {
            *e = copy_expr_renamed(src, dst, *e, rename, interner);
        }
    }
    for child in &mut g.children {
        if let GsxChildHIR::Expr(e) = child {
            *e = copy_expr_renamed(src, dst, *e, rename, interner);
        }
    }
}
