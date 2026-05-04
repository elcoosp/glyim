use super::*;
use crate::item::HirItem;
use crate::node::{HirExpr, HirStmt};
use crate::types::{ExprId, HirType};
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn collect_and_specialize(&mut self) {
        for (expr_id, type_args) in self.call_type_args.iter() {
            if type_args.iter().any(|a| self.has_unresolved_type_param(a)) {
                continue;
            }
            if let Some(callee) = self.find_callee_by_id_from_hir(*expr_id) {
                self.queue_fn_specialization(callee, type_args.clone());
            }
        }

        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) => {
                    self.scan_expr_for_generic_calls(&f.body, &HashMap::new());
                    self.scan_expr_for_struct_instantiations(&f.body, &HashMap::new());
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        self.scan_expr_for_generic_calls(&m.body, &HashMap::new());
                        self.scan_expr_for_struct_instantiations(&m.body, &HashMap::new());
                    }
                }
                _ => {}
            }
        }

        while let Some((fn_name, type_args)) = self.fn_work_queue.pop() {
            let key = (fn_name, type_args.clone());
            if self.fn_specs.contains_key(&key) {
                continue;
            }
            if let Some(generic_fn) = self.find_fn(fn_name) {
                let specialized = self.specialize_fn(&generic_fn, &type_args);
                let sub: HashMap<Symbol, HirType> = generic_fn
                    .type_params
                    .iter()
                    .zip(type_args.iter())
                    .map(|(tp, ct)| (*tp, ct.clone()))
                    .collect();
                self.scan_expr_for_generic_calls(&specialized.body, &sub);
                self.scan_expr_for_struct_instantiations(&specialized.body, &sub);
                self.fn_specs.insert(key, specialized);
            }
        }
    }

    fn find_callee_by_id_from_hir(&mut self, search_id: ExprId) -> Option<Symbol> {
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) => {
                    if let Some(callee) = Self::find_callee_in_expr(&f.body, search_id, self) {
                        return Some(callee);
                    }
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if let Some(callee) = Self::find_callee_in_expr(&m.body, search_id, self) {
                            return Some(callee);
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn find_callee_in_expr(
        expr: &HirExpr,
        search_id: ExprId,
        ctx: &mut MonoContext<'a>,
    ) -> Option<Symbol> {
        match expr {
            HirExpr::Call { id, callee, .. } if *id == search_id => Some(*callee),
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                ..
            } if *id == search_id => {
                let receiver_ty = ctx.get_expr_type(receiver.get_id());
                let inner_ty = match receiver_ty {
                    Some(HirType::RawPtr(inner)) => Some(inner.as_ref().clone()),
                    other => other,
                };
                match inner_ty {
                    Some(HirType::Named(type_name)) => {
                        let mangled = format!(
                            "{}_{}",
                            ctx.interner.resolve(type_name),
                            ctx.interner.resolve(*method_name)
                        );
                        Some(ctx.interner.intern(&mangled))
                    }
                    Some(HirType::Generic(type_name, _)) => {
                        let mangled = format!(
                            "{}_{}",
                            ctx.interner.resolve(type_name),
                            ctx.interner.resolve(*method_name)
                        );
                        Some(ctx.interner.intern(&mangled))
                    }
                    _ => None,
                }
            }
            HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| match s {
                HirStmt::Expr(e) => Self::find_callee_in_expr(e, search_id, ctx),
                HirStmt::Let { value, .. }
                | HirStmt::LetPat { value, .. }
                | HirStmt::Assign { value, .. }
                | HirStmt::AssignField { value, .. } => {
                    Self::find_callee_in_expr(value, search_id, ctx)
                }
                HirStmt::AssignDeref { target, value, .. } => {
                    Self::find_callee_in_expr(target, search_id, ctx)
                        .or_else(|| Self::find_callee_in_expr(value, search_id, ctx))
                }
            }),
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => Self::find_callee_in_expr(condition, search_id, ctx)
                .or_else(|| Self::find_callee_in_expr(then_branch, search_id, ctx))
                .or_else(|| {
                    else_branch
                        .as_ref()
                        .and_then(|e| Self::find_callee_in_expr(e, search_id, ctx))
                }),
            HirExpr::Match {
                scrutinee, arms, ..
            } => Self::find_callee_in_expr(scrutinee, search_id, ctx).or_else(|| {
                arms.iter().find_map(|(_, guard, body)| {
                    guard
                        .as_ref()
                        .and_then(|g| Self::find_callee_in_expr(g, search_id, ctx))
                        .or_else(|| Self::find_callee_in_expr(body, search_id, ctx))
                })
            }),
            HirExpr::Binary { lhs, rhs, .. } => Self::find_callee_in_expr(lhs, search_id, ctx)
                .or_else(|| Self::find_callee_in_expr(rhs, search_id, ctx)),
            HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } => {
                Self::find_callee_in_expr(operand, search_id, ctx)
            }
            HirExpr::While {
                condition: _, body, ..
            } => Self::find_callee_in_expr(body, search_id, ctx),
            HirExpr::ForIn { iter, body, .. } => Self::find_callee_in_expr(iter, search_id, ctx)
                .or_else(|| Self::find_callee_in_expr(body, search_id, ctx)),
            HirExpr::Return { value: Some(v), .. } => Self::find_callee_in_expr(v, search_id, ctx),
            _ => None,
        }
    }

    pub(crate) fn concretize_type_args(&mut self, args: &[HirType]) -> Vec<HirType> {
        args.iter().map(|ty| self.concretize_type(ty)).collect()
    }

    fn concretize_type(&mut self, ty: &HirType) -> HirType {
        match ty {
            HirType::Generic(sym, inner_args) => {
                let concrete_inner: Vec<HirType> =
                    inner_args.iter().map(|a| self.concretize_type(a)).collect();
                let all_concrete = concrete_inner
                    .iter()
                    .all(|a| !self.has_unresolved_type_param(a));
                if all_concrete {
                    let key = (*sym, concrete_inner.clone());
                    if self.struct_specs.contains_key(&key) {
                        let mangled = self.interner.intern(&format!(
                            "{}__{}",
                            self.interner.resolve(*sym),
                            concrete_inner
                                .iter()
                                .map(|t| super::mangling::type_to_short_string(t, self.interner))
                                .collect::<Vec<_>>()
                                .join("_")
                        ));
                        return HirType::Named(mangled);
                    }
                }
                HirType::Generic(*sym, concrete_inner)
            }
            HirType::Named(_)
            | HirType::Int
            | HirType::Bool
            | HirType::Float
            | HirType::Str
            | HirType::Unit
            | HirType::Never
            | HirType::Opaque(_) => ty.clone(),
            HirType::RawPtr(inner) => HirType::RawPtr(Box::new(self.concretize_type(inner))),
            HirType::Option(inner) => HirType::Option(Box::new(self.concretize_type(inner))),
            HirType::Result(ok, err) => HirType::Result(
                Box::new(self.concretize_type(ok)),
                Box::new(self.concretize_type(err)),
            ),
            HirType::Tuple(elems) => {
                HirType::Tuple(elems.iter().map(|e| self.concretize_type(e)).collect())
            }
            HirType::Func(params, ret) => HirType::Func(
                params.iter().map(|p| self.concretize_type(p)).collect(),
                Box::new(self.concretize_type(ret)),
            ),
        }
    }

    pub(crate) fn queue_fn_specialization(&mut self, name: Symbol, args: Vec<HirType>) {
        let args = self.concretize_type_args(&args);
        if args.iter().any(|a| self.has_unresolved_type_param(a)) {
            return;
        }
        let key = (name, args);
        if self.fn_specs.contains_key(&key) || self.fn_queued.contains(&key) {
            return;
        }
        self.fn_queued.insert(key.clone());
        self.fn_work_queue.push(key);
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn scan_expr_for_generic_calls(
        &mut self,
        expr: &HirExpr,
        current_sub: &HashMap<Symbol, HirType>,
    ) {
        match expr {
            HirExpr::Call { id, callee, args, .. } => {
                let callee_name = self.interner.resolve(*callee).to_string();
                let fn_def_opt = self.find_fn(*callee);
                if let Some(ref fn_def) = fn_def_opt {
                    if !fn_def.type_params.is_empty() {
                        eprintln!("[mono scan] Call callee={} found fn_def with {} type_params", callee_name, fn_def.type_params.len());
                        if let Some(type_args) = self.call_type_args.get(&expr.get_id()) {
                            let substituted = self.substitute_type_args(type_args, current_sub);
                            let concrete_args = self.concretize_type_args(&substituted);
                            if !concrete_args.is_empty()
                                && !concrete_args
                                    .iter()
                                    .any(|a| self.has_unresolved_type_param(a))
                            {
                                self.queue_fn_specialization(*callee, concrete_args);
                            }
                        } else {
                            let mut sub = HashMap::new();
                            for (param_idx, (_, param_ty)) in fn_def.params.iter().enumerate() {
                                if let Some(arg_expr) = args.get(param_idx) {
                                    let arg_ty = self
                                        .get_expr_type(arg_expr.get_id())
                                        .unwrap_or(HirType::Never);
                                    if arg_ty != HirType::Never {
                                        Self::extract_type_substitutions(
                                            param_ty,
                                            &arg_ty,
                                            &fn_def.type_params,
                                            &mut sub,
                                        );
                                    }
                                }
                            }
                            if sub.len() == fn_def.type_params.len() {
                                let concrete: Vec<HirType> = fn_def
                                    .type_params
                                    .iter()
                                    .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                    .collect();
                                if !concrete.iter().any(|a| self.has_unresolved_type_param(a)) {
                                    self.queue_fn_specialization(*callee, concrete);
                                }
                            }

                            // FALLBACK: try to infer type args from later calls on the same variable
                            if sub.is_empty() && args.is_empty() {
                                if let Some(concrete) = self.infer_from_same_var_in_block(
                                    callee,
                                    expr.get_id(),
                                    &fn_def.type_params,
                                ) {
                                    self.call_type_args_overrides
                                        .insert(expr.get_id(), concrete.clone());
                                    self.queue_fn_specialization(*callee, concrete);
                                }
                            }

                            // SAFE FALLBACK (inner): inside else block for no call_type_args
                            if sub.is_empty()
                                && !self.body_depends_on_type_params(&fn_def.body, &fn_def.type_params)
                            {
                                let concrete: Vec<HirType> =
                                    fn_def.type_params.iter().map(|_| HirType::Int).collect();
                                eprintln!("[mono scan] SAFE FALLBACK (inner): queueing {} with [Int]", callee_name);
                                self.call_type_args_overrides.insert(expr.get_id(), concrete.clone());
                                self.queue_fn_specialization(*callee, concrete);
                            }
                        }
                    }
                }
                // SAFE FALLBACK (outer): runs when call_type_args existed but were unresolved
                if let Some(ref fn_def) = fn_def_opt {
                    if !fn_def.type_params.is_empty()
                        && self.call_type_args.get(&expr.get_id()).is_some()
                        && !self.fn_queued.contains(&(*callee, vec![HirType::Int]))
                    {
                        if !self.body_depends_on_type_params(&fn_def.body, &fn_def.type_params) {
                            let concrete: Vec<HirType> =
                                fn_def.type_params.iter().map(|_| HirType::Int).collect();
                            eprintln!("[mono scan] SAFE FALLBACK (outer): queueing {} with [Int]", callee_name);
                            self.call_type_args_overrides.insert(expr.get_id(), concrete.clone());
                            self.queue_fn_specialization(*callee, concrete);
                        }
                    }
                }
                for a in args {
                    self.scan_expr_for_generic_calls(a, current_sub);
                }
            }
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                ..
            } => {
                // Check for explicit type arguments first
                if let Some(type_args) = self.call_type_args.get(id) {
                    let concrete_args = self.substitute_type_args(type_args, current_sub);
                    if !concrete_args.is_empty()
                        && !concrete_args
                            .iter()
                            .any(|a| self.has_unresolved_type_param(a))
                    {
                        let receiver_ty = self.get_expr_type(receiver.get_id());
                        let base_type = match receiver_ty.as_ref() {
                            Some(HirType::Named(name)) => *name,
                            Some(HirType::Generic(name, _)) => *name,
                            _ => {
                                self.scan_expr_for_generic_calls(receiver, current_sub);
                                for a in args {
                                    self.scan_expr_for_generic_calls(a, current_sub);
                                }
                                return;
                            }
                        };
                        let mangled = format!(
                            "{}_{}",
                            self.interner.resolve(base_type),
                            self.interner.resolve(*method_name)
                        );
                        let mangled_sym = self.interner.intern(&mangled);
                        if self.find_fn(mangled_sym).is_some() {
                            self.queue_fn_specialization(mangled_sym, concrete_args);
                        }
                    }
                }

                // Try to infer from receiver type
                let receiver_ty = self.get_expr_type(receiver.get_id());
                if let Some(HirType::Generic(type_name, type_args)) = receiver_ty {
                    let mangled = format!(
                        "{}_{}",
                        self.interner.resolve(type_name),
                        self.interner.resolve(*method_name)
                    );
                    let mangled_sym = self.interner.intern(&mangled);
                    let concrete_args = self.substitute_type_args(&type_args, current_sub);
                    if self.find_fn(mangled_sym).is_some()
                        && !concrete_args.is_empty()
                        && !concrete_args
                            .iter()
                            .any(|a| self.has_unresolved_type_param(a))
                    {
                        self.queue_fn_specialization(mangled_sym, concrete_args);
                    }
                }
                self.scan_expr_for_generic_calls(receiver, current_sub);
                for a in args {
                    self.scan_expr_for_generic_calls(a, current_sub);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => self.scan_expr_for_generic_calls(e, current_sub),
                        HirStmt::Let { value, .. }
                        | HirStmt::Assign { value, .. }
                        | HirStmt::AssignField { value, .. } => {
                            self.scan_expr_for_generic_calls(value, current_sub)
                        }
                        HirStmt::LetPat {
                            pattern: _,
                            mutable: _,
                            value,
                            ty,
                            ..
                        } => {
                            if let Some(HirType::Generic(_, type_args)) = ty {
                                if let HirExpr::Call { id, .. } = value {
                                    let concrete =
                                        self.substitute_type_args(type_args, current_sub);
                                    if !concrete.iter().any(|a| self.has_unresolved_type_param(a)) {
                                        self.call_type_args_overrides.insert(*id, concrete);
                                    }
                                }
                            }
                            self.scan_expr_for_generic_calls(value, current_sub)
                        }
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.scan_expr_for_generic_calls(target, current_sub);
                            self.scan_expr_for_generic_calls(value, current_sub);
                        }
                    }
                }
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.scan_expr_for_generic_calls(condition, current_sub);
                self.scan_expr_for_generic_calls(then_branch, current_sub);
                if let Some(e) = else_branch {
                    self.scan_expr_for_generic_calls(e, current_sub);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.scan_expr_for_generic_calls(scrutinee, current_sub);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.scan_expr_for_generic_calls(g, current_sub);
                    }
                    self.scan_expr_for_generic_calls(body, current_sub);
                }
            }
            HirExpr::While {
                condition, body, ..
            } => {
                self.scan_expr_for_generic_calls(condition, current_sub);
                self.scan_expr_for_generic_calls(body, current_sub);
            }
            HirExpr::ForIn { iter, body, .. } => {
                self.scan_expr_for_generic_calls(iter, current_sub);
                self.scan_expr_for_generic_calls(body, current_sub);
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.scan_expr_for_generic_calls(lhs, current_sub);
                self.scan_expr_for_generic_calls(rhs, current_sub);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. } => {
                self.scan_expr_for_generic_calls(operand, current_sub)
            }
            HirExpr::Return { value: Some(v), .. } => {
                self.scan_expr_for_generic_calls(v, current_sub)
            }
            HirExpr::StructLit {
                struct_name,
                fields,
                ..
            } => {
                for (field_sym, f) in fields {
                    if let HirExpr::Call { callee, args, .. } = f {
                        if args.is_empty() {
                            self.try_infer_call_from_struct_field(
                                *callee,
                                *struct_name,
                                *field_sym,
                                current_sub,
                            );
                        }
                    }
                    self.scan_expr_for_generic_calls(f, current_sub);
                }
            }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                for a in args {
                    self.scan_expr_for_generic_calls(a, current_sub);
                }
            }
            _ => {}
        }
    }

    fn extract_type_substitutions(
        param_ty: &HirType,
        arg_ty: &HirType,
        type_params: &[Symbol],
        sub: &mut HashMap<Symbol, HirType>,
    ) {
        match (param_ty, arg_ty) {
            (HirType::Named(param_sym), at)
                if type_params.contains(param_sym) && *at != HirType::Never =>
            {
                sub.insert(*param_sym, at.clone());
            }
            (HirType::RawPtr(inner_param), HirType::RawPtr(inner_arg)) => {
                Self::extract_type_substitutions(inner_param, inner_arg, type_params, sub);
            }
            (HirType::Generic(p_sym, p_args), HirType::Generic(a_sym, a_args))
                if p_sym == a_sym && p_args.len() == a_args.len() =>
            {
                for (p, a) in p_args.iter().zip(a_args.iter()) {
                    Self::extract_type_substitutions(p, a, type_params, sub);
                }
            }
            (HirType::Tuple(p_elems), HirType::Tuple(a_elems))
                if p_elems.len() == a_elems.len() =>
            {
                for (p, a) in p_elems.iter().zip(a_elems.iter()) {
                    Self::extract_type_substitutions(p, a, type_params, sub);
                }
            }
            _ => {}
        }
    }

    /// Try to infer concrete type args for a zero‑argument generic call based on
    /// the expected type of a struct field.
    fn try_infer_call_from_struct_field(
        &mut self,
        callee: Symbol,
        struct_name: Symbol,
        field_sym: Symbol,
        current_sub: &HashMap<Symbol, HirType>,
    ) {
        if let Some(fn_def) = self.find_fn(callee) {
            if fn_def.type_params.is_empty() {
                return;
            }
            if let Some(struct_def) = self.find_struct(struct_name) {
                if let Some(field) = struct_def.fields.iter().find(|f| f.name == field_sym) {
                    let field_ty = crate::types::substitute_type(&field.ty, current_sub);
                    if let Some(ret_ty) = &fn_def.ret {
                        let mut sub = HashMap::new();
                        Self::extract_type_substitutions(
                            ret_ty,
                            &field_ty,
                            &fn_def.type_params,
                            &mut sub,
                        );
                        if sub.len() == fn_def.type_params.len() {
                            let concrete: Vec<HirType> = fn_def
                                .type_params
                                .iter()
                                .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int))
                                .collect();
                            self.queue_fn_specialization(callee, concrete);
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn scan_expr_for_struct_instantiations(
        &mut self,
        expr: &HirExpr,
        current_sub: &HashMap<Symbol, HirType>,
    ) {
        match expr {
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                ..
            } => {
                if let Some(struct_def) = self.find_struct(*struct_name)
                    && !struct_def.type_params.is_empty()
                {
                    let struct_ty = self.get_expr_type(*id);
                    let concrete_args = match struct_ty.as_ref() {
                        Some(HirType::Generic(_, type_args)) => {
                            self.substitute_type_args(type_args, current_sub)
                        }
                        _ => vec![],
                    };
                    if concrete_args.len() == struct_def.type_params.len()
                        && !concrete_args
                            .iter()
                            .any(|a| self.has_unresolved_type_param(a))
                    {
                        let key = (*struct_name, concrete_args.clone());
                        if !self.struct_specs.contains_key(&key) {
                            let specialized = self.specialize_struct(&struct_def, &concrete_args);
                            self.struct_specs.insert(key, specialized);
                        }
                        let mangled = self.mangle_name(*struct_name, &concrete_args);
                        self.type_overrides.insert(*id, HirType::Named(mangled));
                    }
                }
                for (_, f) in fields {
                    self.scan_expr_for_struct_instantiations(f, current_sub);
                }
            }
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    match s {
                        HirStmt::Expr(e) => {
                            self.scan_expr_for_struct_instantiations(e, current_sub)
                        }
                        HirStmt::Let { value, .. }
                        | HirStmt::LetPat { value, .. }
                        | HirStmt::Assign { value, .. }
                        | HirStmt::AssignField { value, .. } => {
                            self.scan_expr_for_struct_instantiations(value, current_sub)
                        }
                        HirStmt::AssignDeref { target, value, .. } => {
                            self.scan_expr_for_struct_instantiations(target, current_sub);
                            self.scan_expr_for_struct_instantiations(value, current_sub);
                        }
                    }
                }
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.scan_expr_for_struct_instantiations(condition, current_sub);
                self.scan_expr_for_struct_instantiations(then_branch, current_sub);
                if let Some(e) = else_branch {
                    self.scan_expr_for_struct_instantiations(e, current_sub);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.scan_expr_for_struct_instantiations(scrutinee, current_sub);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.scan_expr_for_struct_instantiations(g, current_sub);
                    }
                    self.scan_expr_for_struct_instantiations(body, current_sub);
                }
            }
            HirExpr::While {
                condition, body, ..
            } => {
                self.scan_expr_for_struct_instantiations(condition, current_sub);
                self.scan_expr_for_struct_instantiations(body, current_sub);
            }
            HirExpr::ForIn { iter, body, .. } => {
                self.scan_expr_for_struct_instantiations(iter, current_sub);
                self.scan_expr_for_struct_instantiations(body, current_sub);
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.scan_expr_for_struct_instantiations(lhs, current_sub);
                self.scan_expr_for_struct_instantiations(rhs, current_sub);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::As { expr: operand, .. } => {
                self.scan_expr_for_struct_instantiations(operand, current_sub)
            }
            HirExpr::Return { value: Some(v), .. } => {
                self.scan_expr_for_struct_instantiations(v, current_sub)
            }
            HirExpr::MethodCall { receiver, args, .. } => {
                self.scan_expr_for_struct_instantiations(receiver, current_sub);
                for a in args {
                    self.scan_expr_for_struct_instantiations(a, current_sub);
                }
            }
            HirExpr::Call { args, .. } => {
                for a in args {
                    self.scan_expr_for_struct_instantiations(a, current_sub);
                }
            }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                for a in args {
                    self.scan_expr_for_struct_instantiations(a, current_sub);
                }
            }
            HirExpr::SizeOf { target_type, .. } => {
                if let HirType::Generic(sym, args) = target_type {
                    if let Some(s) = self.find_struct(*sym) {
                        let concrete_args = self.substitute_type_args(args, current_sub);
                        let key = (*sym, concrete_args.clone());
                        if !self.struct_specs.contains_key(&key) {
                            let specialized = self.specialize_struct(&s, &concrete_args);
                            self.struct_specs.insert(key, specialized);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
