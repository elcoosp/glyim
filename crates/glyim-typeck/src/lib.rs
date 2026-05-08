pub mod chr;
pub mod comptime;
pub mod diagnostics;
pub mod freeze;
pub mod queries;
pub mod reflect;
pub mod rep;
pub mod staging;
pub mod ty;
pub mod type_errors;
pub mod unify;
pub use type_errors::TypeError;

use glyim_hir::{
    EnumDef, ExternFn, Hir, HirExpr, HirFn, HirImplDef, HirItem, HirStmt, HirVariant, StructDef,
    StructField,
    types::{ExprId, HirPattern, HirType},
};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

pub struct TypeCheckOutput {
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    pub interner: Interner,
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
    fn new() -> Self {
        Self {
            bindings: HashMap::new(),
        }
    }
    fn insert(&mut self, name: Symbol, ty: HirType, mutable: bool) {
        self.bindings.insert(name, Binding { ty, mutable });
    }
    fn lookup(&self, name: &Symbol) -> Option<&HirType> {
        self.bindings.get(name).map(|b| &b.ty)
    }
    fn lookup_binding(&self, name: &Symbol) -> Option<&Binding> {
        self.bindings.get(name)
    }
}

pub struct TypeChecker {
    pub interner: Interner,
    scopes: Vec<Scope>,
    structs: HashMap<Symbol, StructInfo>,
    enums: HashMap<Symbol, EnumInfo>,
    extern_fns: HashMap<Symbol, ExternFn>,
    impl_methods: HashMap<Symbol, Vec<HirFn>>,
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
            extern_fns: HashMap::new(),
            impl_methods: HashMap::new(),
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
                HirItem::Fn(f) => {
                    self.check_fn(f);
                }
                HirItem::Impl(imp) => {
                    for method in &imp.methods {
                        self.check_fn(method);
                    }
                }
                _ => {}
            }
        }
        if self.errors.is_empty() {
            Ok(TypeCheckOutput {
                expr_types: self.expr_types.clone(),
                call_type_args: self.call_type_args.clone(),
                interner: self.interner.clone(),
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
                HirItem::Extern(ext) => self.register_extern(ext),
                HirItem::Impl(imp) => self.register_impl(imp),
                HirItem::Fn(f) => {
                    self.fns.push(f.clone());
                }
            }
        }
    }

    fn register_struct(&mut self, s: &StructDef) {
        let mut field_map = HashMap::new();
        for (i, field) in s.fields.iter().enumerate() {
            field_map.insert(field.name, i);
        }
        self.structs.insert(
            s.name,
            StructInfo {
                fields: s.fields.clone(),
                field_map,
                type_params: s.type_params.clone(),
            },
        );
    }

    fn register_enum(&mut self, e: &EnumDef) {
        let mut variant_map = HashMap::new();
        for (i, v) in e.variants.iter().enumerate() {
            variant_map.insert(v.name, i);
        }
        self.enums.insert(
            e.name,
            EnumInfo {
                variants: e.variants.clone(),
                variant_map,
                type_params: e.type_params.clone(),
            },
        );
    }

    fn register_extern(&mut self, ext: &glyim_hir::item::ExternBlock) {
        for f in &ext.functions {
            self.extern_fns.insert(f.name, f.clone());
        }
    }

    fn register_impl(&mut self, imp: &HirImplDef) {
        let methods: Vec<HirFn> = imp.methods.to_vec();
        for m in &methods {
            self.fns.push(m.clone());
        }
        self.impl_methods.insert(imp.target_name, methods);
    }

    fn check_fn(&mut self, f: &HirFn) {
        self.scopes = vec![Scope::new()];
        for (i, &(sym, ref ty)) in f.params.iter().enumerate() {
            let mutable = f.param_mutability.get(i).copied().unwrap_or(false);
            self.scopes[0].insert(sym, ty.clone(), mutable);
        }
        let body_type = self.check_expr(&f.body);
        if let Some(expected) = &f.ret {
            if let Some(actual) = body_type {
                // Only check for non-generic functions AND when the actual type
                // is not a type parameter (avoids false positives on generics).
                let is_type_param =
                    |ty: &HirType| matches!(ty, HirType::Named(s) if f.type_params.contains(s));
                if f.type_params.is_empty()
                    && *expected != actual
                    && !is_type_param(&actual)
                    && !is_type_param(expected)
                {
                    self.errors.push(TypeError::InvalidReturnType {
                        expected: expected.clone(),
                        found: actual.clone(),
                    });
                }
            }
        }
    }

    /// Returns true if the type contains a generic type parameter (single uppercase letter).
    fn has_type_parameter(&self, ty: &HirType) -> bool {
        match ty {
            HirType::Named(sym) => {
                let s = self.interner.resolve(*sym);
                s.len() == 1 && s.chars().next().is_some_and(|c| c.is_uppercase())
            }
            HirType::Generic(_, args) => args.iter().any(|a| self.has_type_parameter(a)),
            HirType::Tuple(elems) => elems.iter().any(|e| self.has_type_parameter(e)),
            HirType::RawPtr(inner) | HirType::Option(inner) => self.has_type_parameter(inner),
            HirType::Result(ok, err) => {
                self.has_type_parameter(ok) || self.has_type_parameter(err)
            }
            _ => false,
        }
    }

    fn set_type(&mut self, id: ExprId, ty: &HirType) {
        let idx = id.as_usize();
        if idx >= self.expr_types.len() {
            self.expr_types.resize(idx + 1, HirType::Never);
        }
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
            HirExpr::Ident { name, span, .. } => self
                .scopes
                .iter()
                .rev()
                .find_map(|s| s.lookup(name).cloned())
                .unwrap_or_else(|| {
                    let resolved = self.interner.resolve(*name).to_string();
                    let suggestions =
                        glyim_diag::suggest::suggest_similar(&resolved, &self.interner, 3);
                    self.errors.push(TypeError::UnresolvedName {
                        name: resolved,
                        span: (span.start, span.end),
                        suggestions,
                    });
                    HirType::Error
                }),
            HirExpr::Binary { lhs, rhs, op, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
                match op {
                    glyim_hir::HirBinOp::Eq
                    | glyim_hir::HirBinOp::Neq
                    | glyim_hir::HirBinOp::Lt
                    | glyim_hir::HirBinOp::Gt
                    | glyim_hir::HirBinOp::Lte
                    | glyim_hir::HirBinOp::Gte => HirType::Bool,
                    _ => HirType::Int,
                }
            }
            HirExpr::Unary { operand, .. } => {
                self.check_expr(operand);
                HirType::Int
            }
            HirExpr::Block { stmts, .. } => {
                let mut last = HirType::Unit;
                for stmt in stmts {
                    if let Some(t) = self.check_stmt(stmt) {
                        last = t;
                    }
                }
                last
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.check_expr(condition);
                let then_ty = self.check_expr(then_branch);
                if let Some(e) = else_branch {
                    self.check_expr(e);
                }
                then_ty.unwrap_or(HirType::Unit)
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                let scrutinee_ty = self.check_expr(scrutinee).unwrap_or(HirType::Never);
                self.check_match_exhaustiveness(&scrutinee_ty, arms, expr.get_span());
                let mut arm_types = vec![];
                for arm in arms {
                    self.scopes.push(Scope::new());
                    self.bind_match_pattern(&arm.pattern, &scrutinee_ty);
                    if let Some(ref g) = arm.guard {
                        self.check_expr(g);
                    }
                    if let Some(t) = self.check_expr(&arm.body) {
                        arm_types.push(t);
                    }
                    self.scopes.pop();
                }
                arm_types.first().cloned().unwrap_or(HirType::Unit)
            }
            HirExpr::Println { arg, .. } => {
                self.check_expr(arg);
                HirType::Unit
            }
            HirExpr::Assert {
                condition, message, ..
            } => {
                self.check_expr(condition);
                if let Some(m) = message {
                    self.check_expr(m);
                }
                HirType::Unit
            }
            HirExpr::StructLit {
                struct_name,
                fields,
                ..
            } => {
                let field_names: Vec<Symbol> = fields.iter().map(|(n, _)| *n).collect();
                let field_count = fields.len();
                for (field_name, v) in fields {
                    self.check_expr(v);
                    if let Some(info) = self.structs.get(struct_name) {
                        if !info.field_map.contains_key(field_name) {
                            self.errors.push(TypeError::UnknownField {
                                struct_name: self.interner.resolve(*struct_name).to_string(),
                                field: self.interner.resolve(*field_name).to_string(),
                                span: (0, 0),
                            });
                        }
                    }
                }
                if let Some(info) = self.structs.get(struct_name) {
                    if field_count != info.fields.len() {
                        for field in &info.fields {
                            if !field_names.contains(&field.name) {
                                self.errors.push(TypeError::MissingField {
                                    struct_name: self.interner.resolve(*struct_name).to_string(),
                                    field: self.interner.resolve(field.name).to_string(),
                                    span: (0, 0),
                                });
                            }
                        }
                    }
                }
                if let Some(info) = self.structs.get(struct_name) {
                    if info.type_params.is_empty() {
                        HirType::Named(*struct_name)
                    } else {
                        let field_value_types: Vec<(Symbol, HirType)> = fields
                            .iter()
                            .map(|(fname, fexpr)| {
                                let fty = self.expr_types[fexpr.get_id().as_usize()].clone();
                                (*fname, fty)
                            })
                            .collect();
                        match self.infer_struct_type_args(
                            info,
                            &field_value_types,
                            expr.get_id(),
                            expr.get_span(),
                        ) {
                            Ok(sub) => {
                                let concrete_args: Vec<HirType> = info
                                    .type_params
                                    .iter()
                                    .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Error))
                                    .collect();
                                HirType::Generic(*struct_name, concrete_args)
                            }
                            Err(_) => HirType::Generic(
                                *struct_name,
                                info.type_params
                                    .iter()
                                    .map(|tp| HirType::Named(*tp))
                                    .collect(),
                            ),
                        }
                    }
                } else {
                    HirType::Named(*struct_name)
                }
            }
            HirExpr::EnumVariant {
                enum_name, args, ..
            } => {
                for a in args {
                    self.check_expr(a);
                }
                if let Some(info) = self.enums.get(enum_name) {
                    if info.type_params.is_empty() {
                        HirType::Named(*enum_name)
                    } else {
                        HirType::Generic(
                            *enum_name,
                            info.type_params.iter().map(|_| HirType::Int).collect(),
                        )
                    }
                } else {
                    HirType::Named(*enum_name)
                }
            }
            HirExpr::FieldAccess { object, field, .. } => {
                let obj_ty = self.check_expr(object).unwrap_or(HirType::Error);
                match &obj_ty {
                    HirType::Named(s) | HirType::Generic(s, _) => {
                        // only check known structs; type params silently return Int
                        if let Some(info) = self.structs.get(s) {
                            if let Some(fld) = info.fields.iter().find(|f| f.name == *field) {
                                return fld.ty.clone();
                            } else {
                                self.errors.push(TypeError::UnknownField {
                                    struct_name: self.interner.resolve(*s).to_string(),
                                    field: self.interner.resolve(*field).to_string(),
                                    span: (0, 0),
                                });
                            }
                        }
                    }
                    HirType::Tuple(elems) => {
                        let field_name = self.interner.resolve(*field);
                        if let Some(idx) = field_name
                            .strip_prefix('_')
                            .and_then(|s| s.parse::<usize>().ok())
                        {
                            if idx < elems.len() {
                                return elems[idx].clone();
                            }
                        }
                        self.errors.push(TypeError::UnknownField {
                            struct_name: "tuple".into(),
                            field: field_name.to_string(),
                            span: (0, 0),
                        });
                    }
                    _ => { /* ignore */ }
                }
                HirType::Error
            }
            HirExpr::As {
                expr, target_type, ..
            } => {
                let from_ty = self.check_expr(expr).unwrap_or(HirType::Error);
                if !self.is_valid_cast(&from_ty, target_type) {
                    self.errors.push(TypeError::MismatchedTypes {
                        expected: target_type.clone(),
                        found: from_ty,
                        expr_id: expr.get_id(),
                        span: (0, 0),
                    });
                }
                target_type.clone()
            }
            HirExpr::Deref { expr, .. } => {
                let inner_ty = self.check_expr(expr).unwrap_or(HirType::Error);
                match inner_ty {
                    HirType::RawPtr(inner) => *inner,
                    _ => {
                        self.errors.push(TypeError::DerefNonPointer {
                            found: inner_ty,
                            expr_id: expr.get_id(),
                            span: (0, 0),
                        });
                        HirType::Error
                    }
                }
            }
            HirExpr::Call {
                id, callee, args, ..
            } => {
                for a in args {
                    self.check_expr(a);
                }
                let callee_sym = *callee;
                // Try external first
                if let Some(ext_fn) = self.extern_fns.get(&callee_sym) {
                    return ext_fn.ret.clone();
                }
                if let Some(fn_def) = self.fns.iter().find(|f| f.name == callee_sym) {
                    // Infer generics
                    if !fn_def.type_params.is_empty() {
                        let arg_types: Vec<HirType> = args
                            .iter()
                            .map(|a| self.expr_types[a.get_id().as_usize()].clone())
                            .collect();
                        match self.unify_generics(fn_def, &arg_types, *id, expr.get_span()) {
                            Ok(sub) => {
                                let ret = fn_def.ret.clone().unwrap_or(HirType::Unit);
                                let concrete_ret = glyim_hir::types::substitute_type(&ret, &sub);
                                let concrete_args: Vec<HirType> = fn_def
                                    .type_params
                                    .iter()
                                    .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Error))
                                    .collect();
                                self.call_type_args.insert(*id, concrete_args);
                                return concrete_ret;
                            }
                            Err(e) => {
                                self.errors.push(e);
                                return HirType::Error;
                            }
                        }
                    }
                    // Non-generic: check argument types against parameter types
                    // Only for non-generic functions (generic ones are handled by unify_generics above)
                    if fn_def.type_params.is_empty() {
                        for (_, ((_, param_ty), arg_expr)) in
                            fn_def.params.iter().zip(args.iter()).enumerate()
                        {
                            let arg_ty = self.expr_types[arg_expr.get_id().as_usize()].clone();
                            if arg_ty != *param_ty
                                && arg_ty != HirType::Error
                                && *param_ty != HirType::Never
                                && !self.has_type_parameter(param_ty)
                                && !self.has_type_parameter(&arg_ty)
                            {
                                self.errors.push(TypeError::MismatchedTypes {
                                    expected: param_ty.clone(),
                                    found: arg_ty,
                                    expr_id: arg_expr.get_id(),
                                    span: (arg_expr.get_span().start, arg_expr.get_span().end),
                                });
                            }
                        }
                    }
                    return fn_def.ret.clone().unwrap_or(HirType::Int);
                }
                HirType::Int
            }
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                ..
            } => {
                let recv_ty = self.check_expr(receiver).unwrap_or(HirType::Int);
                for a in args {
                    self.check_expr(a);
                }
                let type_sym = match &recv_ty {
                    HirType::Named(s) | HirType::Generic(s, _) => Some(*s),
                    _ => None,
                };
                if let Some(type_name) = type_sym {
                    if let Some(methods) = self.impl_methods.get(&type_name) {
                        let mangled = self.interner.intern(&format!(
                            "{}_{}",
                            self.interner.resolve(type_name),
                            self.interner.resolve(*method_name)
                        ));
                        if let Some(fn_def) = methods.iter().find(|m| m.name == mangled) {
                            eprintln!(
                                "DEBUG MethodCall ret: name={}, ret={:?}",
                                self.interner.resolve(*method_name),
                                fn_def.ret
                            );
                            if !fn_def.type_params.is_empty() {
                                let all_arg_types: Vec<HirType> =
                                    std::iter::once(recv_ty.clone())
                                        .chain(args.iter().map(|a| {
                                            self.expr_types[a.get_id().as_usize()].clone()
                                        }))
                                        .collect();
                                match self.unify_generics(
                                    fn_def,
                                    &all_arg_types,
                                    *id,
                                    expr.get_span(),
                                ) {
                                    Ok(sub) => {
                                        let ret = fn_def.ret.clone().unwrap_or(HirType::Int);
                                        let concrete_ret =
                                            glyim_hir::types::substitute_type(&ret, &sub);
                                        let concrete_args: Vec<HirType> = fn_def
                                            .type_params
                                            .iter()
                                            .map(|tp| {
                                                sub.get(tp).cloned().unwrap_or(HirType::Error)
                                            })
                                            .collect();
                                        self.call_type_args.insert(*id, concrete_args);
                                        return concrete_ret;
                                    }
                                    Err(e) => {
                                        self.errors.push(e);
                                        return HirType::Error;
                                    }
                                }
                            }
                            return fn_def.ret.clone().unwrap_or(HirType::Int);
                        }
                    }
                }
                HirType::Int
            }
            HirExpr::While {
                condition, body, ..
            }
            | HirExpr::ForIn {
                iter: condition,
                body,
                ..
            } => {
                self.check_expr(condition);
                self.check_expr(body);
                HirType::Unit
            }
            HirExpr::Return { value, .. } => {
                if let Some(v) = value {
                    self.check_expr(v);
                }
                HirType::Never
            }
            HirExpr::SizeOf { .. } => HirType::Int,
            HirExpr::AddrOf { .. } => HirType::Int,
            HirExpr::TupleLit { elements, .. } => {
                let types: Vec<HirType> = elements
                    .iter()
                    .map(|e| self.check_expr(e).unwrap_or(HirType::Error))
                    .collect();
                if types.iter().any(|t| *t == HirType::Error) {
                    HirType::Error
                } else {
                    HirType::Tuple(types)
                }
            }
        }
    }

    fn check_stmt(&mut self, stmt: &HirStmt) -> Option<HirType> {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                ..
            } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(*name, ty.clone(), *mutable);
                None
            }
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                ty: annotation,
                ..
            } => {
                let inferred = self.check_expr(value).unwrap_or(HirType::Int);
                let ty = if let Some(annotated) = annotation {
                    // annotation compatibility check
                    // Compatibility check: accept generic vs. concrete with same base name
                    let compat = Self::types_compatible(&annotated, &inferred);
                    if !compat && inferred != HirType::Error {
                        self.errors.push(TypeError::MismatchedTypes {
                            expected: annotated.clone(),
                            found: inferred.clone(),
                            expr_id: value.get_id(),
                            span: (0, 0),
                        });
                    }
                    // Backward inference: if value is a generic Call, extract type args from annotation
                    if let HirExpr::Call {
                        id: call_id,
                        callee,
                        ..
                    } = value
                    {
                        if let Some(fn_def) = self.fns.iter().find(|f| f.name == *callee) {
                            if !fn_def.type_params.is_empty() {
                                if let HirType::Generic(_, type_args) = annotated {
                                    if type_args.len() == fn_def.type_params.len() {
                                        self.call_type_args.insert(*call_id, type_args.clone());
                                    }
                                }
                            }
                        }
                    }
                    annotated.clone()
                } else {
                    inferred
                };
                self.bind_pattern(pattern, &ty, *mutable);
                None
            }
            HirStmt::Assign { target, value, .. } => {
                if let Some(b) = self.scopes.last().and_then(|s| s.lookup_binding(target)) {
                    if !b.mutable {
                        self.errors.push(TypeError::AssignToImmutable {
                            name: self.interner.resolve(*target).to_string(),
                            expr_id: value.get_id(),
                            span: (0, 0),
                        });
                    }
                }
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(*target, ty.clone(), true);
                Some(ty)
            }
            HirStmt::AssignDeref { target, value, .. } => {
                // target is a Deref; get the pointer operand type
                let pointer_ty = if let HirExpr::Deref { expr, .. } = target.as_ref() {
                    self.check_expr(expr).unwrap_or(HirType::Never)
                } else {
                    self.check_expr(target).unwrap_or(HirType::Never)
                };
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                match pointer_ty {
                    HirType::RawPtr(_) => {}
                    _ => {
                        self.errors.push(TypeError::AssignThroughNonPointer {
                            found: pointer_ty,
                            expr_id: value.get_id(),
                            span: (0, 0),
                        });
                    }
                }
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
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(*sym, value_ty.clone(), mutable);
            }
            HirPattern::Wild => {}
            HirPattern::Struct { name, bindings, .. } => {
                if let Some(info) = self.structs.get(name) {
                    let field_tys: Vec<(HirPattern, HirType)> = bindings
                        .iter()
                        .filter_map(|(field_sym, field_pat)| {
                            info.field_map.get(field_sym).and_then(|&idx| {
                                info.fields
                                    .get(idx)
                                    .map(|f| (field_pat.clone(), f.ty.clone()))
                            })
                        })
                        .collect();
                    for (field_pat, field_ty) in field_tys {
                        self.bind_pattern(&field_pat, &field_ty, mutable);
                    }
                }
            }
            HirPattern::Tuple { elements, .. } => {
                if let HirType::Tuple(elem_types) = value_ty {
                    let pats_and_tys: Vec<(HirPattern, HirType)> = elements
                        .iter()
                        .zip(elem_types.iter())
                        .map(|(p, t)| (p.clone(), t.clone()))
                        .collect();
                    for (p, t) in pats_and_tys {
                        self.bind_pattern(&p, &t, mutable);
                    }
                }
            }
            _ => {}
        }
    }

    fn bind_match_pattern(&mut self, pattern: &HirPattern, scrutinee_ty: &HirType) {
        match pattern {
            HirPattern::Var(sym) => {
                self.scopes
                    .last_mut()
                    .unwrap()
                    .insert(*sym, scrutinee_ty.clone(), false);
            }
            HirPattern::Wild => {}
            HirPattern::Struct { name, bindings, .. } => {
                let field_tys: Vec<(HirPattern, HirType)> =
                    if let Some(info) = self.structs.get(name) {
                        bindings
                            .iter()
                            .filter_map(|(field_sym, field_pat)| {
                                info.field_map.get(field_sym).and_then(|&idx| {
                                    info.fields
                                        .get(idx)
                                        .map(|f| (field_pat.clone(), f.ty.clone()))
                                })
                            })
                            .collect()
                    } else {
                        vec![]
                    };
                for (field_pat, field_ty) in field_tys {
                    self.bind_match_pattern(&field_pat, &field_ty);
                }
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

    fn check_match_exhaustiveness(
        &mut self,
        scrutinee_type: &HirType,
        arms: &[glyim_hir::MatchArm],
        span: glyim_diag::Span,
    ) {
        let enum_variants = match scrutinee_type {
            HirType::Named(name) | HirType::Generic(name, _) => {
                if let Some(info) = self.enums.get(name) {
                    info.variants.iter().map(|v| v.name).collect()
                } else {
                    vec![]
                }
            }
            HirType::Option(_) => vec![self.interner.intern("Some"), self.interner.intern("None")],
            HirType::Result(_, _) => vec![self.interner.intern("Ok"), self.interner.intern("Err")],
            _ => vec![],
        };
        if enum_variants.is_empty() {
            return;
        }
        let has_wildcard = arms
            .iter()
            .any(|arm| matches!(arm.pattern, HirPattern::Wild));
        if has_wildcard {
            return;
        }
        let covered: Vec<Symbol> = arms
            .iter()
            .filter_map(|arm| match &arm.pattern {
                HirPattern::EnumVariant { variant_name, .. } => Some(*variant_name),
                HirPattern::OptionSome(_) => Some(self.interner.intern("Some")),
                HirPattern::OptionNone => Some(self.interner.intern("None")),
                HirPattern::ResultOk(_) => Some(self.interner.intern("Ok")),
                HirPattern::ResultErr(_) => Some(self.interner.intern("Err")),
                _ => None,
            })
            .collect();
        let missing: Vec<String> = enum_variants
            .iter()
            .filter(|v| !covered.contains(v))
            .map(|v| self.interner.resolve(*v).to_string())
            .collect();
        if !missing.is_empty() {
            self.errors.push(TypeError::NonExhaustiveMatch {
                missing,
                span: (span.start, span.end),
            });
        }
    }

    fn is_valid_cast(&self, from: &HirType, to: &HirType) -> bool {
        // Allow casts involving generic type parameters; they'll be validated during monomorphization.
        if self.has_type_parameter(from) || self.has_type_parameter(to) {
            return true;
        }
        let resolved_from = self.resolve_to_primitive(from).unwrap_or(from.clone());
        let resolved_to = self.resolve_to_primitive(to).unwrap_or(to.clone());
        match (&resolved_from, &resolved_to) {
            (HirType::Int, HirType::Float) | (HirType::Float, HirType::Int) => true,
            (HirType::Int, HirType::Int) | (HirType::Float, HirType::Float) => true,
            (_, HirType::RawPtr(_)) => true,
            (HirType::RawPtr(_), _) => true,
            (a, b) if a == b => true,
            _ => false,
        }
    }

    fn resolve_to_primitive(&self, ty: &HirType) -> Option<HirType> {
        match ty {
            HirType::Int | HirType::Float | HirType::Bool | HirType::Str => Some(ty.clone()),
            HirType::Named(sym) => {
                let name = self.interner.resolve(*sym);
                match name {
                    "i8" | "i16" | "i32" | "i64" | "Int" => Some(HirType::Int),
                    "u8" | "u16" | "u32" | "u64" => Some(HirType::Int),
                    "f32" | "f64" | "Float" => Some(HirType::Float),
                    "bool" | "Bool" => Some(HirType::Bool),
                    "Str" | "str" => Some(HirType::Str),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ── Generic inference helpers ──────────────────────────────────

    fn hir_type_to_ty(
        arena: &mut ty::TyArena,
        unify: &mut unify::UnificationTable,
        param_vars: &std::collections::HashMap<glyim_interner::Symbol, ty::Ty>,
        hir: &HirType,
    ) -> ty::Ty {
        use ty::TyKind;
        match hir {
            HirType::Int => arena.alloc(TyKind::Int),
            HirType::Float => arena.alloc(TyKind::Float),
            HirType::Bool => arena.alloc(TyKind::Bool),
            HirType::Str => arena.alloc(TyKind::Str),
            HirType::Unit => arena.alloc(TyKind::Unit),
            HirType::Never => arena.alloc(TyKind::Never),
            HirType::Error => arena.alloc(TyKind::Error),
            HirType::Named(sym) => {
                if let Some(&var) = param_vars.get(sym) {
                    var
                } else {
                    arena.alloc(TyKind::Named(*sym))
                }
            }
            HirType::Generic(sym, args) => {
                let args: Vec<ty::Ty> = args
                    .iter()
                    .map(|a| TypeChecker::hir_type_to_ty(arena, unify, param_vars, a))
                    .collect();
                arena.alloc(TyKind::App(*sym, args))
            }
            HirType::RawPtr(inner) => {
                let inner = TypeChecker::hir_type_to_ty(arena, unify, param_vars, inner);
                arena.alloc(TyKind::RawPtr(inner))
            }
            _ => arena.alloc(TyKind::Error),
        }
    }

    fn unify_generics(
        &self,
        fn_def: &glyim_hir::HirFn,
        arg_types: &[HirType],
        call_expr_id: ExprId,
        call_span: glyim_diag::Span,
    ) -> Result<std::collections::HashMap<glyim_interner::Symbol, HirType>, TypeError> {
        use freeze::resolve_ty;
        use ty::TyArena;
        use unify::UnificationTable;
        let mut arena = TyArena::new();
        let mut unify_table = UnificationTable::with_interner(self.interner.clone());
        let mut param_vars = std::collections::HashMap::new();
        for tp in &fn_def.type_params {
            param_vars.insert(*tp, unify_table.new_var(&mut arena, call_span));
        }
        let params: Vec<ty::Ty> = fn_def
            .params
            .iter()
            .map(|(_, pty)| {
                TypeChecker::hir_type_to_ty(&mut arena, &mut unify_table, &param_vars, pty)
            })
            .collect();
        let args: Vec<ty::Ty> = arg_types
            .iter()
            .map(|a| {
                TypeChecker::hir_type_to_ty(
                    &mut arena,
                    &mut unify_table,
                    &std::collections::HashMap::new(),
                    a,
                )
            })
            .collect();
        for (i, (p, a)) in params.iter().zip(args.iter()).enumerate() {
            let mut errs = Vec::new();
            let _ = unify_table.unify(&mut arena, *p, *a, call_span, &mut |e| errs.push(e));
            if !errs.is_empty() {
                return Err(TypeError::MismatchedTypes {
                    expected: fn_def.params[i].1.clone(),
                    found: arg_types[i].clone(),
                    expr_id: call_expr_id,
                    span: (call_span.start, call_span.end),
                });
            }
        }
        let mut sub = std::collections::HashMap::new();
        for tp in &fn_def.type_params {
            let var = param_vars[tp];
            let resolved = unify_table.find(&arena, var);
            let hir_ty = resolve_ty(&arena, &unify_table, resolved);
            if hir_ty == HirType::Error {
                sub.insert(*tp, HirType::Named(*tp));
            } else {
                sub.insert(*tp, hir_ty);
            }
        }
        Ok(sub)
    }

    /// Check if two types are compatible (generic vs concrete with same base name).
    fn types_compatible(annotated: &HirType, inferred: &HirType) -> bool {
        use HirType::*;
        match (annotated, inferred) {
            (Named(a), Named(b)) => a == b,
            (Generic(a_sym, _), Named(b_sym)) => a_sym == b_sym,
            (Named(a_sym), Generic(b_sym, _)) => a_sym == b_sym,
            (Generic(a_sym, _), Generic(b_sym, _)) => a_sym == b_sym,
            _ => annotated == inferred,
        }
    }

    fn infer_struct_type_args(
        &self,
        struct_info: &StructInfo,
        field_value_types: &[(Symbol, HirType)],
        expr_id: ExprId,
        span: glyim_diag::Span,
    ) -> Result<HashMap<Symbol, HirType>, TypeError> {
        use freeze::resolve_ty;
        use ty::TyArena;
        use unify::UnificationTable;
        let mut arena = TyArena::new();
        let mut unify_table = UnificationTable::with_interner(self.interner.clone());
        let mut param_vars = HashMap::new();
        for tp in &struct_info.type_params {
            param_vars.insert(*tp, unify_table.new_var(&mut arena, span));
        }
        for (field_name, original_hir_ty) in field_value_types {
            if let Some(struct_field) = struct_info.fields.iter().find(|f| f.name == *field_name) {
                let declared_ty = TypeChecker::hir_type_to_ty(
                    &mut arena,
                    &mut unify_table,
                    &param_vars,
                    &struct_field.ty,
                );
                let value_ty = TypeChecker::hir_type_to_ty(
                    &mut arena,
                    &mut unify_table,
                    &HashMap::new(),
                    original_hir_ty,
                );
                let mut errs = Vec::new();
                let _ = unify_table.unify(&mut arena, declared_ty, value_ty, span, &mut |e| {
                    errs.push(e)
                });
                if !errs.is_empty() {
                    return Err(TypeError::MismatchedTypes {
                        expected: struct_field.ty.clone(),
                        found: original_hir_ty.clone(),
                        expr_id,
                        span: (span.start, span.end),
                    });
                }
            }
        }
        let mut sub = HashMap::new();
        for tp in &struct_info.type_params {
            let var = param_vars[tp];
            let resolved = unify_table.find(&arena, var);
            let hir_ty = resolve_ty(&arena, &unify_table, resolved);
            if hir_ty == HirType::Error {
                sub.insert(*tp, HirType::Named(*tp));
            } else {
                sub.insert(*tp, hir_ty);
            }
        }
        Ok(sub)
    }
}
