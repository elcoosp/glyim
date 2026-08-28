//! Expression checking logic for FnCtxt.

use std::collections::{HashMap, HashSet};

use glyim_core::def_id::{AdtId, ClosureId, FnDefId, TraitDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::*;
use glyim_diag::GlyimDiagnostic;
use glyim_hir::*;
use glyim_span::Span;
use glyim_type::{AdtKind, Const, ConstKind, FnSig, GenericArg, Region, Ty, TyKind};
use glyim_type::display::PrintTy;

use crate::check_body::FnCtxt;
use crate::thir;
use crate::unify::{literal_ty, thir_literal};

/// How a method call (`recv.method(..)`) should be dispatched after
/// type-checking.
#[derive(Clone)]
pub(crate) enum MethodDispatch {
    /// The receiver's type has a concrete `impl` providing this method, so the
    /// call is statically resolved to that impl function. Lowered to a direct
    /// `Call` of `FnDefId`.
    Static(FnDefId),
    /// The receiver is a generic param (`f: F` where `F: Trait`), so the
    /// concrete `impl` is unknown until monomorphization. Carries the trait so
    /// the call can be devirtualized against the instantiated receiver type.
    /// Lowered to a `DynamicCall` carrying the trait + method identity.
    Virtual(TraitDefId),
}

/// Collect the enum variant indices covered by a THIR `Pattern`. Used by the
/// match-exhaustiveness diagnostic (plan §22.1). A `Wild` or plain `Binding`
/// pattern is a catch-all (covers every variant); an `Or` pattern covers the
/// union of its sub-patterns; a `Struct` variant pattern covers exactly its
/// `variant_idx`. Other pattern kinds (literal/range/tuple/slice) do not cover
/// enum variants.
fn collect_covered_variants(pat: &thir::Pattern, out: &mut HashSet<u32>, has_wildcard: &mut bool) {
    match &pat.kind {
        thir::PatternKind::Wild => *has_wildcard = true,
        thir::PatternKind::Binding { subpattern, .. } => {
            if subpattern.is_none() {
                *has_wildcard = true;
            } else if let Some(sub) = subpattern {
                collect_covered_variants(sub, out, has_wildcard);
            }
        }
        thir::PatternKind::Struct { variant_idx, .. } => {
            out.insert(*variant_idx);
        }
        thir::PatternKind::Or(pats) => {
            for p in pats {
                collect_covered_variants(p, out, has_wildcard);
            }
        }
        thir::PatternKind::Tuple(pats) => {
            for p in pats {
                collect_covered_variants(p, out, has_wildcard);
            }
        }
        _ => {}
    }
}

/// Align an impl-level generic parameter carried inside a formal receiver type
/// with the corresponding concrete argument type, recording the mapping
/// `param_index -> concrete_arg` into `subst`.
///
/// Example: a method of `impl<T> Future for ReadyT<T>` has formal receiver
/// `&mut ReadyT<T_impl>`. When called with a `ReadyT<i32>` argument, this maps
/// `T_impl.index -> i32` so the method's return type `Poll<T_impl>` instantiates
/// to `Poll<i32>` at the call site instead of keeping the rigid impl param.
///
/// Handles `Ref`/`Adt` wrappers and recurses into the ADT's generic args, which
/// may themselves be `Ref`/`Adt` (e.g. `Box<Vec<T>>`). Positionally aligns the
/// formal's generic args with the actual's; a `Param` in the formal position is
/// mapped to the actual's argument at that position.
fn align_impl_param_subst(
    ctx: &glyim_type::TyCtxMut,
    formal: glyim_type::Ty,
    actual: glyim_type::Ty,
    subst: &mut std::collections::HashMap<u32, glyim_type::GenericArg>,
) {
    let formal = match ctx.ty_kind(formal) {
        glyim_type::TyKind::Ref(_, inner, _) => *inner,
        other => match other {
            glyim_type::TyKind::Adt(..) => formal,
            _ => return,
        },
    };
    let actual = match ctx.ty_kind(actual) {
        glyim_type::TyKind::Ref(_, inner, _) => *inner,
        other => match other {
            glyim_type::TyKind::Adt(..) => actual,
            _ => return,
        },
    };
    let (TyKind::Adt(_, fsub), TyKind::Adt(_, asub)) =
        (ctx.ty_kind(formal), ctx.ty_kind(actual))
    else {
        return;
    };
    let formal_args = ctx.substitution_args(*fsub);
    let actual_args = ctx.substitution_args(*asub);
    if formal_args.len() != actual_args.len() {
        return;
    }
    for (fa, aa) in formal_args.iter().zip(actual_args.iter()) {
        match fa {
            glyim_type::GenericArg::Ty(ft) => {
                if let glyim_type::TyKind::Param(p) = ctx.ty_kind(*ft) {
                    subst.insert(p.index, aa.clone());
                } else {
                    // Recurse into nested ADT args (e.g. Box<Vec<T>>).
                    align_impl_param_subst(ctx, *ft, match aa {
                        glyim_type::GenericArg::Ty(t) => *t,
                        _ => continue,
                    }, subst);
                }
            }
            _ => {}
        }
    }
}

impl<'a> FnCtxt<'a> {
    pub fn check_expr(&mut self, expr_id: ExprId) -> (thir::Expr, Ty) {
        if let Some(cached) = self.expr_cache.get(&expr_id) {
            return (cached.0.clone(), cached.1);
        }

        let expr = &self.body.exprs[expr_id];
        let span = self.expr_span(expr_id);

        let result = match expr {
            Expr::Literal(lit) => {
                let ty = literal_ty(self.ctx, lit);
                (
                    thir::Expr {
                        kind: thir::ExprKind::Literal(thir_literal(lit)),
                        ty,
                        span,
                    },
                    ty,
                )
            }

            Expr::Path(path) => self.check_path(path, span),

            Expr::Block { stmts, tail } => {
                let mut thir_stmts = Vec::new();
                for &stmt_id in stmts {
                    // Statements are *not* expressions: routing them through
                    // `check_expr` hits the `Expr::Let`/`Expr::Assign` arms
                    // that return `thir::Expr::err`, corrupting any statement
                    // nested inside a block (e.g. an `if` body). Use the shared
                    // statement checker that produces proper `thir::Stmt`s.
                    thir_stmts.push(self.check_stmt_to_thir(stmt_id, false));
                }
                if let Some(tail_id) = tail {
                    let (tail_expr, tail_ty) = self.check_expr(*tail_id);
                    let block_expr = thir::Expr {
                        kind: thir::ExprKind::Block {
                            stmts: thir_stmts,
                            tail: Some(Box::new(tail_expr)),
                        },
                        ty: tail_ty,
                        span,
                    };
                    (block_expr, tail_ty)
                } else {
                    let unit_expr = thir::Expr {
                        kind: thir::ExprKind::Block {
                            stmts: thir_stmts,
                            tail: None,
                        },
                        ty: Ty::UNIT,
                        span,
                    };
                    (unit_expr, Ty::UNIT)
                }
            }

            Expr::Unary { op, expr: operand } => {
                let (inner_expr, inner_ty) = self.check_expr(*operand);
                let result_ty = match op {
                    UnOp::Neg => {
                        if matches!(
                            self.ctx.ty_kind(inner_ty),
                            TyKind::Int(_) | TyKind::Float(_)
                        ) {
                            inner_ty
                        } else {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "cannot apply unary negation to non-numeric type",
                            ));
                            Ty::ERROR
                        }
                    }
                    UnOp::Not => {
                        if matches!(self.ctx.ty_kind(inner_ty), TyKind::Bool | TyKind::Int(_)) {
                            inner_ty
                        } else {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "cannot apply unary not to non-bool/non-int type",
                            ));
                            Ty::ERROR
                        }
                    }
                    UnOp::Deref => match self.ctx.ty_kind(inner_ty) {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                        _ => {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "cannot dereference non-pointer type",
                            ));
                            Ty::ERROR
                        }
                    },
                };
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::Unary {
                        op: *op,
                        operand: Box::new(inner_expr),
                    },
                    ty: result_ty,
                    span,
                };
                (thir_expr, result_ty)
            }

            Expr::Binary { op, lhs, rhs } => {
                let (lhs_expr, lhs_ty) = self.check_expr(*lhs);
                let (rhs_expr, rhs_ty) = self.check_expr(*rhs);

                let operand_ty = if self.unify(lhs_ty, rhs_ty, span) {
                    lhs_ty
                } else {
                    Ty::ERROR
                };

                let result_ty = match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                        Ty::BOOL
                    }
                    BinOp::And | BinOp::Or => {
                        self.unify(operand_ty, Ty::BOOL, span);
                        Ty::BOOL
                    }
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                        if !matches!(
                            self.ctx.ty_kind(operand_ty),
                            TyKind::Int(_) | TyKind::Uint(_)
                        ) && operand_ty != Ty::ERROR
                        {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "bitwise operators require integer operands",
                            ));
                        }
                        operand_ty
                    }
                    _ => operand_ty,
                };

                if result_ty == Ty::ERROR || operand_ty == Ty::ERROR {
                    (thir::Expr::err(span), Ty::ERROR)
                } else {
                    (
                        thir::Expr {
                            kind: thir::ExprKind::Binary {
                                op: *op,
                                lhs: Box::new(lhs_expr),
                                rhs: Box::new(rhs_expr),
                            },
                            ty: result_ty,
                            span,
                        },
                        result_ty,
                    )
                }
            }

            Expr::Ref { expr, mutability } => {
                let (inner_expr, inner_ty) = self.check_expr(*expr);
                // If we just took a mutable reference to a captured local,
                // record the mutating use so capture analysis can classify it
                // as `ByRef(Mut)`.
                if *mutability == Mutability::Mut
                    && let thir::ExprKind::VarRef(id) = inner_expr.kind
                        && let Some(entry) = self
                            .capture_log
                            .iter_mut()
                            .rev()
                            .find(|(vid, ..)| *vid == id)
                        {
                            entry.2 = true;
                        }
                let ref_ty = self.ctx.mk_ref(Region::Erased, inner_ty, *mutability);
                (
                    thir::Expr {
                        kind: thir::ExprKind::Ref {
                            mutability: *mutability,
                            operand: Box::new(inner_expr),
                        },
                        ty: ref_ty,
                        span,
                    },
                    ref_ty,
                )
            }

            Expr::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let (cond_expr, cond_ty) = self.check_expr(*cond);
                self.unify(cond_ty, Ty::BOOL, span);

                let (then_expr, then_ty) = self.check_expr(*then_branch);

                let (else_opt, else_ty) = if let Some(else_id) = else_branch {
                    let (e, t) = self.check_expr(*else_id);
                    (Some(Box::new(e)), t)
                } else {
                    (None, Ty::UNIT)
                };

                let result_ty = if then_ty == Ty::ERROR || else_ty == Ty::ERROR {
                    Ty::ERROR
                } else if self.unify(then_ty, else_ty, span) {
                    then_ty
                } else {
                    Ty::ERROR
                };

                (
                    thir::Expr {
                        kind: thir::ExprKind::If {
                            cond: Box::new(cond_expr),
                            then_branch: Box::new(then_expr),
                            else_branch: else_opt,
                        },
                        ty: result_ty,
                        span,
                    },
                    result_ty,
                )
            }

            Expr::While { cond, body } => {
                let (cond_expr, cond_ty) = self.check_expr(*cond);
                self.unify(cond_ty, Ty::BOOL, span);
                let (body_expr, _) = self.check_expr(*body);
                (
                    thir::Expr {
                        kind: thir::ExprKind::While {
                            cond: Box::new(cond_expr),
                            body: Box::new(body_expr),
                        },
                        ty: Ty::UNIT,
                        span,
                    },
                    Ty::UNIT,
                )
            }

            Expr::Loop { body } => {
                let (body_expr, _) = self.check_expr(*body);
                (
                    thir::Expr {
                        kind: thir::ExprKind::Loop {
                            body: Box::new(body_expr),
                        },
                        ty: Ty::NEVER,
                        span,
                    },
                    Ty::NEVER,
                )
            }

            Expr::For {
                pat,
                iterable,
                body,
            } => {
                let (iter_expr, _iter_ty) = self.check_expr(*iterable);
                let iter_ty = iter_expr.ty;
                // Phase 1 (GLYIM_DESTUB_PLAN): resolve the `Iterator::next`
                // method for the iterable's concrete type here (where we hold a
                // `&mut TyCtxMut` and the resolved `FnDefId` + signatures), and
                // thread it through the THIR `For` node. The lowering pass then
                // takes the real multi-iteration path without re-solving the
                // trait. Mirrors `resolve_trait_method_fn`'s impl scan, scoped
                // to the `Iterator` trait's `next` method.
                let next_info = {
                    let iterator_name = self.ctx.resolver().intern("Iterator");
                    let iterator_path = glyim_hir::Path {
                        segments: vec![glyim_hir::PathSegment {
                            name: iterator_name,
                            generic_args: None,
                        }],
                        kind: glyim_core::path::PathKind::Plain,
                    };
                    let next_name = self.ctx.resolver().intern("next");
                    match crate::tyconv::resolve_path_to_trait_def_id(
                        self.def_map,
                        self.ctx,
                        &iterator_path,
                        span,
                    ) {
                        Some(trait_def_id) => self
                            .resolve_trait_method_fn(iter_ty, trait_def_id, next_name, span)
                            .map(|fn_def_id| {
                                let option_ty = self
                                    .ctx
                                    .fn_sig(fn_def_id)
                                    .map(|s| s.output)
                                    .unwrap_or_else(|| self.ctx.error_ty());
                                let ref_iter_ty =
                                    self.ctx.mk_ref(Region::Erased, iter_ty, Mutability::Mut);
                                let fn_substs = self.ctx.intern_substitution(vec![]);
                                let fn_ty = self
                                    .ctx
                                    .mk_ty(TyKind::FnDef(fn_def_id, fn_substs));
                                let discr_ty =
                                    self.ctx.mk_ty(TyKind::Uint(UintTy::U8));
                                thir::ForIteratorNext {
                                    fn_def_id,
                                    fn_substs,
                                    option_ty,
                                    discr_ty,
                                    ref_iter_ty,
                                    fn_ty,
                                }
                            }),
                        None => None,
                    }
                };
                // Resolve Iterator::Item for the iterable type.
                // For now, we use a fresh inference variable to represent the item type;
                // the actual resolution will be done by the trait solver when lowering.
                let item_ty = self.fresh_infer_ty();
                // Phase 1 (GLYIM_DESTUB_PLAN): the loop-pattern bindings (`x` in
                // `for x in ..`) must remain in scope while the body is checked.
                // The previous code opened+closed a scope around `check_pattern`
                // and then opened a *separate* scope for the body, so `x` was
                // already out of scope by the time the body resolved it — every
                // for-loop body raised `unresolved name x` and fell back to the
                // single-iteration path. Bind the pattern and check the body in
                // the *same* scope.
                self.env.enter_scope();
                let pat_thir = self.check_pattern(*pat, item_ty);
                let (body_expr, _) = self.check_expr(*body);
                self.env.leave_scope();

                (
                    thir::Expr {
                        kind: thir::ExprKind::For {
                            pat: Box::new(pat_thir),
                            iterable: Box::new(iter_expr),
                            body: Box::new(body_expr),
                            next: next_info,
                        },
                        ty: Ty::UNIT,
                        span,
                    },
                    Ty::UNIT,
                )
            }

            Expr::Match { scrutinee, arms } => {
                let (scrut_expr, scrut_ty) = self.check_expr(*scrutinee);
                let result_ty = self.fresh_infer_ty();

                let mut thir_arms = Vec::with_capacity(arms.len());
                for arm in arms {
                    self.env.enter_scope();
                    let pat_thir = self.check_pattern(arm.pat, scrut_ty);
                    let guard_thir = arm
                        .guard
                        .map(|guard_id| Box::new(self.check_expr(guard_id).0));
                    let (body_expr, body_ty) = self.check_expr(arm.body);
                    self.env.leave_scope();
                    if body_ty != Ty::ERROR {
                        self.unify(body_ty, result_ty, span);
                    }
                    thir_arms.push(thir::MatchArm {
                        pat: pat_thir,
                        guard: guard_thir,
                        body: body_expr,
                    });
                }

                let resolved_result = self.infer.resolve_ty_shallow(self.ctx, result_ty);
                let final_ty = if resolved_result == Ty::ERROR {
                    Ty::ERROR
                } else if matches!(self.ctx.ty_kind(resolved_result), TyKind::Infer(_)) {
                    Ty::UNIT
                } else {
                    resolved_result
                };

                // Plan §22.1 (prereq): exhaustiveness check for `match` over an
                // enum. Collect the variant indices covered by the patterns; if
                // the scrutinee is an enum and not every variant is covered by a
                // concrete variant pattern (and there is no wildcard catch-all),
                // emit a `NonExhaustiveMatch` diagnostic listing the missing
                // variant names so the LSP can offer "Add missing match arm(s)".
                if let TyKind::Adt(adt_id, _) = self.ctx.ty_kind(scrut_ty) {
                    if let Some(adt) = self.ctx.adt_def(*adt_id) {
                        if adt.kind == AdtKind::Enum {
                            let mut covered: HashSet<u32> = HashSet::new();
                            let mut has_wildcard = false;
                            for arm in &thir_arms {
                                collect_covered_variants(&arm.pat, &mut covered, &mut has_wildcard);
                            }
                            if !has_wildcard {
                                let missing: Vec<String> = (0..adt.variants.len())
                                    .filter(|i| !covered.contains(&(*i as u32)))
                                    .map(|i| {
                                        self.ctx.name_str(adt.variants[i as usize].name).to_string()
                                    })
                                    .collect();
                                if !missing.is_empty() {
                                    // Plan §5.1 / §5.2: carry each missing variant's
                                    // declared shape so the LSP can synthesize an
                                    // arity-correct (compiling) match arm and reads a
                                    // typed payload instead of re-parsing prose.
                                    let shapes: Vec<(String, glyim_diag::VariantShape)> = missing
                                        .iter()
                                        .zip((0..adt.variants.len()).filter(|i| !covered.contains(&(*i as u32))))
                                        .map(|(name, vi)| {
                                            let shape = match adt.variants[vi as usize].style {
                                                glyim_type::adt_def::VariantStyle::Unit => glyim_diag::VariantShape::Unit,
                                                glyim_type::adt_def::VariantStyle::Tuple => glyim_diag::VariantShape::Tuple(adt.variants[vi as usize].fields.len()),
                                                glyim_type::adt_def::VariantStyle::Struct => glyim_diag::VariantShape::Struct(
                                                    adt.variants[vi as usize]
                                                        .fields
                                                        .iter()
                                                        .map(|f| self.ctx.name_str(f.name).to_string())
                                                        .collect(),
                                                ),
                                            };
                                            (name.clone(), shape)
                                        })
                                        .collect();
                                    self.diagnostics.push(GlyimDiagnostic::non_exhaustive_match(
                                        span, &missing, &shapes,
                                    ));
                                }
                            }
                        }
                    }
                }

                (
                    thir::Expr {
                        kind: thir::ExprKind::Match {
                            scrutinee: Box::new(scrut_expr),
                            arms: thir_arms,
                        },
                        ty: final_ty,
                        span,
                    },
                    final_ty,
                )
            }

            Expr::Call { func, args } => {
                // Detect a path-qualified trait method call
                // `Trait::method(receiver, ..)` *before* resolving the callee,
                // so we can statically dispatch to the concrete impl function
                // selected by the receiver's type (see
                // `resolve_trait_method_fn`). We read the HIR callee path here
                // (not the THIR node) because trait-method resolution needs the
                // receiver, which only the `Call` has.
                let trait_call = match &self.body.exprs[*func] {
                    Expr::Path(path) if path.segments.len() == 2 => {
                        let trait_path = glyim_hir::Path {
                            segments: vec![glyim_hir::PathSegment {
                                name: path.segments[0].name,
                                generic_args: None,
                            }],
                            kind: path.kind,
                        };
                        crate::tyconv::resolve_path_to_trait_def_id(
                            self.def_map,
                            self.ctx,
                            &trait_path,
                            span,
                        )
                        .filter(|tid| self.ctx.trait_def(*tid).is_some())
                        .map(|tid| (tid, path.segments[1].name))
                    }
                    _ => None,
                };

                let (func_expr, func_ty) = self.check_expr(*func);
                let mut arg_exprs = Vec::with_capacity(args.len());
                for &arg_id in args {
                    arg_exprs.push(self.check_expr(arg_id).0);
                }

                if let Some((trait_def_id, method_name)) = trait_call {
                    let recv_ty = arg_exprs
                        .first()
                        .map(|e| e.ty)
                        .unwrap_or(Ty::ERROR);
                    if let Some(fn_def_id) =
                        self.resolve_trait_method_fn(recv_ty, trait_def_id, method_name, span)
                    {
                        let substs = self.ctx.intern_substitution(vec![]);
                        let fn_ty = self.ctx.mk_ty(TyKind::FnDef(fn_def_id, substs));
                        let ret_ty = self.instantiate_fn_sig(fn_def_id, span);
                        let callee = thir::Expr {
                            kind: thir::ExprKind::FnRef(fn_def_id),
                            ty: fn_ty,
                            span,
                        };
                        // Overwrite the standalone callee node (which
                        // `check_path` left as `Err`) so no stray
                        // trait-method node survives into MIR lowering.
                        self.expr_cache.insert(*func, (callee.clone(), fn_ty));
                        return (
                            thir::Expr {
                                kind: thir::ExprKind::Call {
                                    func: Box::new(callee),
                                    args: arg_exprs,
                                },
                                ty: ret_ty,
                                span,
                            },
                            ret_ty,
                        );
                    } else {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            format!(
                                "no impl of trait method `{}` found for receiver type",
                                self.ctx.name_str(method_name)
                            ),
                        ));
                        return (thir::Expr::err(span), Ty::ERROR);
                    }
                }

                let (is_fn_def, def_id, is_error) = match self.ctx.ty_kind(func_ty) {
                    TyKind::FnDef(def_id, _) => (true, *def_id, false),
                    TyKind::FnPtr(sig) => {
                        let expected_args = sig.inputs.len() as usize;
                        if args.len() != expected_args && !sig.c_variadic {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                format!(
                                    "function pointer expects {} arguments, got {}",
                                    expected_args,
                                    args.len()
                                ),
                            ));
                        }
                        return (
                            thir::Expr {
                                kind: thir::ExprKind::Call {
                                    func: Box::new(func_expr),
                                    args: arg_exprs,
                                },
                                ty: sig.output,
                                span,
                            },
                            sig.output,
                        );
                    }
                    TyKind::Error => (false, FnDefId::from_raw(0), true),
                    TyKind::Closure(closure_id, _) => {
                        // A closure-typed callee: the closure's registered
                        // signature carries [captures.., params..]; the call
                        // supplies only the explicit parameters.
                        if let Some(sig) = self.ctx.closure_sig(*closure_id) {
                            let inputs = self.ctx.substitution_args(sig.inputs);
                            let capture_count = self
                                .ctx
                                .closure_adt(*closure_id)
                                .and_then(|adt_id| self.ctx.adt_def(adt_id))
                                .and_then(|adt| adt.variants.first())
                                .map(|v| v.fields.len())
                                .unwrap_or(0);
                            let param_count = inputs.len() - capture_count;
                            if args.len() != param_count {
                                self.diagnostics.push(GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "closure expects {} arguments, got {}",
                                        param_count, args.len()
                                    ),
                                ));
                            }
                            return (
                                thir::Expr {
                                    kind: thir::ExprKind::Call {
                                        func: Box::new(func_expr),
                                        args: arg_exprs,
                                    },
                                    ty: sig.output,
                                    span,
                                },
                                sig.output,
                            );
                        }
                        (false, FnDefId::from_raw(0), false)
                    }
                    _ => (false, FnDefId::from_raw(0), false),
                };

                let (ret_ty, callee_ty) = if is_fn_def {
                    // Plan unstub-5 P5: generic call instantiation. For a generic
                    // function `fn id<T>(x: T) -> T`, build a substitution from
                    // the formal parameter types (which carry the type params)
                    // to the *call argument* types, then instantiate the
                    // registered return type through it. This makes
                    // `id(40)` return `i32` (not the rigid `T`) and is what
                    // unblocks `block_on<F: MyFuture>`-style dispatch.
                    if let Some(sig) = self.ctx.fn_sig(def_id) {
                        let mut subst: std::collections::HashMap<u32, GenericArg> =
                            std::collections::HashMap::new();
                        // Collect the formal input types into an owned vec so the
                        // immutable borrow of `self.ctx` ends before the mutable
                        // `intern_substitution`/`mk_ty` calls below.
                        let inputs: Vec<GenericArg> =
                            self.ctx.substitution_args(sig.inputs).to_vec();
                        for (i, arg_expr) in arg_exprs.iter().enumerate() {
                            if let Some(GenericArg::Ty(param_ty)) = inputs.get(i) {
                                // Direct bare-param formal (e.g. `fn id<T>(x: T)`):
                                // map the param index to the argument's concrete type.
                                if let TyKind::Param(pt) = self.ctx.ty_kind(*param_ty) {
                                    subst.insert(pt.index, GenericArg::Ty(arg_expr.ty));
                                } else {
                                    // Structural alignment: a formal that is an
                                    // `Adt`/`Ref(Adt)` whose generic args carry the
                                    // *impl's* type parameters (e.g. the `self`
                                    // receiver `&mut ReadyT<T_impl>` of an
                                    // `impl<T> Trait for ReadyT<T>` method). Align
                                    // each `Param` in the formal's ADT args with the
                                    // corresponding concrete argument ADT args so the
                                    // impl-level parameter is instantiated at the
                                    // call site (T_impl := i32). Without this the
                                    // method's return type keeps the rigid impl
                                    // param and unification against the caller's
                                    // concrete type fails (mismatched types: T vs i32).
                                    align_impl_param_subst(
                                        &self.ctx,
                                        *param_ty,
                                        arg_expr.ty,
                                        &mut subst,
                                    );
                                }
                            }
                        }
                        let ret = self.ctx.subst_ty(sig.output, &subst);
                        // Rebuild the callee's `FnDef` type carrying the inferred
                        // substitution. Without this, the `FnRef` node keeps the
                        // unbound generic type (`FnDef(id, [])`), so MIR lowering
                        // emits `MirConstKind::Fn(id, [])` with empty substs and
                        // monomorphization never instantiates the generic body
                        // (e.g. `id<i32>`), leaving a local typed `TyKind::Param`
                        // that codegen cannot lower. This is the single-await
                        // `TyKind::Error` gap (generic `Future::Output`/`block_on`).
                        let new_substs = self.ctx.intern_substitution(
                            inputs
                                .iter()
                                .map(|a| match a {
                                    GenericArg::Ty(pt) => {
                                        if let TyKind::Param(p) = self.ctx.ty_kind(*pt) {
                                            subst
                                                .get(&p.index)
                                                .cloned()
                                                .unwrap_or(GenericArg::Ty(*pt))
                                        } else {
                                            a.clone()
                                        }
                                    }
                                    other => other.clone(),
                                })
                                .collect(),
                        );
                        let callee_ty = self.ctx.mk_ty(TyKind::FnDef(def_id, new_substs));
                        (ret, callee_ty)
                    } else {
                        (self.instantiate_fn_sig(def_id, span), func_ty)
                    }
                } else if is_error {
                    (Ty::ERROR, func_ty)
                } else {
                    self.diagnostics.push(GlyimDiagnostic::type_error(
                        span,
                        "call to non-function type",
                    ));
                    (Ty::ERROR, func_ty)
                };

                // Propagate the instantiated callee type onto the `FnRef` node so
                // MIR lowering emits `MirConstKind::Fn(def_id, substs)` with the
                // concrete substitution, enabling monomorphization.
                let mut func_expr = func_expr;
                func_expr.ty = callee_ty;
                self.expr_cache.insert(*func, (func_expr.clone(), callee_ty));

                // DEBUG: print callee + ret_ty for EVERY call
                {
                    let _cn = if let Expr::Path(p) = &self.body.exprs[*func] {
                        p.as_name().map(|n| self.ctx.name_str(n).to_string()).or_else(|| Some(format!("{:?}", p)))
                    } else { None };
                    let _raw_name = if let Expr::Path(p) = &self.body.exprs[*func] {
                        Some(p.segments.iter().map(|s| format!("{:?}=`{}`", s.name, self.ctx.name_str(s.name))).collect::<Vec<_>>().join("::"))
                    } else { None };
                    let _dbg_def = if is_fn_def { Some(def_id) } else { None };
                    let dbg_sigout = if is_fn_def { self.ctx.fn_sig(def_id).map(|s| PrintTy::new(s.output, &*self.ctx)) } else { None };
                    let _sigout_str = match dbg_sigout {
                        Some(p) => format!("{}", p),
                        None => "<none>".to_string(),
                    };
                }

                if matches!(self.ctx.ty_kind(ret_ty), TyKind::Error) {
                    let _callee_name = if let Expr::Path(p) = &self.body.exprs[*func] {
                        p.as_name().map(|n| self.ctx.name_str(n))
                    } else {
                        None
                    };
                }
                (
                    thir::Expr {
                        kind: thir::ExprKind::Call {
                            func: Box::new(func_expr),
                            args: arg_exprs,
                        },
                        ty: ret_ty,
                        span,
                    },
                    ret_ty,
                )
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                let (recv_expr, recv_ty) = self.check_expr(*receiver);
                let mut arg_exprs = Vec::new();
                for &arg_id in args {
                    arg_exprs.push(self.check_expr(arg_id).0);
                }
                let (ret_ty, dispatch) = self.resolve_method_call(recv_ty, *method, span);
                let ret_ty = self.extract_return_ty(ret_ty, span);
                let thir_expr = match dispatch {
                    Some(MethodDispatch::Static(fn_def_id)) => {
                        // Static dispatch: call the concrete impl function
                        // directly. `self` is the first argument (the
                        // receiver); the remaining `arg_exprs` are the method's
                        // explicit parameters.
                        let substs = self.ctx.intern_substitution(vec![]);
                        let fn_ty = self.ctx.mk_ty(TyKind::FnDef(fn_def_id, substs));
                        let callee = thir::Expr {
                            kind: thir::ExprKind::FnRef(fn_def_id),
                            ty: fn_ty,
                            span,
                        };
                        let mut call_args = Vec::with_capacity(arg_exprs.len() + 1);
                        call_args.push(recv_expr);
                        call_args.extend(arg_exprs);
                        thir::Expr {
                            kind: thir::ExprKind::Call {
                                func: Box::new(callee),
                                args: call_args,
                            },
                            ty: ret_ty,
                            span,
                        }
                    }
                    Some(MethodDispatch::Virtual(trait_def_id)) => {
                        // Generic-bound receiver (`f: F` where `F: Trait`):
                        // the concrete impl is unknown until monomorphization.
                        // Carry the trait + method identity so the call can be
                        // devirtualized against the instantiated receiver type.
                        let mut dyn_args = Vec::with_capacity(arg_exprs.len() + 1);
                        dyn_args.push(recv_expr.clone());
                        dyn_args.extend(arg_exprs);
                        thir::Expr {
                            kind: thir::ExprKind::DynamicCall {
                                receiver: Box::new(recv_expr),
                                trait_def_id,
                                method_name: *method,
                                args: dyn_args,
                            },
                            ty: ret_ty,
                            span,
                        }
                    }
                    None => {
                        // No dispatch resolved and no diagnostic was emitted
                        // (shouldn't happen — resolve_method_call always
                        // diagnoses). Fall back to treating the receiver as the
                        // callee to avoid an ICE, matching historical behavior.
                        thir::Expr {
                            kind: thir::ExprKind::Call {
                                func: Box::new(recv_expr),
                                args: arg_exprs,
                            },
                            ty: ret_ty,
                            span,
                        }
                    }
                };
                (thir_expr, ret_ty)
            }

            Expr::Field { receiver, field } => {
                let (recv_expr, recv_ty) = self.check_expr(*receiver);

                // Field access auto-derefs its receiver, mirroring Rust:
                // `(&mut Counter).field` and `(&Counter).field` both resolve to
                // the underlying ADT/tuple's field. Peel reference layers until
                // we reach the pointee type before looking up the field.
                let mut adj_recv_ty = recv_ty;
                while let TyKind::Ref(_, inner, _) = self.ctx.ty_kind(adj_recv_ty) {
                    adj_recv_ty = *inner;
                }

                let field_ty = {
                    match self.ctx.ty_kind(adj_recv_ty) {
                    TyKind::Adt(adt_id, substs) => {
                        self.lookup_field_ty_with_substs(*adt_id, *field, span, *substs)
                    }
                    TyKind::Tuple(substs) => {
                        let idx = self.ctx.name_str(*field).parse::<usize>().ok();
                        if let Some(idx) = idx {
                            let args = self.ctx.substitution_args(*substs);
                            if idx < args.len() {
                                if let GenericArg::Ty(ty) = args[idx] {
                                    ty
                                } else {
                                    Ty::ERROR
                                }
                            } else {
                                self.diagnostics.push(GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "tuple index {} out of bounds (length {})",
                                        idx,
                                        args.len()
                                    ),
                                ));
                                Ty::ERROR
                            }
                        } else {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                format!("no field `{}` on tuple", self.ctx.name_str(*field)),
                            ));
                            Ty::ERROR
                        }
                    }
                    _ => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            "field access on non-ADT, non-tuple type",
                        ));
                        Ty::ERROR
                    }
                }
                };

                (
                    thir::Expr {
                        kind: thir::ExprKind::Field {
                            receiver: Box::new(recv_expr),
                            field: *field,
                            ty: field_ty,
                        },
                        ty: field_ty,
                        span,
                    },
                    field_ty,
                )
            }

            Expr::Index { base, index } => {
                let (base_expr, base_ty) = self.check_expr(*base);
                let (idx_expr, idx_ty) = self.check_expr(*index);
                // Check if the index is a Range expression.
                if let thir::ExprKind::Range { .. } = idx_expr.kind {
                    // Slicing: result type is slice of element type.
                    let elem_ty = match self.ctx.ty_kind(base_ty) {
                        TyKind::Array(elem, _) | TyKind::Slice(elem) => *elem,
                        _ => {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "slicing requires array or slice type",
                            ));
                            self.fresh_infer_ty()
                        }
                    };
                    let slice_ty = self.ctx.mk_ty(TyKind::Slice(elem_ty));
                    let thir_expr = thir::Expr {
                        kind: thir::ExprKind::Index {
                            base: Box::new(base_expr),
                            index: Box::new(idx_expr),
                        },
                        ty: slice_ty,
                        span,
                    };
                    (thir_expr, slice_ty)
                } else {
                    // Regular indexing: check integer type.
                    if !matches!(self.ctx.ty_kind(idx_ty), TyKind::Int(_) | TyKind::Uint(_))
                        && idx_ty != Ty::ERROR
                    {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            "index expression must have integer type",
                        ));
                    }
                    let elem_ty = match self.ctx.ty_kind(base_ty) {
                        TyKind::Array(elem, _) | TyKind::Slice(elem) => *elem,
                        _ => {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "indexing operation requires array or slice type",
                            ));
                            self.fresh_infer_ty()
                        }
                    };
                    let thir_expr = thir::Expr {
                        kind: thir::ExprKind::Index {
                            base: Box::new(base_expr),
                            index: Box::new(idx_expr),
                        },
                        ty: elem_ty,
                        span,
                    };
                    (thir_expr, elem_ty)
                }
            }

            Expr::Cast {
                expr,
                ty: target_ref,
            } => {
                let (inner_expr, inner_ty) = self.check_expr(*expr);
                let target_ty = crate::tyconv::resolve_type_ref(
                    self.ctx,
                    self.infer,
                    self.def_map,
                    self.diagnostics,
                    target_ref,
                    &HashMap::new(),
                    span,
                );

                if target_ty != Ty::ERROR
                    && inner_ty != Ty::ERROR
                    && !self.is_cast_valid(inner_ty, target_ty)
                {
                    self.diagnostics
                        .push(GlyimDiagnostic::type_error(span, "invalid cast"));
                }

                let result_ty = if target_ty == Ty::ERROR {
                    Ty::ERROR
                } else {
                    target_ty
                };

                (
                    thir::Expr {
                        kind: thir::ExprKind::Cast {
                            expr: Box::new(inner_expr),
                        },
                        ty: result_ty,
                        span,
                    },
                    result_ty,
                )
            }

            Expr::Array(elements) => {
                let elem_ty = self.fresh_infer_ty();
                let mut elem_exprs = Vec::with_capacity(elements.len());
                for &elem_id in elements {
                    let (e_expr, e_ty) = self.check_expr(elem_id);
                    if e_ty != Ty::ERROR {
                        self.unify(e_ty, elem_ty, span);
                    }
                    elem_exprs.push(e_expr);
                }
                let elem_const_ty =
                    self.ctx.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::Isize));
                let arr_ty = self.ctx.mk_ty(TyKind::Array(
                    elem_ty,
                    Const {
                        kind: ConstKind::Int(elements.len() as i128),
                        ty: elem_const_ty,
                    },
                ));
                (
                    thir::Expr {
                        kind: thir::ExprKind::Array(elem_exprs),
                        ty: arr_ty,
                        span,
                    },
                    arr_ty,
                )
            }

            Expr::Tuple(elements) => {
                let mut elem_exprs = Vec::with_capacity(elements.len());
                let mut elem_tys = Vec::with_capacity(elements.len());
                for &elem_id in elements {
                    let (e_expr, e_ty) = self.check_expr(elem_id);
                    elem_exprs.push(e_expr);
                    elem_tys.push(GenericArg::Ty(e_ty));
                }
                let substs = self.ctx.intern_substitution(elem_tys);
                let tup_ty = self.ctx.mk_ty(TyKind::Tuple(substs));
                (
                    thir::Expr {
                        kind: thir::ExprKind::Tuple(elem_exprs),
                        ty: tup_ty,
                        span,
                    },
                    tup_ty,
                )
            }

            Expr::Struct {
                path,
                fields,
                spread,
            } => {
                // A two-segment `Enum::Variant` path is a *variant constructor*
                // (e.g. `twoFutureState::S0 { f0, fut0 }`), not a struct type.
                // Resolve it directly against the enum ADT's variant list by
                // name. This also covers HIR-generated enums (the async
                // state-machine `FooState` enum) that have no syntax nodes and
                // therefore never populate the def-map synthetic variant
                // module.
                if let Some((adt_id, variant_idx)) =
                    crate::tyconv::resolve_enum_variant_path(self.ctx, self.def_map, path)
                {
                    // Collect the variant's field infos into owned values FIRST
                    // so the immutable `self.ctx.adt_def` borrow is released
                    // before we mutably borrow `self.ctx` below.
                    let field_infos: Vec<(Name, Ty)> = match self.ctx.adt_def(adt_id) {
                        Some(def) => match def.variants.get(variant_idx as usize) {
                            Some(v) => v.fields.iter().map(|f| (f.name, f.ty)).collect(),
                            None => {
                                self.diagnostics.push(GlyimDiagnostic::type_error(
                                    span,
                                    "unknown variant in variant constructor",
                                ));
                                return (thir::Expr::err(span), Ty::ERROR);
                            }
                        },
                        None => {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                "unknown ADT in variant constructor",
                            ));
                            return (thir::Expr::err(span), Ty::ERROR);
                        }
                    };

                    let substs = self.ctx.intern_substitution(vec![]);
                    let struct_ty = self.ctx.mk_ty(TyKind::Adt(adt_id, substs));

                    let mut provided_fields = std::collections::HashSet::new();
                    let mut thir_fields = Vec::with_capacity(fields.len());

                    for &(field_name, field_expr_id) in fields {
                        provided_fields.insert(field_name);
                        let expected_field_ty =
                            if let Some((_, ty)) = field_infos.iter().find(|(n, _)| *n == field_name) {
                                self.substitute_type(*ty, substs, span)
                            } else {
                                self.diagnostics.push(GlyimDiagnostic::type_error(
                                    span,
                                    format!("no field `{}` on variant", self.ctx.name_str(field_name)),
                                ));
                                Ty::ERROR
                            };
                        let (field_expr, field_ty) = self.check_expr(field_expr_id);
                        if expected_field_ty != Ty::ERROR && field_ty != Ty::ERROR {
                            self.unify(field_ty, expected_field_ty, span);
                        }
                        thir_fields.push((field_name, field_expr));
                    }

                    if spread.is_none() {
                        for (name, _ty) in &field_infos {
                            if !provided_fields.contains(name) {
                                self.diagnostics.push(GlyimDiagnostic::type_error(
                                    span,
                                    format!(
                                        "missing field `{}` in variant constructor",
                                        self.ctx.name_str(*name)
                                    ),
                                ));
                            }
                        }
                    }

                    let thir_expr = thir::Expr {
                        kind: thir::ExprKind::Struct {
                            adt_id,
                            fields: thir_fields,
                            spread: None,
                            variant_idx,
                        },
                        ty: struct_ty,
                        span,
                    };
                    return (thir_expr, struct_ty);
                }

                // Otherwise resolve the struct type from the path (single
                // segment, or `Enum` without a variant).
                let struct_ty = crate::tyconv::resolve_path_type(
                    self.ctx,
                    self.infer,
                    self.def_map,
                    self.diagnostics,
                    path,
                    &HashMap::new(),
                    span,
                );

                if struct_ty == Ty::ERROR {
                    return (thir::Expr::err(span), Ty::ERROR);
                }

                let (adt_id, substs) = match self.ctx.ty_kind(struct_ty) {
                    TyKind::Adt(adt_id, substs) => (*adt_id, *substs),
                    _ => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            "struct literal requires ADT type",
                        ));
                        return (thir::Expr::err(span), Ty::ERROR);
                    }
                };

                let adt_def = match self.ctx.adt_def(adt_id) {
                    Some(def) => def,
                    None => {
                        self.diagnostics.push(GlyimDiagnostic::type_error(
                            span,
                            "unknown ADT in struct literal",
                        ));
                        return (thir::Expr::err(span), Ty::ERROR);
                    }
                };

                let field_infos: Vec<(Name, Ty)> =
                    adt_def.fields.iter().map(|f| (f.name, f.ty)).collect();

                let mut provided_fields = std::collections::HashSet::new();
                let mut thir_fields = Vec::with_capacity(fields.len());

                for &(field_name, field_expr_id) in fields {
                    provided_fields.insert(field_name);

                    let expected_field_ty =
                        if let Some((_, ty)) = field_infos.iter().find(|(n, _)| *n == field_name) {
                            self.substitute_type(*ty, substs, span)
                        } else {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                format!("no field `{}` on struct", self.ctx.name_str(field_name)),
                            ));
                            Ty::ERROR
                        };

                    let (field_expr, field_ty) = self.check_expr(field_expr_id);
                    if expected_field_ty != Ty::ERROR && field_ty != Ty::ERROR {
                        self.unify(field_ty, expected_field_ty, span);
                    }
                    thir_fields.push((field_name, field_expr));
                }

                let spread_expr = if let Some(spread_id) = spread {
                    let (spread_expr, spread_ty) = self.check_expr(*spread_id);
                    if spread_ty != Ty::ERROR {
                        self.unify(spread_ty, struct_ty, span);
                    }
                    Some(Box::new(spread_expr))
                } else {
                    // When there is no spread, all fields must be present.
                    for (name, _ty) in &field_infos {
                        if !provided_fields.contains(name) {
                            self.diagnostics.push(GlyimDiagnostic::type_error(
                                span,
                                format!(
                                    "missing field `{}` in struct literal",
                                    self.ctx.name_str(*name)
                                ),
                            ));
                        }
                    }
                    None
                };

                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::Struct {
                        adt_id,
                        fields: thir_fields,
                        spread: spread_expr,
                        variant_idx: 0,
                    },
                    ty: struct_ty,
                    span,
                };

                (thir_expr, struct_ty)
            }

            Expr::Closure { params, body, is_move } => {
                // 1. Enter the closure's own scope and bind the parameters so
                //    that the body's own bindings are distinguishable (by
                //    LocalVarId boundary) from captures of the enclosing env.
                self.env.enter_scope();
                let boundary = self.env.next_var_id();
                let mut thir_params: Vec<thir::Param> = Vec::with_capacity(params.len());
                for pat_id in params {
                    let ty = self.fresh_infer_ty();
                    let local = self.bind_pattern(*pat_id, ty, Mutability::Not);
                    // Recover the binding name from the HIR pattern for the
                    // THIR param (used as the MIR local debug name).
                    let name = match &self.body.pats[*pat_id] {
                        glyim_hir::Pat::Binding { name, .. } => *name,
                        _ => self.ctx.resolver().intern("_"),
                    };
                    thir_params.push(thir::Param {
                        name,
                        ty,
                        span,
                        pat: thir::Pattern::binding(local, name, Mutability::Not, ty, span),
                        local,
                    });
                }

                // 2. Check the body exactly once. The capture log records every
                //    VarRef resolved while checking it.
                //
                //    NOTE: `check_stmt::check` iterates every body expr as a
                //    statement, so the closure body may have already been
                //    type-checked (and cached) at the enclosing scope before
                //    this arm runs. Clear the per-expr cache so the body is
                //    re-checked *inside* the closure scope, ensuring its
                //    VarRefs are recorded in the capture log within the drain
                //    window below (and resolved against the closure's own
                //    bindings where appropriate).
                self.expr_cache.clear();
                let log_start = self.capture_log.len();
                let (body_expr, body_ty) = self.check_expr(*body);

                // 3. Classify captures: anything resolved below the boundary
                //    is a capture from an enclosing scope; classify mutability
                //    from the is_mut_use flag recorded in the log.
                //
                //    A `move` closure takes every capture *by value*: the
                //    captured variable is moved into the closure environment
                //    rather than referenced. (§2.2)
                let mut seen = std::collections::HashSet::new();
                let mut captures = Vec::new();
                for (id, ty, is_mut) in self.capture_log.drain(log_start..) {
                    if id.to_raw() >= boundary.to_raw() {
                        continue; // bound inside the closure itself — not a capture
                    }
                    if !seen.insert(id) {
                        continue; // already recorded (e.g. used twice) — keep first classification
                    }
                    let kind = if *is_move {
                        thir::CaptureKind::ByValue
                    } else if is_mut {
                        thir::CaptureKind::ByRef(Mutability::Mut)
                    } else {
                        thir::CaptureKind::ByRef(Mutability::Not)
                    };
                    captures.push((id, kind, ty));
                }
                self.env.leave_scope();

                // 4. Build a real closure type (see Tier 1.1b).
                let capture_tys: Vec<(Name, Ty)> = captures
                    .iter()
                    .enumerate()
                    .map(|(i, (_, _, ty))| (self.ctx.resolver().intern(&format!("capture_{i}")), *ty))
                    .collect();
                let closure_adt = self.ctx.register_closure(capture_tys.clone());
                // The closure *value* type is `TyKind::Closure(id, substs)`
                // (not the synthetic ADT): the lower stage and `lower_call`
                // recover the `ClosureId` from this type, while the ADT (kept in
                // `closure_adt_map`) supplies the capture field types. The
                // `substs` carry the capture types so the value lays out as a
                // struct of captures (mirroring the synthetic ADT's fields).
                let closure_id = ClosureId::from_raw(closure_adt.to_raw());
                let closure_substs = self.ctx.intern_substitution(
                    capture_tys.iter().map(|(_, t)| GenericArg::Ty(*t)).collect(),
                );
                let closure_ty = self.ctx.mk_ty(TyKind::Closure(closure_id, closure_substs));

                // Register the closure's full signature (captures followed by
                // its own parameters) so `Expr::Call` can resolve a call through
                // a closure-typed value and `lower_call` can emit the target.
                let closure_id = ClosureId::from_raw(closure_adt.to_raw());
                let mut sig_inputs: Vec<GenericArg> =
                    capture_tys.iter().map(|(_, t)| GenericArg::Ty(*t)).collect();
                sig_inputs.extend(thir_params.iter().map(|p| GenericArg::Ty(p.ty)));
                let closure_sig = FnSig {
                    inputs: self.ctx.intern_substitution(sig_inputs),
                    output: body_ty,
                    c_variadic: false,
                    unsafety: Safety::Safe,
                    abi: Abi::Glyim,
                };
                self.ctx.register_closure_sig(closure_id, closure_sig);

                // 5. Build THIR for the closure: capture list and body.
                let capture_thir: Vec<thir::Capture> = captures
                    .into_iter()
                    .map(|(local, kind, ty)| thir::Capture { local, kind, ty })
                    .collect();

                let closure_expr = thir::Expr {
                    kind: thir::ExprKind::Closure {
                        body: Box::new(thir::Body {
                            owner: self.owner,
                            params: thir_params, // closure's own parameters
                            return_ty: body_ty,
                            stmts: vec![thir::Stmt::Expr { expr: body_expr }],
                            span,
                        }),
                        captures: capture_thir,
                        is_move: *is_move,
                    },
                    ty: closure_ty,
                    span,
                };
                (closure_expr, closure_ty)
            }

            Expr::Let { pat, value } => {
                // A `let` appearing in expression position (e.g. as a block
                // tail) is rare; evaluate its value and bind the pattern,
                // yielding unit.
                let (_value_expr, value_ty) = self.check_expr(*value);
                self.check_pattern(*pat, value_ty);
                (thir::Expr::err(span), Ty::UNIT)
            }

            Expr::Assign { lhs, rhs } => {
                let (lhs_expr, lhs_ty) = self.check_expr(*lhs);
                // An assignment to a captured local is a mutating use.
                if let thir::ExprKind::VarRef(id) = lhs_expr.kind
                    && let Some(entry) =
                        self.capture_log.iter_mut().rev().find(|(vid, ..)| *vid == id)
                    {
                        entry.2 = true;
                    }
                let (_rhs_expr, rhs_ty) = self.check_expr(*rhs);
                if lhs_ty != Ty::ERROR && rhs_ty != Ty::ERROR {
                    self.unify(rhs_ty, lhs_ty, span);
                }
                (thir::Expr::err(span), Ty::UNIT)
            }

            Expr::Return { value } => {
                let value_opt = value.map(|val_id| {
                    let (val_expr, val_ty) = self.check_expr(val_id);
                    if val_ty != Ty::ERROR && self.return_ty != Ty::ERROR {
                        self.unify(val_ty, self.return_ty, span);
                    }
                    val_expr
                });
                (
                    thir::Expr {
                        kind: thir::ExprKind::Return {
                            value: value_opt.map(Box::new),
                        },
                        ty: Ty::NEVER,
                        span,
                    },
                    Ty::NEVER,
                )
            }

            Expr::Break { value } => {
                let value_expr = value.map(|val_id| Box::new(self.check_expr(val_id).0));
                (
                    thir::Expr {
                        kind: thir::ExprKind::Break { value: value_expr },
                        ty: Ty::NEVER,
                        span,
                    },
                    Ty::NEVER,
                )
            }

            Expr::Continue => (
                thir::Expr {
                    kind: thir::ExprKind::Continue,
                    ty: Ty::NEVER,
                    span,
                },
                Ty::NEVER,
            ),

            Expr::Range {
                start,
                end,
                inclusive,
            } => {
                let start_expr = start.map(|id| Box::new(self.check_expr(id).0));
                let end_expr = end.map(|id| Box::new(self.check_expr(id).0));
                // Resolve the range's element type `T` from the endpoint types
                // (prefer `start`, fall back to `end`), defaulting to an error
                // type for a full range `..` with no endpoints.
                let elem_ty = start_expr
                    .as_ref()
                    .map(|e| e.ty)
                    .or_else(|| end_expr.as_ref().map(|e| e.ty))
                    .unwrap_or_else(|| self.ctx.error_ty());
                let adt_id = glyim_core::def_id::AdtId::from_raw(if *inclusive { 1001 } else { 1000 });
                let substs = self
                    .ctx
                    .intern_substitution(vec![glyim_type::GenericArg::Ty(elem_ty)]);
                let range_ty = self.ctx.mk_adt(adt_id, substs);
                let thir_expr = thir::Expr {
                    kind: thir::ExprKind::Range {
                        start: start_expr,
                        end: end_expr,
                        inclusive: *inclusive,
                    },
                    ty: range_ty,
                    span,
                };
                (thir_expr, range_ty)
            }

            Expr::Missing => {
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    "encountered missing expression",
                ));
                (thir::Expr::err(span), Ty::ERROR)
            }

            Expr::Err => (thir::Expr::err(span), Ty::ERROR),
            // Defensive: `await` must be desugared to a poll loop by the async
            // desugaring pass (`lower_async`) before type-checking. If it
            // survives, the desugar pass did not run — treat as an error rather
            // than panicking on the uncovered pattern.
            Expr::Await { .. } => {
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    "`await` reached type-checking without desugaring".to_string(),
                ));
                (thir::Expr::err(span), Ty::ERROR)
            }
        };

        self.expr_cache.insert(expr_id, result.clone());
        result
    }

    // Helper: substitute generic args in a type (simplified)
    fn substitute_type(&self, ty: Ty, substs: glyim_type::Substitution, _span: Span) -> Ty {
        match self.ctx.ty_kind(ty) {
            TyKind::Param(pt) => {
                let args = self.ctx.substitution_args(substs);
                if (pt.index as usize) < args.len()
                    && let GenericArg::Ty(replacement) = args[pt.index as usize]
                {
                    return replacement;
                }
                ty
            }
            _ => ty,
        }
    }

    fn is_cast_valid(&self, from: Ty, to: Ty) -> bool {
        // Plan §13.2: delegate to the shared single-source-of-truth cast legality
        // check in `glyim-type` (also used by constant evaluation).
        glyim_type::is_valid_cast(self.ctx, from, to)
    }

    fn resolve_method_call(&mut self, recv_ty: Ty, method_name: Name, span: Span) -> (Ty, Option<MethodDispatch>) {
        // §9.1 / §9.2: collect *every* impl whose Self type unifies with the
        // receiver and that defines `method_name`. If more than one matches,
        // this is an ambiguous method call — surface all candidates (rustc's
        // E0034 style) instead of silently returning the first.
        //
        // Autoref/auto-deref (de-stubbing plan §9.1): try the receiver as-is,
        // then `&recv` / `&mut recv`, then successively deref'd receivers
        // (`recv`, `deref(recv)`, `deref(deref(recv))`, …). Structural derefs
        // (references / raw pointers) are resolved via `TyCtx::deref_ty`; ADT
        // `Deref` impls require the trait-DB population and fall back to `None`.
        // Once a step yields candidates we stop descending (standard autoref
        // priority), so `x.method()` still prefers `x`'s own impls.
        let mut steps: Vec<Ty> = Vec::new();
        let mut cur = Some(recv_ty);
        while let Some(t) = cur {
            steps.push(t);
            cur = self.ctx.deref_ty(t);
            if steps.len() >= 10 {
                break;
            }
        }
        // Autoref candidates: the receiver and its mutable/shared borrows.
        let autoref_steps = [
            recv_ty,
            self.ctx.mk_ref(Region::Erased, recv_ty, Mutability::Not),
            self.ctx.mk_ref(Region::Erased, recv_ty, Mutability::Mut),
        ];

        let recv_is_param = matches!(self.ctx.ty_kind(recv_ty), TyKind::Param(_));
        let collect_for = |this: &mut Self, step_ty: Ty| -> Vec<(Ty, Ty, Option<MethodDispatch>)> {
            let mut found: Vec<(Ty, Ty, Option<MethodDispatch>)> = Vec::new();
            for (_id, item) in this.hir.items.iter_enumerated() {
                if let glyim_hir::ItemKind::Impl(impl_item) = &item.kind {
                    let param_map =
                        crate::tyconv::build_param_tys(this.ctx, &impl_item.generic_params);
                    let impl_self_ty = crate::tyconv::resolve_type_ref(
                        this.ctx,
                        this.infer,
                        this.def_map,
                        this.diagnostics,
                        &impl_item.self_ty,
                        &param_map,
                        span,
                    );
                    // Plan unstub-5 P5 (generic-body monomorphization): when the
                    // receiver is a *type parameter* (`f: F`, `&F`, `&mut F`), a
                    // concrete `impl Trait for AddOne`'s `Self` can never unify
                    // with the rigid param `F`. The hard `unify(Param, Adt)`
                    // below would emit a spurious `F vs Adt` error. Skip
                    // concrete-impl matching for generic receivers and let the
                    // generic-receiver fallback (lower in this fn) resolve the
                    // method from the bound trait's HIR, so `f.poll()` on a
                    // generic `F: MyFuture` type-checks instead of erroring.
                    if recv_is_param {
                        continue;
                    }
                    // Probe whether this impl's `Self` type unifies with the
                    // receiver *without* committing side effects (a non-matching
                    // candidate must not emit a spurious "mismatched types"
                    // diagnostic — only the ultimately-selected method may).
                    // Snapshot the inference table and the diagnostics buffer;
                    // roll both back if the probe fails.
                    let inf_snap = this.infer.snapshot();
                    let diag_len = this.diagnostics.len();
                    let matches = this.unify(step_ty, impl_self_ty, span);
                    if !matches {
                        this.infer.rollback_to(inf_snap);
                        this.diagnostics.truncate(diag_len);
                        continue;
                    }
                    for method in &impl_item.methods {
                        if method.name == method_name {
                            let return_ty =
                                if let Some(return_ty_ref) = &method.return_ty {
                                    crate::tyconv::resolve_type_ref(
                                        this.ctx,
                                        this.infer,
                                        this.def_map,
                                        this.diagnostics,
                                        return_ty_ref,
                                        &param_map,
                                        span,
                                    )
                                } else {
                                    Ty::UNIT
                                };
                            let fn_def_id = method
                                .body
                                .and_then(|bid| this.body_owner_map.get(&bid).copied())
                                .map(|local| FnDefId::from_raw(local.to_raw()));
                            found.push((impl_self_ty, return_ty, fn_def_id.map(MethodDispatch::Static)));
                        }
                    }
                }
            }
            found
        };

        let mut candidates: Vec<(Ty, Ty, Option<MethodDispatch>)> = Vec::new();
        for &step in steps.iter().chain(autoref_steps.iter()) {
            let found = collect_for(self, step);
            if !found.is_empty() {
                candidates = found;
                break;
            }
        }

        if candidates.is_empty() {
            // Plan unstub-5 P5: method dispatch on a *generic* receiver
            // (`f.poll()` where `f: F` and `F: MyFuture`). No impl's `Self`
            // unifies with a type param, so the impl scan above finds nothing.
            // Instead, resolve the method from the bound trait's HIR
            // definition, substituting `Self` → the receiver type so associated
            // types in the signature (`Self::Output`) project correctly.
            if let TyKind::Param(param) = self.ctx.ty_kind(recv_ty) {
                let traits = self.ctx.param_bounds_for(param.name).map(|t| t.to_vec());
                if let Some(traits) = traits {
                    let self_name = self.ctx.resolver().intern("Self");
                    let mut pm: HashMap<Name, Ty> = HashMap::new();
                    pm.insert(self_name, recv_ty);
                    for tid in traits {
                        for item in self.hir.items.iter() {
                            if let glyim_hir::ItemKind::Trait(trait_item) = &item.kind {
                                let trait_path = glyim_hir::Path {
                                    segments: vec![glyim_hir::PathSegment {
                                        name: item.name,
                                        generic_args: None,
                                    }],
                                    kind: glyim_core::path::PathKind::Plain,
                                };
                                let Some(local) =
                                    crate::tyconv::resolve_path_to_local_def_id(self.ctx, self.def_map, &trait_path)
                                else {
                                    continue;
                                };
                                if TraitDefId::from_raw(local.to_raw()) != tid {
                                    continue;
                                }
                                for m in &trait_item.methods {
                                    if m.name == method_name {
                                        let return_ty = if let Some(rt) = &m.return_ty {
                                            crate::tyconv::resolve_type_ref(
                                                self.ctx,
                                                self.infer,
                                                self.def_map,
                                                self.diagnostics,
                                                rt,
                                                &pm,
                                                span,
                                            )
                                        } else {
                                            Ty::UNIT
                                        };
                                        candidates.push((recv_ty, return_ty, Some(MethodDispatch::Virtual(tid))));
                                        break;
                                    }
                                }
                            }
                        }
                        if !candidates.is_empty() {
                            break;
                        }
                    }
                }
            }

            if candidates.is_empty() {
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    format!(
                        "no method `{}` found for type",
                        self.ctx.name_str(method_name)
                    ),
                ));
                // Plan §22.1 (prereq for "Generate impl"): if the method name is
                // declared by a trait in this crate, surface a `trait_not_implemented`
                // diagnostic naming that trait and the receiver type, so the LSP can
                // offer to synthesize `impl Trait for Type { }`.
                let recv_name = PrintTy::new(recv_ty, &*self.ctx).to_string();
                for item in self.hir.items.iter() {
                    if let glyim_hir::ItemKind::Trait(trait_item) = &item.kind {
                        if trait_item
                            .methods
                            .iter()
                            .any(|m| m.name == method_name)
                        {
                            self.diagnostics.push(GlyimDiagnostic::trait_not_implemented(
                                span,
                                self.ctx.name_str(item.name),
                                recv_name.clone(),
                            ));
                            break;
                        }
                    }
                }
                return (Ty::ERROR, None);
            }
        }

        if candidates.len() > 1 {
            let list: Vec<String> = candidates
                .iter()
                .map(|(self_ty, _, _)| format!("  {}", PrintTy::new(*self_ty, &*self.ctx)))
                .collect();
            self.diagnostics.push(GlyimDiagnostic::type_error(
                span,
                format!(
                    "ambiguous method `{}` found in multiple impls for type `{}`:\n{}",
                    self.ctx.name_str(method_name),
                    PrintTy::new(recv_ty, &*self.ctx),
                    list.join("\n")
                ),
            ));
            // Still return the first candidate's type so downstream typing is
            // not worse than before; the diagnostic is the real signal.
            return (candidates[0].1, None);
        }

        (candidates[0].1, candidates[0].2.clone())
    }

    /// Resolve a path-qualified trait method call `Trait::method(receiver, ..)`
    /// to the concrete impl function for the given *receiver* type (static
    /// dispatch). Scans `impl Trait for Type` items, selects the one whose
    /// trait matches `trait_def_id` and whose `Self` type unifies with the
    /// receiver (with autoref/auto-deref, mirroring `resolve_method_call`),
    /// and returns that impl method's `FnDefId`.
    fn resolve_trait_method_fn(
        &mut self,
        recv_ty: Ty,
        trait_def_id: TraitDefId,
        method_name: Name,
        span: Span,
    ) -> Option<FnDefId> {
        // Receiver candidate types: the receiver as-is, then autoref
        // (`&`/`&mut`), then successive structural derefs — same priority as
        // `resolve_method_call`.
        let mut steps: Vec<Ty> = Vec::new();
        let mut cur = Some(recv_ty);
        while let Some(t) = cur {
            steps.push(t);
            cur = self.ctx.deref_ty(t);
            if steps.len() >= 10 {
                break;
            }
        }
        let autoref_steps = [
            recv_ty,
            self.ctx.mk_ref(Region::Erased, recv_ty, Mutability::Not),
            self.ctx.mk_ref(Region::Erased, recv_ty, Mutability::Mut),
        ];

        for _step in steps.iter().chain(autoref_steps.iter()) {
            for (_id, item) in self.hir.items.iter_enumerated() {
                if let glyim_hir::ItemKind::Impl(impl_item) = &item.kind {
                    // The impl must be for the target trait.
                    let Some(trait_path) = &impl_item.trait_ref else {
                        continue;
                    };
                    let Some(impl_trait_id) = crate::tyconv::resolve_path_to_trait_def_id(
                        self.def_map,
                        self.ctx,
                        trait_path,
                        span,
                    ) else {
                        continue;
                    };
                    if impl_trait_id != trait_def_id {
                        continue;
                    }
                    let param_map = crate::tyconv::build_param_tys(
                        self.ctx,
                        &impl_item.generic_params,
                    );
                    let impl_self_ty = crate::tyconv::resolve_type_ref(
                        self.ctx,
                        self.infer,
                        self.def_map,
                        self.diagnostics,
                        &impl_item.self_ty,
                        &param_map,
                        span,
                    );
                    // Probe receiver compatibility *without* emitting
                    // diagnostics: `InferCtx::unify` returns `Result` and only
                    // `FnCtxt::unify` pushes to the diagnostics vec. The
                    // receiver may match `Self`, `&Self` (for `&self`
                    // methods), or `&mut Self`.
                    let ref_self = self.ctx.mk_ref(Region::Erased, impl_self_ty, Mutability::Not);
                    let ref_mut_self =
                        self.ctx.mk_ref(Region::Erased, impl_self_ty, Mutability::Mut);
                    let infer = &mut *self.infer;
                    let recv_steps = steps
                        .iter()
                        .chain(autoref_steps.iter())
                        .copied()
                        .collect::<Vec<_>>();
                    let self_matches = recv_steps.iter().any(|&rt| {
                        infer.unify(self.ctx, rt, impl_self_ty, span).is_ok()
                            || infer.unify(self.ctx, rt, ref_self, span).is_ok()
                            || infer.unify(self.ctx, rt, ref_mut_self, span).is_ok()
                    });
                    if !self_matches {
                        continue;
                    }
                    for method in &impl_item.methods {
                        if method.name == method_name {
                            if let Some(body_id) = method.body {
                                let local = self
                                    .body_owner_map
                                    .get(&body_id)
                                    .copied()
                                    .unwrap_or_else(|| self.hir.body_owners[body_id]);
                                return Some(FnDefId::from_raw(local.to_raw()));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn extract_return_ty(&mut self, fn_ty: Ty, _span: Span) -> Ty {
        match self.ctx.ty_kind(fn_ty) {
            TyKind::FnDef(_, _) | TyKind::FnPtr(_) => self.fresh_infer_ty(),
            _ => fn_ty,
        }
    }

    fn lookup_field_ty_with_substs(
        &mut self,
        adt_id: AdtId,
        field_name: Name,
        span: Span,
        substs: glyim_type::Substitution,
    ) -> Ty {
        let adt_def = match self.ctx.adt_def(adt_id) {
            Some(def) => def,
            None => {
                self.diagnostics.push(GlyimDiagnostic::type_error(
                    span,
                    "unknown ADT in field lookup",
                ));
                return Ty::ERROR;
            }
        };
        if let Some(field_def) = adt_def.fields.iter().find(|f| f.name == field_name) {
            self.substitute_type(field_def.ty, substs, span)
        } else {
            self.diagnostics.push(GlyimDiagnostic::type_error(
                span,
                format!("no field `{}` on type", self.ctx.name_str(field_name)),
            ));
            Ty::ERROR
        }
    }
}

