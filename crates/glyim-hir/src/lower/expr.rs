use crate::node::MatchArm;
use crate::lower::context::LoweringContext;
use crate::lower::ops::{lower_binop, lower_unop};
use crate::lower::pattern::lower_pattern;
use crate::lower::types::lower_type_expr;
use crate::types::{ExprId, HirType};
use crate::{HirExpr, HirPattern, HirStmt};
use glyim_parse::{BlockItem, ExprKind, StmtKind};

pub fn lower_expr(expr: &glyim_parse::ExprNode, ctx: &mut LoweringContext) -> HirExpr {
    let id = ctx.fresh_id();
    let span = expr.span;
    match &expr.kind {
        ExprKind::IntLit(n) => HirExpr::IntLit {
            id,
            value: *n,
            span,
        },
        ExprKind::FloatLit(f) => HirExpr::FloatLit {
            id,
            value: *f,
            span,
        },
        ExprKind::BoolLit(b) => HirExpr::BoolLit {
            id,
            value: *b,
            span,
        },
        ExprKind::StrLit(s) => HirExpr::StrLit {
            id,
            value: s.clone(),
            span,
        },
        ExprKind::Ident(sym) => HirExpr::Ident {
            id,
            name: *sym,
            span,
        },
        ExprKind::UnitLit => HirExpr::UnitLit { id, span },

        ExprKind::Binary { op, lhs, rhs } => HirExpr::Binary {
            id,
            op: lower_binop(op.clone()),
            lhs: Box::new(lower_expr(lhs, ctx)),
            rhs: Box::new(lower_expr(rhs, ctx)),
            span,
        },
        ExprKind::Unary { op, operand } => HirExpr::Unary {
            id,
            op: lower_unop(op.clone()),
            operand: Box::new(lower_expr(operand, ctx)),
            span,
        },

        ExprKind::Lambda { params: _, body } => lower_expr(body, ctx),

        ExprKind::Block(items) => lower_block(items, span, ctx),

        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => HirExpr::If {
            id,
            condition: Box::new(lower_expr(condition, ctx)),
            then_branch: Box::new(lower_expr(then_branch, ctx)),
            else_branch: else_branch.as_ref().map(|e| Box::new(lower_expr(e, ctx))),
            span,
        },

        ExprKind::StructLit { name, fields } => HirExpr::StructLit {
            id,
            struct_name: *name,
            fields: fields
                .iter()
                .map(|(sym, e)| (*sym, lower_expr(e, ctx)))
                .collect(),
            span,
        },

        ExprKind::Match { scrutinee, arms } => lower_match(id, scrutinee, arms, span, ctx),

        ExprKind::EnumVariant {
            enum_name,
            variant_name,
            args,
        } => {
            if ctx.struct_names.contains(enum_name) {
                // This is a struct-associated function call, e.g., Point::zero().
                let mangled = ctx.intern(&format!(
                    "{}_{}",
                    ctx.resolve(*enum_name),
                    ctx.resolve(*variant_name)
                ));
                let call_args: Vec<HirExpr> = args.iter().map(|a| lower_expr(a, ctx)).collect();
                HirExpr::Call {
                    id,
                    callee: mangled,
                    args: call_args,
                    span,
                }
            } else {
                HirExpr::EnumVariant {
                    id,
                    enum_name: *enum_name,
                    variant_name: *variant_name,
                    args: args.iter().map(|a| lower_expr(a, ctx)).collect(),
                    span,
                }
            }
        }

        ExprKind::FieldAccess { object, field } => HirExpr::FieldAccess {
            id,
            object: Box::new(lower_expr(object, ctx)),
            field: *field,
            span,
        },

        ExprKind::SomeExpr(e) => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Option"),
            variant_name: ctx.intern("Some"),
            args: vec![lower_expr(e, ctx)],
            span,
        },
        ExprKind::NoneExpr => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Option"),
            variant_name: ctx.intern("None"),
            args: vec![],
            span,
        },
        ExprKind::OkExpr(e) => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Result"),
            variant_name: ctx.intern("Ok"),
            args: vec![lower_expr(e, ctx)],
            span,
        },
        ExprKind::ErrExpr(e) => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Result"),
            variant_name: ctx.intern("Err"),
            args: vec![lower_expr(e, ctx)],
            span,
        },

        ExprKind::Pointer { mutable: _, target } => HirExpr::As {
            id,
            expr: Box::new(HirExpr::IntLit {
                id: ctx.fresh_id(),
                value: 0,
                span,
            }),
            target_type: HirType::RawPtr(Box::new(HirType::Named(*target))),
            span,
        },

        ExprKind::As { expr, target_type } => {
            let target_hir = crate::lower::types::lower_type_expr(target_type, ctx);
            HirExpr::As {
                id,
                expr: Box::new(lower_expr(expr, ctx)),
                target_type: target_hir,
                span,
            }
        }

        ExprKind::MacroCall { name, arg } => {
            if ctx.resolve(*name) == "identity" {
                lower_expr(arg, ctx)
            } else {
                HirExpr::IntLit { id, value: 0, span }
            }
        }

        ExprKind::TryExpr(e) => lower_try_expr(id, e, ctx),

        ExprKind::Call { callee, args } => lower_call(callee, args, ctx),

        ExprKind::MethodCall {
            receiver,
            method,
            args,
        } => {
            // AST args include the receiver as first element; strip it
            let method_args: Vec<_> = args.iter().skip(1).map(|a| lower_expr(a, ctx)).collect();
            tracing::debug!(
                "LOWER MethodCall: receiver={:?} method={:?}",
                receiver.kind,
                ctx.resolve(*method)
            );
            HirExpr::MethodCall {
                id,
                receiver: Box::new(lower_expr(receiver, ctx)),
                method_name: *method,
                resolved_callee: None,
                args: method_args,
                span,
            }
        }

        ExprKind::TupleLit(elems) => HirExpr::TupleLit {
            id,
            elements: elems.iter().map(|e| lower_expr(e, ctx)).collect(),
            span,
        },

        ExprKind::Deref(e) => HirExpr::Deref {
            id: ctx.fresh_id(),
            expr: Box::new(lower_expr(e, ctx)),
            span,
        },

        ExprKind::ForIn {
            pattern,
            iter,
            body,
        } => {
            // Desugar: for x in iter { body }
            //   → let mut __iter = iter;
            //     let mut __done = false;
            //     while !__done {
            //         match __iter.next() {
            //             Some(x) => { body },
            //             None => { __done = true; },
            //         }
            //     }
            let iter_expr = lower_expr(iter, ctx);
            let iter_sym = ctx.intern("__iter");
            let done_sym = ctx.intern("__done");
            let next_sym = ctx.intern("next");
            let __some_sym = ctx.intern("Some");
            let __none_sym = ctx.intern("None");

            let let_done = HirStmt::LetPat {
                pattern: HirPattern::Var(done_sym),
                mutable: true,
                value: HirExpr::BoolLit {
                    id: ctx.fresh_id(),
                    value: false,
                    span,
                },
                ty: None,
                span,
            };
            let let_iter = HirStmt::LetPat {
                pattern: HirPattern::Var(iter_sym),
                mutable: true,
                value: iter_expr,
                span,
                ty: None,
            };

            let body_expr = lower_expr(body, ctx);
            let match_expr = HirExpr::Match {
                id: ctx.fresh_id(),
                scrutinee: Box::new(HirExpr::MethodCall {
                    id: ctx.fresh_id(),
                    receiver: Box::new(HirExpr::Ident {
                        id: ctx.fresh_id(),
                        name: iter_sym,
                        span,
                    }),
                    method_name: next_sym,
                    resolved_callee: None,
                    args: vec![],
                    span,
                }),
                arms: vec![
                    MatchArm {
                        pattern: HirPattern::OptionSome(Box::new(lower_pattern(pattern, ctx))),
                        guard: None,
                        body: body_expr,
                    },
                    MatchArm {
                        pattern: HirPattern::OptionNone,
                        guard: None,
                        body: HirExpr::Block {
                            id: ctx.fresh_id(),
                            stmts: vec![HirStmt::Assign {
                                target: done_sym,
                                value: HirExpr::BoolLit {
                                    id: ctx.fresh_id(),
                                    value: true,
                                    span,
                                },
                                span,
                            }],
                            span,
                        },
                    },
                ],
                span,
            };

            let while_expr = HirExpr::While {
                id,
                condition: Box::new(HirExpr::Binary {
                    id: ctx.fresh_id(),
                    op: crate::node::HirBinOp::Eq,
                    lhs: Box::new(HirExpr::Ident {
                        id: ctx.fresh_id(),
                        name: done_sym,
                        span,
                    }),
                    rhs: Box::new(HirExpr::IntLit {
                        id: ctx.fresh_id(),
                        value: 0,
                        span,
                    }),
                    span,
                }),
                body: Box::new(HirExpr::Block {
                    id: ctx.fresh_id(),
                    stmts: vec![HirStmt::Expr(match_expr)],
                    span,
                }),
                span,
            };

            HirExpr::Block {
                id: ctx.fresh_id(),
                stmts: vec![let_iter, let_done, HirStmt::Expr(while_expr)],
                span,
            }
        }

        ExprKind::While { condition, body } => HirExpr::While {
            id,
            condition: Box::new(lower_expr(condition, ctx)),
            body: Box::new(lower_expr(body, ctx)),
            span,
        },

        ExprKind::SizeOf(ty) => HirExpr::SizeOf {
            id,
            target_type: lower_type_expr(ty, ctx),
            span,
        },
    }
}

fn lower_block(
    items: &[BlockItem],
    block_span: glyim_diag::Span,
    ctx: &mut LoweringContext,
) -> HirExpr {
    let id = ctx.fresh_id();
    let stmts: Vec<HirStmt> = items
        .iter()
        .map(|item| match item {
            BlockItem::Expr(e) => HirStmt::Expr(lower_expr(e, ctx)),
            BlockItem::Stmt(s) => lower_stmt(s, ctx),
        })
        .collect();
    HirExpr::Block {
        id,
        stmts,
        span: block_span,
    }
}

fn lower_stmt(stmt: &glyim_parse::StmtNode, ctx: &mut LoweringContext) -> HirStmt {
    let span = stmt.span;
    match &stmt.kind {
        StmtKind::Let {
            pattern,
            mutable,
            value,
            ty,
        } => {
            let val = lower_expr(value, ctx);
            tracing::debug!("LOWER LetPat: value={:?}", val);
            let pat = lower_pattern(pattern, ctx);
            HirStmt::LetPat {
                pattern: pat,
                mutable: *mutable,
                value: val,
                span,
                ty: ty
                    .as_ref()
                    .map(|t| crate::lower::types::lower_type_expr(t, ctx)),
            }
        }
        StmtKind::Assign { target, value } => HirStmt::Assign {
            target: *target,
            value: lower_expr(value, ctx),
            span,
        },
        StmtKind::AssignField {
            object,
            field,
            value,
        } => HirStmt::AssignField {
            object: Box::new(lower_expr(object, ctx)),
            field: *field,
            value: lower_expr(value, ctx),
            span,
        },
        StmtKind::AssignDeref { target, value } => HirStmt::AssignDeref {
            target: Box::new(lower_expr(target, ctx)),
            value: lower_expr(value, ctx),
            span,
        },
    }
}

fn lower_match(
    id: ExprId,
    scrutinee: &glyim_parse::ExprNode,
    arms: &[glyim_parse::MatchArm],
    match_span: glyim_diag::Span,
    ctx: &mut LoweringContext,
) -> HirExpr {
    let hir_arms: Vec<MatchArm> = arms
        .iter()
        .map(|arm| {
            let pattern = lower_pattern(&arm.pattern, ctx);
            let guard = arm.guard.as_ref().map(|e| lower_expr(e, ctx));
            let body = lower_expr(&arm.body, ctx);
            MatchArm { pattern, guard, body }
        })
        .collect();
    HirExpr::Match {
        id,
        scrutinee: Box::new(lower_expr(scrutinee, ctx)),
        arms: hir_arms,
        span: match_span,
    }
}

fn lower_try_expr(id: ExprId, expr: &glyim_parse::ExprNode, ctx: &mut LoweringContext) -> HirExpr {
    let span = expr.span;
    let abort_sym = ctx.intern("abort");
    let fail_block = HirExpr::Block {
        id: ctx.fresh_id(),
        stmts: vec![
            HirStmt::Expr(HirExpr::Println {
                id: ctx.fresh_id(),
                arg: Box::new(HirExpr::StrLit {
                    id: ctx.fresh_id(),
                    value: "FAILED\n".to_string(),
                    span,
                }),
                span,
            }),
            HirStmt::Expr(HirExpr::Call {
                id: ctx.fresh_id(),
                callee: abort_sym,
                args: vec![],
                span,
            }),
        ],
        span,
    };
    HirExpr::Match {
        id,
        scrutinee: Box::new(lower_expr(expr, ctx)),
        arms: vec![
            MatchArm {
                pattern: HirPattern::ResultOk(Box::new(HirPattern::Var(ctx.intern("v")))),
                guard: None,
                body: HirExpr::Ident {
                    id: ctx.fresh_id(),
                    name: ctx.intern("v"),
                    span,
                },
            },
            MatchArm {
                pattern: HirPattern::ResultErr(Box::new(HirPattern::Wild)),
                guard: None,
                body: fail_block,
            },
        ],
        span,
    }
}

fn lower_call(
    callee: &glyim_parse::ExprNode,
    args: &[glyim_parse::ExprNode],
    ctx: &mut LoweringContext,
) -> HirExpr {
    let call_span = {
        let start = callee.span.start;
        let end = args.last().map_or(callee.span.end, |a| a.span.end);
        glyim_diag::Span::new(start, end)
    };

    if let ExprKind::EnumVariant {
        enum_name,
        variant_name,
        args: enum_args,
    } = &callee.kind
        && enum_args.is_empty()
    {
        let mangled = ctx.intern(&format!(
            "{}_{}",
            ctx.resolve(*enum_name),
            ctx.resolve(*variant_name)
        ));
        let call_args: Vec<HirExpr> = args.iter().map(|a| lower_expr(a, ctx)).collect();
        return HirExpr::Call {
            id: ctx.fresh_id(),
            callee: mangled,
            args: call_args,
            span: call_span,
        };
    }

    let call_args: Vec<HirExpr> = args.iter().map(|a| lower_expr(a, ctx)).collect();
    if let ExprKind::Ident(sym) = &callee.kind {
        match ctx.resolve(*sym) {
            "println" => {
                return HirExpr::Println {
                    id: ctx.fresh_id(),
                    arg: Box::new(call_args.into_iter().next().unwrap_or(HirExpr::IntLit {
                        id: ctx.fresh_id(),
                        value: 0,
                        span: glyim_diag::Span::new(0, 0),
                    })),
                    span: call_span,
                };
            }
            "assert" => {
                let cond = if let Some(first) = args.first() {
                    lower_expr(first, ctx)
                } else {
                    HirExpr::IntLit {
                        id: ctx.fresh_id(),
                        value: 0,
                        span: glyim_diag::Span::new(0, 0),
                    }
                };
                let msg = args.get(1).map(|a| Box::new(lower_expr(a, ctx)));
                return HirExpr::Assert {
                    id: ctx.fresh_id(),
                    condition: Box::new(cond),
                    message: msg,
                    span: call_span,
                };
            }
            _ => {
                return HirExpr::Call {
                    id: ctx.fresh_id(),
                    callee: *sym,
                    args: call_args,
                    span: call_span,
                };
            }
        }
    }

    HirExpr::IntLit {
        id: ctx.fresh_id(),
        value: 0,
        span: call_span,
    }
}
