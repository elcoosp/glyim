use crate::TypeChecker;
use crate::typeck::error::TypeError;
use crate::typeck::resolver::{is_valid_cast, resolve_named_type};
use glyim_diag::Span;
use glyim_hir::HirBinOp;
use glyim_hir::monomorphize::type_to_short_string;
use glyim_hir::node::HirExpr;
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::Symbol;
use std::collections::HashMap;
impl TypeChecker {
    #[tracing::instrument(skip_all)]
    pub(crate) fn check_expr(&mut self, expr: &HirExpr) -> Option<HirType> {
        let id = self.extract_expr_id(expr);
        let ty = self.infer_expr(expr);
        self.set_type(id, ty.clone());
        Some(ty)
    }
    fn extract_expr_id(&self, expr: &HirExpr) -> ExprId {
        match expr {
            HirExpr::IntLit { id, .. } => *id,
            HirExpr::FloatLit { id, .. } => *id,
            HirExpr::BoolLit { id, .. } => *id,
            HirExpr::StrLit { id, .. } => *id,
            HirExpr::Ident { id, .. } => *id,
            HirExpr::UnitLit { id, .. } => *id,
            HirExpr::Binary { id, .. } => *id,
            HirExpr::Unary { id, .. } => *id,
            HirExpr::Block { id, .. } => *id,
            HirExpr::If { id, .. } => *id,
            HirExpr::Println { id, .. } => *id,
            HirExpr::Call { id, .. } => *id,
            HirExpr::Assert { id, .. } => *id,
            HirExpr::As { id, .. } => *id,
            HirExpr::Match { id, .. } => *id,
            HirExpr::FieldAccess { id, .. } => *id,
            HirExpr::StructLit { id, .. } => *id,
            HirExpr::EnumVariant { id, .. } => *id,
            HirExpr::TupleLit { id, .. } => *id,
            HirExpr::SizeOf { id, .. } => *id,
            HirExpr::AddrOf { id, .. } => *id,
            HirExpr::Return { id, .. } => *id,
            HirExpr::Deref { id, .. } => *id,
            HirExpr::ForIn { id, .. } => *id,
            HirExpr::While { id, .. } => *id,
            HirExpr::MethodCall { id, .. } => *id,
        }
    }
    fn infer_expr(&mut self, expr: &HirExpr) -> HirType {
        match expr {
            HirExpr::IntLit { .. } => HirType::Int,
            HirExpr::FloatLit { .. } => HirType::Float,
            HirExpr::BoolLit { .. } => HirType::Bool,
            HirExpr::StrLit { .. } => HirType::Str,
            HirExpr::UnitLit { .. } => HirType::Unit,
            HirExpr::Ident { name, span, .. } => self.lookup_binding(name).unwrap_or_else(|| {
                self.errors.push(TypeError::UnresolvedName {
                    name: *name,
                    span: (span.start, span.end),
                });
                HirType::Error
            }),
            HirExpr::Binary { op, lhs, rhs, .. } => {
                let lt = self.check_expr(lhs).unwrap_or(HirType::Error);
                let rt = self.check_expr(rhs).unwrap_or(HirType::Error);
                if matches!(lt, HirType::Error) || matches!(rt, HirType::Error) {
                    return HirType::Error;
                }
                match op {
                    HirBinOp::Eq
                    | HirBinOp::Neq
                    | HirBinOp::Lt
                    | HirBinOp::Gt
                    | HirBinOp::Lte
                    | HirBinOp::Gte => HirType::Bool,
                    _ => HirType::Int,
                }
            }
            HirExpr::Unary { operand, .. } => {
                let t = self.check_expr(operand).unwrap_or(HirType::Error);
                if matches!(t, HirType::Error) {
                    return HirType::Error;
                }
                HirType::Int
            }
            HirExpr::Block { stmts, .. } => {
                let mut last = HirType::Unit;
                for stmt in stmts {
                    if let Some(t) = self.check_stmt(stmt) {
                        if matches!(t, HirType::Error) {
                            return HirType::Error;
                        }
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
                let cond_t = self.check_expr(condition).unwrap_or(HirType::Error);
                if matches!(cond_t, HirType::Error) {
                    return HirType::Error;
                }
                let then_type = self.check_expr(then_branch);
                if let Some(eb) = else_branch {
                    self.check_expr(eb);
                }
                then_type.unwrap_or(HirType::Unit)
            }
            HirExpr::Println { arg, .. } => {
                let t = self.check_expr(arg).unwrap_or(HirType::Error);
                if matches!(t, HirType::Error) {
                    return HirType::Error;
                }
                HirType::Unit
            }
            HirExpr::Assert {
                condition, message, ..
            } => {
                let ct = self.check_expr(condition).unwrap_or(HirType::Error);
                if matches!(ct, HirType::Error) {
                    return HirType::Error;
                }
                if let Some(msg) = message {
                    self.check_expr(msg);
                }
                HirType::Unit
            }
            HirExpr::StructLit {
                struct_name,
                fields,
                ..
            } => self.check_struct_lit(*struct_name, fields, expr.get_span()),
            HirExpr::FieldAccess { object, field, .. } => self.check_field_access(object, *field, expr.get_span()),
            HirExpr::EnumVariant {
                enum_name,
                variant_name,
                args,
                ..
            } => self.check_enum_variant(*enum_name, *variant_name, args, expr.get_span()),
            HirExpr::Match {
                scrutinee, arms, ..
            } => self.check_match(
                scrutinee,
                &arms
                    .iter()
                    .map(|arm| (arm.pattern.clone(), arm.guard.clone(), arm.body.clone()))
                    .collect::<Vec<_>>(),
                expr.get_span(),
            ),
            HirExpr::Call {
                id, callee, args, ..
            } => {
                // If call_type_args already contains this id (from a MethodCall
                // that was desugared into this Call), use the stored type args
                // instead of inferring from the argument types again.
                if let Some(type_args) = self.call_type_args.get(id).cloned() {
                    let fn_def = self.fns.iter().find(|f| f.name == *callee).or_else(|| {
                        self.impl_methods
                            .values()
                            .find_map(|ms| ms.iter().find(|m| m.name == *callee))
                    });
                    if let Some(fn_def) = fn_def {
                        let mut sub = std::collections::HashMap::new();
                        for (i, tp) in fn_def.type_params.iter().enumerate() {
                            if let Some(ct) = type_args.get(i) {
                                sub.insert(*tp, ct.clone());
                            }
                        }
                        let ret = fn_def.ret.clone().unwrap_or(HirType::Int);

                        glyim_hir::types::substitute_type(&ret, &sub)
                    } else {
                        HirType::Int
                    }
                } else {
                    let (ret_ty, inferred_args) = self.check_call_with_type_args(*callee, args, expr.get_span());
                    if let Some(type_args) = inferred_args {
                        self.call_type_args.insert(*id, type_args);
                    }
                    ret_ty
                }
            }
            HirExpr::As {
                expr, target_type, ..
            } => self.check_as(expr, target_type, expr.get_span()),
            HirExpr::TupleLit { elements, .. } => {
                let elem_types: Vec<HirType> =
                    elements.iter().filter_map(|e| self.check_expr(e)).collect();
                HirType::Tuple(elem_types)
            }
            HirExpr::ForIn { iter, body, .. } => {
                self.check_expr(iter);
                self.check_expr(body);
                HirType::Unit
            }
            HirExpr::While {
                condition, body, ..
            } => {
                self.check_expr(condition);
                self.check_expr(body);
                HirType::Unit
            }
            HirExpr::SizeOf { .. } => HirType::Int,
            HirExpr::AddrOf { .. } => HirType::Int,
            HirExpr::Return { .. } => HirType::Never,
            HirExpr::Deref { expr, id, .. } => {
                let inner_ty = self.check_expr(expr).unwrap_or(HirType::Never);
                match inner_ty {
                    HirType::RawPtr(inner) => *inner,
                    _ => {
                        self.errors.push(TypeError::DerefNonPointer {
                            found: inner_ty,
                            expr_id: *id,
                            span: (expr.get_span().start, expr.get_span().end),
                        });
                        HirType::Never
                    }
                }
            }
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                ..
            } => {
                let receiver_ty = self.check_expr(receiver).unwrap_or(HirType::Int);
                let arg_types: Vec<HirType> =
                    args.iter().filter_map(|a| self.check_expr(a)).collect();
                // look up method in impl methods by mangled name computed from receiver type
                let type_sym = match &receiver_ty {
                    HirType::Named(s) | HirType::Generic(s, _) => Some(*s),
                    _ => None,
                };
                if let Some(type_name) = type_sym {
                    let base = format!(
                        "{}_{}",
                        self.interner.resolve(type_name),
                        self.interner.resolve(*method_name)
                    );
                    let base_sym = self.interner.intern(&base);
                    let mangled = match &receiver_ty {
                        HirType::Generic(_, type_args) if !type_args.is_empty() => {
                            let suffix = type_args
                                .iter()
                                .map(|a| type_to_short_string(a, &self.interner))
                                .collect::<Vec<_>>()
                                .join("_");
                            format!("{}__{}", base, suffix)
                        }
                        _ => base.clone(),
                    };
                    let _mangled_sym = self.interner.intern(&mangled);

                    // (method_resolved removed)
                    eprintln!("[typeck MethodCall] receiver_ty={:?}", receiver_ty);
                    eprintln!("[typeck MethodCall] mangled name: {}", mangled);
                    eprintln!(
                        "[typeck MethodCall] known impl_methods for {:?}: {:?}",
                        type_name,
                        self.impl_methods.get(&type_name).map(|ms| ms
                            .iter()
                            .map(|f| self.interner.resolve(f.name))
                            .collect::<Vec<_>>())
                    );
                    if let Some(methods) = self.impl_methods.get(&type_name) {
                        // Try both the base name and the mangled name with type suffix
                        if let Some(fn_def) = methods.iter().filter(|f| f.name == base_sym).next() {
                            let mut sub = std::collections::HashMap::new();
                            // Infer from receiver type args
                            if let HirType::Generic(_, type_args) = &receiver_ty {
                                for (tp, ct) in fn_def.type_params.iter().zip(type_args.iter()) {
                                    sub.insert(*tp, ct.clone());
                                }
                            }
                            // Infer from argument types (params[0] is self, args[0] matches params[1])
                            for (arg_ty, (_, param_ty)) in
                                arg_types.iter().zip(fn_def.params.iter().skip(1))
                            {
                                if let HirType::Named(param_sym) = param_ty
                                    && fn_def.type_params.contains(param_sym)
                                    && *arg_ty != HirType::Never
                                    && *arg_ty != HirType::Named(*param_sym)
                                {
                                    sub.insert(*param_sym, arg_ty.clone());
                                }
                            }
                            // Record call_type_args for monomorphizer
                            let concrete_args: Vec<HirType> = fn_def
                                .type_params
                                .iter()
                                .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                .collect();
                            if !concrete_args.is_empty() {
                                self.call_type_args.insert(*id, concrete_args);
                            }
                            let ret = fn_def.ret.clone().unwrap_or(HirType::Int);
                            let concrete_ret = glyim_hir::types::substitute_type(&ret, &sub);
                            return concrete_ret;
                        }
                    }
                }
                HirType::Int
            }
        }
    }
    fn check_struct_lit(&mut self, struct_name: Symbol, fields: &[(Symbol, HirExpr)], span: Span) -> HirType {
        let field_names: Vec<Symbol> = fields.iter().map(|(sym, _)| *sym).collect();
        let field_count = fields.len();
        let field_value_types: Vec<HirType> = fields
            .iter()
            .filter_map(|(_, val)| {
                let t = self.check_expr(val).unwrap_or(HirType::Error);
                if matches!(t, HirType::Error) {
                    return Some(HirType::Error);
                }
                Some(t)
            })
            .collect();
        if field_value_types
            .iter()
            .any(|t| matches!(t, HirType::Error))
        {
            return HirType::Error;
        }
        if let Some(info) = self.structs.get(&struct_name) {
            for field_sym in &field_names {
                if !info.field_map.contains_key(field_sym) {
                    self.errors.push(TypeError::UnknownField {
                        struct_name,
                        field: *field_sym,
                        span: (span.start, span.end),
                    });
                }
            }
            if field_count != info.fields.len() {
                for field in &info.fields {
                    if !field_names.contains(&field.name) {
                        self.errors.push(TypeError::MissingField {
                            struct_name,
                            field: field.name,
                            span: (span.start, span.end),
                        });
                    }
                }
            }
            // If the struct is generic, and we are inside a generic function (impl method)
            // with the same type params, use those params as the concrete args.
            if !info.type_params.is_empty() {
                if !self.current_fn_type_params.is_empty()
                    && info.type_params == self.current_fn_type_params
                {
                    let concrete_args: Vec<HirType> = self
                        .current_fn_type_params
                        .iter()
                        .map(|sym| HirType::Named(*sym))
                        .collect();
                    return HirType::Generic(struct_name, concrete_args);
                }
                // Otherwise, infer type args from field values (old logic)
                if field_value_types.len() == info.fields.len() {
                    let mut sub: HashMap<Symbol, HirType> = HashMap::new();
                    for (i, tp) in info.type_params.iter().enumerate() {
                        if let Some(field_ty) = info.fields.get(i).map(|f| &f.ty)
                            && let Some(val_ty) = field_value_types.get(i)
                            && let HirType::Named(param_sym) = field_ty
                            && *param_sym == *tp
                        {
                            match val_ty {
                                HirType::Named(v_sym) if info.type_params.contains(v_sym) => {}
                                _ => {
                                    sub.insert(*tp, val_ty.clone());
                                }
                            }
                        }
                    }
                    if sub.len() == info.type_params.len() {
                        let concrete_args: Vec<HirType> = info
                            .type_params
                            .iter()
                            .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                            .collect();
                        return HirType::Generic(struct_name, concrete_args);
                    }
                }
            }
        }
        HirType::Named(struct_name)
    }
    fn check_field_access(&mut self, object: &HirExpr, field: Symbol, span: Span) -> HirType {
        let obj_type = self.check_expr(object).unwrap_or(HirType::Never);

        match &obj_type {
            HirType::Tuple(elems) => self.check_tuple_field_access(field, elems, span),
            HirType::Named(name) => {
                if self.structs.contains_key(name) {
                    self.check_struct_field_access(*name, field, span)
                } else {
                    self.errors.push(TypeError::UnknownField {
                        struct_name: *name,
                        field,
                        span: (span.start, span.end),
                    });
                    HirType::Never
                }
            }
            HirType::Generic(name, _args) => {
                if self.structs.contains_key(name) {
                    self.check_struct_field_access(*name, field, span)
                } else {
                    self.errors.push(TypeError::UnknownField {
                        struct_name: *name,
                        field,
                        span: (span.start, span.end),
                    });
                    HirType::Never
                }
            }
            _ => {
                self.errors.push(TypeError::UnknownField {
                    struct_name: self.dummy_symbol(),
                    field,
                    span: (0, 0),
                });
                HirType::Never
            }
        }
    }
    fn check_tuple_field_access(&mut self, field: Symbol, elems: &[HirType], _span: Span) -> HirType {
        let field_name = self.interner.resolve(field);
        if let Some(index_str) = field_name.strip_prefix('_')
            && let Ok(idx) = index_str.parse::<usize>()
            && idx < elems.len()
        {
            return elems[idx].clone();
        }
        self.errors.push(TypeError::UnknownField {
            struct_name: self.dummy_symbol(),
            field,
            span: (0, 0),
        });
        HirType::Int
    }
    fn check_struct_field_access(&mut self, struct_name: Symbol, field: Symbol, _span: Span) -> HirType {
        if let Some(info) = self.structs.get(&struct_name) {
            if !info.field_map.contains_key(&field) {
                self.errors.push(TypeError::UnknownField {
                    struct_name,
                    field,
                    span: (0, 0),
                });
            } else if let Some(field_info) = info.fields.iter().find(|f| f.name == field) {
                return field_info.ty.clone();
            }
        }
        HirType::Int
    }
    fn check_enum_variant(
        &mut self,
        enum_name: Symbol,
        variant_name: Symbol,
        args: &[HirExpr],
        span: Span,
    ) -> HirType {
        if let Some(info) = self.enums.get(&enum_name)
            && !info.variant_map.contains_key(&variant_name)
        {
            self.errors.push(TypeError::UnknownField {
                struct_name: enum_name,
                field: variant_name,
                span: (span.start, span.end),
            });
        }
        let mut arg_types: Vec<HirType> = args.iter().filter_map(|a| self.check_expr(a)).collect();
        if let Some(info) = self.enums.get(&enum_name) {
            // If enum has more type params than provided args AND the enum has exactly 2 type params
            // (Result<T,E> pattern), pad with type param symbols.
            if arg_types.len() == 1 && info.type_params.len() == 2 {
                let tp = info.type_params[1];
                arg_types.push(HirType::Named(tp));
            }
        }
        if !arg_types.is_empty() {
            HirType::Generic(enum_name, arg_types)
        } else {
            HirType::Named(enum_name)
        }
    }
    fn check_match(
        &mut self,
        scrutinee: &HirExpr,
        arms: &[(glyim_hir::HirPattern, Option<HirExpr>, HirExpr)],
        span: Span,
    ) -> HirType {
        let scrutinee_ty = self.check_expr(scrutinee).unwrap_or(HirType::Never);
        self.check_match_exhaustiveness(&scrutinee_ty, arms, span);
        let mut arm_types = vec![];
        for arm in arms {
            self.push_scope();
            self.bind_match_pattern(&arm.0, &scrutinee_ty);
            if let Some(ref g) = arm.1 {
                self.check_expr(g);
            }
            if let Some(t) = self.check_expr(&arm.2) {
                arm_types.push(t);
            }
            self.pop_scope();
        }
        arm_types.first().cloned().unwrap_or(HirType::Unit)
    }
    fn check_call_with_type_args(
        &mut self,
        callee: Symbol,
        args: &[HirExpr],
        call_span: Span,
    ) -> (HirType, Option<Vec<HirType>>) {
        let arg_types: Vec<HirType> = args.iter().filter_map(|a| self.check_expr(a)).collect();
        // If the callee name already contains __ (e.g., Vec_get__Entry_i64_i64),
        // the function is already specialized — don't infer new type args.
        let callee_name = self.interner.resolve(callee);
        if callee_name.contains("__") {
            // Already specialized — return the function's return type without type args
            if let Some(fn_def) = self.fns.iter().find(|f| f.name == callee) {
                return (fn_def.ret.clone().unwrap_or(HirType::Int), None);
            }
            // Look up in impl_methods by mangled name
            for methods in self.impl_methods.values() {
                if let Some(fn_def) = methods.iter().find(|f| f.name == callee) {
                    return (fn_def.ret.clone().unwrap_or(HirType::Int), None);
                }
            }
        }
        // Check argument types against parameter types (only for non-generic fns)
        if let Some(fn_def) = self.fns.iter().find(|f| f.name == callee) {
            // Only check arg types for fully concrete functions
            if fn_def.type_params.is_empty() {
                for (i, arg_ty) in arg_types.iter().enumerate() {
                    if let Some((_, param_ty)) = fn_def.params.get(i)
                        && param_ty != arg_ty
                    {
                        self.errors.push(TypeError::MismatchedTypes {
                            expected: param_ty.clone(),
                            found: arg_ty.clone(),
                            expr_id: args.get(i).map(|a| a.get_id()).unwrap_or(ExprId::new(0)),
                span: (0, 0),
                        });
                    }
                }
            }
        }
        let fn_def = self.fns.iter().find(|f| f.name == callee);
        if let Some(fn_def) = fn_def {
            if !fn_def.type_params.is_empty() {
                let sub: HashMap<Symbol, HirType> = fn_def
                    .type_params
                    .iter()
                    .zip(arg_types.iter())
                    .filter_map(|(tp, at)| {
                        if at == &HirType::Never {
                            None
                        } else {
                            Some((*tp, at.clone()))
                        }
                    })
                    .collect();
                if sub.len() == fn_def.type_params.len() {
                    let type_args: Vec<HirType> = fn_def
                        .type_params
                        .iter()
                        .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                        .collect();
                    eprintln!(
                        "[typeck DEBUG] check_call_with_type_args fn={} type_args=[{}]",
                        self.interner.resolve(callee),
                        type_args
                            .iter()
                            .map(|t| format!("{:?}", t))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    let ret = fn_def.ret.clone().unwrap_or(HirType::Int);
                    return (
                        glyim_hir::types::substitute_type(&ret, &sub),
                        Some(type_args),
                    );
                }
            }
            return (fn_def.ret.clone().unwrap_or(HirType::Int), None);
        }
        // Look up in impl_methods by mangled name; also infer type params
        for methods in self.impl_methods.values() {
            if let Some(fn_def) = methods.iter().find(|f| f.name == callee) {
                if !fn_def.type_params.is_empty() && !arg_types.is_empty() {
                    let sub: HashMap<Symbol, HirType> = fn_def
                        .type_params
                        .iter()
                        .zip(arg_types.iter())
                        .filter_map(|(tp, at)| {
                            if at == &HirType::Never {
                                None
                            } else {
                                Some((*tp, at.clone()))
                            }
                        })
                        .collect();
                    if sub.len() == fn_def.type_params.len() {
                        let type_args: Vec<HirType> = fn_def
                            .type_params
                            .iter()
                            .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                            .collect();
                        let ret = fn_def.ret.clone().unwrap_or(HirType::Int);
                        return (
                            glyim_hir::types::substitute_type(&ret, &sub),
                            Some(type_args),
                        );
                    }
                }
                return (fn_def.ret.clone().unwrap_or(HirType::Int), None);
            }
        }
        if self.extern_fns.contains_key(&callee) {
            return (
                self.extern_fns
                    .get(&callee)
                    .map(|sig| sig.ret.clone())
                    .unwrap_or(HirType::Int),
                None,
            );
        }
        (HirType::Int, None)
    }
    fn check_as(&mut self, expr: &HirExpr, target_type: &HirType, as_span: Span) -> HirType {
        let from_ty = self.check_expr(expr).unwrap_or(HirType::Int);
        let resolved_target = resolve_named_type(&self.interner, target_type);
        let resolved_from = resolve_named_type(&self.interner, &from_ty);
        if !is_valid_cast(&resolved_from, &resolved_target) {
            self.errors.push(TypeError::MismatchedTypes {
                expected: target_type.clone(),
                found: from_ty,
                expr_id: ExprId::new(0),
                span: (as_span.start, as_span.end),
            });
        }
        target_type.clone()
    }
}
