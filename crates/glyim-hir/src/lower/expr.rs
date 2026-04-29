use crate::types::{HirType, ExprId};
use crate::{HirExpr, HirPattern, HirStmt};
use crate::lower::context::LoweringContext;
use crate::lower::ops::{lower_binop, lower_unop};
use crate::lower::pattern::lower_pattern;
use crate::lower::types::resolve_type_name;
use glyim_parse::{BlockItem, ExprKind, Pattern, StmtKind};

pub fn lower_expr(expr: &ExprKind, ctx: &mut LoweringContext) -> HirExpr {
    let id = ctx.fresh_id();
    match expr {
        ExprKind::IntLit(n) => HirExpr::IntLit { id, value: *n },
        ExprKind::FloatLit(f) => HirExpr::FloatLit { id, value: *f },
        ExprKind::BoolLit(b) => HirExpr::BoolLit { id, value: *b },
        ExprKind::StrLit(s) => HirExpr::StrLit { id, value: s.clone() },
        ExprKind::Ident(sym) => HirExpr::Ident { id, name: *sym },
        ExprKind::UnitLit => HirExpr::UnitLit { id },

        ExprKind::Binary { op, lhs, rhs } => HirExpr::Binary {
            id,
            op: lower_binop(op.clone()),
            lhs: Box::new(lower_expr(&lhs.kind, ctx)),
            rhs: Box::new(lower_expr(&rhs.kind, ctx)),
        },
        ExprKind::Unary { op, operand } => HirExpr::Unary {
            id,
            op: lower_unop(op.clone()),
            operand: Box::new(lower_expr(&operand.kind, ctx)),
        },

        ExprKind::Lambda { params: _, body } => lower_expr(&body.kind, ctx),

        ExprKind::Block(items) => lower_block(items, ctx),

        ExprKind::If { condition, then_branch, else_branch } => HirExpr::If {
            id,
            condition: Box::new(lower_expr(&condition.kind, ctx)),
            then_branch: Box::new(lower_expr(&then_branch.kind, ctx)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(lower_expr(&e.kind, ctx))),
        },

        ExprKind::StructLit { name, fields } => HirExpr::StructLit {
            id,
            struct_name: *name,
            fields: fields
                .iter()
                .map(|(sym, e)| (*sym, lower_expr(&e.kind, ctx)))
                .collect(),
        },

        ExprKind::Match { scrutinee, arms } => lower_match(id, scrutinee, arms, ctx),

        ExprKind::EnumVariant { enum_name, variant_name, args } => HirExpr::EnumVariant {
            id,
            enum_name: *enum_name,
            variant_name: *variant_name,
            args: args.iter().map(|a| lower_expr(&a.kind, ctx)).collect(),
        },

        ExprKind::FieldAccess { object, field } => HirExpr::FieldAccess {
            id,
            object: Box::new(lower_expr(&object.kind, ctx)),
            field: *field,
        },

        // Option/Result sugar
        ExprKind::SomeExpr(e) => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Option"),
            variant_name: ctx.intern("Some"),
            args: vec![lower_expr(&e.kind, ctx)],
        },
        ExprKind::NoneExpr => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Option"),
            variant_name: ctx.intern("None"),
            args: vec![],
        },
        ExprKind::OkExpr(e) => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Result"),
            variant_name: ctx.intern("Ok"),
            args: vec![lower_expr(&e.kind, ctx)],
        },
        ExprKind::ErrExpr(e) => HirExpr::EnumVariant {
            id,
            enum_name: ctx.intern("Result"),
            variant_name: ctx.intern("Err"),
            args: vec![lower_expr(&e.kind, ctx)],
        },

        ExprKind::Pointer { mutable: _, target } => HirExpr::As {
            id,
            expr: Box::new(HirExpr::IntLit { id: ctx.fresh_id(), value: 0 }),
            target_type: HirType::RawPtr(Box::new(HirType::Named(*target))),
        },

        ExprKind::As { expr, target_type } => HirExpr::As {
            id,
            expr: Box::new(lower_expr(&expr.kind, ctx)),
            target_type: resolve_type_name(ctx.resolve(*target_type), *target_type),
        },

        ExprKind::MacroCall { name, arg } => {
            if ctx.resolve(*name) == "identity" {
                lower_expr(&arg.kind, ctx)
            } else {
                HirExpr::IntLit { id, value: 0 }
            }
        },

        ExprKind::TryExpr(e) => lower_try_expr(id, e, ctx),

        ExprKind::Call { callee, args } => lower_call(id, callee, args, ctx),

        ExprKind::TupleLit(elems) => HirExpr::TupleLit {
            id,
            elements: elems.iter().map(|e| lower_expr(&e.kind, ctx)).collect(),
        },
    }
}

fn lower_block(items: &[BlockItem], ctx: &mut LoweringContext) -> HirExpr {
    let id = ctx.fresh_id();
    let stmts: Vec<HirStmt> = items
        .iter()
        .map(|item| match item {
            BlockItem::Expr(e) => HirStmt::Expr(lower_expr(&e.kind, ctx)),
            BlockItem::Stmt(s) => lower_stmt(s, ctx),
        })
        .collect();
    HirExpr::Block { id, stmts }
}

fn lower_stmt(stmt: &glyim_parse::StmtNode, ctx: &mut LoweringContext) -> HirStmt {
    match &stmt.kind {
        StmtKind::Let { pattern, mutable, value } => {
            let val = lower_expr(&value.kind, ctx);
            match pattern {
                Pattern::Var(name) => HirStmt::Let {
                    name: *name,
                    mutable: *mutable,
                    value: val,
                },
                _ => HirStmt::Let {
                    name: ctx.intern("_"),
                    mutable: false,
                    value: val,
                },
            }
        }
        StmtKind::Assign { target, value } => HirStmt::Assign {
            target: *target,
            value: lower_expr(&value.kind, ctx),
        },
    }
}

fn lower_match(
    id: ExprId,
    scrutinee: &glyim_parse::ExprNode,
    arms: &[glyim_parse::MatchArm],
    ctx: &mut LoweringContext,
) -> HirExpr {
    let hir_arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)> = arms
        .iter()
        .map(|arm| {
            let pattern = lower_pattern(&arm.pattern, ctx);
            let guard = arm.guard.as_ref().map(|e| lower_expr(&e.kind, ctx));
            let body = lower_expr(&arm.body.kind, ctx);
            (pattern, guard, body)
        })
        .collect();
    HirExpr::Match {
        id,
        scrutinee: Box::new(lower_expr(&scrutinee.kind, ctx)),
        arms: hir_arms,
    }
}

fn lower_try_expr(id: ExprId, expr: &glyim_parse::ExprNode, ctx: &mut LoweringContext) -> HirExpr {
    HirExpr::Match {
        id,
        scrutinee: Box::new(lower_expr(&expr.kind, ctx)),
        arms: vec![
            (
                HirPattern::ResultOk(Box::new(HirPattern::Var(ctx.intern("v")))),
                None,
                HirExpr::Ident { id: ctx.fresh_id(), name: ctx.intern("v") },
            ),
            (
                HirPattern::ResultErr(Box::new(HirPattern::Var(ctx.intern("e")))),
                None,
                HirExpr::IntLit { id: ctx.fresh_id(), value: 0 },
            ),
        ],
    }
}

fn lower_call(
    _id: ExprId,
    callee: &glyim_parse::ExprNode,
    args: &[glyim_parse::ExprNode],
    ctx: &mut LoweringContext,
) -> HirExpr {
    // Handle namespaced calls: StructName::method(args)
    if let ExprKind::EnumVariant { enum_name, variant_name, args: enum_args } = &callee.kind {
        if enum_args.is_empty() {
            let mangled = ctx.intern(&format!(
                "{}_{}",
                ctx.resolve(*enum_name),
                ctx.resolve(*variant_name)
            ));
            let call_args: Vec<HirExpr> = args.iter().map(|a| lower_expr(&a.kind, ctx)).collect();
            return HirExpr::Call { id: ctx.fresh_id(), callee: mangled, args: call_args };
        }
    }

    // Handle built-in functions
    if let ExprKind::Ident(sym) = &callee.kind {
        match ctx.resolve(*sym) {
            "println" => return lower_println_call(args, ctx),
            "assert" => return lower_assert_call(args, ctx),
            _ => {}
        }
    }

    // Default fallback
    HirExpr::IntLit { id: ctx.fresh_id(), value: 0 }
}

fn lower_println_call(args: &[glyim_parse::ExprNode], ctx: &mut LoweringContext) -> HirExpr {
    let arg = args
        .first()
        .map(|a| lower_expr(&a.kind, ctx))
        .unwrap_or(HirExpr::IntLit { id: ctx.fresh_id(), value: 0 });
    HirExpr::Println { id: ctx.fresh_id(), arg: Box::new(arg) }
}

fn lower_assert_call(args: &[glyim_parse::ExprNode], ctx: &mut LoweringContext) -> HirExpr {
    let cond = args
        .first()
        .map(|a| lower_expr(&a.kind, ctx))
        .unwrap_or(HirExpr::IntLit { id: ctx.fresh_id(), value: 0 });
    let msg = args.get(1).map(|a| Box::new(lower_expr(&a.kind, ctx)));
    HirExpr::Assert { id: ctx.fresh_id(), condition: Box::new(cond), message: msg }
}
