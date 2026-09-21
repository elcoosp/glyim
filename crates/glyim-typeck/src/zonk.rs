//! Post-inference zonking of THIR bodies.
//!
//! ## Why this exists
//!
//! THIR `Expr.ty` (and `Stmt::Let.ty`, `Pattern.ty`, `Capture.ty`, the
//! `ForIteratorNext` fields, `Body.return_ty`, …) are written at
//! *construction* time — before unification runs. By the time type-checking
//! finishes, every literal's `Infer(Int(_))` has been unified against its
//! context (e.g. `add_one(41)` constrains `41 : i32`, and `x + 1` constrains
//! `1 : i32`), but the THIR body still holds the *pre-unification* snapshot.
//!
//! Downstream phases (MIR lowering, `glyim-codegen-llvm`, the bytecode VM)
//! read those slots directly. Without zonking, a literal's `Infer(Int(_))`
//! either silently miscompiles or, more typically, hits the LLVM backend's
//! deliberate ICE:
//!
//! ```text
//! internal compiler error: TyKind::Infer(Int(IntVar(0))) reached LLVM codegen
//! ```
//!
//! This is exactly the gap rustc closes with
//! `rustc_hir_typeck::resolve_type_vars_in_body` (its `TypeFolder`): after
//! inference and obligation fulfillment, walk the typed body once and fold
//! every inference variable to its resolved form. We do the same.
//!
//! ## Contract
//!
//! After [`zonk_bodies`] runs, no `Ty` reachable from any THIR body carries
//! `InferVar::Int` / `InferVar::Float` in either its bound or unbound form:
//!
//! * a **bound** variable resolves through the inference table to its value
//!   (which may itself be another variable, or a concrete type like `i32`);
//! * an **unbound** `Int` variable defaults to `i32` (Rust fallback
//!   semantics, matching `InferenceTable::resolve_ty_shallow`);
//! * an **unbound** `Float` variable is left as-is (matching today's
//!   behaviour — the compiler has no float default yet);
//! * an **unbound** `InferVar::Ty` is left as-is; those signal a genuine
//!   "type annotations needed" gap that should already have been diagnosed,
//!   and the LLVM backend's existing ICE for them is the correct signal if
//!   they survive to codegen.
//!
//! The invariant is enforced by a `debug_assert!` in `typeck_crate`; any
//! future addition of a `Ty` slot to THIR that the walker does not cover
//! will fail that assert in debug builds *at the source*, before any
//! downstream phase has a chance to misinterpret the raw type.

use glyim_solve::InferenceTable;
use glyim_core::def_id::AdtId;
use glyim_type::{GenericArg, Substitution, Ty, TyCtx, TyKind};

use crate::thir;

/// Zonk every THIR body in `bodies` in place, folding inference variables
/// in every `Ty` slot through `infer`. See module docs for the contract.
pub fn zonk_bodies(
    infer: &InferenceTable,
    ctx: &TyCtx,
    bodies: &mut [(glyim_core::def_id::LocalDefId, thir::Body)],
) {
    for (_owner, body) in bodies.iter_mut() {
        zonk_body(infer, ctx, body);
    }
}

/// Whether any `Ty` reachable from `body` still carries an integer or float
/// inference variable. Used by the debug-only invariant check in
/// `typeck_crate` (see module docs).
pub fn body_has_infer_ints_or_floats(ctx: &TyCtx, body: &thir::Body) -> bool {
    let mut found = false;
    if has_infer_int_or_float(ctx, body.return_ty) {
        found = true;
    }
    for param in body.params.iter() {
        if has_infer_int_or_float(ctx, param.ty) {
            found = true;
        }
    }
    for stmt in body.stmts.iter() {
        let mut stmt_found = false;
        walk_stmt(ctx, stmt, &mut |ty| {
            if has_infer_int_or_float(ctx, ty) {
                stmt_found = true;
            }
        });
        if stmt_found {
            found = true;
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Walker: apply `f` to every `Ty` slot reachable from a THIR construct.
// ---------------------------------------------------------------------------

fn walk_expr(ctx: &TyCtx, expr: &thir::Expr, f: &mut impl FnMut(Ty)) {
    f(expr.ty);
    match &expr.kind {
        thir::ExprKind::Literal(_)
        | thir::ExprKind::VarRef(_)
        | thir::ExprKind::FnRef(_)
        | thir::ExprKind::ConstRef(_)
        | thir::ExprKind::VariantRef(_, _)
        | thir::ExprKind::VariantCtor { .. }
        | thir::ExprKind::TraitMethodRef { .. }
        | thir::ExprKind::Continue
        | thir::ExprKind::Err => {}

        thir::ExprKind::Binary { lhs, rhs, .. } => {
            walk_expr(ctx, lhs, f);
            walk_expr(ctx, rhs, f);
        }
        thir::ExprKind::Unary { operand, .. } => walk_expr(ctx, operand, f),
        thir::ExprKind::Call { func, args } => {
            walk_expr(ctx, func, f);
            for a in args {
                walk_expr(ctx, a, f);
            }
        }
        thir::ExprKind::DynamicCall { receiver, args, .. } => {
            walk_expr(ctx, receiver, f);
            for a in args {
                walk_expr(ctx, a, f);
            }
        }
        thir::ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ctx, cond, f);
            walk_expr(ctx, then_branch, f);
            if let Some(e) = else_branch {
                walk_expr(ctx, e, f);
            }
        }
        thir::ExprKind::Match { scrutinee, arms } => {
            walk_expr(ctx, scrutinee, f);
            for arm in arms {
                walk_pattern(ctx, &arm.pat, f);
                if let Some(g) = &arm.guard {
                    walk_expr(ctx, g, f);
                }
                walk_expr(ctx, &arm.body, f);
            }
        }
        thir::ExprKind::Block { stmts, tail } => {
            for s in stmts {
                walk_stmt(ctx, s, f);
            }
            if let Some(t) = tail {
                walk_expr(ctx, t, f);
            }
        }
        thir::ExprKind::Ref { operand, .. } => walk_expr(ctx, operand, f),
        thir::ExprKind::Field { receiver, ty, .. } => {
            walk_expr(ctx, receiver, f);
            f(*ty);
        }
        thir::ExprKind::Index { base, index } => {
            walk_expr(ctx, base, f);
            walk_expr(ctx, index, f);
        }
        thir::ExprKind::Cast { expr } => walk_expr(ctx, expr, f),
        thir::ExprKind::While { cond, body } => {
            walk_expr(ctx, cond, f);
            walk_expr(ctx, body, f);
        }
        thir::ExprKind::Loop { body } => walk_expr(ctx, body, f),
        thir::ExprKind::For {
            pat,
            iterable,
            body,
            next,
        } => {
            walk_pattern(ctx, pat, f);
            walk_expr(ctx, iterable, f);
            walk_expr(ctx, body, f);
            if let Some(n) = next {
                f(n.option_ty);
                f(n.discr_ty);
                f(n.ref_iter_ty);
                f(n.fn_ty);
            }
        }
        thir::ExprKind::Array(elems) | thir::ExprKind::Tuple(elems) => {
            for e in elems {
                walk_expr(ctx, e, f);
            }
        }
        thir::ExprKind::Struct {
            fields, spread, ..
        } => {
            for (_name, e) in fields {
                walk_expr(ctx, e, f);
            }
            if let Some(s) = spread {
                walk_expr(ctx, s, f);
            }
        }
        thir::ExprKind::Break { value } | thir::ExprKind::Return { value } => {
            if let Some(v) = value {
                walk_expr(ctx, v, f);
            }
        }
        thir::ExprKind::Closure { body, captures, .. } => {
            for c in captures {
                f(c.ty);
            }
            // Recurse into the closure's own THIR body: its types were
            // resolved by the *same* outer inference table (see
            // `check_expr::Expr::Closure`), so a single zonk covers them.
            walk_body(ctx, body, f);
        }
        thir::ExprKind::Range { start, end, .. } => {
            if let Some(s) = start {
                walk_expr(ctx, s, f);
            }
            if let Some(e) = end {
                walk_expr(ctx, e, f);
            }
        }
        thir::ExprKind::Try { expr } => walk_expr(ctx, expr, f),
    }
}

fn walk_stmt(ctx: &TyCtx, stmt: &thir::Stmt, f: &mut impl FnMut(Ty)) {
    match stmt {
        thir::Stmt::Let {
            ty, pat, init, ..
        } => {
            f(*ty);
            walk_pattern(ctx, pat, f);
            if let Some(i) = init {
                walk_expr(ctx, i, f);
            }
        }
        thir::Stmt::Assign { lhs, rhs, .. } => {
            walk_expr(ctx, lhs, f);
            walk_expr(ctx, rhs, f);
        }
        thir::Stmt::Return { value, .. } => {
            if let Some(v) = value {
                walk_expr(ctx, v, f);
            }
        }
        thir::Stmt::Expr { expr } => walk_expr(ctx, expr, f),
    }
}

fn walk_pattern(ctx: &TyCtx, pat: &thir::Pattern, f: &mut impl FnMut(Ty)) {
    f(pat.ty);
    match &pat.kind {
        thir::PatternKind::Wild
        | thir::PatternKind::Literal(_)
        | thir::PatternKind::Range { .. }
        | thir::PatternKind::ConstBlock(_)
        | thir::PatternKind::Error => {}
        thir::PatternKind::Binding { subpattern, .. } => {
            if let Some(sub) = subpattern {
                walk_pattern(ctx, sub, f);
            }
        }
        thir::PatternKind::Struct { fields, .. } => {
            for fp in fields {
                walk_pattern(ctx, &fp.pattern, f);
            }
        }
        thir::PatternKind::Tuple(pats) | thir::PatternKind::Or(pats) => {
            for p in pats {
                walk_pattern(ctx, p, f);
            }
        }
        thir::PatternKind::Slice {
            prefix,
            slice,
            suffix,
        } => {
            for p in prefix {
                walk_pattern(ctx, p, f);
            }
            for p in suffix {
                walk_pattern(ctx, p, f);
            }
            if let Some(s) = slice {
                walk_pattern(ctx, s, f);
            }
        }
    }
}

fn walk_body(ctx: &TyCtx, body: &thir::Body, f: &mut impl FnMut(Ty)) {
    f(body.return_ty);
    for param in &body.params {
        f(param.ty);
        walk_pattern(ctx, &param.pat, f);
    }
    for stmt in &body.stmts {
        walk_stmt(ctx, stmt, f);
    }
}

// ---------------------------------------------------------------------------
// Rewriter: same traversal shape, but replaces each `Ty` in place.
// ---------------------------------------------------------------------------

/// Resolve a `Ty` through the inference table and rebuild it recursively.
///
/// A single call to `resolve_ty_shallow` only peels *one* layer of
/// inference variables; a type like `Vec<?T>` where `?T := i32` still needs
/// the substitution walked. We do both: shallow-resolve the head (which
/// handles a top-level `Infer(_)`), then structurally rebuild the kind's
/// child types. `resolve_ty_shallow` is idempotent, so calling it here and
/// again on children is cheap.
fn zonk_ty(infer: &InferenceTable, ctx: &TyCtx, ty: Ty) -> Ty {
    // Resolve the *head* of the type first: a top-level `Infer(_)` must be
    // folded to its bound value (or its fallback) before we inspect its
    // `TyKind` for children. Without this, the `TyKind::Infer(_)` arm of the
    // match below would short-circuit and return the raw var unchanged, and
    // no child walk would ever happen — which is exactly how a
    // `Infer(Int(_))` survived zonking to reach codegen.
    let ty = infer.resolve_ty_shallow(ctx, ty);
    match ctx.ty_kind(ty).clone() {
        // Primitive scalars / never / unit / bool / char / string / error
        // have no children to walk.
        TyKind::Never
        | TyKind::Unit
        | TyKind::Bool
        | TyKind::Int(_)
        | TyKind::Uint(_)
        | TyKind::Float(_)
        | TyKind::Char
        | TyKind::String
        | TyKind::Error
        | TyKind::Infer(_)
        | TyKind::Param(_)
        | TyKind::Bound(_, _)
        | TyKind::Dynamic(_, _)
        | TyKind::Projection(_) => ty,

        TyKind::Ref(region, inner, mutability) => {
            let inner = zonk_ty(infer, ctx, inner);
            ctx.mk_ty(TyKind::Ref(region, inner, mutability))
        }
        TyKind::RawPtr(inner, mutability) => {
            let inner = zonk_ty(infer, ctx, inner);
            ctx.mk_ty(TyKind::RawPtr(inner, mutability))
        }
        TyKind::Slice(inner) => {
            let inner = zonk_ty(infer, ctx, inner);
            ctx.mk_ty(TyKind::Slice(inner))
        }
        TyKind::Array(inner, cst) => {
            let inner = zonk_ty(infer, ctx, inner);
            ctx.mk_ty(TyKind::Array(inner, cst))
        }
        TyKind::Adt(adt, substs) => {
            let new_substs = zonk_substitution(infer, ctx, substs);
            ctx.mk_ty(TyKind::Adt(adt, new_substs))
        }
        TyKind::FnDef(id, substs) => {
            let new_substs = zonk_substitution(infer, ctx, substs);
            ctx.mk_ty(TyKind::FnDef(id, new_substs))
        }
        TyKind::Closure(id, substs) => {
            let new_substs = zonk_substitution(infer, ctx, substs);
            ctx.mk_ty(TyKind::Closure(id, new_substs))
        }
        TyKind::Opaque(id, substs) => {
            let new_substs = zonk_substitution(infer, ctx, substs);
            ctx.mk_ty(TyKind::Opaque(id, new_substs))
        }
        TyKind::Tuple(substs) => {
            let new_substs = zonk_substitution(infer, ctx, substs);
            ctx.mk_ty(TyKind::Tuple(new_substs))
        }
        TyKind::FnPtr(sig) => {
            // `FnSig` carries its own substitution of inputs; zonk them so a
            // closure's inferred parameter types don't survive.
            let args = ctx.substitution_args(sig.inputs);
            let mut new_args: Vec<GenericArg> = Vec::with_capacity(args.len());
            for a in args {
                match a {
                    GenericArg::Ty(t) => {
                        new_args.push(GenericArg::Ty(zonk_ty(infer, ctx, *t)))
                    }
                    other => new_args.push(other.clone()),
                }
            }
            let new_inputs = ctx.intern_substitution(new_args);
            let new_output = zonk_ty(infer, ctx, sig.output);
            ctx.mk_ty(TyKind::FnPtr(glyim_type::FnSig {
                inputs: new_inputs,
                output: new_output,
                c_variadic: sig.c_variadic,
                unsafety: sig.unsafety,
                abi: sig.abi,
            }))
        }
    }
}

fn zonk_substitution(infer: &InferenceTable, ctx: &TyCtx, substs: Substitution) -> Substitution {
    if substs.is_empty() {
        return substs;
    }
    let args = ctx.substitution_args(substs);
    let mut new_args: Vec<GenericArg> = Vec::with_capacity(args.len());
    let mut changed = false;
    for a in args {
        match a {
            GenericArg::Ty(t) => {
                let zonked = zonk_ty(infer, ctx, *t);
                if zonked != *t {
                    changed = true;
                }
                new_args.push(GenericArg::Ty(zonked));
            }
            other => new_args.push(other.clone()),
        }
    }
    if changed {
        ctx.intern_substitution(new_args)
    } else {
        substs
    }
}

// ---------------------------------------------------------------------------
// In-place rewriting of THIR.
// ---------------------------------------------------------------------------

fn zonk_body(infer: &InferenceTable, ctx: &TyCtx, body: &mut thir::Body) {
    body.return_ty = zonk_ty(infer, ctx, body.return_ty);
    for param in &mut body.params {
        param.ty = zonk_ty(infer, ctx, param.ty);
        zonk_pattern(infer, ctx, &mut param.pat);
    }
    for stmt in &mut body.stmts {
        zonk_stmt(infer, ctx, stmt);
    }
}

fn zonk_stmt(infer: &InferenceTable, ctx: &TyCtx, stmt: &mut thir::Stmt) {
    match stmt {
        thir::Stmt::Let {
            ty, pat, init, ..
        } => {
            *ty = zonk_ty(infer, ctx, *ty);
            zonk_pattern(infer, ctx, pat);
            if let Some(i) = init {
                zonk_expr(infer, ctx, i);
            }
        }
        thir::Stmt::Assign { lhs, rhs, .. } => {
            zonk_expr(infer, ctx, lhs);
            zonk_expr(infer, ctx, rhs);
        }
        thir::Stmt::Return { value, .. } => {
            if let Some(v) = value {
                zonk_expr(infer, ctx, v);
            }
        }
        thir::Stmt::Expr { expr } => zonk_expr(infer, ctx, expr),
    }
}

fn zonk_pattern(infer: &InferenceTable, ctx: &TyCtx, pat: &mut thir::Pattern) {
    pat.ty = zonk_ty(infer, ctx, pat.ty);
    match &mut pat.kind {
        thir::PatternKind::Wild
        | thir::PatternKind::Literal(_)
        | thir::PatternKind::Range { .. }
        | thir::PatternKind::ConstBlock(_)
        | thir::PatternKind::Error => {}
        thir::PatternKind::Binding { subpattern, .. } => {
            if let Some(sub) = subpattern {
                zonk_pattern(infer, ctx, sub);
            }
        }
        thir::PatternKind::Struct { fields, .. } => {
            for fp in fields {
                zonk_pattern(infer, ctx, &mut fp.pattern);
            }
        }
        thir::PatternKind::Tuple(pats) | thir::PatternKind::Or(pats) => {
            for p in pats {
                zonk_pattern(infer, ctx, p);
            }
        }
        thir::PatternKind::Slice {
            prefix,
            slice,
            suffix,
        } => {
            for p in prefix {
                zonk_pattern(infer, ctx, p);
            }
            for p in suffix {
                zonk_pattern(infer, ctx, p);
            }
            if let Some(s) = slice {
                zonk_pattern(infer, ctx, s);
            }
        }
    }
}

fn zonk_expr(infer: &InferenceTable, ctx: &TyCtx, expr: &mut thir::Expr) {
    expr.ty = zonk_ty(infer, ctx, expr.ty);
    match &mut expr.kind {
        thir::ExprKind::Literal(_)
        | thir::ExprKind::VarRef(_)
        | thir::ExprKind::FnRef(_)
        | thir::ExprKind::ConstRef(_)
        | thir::ExprKind::VariantRef(_, _)
        | thir::ExprKind::VariantCtor { .. }
        | thir::ExprKind::TraitMethodRef { .. }
        | thir::ExprKind::Continue
        | thir::ExprKind::Err => {}

        thir::ExprKind::Binary { lhs, rhs, .. } => {
            zonk_expr(infer, ctx, lhs);
            zonk_expr(infer, ctx, rhs);
        }
        thir::ExprKind::Unary { operand, .. } => zonk_expr(infer, ctx, operand),
        thir::ExprKind::Call { func, args } => {
            zonk_expr(infer, ctx, func);
            for a in args {
                zonk_expr(infer, ctx, a);
            }
        }
        thir::ExprKind::DynamicCall { receiver, args, .. } => {
            zonk_expr(infer, ctx, receiver);
            for a in args {
                zonk_expr(infer, ctx, a);
            }
        }
        thir::ExprKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            zonk_expr(infer, ctx, cond);
            zonk_expr(infer, ctx, then_branch);
            if let Some(e) = else_branch {
                zonk_expr(infer, ctx, e);
            }
        }
        thir::ExprKind::Match { scrutinee, arms } => {
            zonk_expr(infer, ctx, scrutinee);
            for arm in arms {
                zonk_pattern(infer, ctx, &mut arm.pat);
                if let Some(g) = &mut arm.guard {
                    zonk_expr(infer, ctx, g);
                }
                zonk_expr(infer, ctx, &mut arm.body);
            }
        }
        thir::ExprKind::Block { stmts, tail } => {
            for s in stmts {
                zonk_stmt(infer, ctx, s);
            }
            if let Some(t) = tail {
                zonk_expr(infer, ctx, t);
            }
        }
        thir::ExprKind::Ref { operand, .. } => zonk_expr(infer, ctx, operand),
        thir::ExprKind::Field { receiver, ty, .. } => {
            zonk_expr(infer, ctx, receiver);
            *ty = zonk_ty(infer, ctx, *ty);
        }
        thir::ExprKind::Index { base, index } => {
            zonk_expr(infer, ctx, base);
            zonk_expr(infer, ctx, index);
        }
        thir::ExprKind::Cast { expr } => zonk_expr(infer, ctx, expr),
        thir::ExprKind::While { cond, body } => {
            zonk_expr(infer, ctx, cond);
            zonk_expr(infer, ctx, body);
        }
        thir::ExprKind::Loop { body } => zonk_expr(infer, ctx, body),
        thir::ExprKind::For {
            pat,
            iterable,
            body,
            next,
        } => {
            zonk_pattern(infer, ctx, pat);
            zonk_expr(infer, ctx, iterable);
            zonk_expr(infer, ctx, body);
            if let Some(n) = next {
                n.option_ty = zonk_ty(infer, ctx, n.option_ty);
                n.discr_ty = zonk_ty(infer, ctx, n.discr_ty);
                n.ref_iter_ty = zonk_ty(infer, ctx, n.ref_iter_ty);
                n.fn_ty = zonk_ty(infer, ctx, n.fn_ty);
                n.fn_substs = zonk_substitution(infer, ctx, n.fn_substs);
            }
        }
        thir::ExprKind::Array(elems) | thir::ExprKind::Tuple(elems) => {
            for e in elems {
                zonk_expr(infer, ctx, e);
            }
        }
        thir::ExprKind::Struct {
            fields, spread, ..
        } => {
            for (_name, e) in fields {
                zonk_expr(infer, ctx, e);
            }
            if let Some(s) = spread {
                zonk_expr(infer, ctx, s);
            }
        }
        thir::ExprKind::Break { value } | thir::ExprKind::Return { value } => {
            if let Some(v) = value {
                zonk_expr(infer, ctx, v);
            }
        }
        thir::ExprKind::Closure { body, captures, .. } => {
            for c in captures {
                c.ty = zonk_ty(infer, ctx, c.ty);
            }
            zonk_body(infer, ctx, body);
        }
        thir::ExprKind::Range { start, end, .. } => {
            if let Some(s) = start {
                zonk_expr(infer, ctx, s);
            }
            if let Some(e) = end {
                zonk_expr(infer, ctx, e);
            }
        }
        thir::ExprKind::Try { expr } => zonk_expr(infer, ctx, expr),
    }
}

// ---------------------------------------------------------------------------
// Invariant check
// ---------------------------------------------------------------------------

fn has_infer_int_or_float(ctx: &TyCtx, ty: Ty) -> bool {
    let mut found = false;
    walk_ty(ctx, ty, &mut |t| {
        if matches!(
            ctx.ty_kind(t),
            TyKind::Infer(glyim_type::InferVar::Int(_))
                | TyKind::Infer(glyim_type::InferVar::Float(_))
        ) {
            found = true;
        }
    });
    found
}

/// Walk every child `Ty` of `ty` (recursively), calling `f` on `ty` itself
/// and all descendants.
fn walk_ty(ctx: &TyCtx, ty: Ty, f: &mut impl FnMut(Ty)) {
    f(ty);
    match ctx.ty_kind(ty) {
        TyKind::Ref(_, inner, _)
        | TyKind::RawPtr(inner, _)
        | TyKind::Slice(inner)
        | TyKind::Array(inner, _) => walk_ty(ctx, *inner, f),
        TyKind::Adt(_, s)
        | TyKind::FnDef(_, s)
        | TyKind::Closure(_, s)
        | TyKind::Opaque(_, s)
        | TyKind::Tuple(s) => {
            for arg in ctx.substitution_args(*s) {
                if let GenericArg::Ty(t) = arg {
                    walk_ty(ctx, *t, f);
                }
            }
        }
        TyKind::FnPtr(sig) => {
            for arg in ctx.substitution_args(sig.inputs) {
                if let GenericArg::Ty(t) = arg {
                    walk_ty(ctx, *t, f);
                }
            }
            walk_ty(ctx, sig.output, f);
        }
        _ => {}
    }
}


// ---------------------------------------------------------------------------
// ADT definitions
// ---------------------------------------------------------------------------

/// Zonk every ADT definition registered in `ctx`: each struct/variant field
/// type is passed through `zonk_ty`, so a definition-time `TypeRef::Infer`
/// (e.g. the placeholder result slot of a synthetic async state-enum variant)
/// never survives to codegen. Without this, `glyim-codegen-llvm`'s layout
/// pass rejects the ADT with `fn_abi_of failed: UnknownType(Infer(Ty(_)))`.
///
/// Like [`zonk_bodies`], this runs *after* obligation fulfillment — the
/// inference table passed in is the same one that produced the ADT field
/// types, so var bindings established during body type-checking are honoured
/// here as well.
pub fn zonk_adt_defs(infer: &InferenceTable, ctx: &mut glyim_type::TyCtxMut) {
    // Freeze *once* to obtain a `&TyCtx` for the read side of `zonk_ty`
    // (which expects `&TyCtx`). The frozen snapshot shares the same
    // `&'static` type arena with `ctx`, so any `Ty` interned via the frozen
    // view (e.g. a rebuilt `Adt` substitution) is valid in `ctx` — and,
    // because we take the snapshot before mutating, it also sees the current
    // (pre-zonk) `AdtDef`s for the field type lookups.
    let view = ctx.freeze();
    let ids = view.adt_def_ids();
    let mut updated: Vec<(AdtId, glyim_type::AdtDef)> = Vec::with_capacity(ids.len());
    for adt_id in ids {
        let Some(mut def) = view.adt_def(adt_id).cloned() else {
            continue;
        };
        for field in def.fields.iter_mut() {
            field.ty = zonk_ty(infer, &view, field.ty);
        }
        for variant in def.variants.iter_mut() {
            for field in variant.fields.iter_mut() {
                field.ty = zonk_ty(infer, &view, field.ty);
            }
        }
        updated.push((adt_id, def));
    }
    // `register_adt` re-inserts and recomputes variant-type metadata; using
    // it (rather than poking the map directly) keeps the variant_types /
    // interior-mutability caches consistent.
    for (adt_id, def) in updated {
        ctx.register_adt(adt_id, def);
    }
}

/// Whether any ADT field in `ctx` still carries an int/float inference
/// variable. Counterpart to [`body_has_infer_ints_or_floats`] for the
/// debug-only invariant.
pub fn adt_defs_have_infer_ints_or_floats(ctx: &TyCtx) -> bool {
    for adt_id in ctx.adt_def_ids() {
        let Some(def) = ctx.adt_def(adt_id) else {
            continue;
        };
        for field in def.fields.iter() {
            if has_infer_int_or_float(ctx, field.ty) {
                return true;
            }
        }
        for variant in def.variants.iter() {
            for field in variant.fields.iter() {
                if has_infer_int_or_float(ctx, field.ty) {
                    return true;
                }
            }
        }
    }
    false
}
