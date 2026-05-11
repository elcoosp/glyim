use crate::env::TypeEnv;
use crate::errors::TypeError;
use crate::symbols::KnownSymbols;
use crate::unify::UnificationTable;
use glyim_hir::types::{substitute_type_safe as substitute_type, HirType, TypeVar};
use glyim_diag::Span;
use glyim_hir::{HirExpr, HirStmt, HirItem};
use glyim_hir::types::HirPattern as HirPat;
use glyim_hir::index::{HirIndex, StructInfo, EnumInfo, FnInfo};
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
    pub call_type_args: std::collections::HashMap<glyim_hir::types::ExprId, Vec<glyim_hir::types::HirType>>,
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
            interner, known, env: TypeEnv::new(), table: UnificationTable::new(), hir_index: None,
            expr_types: HashMap::new(), call_type_args: HashMap::new(),
            sizeof_types: HashMap::new(), errors: Vec::new(), fn_types_map: HashMap::new(),
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
        self.env.insert_global(self.known.i64_type, HirType::Named(self.known.i64_type), false);
        self.env.insert_global(self.known.bool_type, HirType::Named(self.known.bool_type), false);
        self.env.insert_global(self.known.f64_type, HirType::Named(self.known.f64_type), false);
        self.env.insert_global(self.known.str_type, HirType::Named(self.known.str_type), false);
        self.env.insert_global(self.known.unit_type, HirType::Unit, false);

        // Register built-in intrinsics that the prelude extern block provides
        let i64_t = HirType::Named(self.known.i64_type);
        let ptr_u8 = HirType::RawPtr(Box::new(HirType::Named(self.interner.intern("u8"))));
        let void_t = HirType::Unit;

        // __ptr_offset(ptr: *mut u8, offset: i64) -> *mut u8
        let ptr_offset = self.interner.intern("__ptr_offset");
        self.env.insert_global(ptr_offset, HirType::Func(vec![ptr_u8.clone(), i64_t.clone()], Box::new(ptr_u8.clone())), false);

        // __glyim_alloc(size: i64) -> *mut u8
        let alloc = self.interner.intern("__glyim_alloc");
        self.env.insert_global(alloc, HirType::Func(vec![i64_t.clone()], Box::new(ptr_u8.clone())), false);

        // __glyim_free(ptr: *mut u8) -> ()
        let free = self.interner.intern("__glyim_free");
        self.env.insert_global(free, HirType::Func(vec![ptr_u8.clone()], Box::new(void_t.clone())), false);

        // __glyim_hash_bytes(data: *const u8, len: i64) -> i64
        let hash_bytes = self.interner.intern("__glyim_hash_bytes");
        self.env.insert_global(hash_bytes, HirType::Func(vec![ptr_u8.clone(), i64_t.clone()], Box::new(i64_t.clone())), false);

        // __glyim_hash_seed() -> i64
        let hash_seed = self.interner.intern("__glyim_hash_seed");
        self.env.insert_global(hash_seed, HirType::Func(vec![], Box::new(i64_t.clone())), false);

        // abort() -> !
        let abort = self.interner.intern("abort");
        self.env.insert_global(abort, HirType::Func(vec![], Box::new(HirType::Never)), false);

        // __size_of<T>() -> i64 (simplified)
        let sizeof = self.interner.intern("__size_of");
        self.env.insert_global(sizeof, HirType::Func(vec![], Box::new(i64_t.clone())), false);

        // Register all HIR items in the global environment
        for item in &hir.items {
            match item {
                HirItem::Fn(f) => {
                    let param_tys: Vec<HirType> = f.params.iter().map(|(_, t)| t.clone()).collect();
                    let ret_ty = if let Some(r) = &f.ret { r.clone() } else { HirType::Unit };
                    self.env.insert_global(f.name, HirType::Func(param_tys, Box::new(ret_ty)), false);
                }
                HirItem::Struct(s) => {
                    self.env.insert_global(s.name, HirType::Named(s.name), false);
                }
                HirItem::Enum(e) => {
                    self.env.insert_global(e.name, HirType::Named(e.name), false);
                }
                HirItem::Extern(ext) => {
                    for ef in &ext.functions {
                        let param_tys: Vec<HirType> = ef.params.to_vec();
                        let ret_ty = ef.ret.clone();
                        self.env.insert_global(ef.name, HirType::Func(param_tys, Box::new(ret_ty)), false);
                    }
                }
                HirItem::Impl(imp) => {
                    // Register each impl method by its mangled name (already in HIR)
                    for m in &imp.methods {
                        let param_tys: Vec<HirType> = m.params.iter().map(|(_, t)| t.clone()).collect();
                        let ret_ty = if let Some(r) = &m.ret { r.clone() } else { HirType::Unit };
                        self.env.insert_global(m.name, HirType::Func(param_tys, Box::new(ret_ty)), false);
                    }
                }
            }
        }
    }fn check_fn(&mut self, f: &glyim_hir::HirFn) {
        self.env.clear_locals();
        self.table.reset();
        self.expr_types.clear();
        self.call_type_args.clear();
        self.sizeof_types.clear();

        let is_generic = !f.type_params.is_empty();
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
            self.env.insert(*sym, concrete, f.param_mutability.get(i).copied().unwrap_or(false));
        }

        let concrete_ret = f.ret.as_ref().map(|r| substitute_type(r, &type_param_sub).unwrap_or(HirType::Error));

        self.env.push_scope();
        let body_ty = self.infer_dispatch(&f.body, concrete_ret.as_ref());
        if let Some(expected) = &concrete_ret {
            self.unify_and_record(expected, &body_ty, f.body.get_span(), f.body.get_span());
        }
        self.env.pop_scope();
        self.env.pop_scope();

        self.finalize_fn(f, is_generic, &type_param_map);
    }

    fn freeze_ty(ty: HirType, tp_map: &HashMap<TypeVar, Symbol>, tbl: &mut UnificationTable) -> HirType {
        // First resolve through the unification table
        let resolved = match ty {
            HirType::Infer(_) | HirType::Generic(..) | HirType::Tuple(_) | HirType::RawPtr(_) | HirType::Func(..) => {
                tbl.resolve(&ty).unwrap_or(ty)
            }
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
            HirType::Generic(s, a) => HirType::Generic(s, a.into_iter().map(|x| Self::freeze_ty(x, tp_map, tbl)).collect()),
            HirType::Tuple(e) => HirType::Tuple(e.into_iter().map(|x| Self::freeze_ty(x, tp_map, tbl)).collect()),
            HirType::RawPtr(i) => HirType::RawPtr(Box::new(Self::freeze_ty(*i, tp_map, tbl))),
            HirType::Func(p, r) => HirType::Func(
                p.into_iter().map(|x| Self::freeze_ty(x, tp_map, tbl)).collect(),
                Box::new(Self::freeze_ty(*r, tp_map, tbl))
            ),
            o => o,
        }
    }

    fn finalize_fn(&mut self, f: &glyim_hir::HirFn, is_generic: bool, type_param_map: &HashMap<TypeVar, Symbol>) {

        let mut new_expr = HashMap::new();
        for (&id, ty) in &self.expr_types {
            new_expr.insert(id, Self::freeze_ty(ty.clone(), type_param_map, &mut self.table));
        }
        let mut new_call = HashMap::new();
        for (&id, args) in &self.call_type_args {
            new_call.insert(id, args.iter().map(|a| Self::freeze_ty(a.clone(), type_param_map, &mut self.table)).collect());
        }
        let mut new_sizeof = HashMap::new();
        for (&id, ty) in &self.sizeof_types {
            new_sizeof.insert(id, Self::freeze_ty(ty.clone(), type_param_map, &mut self.table));
        }

        self.fn_types_map.insert(f.name, FnTypes {
            expr_types: new_expr,
            call_type_args: new_call,
            sizeof_types: new_sizeof,
            is_generic,
            type_params: f.type_params.clone(),
            span: f.span,
        });
    }

    fn unify_and_record(&mut self, expected: &HirType, found: &HirType, expected_span: Span, found_span: Span) -> bool {
        match self.table.unify(expected, found, expected_span, found_span) {
            Ok(_) => true,
            Err(e) => { self.errors.push(e.into_type_error()); false }
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
            HirExpr::Ident { name, span, .. } => self.env.lookup(*name).cloned().unwrap_or_else(|| {
                self.errors.push(TypeError::UnresolvedName { name: self.interner.resolve(*name).to_string(), span: *span });
                HirType::Error
            }),
            HirExpr::Return { value, .. } => {
                if let Some(v) = value { self.infer_dispatch(v, expected); }
                HirType::Never
            }
            HirExpr::Block { stmts, .. } => self.infer_block(stmts),
            HirExpr::If { condition, then_branch, else_branch, span, .. } => {
                self.infer_if(condition, then_branch, else_branch, expected, *span)
            }
            HirExpr::Call { id, callee, args, span, .. } => {
                self.infer_call(*id, callee, args, expected, *span)
            }
            HirExpr::Binary { op, lhs, rhs, span, .. } => self.infer_binary(*op, lhs, rhs, *span),
            HirExpr::Unary { op, operand, span, .. } => self.infer_unary(*op, operand, *span),
            HirExpr::MethodCall { id, receiver, method_name, args, span, .. } => {
                self.infer_method_call(*id, receiver, *method_name, args, expected, *span)
            }
            HirExpr::StructLit { struct_name, fields, span, .. } => {
                self.infer_struct_lit(*struct_name, fields, *span)
            }
            HirExpr::EnumVariant { enum_name, variant_name, args, span, .. } => {
                self.infer_enum_variant(*enum_name, *variant_name, args, *span)
            }
            HirExpr::FieldAccess { object, field, span, .. } => {
                self.infer_field_access(object, *field, *span)
            }
            HirExpr::As { expr, target_type, .. } => { self.infer_dispatch(expr, None); target_type.clone() }
            HirExpr::Deref { expr, span, .. } => self.infer_deref(expr, *span),
            HirExpr::Match { scrutinee, arms, span, .. } => self.infer_match(scrutinee, arms, expected, *span),
            HirExpr::While { condition, body, span, .. } => self.infer_while(condition, body, *span),
            HirExpr::ForIn { pattern, iter, body, span, .. } => self.infer_for_in(pattern, iter, body, *span),
            HirExpr::SizeOf { id, target_type, .. } => {
                self.sizeof_types.insert(*id, target_type.clone());
                HirType::Int
            }
            HirExpr::TupleLit { elements, span, .. } => self.infer_tuple_lit(elements, expected, *span),
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

    fn infer_if(&mut self, cond: &HirExpr, then: &HirExpr, els: &Option<Box<HirExpr>>, exp: Option<&HirType>, span: Span) -> HirType {
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

    fn infer_call(&mut self, id: glyim_hir::types::ExprId, callee: &HirExpr, args: &[HirExpr], _exp: Option<&HirType>, span: Span) -> HirType {
        let at: Vec<HirType> = args.iter().map(|a| self.infer_dispatch(a, None)).collect();
        if let HirExpr::Ident { name, .. } = callee {
            if let Some(fty) = self.env.lookup(*name).cloned() {
                if let HirType::Func(params, ret) = fty {
                    let fn_types_opt = self.fn_types_map.get(name);
                    let is_generic = fn_types_opt.map(|ft| ft.is_generic).unwrap_or(false);

                    if is_generic {
                        // Build the formal parameter types: (Symbol, HirType) pairs
                        let formal_params: Vec<(Symbol, HirType)> = params.iter().enumerate()
                            .map(|(i, ty)| {
                                // Use synthetic symbol like _p0, _p1
                                let sym = self.interner.intern(&format!("_p{}", i));
                                (sym, ty.clone())
                            })
                            .collect();
                        let type_params = fn_types_opt.map(|ft| ft.type_params.clone()).unwrap_or_default();

                        // Run the generic solver
                        let solve_result = crate::solve::solve_generic_params(
                            &mut self.table,
                            &type_params,
                            &formal_params,
                            Some(&*ret),
                            &at,
                            None,
                            span,
                            span,
                            &mut |e| { self.errors.push(e); },
                        );

                        // Record the discovered type arguments
                        if solve_result.fully_resolved && !solve_result.concrete_args.is_empty() {
                            self.record_call_type_args(id, solve_result.concrete_args);
                        }

                        // Apply the substitution to the return type
                        let ret_subst = glyim_hir::types::substitute_type(&ret, &solve_result.subst);
                        return ret_subst;
                    }

                    // Non-generic: just unify
                    for (f, a) in params.iter().zip(at.iter()) {
                        self.unify_and_record(f, a, span, span);
                    }
                    return *ret;
                }
            }
            self.errors.push(TypeError::UnresolvedName { name: self.interner.resolve(*name).to_string(), span });
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

    fn infer_binary(&mut self, op: glyim_hir::HirBinOp, lhs: &HirExpr, rhs: &HirExpr, span: Span) -> HirType {
        let lt = self.infer_dispatch(lhs, None);
        let rt = self.infer_dispatch(rhs, None);
        match op {
            glyim_hir::HirBinOp::Eq | glyim_hir::HirBinOp::Neq | glyim_hir::HirBinOp::Lt |
            glyim_hir::HirBinOp::Gt | glyim_hir::HirBinOp::Lte | glyim_hir::HirBinOp::Gte => {
                self.unify_and_record(&lt, &rt, span, span);
                HirType::Bool
            }
            glyim_hir::HirBinOp::And | glyim_hir::HirBinOp::Or => {
                self.unify_and_record(&HirType::Bool, &lt, span, span);
                self.unify_and_record(&HirType::Bool, &rt, span, span);
                HirType::Bool
            }
            _ => { self.unify_and_record(&lt, &rt, span, span); lt }
        }
    }

    fn infer_unary(&mut self, op: glyim_hir::HirUnOp, operand: &HirExpr, span: Span) -> HirType {
        let ot = self.infer_dispatch(operand, None);
        match op {
            glyim_hir::HirUnOp::Not => { self.unify_and_record(&HirType::Bool, &ot, span, span); HirType::Bool }
            glyim_hir::HirUnOp::Neg => ot,
        }
    }

    fn infer_method_call(&mut self, _id: glyim_hir::types::ExprId, recv: &HirExpr, meth: Symbol, args: &[HirExpr], _exp: Option<&HirType>, span: Span) -> HirType {
        let rt = self.infer_dispatch(recv, None);
        let at: Vec<HirType> = std::iter::once(rt.clone()).chain(args.iter().map(|a| self.infer_dispatch(a, None))).collect();

        // Try to look up method by mangled name: TypeName_methodName
        let type_name = match &rt {
            HirType::Named(s) | HirType::Generic(s, _) => *s,
            HirType::RawPtr(inner) => match inner.as_ref() {
                HirType::Named(s) | HirType::Generic(s, _) => *s,
                _ => {
                    self.errors.push(TypeError::UnresolvedMethod { method_name: meth, receiver_type: Box::new(rt.clone()), span });
                    return HirType::Error;
                }
            },
            _ => {
                self.errors.push(TypeError::UnresolvedMethod { method_name: meth, receiver_type: Box::new(rt.clone()), span });
                return HirType::Error;
            }
        };

        let type_str = self.interner.resolve(type_name);
        let method_str = self.interner.resolve(meth);
        let mangled = self.interner.intern(&format!("{}_{}", type_str, method_str));

        // Look up mangled function in environment
        if let Some(fty) = self.env.lookup(mangled).cloned() {
            if let HirType::Func(params, ret) = fty {
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

    fn infer_struct_lit(&mut self, struct_name: Symbol, fields: &[(Symbol, HirExpr)], span: Span) -> HirType {
        // Infer types for all field values
        for (_, f_expr) in fields {
            self.infer_dispatch(f_expr, None);
        }

        // If it's a known struct, return Named(struct_name)
        if let Some(ref idx) = self.hir_index {
            if idx.find_struct(struct_name).is_some() {
                return HirType::Named(struct_name);
            }
        }

        // Unknown struct - still return Named to avoid cascading errors
        HirType::Named(struct_name)
    }

    fn infer_enum_variant(&mut self, enum_name: Symbol, variant_name: Symbol, args: &[HirExpr], span: Span) -> HirType {
        // Infer types for all variant arguments
        for a in args {
            self.infer_dispatch(a, None);
        }

        // If it's a known enum, return Named(enum_name)
        if let Some(ref idx) = self.hir_index {
            if idx.find_enum(enum_name).is_some() {
                return HirType::Named(enum_name);
            }
        }

        // Unknown enum - still return Named
        HirType::Named(enum_name)
    }

    fn infer_field_access(&mut self, obj: &HirExpr, field: Symbol, span: Span) -> HirType {
        let obj_ty = self.infer_dispatch(obj, None);
        match &obj_ty {
            HirType::Named(s) | HirType::Generic(s, _) => {
                if let Some(ref idx) = self.hir_index {
                    if let Some(si) = idx.find_struct(*s) {
                        if let Some(&fi) = si.field_map.get(&field) {
                            if fi < si.fields.len() {
                                return si.fields[fi].1.clone();
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
                if let Some(idx) = field_name.strip_prefix('_').and_then(|s| s.parse::<usize>().ok()) {
                    if idx < elems.len() {
                        return elems[idx].clone();
                    }
                }
            }
            _ => {}
        }
        let resolved = self.interner.resolve(field).to_string();
        self.errors.push(TypeError::UnresolvedName { name: resolved, span });
        HirType::Error
    }

    fn infer_deref(&mut self, expr: &HirExpr, span: Span) -> HirType {
        let it = self.infer_dispatch(expr, None);
        match it {
            HirType::RawPtr(i) => *i,
            _ => HirType::Error,
        }
    }

    fn infer_match(&mut self, scrut: &HirExpr, arms: &[glyim_hir::MatchArm], _exp: Option<&HirType>, span: Span) -> HirType {
        let scrut_ty = self.infer_dispatch(scrut, None);
        // For each arm, bind pattern variables
        let mut result_ty = None;
        for arm in arms {
            self.env.push_scope();
            self.bind_pattern(&arm.pattern, &scrut_ty, false);
            let arm_ty = self.infer_dispatch(&arm.body, None);
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

    fn infer_for_in(&mut self, _pat: &HirPat, iter: &HirExpr, body: &HirExpr, _span: Span) -> HirType {
        self.infer_dispatch(iter, None);
        self.infer_dispatch(body, None);
        HirType::Unit
    }

    fn infer_tuple_lit(&mut self, elems: &[HirExpr], _exp: Option<&HirType>, _span: Span) -> HirType {
        let et: Vec<HirType> = elems.iter().map(|e| self.infer_dispatch(e, None)).collect();
        HirType::Tuple(et)
    }

    fn infer_stmt(&mut self, stmt: &HirStmt) -> HirType {
        match stmt {
            HirStmt::Let { name, mutable, value, .. } => {
                let t = self.infer_dispatch(value, None);
                self.env.insert(*name, t, *mutable);
                HirType::Unit
            }
            HirStmt::LetPat { pattern, mutable, value, .. } => {
                let t = self.infer_dispatch(value, None);
                self.bind_pattern(pattern, &t, *mutable);
                HirType::Unit
            }
            HirStmt::Assign { target, value, span, .. } => {
                if let Some(_e) = self.env.lookup(*target).cloned() {
                    self.infer_dispatch(value, None);
                } else {
                    self.errors.push(TypeError::UnresolvedName { name: self.interner.resolve(*target).to_string(), span: *span });
                }
                HirType::Unit
            }
            HirStmt::Expr(expr) => self.infer_dispatch(expr, None),
            _ => HirType::Unit,
        }
    }
}
