pub mod ty;
pub mod unify;
pub mod diagnostics;
pub mod chr;
pub mod freeze;
pub mod staging;
pub mod rep;
pub mod reflect;
pub mod comptime;
pub mod queries;

use std::collections::HashMap;
use glyim_hir::{
    types::{ExprId, HirType},
    Hir, HirExpr, HirStmt, HirItem, HirFn, HirPattern,
    StructDef, EnumDef, HirImplDef, StructField, HirVariant,
};
use glyim_interner::{Interner, Symbol};
use crate::diagnostics::TypeError;

pub struct TypeCheckOutput {
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    pub interner: Interner,
    pub reflect_metadata: Vec<()>,
    pub generated_items: Vec<()>,
}

#[derive(Clone, Debug)]
pub struct StructInfo {
    pub fields: Vec<StructField>,
    pub field_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Clone, Debug)]
pub struct EnumInfo {
    pub variants: Vec<HirVariant>,
    pub variant_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Clone, Debug)]
struct Binding {
    pub ty: HirType,
    pub mutable: bool,
}

#[derive(Clone, Debug)]
struct Scope {
    pub bindings: HashMap<Symbol, Binding>,
}

impl Scope {
    fn new() -> Self { Self { bindings: HashMap::new() } }
    fn insert(&mut self, name: Symbol, ty: HirType, mutable: bool) {
        self.bindings.insert(name, Binding { ty, mutable });
    }
    fn lookup(&self, name: &Symbol) -> Option<&HirType> {
        self.bindings.get(name).map(|b| &b.ty)
    }
}

pub struct TypeChecker {
    pub interner: Interner,
    scopes: Vec<Scope>,
    structs: HashMap<Symbol, StructInfo>,
    enums: HashMap<Symbol, EnumInfo>,
    expr_types: Vec<HirType>,
    call_type_args: HashMap<ExprId, Vec<HirType>>,
    errors: Vec<TypeError>,
    fns: Vec<HirFn>,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        TypeChecker {
            interner,
            scopes: vec![Scope::new()],
            structs: HashMap::new(),
            enums: HashMap::new(),
            expr_types: Vec::new(),
            call_type_args: HashMap::new(),
            errors: Vec::new(),
            fns: Vec::new(),
        }
    }

    pub fn check(&mut self, hir: &Hir) -> Result<TypeCheckOutput, Vec<TypeError>> {
        self.register_items(hir);
        for item in &hir.items {
            match item {
                HirItem::Fn(f) => { self.check_fn(f); }
                HirItem::Impl(imp) => {
                    for method in &imp.methods { self.check_fn(method); }
                }
                _ => {}
            }
        }
        if self.errors.is_empty() {
            Ok(TypeCheckOutput {
                expr_types: self.expr_types.clone(),
                call_type_args: self.call_type_args.clone(),
                interner: self.interner.clone(),
                reflect_metadata: vec![],
                generated_items: vec![],
            })
        } else {
            Err(self.errors.clone())
        }
    }

    fn register_items(&mut self, hir: &Hir) {
        for item in &hir.items {
            match item {
                HirItem::Struct(s) => self.register_struct(s),
                HirItem::Enum(e) => self.register_enum(e),
                HirItem::Impl(imp) => self.register_impl(imp),
                HirItem::Fn(f) => { self.fns.push(f.clone()); }
                _ => {}
            }
        }
    }

    fn register_struct(&mut self, s: &StructDef) {
        let mut field_map = HashMap::new();
        for (i, field) in s.fields.iter().enumerate() { field_map.insert(field.name, i); }
        self.structs.insert(s.name, StructInfo { fields: s.fields.clone(), field_map, type_params: s.type_params.clone() });
    }

    fn register_enum(&mut self, e: &EnumDef) {
        let mut variant_map = HashMap::new();
        for (i, v) in e.variants.iter().enumerate() { variant_map.insert(v.name, i); }
        self.enums.insert(e.name, EnumInfo { variants: e.variants.clone(), variant_map, type_params: e.type_params.clone() });
    }

    fn register_impl(&mut self, imp: &HirImplDef) {
        let methods: Vec<HirFn> = imp.methods.to_vec();
        for m in &methods {
            self.fns.push(m.clone());
            if !m.type_params.is_empty() {
                let base_name = self.interner.resolve(m.name).to_string();
                if let Some(pos) = base_name.rfind('_') {
                    let prefix = &base_name[..pos];
                    let prefix_sym = self.interner.intern(prefix);
                    if self.structs.contains_key(&prefix_sym) {
                        let short_name = base_name[pos+1..].to_string();
                        let short_sym = self.interner.intern(&short_name);
                        let mut short_fn = m.clone();
                        short_fn.name = short_sym;
                        self.fns.push(short_fn);
                    }
                }
            }
        }
    }

    fn check_fn(&mut self, f: &HirFn) {
        self.scopes = vec![Scope::new()];
        for (i, &(sym, ref ty)) in f.params.iter().enumerate() {
            let mutable = f.param_mutability.get(i).copied().unwrap_or(false);
            self.scopes[0].insert(sym, ty.clone(), mutable);
        }
        self.check_expr(&f.body);
    }

    fn set_type(&mut self, id: ExprId, ty: &HirType) {
        let idx = id.as_usize();
        if idx >= self.expr_types.len() { self.expr_types.resize(idx + 1, HirType::Never); }
        self.expr_types[idx] = ty.clone();
    }

    fn check_expr(&mut self, expr: &HirExpr) -> Option<HirType> {
        let ty = self.infer_expr(expr);
        self.set_type(expr.get_id(), &ty);
        Some(ty)
    }

    fn infer_expr(&mut self, expr: &HirExpr) -> HirType {
        match expr {
            HirExpr::IntLit { .. } => HirType::Int,
            HirExpr::FloatLit { .. } => HirType::Float,
            HirExpr::BoolLit { .. } => HirType::Bool,
            HirExpr::StrLit { .. } => HirType::Str,
            HirExpr::UnitLit { .. } => HirType::Unit,
            HirExpr::Ident { name, .. } => {
                self.scopes.iter().rev().find_map(|s| s.lookup(name).cloned()).unwrap_or(HirType::Error)
            }
            HirExpr::Binary { lhs, rhs, .. } => { self.check_expr(lhs); self.check_expr(rhs); HirType::Int }
            HirExpr::Unary { operand, .. } => { self.check_expr(operand); HirType::Int }
            HirExpr::Block { stmts, .. } => {
                let mut last = HirType::Unit;
                for stmt in stmts { if let Some(t) = self.check_stmt(stmt) { last = t; } }
                last
            }
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.check_expr(condition);
                let then_ty = self.check_expr(then_branch);
                if let Some(e) = else_branch { self.check_expr(e); }
                then_ty.unwrap_or(HirType::Unit)
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                let scrutinee_ty = self.check_expr(scrutinee).unwrap_or(HirType::Never);
                let mut arm_types = vec![];
                for arm in arms {
                    self.scopes.push(Scope::new());
                    self.bind_match_pattern(&arm.pattern, &scrutinee_ty);
                    if let Some(ref g) = arm.guard { self.check_expr(g); }
                    if let Some(t) = self.check_expr(&arm.body) { arm_types.push(t); }
                    self.scopes.pop();
                }
                arm_types.first().cloned().unwrap_or(HirType::Unit)
            }
            HirExpr::Println { arg, .. } => { self.check_expr(arg); HirType::Unit }
            HirExpr::Assert { condition, message, .. } => {
                self.check_expr(condition);
                if let Some(m) = message { self.check_expr(m); }
                HirType::Unit
            }
            HirExpr::StructLit { struct_name, fields, .. } => {
                for (field_name, v) in fields {
                    self.check_expr(v);
                    if let Some(info) = self.structs.get(struct_name) {
                        if !info.field_map.contains_key(field_name) {
                            self.errors.push(TypeError::MismatchedTypes {
                                expected_span: (0..0).into(),
                                found_span: (0..0).into(),
                                expected: format!("struct `{}`", self.interner.resolve(*struct_name)),
                                found: format!("unknown field `{}`", self.interner.resolve(*field_name)),
                                diff_path: None,
                                autofix: None,
                            });
                        }
                    }
                }
                if let Some(info) = self.structs.get(struct_name) {
                    for field in &info.fields {
                        if !fields.iter().any(|(n, _)| n == &field.name) {
                            self.errors.push(TypeError::MismatchedTypes {
                                expected_span: (0..0).into(),
                                found_span: (0..0).into(),
                                expected: format!("struct `{}`", self.interner.resolve(*struct_name)),
                                found: format!("missing field `{}`", self.interner.resolve(field.name)),
                                diff_path: None,
                                autofix: None,
                            });
                        }
                    }
                    if info.type_params.is_empty() { HirType::Named(*struct_name) }
                    else { HirType::Generic(*struct_name, vec![HirType::Int; info.type_params.len()]) }
                } else { HirType::Named(*struct_name) }
            }
            HirExpr::EnumVariant { enum_name, args, .. } => {
                for a in args { self.check_expr(a); }
                if let Some(info) = self.enums.get(enum_name) {
                    if info.type_params.is_empty() { HirType::Named(*enum_name) }
                    else { HirType::Generic(*enum_name, vec![HirType::Int; info.type_params.len()]) }
                } else { HirType::Named(*enum_name) }
            }
            HirExpr::FieldAccess { object, field, .. } => {
                let obj_ty = self.check_expr(object).unwrap_or(HirType::Error);
                let struct_sym = match &obj_ty {
                    HirType::Named(s) | HirType::Generic(s, _) => *s,
                    _ => return HirType::Error,
                };
                if let Some(info) = self.structs.get(&struct_sym) {
                    for fld in &info.fields {
                        if fld.name == *field {
                            return fld.ty.clone();
                        }
                    }
                }
                HirType::Error
            }
            HirExpr::Call { callee, args, .. } => {
                for a in args { self.check_expr(a); }
                if let Some(fn_def) = self.fns.iter().find(|f| f.name == *callee) {
                    if !fn_def.type_params.is_empty() {
                        let n = fn_def.type_params.len();
                        self.call_type_args.entry(expr.get_id()).or_insert_with(|| vec![HirType::Int; n]);
                    }
                    fn_def.ret.clone().unwrap_or(HirType::Int)
                } else {
                    HirType::Int
                }
            }
            HirExpr::MethodCall { id, receiver, method_name, args, .. } => {
                let recv_ty = self.check_expr(receiver).unwrap_or(HirType::Int);
                for a in args { self.check_expr(a); }
                let type_sym = match &recv_ty { HirType::Named(s) | HirType::Generic(s, _) => Some(*s), _ => None };
                if let Some(type_name) = type_sym {
                    let mangled = self.interner.intern(&format!("{}_{}",
                        self.interner.resolve(type_name), self.interner.resolve(*method_name)));
                    if let Some(fn_def) = self.fns.iter().find(|f| f.name == mangled) {
                        if !fn_def.type_params.is_empty() {
                            let type_args = match &recv_ty {
                                HirType::Generic(_, ta) => ta.clone(),
                                _ => vec![HirType::Int; fn_def.type_params.len()],
                            };
                            self.call_type_args.entry(*id).or_insert_with(|| type_args);
                        }
                        fn_def.ret.clone().unwrap_or(HirType::Int)
                    } else {
                        HirType::Int
                    }
                } else {
                    HirType::Int
                }
            }
            HirExpr::While { condition, body, .. } | HirExpr::ForIn { iter: condition, body, .. } => {
                self.check_expr(condition);
                self.check_expr(body);
                HirType::Unit
            }
            HirExpr::Return { value, .. } => {
                if let Some(v) = value { self.check_expr(v); }
                HirType::Never
            }
            HirExpr::As { target_type, .. } => target_type.clone(),
            HirExpr::SizeOf { .. } => HirType::Int,
            HirExpr::AddrOf { .. } => HirType::Int,
            HirExpr::Deref { expr: e, .. } => { self.check_expr(e); HirType::Int }
            HirExpr::TupleLit { elements, .. } => { for e in elements { self.check_expr(e); } HirType::Int }
        }
    }

    fn check_stmt(&mut self, stmt: &HirStmt) -> Option<HirType> {
        match stmt {
            HirStmt::Let { name, mutable, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.scopes.last_mut().unwrap().insert(*name, ty.clone(), *mutable);
                None
            }
            HirStmt::LetPat { pattern, mutable, value, ty: annotation, .. } => {
                let inferred = self.check_expr(value).unwrap_or(HirType::Int);
                let ty = match annotation {
                    Some(annot) => annot.clone(),
                    None => {
                        if let HirPattern::Var(sym) = pattern {
                            if let Some(existing) = self.scopes.last().unwrap().lookup(sym) {
                                if let HirType::Generic(base, existing_args) = existing {
                                    HirType::Generic(*base, existing_args.clone())
                                } else {
                                    inferred
                                }
                            } else {
                                inferred
                            }
                        } else {
                            inferred
                        }
                    }
                };
                self.bind_pattern(pattern, &ty, *mutable);
                None
            }
            HirStmt::Assign { target, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.scopes.last_mut().unwrap().insert(*target, ty.clone(), true);
                Some(ty)
            }
            HirStmt::AssignDeref { target, value, .. } => {
                self.check_expr(target);
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                Some(ty)
            }
            HirStmt::AssignField { object, value, .. } => {
                self.check_expr(object);
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                Some(ty)
            }
            HirStmt::Expr(e) => self.check_expr(e),
        }
    }

    fn bind_pattern(&mut self, pattern: &HirPattern, value_ty: &HirType, mutable: bool) {
        match pattern {
            HirPattern::Var(sym) => {
                self.scopes.last_mut().unwrap().insert(*sym, value_ty.clone(), mutable);
            }
            HirPattern::Wild => {}
            _ => {}
        }
    }

    fn bind_match_pattern(&mut self, pattern: &HirPattern, scrutinee_ty: &HirType) {
        match pattern {
            HirPattern::Var(sym) => {
                self.scopes.last_mut().unwrap().insert(*sym, scrutinee_ty.clone(), false);
            }
            HirPattern::Wild => {}
            HirPattern::Struct { name, bindings, .. } => {
                let field_tys: Vec<(HirPattern, HirType)> = if let Some(info) = self.structs.get(name) {
                    bindings.iter().filter_map(|(field_sym, field_pat)| {
                        info.field_map.get(field_sym).and_then(|&idx| info.fields.get(idx).map(|f| (field_pat.clone(), f.ty.clone())))
                    }).collect()
                } else { vec![] };
                for (field_pat, field_ty) in field_tys { self.bind_match_pattern(&field_pat, &field_ty); }
            }
            HirPattern::OptionSome(inner) => {
                let inner_ty = match scrutinee_ty {
                    HirType::Option(inner) => inner.as_ref().clone(),
                    HirType::Generic(_, args) if args.len() == 1 => args[0].clone(),
                    _ => HirType::Error,
                };
                self.bind_match_pattern(inner, &inner_ty);
            }
            HirPattern::ResultOk(inner) => {
                let ok_ty = match scrutinee_ty {
                    HirType::Result(ok, _) => ok.as_ref().clone(),
                    HirType::Generic(_, args) if args.len() >= 1 => args[0].clone(),
                    _ => HirType::Error,
                };
                self.bind_match_pattern(inner, &ok_ty);
            }
            HirPattern::ResultErr(inner) => {
                let err_ty = match scrutinee_ty {
                    HirType::Result(_, err) => err.as_ref().clone(),
                    HirType::Generic(_, args) if args.len() >= 2 => args[1].clone(),
                    _ => HirType::Error,
                };
                self.bind_match_pattern(inner, &err_ty);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
