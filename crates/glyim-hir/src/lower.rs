use crate::{Hir, HirBinOp, HirExpr, HirFn, HirPattern, HirStmt, HirUnOp};
use crate::item::{ExternBlock, ExternFn, HirItem, StructDef, EnumDef, HirVariant, StructField};
use crate::types::HirType;
use glyim_interner::{Interner, Symbol};
use glyim_parse::{BinOp, BlockItem, ExprKind, Item, StmtKind, UnOp};

pub fn lower(ast: &glyim_parse::Ast, interner: &mut Interner) -> Hir {
    let mut fns = vec![];
    for item in &ast.items {
        match item {
            Item::Binding { name, value, .. } => {
                if let ExprKind::Lambda { params, body } = &value.kind {
                    fns.push(HirItem::Fn(HirFn {
                        name: *name,
                        params: params.clone(),
                        body: lower_expr(&body.kind, interner),
                    }));
                }
            }
            Item::FnDef {
                name, params, body, ..
            } => {
                let param_syms: Vec<_> = params.iter().map(|(sym, _)| *sym).collect();
                fns.push(HirItem::Fn(HirFn {
                    name: *name,
                    params: param_syms,
                    body: lower_expr(&body.kind, interner),
                }));
            }
            Item::StructDef { name, fields, .. } => {
                let hir_fields: Vec<StructField> = fields.iter().map(|(sym, _)| {
                    StructField { name: *sym, ty: HirType::Int } // field type defaults to Int for now
                }).collect();
                fns.push(HirItem::Struct(StructDef { name: *name, fields: hir_fields }));
            }
                        Item::EnumDef { name, variants, .. } => {
                let hir_variants: Vec<HirVariant> = variants.iter().enumerate().map(|(i, v)| {
                    let fields = match &v.kind {
                        glyim_parse::VariantKind::Unnamed(types) | glyim_parse::VariantKind::Named(types) => {
                            types.iter().map(|(sym, _)| StructField { name: *sym, ty: HirType::Int }).collect()
                        }
                    };
                    HirVariant { name: v.name, fields, tag: i as u32 }
                }).collect();
                fns.push(HirItem::Enum(EnumDef { name: *name, variants: hir_variants }));
            }
            Item::MacroDef { name, body, .. } => {
                fns.push(HirItem::Fn(HirFn { name: *name, params: vec![], body: lower_expr(&body.kind, interner) }));
            }
            Item::MacroDef { name, body, .. } => {
                fns.push(HirItem::Fn(HirFn { name: *name, params: vec![], body: lower_expr(&body.kind, interner) }));
            }
            Item::MacroDef { name, body, .. } => {
                fns.push(HirItem::Fn(HirFn { name: *name, params: vec![], body: lower_expr(&body.kind, interner) }));
            }
            Item::ExternBlock { functions, .. } => {
                let ex_fns: Vec<ExternFn> = functions.iter().map(|f| ExternFn {
                    name: f.name,
                    params: f.params.iter().map(|_| HirType::Int).collect(),
                    ret: HirType::Int,
                }).collect();
                fns.push(HirItem::Extern(ExternBlock { functions: ex_fns }));
            }
            Item::Use(_) => {} // No-op
            Item::Stmt(_) => {}
        }
    }
    Hir { items: fns }
}

fn lower_expr(expr: &ExprKind, interner: &mut Interner) -> HirExpr {
    match expr {
        ExprKind::IntLit(n) => HirExpr::IntLit(*n),
        ExprKind::FloatLit(f) => HirExpr::FloatLit(*f),
        ExprKind::BoolLit(b) => HirExpr::BoolLit(*b),
        ExprKind::UnitLit => HirExpr::UnitLit,
        ExprKind::StrLit(s) => HirExpr::StrLit(s.clone()),
        ExprKind::Ident(sym) => HirExpr::Ident(*sym),
        ExprKind::Binary { op, lhs, rhs } => HirExpr::Binary {
            op: lower_binop(op.clone()),
            lhs: Box::new(lower_expr(&lhs.kind, interner)),
            rhs: Box::new(lower_expr(&rhs.kind, interner)),
        },
        ExprKind::Unary { op, operand } => HirExpr::Unary {
            op: lower_unop(op.clone()),
            operand: Box::new(lower_expr(&operand.kind, interner)),
        },
        ExprKind::Lambda { params: _, body } => lower_expr(&body.kind, interner),
        ExprKind::Block(items) => {
            let stmts: Vec<HirStmt> = items
                .iter()
                .map(|item| match item {
                    BlockItem::Expr(e) => HirStmt::Expr(lower_expr(&e.kind, interner)),
                    BlockItem::Stmt(s) => match &s.kind {
                        StmtKind::Let {
                            name,
                            mutable,
                            value,
                        } => HirStmt::Let {
                            name: *name,
                            mutable: *mutable,
                            value: lower_expr(&value.kind, interner),
                        },
                        StmtKind::Assign { target, value } => HirStmt::Assign {
                            target: *target,
                            value: lower_expr(&value.kind, interner),
                        },
                    },
                })
                .collect();
            HirExpr::Block(stmts)
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => HirExpr::If {
            condition: Box::new(lower_expr(&condition.kind, interner)),
            then_branch: Box::new(lower_expr(&then_branch.kind, interner)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(lower_expr(&e.kind, interner))),
        },
        ExprKind::StructLit { name, fields } => {
            let hir_fields: Vec<(Symbol, HirExpr)> = fields.iter().map(|(sym, e)| {
                (*sym, lower_expr(&e.kind, interner))
            }).collect();
            HirExpr::StructLit { struct_name: *name, fields: hir_fields }
        }
        ExprKind::Match { scrutinee, arms } => {
            let hir_arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)> = arms.iter().map(|arm| {
                let pattern = lower_pattern(&arm.pattern, interner);
                let guard = arm.guard.as_ref().map(|e| lower_expr(&e.kind, interner));
                let body = lower_expr(&arm.body.kind, interner);
                (pattern, guard, body)
            }).collect();
            HirExpr::Match {
                scrutinee: Box::new(lower_expr(&scrutinee.kind, interner)),
                arms: hir_arms,
            }
        }
        ExprKind::EnumVariant { enum_name, variant_name, args } => {
            let hir_args: Vec<HirExpr> = args.iter().map(|a| lower_expr(&a.kind, interner)).collect();
            HirExpr::EnumVariant { enum_name: *enum_name, variant_name: *variant_name, args: hir_args }
        }
        ExprKind::FieldAccess { object, field } => {
            HirExpr::FieldAccess {
                object: Box::new(lower_expr(&object.kind, interner)),
                field: *field,
            }
        }
        ExprKind::SomeExpr(e) => HirExpr::EnumVariant {
            enum_name: interner.intern("Option"),
            variant_name: interner.intern("Some"),
            args: vec![lower_expr(&e.kind, interner)],
        },
        ExprKind::NoneExpr => HirExpr::EnumVariant {
            enum_name: interner.intern("Option"),
            variant_name: interner.intern("None"),
            args: vec![],
        },
        ExprKind::OkExpr(e) => HirExpr::EnumVariant {
            enum_name: interner.intern("Result"),
            variant_name: interner.intern("Ok"),
            args: vec![lower_expr(&e.kind, interner)],
        },
        ExprKind::ErrExpr(e) => HirExpr::EnumVariant {
            enum_name: interner.intern("Result"),
            variant_name: interner.intern("Err"),
            args: vec![lower_expr(&e.kind, interner)],
        },
        ExprKind::Pointer { mutable: _, target } => HirExpr::As {
            expr: Box::new(HirExpr::IntLit(0)),
            target_type: HirType::RawPtr(Box::new(HirType::Named(*target))),
        },
        ExprKind::As { expr, target_type } => HirExpr::As {
            expr: Box::new(lower_expr(&expr.kind, interner)),
            target_type: HirType::Named(*target_type),
        },
        ExprKind::MacroCall { name, arg } => {
            let mac_name = interner.resolve(*name);
            if mac_name == "identity" {
                lower_expr(&arg.kind, interner)
            } else {
                HirExpr::IntLit(0)
            }
        }
        ExprKind::MacroCall { name, arg } => {
            let mac_name = interner.resolve(*name);
            if mac_name == "identity" {
                lower_expr(&arg.kind, interner)
            } else {
                HirExpr::IntLit(0)
            }
        }
        ExprKind::MacroCall { name, arg } => {
            let mac_name = interner.resolve(*name);
            if mac_name == "identity" {
                lower_expr(&arg.kind, interner)
            } else {
                HirExpr::IntLit(0)
            }
        }
        ExprKind::TryExpr(e) => {
            HirExpr::Match {
                scrutinee: Box::new(lower_expr(&e.kind, interner)),
                arms: vec![
                    (HirPattern::ResultOk(Box::new(HirPattern::Var(interner.intern("v")))), None, HirExpr::Ident(interner.intern("v"))),
                    (HirPattern::ResultErr(Box::new(HirPattern::Var(interner.intern("e")))), None, HirExpr::IntLit(0)),
                ],
            }
        },
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(sym) = &callee.kind {
                let name = interner.resolve(*sym);
                match name {
                    "println" => {
                        let arg = args
                            .first()
                            .map(|a| lower_expr(&a.kind, interner))
                            .unwrap_or(HirExpr::IntLit(0));
                        return HirExpr::Println(Box::new(arg));
                    }
                    "assert" => {
                        let cond = args
                            .first()
                            .map(|a| lower_expr(&a.kind, interner))
                            .unwrap_or(HirExpr::IntLit(0));
                        let msg = args
                            .get(1)
                            .map(|a| lower_expr(&a.kind, interner))
                            .map(Box::new);
                        return HirExpr::Assert {
                            condition: Box::new(cond),
                            message: msg,
                        };
                    }
                    _ => {}
                }
            }
            HirExpr::IntLit(0) // fallback for non-builtin calls
        }
    }
}

fn lower_binop(op: BinOp) -> HirBinOp {
    match op {
        BinOp::Add => HirBinOp::Add,
        BinOp::Sub => HirBinOp::Sub,
        BinOp::Mul => HirBinOp::Mul,
        BinOp::Div => HirBinOp::Div,
        BinOp::Mod => HirBinOp::Mod,
        BinOp::Eq => HirBinOp::Eq,
        BinOp::Neq => HirBinOp::Neq,
        BinOp::Lt => HirBinOp::Lt,
        BinOp::Gt => HirBinOp::Gt,
        BinOp::Lte => HirBinOp::Lte,
        BinOp::Gte => HirBinOp::Gte,
        BinOp::And => HirBinOp::And,
        BinOp::Or => HirBinOp::Or,
    }
}
fn lower_unop(op: UnOp) -> HirUnOp {
    match op {
        UnOp::Neg => HirUnOp::Neg,
        UnOp::Not => HirUnOp::Not,
    }
}

fn lower_pattern(pat: &glyim_parse::Pattern, interner: &mut Interner) -> HirPattern {
    match pat {
        glyim_parse::Pattern::Wild => HirPattern::Wild,
        glyim_parse::Pattern::BoolLit(b) => HirPattern::BoolLit(*b),
        glyim_parse::Pattern::IntLit(n) => HirPattern::IntLit(*n),
        glyim_parse::Pattern::FloatLit(f) => HirPattern::FloatLit(*f),
        glyim_parse::Pattern::StrLit(s) => HirPattern::StrLit(s.clone()),
        glyim_parse::Pattern::Unit => HirPattern::Unit,
        glyim_parse::Pattern::Var(sym) => HirPattern::Var(*sym),
        glyim_parse::Pattern::Struct { name, fields } => {
            HirPattern::Struct {
                name: *name,
                bindings: fields.iter().map(|(sym, p)| (*sym, lower_pattern(p, interner))).collect(),
            }
        }
        glyim_parse::Pattern::EnumVariant { enum_name, variant_name, args } => {
            HirPattern::EnumVariant {
                enum_name: *enum_name,
                variant_name: *variant_name,
                bindings: args.iter().enumerate().map(|(i, p)| {
                    let name = interner.intern(&i.to_string());
                    (name, lower_pattern(p, interner))
                }).collect(),
            }
        }
        glyim_parse::Pattern::OptionSome(p) => HirPattern::OptionSome(Box::new(lower_pattern(p, interner))),
        glyim_parse::Pattern::OptionNone => HirPattern::OptionNone,
        glyim_parse::Pattern::ResultOk(p) => HirPattern::ResultOk(Box::new(lower_pattern(p, interner))),
        glyim_parse::Pattern::ResultErr(p) => HirPattern::ResultErr(Box::new(lower_pattern(p, interner))),
    }
}