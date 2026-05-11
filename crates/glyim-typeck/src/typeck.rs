use crate::env::TypeEnv;
use crate::errors::TypeError;
use crate::symbols::KnownSymbols;
use crate::unify::UnificationTable;
use glyim_diag::Span;
use glyim_hir::index::HirIndex;
use glyim_hir::types::HirPattern as HirPat;
use glyim_hir::types::{HirType, TypeVar, substitute_type_safe as substitute_type};
use glyim_hir::{HirExpr, HirItem, HirStmt};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

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

    fn normalize_type(&self, ty: &HirType) -> HirType {
        match ty {
            HirType::Named(s) if *s == self.known.i64_type => HirType::Int,
            HirType::Named(s) if *s == self.known.f64_type => HirType::Float,
            HirType::Named(s) if *s == self.known.bool_type => HirType::Bool,
            HirType::Named(s) if *s == self.known.str_type => HirType::Str,
            HirType::Generic(s, args) => {
                let new_args = args.iter().map(|a| self.normalize_type(a)).collect();
                HirType::Generic(*s, new_args)
            }
            HirType::RawPtr(inner) => HirType::RawPtr(Box::new(self.normalize_type(inner))),
            HirType::Tuple(elems) => HirType::Tuple(elems.iter().map(|e| self.normalize_type(e)).collect()),
            HirType::Func(params, ret) => HirType::Func(
                params.iter().map(|p| self.normalize_type(p)).collect(),
                Box::new(self.normalize_type(ret))
            ),
            _ => ty.clone(),
        }
    }

    pub fn check(&mut self, hir: &glyim_hir::Hir) -> TypeCheckResult {
        self.seed_environment(hir);
        for item in &hir.items {
            match item {
                HirItem::Fn(f) => self.check_fn(f),
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        self.check_fn(m);
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
        // Build the HIR index for name resolution
        if let Ok(idx) = HirIndex::build(hir) {
            self.hir_index = Some(idx);
        }

        // Register built-in primitive types as global names
        self.env.insert_global(
            self.known.i64_type,
            HirType::Named(self.known.i64_type),
            false,
        );
        self.env.insert_global(
            self.known.bool_type,
            HirType::Named(self.known.bool_type),
            false,
        );
        self.env.insert_global(
            self.known.f64_type,
            HirType::Named(self.known.f64_type),
            false,
        );
        self.env.insert_global(
            self.known.str_type,
            HirType::Named(self.known.str_type),
            false,
        );
        self.env
            .insert_global(self.known.unit_type, HirType::Unit, false);

        // Register built-in intrinsics that the prelude extern block provides
        let i64_t = HirType::Named(self.known.i64_type);
        let ptr_u8 = HirType::RawPtr(Box::new(HirType::Named(self.interner.intern("u8"))));
        let void_t = HirType::Unit;

        // __ptr_offset(ptr: *mut u8, offset: i64) -> *mut u8
        let ptr_offset = self.interner.intern("__ptr_offset");
        self.env.insert_global(
            ptr_offset,
            HirType::Func(
                vec![ptr_u8.clone(), i64_t.clone()],
                Box::new(ptr_u8.clone()),
            ),
            false,
        );

        // __glyim_alloc(size: i64) -> *mut u8
        let alloc = self.interner.intern("__glyim_alloc");
        self.env.insert_global(
            alloc,
            HirType::Func(vec![i64_t.clone()], Box::new(ptr_u8.clone())),
            false,
        );

        // __glyim_free(ptr: *mut u8) -> ()
        let free = self.interner.intern("__glyim_free");
        self.env.insert_global(
            free,
            HirType::Func(vec![ptr_u8.clone()], Box::new(void_t.clone())),
            false,
        );

        // __glyim_hash_bytes(data: *const u8, len: i64) -> i64
        let hash_bytes = self.interner.intern("__glyim_hash_bytes");
        self.env.insert_global(
            hash_bytes,
            HirType::Func(vec![ptr_u8.clone(), i64_t.clone()], Box::new(i64_t.clone())),
            false,
        );

        // __glyim_hash_seed() -> i64
        let hash_seed = self.interner.intern("__glyim_hash_seed");
        self.env.insert_global(
            hash_seed,
            HirType::Func(vec![], Box::new(i64_t.clone())),
            false,
        );

        // abort() -> !
        let abort = self.interner.intern("abort");
        self.env.insert_global(
            abort,
            HirType::Func(vec![], Box::new(HirType::Never)),
            false,
        );

        // __size_of<T>() -> i64 (simplified)
        let sizeof = self.interner.intern("__size_of");
        self.env.insert_global(
            sizeof,
            HirType::Func(vec![], Box::new(i64_t.clone())),
            false,
        );

        // Pre‑seed fn_types_map with generic functions so that calls to
        // them are correctly recognised as generic even before their
        // bodies have been type‑checked.
        for item in &hir.items {
            if let HirItem::Fn(f) = item {
                if !f.type_params.is_empty() {
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
            }
        }
        // Register all HIR items in the global environment
        for item in &hir.items {
            match item {
                HirItem::Fn(f) => {
                    let param_tys: Vec<HirType> = f.params.iter().map(|(_, t)| t.clone()).collect();
                    let ret_ty = if let Some(r) = &f.ret {
                        r.clone()
                    } else {
                        HirType::Unit
                    };
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
                        let param_tys: Vec<HirType> = ef.params.to_vec();
                        let ret_ty = ef.ret.clone();
                        self.env.insert_global(
                            ef.name,
                            HirType::Func(param_tys, Box::new(ret_ty)),
                            false,
                        );
                    }
                }
                HirItem::Impl(imp) => {
                    // Register each impl method by its mangled name (already in HIR)
                    for m in &imp.methods {
                        let param_tys: Vec<HirType> =
                            m.params.iter().map(|(_, t)| t.clone()).collect();
                        let ret_ty = if let Some(r) = &m.ret {
                            r.clone()
                        } else {
                            HirType::Unit
                        };
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
        self.env.clear_locals();
        self.table.reset();
        self.expr_types.clear();
        self.call_type_args.clear();
        self.sizeof_types.clear();

        // Check if this function is a method on a non-generic struct
        let fn_name = self.interner.resolve(f.name).to_string();
        let is_method = fn_name.contains("_");
        let mut is_generic = !f.type_params.is_empty();

        // If it's a method and the struct has no type parameters, mark as non-generic
        if is_method && is_generic {
            // Extract struct name from method name (e.g., "Range_new" -> "Range")
            if let Some(struct_name) = fn_name.split('_').next() {
                let struct_sym = self.interner.intern(struct_name);
                if let Some(ref idx) = self.hir_index {
                    if let Some(si) = idx.find_struct(struct_sym) {
                        if si.type_params.is_empty() {
                            eprintln!(
                                "[FIX] Method {} on non-generic struct {} marked as non-generic",
                                fn_name, struct_name
                            );
                            is_generic = false;
                        }
                    }
                }
            }
        }
        let mut type_param_sub = HashMap::new();
        let mut type_param_map = HashMap::new();

        for tp in &f.type_params {
            let var = self.table.fresh_var(f.body.get_span());
            type_param_map.insert(var, *tp);
            type_param_sub.insert(*tp, HirType::Infer(var));
        }

        self.env.push_scope();
        for (i, (sym, ty)) in f.params.iter().enumerate() {
            let concrete = substitute_type(ty, &type_param_sub).unwrap_or(HirType::Error);
            self.env.insert(
                *sym,
                concrete,
                f.param_mutability.get(i).copied().unwrap_or(false),
            );
        }

        let concrete_ret = f
            .ret
            .as_ref()
            .map(|r| substitute_type(r, &type_param_sub).unwrap_or(HirType::Error));

        self.env.push_scope();
        let body_ty = self.infer_dispatch(&f.body, concrete_ret.as_ref());
        if let Some(expected) = &concrete_ret {
            self.unify_and_record(expected, &body_ty, f.body.get_span(), f.body.get_span());
        }
        self.env.pop_scope();
        self.env.pop_scope();

        self.finalize_fn(f, is_generic, &type_param_map);
    }

    fn freeze_ty(
        ty: HirType,
        tp_map: &HashMap<TypeVar, Symbol>,
        tbl: &mut UnificationTable,
    ) -> HirType {
        // First resolve through the unification table
        let resolved = match ty {
            HirType::Infer(_)
            | HirType::Generic(..)
            | HirType::Tuple(_)
            | HirType::RawPtr(_)
            | HirType::Func(..) => tbl.resolve(&ty).unwrap_or(ty),
            other => other,
        };
        match resolved {
            HirType::Infer(var) => {
                if let Some(&sym) = tp_map.get(&var) {
                    HirType::Param(sym)
                } else {
                    HirType::Error
                }
            }
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
        eprintln!("[DEBUG] finalize_fn for {:?}, is_generic={}", self.interner.resolve(f.name), is_generic);
        let mut new_expr = HashMap::new();
        for (&id, ty) in &self.expr_types {
            let frozen = Self::freeze_ty(ty.clone(), type_param_map, &mut self.table);
            eprintln!("[DEBUG] finalize_fn: expr_id {:?}, ty={:?} -> frozen={:?}", id, ty, frozen);
            new_expr.insert(id, frozen);
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
        let exp = self.normalize_type(expected);
        let found = self.normalize_type(found);
        match self.table.unify(&exp, &found, expected_span, found_span) {
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
            glyim_hir::types::HirPattern::Wild => {}
            glyim_hir::types::HirPattern::Var(name) => {
                self.env.insert(*name, ty.clone(), mutable);
            }
            glyim_hir::types::HirPattern::OptionSome(inner) => {
                // For `Some(x)`, bind `x` to the inner type of the Option
                if let HirType::Generic(_, type_args) = ty {
                    if let Some(inner_ty) = type_args.first() {
                        self.bind_pattern(inner, inner_ty, mutable);
                    }
                }
            }
            glyim_hir::types::HirPattern::OptionNone => {}
            glyim_hir::types::HirPattern::ResultOk(inner) => {
                if let HirType::Generic(_, type_args) = ty {
                    if let Some(inner_ty) = type_args.first() {
                        self.bind_pattern(inner, inner_ty, mutable);
                    }
                }
            }
            glyim_hir::types::HirPattern::ResultErr(inner) => {
                if let HirType::Generic(_, type_args) = ty {
                    if type_args.len() >= 2 {
                        self.bind_pattern(inner, &type_args[1], mutable);
                    }
                }
            }
            glyim_hir::types::HirPattern::Struct { bindings, .. } => {
                for (_, sub_pat) in bindings {
                    self.bind_pattern(sub_pat, ty, mutable);
                }
            }
            glyim_hir::types::HirPattern::Tuple { elements, .. } => {
                if let HirType::Tuple(types) = ty {
                    for (sub_pat, sub_ty) in elements.iter().zip(types.iter()) {
                        self.bind_pattern(sub_pat, sub_ty, mutable);
                    }
                }
            }
            glyim_hir::types::HirPattern::EnumVariant { bindings, .. } => {
                for (_, sub_pat) in bindings {
                    self.bind_pattern(sub_pat, ty, mutable);
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
                // Special case: 0 as struct type should return the struct type directly
                if let HirExpr::IntLit { value: 0, .. } = expr.as_ref() {
                    match target_type {
                        HirType::Named(sym) | HirType::Generic(sym, _) => {
                            if let Some(ref idx) = self.hir_index {
                                if idx.find_struct(*sym).is_some() || idx.find_enum(*sym).is_some()
                                {
                                    return target_type.clone();
                                }
                            }
                        }
                        _ => {}
                    }
                }
                self.infer_dispatch(expr, None);
                target_type.clone()
            }
            HirExpr::Deref { expr, span, .. } => self.infer_deref(expr, *span),
            HirExpr::Match {
                scrutinee,
                arms,
                span,
                ..
            } => self.infer_match(scrutinee, arms, expected, *span),
            HirExpr::While {
                condition,
                body,
                span,
                ..
            } => self.infer_while(condition, body, *span),
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
            HirExpr::TupleLit { elements, span, .. } => {
                self.infer_tuple_lit(elements, expected, *span)
            }
            _ => HirType::Error,
        };
        eprintln!("[DEBUG] infer_dispatch: expr_id {:?}, ty={:?}", expr.get_id(), ty);
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
            tt
        } else {
            self.unify_and_record(&HirType::Unit, &tt, span, then.get_span());
            HirType::Unit
        }
    }

    fn infer_call(
        &mut self,
        id: glyim_hir::types::ExprId,
        callee: &HirExpr,
        args: &[HirExpr],
        _exp: Option<&HirType>,
        span: Span,
    ) -> HirType {
        let at: Vec<HirType> = args.iter().map(|a| self.infer_dispatch(a, None)).collect();
        if let HirExpr::Ident { name, .. } = callee {
            if let Some(fty) = self.env.lookup(*name).cloned() {
                if let HirType::Func(params, ret) = fty {
                    let fn_types_opt = self.fn_types_map.get(name);
                    let is_generic = fn_types_opt.map(|ft| ft.is_generic).unwrap_or(false);

                    if is_generic {
                        // Substitute callee's type params with fresh Infer variables
                        let fn_type_params = fn_types_opt
                            .map(|ft| ft.type_params.clone())
                            .unwrap_or_default();
                        for tp in &fn_type_params {
                            let fresh = HirType::Infer(self.table.fresh_var(span));
                        }

                        // Build the formal parameter types: the solver will create its own
                        // fresh type variables, so pass the original types (still with type
                        // params), not our pre-substituted versions.
                        let formal_params: Vec<(Symbol, HirType)> = params
                            .iter()
                            .enumerate()
                            .map(|(i, ty)| {
                                let sym = self.interner.intern(&format!("_p{}", i));
                                (sym, ty.clone())
                            })
                            .collect();

                        // Run the generic solver — let it handle all fresh variable creation
                        let solve_result = crate::solve::solve_generic_params(
                            &mut self.table,
                            &fn_type_params,
                            &formal_params,
                            Some(&ret),
                            &at,
                            _exp,
                            span,
                            span,
                            &mut |e| {
                                self.errors.push(e);
                            },
                        );

                        // Record the discovered type arguments
                        if solve_result.fully_resolved && !solve_result.concrete_args.is_empty() {
                            self.record_call_type_args(id, solve_result.concrete_args);
                        }

                        // Apply the substitution to the return type
                        let ret_subst =
                            glyim_hir::types::substitute_type(&ret, &solve_result.subst);
                        return ret_subst;
                    }

                    // Non-generic: just unify
                    for (f, a) in params.iter().zip(at.iter()) {
                        self.unify_and_record(f, a, span, span);
                    }
                    return *ret;
                }
            }
            self.errors.push(TypeError::UnresolvedName {
                name: self.interner.resolve(*name).to_string(),
                span,
            });
            return HirType::Error;
        }
        let ct = self.infer_dispatch(callee, None);
        if let HirType::Func(params, ret) = ct {
            for (f, a) in params.iter().zip(at.iter()) {
                self.unify_and_record(f, a, span, span);
            }
            *ret
        } else {
            HirType::Error
        }
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
        _exp: Option<&HirType>,
        span: Span,
    ) -> HirType {
        let rt = self.infer_dispatch(recv, None);
        let at: Vec<HirType> = std::iter::once(rt.clone())
            .chain(args.iter().map(|a| self.infer_dispatch(a, None)))
            .collect();

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
        eprintln!("[TRACE] infer_method_call: rt={:?}", rt);

        let type_str = self.interner.resolve(type_name);
        let method_str = self.interner.resolve(meth);
        let mangled = self
            .interner
            .intern(&format!("{}_{}", type_str, method_str));

        // Look up mangled function in environment
        if let Some(fty_orig) = self.env.lookup(mangled).cloned() {
            let fty = self.normalize_type(&fty_orig);
            if let HirType::Func(params, ret) = fty {
                let is_generic = self
                    .fn_types_map
                    .get(&mangled)
                    .map(|ft| ft.is_generic)
                    .unwrap_or(false);

                if is_generic {
                    let type_params = self
                        .fn_types_map
                        .get(&mangled)
                        .map(|ft| ft.type_params.clone())
                        .unwrap_or_default();
                    // Let solve_generic_params create its own fresh variables.
                    // Pass the original param/return types (still with type params),
                    // not our pre-substituted versions.
                    let formal_params: Vec<(Symbol, HirType)> = params
                        .iter()
                        .enumerate()
                        .map(|(i, ty)| {
                            let sym = self.interner.intern(&format!("_p{}", i));
                            (sym, ty.clone())
                        })
                        .collect();
                    eprintln!("[DEBUG] infer_method_call: formal_params={:?}", formal_params);
                    eprintln!("[DEBUG] infer_method_call: ret={:?}", ret);
                    eprintln!("[DEBUG] infer_method_call: at (actual args)={:?}", at);
                    eprintln!("[DEBUG] infer_method_call: type_params={:?}", type_params);
                    let solve_result = crate::solve::solve_generic_params(
                        &mut self.table,
                        &type_params,
                        &formal_params,
                        Some(&ret),
                        &at,
                        _exp,
                        span,
                        span,
                        &mut |e| {
                            self.errors.push(e);
                        },
                    );
                    eprintln!("[DEBUG] solve_generic_params result:");
                    eprintln!("[DEBUG]   subst={:?}", solve_result.subst);
                    eprintln!("[DEBUG]   concrete_args={:?}", solve_result.concrete_args);
                    eprintln!("[DEBUG]   fully_resolved={:?}", solve_result.fully_resolved);
                    eprintln!("[DEBUG]   had_errors={:?}", solve_result.had_errors);
                    if solve_result.fully_resolved && !solve_result.concrete_args.is_empty() {
                        self.record_call_type_args(id, solve_result.concrete_args);
                    }
                    let ret_subst = glyim_hir::types::substitute_type(&ret, &solve_result.subst);
                    eprintln!("[DEBUG] ret_subst (after substitution)={:?}", ret_subst);
                    eprintln!("[DEBUG] infer_method_call: returning type for expr_id {:?} = {:?}", id, ret_subst);
                    return ret_subst;
                }

                // Non-generic
                for (f, a) in params.iter().zip(at.iter()) {
                    self.unify_and_record(f, a, span, span);
                }
                return *ret;
            }
        }

        // Not found
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

        // FIX: Early return for non-generic structs
        if let Some(ref idx) = self.hir_index {
            if let Some(si) = idx.find_struct(struct_name) {
                if si.type_params.is_empty() {
                    return HirType::Named(struct_name);
                }
            }
        }

        // Extract type param info before mutable borrow
        let generic_info = self.hir_index.as_ref().and_then(|idx| {
            idx.find_struct(struct_name).and_then(|si| {
                if !si.type_params.is_empty() {
                    let formal_field_types: Vec<HirType> =
                        si.fields.iter().map(|(_, t)| t.clone()).collect();
                    let actual_exprs: Vec<HirExpr> =
                        fields.iter().map(|(_, e)| e.clone()).collect();
                    Some((si.type_params.clone(), formal_field_types, actual_exprs))
                } else {
                    None
                }
            })
        });
        if let Some((type_params, formal_field_types, actual_exprs)) = generic_info {
            let type_args =
                self.infer_generic_args(&type_params, &formal_field_types, &actual_exprs, span);
            return HirType::Generic(struct_name, type_args);
        }
        // Not generic or unknown: return Named
        HirType::Named(struct_name)
    }

    fn infer_generic_args(
        &mut self,
        type_params: &[Symbol],
        formal_field_types: &[HirType],
        actual_exprs: &[HirExpr],
        span: Span,
    ) -> Vec<HirType> {
        eprintln!(
            "[TRACE] infer_generic_args called for type_params={:?}",
            type_params
        );
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
        // Extract type param info before mutable borrow
        let generic_info = self.hir_index.as_ref().and_then(|idx| {
            idx.find_enum(enum_name).and_then(|ei| {
                if !ei.type_params.is_empty() {
                    let variant_idx = ei.variant_map.get(&variant_name)?;
                    let variant = &ei.variants[*variant_idx];
                    let field_types: Vec<HirType> =
                        variant.fields.iter().map(|f| f.ty.clone()).collect();
                    Some((ei.type_params.clone(), field_types))
                } else {
                    None
                }
            })
        });
        if let Some((type_params, field_types)) = generic_info {
            let type_args = self.infer_generic_args(&type_params, &field_types, args, span);
            eprintln!(
                "[TRACE] Returning Generic for enum: {:?} with args {:?}",
                enum_name, type_args
            );
            if enum_name == self.known.option
                && variant_name == self.known.none
                && type_args.is_empty()
            {
                let tv = self.table.fresh_var(span);
                eprintln!("[FIX] Option::None created with fresh type variable");
                return HirType::Generic(enum_name, vec![HirType::Infer(tv)]);
            }
            return HirType::Generic(enum_name, type_args);
        }
        // Not generic: return Named if the enum exists, else Named fallback
        if self
            .hir_index
            .as_ref()
            .and_then(|idx| idx.find_enum(enum_name))
            .is_some()
        {
            HirType::Named(enum_name)
        } else {
            HirType::Named(enum_name)
        }
    }

    fn infer_field_access(&mut self, obj: &HirExpr, field: Symbol, span: Span) -> HirType {
        let obj_ty = self.infer_dispatch(obj, None);
        eprintln!("[DEBUG] infer_field_access: field={:?}, obj_ty={:?}", field, obj_ty);
        match &obj_ty {
            HirType::Named(s) | HirType::Generic(s, _) => {
                if let Some(ref idx) = self.hir_index {
                    if let Some(si) = idx.find_struct(*s) {
                        if let Some(&fi) = si.field_map.get(&field) {
                            if fi < si.fields.len() {
                                let field_ty = si.fields[fi].1.clone();
                                let norm_ty = self.normalize_type(&field_ty);
                                if let HirType::Generic(_, type_args) = &obj_ty
                                    && !type_args.is_empty()
                                    && !si.type_params.is_empty()
                                {
                                    let sub: std::collections::HashMap<_, _> = si
                                        .type_params
                                        .iter()
                                        .zip(type_args.iter())
                                        .map(|(&p, a)| (p, a.clone()))
                                        .collect();
                                    return glyim_hir::types::substitute_type(&norm_ty, &sub);
                                }
                                return norm_ty;
                            }
                        } else {
                            let resolved = self.interner.resolve(field).to_string();
                            let struct_name = self.interner.resolve(*s).to_string();
                            self.errors.push(TypeError::UnknownField {
                                struct_name,
                                field: resolved,
                                span: (span.start, span.end),
                            });
                            return HirType::Error;
                        }
                    }
                }
            }
            HirType::Tuple(elems) => {
                let field_name = self.interner.resolve(field);
                if let Some(idx) = field_name
                    .strip_prefix('_')
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    if idx < elems.len() {
                        return elems[idx].clone();
                    }
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

    fn infer_deref(&mut self, expr: &HirExpr, span: Span) -> HirType {
        let it = self.infer_dispatch(expr, None);
        match it {
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
        eprintln!("[FIX] infer_match: scrut_ty={:?}", scrut_ty);

        // For each arm, bind pattern variables
        let mut result_ty = None;
        for arm in arms {
            self.env.push_scope();
            self.bind_pattern(&arm.pattern, &scrut_ty, false);
            let arm_ty = self.infer_dispatch(&arm.body, None);
            eprintln!("[FIX] arm pattern={:?}, arm_ty={:?}", arm.pattern, arm_ty);
            self.env.pop_scope();
            if result_ty.is_none() {
                result_ty = Some(arm_ty);
            } else if let Some(ref first) = result_ty {
                self.unify_and_record(first, &arm_ty, span, span);
            }
        }
        result_ty.unwrap_or(HirType::Unit)
    }

    fn infer_while(&mut self, cond: &HirExpr, body: &HirExpr, span: Span) -> HirType {
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
        self.infer_dispatch(iter, None);
        // Bind the loop variable to a fresh type variable, which will be
        // constrained by usage inside the body.
        let item_ty = HirType::Infer(self.table.fresh_var(span));
        self.env.push_scope();
        self.bind_pattern(pat, &item_ty, false);
        let body_ty = self.infer_dispatch(body, None);
        self.env.pop_scope();
        HirType::Unit
    }

    fn infer_tuple_lit(
        &mut self,
        elems: &[HirExpr],
        _exp: Option<&HirType>,
        _span: Span,
    ) -> HirType {
        let et: Vec<HirType> = elems.iter().map(|e| self.infer_dispatch(e, None)).collect();
        HirType::Tuple(et)
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
                // Resolve through unification to get concrete type
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
                if let Some(_e) = self.env.lookup(*target).cloned() {
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
}
