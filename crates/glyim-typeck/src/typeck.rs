use crate::env::TypeEnv;
use crate::errors::TypeError;
use crate::symbols::KnownSymbols;
use crate::unify::UnificationTable;
use glyim_diag::Span;
use glyim_hir::index::HirIndex;
use glyim_hir::types::HirPattern as HirPat;
use glyim_hir::types::{HirType, TypeVar};
use glyim_hir::{HirExpr, HirItem, HirStmt};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

fn dump_expr(expr: &glyim_hir::HirExpr, depth: usize) {
    let indent = "  ".repeat(depth);
    match expr {
        glyim_hir::HirExpr::Unary { op, operand, span, .. } => {
            eprintln!("{}Unary op={:?} span={:?}", indent, op, span);
            dump_expr(operand, depth + 1);
        }
        glyim_hir::HirExpr::Ident { name, span, .. } => {
            eprintln!("{}Ident name={:?} span={:?}", indent, name, span);
        }
        glyim_hir::HirExpr::FieldAccess { object, field, span, .. } => {
            eprintln!("{}FieldAccess field={:?} span={:?}", indent, field, span);
            dump_expr(object, depth + 1);
        }
        glyim_hir::HirExpr::Binary { op, lhs, rhs, span, .. } => {
            eprintln!("{}Binary op={:?} span={:?}", indent, op, span);
            dump_expr(lhs, depth + 1);
            dump_expr(rhs, depth + 1);
        }
        glyim_hir::HirExpr::If { condition, then_branch, else_branch, span, .. } => {
            eprintln!("{}If span={:?}", indent, span);
            eprintln!("{}  condition:", indent);
            dump_expr(condition, depth + 2);
            eprintln!("{}  then:", indent);
            dump_expr(then_branch, depth + 2);
            if let Some(e) = else_branch {
                eprintln!("{}  else:", indent);
                dump_expr(e, depth + 2);
            }
        }
        _ => {
            eprintln!("{}{:?}", indent, expr);
        }
    }
}

#[derive(Debug, Clone)]
pub struct FnTypes {
    pub expr_types: HashMap<glyim_hir::types::ExprId, HirType>,
    pub call_type_args: HashMap<glyim_hir::types::ExprId, Vec<HirType>>,
    pub sizeof_types: HashMap<glyim_hir::types::ExprId, HirType>,
    pub is_generic: bool,
    pub type_params: Vec<Symbol>,
    pub span: Span,
}

impl Default for FnTypes {
    fn default() -> Self {
        Self {
            expr_types: HashMap::new(),
            call_type_args: HashMap::new(),
            sizeof_types: HashMap::new(),
            is_generic: false,
            type_params: Vec::new(),
            span: Span::new(0, 0),
        }
    }
}

pub struct TypeCheckOutput {
    pub expr_types: Vec<glyim_hir::types::HirType>,
    pub call_type_args:
        std::collections::HashMap<glyim_hir::types::ExprId, Vec<glyim_hir::types::HirType>>,
    pub interner: glyim_interner::Interner,
}

pub struct TypeCheckResult {
    pub fn_types_map: HashMap<Symbol, FnTypes>,
    pub type_errors: Vec<TypeError>,
}

impl TypeCheckResult {
    pub fn has_errors(&self) -> bool {
        !self.type_errors.is_empty()
    }
}

pub struct TypeChecker {
    pub interner: Interner,
    known: KnownSymbols,
    hir_index: Option<HirIndex>,
    env: TypeEnv,
    table: UnificationTable,
    expr_types: HashMap<glyim_hir::types::ExprId, HirType>,
    call_type_args: HashMap<glyim_hir::types::ExprId, Vec<HirType>>,
    sizeof_types: HashMap<glyim_hir::types::ExprId, HirType>,
    errors: Vec<TypeError>,
    fn_types_map: HashMap<Symbol, FnTypes>,
}

impl TypeChecker {
    pub fn new(interner: Interner, known: KnownSymbols) -> Self {
        Self {
            interner,
            known,
            env: TypeEnv::new(),
            table: UnificationTable::new(),
            hir_index: None,
            expr_types: HashMap::new(),
            call_type_args: HashMap::new(),
            sizeof_types: HashMap::new(),
            errors: Vec::new(),
            fn_types_map: HashMap::new(),
        }
    }

    pub fn check(&mut self, hir: &glyim_hir::Hir) -> TypeCheckResult {
        self.seed_environment(hir);
        // First pass: register all generic signatures so calls can resolve them
        for item in &hir.items {
            match item {
                HirItem::Fn(f) if !f.type_params.is_empty() => {
                    self.fn_types_map.insert(f.name, FnTypes {
                        expr_types: std::collections::HashMap::new(),
                        call_type_args: std::collections::HashMap::new(),
                        sizeof_types: std::collections::HashMap::new(),
                        is_generic: true,
                        type_params: f.type_params.clone(),
                        span: f.span,
                    });
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if !m.type_params.is_empty() {
                            self.fn_types_map.insert(m.name, FnTypes {
                                expr_types: std::collections::HashMap::new(),
                                call_type_args: std::collections::HashMap::new(),
                                sizeof_types: std::collections::HashMap::new(),
                                is_generic: true,
                                type_params: m.type_params.clone(),
                                span: m.span,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        // Pass 2a: check non‑generic functions first (so call_type_args are populated)
        for item in &hir.items {
            match item {
                HirItem::Fn(f) if f.type_params.is_empty() => self.check_fn(f),
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if m.type_params.is_empty() {
                            self.check_fn(m);
                        }
                    }
                }
                _ => {}
            }
        }
        // Pass 2b: now check generic functions – their call sites have been processed
        for item in &hir.items {
            match item {
                HirItem::Fn(f) if !f.type_params.is_empty() => self.check_fn(f),
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if !m.type_params.is_empty() {
                            self.check_fn(m);
                        }
                    }
                }
                _ => {}
            }
        }
        TypeCheckResult {
            fn_types_map: std::mem::take(&mut self.fn_types_map),
            type_errors: std::mem::take(&mut self.errors),
        }
    }

    fn seed_environment(&mut self, hir: &glyim_hir::Hir) {
        if let Ok(idx) = HirIndex::build(hir) {
            self.hir_index = Some(idx);
        }
        self.env
            .insert_global(self.known.i64_type, HirType::Int, false);
        self.env
            .insert_global(self.known.bool_type, HirType::Bool, false);
        self.env
            .insert_global(self.known.f64_type, HirType::Float, false);
        self.env
            .insert_global(self.known.str_type, HirType::Str, false);
        self.env
            .insert_global(self.known.unit_type, HirType::Unit, false);

        let i64_t = HirType::Int;
        let ptr_t = HirType::RawPtr(Box::new(HirType::Int));
        let void_t = HirType::Unit;

        // Built-in intrinsics
        for (name_s, param_tys, ret_ty) in [
            (
                "__ptr_offset",
                vec![ptr_t.clone(), i64_t.clone()],
                ptr_t.clone(),
            ),
            ("__glyim_alloc", vec![i64_t.clone()], ptr_t.clone()),
            ("__glyim_free", vec![ptr_t.clone()], void_t.clone()),
            (
                "__glyim_hash_bytes",
                vec![ptr_t.clone(), i64_t.clone()],
                i64_t.clone(),
            ),
            ("__glyim_hash_seed", vec![], i64_t.clone()),
            ("abort", vec![], HirType::Never),
            ("__size_of", vec![], i64_t.clone()),
        ] {
            let sym = self.interner.intern(name_s);
            self.env
                .insert_global(sym, HirType::Func(param_tys, Box::new(ret_ty)), false);
        }

        // Pre-seed generic fns
        for item in &hir.items {
            if let HirItem::Fn(f) = item
                && !f.type_params.is_empty()
            {
                self.fn_types_map.insert(
                    f.name,
                    FnTypes {
                        expr_types: HashMap::new(),
                        call_type_args: HashMap::new(),
                        sizeof_types: HashMap::new(),
                        is_generic: true,
                        type_params: f.type_params.clone(),
                        span: f.span,
                    },
                );
            }
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if !m.type_params.is_empty() {
                        self.fn_types_map.insert(
                            m.name,
                            FnTypes {
                                expr_types: HashMap::new(),
                                call_type_args: HashMap::new(),
                                sizeof_types: HashMap::new(),
                                is_generic: true,
                                type_params: m.type_params.clone(),
                                span: m.span,
                            },
                        );
                    }
                }
            }
        }

        // Register all HIR items (types already canonicalized by HIR lowering)
        for item in &hir.items {
            match item {
                HirItem::Fn(f) => {
                    let param_tys: Vec<HirType> = f.params.iter().map(|(_, t)| t.clone()).collect();
                    let ret_ty = f.ret.clone().unwrap_or(HirType::Unit);
                    self.env.insert_global(
                        f.name,
                        HirType::Func(param_tys, Box::new(ret_ty)),
                        false,
                    );
                }
                HirItem::Struct(s) => {
                    self.env
                        .insert_global(s.name, HirType::Named(s.name), false);
                }
                HirItem::Enum(e) => {
                    self.env
                        .insert_global(e.name, HirType::Named(e.name), false);
                }
                HirItem::Extern(ext) => {
                    for ef in &ext.functions {
                        let param_tys: Vec<HirType> = ef.params.clone();
                        let ret_ty = ef.ret.clone();
                        self.env.insert_global(
                            ef.name,
                            HirType::Func(param_tys, Box::new(ret_ty)),
                            false,
                        );
                    }
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        let param_tys: Vec<HirType> =
                            m.params.iter().map(|(_, t)| t.clone()).collect();
                        let ret_ty = m.ret.clone().unwrap_or(HirType::Unit);
                        self.env.insert_global(
                            m.name,
                            HirType::Func(param_tys, Box::new(ret_ty)),
                            false,
                        );
                    }
                }
            }
        }
    }

    fn check_fn(&mut self, f: &glyim_hir::HirFn) {
        let fn_name = self.interner.resolve(f.name).to_string();
        if fn_name == "Vec_push" {
            eprintln!("[DEBUG Vec_push FULL BODY]");
            dump_expr(&f.body, 0);
        }
        self.env.clear_locals();
        self.table.reset();
        self.expr_types.clear();
        self.call_type_args.clear();
        self.sizeof_types.clear();

        let is_generic = !f.type_params.is_empty();

        // For generic functions, register type parameters as Named types
        // so field accesses and method calls can resolve correctly.
        // e.g., for HashMap<K,V>, K and V become Named types in scope.
        for &tp in &f.type_params {
            self.env.insert_global(tp, HirType::Param(tp), false);
        }

        self.env.push_scope();
        for (i, (sym, ty)) in f.params.iter().enumerate() {
            // For generic impl methods, the first param (self) type uses
            // the impl's type params. Substitute them with Param types.
            let resolved_ty = if is_generic {
                let sub: std::collections::HashMap<_, _> = f.type_params.iter()
                    .map(|&tp| (tp, HirType::Param(tp)))
                    .collect();
                glyim_hir::types::substitute_type(ty, &sub)
            } else {
                ty.clone()
            };
            self.env.insert(
                *sym,
                resolved_ty,
                f.param_mutability.get(i).copied().unwrap_or(false),
            );
        }
        let concrete_ret = f.ret.clone();
        self.env.push_scope();
        let body_ty = self.infer_dispatch(&f.body, concrete_ret.as_ref());
        if let Some(expected) = &concrete_ret {
            self.unify_and_record(expected, &body_ty, f.body.get_span(), f.body.get_span());
        }
        self.env.pop_scope();
        self.env.pop_scope();
        let empty_map = std::collections::HashMap::new();
        self.finalize_fn(f, is_generic, &empty_map);
    }

    fn freeze_ty(
        ty: HirType,
        tp_map: &HashMap<TypeVar, Symbol>,
        tbl: &mut UnificationTable,
    ) -> HirType {
        let resolved = match ty {
            HirType::Infer(_)
            | HirType::Generic(..)
            | HirType::Tuple(_)
            | HirType::RawPtr(_)
            | HirType::Func(..) => tbl.resolve(&ty).unwrap_or(ty),
            other => other,
        };
        match resolved {
            HirType::Infer(var) => tp_map
                .get(&var)
                .map(|&sym| HirType::Param(sym))
                .unwrap_or(HirType::Error),
            HirType::Generic(s, a) => HirType::Generic(
                s,
                a.into_iter()
                    .map(|x| Self::freeze_ty(x, tp_map, tbl))
                    .collect(),
            ),
            HirType::Tuple(e) => HirType::Tuple(
                e.into_iter()
                    .map(|x| Self::freeze_ty(x, tp_map, tbl))
                    .collect(),
            ),
            HirType::RawPtr(i) => HirType::RawPtr(Box::new(Self::freeze_ty(*i, tp_map, tbl))),
            HirType::Func(p, r) => HirType::Func(
                p.into_iter()
                    .map(|x| Self::freeze_ty(x, tp_map, tbl))
                    .collect(),
                Box::new(Self::freeze_ty(*r, tp_map, tbl)),
            ),
            o => o,
        }
    }

    fn finalize_fn(
        &mut self,
        f: &glyim_hir::HirFn,
        is_generic: bool,
        type_param_map: &HashMap<TypeVar, Symbol>,
    ) {
        let mut new_expr = HashMap::new();
        for (&id, ty) in &self.expr_types {
            new_expr.insert(
                id,
                Self::freeze_ty(ty.clone(), type_param_map, &mut self.table),
            );
        }
        let mut new_call = HashMap::new();
        for (&id, args) in &self.call_type_args {
            new_call.insert(
                id,
                args.iter()
                    .map(|a| Self::freeze_ty(a.clone(), type_param_map, &mut self.table))
                    .collect(),
            );
        }
        let mut new_sizeof = HashMap::new();
        for (&id, ty) in &self.sizeof_types {
            new_sizeof.insert(
                id,
                Self::freeze_ty(ty.clone(), type_param_map, &mut self.table),
            );
        }
        self.fn_types_map.insert(
            f.name,
            FnTypes {
                expr_types: new_expr,
                call_type_args: new_call,
                sizeof_types: new_sizeof,
                is_generic,
                type_params: f.type_params.clone(),
                span: f.span,
            },
        );
    }

    fn unify_and_record(
        &mut self,
        expected: &HirType,
        found: &HirType,
        expected_span: Span,
        found_span: Span,
    ) -> bool {
        match self.table.unify(expected, found, expected_span, found_span) {
            Ok(_) => true,
            Err(e) => {
                self.errors.push(e.into_type_error());
                false
            }
        }
    }

    fn record_expr_type(&mut self, id: glyim_hir::types::ExprId, ty: HirType) {
        self.expr_types.insert(id, ty);
    }
    fn record_call_type_args(&mut self, id: glyim_hir::types::ExprId, args: Vec<HirType>) {
        if !args.is_empty() {
            self.call_type_args.insert(id, args);
        }
    }

    fn bind_pattern(&mut self, pat: &HirPat, ty: &HirType, mutable: bool) {
        match pat {
            HirPat::Wild => {}
            HirPat::Var(name) => {
                self.env.insert(*name, ty.clone(), mutable);
            }
            HirPat::OptionSome(inner) => {
                if let HirType::Generic(_, type_args) = ty
                    && let Some(inner_ty) = type_args.first()
                {
                    self.bind_pattern(inner, inner_ty, mutable);
                }
            }
            HirPat::OptionNone => {}
            HirPat::ResultOk(inner) => {
                if let HirType::Generic(_, type_args) = ty
                    && let Some(inner_ty) = type_args.first()
                {
                    self.bind_pattern(inner, inner_ty, mutable);
                }
            }
            HirPat::ResultErr(inner) => {
                if let HirType::Generic(_, type_args) = ty
                    && type_args.len() >= 2
                {
                    self.bind_pattern(inner, &type_args[1], mutable);
                }
            }
            HirPat::Struct { name, bindings, .. } => {
                let field_types: Vec<Option<HirType>> = if let HirType::Named(_)
                | HirType::Generic(_, _) = ty
                    && let Some(ref idx) = self.hir_index
                    && let Some(si) = idx.find_struct(*name)
                {
                    let generic_sub: Option<HashMap<Symbol, HirType>> =
                        if let HirType::Generic(_, type_args) = ty
                            && !type_args.is_empty()
                            && !si.type_params.is_empty()
                        {
                            Some(
                                si.type_params
                                    .iter()
                                    .zip(type_args.iter())
                                    .map(|(&p, a)| (p, a.clone()))
                                    .collect(),
                            )
                        } else {
                            None
                        };
                    bindings
                        .iter()
                        .map(|(field_name, _)| {
                            si.field_map.get(field_name).and_then(|&fi| {
                                if fi < si.fields.len() {
                                    let field_ty = si.fields[fi].1.clone();
                                    if let Some(ref sub) = generic_sub {
                                        Some(glyim_hir::types::substitute_type(&field_ty, sub))
                                    } else {
                                        Some(field_ty)
                                    }
                                } else {
                                    None
                                }
                            })
                        })
                        .collect()
                } else {
                    vec![]
                };
                for (i, (_, sub_pat)) in bindings.iter().enumerate() {
                    if let Some(Some(field_ty)) = field_types.get(i) {
                        self.bind_pattern(sub_pat, field_ty, mutable);
                    } else {
                        self.bind_pattern(sub_pat, ty, mutable);
                    }
                }
            }
            HirPat::Tuple { elements, .. } => {
                if let HirType::Tuple(types) = ty {
                    for (sub_pat, sub_ty) in elements.iter().zip(types.iter()) {
                        self.bind_pattern(sub_pat, sub_ty, mutable);
                    }
                }
            }
            HirPat::EnumVariant {
                enum_name,
                variant_name,
                bindings,
                ..
            } => {
                let field_types: Vec<HirType> = if let Some(ref idx) = self.hir_index
                    && let Some(ei) = idx.find_enum(*enum_name)
                    && let Some(&variant_idx) = ei.variant_map.get(variant_name)
                    && variant_idx < ei.variants.len()
                {
                    let variant = &ei.variants[variant_idx];
                    let generic_sub: Option<HashMap<Symbol, HirType>> =
                        if let HirType::Generic(_, type_args) = ty
                            && !type_args.is_empty()
                            && !ei.type_params.is_empty()
                        {
                            Some(
                                ei.type_params
                                    .iter()
                                    .zip(type_args.iter())
                                    .map(|(&p, a)| (p, a.clone()))
                                    .collect(),
                            )
                        } else {
                            None
                        };
                    variant
                        .fields
                        .iter()
                        .map(|f| {
                            if let Some(ref sub) = generic_sub {
                                glyim_hir::types::substitute_type(&f.ty, sub)
                            } else {
                                f.ty.clone()
                            }
                        })
                        .collect()
                } else {
                    vec![]
                };
                for (i, (_, sub_pat)) in bindings.iter().enumerate() {
                    if i < field_types.len() {
                        self.bind_pattern(sub_pat, &field_types[i], mutable);
                    } else {
                        self.bind_pattern(sub_pat, ty, mutable);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn infer_dispatch(&mut self, expr: &HirExpr, expected: Option<&HirType>) -> HirType {
        let ty = match expr {
            HirExpr::IntLit { .. } => HirType::Int,
            HirExpr::FloatLit { .. } => HirType::Float,
            HirExpr::BoolLit { .. } => HirType::Bool,
            HirExpr::StrLit { .. } => HirType::Str,
            HirExpr::UnitLit { .. } => HirType::Unit,
            HirExpr::AddrOf { .. } => HirType::RawPtr(Box::new(HirType::Int)),
            HirExpr::Ident { name, span, .. } => {
                self.env.lookup(*name).cloned().unwrap_or_else(|| {
                    self.errors.push(TypeError::UnresolvedName {
                        name: self.interner.resolve(*name).to_string(),
                        span: *span,
                    });
                    HirType::Error
                })
            }
            HirExpr::Return { value, .. } => {
                if let Some(v) = value {
                    self.infer_dispatch(v, expected);
                }
                HirType::Never
            }
            HirExpr::Block { stmts, .. } => self.infer_block(stmts),
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                span,
                ..
            } => self.infer_if(condition, then_branch, else_branch, expected, *span),
            HirExpr::Call {
                id,
                callee,
                args,
                span,
                ..
            } => self.infer_call(*id, callee, args, expected, *span),
            HirExpr::Binary {
                op, lhs, rhs, span, ..
            } => self.infer_binary(*op, lhs, rhs, *span),
            HirExpr::Unary {
                op, operand, span, ..
            } => self.infer_unary(*op, operand, *span),
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                span,
                ..
            } => self.infer_method_call(*id, receiver, *method_name, args, expected, *span),
            HirExpr::StructLit {
                struct_name,
                fields,
                span,
                ..
            } => self.infer_struct_lit(*struct_name, fields, *span),
            HirExpr::EnumVariant {
                enum_name,
                variant_name,
                args,
                span,
                ..
            } => self.infer_enum_variant(*enum_name, *variant_name, args, *span),
            HirExpr::FieldAccess {
                object,
                field,
                span,
                ..
            } => self.infer_field_access(object, *field, *span),
            HirExpr::As {
                expr, target_type, ..
            } => {
                if let HirExpr::IntLit { value: 0, .. } = expr.as_ref() {
                    // Allow `0 as T` for any type: structs, enums, and type parameters.
                    // This is the zero value for all types in Glyim's runtime representation.
                    match target_type {
                        HirType::Named(sym) | HirType::Generic(sym, _) => {
                            if let Some(ref idx) = self.hir_index
                                && (idx.find_struct(*sym).is_some() || idx.find_enum(*sym).is_some())
                            {
                                return target_type.clone();
                            }
                        }
                        HirType::Param(_) => {
                            // `0 as K` where K is a generic type parameter - allow it
                            return target_type.clone();
                        }
                        _ => {}
                    }
                }
                let src_ty = self.infer_dispatch(expr, None);
                if !self.is_valid_cast(&src_ty, target_type) {
                    self.errors.push(TypeError::MismatchedTypes {
                        expected: Box::new(target_type.clone()),
                        found: Box::new(src_ty),
                        expected_span: expr.get_span(),
                        found_span: expr.get_span(),
                    });
                }
                target_type.clone()
            }
            HirExpr::Deref { expr, .. } => self.infer_deref(expr),
            HirExpr::Match {
                scrutinee,
                arms,
                span,
                ..
            } => self.infer_match(scrutinee, arms, expected, *span),
            HirExpr::While {
                condition, body, ..
            } => self.infer_while(condition, body),
            HirExpr::ForIn {
                pattern,
                iter,
                body,
                span,
                ..
            } => self.infer_for_in(pattern, iter, body, *span),
            HirExpr::SizeOf {
                id, target_type, ..
            } => {
                self.sizeof_types.insert(*id, target_type.clone());
                HirType::Int
            }
            HirExpr::TupleLit { elements, .. } => self.infer_tuple_lit(elements),
            _ => HirType::Error,
        };
        self.record_expr_type(expr.get_id(), ty.clone());
        ty
    }

    fn infer_block(&mut self, stmts: &[HirStmt]) -> HirType {
        self.env.push_scope();
        let mut last = HirType::Unit;
        for s in stmts {
            last = self.infer_stmt(s);
        }
        self.env.pop_scope();
        last
    }

    fn infer_if(
        &mut self,
        cond: &HirExpr,
        then: &HirExpr,
        els: &Option<Box<HirExpr>>,
        exp: Option<&HirType>,
        span: Span,
    ) -> HirType {
        let ct = self.infer_dispatch(cond, None);
        self.unify_and_record(&HirType::Bool, &ct, span, cond.get_span());
        let tt = self.infer_dispatch(then, exp);
        if let Some(e) = els {
            let et = self.infer_dispatch(e, exp);
            self.unify_and_record(&tt, &et, then.get_span(), e.get_span());
        }
        tt
    }

    fn infer_call(
        &mut self,
        id: glyim_hir::types::ExprId,
        callee: &HirExpr,
        args: &[HirExpr],
        exp: Option<&HirType>,
        span: Span,
    ) -> HirType {
        let at: Vec<HirType> = args.iter().map(|a| self.infer_dispatch(a, None)).collect();
        if let HirExpr::Ident { name, .. } = callee
            && let Some(fty) = self.env.lookup(*name).cloned()
            && let HirType::Func(params, ret) = fty
        {
            let is_generic = self
                .fn_types_map
                .get(name)
                .map(|ft| ft.is_generic)
                .unwrap_or(false);
            if std::env::var("TYPE_VERBOSE").is_ok() {
                eprintln!("[typeck debug] infer_call id={:?} callee={:?} is_generic={} type_params={:?}",
                    id, self.interner.resolve(*name), is_generic,
                    self.fn_types_map.get(name).map(|ft| &ft.type_params));
                eprintln!("  arg types: {:?}", at);
                eprintln!("  expected: {:?}", exp);
                eprintln!("  formal params: {:?}", params);
            }
            if is_generic {
                let fn_type_params = self
                    .fn_types_map
                    .get(name)
                    .map(|ft| ft.type_params.clone())
                    .unwrap_or_default();
                if at.is_empty()
                    && params.is_empty()
                    && let Some(expected) = exp
                {
                    let mut fresh_vars = Vec::new();
                    let ret_subst = glyim_hir::types::substitute_type(
                        &ret,
                        &fn_type_params
                            .iter()
                            .map(|tp| {
                                let fresh = self.table.fresh_var(span);
                                fresh_vars.push((*tp, fresh));
                                (*tp, HirType::Infer(fresh))
                            })
                            .collect(),
                    );
                    self.table.unify(&ret_subst, expected, span, span).ok();
                    let resolved_args: Vec<_> = fresh_vars
                        .iter()
                        .map(|(_, var)| {
                            self.table
                                .resolve(&HirType::Infer(*var))
                                .unwrap_or(HirType::Error)
                        })
                        .collect();
                    if !resolved_args.is_empty() {
                        self.record_call_type_args(id, resolved_args);
                    }
                    return expected.clone();
                }
                let formal_params: Vec<(Symbol, HirType)> = params
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| (self.interner.intern(&format!("_p{}", i)), ty.clone()))
                    .collect();
                let solve_result = crate::solve::solve_generic_params(
                    &mut self.table,
                    &self.interner,
                    &self.known,
                    &fn_type_params,
                    &formal_params,
                    Some(&ret),
                    &at,
                    exp,
                    span,
                    span,
                    &mut |e| {
                        self.errors.push(e);
                    },
                );
                if solve_result.fully_resolved && !solve_result.concrete_args.is_empty() {
                    self.record_call_type_args(id, solve_result.concrete_args);
                }
                let ret_subst = glyim_hir::types::substitute_type(&ret, &solve_result.subst);
                if let Some(ref exp) = exp {
                    self.unify_and_record(exp, &ret_subst, span, span);
                }
                return ret_subst;
            }
            for (f, a) in params.iter().zip(at.iter()) {
                self.unify_and_record(f, a, span, span);
            }
            if let Some(ref exp) = exp {
                self.unify_and_record(exp, &ret, span, span);
            }
            return *ret;
        }
        HirType::Error
    }

    fn infer_binary(
        &mut self,
        op: glyim_hir::HirBinOp,
        lhs: &HirExpr,
        rhs: &HirExpr,
        span: Span,
    ) -> HirType {
        let lt = self.infer_dispatch(lhs, None);
        let rt = self.infer_dispatch(rhs, None);
        match op {
            glyim_hir::HirBinOp::Eq
            | glyim_hir::HirBinOp::Neq
            | glyim_hir::HirBinOp::Lt
            | glyim_hir::HirBinOp::Gt
            | glyim_hir::HirBinOp::Lte
            | glyim_hir::HirBinOp::Gte => {
                self.unify_and_record(&lt, &rt, span, span);
                HirType::Bool
            }
            glyim_hir::HirBinOp::And | glyim_hir::HirBinOp::Or => {
                self.unify_and_record(&HirType::Bool, &lt, span, span);
                self.unify_and_record(&HirType::Bool, &rt, span, span);
                HirType::Bool
            }
            _ => {
                self.unify_and_record(&lt, &rt, span, span);
                lt
            }
        }
    }

    fn infer_unary(&mut self, op: glyim_hir::HirUnOp, operand: &HirExpr, span: Span) -> HirType {
        let ot = self.infer_dispatch(operand, None);
        match op {
            glyim_hir::HirUnOp::Not => {
                self.unify_and_record(&HirType::Bool, &ot, span, span);
                HirType::Bool
            }
            glyim_hir::HirUnOp::Neg => ot,
        }
    }

    fn infer_method_call(
        &mut self,
        id: glyim_hir::types::ExprId,
        recv: &HirExpr,
        meth: Symbol,
        args: &[HirExpr],
        exp: Option<&HirType>,
        span: Span,
    ) -> HirType {
        let rt = self.infer_dispatch(recv, None);
        eprintln!("[TYPECK] infer_method_call: self type={:?}, method={}",
            rt, self.interner.resolve(meth));
        if std::env::var("TYPE_VERBOSE").is_ok() {
            eprintln!("[typeck debug] infer_method_call id={:?} receiver={:?} method={:?}",
                id, rt, self.interner.resolve(meth));
            eprintln!("  expected: {:?}", exp);
        }
        let type_name = match &rt {
            HirType::Named(s) | HirType::Generic(s, _) => *s,
            HirType::RawPtr(inner) => match inner.as_ref() {
                HirType::Named(s) | HirType::Generic(s, _) => *s,
                _ => {
                    self.errors.push(TypeError::UnresolvedMethod {
                        method_name: meth,
                        receiver_type: Box::new(rt.clone()),
                        span,
                    });
                    return HirType::Error;
                }
            },
            _ => {
                self.errors.push(TypeError::UnresolvedMethod {
                    method_name: meth,
                    receiver_type: Box::new(rt.clone()),
                    span,
                });
                return HirType::Error;
            }
        };
        let mangled_sym = self.interner.intern(&format!(
            "{}_{}",
            self.interner.resolve(type_name),
            self.interner.resolve(meth)
        ));
        let has_self_param = self
            .env
            .lookup(mangled_sym)
            .map(|fty| matches!(fty, HirType::Func(params, _) if !params.is_empty()))
            .unwrap_or(false);
        let at: Vec<HirType> = if has_self_param {
            std::iter::once(rt.clone())
                .chain(args.iter().map(|a| self.infer_dispatch(a, None)))
                .collect()
        } else {
            args.iter().map(|a| self.infer_dispatch(a, None)).collect()
        };
        if let Some(fty_orig) = self.env.lookup(mangled_sym).cloned()
            && let HirType::Func(params, ret) = fty_orig
        {
            let is_generic = self
                .fn_types_map
                .get(&mangled_sym)
                .map(|ft| ft.is_generic)
                .unwrap_or(false);
            if is_generic {
                let type_params = self
                    .fn_types_map
                    .get(&mangled_sym)
                    .map(|ft| ft.type_params.clone())
                    .unwrap_or_default();
                let formal_params: Vec<(Symbol, HirType)> = params
                    .iter()
                    .enumerate()
                    .map(|(i, ty)| (self.interner.intern(&format!("_p{}", i)), ty.clone()))
                    .collect();
                let solve_result = crate::solve::solve_generic_params(
                    &mut self.table,
                    &self.interner,
                    &self.known,
                    &type_params,
                    &formal_params,
                    Some(&ret),
                    &at,
                    exp,
                    span,
                    span,
                    &mut |e| {
                        self.errors.push(e);
                    },
                );
                if solve_result.fully_resolved && !solve_result.concrete_args.is_empty() {
                    self.record_call_type_args(id, solve_result.concrete_args);
                }
                let ret_subst = glyim_hir::types::substitute_type(&ret, &solve_result.subst);
                if let Some(ref exp) = exp {
                    self.unify_and_record(exp, &ret_subst, span, span);
                }
                return ret_subst;
            }
            for (f, a) in params.iter().zip(at.iter()) {
                self.unify_and_record(f, a, span, span);
            }
            if let Some(ref exp) = exp {
                self.unify_and_record(exp, &ret, span, span);
            }
            return *ret;
        }
        self.errors.push(TypeError::UnresolvedMethod {
            method_name: meth,
            receiver_type: Box::new(rt.clone()),
            span,
        });
        HirType::Error
    }

    fn infer_struct_lit(
        &mut self,
        struct_name: Symbol,
        fields: &[(Symbol, HirExpr)],
        span: Span,
    ) -> HirType {
        for (_, f_expr) in fields {
            self.infer_dispatch(f_expr, None);
        }
        if let Some(ref idx) = self.hir_index
            && let Some(si) = idx.find_struct(struct_name)
            && si.type_params.is_empty()
        {
            return HirType::Named(struct_name);
        }
        let generic_info = self.hir_index.as_ref().and_then(|idx| {
            idx.find_struct(struct_name).and_then(|si| {
                if !si.type_params.is_empty() {
                    Some((
                        si.type_params.clone(),
                        si.fields.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
                        fields.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>(),
                    ))
                } else {
                    None
                }
            })
        });
        if let Some((type_params, formal_field_types, actual_exprs)) = generic_info {
            return HirType::Generic(
                struct_name,
                self.infer_generic_args(&type_params, &formal_field_types, &actual_exprs, span),
            );
        }
        HirType::Named(struct_name)
    }

    fn infer_generic_args(
        &mut self,
        type_params: &[Symbol],
        formal_field_types: &[HirType],
        actual_exprs: &[HirExpr],
        span: Span,
    ) -> Vec<HirType> {
        let actual_types: Vec<HirType> = actual_exprs
            .iter()
            .map(|a| self.infer_dispatch(a, None))
            .collect();
        let params: Vec<(Symbol, HirType)> = formal_field_types
            .iter()
            .enumerate()
            .map(|(i, ty)| (self.interner.intern(&format!("_f{}", i)), ty.clone()))
            .collect();
        let solve_result = crate::solve::solve_generic_params(
            &mut self.table,
            &self.interner,
            &self.known,
            type_params,
            &params,
            None,
            &actual_types,
            None,
            span,
            span,
            &mut |e| self.errors.push(e),
        );
        if !solve_result.had_errors {
            solve_result.concrete_args
        } else {
            type_params
                .iter()
                .map(|_| HirType::Infer(self.table.fresh_var(span)))
                .collect()
        }
    }

    fn infer_enum_variant(
        &mut self,
        enum_name: Symbol,
        variant_name: Symbol,
        args: &[HirExpr],
        span: Span,
    ) -> HirType {
        let generic_info = self.hir_index.as_ref().and_then(|idx| {
            idx.find_enum(enum_name).and_then(|ei| {
                if !ei.type_params.is_empty()
                    && let Some(&variant_idx) = ei.variant_map.get(&variant_name)
                    && variant_idx < ei.variants.len()
                {
                    Some((
                        ei.type_params.clone(),
                        ei.variants[variant_idx]
                            .fields
                            .iter()
                            .map(|f| f.ty.clone())
                            .collect::<Vec<_>>(),
                    ))
                } else {
                    None
                }
            })
        });
        if let Some((type_params, field_types)) = generic_info {
            let type_args = self.infer_generic_args(&type_params, &field_types, args, span);
            if enum_name == self.known.option
                && variant_name == self.known.none
                && type_args.is_empty()
            {
                return HirType::Generic(
                    enum_name,
                    vec![HirType::Infer(self.table.fresh_var(span))],
                );
            }
            return HirType::Generic(enum_name, type_args);
        }
        HirType::Named(enum_name)
    }

    fn infer_field_access(&mut self, obj: &HirExpr, field: Symbol, span: Span) -> HirType {
        let obj_ty = self.infer_dispatch(obj, None);
        match &obj_ty {
            HirType::Named(s) | HirType::Generic(s, _) => {
                if let Some(ref idx) = self.hir_index
                    && let Some(si) = idx.find_struct(*s)
                    && let Some(&fi) = si.field_map.get(&field)
                    && fi < si.fields.len()
                {
                    let field_ty = si.fields[fi].1.clone();
                    if let HirType::Generic(_, type_args) = &obj_ty
                        && !type_args.is_empty()
                        && !si.type_params.is_empty()
                    {
                        let sub: HashMap<_, _> = si
                            .type_params
                            .iter()
                            .zip(type_args.iter())
                            .map(|(&p, a)| (p, a.clone()))
                            .collect();
                        return glyim_hir::types::substitute_type(&field_ty, &sub);
                    }
                    return field_ty;
                }
                let resolved = self.interner.resolve(field).to_string();
                self.errors.push(TypeError::UnknownField {
                    struct_name: self.interner.resolve(*s).to_string(),
                    field: resolved,
                    span,
                });
                return HirType::Error;
            }
            HirType::Tuple(elems) => {
                let field_name = self.interner.resolve(field);
                if let Some(idx) = field_name
                    .strip_prefix('_')
                    .and_then(|s| s.parse::<usize>().ok())
                    && idx < elems.len()
                {
                    return elems[idx].clone();
                }
            }
            _ => {}
        }
        let resolved = self.interner.resolve(field).to_string();
        self.errors.push(TypeError::UnresolvedName {
            name: resolved,
            span,
        });
        HirType::Error
    }

    fn infer_deref(&mut self, expr: &HirExpr) -> HirType {
        match self.infer_dispatch(expr, None) {
            HirType::RawPtr(i) => *i,
            _ => HirType::Error,
        }
    }

    fn infer_match(
        &mut self,
        scrut: &HirExpr,
        arms: &[glyim_hir::MatchArm],
        _exp: Option<&HirType>,
        span: Span,
    ) -> HirType {
        let scrut_ty = self.infer_dispatch(scrut, None);
        let resolved_scrut = self
            .table
            .resolve(&scrut_ty)
            .unwrap_or_else(|_| scrut_ty.clone());
        let mut result_ty = None;
        for arm in arms {
            self.env.push_scope();
            self.bind_pattern(&arm.pattern, &resolved_scrut, false);
            let arm_ty = self.infer_dispatch(&arm.body, None);
            let resolved_arm = self
                .table
                .resolve(&arm_ty)
                .unwrap_or_else(|_| arm_ty.clone());
            self.env.pop_scope();
            if result_ty.is_none() {
                result_ty = Some(resolved_arm);
            } else if let Some(ref first) = result_ty {
                self.unify_and_record(first, &resolved_arm, span, span);
            }
        }
        self.table
            .resolve(&result_ty.unwrap_or(HirType::Unit))
            .unwrap_or(HirType::Unit)
    }

    fn infer_while(&mut self, cond: &HirExpr, body: &HirExpr) -> HirType {
        self.infer_dispatch(cond, None);
        self.infer_dispatch(body, None);
        HirType::Unit
    }

    fn infer_for_in(
        &mut self,
        pat: &HirPat,
        iter: &HirExpr,
        body: &HirExpr,
        span: Span,
    ) -> HirType {
        let _ = self.infer_dispatch(iter, None);
        let item_ty = HirType::Infer(self.table.fresh_var(span));
        self.env.push_scope();
        self.bind_pattern(pat, &item_ty, false);
        self.infer_dispatch(body, None);
        self.env.pop_scope();
        HirType::Unit
    }

    fn infer_tuple_lit(&mut self, elems: &[HirExpr]) -> HirType {
        HirType::Tuple(elems.iter().map(|e| self.infer_dispatch(e, None)).collect())
    }

    fn infer_stmt(&mut self, stmt: &HirStmt) -> HirType {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                ..
            } => {
                let t = self.infer_dispatch(value, None);
                self.env.insert(*name, t, *mutable);
                HirType::Unit
            }
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                ty,
                ..
            } => {
                let expected = ty.as_ref();
                let mut t = self.infer_dispatch(value, expected);
                if let Some(ann_ty) = expected {
                    self.unify_and_record(ann_ty, &t, value.get_span(), value.get_span());
                }
                if let Ok(resolved) = self.table.resolve(&t) {
                    t = resolved;
                }
                self.bind_pattern(pattern, &t, *mutable);
                HirType::Unit
            }
            HirStmt::Assign {
                target,
                value,
                span,
                ..
            } => {
                if !self.env.is_mutable(*target) {
                    self.errors.push(TypeError::AssignToImmutable {
                        name: self.interner.resolve(*target).to_string(),
                        expr_id: value.get_id(),
                        span: *span,
                    });
                }
                if self.env.lookup(*target).is_some() {
                    self.infer_dispatch(value, None);
                } else {
                    self.errors.push(TypeError::UnresolvedName {
                        name: self.interner.resolve(*target).to_string(),
                        span: *span,
                    });
                }
                HirType::Unit
            }
            HirStmt::Expr(expr) => self.infer_dispatch(expr, None),
            _ => HirType::Unit,
        }
    }

    fn is_valid_cast(&self, src: &HirType, target: &HirType) -> bool {
        use HirType::*;
        matches!(
            (src, target),
            (Int, Int)
                | (Int, Float)
                | (Float, Float)
                | (Float, Int)
                | (Int, Bool)
                | (Float, Bool)
                | (Int, RawPtr(_))
                | (Float, RawPtr(_))
                | (RawPtr(_), Int)
                | (RawPtr(_), Float)
                | (RawPtr(_), RawPtr(_))
                | (Bool, Int)
                | (Bool, Float)
                | (Named(_), Named(_))
                | (Named(_), RawPtr(_))
                | (Generic(_, _), Generic(_, _))
                | (Generic(_, _), RawPtr(_))
                | (RawPtr(_), Generic(_, _))
                | (Int, Named(_))
                | (Float, Named(_))
                | (RawPtr(_), Named(_))
                | (Str, Str)
                | (RawPtr(_), Str)
                | (Str, RawPtr(_))
        )
    }
}
