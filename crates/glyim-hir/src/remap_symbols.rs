use crate::{Hir, HirExpr, HirFn, HirItem, HirPattern, HirStmt, HirType};
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

/// Apply a symbol mapping to a HirType, recursively remapping all embedded symbols.
pub fn remap_type(ty: &HirType, mapping: &HashMap<Symbol, Symbol>) -> HirType {
    match ty {
        HirType::Named(sym) => HirType::Named(remap_sym(*sym, mapping)),
        HirType::Generic(sym, args) => HirType::Generic(
            remap_sym(*sym, mapping),
            args.iter().map(|a| remap_type(a, mapping)).collect(),
        ),
        HirType::Tuple(elems) => {
            HirType::Tuple(elems.iter().map(|e| remap_type(e, mapping)).collect())
        }
        HirType::RawPtr(inner) => HirType::RawPtr(Box::new(remap_type(inner, mapping))),
        HirType::Option(inner) => HirType::Option(Box::new(remap_type(inner, mapping))),
        HirType::Result(ok, err) => HirType::Result(
            Box::new(remap_type(ok, mapping)),
            Box::new(remap_type(err, mapping)),
        ),
        HirType::Func(params, ret) => HirType::Func(
            params.iter().map(|p| remap_type(p, mapping)).collect(),
            Box::new(remap_type(ret, mapping)),
        ),
        HirType::Opaque(sym) => HirType::Opaque(remap_sym(*sym, mapping)),
        other => other.clone(),
    }
}

/// Walk the entire HIR and replace every `Symbol` with the given mapping.
pub fn remap_symbols_in_hir(hir: &mut Hir, mapping: &HashMap<Symbol, Symbol>) {
    for item in &mut hir.items {
        remap_item(item, mapping);
    }
}

/// Walk the entire HIR and collect all unique symbols into the provided set.
pub fn collect_all_symbols(hir: &Hir, symbols: &mut HashSet<Symbol>) {
    for item in &hir.items {
        collect_item(item, &mut |sym| {
            symbols.insert(sym);
        });
    }
}

/// Collect all symbols referenced in a type into a set.
pub fn collect_symbols_from_type(ty: &HirType, symbols: &mut HashSet<Symbol>) {
    for_each_type(ty, &mut |sym| {
        symbols.insert(sym);
    });
}

// -- private helpers --

fn collect_item<F: FnMut(Symbol)>(item: &HirItem, f: &mut F) {
    match item {
        HirItem::Fn(fn_def) => for_each_fn(fn_def, f),
        HirItem::Struct(s) => {
            f(s.name);
            for field in &s.fields {
                f(field.name);
            }
            for tp in &s.type_params {
                f(*tp);
            }
        }
        HirItem::Enum(e) => {
            f(e.name);
            for v in &e.variants {
                f(v.name);
                for field in &v.fields {
                    f(field.name);
                }
            }
            for tp in &e.type_params {
                f(*tp);
            }
        }
        HirItem::Impl(imp) => {
            f(imp.target_name);
            for tp in &imp.type_params {
                f(*tp);
            }
            for method in &imp.methods {
                for_each_fn(method, f);
            }
        }
        HirItem::Extern(ext) => {
            for func in &ext.functions {
                f(func.name);
            }
        }
    }
}

fn for_each_fn<F: FnMut(Symbol)>(fn_def: &HirFn, f: &mut F) {
    f(fn_def.name);
    for tp in &fn_def.type_params {
        f(*tp);
    }
    for (sym, ty) in &fn_def.params {
        f(*sym);
        for_each_type(ty, f);
    }
    if let Some(ret) = &fn_def.ret {
        for_each_type(ret, f);
    }
    for_each_expr(&fn_def.body, f);
}

pub(crate) fn for_each_type<F: FnMut(Symbol)>(ty: &HirType, f: &mut F) {
    match ty {
        HirType::Named(sym) => f(*sym),
        HirType::Generic(sym, args) => {
            f(*sym);
            for a in args {
                for_each_type(a, f);
            }
        }
        HirType::Tuple(elems) => {
            for e in elems {
                for_each_type(e, f);
            }
        }
        HirType::RawPtr(inner) => for_each_type(inner, f),
        HirType::Option(inner) => for_each_type(inner, f),
        HirType::Result(ok, err) => {
            for_each_type(ok, f);
            for_each_type(err, f);
        }
        HirType::Func(params, ret) => {
            for p in params {
                for_each_type(p, f);
            }
            for_each_type(ret, f);
        }
        HirType::Opaque(sym) => f(*sym),
        _ => {}
    }
}

fn for_each_expr<F: FnMut(Symbol)>(expr: &HirExpr, f: &mut F) {
    match expr {
        HirExpr::IntLit { .. }
        | HirExpr::FloatLit { .. }
        | HirExpr::BoolLit { .. }
        | HirExpr::StrLit { .. }
        | HirExpr::UnitLit { .. } => {}
        HirExpr::Ident { name, .. } => f(*name),
        HirExpr::Binary { lhs, rhs, .. } => {
            for_each_expr(lhs, f);
            for_each_expr(rhs, f);
        }
        HirExpr::Unary { operand, .. }
        | HirExpr::Deref { expr: operand, .. }
        | HirExpr::As { expr: operand, .. } => for_each_expr(operand, f),
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Let { name, value, .. } => {
                        f(*name);
                        for_each_expr(value, f);
                    }
                    HirStmt::LetPat {
                        pattern, value, ty, ..
                    } => {
                        for_each_pattern(pattern, f);
                        for_each_expr(value, f);
                        if let Some(t) = ty {
                            for_each_type(t, f);
                        }
                    }
                    HirStmt::Assign { target, value, .. } => {
                        f(*target);
                        for_each_expr(value, f);
                    }
                    HirStmt::AssignDeref { target, value, .. } => {
                        for_each_expr(target, f);
                        for_each_expr(value, f);
                    }
                    HirStmt::AssignField {
                        object,
                        field,
                        value,
                        ..
                    } => {
                        for_each_expr(object, f);
                        f(*field);
                        for_each_expr(value, f);
                    }
                    HirStmt::Expr(e) => for_each_expr(e, f),
                }
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            for_each_expr(condition, f);
            for_each_expr(then_branch, f);
            if let Some(e) = else_branch {
                for_each_expr(e, f);
            }
        }
        HirExpr::Println { arg, .. } => for_each_expr(arg, f),
        HirExpr::Assert {
            condition, message, ..
        } => {
            for_each_expr(condition, f);
            if let Some(m) = message {
                for_each_expr(m, f);
            }
        }
        HirExpr::Call { callee, args, .. } => {
            f(*callee);
            for a in args {
                for_each_expr(a, f);
            }
        }
        HirExpr::MethodCall {
            receiver,
            method_name,
            resolved_callee,
            args,
            ..
        } => {
            for_each_expr(receiver, f);
            f(*method_name);
            if let Some(callee) = resolved_callee {
                f(*callee);
            }
            for a in args {
                for_each_expr(a, f);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            for_each_expr(scrutinee, f);
            for arm in arms {
                for_each_pattern(&arm.pattern, f);
                if let Some(g) = &arm.guard {
                    for_each_expr(g, f);
                }
                for_each_expr(&arm.body, f);
            }
        }
        HirExpr::FieldAccess { object, field, .. } => {
            for_each_expr(object, f);
            f(*field);
        }
        HirExpr::StructLit {
            struct_name,
            fields,
            ..
        } => {
            f(*struct_name);
            for (field_name, val) in fields {
                f(*field_name);
                for_each_expr(val, f);
            }
        }
        HirExpr::EnumVariant {
            enum_name,
            variant_name,
            args,
            ..
        } => {
            f(*enum_name);
            f(*variant_name);
            for a in args {
                for_each_expr(a, f);
            }
        }
        HirExpr::ForIn {
            pattern,
            iter,
            body,
            ..
        } => {
            for_each_pattern(pattern, f);
            for_each_expr(iter, f);
            for_each_expr(body, f);
        }
        HirExpr::While {
            condition, body, ..
        } => {
            for_each_expr(condition, f);
            for_each_expr(body, f);
        }
        HirExpr::Return { value, .. } => {
            if let Some(v) = value {
                for_each_expr(v, f);
            }
        }
        HirExpr::SizeOf { target_type, .. } => for_each_type(target_type, f),
        HirExpr::TupleLit { elements, .. } => {
            for e in elements {
                for_each_expr(e, f);
            }
        }
        HirExpr::AddrOf { target, .. } => f(*target),
    }
}

fn for_each_pattern<F: FnMut(Symbol)>(pat: &HirPattern, f: &mut F) {
    match pat {
        HirPattern::Var(sym) => f(*sym),
        HirPattern::Struct { name, bindings, .. } => {
            f(*name);
            for (field_name, sub) in bindings {
                f(*field_name);
                for_each_pattern(sub, f);
            }
        }
        HirPattern::EnumVariant {
            enum_name,
            variant_name,
            bindings,
            ..
        } => {
            f(*enum_name);
            f(*variant_name);
            for (field_name, sub) in bindings {
                f(*field_name);
                for_each_pattern(sub, f);
            }
        }
        HirPattern::Tuple { elements, .. } => {
            for e in elements {
                for_each_pattern(e, f);
            }
        }
        HirPattern::OptionSome(inner) => for_each_pattern(inner, f),
        HirPattern::ResultOk(inner) => for_each_pattern(inner, f),
        HirPattern::ResultErr(inner) => for_each_pattern(inner, f),
        _ => {}
    }
}

// -- remap helpers --

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
        HirExpr::Ident { name, .. } => {
            *name = remap_sym(*name, mapping);
        }
        HirExpr::Call { callee, args, .. } => {
            *callee = remap_sym(*callee, mapping);
            for a in args {
                remap_expr(a, mapping);
            }
        }
        HirExpr::MethodCall {
            receiver,
            method_name,
            resolved_callee,
            args,
            ..
        } => {
            remap_expr(receiver, mapping);
            *method_name = remap_sym(*method_name, mapping);
            if let Some(resolved) = resolved_callee {
                *resolved = remap_sym(*resolved, mapping);
            }
            for a in args {
                remap_expr(a, mapping);
            }
        }
        HirExpr::FieldAccess { object, field, .. } => {
            remap_expr(object, mapping);
            *field = remap_sym(*field, mapping);
        }
        HirExpr::StructLit {
            struct_name,
            fields,
            ..
        } => {
            *struct_name = remap_sym(*struct_name, mapping);
            for (field_name, val) in fields {
                *field_name = remap_sym(*field_name, mapping);
                remap_expr(val, mapping);
            }
        }
        HirExpr::EnumVariant {
            enum_name,
            variant_name,
            args,
            ..
        } => {
            *enum_name = remap_sym(*enum_name, mapping);
            *variant_name = remap_sym(*variant_name, mapping);
            for a in args {
                remap_expr(a, mapping);
            }
        }
        HirExpr::Block { stmts, .. } => {
            for stmt in stmts {
                match stmt {
                    HirStmt::Let { name, value, .. }
                    | HirStmt::Assign {
                        target: name,
                        value,
                        ..
                    } => {
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
                    HirStmt::AssignField {
                        object,
                        field,
                        value,
                        ..
                    } => {
                        remap_expr(object, mapping);
                        *field = remap_sym(*field, mapping);
                        remap_expr(value, mapping);
                    }
                    HirStmt::Expr(e) => remap_expr(e, mapping),
                }
            }
        }
        HirExpr::Binary { lhs, rhs, .. } => {
            remap_expr(lhs, mapping);
            remap_expr(rhs, mapping);
        }
        HirExpr::Unary { operand, .. }
        | HirExpr::Deref { expr: operand, .. }
        | HirExpr::As { expr: operand, .. } => remap_expr(operand, mapping),
        HirExpr::Return { value: Some(v), .. } => remap_expr(v, mapping),
        HirExpr::Println { arg, .. } => remap_expr(arg, mapping),
        HirExpr::Assert {
            condition, message, ..
        } => {
            remap_expr(condition, mapping);
            if let Some(msg) = message {
                remap_expr(msg, mapping);
            }
        }
        HirExpr::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            remap_expr(condition, mapping);
            remap_expr(then_branch, mapping);
            if let Some(e) = else_branch {
                remap_expr(e, mapping);
            }
        }
        HirExpr::Match {
            scrutinee, arms, ..
        } => {
            remap_expr(scrutinee, mapping);
            for arm in arms {
                remap_pattern(&mut arm.pattern, mapping);
                if let Some(g) = &mut arm.guard {
                    remap_expr(g, mapping);
                }
                remap_expr(&mut arm.body, mapping);
            }
        }
        HirExpr::While {
            condition, body, ..
        } => {
            remap_expr(condition, mapping);
            remap_expr(body, mapping);
        }
        HirExpr::ForIn {
            pattern,
            iter,
            body,
            ..
        } => {
            remap_pattern(pattern, mapping);
            remap_expr(iter, mapping);
            remap_expr(body, mapping);
        }
        HirExpr::AddrOf { target, .. } => {
            *target = remap_sym(*target, mapping);
        }
        HirExpr::TupleLit { elements, .. } => {
            for e in elements {
                remap_expr(e, mapping);
            }
        }
        _ => {}
    }
}

fn remap_pattern(pat: &mut HirPattern, mapping: &HashMap<Symbol, Symbol>) {
    match pat {
        HirPattern::Var(sym) => {
            *sym = remap_sym(*sym, mapping);
        }
        HirPattern::Struct { name, bindings, .. } => {
            *name = remap_sym(*name, mapping);
            for (field_name, sub) in bindings {
                *field_name = remap_sym(*field_name, mapping);
                remap_pattern(sub, mapping);
            }
        }
        HirPattern::EnumVariant {
            enum_name,
            variant_name,
            bindings,
            ..
        } => {
            *enum_name = remap_sym(*enum_name, mapping);
            *variant_name = remap_sym(*variant_name, mapping);
            for (field_name, sub) in bindings {
                *field_name = remap_sym(*field_name, mapping);
                remap_pattern(sub, mapping);
            }
        }
        HirPattern::Tuple { elements, .. } => {
            for e in elements {
                remap_pattern(e, mapping);
            }
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
