use crate::item::{EnumDef, ExternBlock, ExternFn, HirItem, HirVariant, StructDef, StructField};
use crate::types::HirType;
use crate::{Hir, HirBinOp, HirExpr, HirFn, HirPattern, HirStmt, HirUnOp};
use glyim_interner::{Interner, Symbol};
use glyim_parse::{BinOp, BlockItem, ExprKind, Item, StmtKind, TypeExpr, UnOp, Pattern};

pub fn lower(ast: &glyim_parse::Ast, interner: &mut Interner) -> Hir {
    let mut fns = vec![];
    for item in &ast.items {
        match item {
            Item::Binding { name, value, .. } => {
                if let ExprKind::Lambda { params, body } = &value.kind {
                    fns.push(HirItem::Fn(HirFn {
                        name: *name,
                        type_params: vec![],
                        params: params.iter().map(|sym| (*sym, HirType::Int)).collect(),
                        ret: None,
                        body: lower_expr(&body.kind, interner),
                    }));
                }
            }
            Item::FnDef { name, type_params, params, ret, body, .. } => {
                let hir_params = params.iter().map(|(sym, _, ty)| {
                    (*sym, ty.as_ref().map(|t| lower_type_expr(t, interner)).unwrap_or(HirType::Int))
                }).collect();
                let hir_ret = ret.as_ref().map(|t| lower_type_expr(t, interner));
                fns.push(HirItem::Fn(HirFn {
                    name: *name, type_params: type_params.clone(), params: hir_params, ret: hir_ret,
                    body: lower_expr(&body.kind, interner),
                }));
            }
            Item::StructDef { name, fields, .. } => {
                let hir_fields: Vec<StructField> = fields.iter().map(|(sym, _, _ty)| {
                    StructField { name: *sym, ty: HirType::Int }
                }).collect();
                fns.push(HirItem::Struct(StructDef { name: *name, fields: hir_fields }));
            }
            Item::EnumDef { name, variants, .. } => {
                let hir_variants: Vec<HirVariant> = variants.iter().enumerate().map(|(i, v)| {
                    let fields = match &v.kind {
                        glyim_parse::VariantKind::Unnamed(types) | glyim_parse::VariantKind::Named(types) => {
                            types.iter().map(|(sym, _, _)| StructField { name: *sym, ty: HirType::Int }).collect()
                        }
                    };
                    HirVariant { name: v.name, fields, tag: i as u32 }
                }).collect();
                fns.push(HirItem::Enum(EnumDef { name: *name, variants: hir_variants }));
            }
            Item::ImplBlock { methods, .. } => {
                for method in methods.iter().filter_map(|m| {
                    if let Item::FnDef { name, type_params, params, ret, body, .. } = m {
                        let hir_params = params.iter().map(|(sym, _, ty)| {
                            (*sym, ty.as_ref().map(|t| lower_type_expr(t, interner)).unwrap_or(HirType::Int))
                        }).collect();
                        let hir_ret = ret.as_ref().map(|t| lower_type_expr(t, interner));
                        Some(HirFn { name: *name, type_params: type_params.clone(), params: hir_params, ret: hir_ret, body: lower_expr(&body.kind, interner) })
                    } else { None }
                }) {
                    fns.push(HirItem::Fn(method));
                }
            }
            Item::MacroDef { name, body, .. } => {
                fns.push(HirItem::Fn(HirFn {
                    name: *name, type_params: vec![], params: vec![], ret: None,
                    body: lower_expr(&body.kind, interner),
                }));
            }
            Item::ExternBlock { functions, .. } => {
                let ex_fns: Vec<ExternFn> = functions.iter().map(|f| ExternFn {
                    name: f.name, params: f.params.iter().map(|_| HirType::Int).collect(), ret: HirType::Int,
                }).collect();
                fns.push(HirItem::Extern(ExternBlock { functions: ex_fns }));
            }
            Item::Use(_) => {}
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
        ExprKind::StrLit(s) => HirExpr::StrLit(s.clone()),
        ExprKind::Ident(sym) => HirExpr::Ident(*sym),
        ExprKind::UnitLit => HirExpr::UnitLit,
        ExprKind::Binary { op, lhs, rhs } => HirExpr::Binary { op: lower_binop(op.clone()), lhs: Box::new(lower_expr(&lhs.kind, interner)), rhs: Box::new(lower_expr(&rhs.kind, interner)) },
        ExprKind::Unary { op, operand } => HirExpr::Unary { op: lower_unop(op.clone()), operand: Box::new(lower_expr(&operand.kind, interner)) },
        ExprKind::Lambda { params: _, body } => lower_expr(&body.kind, interner),
        ExprKind::Block(items) => {
            let stmts: Vec<HirStmt> = items.iter().map(|item| match item {
                BlockItem::Expr(e) => HirStmt::Expr(lower_expr(&e.kind, interner)),
                BlockItem::Stmt(s) => match &s.kind {
                    StmtKind::Let { pattern, mutable, value } => {
                        let val = lower_expr(&value.kind, interner);
                        match pattern {
                            Pattern::Var(name) => HirStmt::Let { name: *name, mutable: *mutable, value: val },
                            _ => HirStmt::Let { name: interner.intern("_"), mutable: false, value: val },
                        }
                    }
                    StmtKind::Assign { target, value } => HirStmt::Assign { target: *target, value: lower_expr(&value.kind, interner) },
                },
            }).collect();
            HirExpr::Block(stmts)
        }
        ExprKind::If { condition, then_branch, else_branch } => HirExpr::If { condition: Box::new(lower_expr(&condition.kind, interner)), then_branch: Box::new(lower_expr(&then_branch.kind, interner)), else_branch: else_branch.as_ref().map(|e| Box::new(lower_expr(&e.kind, interner))) },
        ExprKind::StructLit { name, fields } => { let hir_fields = fields.iter().map(|(sym, e)| (*sym, lower_expr(&e.kind, interner))).collect(); HirExpr::StructLit { struct_name: *name, fields: hir_fields } },
        ExprKind::Match { scrutinee, arms } => {
            let hir_arms: Vec<(HirPattern, Option<HirExpr>, HirExpr)> = arms.iter().map(|arm| {
                let pattern = lower_pattern(&arm.pattern, interner);
                let guard = arm.guard.as_ref().map(|e| lower_expr(&e.kind, interner));
                let body = lower_expr(&arm.body.kind, interner);
                (pattern, guard, body)
            }).collect();
            HirExpr::Match { scrutinee: Box::new(lower_expr(&scrutinee.kind, interner)), arms: hir_arms }
        }
        ExprKind::EnumVariant { enum_name, variant_name, args } => {
            let hir_args = args.iter().map(|a| lower_expr(&a.kind, interner)).collect();
            HirExpr::EnumVariant { enum_name: *enum_name, variant_name: *variant_name, args: hir_args }
        }
        ExprKind::FieldAccess { object, field } => HirExpr::FieldAccess { object: Box::new(lower_expr(&object.kind, interner)), field: *field },
        ExprKind::SomeExpr(e) => HirExpr::EnumVariant { enum_name: interner.intern("Option"), variant_name: interner.intern("Some"), args: vec![lower_expr(&e.kind, interner)] },
        ExprKind::NoneExpr => HirExpr::EnumVariant { enum_name: interner.intern("Option"), variant_name: interner.intern("None"), args: vec![] },
        ExprKind::OkExpr(e) => HirExpr::EnumVariant { enum_name: interner.intern("Result"), variant_name: interner.intern("Ok"), args: vec![lower_expr(&e.kind, interner)] },
        ExprKind::ErrExpr(e) => HirExpr::EnumVariant { enum_name: interner.intern("Result"), variant_name: interner.intern("Err"), args: vec![lower_expr(&e.kind, interner)] },
        ExprKind::Pointer { mutable: _, target } => HirExpr::As { expr: Box::new(HirExpr::IntLit(0)), target_type: HirType::RawPtr(Box::new(HirType::Named(*target))) },
        ExprKind::As { expr, target_type } => HirExpr::As { expr: Box::new(lower_expr(&expr.kind, interner)), target_type: HirType::Named(*target_type) },
        ExprKind::MacroCall { name, arg } => { if interner.resolve(*name) == "identity" { lower_expr(&arg.kind, interner) } else { HirExpr::IntLit(0) } },
        ExprKind::TryExpr(e) => HirExpr::Match {
            scrutinee: Box::new(lower_expr(&e.kind, interner)),
            arms: vec![
                (HirPattern::ResultOk(Box::new(HirPattern::Var(interner.intern("v")))), None, HirExpr::Ident(interner.intern("v"))),
                (HirPattern::ResultErr(Box::new(HirPattern::Var(interner.intern("e")))), None, HirExpr::IntLit(0)),
            ],
        },
        ExprKind::Call { callee, args } => {
            if let ExprKind::Ident(sym) = &callee.kind {
                match interner.resolve(*sym) {
                    "println" => { let arg = args.first().map(|a| lower_expr(&a.kind, interner)).unwrap_or(HirExpr::IntLit(0)); return HirExpr::Println(Box::new(arg)); }
                    "assert" => { let cond = args.first().map(|a| lower_expr(&a.kind, interner)).unwrap_or(HirExpr::IntLit(0)); let msg = args.get(1).map(|a| lower_expr(&a.kind, interner)).map(Box::new); return HirExpr::Assert { condition: Box::new(cond), message: msg }; }
                    _ => {}
                }
            }
            HirExpr::IntLit(0)
        }
        ExprKind::TupleLit(_) => HirExpr::IntLit(0),
    }
}

fn lower_binop(op: BinOp) -> HirBinOp {
    match op {
        BinOp::Add => HirBinOp::Add, BinOp::Sub => HirBinOp::Sub, BinOp::Mul => HirBinOp::Mul,
        BinOp::Div => HirBinOp::Div, BinOp::Mod => HirBinOp::Mod,
        BinOp::Eq => HirBinOp::Eq, BinOp::Neq => HirBinOp::Neq,
        BinOp::Lt => HirBinOp::Lt, BinOp::Gt => HirBinOp::Gt,
        BinOp::Lte => HirBinOp::Lte, BinOp::Gte => HirBinOp::Gte,
        BinOp::And => HirBinOp::And, BinOp::Or => HirBinOp::Or,
    }
}

fn lower_unop(op: UnOp) -> HirUnOp { match op { UnOp::Neg => HirUnOp::Neg, UnOp::Not => HirUnOp::Not } }

fn lower_pattern(pat: &Pattern, interner: &mut Interner) -> HirPattern {
    match pat {
        Pattern::Wild => HirPattern::Wild, Pattern::BoolLit(b) => HirPattern::BoolLit(*b),
        Pattern::IntLit(n) => HirPattern::IntLit(*n), Pattern::FloatLit(f) => HirPattern::FloatLit(*f),
        Pattern::StrLit(s) => HirPattern::StrLit(s.clone()), Pattern::Unit => HirPattern::Unit,
        Pattern::Var(sym) => HirPattern::Var(*sym),
        Pattern::Struct { name, fields } => HirPattern::Struct { name: *name, bindings: fields.iter().map(|(sym, p)| (*sym, lower_pattern(p, interner))).collect() },
        Pattern::EnumVariant { enum_name, variant_name, args } => HirPattern::EnumVariant { enum_name: *enum_name, variant_name: *variant_name, bindings: args.iter().enumerate().map(|(i, p)| (interner.intern(&i.to_string()), lower_pattern(p, interner))).collect() },
        Pattern::Tuple(elems) => HirPattern::Tuple { elements: elems.iter().map(|e| lower_pattern(e, interner)).collect() },
        Pattern::OptionSome(p) => HirPattern::OptionSome(Box::new(lower_pattern(p, interner))),
        Pattern::OptionNone => HirPattern::OptionNone,
        Pattern::ResultOk(p) => HirPattern::ResultOk(Box::new(lower_pattern(p, interner))),
        Pattern::ResultErr(p) => HirPattern::ResultErr(Box::new(lower_pattern(p, interner))),
    }
}

fn lower_type_expr(ty: &TypeExpr, interner: &mut Interner) -> HirType {
    match ty {
        TypeExpr::Int => HirType::Int, TypeExpr::Float => HirType::Float, TypeExpr::Bool => HirType::Bool, TypeExpr::Str => HirType::Str,
        TypeExpr::Unit => HirType::Unit,
        TypeExpr::Named(sym) => HirType::Named(*sym),
        TypeExpr::Generic(sym, args) => HirType::Generic(*sym, args.iter().map(|a| lower_type_expr(a, interner)).collect()),
        TypeExpr::Tuple(elems) => HirType::Tuple(elems.iter().map(|e| lower_type_expr(e, interner)).collect()),
        TypeExpr::RawPtr { mutable: _, inner } => HirType::RawPtr(Box::new(lower_type_expr(inner, interner))),
    }
}
