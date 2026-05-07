use crate::{Hir, HirExpr, HirStmt, HirItem, HirFn, HirPattern};
use glyim_interner::Symbol;
use std::collections::HashMap;

/// Walk the entire HIR and replace every `Symbol` with the given mapping.
pub fn remap_symbols_in_hir(hir: &mut Hir, mapping: &HashMap<Symbol, Symbol>) {
    for item in &mut hir.items {
        remap_item(item, mapping);
    }
}

fn remap_item(item: &mut HirItem, mapping: &HashMap<Symbol, Symbol>) {
    match item {
        HirItem::Fn(f) => remap_fn(f, mapping),
        HirItem::Struct(s) => {
            s.name = remap_sym(s.name, mapping);
            for field in &mut s.fields {
                field.name = remap_sym(field.name, mapping);
            }
        }
        HirItem::Enum(e) => {
            e.name = remap_sym(e.name, mapping);
            for variant in &mut e.variants {
                variant.name = remap_sym(variant.name, mapping);
                for field in &mut variant.fields {
                    field.name = remap_sym(field.name, mapping);
                }
            }
        }
        HirItem::Impl(imp) => {
            imp.target_name = remap_sym(imp.target_name, mapping);
            for method in &mut imp.methods {
                remap_fn(method, mapping);
            }
        }
        HirItem::Extern(ext) => {
            for func in &mut ext.functions {
                func.name = remap_sym(func.name, mapping);
            }
        }
    }
}

fn remap_fn(f: &mut HirFn, mapping: &HashMap<Symbol, Symbol>) {
    f.name = remap_sym(f.name, mapping);
    for tp in &mut f.type_params {
        *tp = remap_sym(*tp, mapping);
    }
    for (param_sym, _ty) in &mut f.params {
        *param_sym = remap_sym(*param_sym, mapping);
    }
    remap_expr(&mut f.body, mapping);
}

fn remap_expr(expr: &mut HirExpr, mapping: &HashMap<Symbol, Symbol>) {
    match expr {
        HirExpr::Ident { name, .. } => { *name = remap_sym(*name, mapping); }
        HirExpr::Call { callee, args, .. } => {
            *callee = remap_sym(*callee, mapping);
            for a in args { remap_expr(a, mapping); }
        }
        HirExpr::MethodCall { receiver, method_name, resolved_callee, args, .. } => {
            remap_expr(receiver, mapping);
            *method_name = remap_sym(*method_name, mapping);
            if let Some(resolved) = resolved_callee {
                *resolved = remap_sym(*resolved, mapping);
            }
            for a in args { remap_expr(a, mapping); }
        }
        HirExpr::FieldAccess { object, field, .. } => {
            remap_expr(object, mapping);
            *field = remap_sym(*field, mapping);
        }
        HirExpr::StructLit { struct_name, fields, .. } => {
            *struct_name = remap_sym(*struct_name, mapping);
            for (field_name, val) in fields {
                *field_name = remap_sym(*field_name, mapping);
                remap_expr(val, mapping);
            }
        }
        HirExpr::EnumVariant { enum_name, variant_name, args, .. } => {
            *enum_name = remap_sym(*enum_name, mapping);
            *variant_name = remap_sym(*variant_name, mapping);
            for a in args { remap_expr(a, mapping); }
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Let { name, value, .. } | HirStmt::Assign { target: name, value, .. } => {
                        *name = remap_sym(*name, mapping);
                        remap_expr(value, mapping);
                    }
                    HirStmt::LetPat { pattern, value, .. } => {
                        remap_pattern(pattern, mapping);
                        remap_expr(value, mapping);
                    }
                    HirStmt::AssignDeref { target, value, .. } => {
                        remap_expr(target, mapping);
                        remap_expr(value, mapping);
                    }
                    HirStmt::AssignField { object, field, value, .. } => {
                        remap_expr(object, mapping);
                        *field = remap_sym(*field, mapping);
                        remap_expr(value, mapping);
                    }
                    HirStmt::Expr(e) => remap_expr(e, mapping),
                }
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => { remap_expr(lhs, mapping); remap_expr(rhs, mapping); }
        HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } | HirExpr::As { expr: operand, .. } => remap_expr(operand, mapping),
        HirExpr::Return { value: Some(v), .. } => remap_expr(v, mapping),
        HirExpr::Println { arg, .. } => remap_expr(arg, mapping),
        HirExpr::Assert { condition, message, .. } => {
            remap_expr(condition, mapping);
            if let Some(msg) = message { remap_expr(msg, mapping); }
        }
        HirExpr::If { condition, then_branch, else_branch, .. } => {
            remap_expr(condition, mapping);
            remap_expr(then_branch, mapping);
            if let Some(e) = else_branch { remap_expr(e, mapping); }
        }
        HirExpr::Match { scrutinee, arms, .. } => {
            remap_expr(scrutinee, mapping);
            for arm in arms {
                remap_pattern(&mut arm.pattern, mapping);
                if let Some(g) = &mut arm.guard { remap_expr(g, mapping); }
                remap_expr(&mut arm.body, mapping);
            }
        }
        HirExpr::While { condition, body, .. } => { remap_expr(condition, mapping); remap_expr(body, mapping); }
        HirExpr::ForIn { pattern, iter, body, .. } => {
            remap_pattern(pattern, mapping);
            remap_expr(iter, mapping);
            remap_expr(body, mapping);
        }
        HirExpr::AddrOf { target, .. } => { *target = remap_sym(*target, mapping); }
        HirExpr::TupleLit { elements, .. } => { for e in elements { remap_expr(e, mapping); } }
        _ => {}
    }
}

fn remap_pattern(pat: &mut HirPattern, mapping: &HashMap<Symbol, Symbol>) {
    match pat {
        HirPattern::Var(sym) => { *sym = remap_sym(*sym, mapping); }
        HirPattern::Struct { name, bindings, .. } => {
            *name = remap_sym(*name, mapping);
            for (field_name, sub_pat) in bindings {
                *field_name = remap_sym(*field_name, mapping);
                remap_pattern(sub_pat, mapping);
            }
        }
        HirPattern::EnumVariant { enum_name, variant_name, bindings, .. } => {
            *enum_name = remap_sym(*enum_name, mapping);
            *variant_name = remap_sym(*variant_name, mapping);
            for (field_name, sub_pat) in bindings {
                *field_name = remap_sym(*field_name, mapping);
                remap_pattern(sub_pat, mapping);
            }
        }
        HirPattern::Tuple { elements, .. } => {
            for e in elements { remap_pattern(e, mapping); }
        }
        HirPattern::OptionSome(inner) => remap_pattern(inner, mapping),
        HirPattern::ResultOk(inner) => remap_pattern(inner, mapping),
        HirPattern::ResultErr(inner) => remap_pattern(inner, mapping),
        _ => {}
    }
}

fn remap_sym(sym: Symbol, mapping: &HashMap<Symbol, Symbol>) -> Symbol {
    *mapping.get(&sym).unwrap_or(&sym)
}
