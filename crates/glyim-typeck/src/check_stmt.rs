//! Statement checking logic for FnCtxt.

use std::collections::HashMap;

use glyim_core::interner::Name;
use glyim_core::primitives::Mutability;
use glyim_hir::{Expr, ExprId};
use glyim_span::Span;
use glyim_type::Ty;

use crate::check_body::FnCtxt;
use crate::thir;

impl<'a> FnCtxt<'a> {
    /// Collect the `ExprId`s that are part of a match arm's *guard* subtree
    /// (the guard expression plus all of its descendant expressions).
    ///
    /// The driving `check` loop below iterates the entire flat `body.exprs`
    /// arena and type-checks every expression as a top-level statement (the
    /// `expr_cache` makes the resulting double-traversal a no-op for exprs that
    /// are also reached via their parent's recursive `check_expr` call).
    ///
    /// A guard is special: the names it references (e.g. `y` in
    /// `Some(y) if y > 0`) are only bound inside the arm's pattern scope, which
    /// the arm establishes *before* calling `check_expr(guard)`. If a guard
    /// subtree were also checked standalone by the driving loop — in the
    /// function scope, before the arm binds `y` — it would spuriously report
    /// "unresolved name `y`". So we skip guard subtrees here and let the arm's
    /// own in-scope `check_expr(guard)` be their sole point of type-checking.
    fn guard_subtree_ids(body: &glyim_hir::Body) -> std::collections::HashSet<glyim_hir::ExprId> {
        use glyim_hir::{Expr, ExprId, MatchArm};
        let mut skip = std::collections::HashSet::new();
        let mut stack: Vec<ExprId> = Vec::new();
        for (_id, expr) in body.exprs.iter_enumerated() {
            if let Expr::Match { arms, .. } = expr {
                for arm in arms {
                    let arm: &MatchArm = arm;
                    // Seed the skip-set with BOTH the guard and the arm body.
                    // The arm body (e.g. `return v`) references names bound by
                    // the arm pattern; it must be checked only inside the
                    // `Expr::Match` handler (which enters the arm scope), never
                    // by the top-level driving loop, or it would be checked
                    // before the pattern binds its names and emit a spurious
                    // "unresolved name" that the expr_cache then freezes.
                    if let Some(guard) = arm.guard {
                        stack.push(guard);
                    }
                    stack.push(arm.body);
                }
            } else if let Expr::Closure { body, .. } = expr {
                // A closure's body is checked only inside `Expr::Closure`
                // (which enters the closure's own scope and binds its
                // parameters); checking it at the top-level loop would resolve
                // `n` against the enclosing scope and emit a spurious
                // "unresolved name".
                stack.push(*body);
            }
        }
        while let Some(eid) = stack.pop() {
            if skip.insert(eid) {
                let children = match &body.exprs[eid] {
                    Expr::Block { stmts, tail } => {
                        let mut v: Vec<ExprId> = stmts.clone();
                        if let Some(t) = tail {
                            v.push(*t);
                        }
                        v
                    }
                    Expr::If {
                        cond,
                        then_branch,
                        else_branch,
                    } => {
                        let mut v = vec![*cond, *then_branch];
                        if let Some(e) = else_branch {
                            v.push(*e);
                        }
                        v
                    }
                    Expr::While { cond, body } => vec![*cond, *body],
                    Expr::Loop { body } => vec![*body],
                    Expr::For { iterable, body, .. } => vec![*iterable, *body],
                    Expr::Match { scrutinee, arms } => {
                        let mut v = vec![*scrutinee];
                        for arm in arms {
                            let arm: &MatchArm = arm;
                            if let Some(g) = arm.guard {
                                v.push(g);
                            }
                            v.push(arm.body);
                        }
                        v
                    }
                    Expr::Call { func, args } => {
                        let mut v = vec![*func];
                        v.extend(args.iter().copied());
                        v
                    }
                    Expr::MethodCall { receiver, args, .. } => {
                        let mut v = vec![*receiver];
                        v.extend(args.iter().copied());
                        v
                    }
                    Expr::Field { receiver, .. } => vec![*receiver],
                    Expr::Index { base, index } => vec![*base, *index],
                    Expr::Unary { expr, .. } => vec![*expr],
                    Expr::Binary { op: _, lhs, rhs } => vec![*lhs, *rhs],
                    Expr::Cast { expr, .. } => vec![*expr],
                    Expr::Ref { expr, .. } => vec![*expr],
                    Expr::Assign { lhs, rhs } => vec![*lhs, *rhs],
                    Expr::Return { value } => value.iter().copied().collect(),
                    Expr::Break { value } => value.iter().copied().collect(),
                    Expr::Closure { body, .. } => vec![*body],
                    Expr::Array(es) | Expr::Tuple(es) => es.clone(),
                    Expr::Let { value, .. } => vec![*value],
                    Expr::Struct { fields, spread, .. } => {
                        let mut v: Vec<ExprId> = fields.iter().map(|(_, e)| *e).collect();
                        if let Some(s) = spread {
                            v.push(*s);
                        }
                        v
                    }
                    Expr::Range { start, end, .. } => {
                        start.iter().chain(end.iter()).copied().collect()
                    }
                    _ => vec![],
                };
                for c in children {
                    if !skip.contains(&c) {
                        stack.push(c);
                    }
                }
            }
        }
        skip
    }

    pub fn check(mut self, params: &[(Name, Ty, Span)]) -> (thir::Body, HashMap<ExprId, Ty>) {
        let mut thir_params = Vec::with_capacity(params.len());
        for (name, ty, span) in params.iter() {
            // CRITICAL: the THIR `Param.local` / `Param.pat.var_id` MUST use the
            // *same* `LocalVarId` that `add_binding` registers in the name map.
            // References to the parameter (`Expr::Path`) resolve through
            // `check_path` -> `env.lookup_by_name`, which returns exactly that
            // binding id. If we instead used `LocalVarId::from_raw(i)` (the
            // parameter *position*), the two id spaces only coincide when the
            // parameter happens to be the first binding in the global env
            // (env id 0 == position 0). For a body type-checked after other
            // bindings already exist (e.g. the `async fn` wrapper desugared by
            // `lower_async`), `add_binding` returns a larger id (say 11) while
            // the param position is still 0 — so the reference resolves to 11
            // but the param binding is at 0. The MIR builder then records
            // `local_var_map[0] = param_local` and the `VarRef(11)` misses,
            // falling back to `LocalIdx(11)` (out of the declared range) — the
            // parameter read returns uninitialized stack garbage. Using the
            // `add_binding` id for both the binding and the param keeps them
            // consistent regardless of check order.
            let local_id = self.env.add_binding(*name, *ty, Mutability::Not);

            thir_params.push(thir::Param {
                name: *name,
                ty: *ty,
                span: *span,
                pat: thir::Pattern::binding(local_id, *name, Mutability::Not, *ty, *span),
                local: local_id,
            });
        }

        // Only the match-arm *guard* and *body* subtrees are skipped from the
        // top-level driving loop: their names are bound inside the arm's
        // pattern scope (which the `Expr::Match` handler sets up before
        // checking the guard and body in `check_expr`), so checking them
        // standalone in the function scope would report spurious "unresolved
        // name" errors. Every other expr in the flat arena is checked as a
        // top-level statement as designed; the `expr_cache` makes the resulting
        // redundant traversal a no-op.
        let guard_skip = Self::guard_subtree_ids(self.body);

        let mut stmts = Vec::new();

        // Drive the type-checking loop over the function body's *top-level*
        // statements only — i.e. the root `Block`'s `stmts` followed by its
        // `tail` — rather than every expression in the flat `body.exprs`
        // arena. The flat arena also contains the root `Block` node itself and
        // each expression's nested children (the `let` value, the tail
        // `VarRef`, etc.). Checking those as standalone top-level statements
        // double-traverses the tree and, worse, re-checks the root `Block`:
        // `check_expr(Block)` recursively visits its child `Expr::Let`, which
        // `check_expr` does not handle (a `let` is a *statement*, not an
        // expression) and therefore lowers to an `Err`/`TyKind::Error` node.
        // That spurious `Error` constant then reaches codegen and ICEs. (This
        // is the root cause of the single-await `TyKind::Error` gap: every
        // `let`-binding or `block_on`-style body was hitting it.)
        // Drive the type-checking loop over the function body's *top-level*
        // statements. Real HIR always wraps the body in a root `Block` (the
        // last `Expr` in the arena), so when one is present we iterate its
        // `stmts` followed by its `tail` — and only those, to avoid re-checking
        // the block itself (which re-emits a spurious `TyKind::Error`).
        //
        // Some callers (hand-built test bodies) push a *flat* expression list
        // with no root `Block`. For those we fall back to driving every
        // expression in the body as a top-level statement.
        let top_level: Vec<glyim_hir::ExprId> = match (0..self.body.exprs.len())
            .map(|i| glyim_hir::ExprId::from_raw(i as u32))
            .rfind(|&rid| matches!(self.body.exprs[rid], glyim_hir::Expr::Block { .. }))
        {
            Some(root) => {
                if let glyim_hir::Expr::Block { stmts, tail } = &self.body.exprs[root] {
                    let mut v = stmts.clone();
                    if let Some(t) = tail {
                        v.push(*t);
                    }
                    v
                } else {
                    Vec::new()
                }
            }
            None => (0..self.body.exprs.len())
                .map(|i| glyim_hir::ExprId::from_raw(i as u32))
                .collect(),
        };
        let len = top_level.len();

        for (pos, &expr_id) in top_level.iter().enumerate() {
            let expr = &self.body.exprs[expr_id];
            if guard_skip.contains(&expr_id) {
                continue;
            }
            let is_tail = pos == len - 1;
            let span = self.expr_span(expr_id);

            match expr {
                Expr::Let { pat, value } => {
                    let (value_expr, value_ty) = self.check_expr(*value);
                    // Bind the pattern into the local environment and build a
                    // THIR `Let` statement (lowered to a storage-live + assign
                    // + bind in MIR).
                    let pat_thir = self.check_pattern(*pat, value_ty);
                    if is_tail {
                        self.unify(Ty::UNIT, self.return_ty, span);
                    }
                    let name = match &pat_thir.kind {
                        thir::PatternKind::Binding { name, .. } => *name,
                        _ => self.ctx.resolver().intern("_"),
                    };
                    stmts.push(thir::Stmt::Let {
                        name,
                        ty: value_ty,
                        pat: pat_thir,
                        init: Some(value_expr),
                        span,
                    });
                }
                Expr::Assign { lhs, rhs } => {
                    let (lhs_expr, lhs_ty) = self.check_expr(*lhs);
                    let (rhs_expr, rhs_ty) = self.check_expr(*rhs);
                    self.unify(rhs_ty, lhs_ty, span);
                    if is_tail {
                        self.unify(Ty::UNIT, self.return_ty, span);
                    }
                    stmts.push(thir::Stmt::Assign {
                        lhs: lhs_expr,
                        rhs: rhs_expr,
                        span,
                    });
                }
                Expr::Return { value } => {
                    let value_opt = value.map(|val_id| {
                        let (val_expr, val_ty) = self.check_expr(val_id);
                        self.unify(val_ty, self.return_ty, span);
                        val_expr
                    });
                    stmts.push(thir::Stmt::Return {
                        value: value_opt,
                        span,
                    });
                }
                _ => {
                    let (thir_expr, ty) = self.check_expr(expr_id);
                    if is_tail && self.return_ty != Ty::UNIT {
                        self.unify(ty, self.return_ty, span);
                    }
                    stmts.push(thir::Stmt::Expr { expr: thir_expr });
                }
            }
        }

        let body = thir::Body {
            owner: self.owner,
            params: thir_params,
            return_ty: self.return_ty,
            stmts,
            span: self.body.span,
        };
        // The per-expression type cache is keyed by HIR ExprId and is consumed
        // by the public `TypeckResult::expr_ty` query after inference
        // resolution (Tier 6.4).
        let expr_types = std::mem::take(&mut self.expr_cache)
            .into_iter()
            .map(|(eid, (_expr, ty))| (eid, ty))
            .collect();
        (body, expr_types)
    }

    /// Type-check a single *statement* HIR node (`let`/`assign`/`return`/expr)
    /// and lower it to a `thir::Stmt`. This is the single source of truth for
    /// how statements are checked — used both by the top-level driving loop in
    /// `check` and by `Expr::Block` in `check_expr` (which previously called
    /// `check_expr` on statement nodes, hitting the `Expr::Let`/`Expr::Assign`
    /// arms that erroneously returned `thir::Expr::err`, corrupting any
    /// statement nested inside a block/if-body).
    pub(crate) fn check_stmt_to_thir(&mut self, stmt_id: glyim_hir::ExprId, is_tail: bool) -> thir::Stmt {
        let expr = &self.body.exprs[stmt_id];
        let span = self.expr_span(stmt_id);
        match expr {
            Expr::Let { pat, value } => {
                let (value_expr, value_ty) = self.check_expr(*value);
                let pat_thir = self.check_pattern(*pat, value_ty);
                if is_tail {
                    self.unify(Ty::UNIT, self.return_ty, span);
                }
                let name = match &pat_thir.kind {
                    thir::PatternKind::Binding { name, .. } => *name,
                    _ => self.ctx.resolver().intern("_"),
                };
                thir::Stmt::Let {
                    name,
                    ty: value_ty,
                    pat: pat_thir,
                    init: Some(value_expr),
                    span,
                }
            }
            Expr::Assign { lhs, rhs } => {
                let (lhs_expr, lhs_ty) = self.check_expr(*lhs);
                let (rhs_expr, rhs_ty) = self.check_expr(*rhs);
                self.unify(rhs_ty, lhs_ty, span);
                if is_tail {
                    self.unify(Ty::UNIT, self.return_ty, span);
                }
                thir::Stmt::Assign {
                    lhs: lhs_expr,
                    rhs: rhs_expr,
                    span,
                }
            }
            Expr::Return { value } => {
                let value_opt = value.map(|val_id| {
                    let (val_expr, val_ty) = self.check_expr(val_id);
                    self.unify(val_ty, self.return_ty, span);
                    val_expr
                });
                thir::Stmt::Return {
                    value: value_opt,
                    span,
                }
            }
            _ => {
                let (thir_expr, ty) = self.check_expr(stmt_id);
                if is_tail && self.return_ty != Ty::UNIT {
                    self.unify(ty, self.return_ty, span);
                }
                thir::Stmt::Expr { expr: thir_expr }
            }
        }
    }
}
