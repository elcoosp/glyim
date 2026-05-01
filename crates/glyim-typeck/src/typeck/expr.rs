use crate::typeck::error::TypeError;
use crate::typeck::resolver::{is_valid_cast, resolve_named_type};
use crate::TypeChecker;
use glyim_hir::node::HirExpr;
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::HirBinOp;
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
            HirExpr::Ident { name, .. } => self.lookup_binding(name).unwrap_or(HirType::Int),
            HirExpr::Binary { op, lhs, rhs, .. } => {
                self.check_expr(lhs);
                self.check_expr(rhs);
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
                let cond_type = self.check_expr(condition).unwrap_or(HirType::Int);
                if cond_type != HirType::Bool {
                    self.errors.push(TypeError::IfConditionMustBeBool {
                        found: cond_type,
                        expr_id: condition.get_id(),
                    });
                }
                let then_type = self.check_expr(then_branch);
                if let Some(eb) = else_branch {
                    self.check_expr(eb);
                }
                then_type.unwrap_or(HirType::Unit)
            }
            HirExpr::Println { arg, .. } => {
                self.check_expr(arg);
                HirType::Unit
            }
            HirExpr::Assert {
                condition, message, ..
            } => {
                self.check_expr(condition);
                if let Some(msg) = message {
                    self.check_expr(msg);
                }
                HirType::Unit
            }
            HirExpr::StructLit {
                struct_name,
                fields,
                ..
            } => self.check_struct_lit(*struct_name, fields),
            HirExpr::FieldAccess { object, field, .. } => self.check_field_access(object, *field),
            HirExpr::EnumVariant {
                enum_name,
                variant_name,
                args,
                ..
            } => self.check_enum_variant(*enum_name, *variant_name, args),
            HirExpr::Match {
                scrutinee, arms, ..
            } => self.check_match(scrutinee, arms),
            HirExpr::Call {
                id, callee, args, ..
            } => {
                let (ret_ty, inferred_args) = self.check_call_with_type_args(*callee, args);
                if let Some(type_args) = inferred_args {
                    eprintln!(
                        "[typeck] Call INSERT id={} callee={} type_args={:?}",
                        id.as_usize(),
                        self.interner.try_resolve(*callee).unwrap_or("<?>"),
                        type_args
                    );
                    self.call_type_args.insert(*id, type_args);
                }
                ret_ty
            }
            HirExpr::As {
                expr, target_type, ..
            } => self.check_as(expr, target_type),
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
                let cond_type = self.check_expr(condition).unwrap_or(HirType::Int);
                if cond_type != HirType::Bool {
                    self.errors.push(TypeError::IfConditionMustBeBool {
                        found: cond_type,
                        expr_id: condition.get_id(),
                    });
                }
                self.check_expr(body);
                HirType::Unit
            }
            HirExpr::SizeOf { .. } => HirType::Int,
            HirExpr::Return { .. } => HirType::Never,
            HirExpr::Deref { expr, id, .. } => {
                let inner_ty = self.check_expr(expr).unwrap_or(HirType::Never);
                match inner_ty {
                    HirType::RawPtr(inner) => *inner,
                    _ => {
                        self.errors.push(TypeError::DerefNonPointer {
                            found: inner_ty,
                            expr_id: *id,
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
                    let mangled = format!(
                        "{}_{}",
                        self.interner.resolve(type_name),
                        self.interner.resolve(*method_name)
                    );
                    let mangled_sym = self.interner.intern(&mangled);
                    if let Some(methods) = self.impl_methods.get(&type_name) {
                        if let Some(fn_def) = methods.iter().find(|f| f.name == mangled_sym) {
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
                                if let HirType::Named(param_sym) = param_ty {
                                    if fn_def.type_params.contains(param_sym) {
                                        if *arg_ty != HirType::Never
                                            && *arg_ty != HirType::Named(*param_sym)
                                        {
                                            sub.insert(*param_sym, arg_ty.clone());
                                        }
                                    }
                                }
                            }
                            // Record call_type_args for monomorphizer
                            if !sub.is_empty() {
                                let concrete_args: Vec<HirType> = fn_def
                                    .type_params
                                    .iter()
                                    .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                    .collect();
                                eprintln!(
                                    "[typeck] MethodCall INSERT id={} method={} args={:?}",
                                    id.as_usize(),
                                    self.interner.resolve(*method_name),
                                    concrete_args
                                );
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
    fn check_struct_lit(&mut self, struct_name: Symbol, fields: &[(Symbol, HirExpr)]) -> HirType {
        let field_names: Vec<Symbol> = fields.iter().map(|(sym, _)| *sym).collect();
        let field_count = fields.len();
        let field_value_types: Vec<HirType> = fields
            .iter()
            .filter_map(|(_, val)| self.check_expr(val))
            .collect();
        if let Some(info) = self.structs.get(&struct_name) {
            for field_sym in &field_names {
                if !info.field_map.contains_key(field_sym) {
                    self.errors.push(TypeError::UnknownField {
                        struct_name,
                        field: *field_sym,
                    });
                }
            }
            if field_count != info.fields.len() {
                for field in &info.fields {
                    if !field_names.contains(&field.name) {
                        self.errors.push(TypeError::MissingField {
                            struct_name,
                            field: field.name,
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
                        if let Some(field_ty) = info.fields.get(i).map(|f| &f.ty) {
                            if let Some(val_ty) = field_value_types.get(i) {
                                if let HirType::Named(param_sym) = field_ty {
                                    if *param_sym == *tp {
                                        match val_ty {
                                            HirType::Named(v_sym)
                                                if info.type_params.contains(v_sym) => {}
                                            _ => {
                                                sub.insert(*tp, val_ty.clone());
                                            }
                                        }
                                    }
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
    fn check_field_access(&mut self, object: &HirExpr, field: Symbol) -> HirType {
        let obj_type = self.check_expr(object);
        let result = match &obj_type {
            Some(HirType::Tuple(elems)) => self.check_tuple_field_access(field, elems),
            Some(HirType::Named(name)) => self.check_struct_field_access(*name, field),
            Some(HirType::Generic(name, args)) => {
                if let Some(info) = self.structs.get(name) {
                    if let Some(field_info) = info.fields.iter().find(|f| f.name == field) {
                        let mut sub = HashMap::new();
                        for (i, tp) in info.type_params.iter().enumerate() {
                            if let Some(arg) = args.get(i) {
                                sub.insert(*tp, arg.clone());
                            }
                        }
                        let result = glyim_hir::types::substitute_type(&field_info.ty, &sub);
                        return result;
                    }
                }
                HirType::Int
            }
            _ => HirType::Int,
        };
        result
    }
    fn check_tuple_field_access(&mut self, field: Symbol, elems: &[HirType]) -> HirType {
        let field_name = self.interner.resolve(field);
        if let Some(index_str) = field_name.strip_prefix('_') {
            if let Ok(idx) = index_str.parse::<usize>() {
                if idx < elems.len() {
                    return elems[idx].clone();
                }
            }
        }
        self.errors.push(TypeError::UnknownField {
            struct_name: self.dummy_symbol(),
            field,
        });
        HirType::Int
    }
    fn check_struct_field_access(&mut self, struct_name: Symbol, field: Symbol) -> HirType {
        if let Some(info) = self.structs.get(&struct_name) {
            if !info.field_map.contains_key(&field) {
                self.errors
                    .push(TypeError::UnknownField { struct_name, field });
            }
        }
        HirType::Int
    }
    fn check_enum_variant(
        &mut self,
        enum_name: Symbol,
        variant_name: Symbol,
        args: &[HirExpr],
    ) -> HirType {
        if let Some(info) = self.enums.get(&enum_name) {
            if !info.variant_map.contains_key(&variant_name) {
                self.errors.push(TypeError::UnknownField {
                    struct_name: enum_name,
                    field: variant_name,
                });
            }
        }
        for arg in args {
            self.check_expr(arg);
        }
        HirType::Named(enum_name)
    }
    fn check_match(
        &mut self,
        scrutinee: &HirExpr,
        arms: &[(glyim_hir::HirPattern, Option<HirExpr>, HirExpr)],
    ) -> HirType {
        let scrutinee_ty = self.check_expr(scrutinee).unwrap_or(HirType::Never);
        self.check_match_exhaustiveness(&scrutinee_ty, arms);
        let mut arm_types = vec![];
        for (_, guard, body) in arms {
            if let Some(g) = guard {
                self.check_expr(g);
            }
            if let Some(t) = self.check_expr(body) {
                arm_types.push(t);
            }
        }
        arm_types.first().cloned().unwrap_or(HirType::Unit)
    }
    fn check_call_with_type_args(
        &mut self,
        callee: Symbol,
        args: &[HirExpr],
    ) -> (HirType, Option<Vec<HirType>>) {
        let arg_types: Vec<HirType> = args.iter().filter_map(|a| self.check_expr(a)).collect();
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
        for (_type_name, methods) in &self.impl_methods {
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
    fn check_as(&mut self, expr: &HirExpr, target_type: &HirType) -> HirType {
        let from_ty = self.check_expr(expr).unwrap_or(HirType::Int);
        let resolved_target = resolve_named_type(&self.interner, target_type);
        let resolved_from = resolve_named_type(&self.interner, &from_ty);
        if !is_valid_cast(&resolved_from, &resolved_target) {
            self.errors.push(TypeError::MismatchedTypes {
                expected: target_type.clone(),
                found: from_ty,
                expr_id: ExprId::new(0),
            });
        }
        target_type.clone()
    }
}
