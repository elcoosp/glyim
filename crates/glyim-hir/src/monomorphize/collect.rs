use super::*;
use crate::item::HirItem;
use crate::node::{HirExpr, HirStmt};
use crate::types::{ExprId, HirType};
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn collect_and_specialize(&mut self) {
        self.init_method_map();
        eprintln!("[mono] call_type_args count: {}", self.call_type_args.len());
        for (k, v) in self.call_type_args.iter() {
            eprintln!("  id={:?} args={:?}", k, v);
        }
        // Phase 1: function specializations from call_type_args
        for (expr_id, type_args) in self.call_type_args.iter() {
            if type_args.iter().any(|a| self.has_unresolved_type_param(a)) {
                continue;
            }
            eprintln!(
                "[mono Phase1] expr_id={:?} type_args={:?}",
                expr_id, type_args
            );

            if let Some(callee) = self.find_callee_by_id_from_hir(*expr_id) {
                if self.interner.resolve(callee).contains("__") {
                    continue;
                }
                self.queue_fn_specialization(callee, type_args.clone());
            }
        }

        // Phase 2: concrete types from expr_types and type_overrides
        for ty in self.expr_types.iter() {
            self.enqueue_type_if_generic(ty);
        }
        let overrides: Vec<HirType> = self.type_overrides.values().cloned().collect();
        for ty in &overrides {
            self.enqueue_type_if_generic(ty);
        }

        self.process_type_specializations();

        // Phase 3: work queue
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
                self.collect_type_overrides_for_expr(&specialized.body, &sub);
                self.fn_specs.insert(key, specialized);
                for (_, param_ty) in &generic_fn.params {
                    let c = crate::types::substitute_type(param_ty, &sub);
                    self.enqueue_type_if_generic(&c);
                }
                if let Some(ret_ty) = &generic_fn.ret {
                    let c = crate::types::substitute_type(ret_ty, &sub);
                    self.enqueue_type_if_generic(&c);
                }
            }
        }

        while !self.type_work_queue.is_empty() {
            self.process_type_specializations();
        }
    }

    // ── find_callee helpers ──

    fn find_callee_by_id_from_hir(&mut self, search_id: ExprId) -> Option<Symbol> {
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) => {
                    if let Some(c) = Self::find_callee_in_expr(&f.body, search_id, self) {
                        return Some(c);
                    }
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if let Some(c) = Self::find_callee_in_expr(&m.body, search_id, self) {
                            return Some(c);
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
        eprintln!("[find_callee_in_expr] searching for id={:?} in expr={:?}", search_id, expr);

        match expr {
            HirExpr::Call { id, callee, .. } => {
                eprintln!("[find_callee_in_expr] Call id={:?} callee={} (search_id={:?})", id, ctx.interner.resolve(*callee), search_id);
                if *id == search_id { return Some(*callee); }
                None
            }
            HirExpr::MethodCall { id, receiver, method_name, .. } => {
                eprintln!("[find_callee_in_expr] MethodCall id={:?} (search_id={:?})", id, search_id);
                if *id != search_id { return None; }
                let receiver_ty = ctx.get_expr_type(receiver.get_id());
                let inner = match receiver_ty {
                    Some(HirType::RawPtr(i)) => Some(i.as_ref().clone()),
                    other => other,
                };
                match inner {
                    Some(HirType::Named(n)) => Some(ctx.interner.intern(&format!(
                        "{}_{}",
                        ctx.interner.resolve(n),
                        ctx.interner.resolve(*method_name)
                    ))),
                    Some(HirType::Generic(n, _)) => Some(ctx.interner.intern(&format!(
                        "{}_{}",
                        ctx.interner.resolve(n),
                        ctx.interner.resolve(*method_name)
                    ))),
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
                arms.iter().find_map(|arm| {
                    arm.guard
                        .as_ref()
                        .and_then(|g| Self::find_callee_in_expr(g, search_id, ctx))
                        .or_else(|| Self::find_callee_in_expr(&arm.body, search_id, ctx))
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

    // ── concretisation ──

    pub(crate) fn concretize_type_args(&mut self, args: &[HirType]) -> Vec<HirType> {
        args.iter().map(|ty| self.concretize_type(ty)).collect()
    }

    pub(crate) fn concretize_type(&mut self, ty: &HirType) -> HirType {
        match ty {
            HirType::Generic(sym, inner) => {
                let inner: Vec<HirType> = inner.iter().map(|a| self.concretize_type(a)).collect();
                let all_concrete = inner.iter().all(|a| !self.has_unresolved_type_param(a));
                if all_concrete {
                    let key = (*sym, inner.clone());
                    if self.struct_specs.contains_key(&key) || self.enum_specs.contains_key(&key) {
                        return HirType::Named(self.mangle_name(*sym, &inner));
                    }
                }
                HirType::Generic(*sym, inner)
            }
            HirType::Named(_)
            | HirType::Int
            | HirType::Bool
            | HirType::Float
            | HirType::Str
            | HirType::Unit
            | HirType::Never
            | HirType::Error
            | HirType::Opaque(_) => ty.clone(),
            HirType::RawPtr(inner) => HirType::RawPtr(Box::new(self.concretize_type(inner))),
            HirType::Option(inner) => {
                let inner = self.concretize_type(inner);
                if !self.has_unresolved_type_param(&inner) {
                    let opt = self.interner.intern("Option");
                    if self.enum_specs.contains_key(&(opt, vec![inner.clone()])) {
                        return HirType::Named(self.mangle_name(opt, &[inner]));
                    }
                }
                HirType::Option(Box::new(inner))
            }
            HirType::Result(ok, err) => {
                let ok = self.concretize_type(ok);
                let err = self.concretize_type(err);
                if !self.has_unresolved_type_param(&ok) && !self.has_unresolved_type_param(&err) {
                    let res = self.interner.intern("Result");
                    let key = (res, vec![ok.clone(), err.clone()]);
                    if self.enum_specs.contains_key(&key) {
                        return HirType::Named(self.mangle_name(res, &[ok, err]));
                    }
                }
                HirType::Result(Box::new(ok), Box::new(err))
            }
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
    // ── scanning for further specialisations (needed by specialize_fn) ──

    #[tracing::instrument(skip_all)]
    pub(crate) fn scan_expr_for_generic_calls(
        &mut self,
        expr: &HirExpr,
        current_sub: &HashMap<Symbol, HirType>,
    ) {
        match expr {
            HirExpr::Call { callee, args, .. } => {
                // Mangled names: just walk args (typechecker handles them now)
                if self.interner.resolve(*callee).contains("__") {
                    for a in args {
                        self.scan_expr_for_generic_calls(a, current_sub);
                    }
                    return;
                }
                if let Some(ref _fn_def) =
                    self.find_fn(*callee).filter(|f| !f.type_params.is_empty())
                {
                    if let Some(type_args) = self.call_type_args.get(&expr.get_id()) {
                        let substituted = self.substitute_type_args(type_args, current_sub);
                        let concrete = self.concretize_type_args(&substituted);
                        if !concrete.is_empty()
                            && concrete.iter().all(|a| !self.has_unresolved_type_param(a))
                        {
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
                eprintln!(
                    "[mono scan MethodCall] id={:?} call_type_args.get(id)={:?}",
                    id,
                    self.call_type_args.get(id)
                );

                if let Some(concrete) = self.call_type_args.get(id).cloned() {
                    let concrete = self.substitute_type_args(&concrete, current_sub);
                    if !concrete.is_empty()
                        && concrete.iter().all(|a| !self.has_unresolved_type_param(a))
                    {
                        if let Some(base) = match self.get_expr_type(receiver.get_id()) {
                            Some(HirType::Named(n) | HirType::Generic(n, _)) => Some(n),
                            _ => None,
                        } {
                            let mangled = self.interner.intern(&format!(
                                "{}_{}",
                                self.interner.resolve(base),
                                self.interner.resolve(*method_name)
                            ));
                            if self.find_fn(mangled).is_some() {
                                self.queue_fn_specialization(mangled, concrete);
                            }
                        }
                    }
                }
                // fallback: infer from receiver generic type
                if let Some(HirType::Generic(type_name, type_args)) =
                    self.get_expr_type(receiver.get_id())
                {
                    let concrete = self.substitute_type_args(&type_args, current_sub);
                    let mangled = self.interner.intern(&format!(
                        "{}_{}",
                        self.interner.resolve(type_name),
                        self.interner.resolve(*method_name)
                    ));
                    if self.find_fn(mangled).is_some()
                        && !concrete.is_empty()
                        && concrete.iter().all(|a| !self.has_unresolved_type_param(a))
                    {
                        self.queue_fn_specialization(mangled, concrete);
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
                        | HirStmt::AssignField { value, .. }
                        | HirStmt::LetPat { value, .. } => {
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
                for arm in arms {
                    if let Some(g) = &arm.guard {
                        self.scan_expr_for_generic_calls(g, current_sub);
                    }
                    self.scan_expr_for_generic_calls(&arm.body, current_sub);
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
            HirExpr::StructLit { fields, .. } => fields
                .iter()
                .for_each(|(_, f)| self.scan_expr_for_generic_calls(f, current_sub)),
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => args
                .iter()
                .for_each(|a| self.scan_expr_for_generic_calls(a, current_sub)),
            _ => {}
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
                if let Some(struct_def) = self
                    .find_struct(*struct_name)
                    .filter(|s| !s.type_params.is_empty())
                {
                    if let Some(HirType::Generic(_, type_args)) = self.get_expr_type(*id) {
                        let concrete = self.substitute_type_args(&type_args, current_sub);
                        if concrete.len() == struct_def.type_params.len()
                            && concrete.iter().all(|a| !self.has_unresolved_type_param(a))
                        {
                            let key = (*struct_name, concrete.clone());
                            if !self.struct_specs.contains_key(&key) {
                                let specialized = self.specialize_struct(&struct_def, &concrete);
                                self.struct_specs.insert(key, specialized);
                            }
                            let mangled = self.mangle_name(*struct_name, &concrete);
                            self.type_overrides.insert(*id, HirType::Named(mangled));
                        }
                    }
                }
                for (_, f) in fields {
                    self.scan_expr_for_struct_instantiations(f, current_sub);
                }
            }
            HirExpr::Block { stmts, .. } => stmts.iter().for_each(|s| match s {
                HirStmt::Expr(e) => self.scan_expr_for_struct_instantiations(e, current_sub),
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
            }),
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
                for arm in arms {
                    if let Some(ref g) = arm.guard {
                        self.scan_expr_for_struct_instantiations(g, current_sub);
                    }
                    self.scan_expr_for_struct_instantiations(&arm.body, current_sub);
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
                args.iter()
                    .for_each(|a| self.scan_expr_for_struct_instantiations(a, current_sub));
            }
            HirExpr::Call { args, .. } => args
                .iter()
                .for_each(|a| self.scan_expr_for_struct_instantiations(a, current_sub)),
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => args
                .iter()
                .for_each(|a| self.scan_expr_for_struct_instantiations(a, current_sub)),
            HirExpr::SizeOf { target_type, .. } => {
                if let HirType::Generic(sym, args) = target_type
                    && let Some(s) = self.find_struct(*sym)
                {
                    let concrete = self.substitute_type_args(args, current_sub);
                    let key = (*sym, concrete.clone());
                    if !self.struct_specs.contains_key(&key) {
                        let specialized = self.specialize_struct(&s, &concrete);
                        self.struct_specs.insert(key, specialized);
                    }
                }
            }
            _ => {}
        }
    }

    // ── type enqueuing ──

    pub(crate) fn enqueue_type_if_generic(&mut self, ty: &HirType) {
        match ty {
            HirType::Generic(sym, args) => {
                let concrete = self.concretize_type_args(args);
                if !concrete.iter().any(|a| self.has_unresolved_type_param(a)) {
                    let key = (*sym, concrete.clone());
                    if !self.type_queued.contains(&key) {
                        self.type_queued.insert(key.clone());
                        self.type_work_queue.push(key);
                    }
                }
                args.iter().for_each(|a| self.enqueue_type_if_generic(a));
            }
            HirType::Option(inner) => {
                let inner = self.concretize_type(inner);
                if !self.has_unresolved_type_param(&inner) {
                    let opt = self.interner.intern("Option");
                    let key = (opt, vec![inner.clone()]);
                    if !self.type_queued.contains(&key) {
                        self.type_queued.insert(key.clone());
                        self.type_work_queue.push(key);
                    }
                }
                self.enqueue_type_if_generic(&inner);
            }
            HirType::Result(ok, err) => {
                let ok = self.concretize_type(ok);
                let err = self.concretize_type(err);
                if !self.has_unresolved_type_param(&ok) && !self.has_unresolved_type_param(&err) {
                    let res = self.interner.intern("Result");
                    let key = (res, vec![ok.clone(), err.clone()]);
                    if !self.type_queued.contains(&key) {
                        self.type_queued.insert(key.clone());
                        self.type_work_queue.push(key);
                    }
                }
                self.enqueue_type_if_generic(&ok);
                self.enqueue_type_if_generic(&err);
            }
            HirType::Named(_)
            | HirType::Int
            | HirType::Bool
            | HirType::Float
            | HirType::Str
            | HirType::Unit
            | HirType::Never
            | HirType::Error
            | HirType::Opaque(_)
            | HirType::RawPtr(_)
            | HirType::Func(_, _) => {
                if let HirType::RawPtr(inner) = ty {
                    self.enqueue_type_if_generic(&inner);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn process_type_specializations(&mut self) {
        while let Some((name, args)) = self.type_work_queue.pop() {
            let key = (name, args.clone());
            if self.struct_specs.contains_key(&key) || self.enum_specs.contains_key(&key) {
                continue;
            }
            if let Some(s) = self.find_struct(name) {
                let specialized = self.specialize_struct(&s, &args);
                for field in &specialized.fields {
                    self.enqueue_type_if_generic(&field.ty);
                }
                self.struct_specs.insert(key, specialized);
            } else if let Some(e) = self.find_enum(name) {
                let specialized = self.specialize_enum(&e, &args);
                for variant in &specialized.variants {
                    for field in &variant.fields {
                        self.enqueue_type_if_generic(&field.ty);
                    }
                }
                self.enum_specs.insert(key, specialized);
            }
        }
        while let Some((name, args)) = self.type_work_queue.pop() {
            let key = (name, args.clone());
            if self.struct_specs.contains_key(&key) || self.enum_specs.contains_key(&key) {
                continue;
            }
            if let Some(s) = self.find_struct(name) {
                let specialized = self.specialize_struct(&s, &args);
                for field in &specialized.fields {
                    self.enqueue_type_if_generic(&field.ty);
                }
                self.struct_specs.insert(key, specialized);
            } else if let Some(e) = self.find_enum(name) {
                let specialized = self.specialize_enum(&e, &args);
                for variant in &specialized.variants {
                    for field in &variant.fields {
                        self.enqueue_type_if_generic(&field.ty);
                    }
                }
                self.enum_specs.insert(key, specialized);
            }
        }
    }
}
