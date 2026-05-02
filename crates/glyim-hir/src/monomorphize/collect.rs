use super::*;
use crate::item::HirItem;
use crate::node::{HirExpr, HirStmt};
use crate::types::{ExprId, HirType};
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn collect_and_specialize(&mut self) {
        let resolved_args: HashMap<ExprId, Vec<HirType>> = self
            .call_type_args
            .iter()
            .map(|(id, args)| {
                let resolved: Vec<HirType> = args.iter().map(|ty| match ty {
                    HirType::Named(sym) => {
                        let name = self.interner.resolve(*sym);
                        if name.len() == 1 && name.chars().next().unwrap().is_uppercase() {
                            HirType::Int
                        } else { ty.clone() }
                    }
                    _ => ty.clone(),
                }).collect();
                (*id, resolved)
            }).collect();

        for (expr_id, type_args) in resolved_args.iter() {
            for item in &self.hir.items {
                match item {
                    HirItem::Fn(f) => {
                        if let Some(callee) = self.find_callee_by_id(&f.body, *expr_id) {
                            self.queue_fn_specialization(callee, type_args.clone());
                        }
                    }
                    HirItem::Impl(imp) => {
                        for m in &imp.methods {
                            if let Some(callee) = self.find_callee_by_id(&m.body, *expr_id) {
                                self.queue_fn_specialization(callee, type_args.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for item in &self.hir.items {
            if let HirItem::Fn(f) = item {
                self.current_type_params = f.type_params.clone();
                self.scan_expr_for_generic_calls(&f.body);
                self.scan_expr_for_struct_instantiations(&f.body);
            }
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    self.current_type_params = m.type_params.clone();
                    self.scan_expr_for_generic_calls(&m.body);
                    self.scan_expr_for_struct_instantiations(&m.body);
                }
            }
        }
        self.current_type_params = vec![];

        while let Some((fn_name, type_args)) = self.fn_work_queue.pop() {
            let key = (fn_name, type_args.clone());
            if self.fn_specs.contains_key(&key) { continue; }
            if let Some(generic_fn) = self.find_fn(fn_name) {
                let specialized = self.specialize_fn(&generic_fn, &type_args);
                self.current_type_params = vec![];
                self.scan_expr_for_generic_calls(&specialized.body);
                self.scan_expr_for_struct_instantiations(&specialized.body);
                self.fn_specs.insert(key.clone(), specialized.clone());
            }
        }
    }

    pub(crate) fn find_callee_by_id(&mut self, expr: &HirExpr, search_id: ExprId) -> Option<Symbol> {
        match expr {
            HirExpr::Call { id, callee, .. } if *id == search_id => Some(*callee),
            HirExpr::MethodCall { id, receiver, method_name, .. } if *id == search_id => {
                let receiver_ty = self.expr_types.get(receiver.get_id().as_usize());
                let inner_ty = match receiver_ty {
                    Some(HirType::RawPtr(inner)) => Some(inner.as_ref().clone()),
                    other => other.cloned(),
                };
                if let Some(HirType::Named(type_name) | HirType::Generic(type_name, _)) = inner_ty {
                    let mangled = format!("{}_{}", self.interner.resolve(type_name), self.interner.resolve(*method_name));
                    Some(self.interner.intern(&mangled))
                } else { None }
            }
            HirExpr::Block { stmts, .. } => stmts.iter().find_map(|s| match s {
                HirStmt::Expr(e) => self.find_callee_by_id(e, search_id),
                HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } | HirStmt::Assign { value, .. } | HirStmt::AssignField { value, .. } => self.find_callee_by_id(value, search_id),
                HirStmt::AssignDeref { target, value, .. } => self.find_callee_by_id(target, search_id).or_else(|| self.find_callee_by_id(value, search_id)),
            }),
            HirExpr::If { condition, then_branch, else_branch, .. } => self.find_callee_by_id(condition, search_id).or_else(|| self.find_callee_by_id(then_branch, search_id)).or_else(|| else_branch.as_ref().and_then(|e| self.find_callee_by_id(e, search_id))),
            HirExpr::Match { scrutinee, arms, .. } => self.find_callee_by_id(scrutinee, search_id).or_else(|| {
                arms.iter().find_map(|(_, guard, body)| guard.as_ref().and_then(|g| self.find_callee_by_id(g, search_id)).or_else(|| self.find_callee_by_id(body, search_id)))
            }),
            HirExpr::Binary { lhs, rhs, .. } => self.find_callee_by_id(lhs, search_id).or_else(|| self.find_callee_by_id(rhs, search_id)),
            HirExpr::Unary { operand, .. } => self.find_callee_by_id(operand, search_id),
            HirExpr::Return { value: Some(v), .. } => self.find_callee_by_id(v, search_id),
            HirExpr::Deref { expr, .. } => self.find_callee_by_id(expr, search_id),
            HirExpr::While { condition, body, .. } => self.find_callee_by_id(condition, search_id).or_else(|| self.find_callee_by_id(body, search_id)),
            HirExpr::ForIn { iter, body, .. } => self.find_callee_by_id(iter, search_id).or_else(|| self.find_callee_by_id(body, search_id)),
            _ => None,
        }
    }

    pub(crate) fn scan_expr_for_struct_instantiations(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::StructLit { id, struct_name, fields, .. } => {
                if let Some(struct_def) = self.find_struct(*struct_name) {
                    if !struct_def.type_params.is_empty() {
                        let field_types: Vec<HirType> = fields.iter().map(|(_, f)| self.expr_types.get(f.get_id().as_usize()).cloned().unwrap_or(HirType::Never)).collect();
                        let mut sub = HashMap::new();
                        for (i, tp) in struct_def.type_params.iter().enumerate() {
                            if let Some(ft) = struct_def.fields.get(i) {
                                if let HirType::Named(param_sym) = &ft.ty {
                                    if let Some(val_ty) = field_types.get(i) {
                                        if *param_sym == *tp && *val_ty != HirType::Never { sub.insert(*tp, val_ty.clone()); }
                                    }
                                }
                            }
                        }
                        if sub.len() == struct_def.type_params.len() && !sub.is_empty() {
                            let concrete: Vec<HirType> = struct_def.type_params.iter().map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int)).collect();
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
                for (_, f) in fields { self.scan_expr_for_struct_instantiations(f); }
            }
            HirExpr::Block { stmts, .. } => for s in stmts { match s {
                HirStmt::Expr(e) => self.scan_expr_for_struct_instantiations(e),
                HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } | HirStmt::Assign { value, .. } | HirStmt::AssignField { value, .. } => self.scan_expr_for_struct_instantiations(value),
                HirStmt::AssignDeref { target, value, .. } => { self.scan_expr_for_struct_instantiations(target); self.scan_expr_for_struct_instantiations(value); }
            }},
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.scan_expr_for_struct_instantiations(condition);
                self.scan_expr_for_struct_instantiations(then_branch);
                if let Some(e) = else_branch { self.scan_expr_for_struct_instantiations(e); }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.scan_expr_for_struct_instantiations(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard { self.scan_expr_for_struct_instantiations(g); }
                    self.scan_expr_for_struct_instantiations(body);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => { self.scan_expr_for_struct_instantiations(lhs); self.scan_expr_for_struct_instantiations(rhs); }
            HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } | HirExpr::As { expr: operand, .. } => self.scan_expr_for_struct_instantiations(operand),
            HirExpr::Return { value: Some(v), .. } => self.scan_expr_for_struct_instantiations(v),
            HirExpr::Return { value: None, .. } => {}
            HirExpr::MethodCall { receiver, args, .. } => { self.scan_expr_for_struct_instantiations(receiver); for a in args { self.scan_expr_for_struct_instantiations(a); } }
            HirExpr::Call { args, .. } => for a in args { self.scan_expr_for_struct_instantiations(a); }
            HirExpr::While { condition, body, .. } => { self.scan_expr_for_struct_instantiations(condition); self.scan_expr_for_struct_instantiations(body); }
            HirExpr::ForIn { iter, body, .. } => { self.scan_expr_for_struct_instantiations(iter); self.scan_expr_for_struct_instantiations(body); }
            _ => {}
        }
    }

    pub(crate) fn queue_fn_specialization(&mut self, name: Symbol, args: Vec<HirType>) {
        let key = (name, args);
        if self.fn_specs.contains_key(&key) || self.fn_queued.contains(&key) { return; }
        self.fn_queued.insert(key.clone());
        self.fn_work_queue.push(key);
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn scan_expr_for_generic_calls(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::Call { id: call_id, callee, args, .. } => {
                if let Some(fn_def) = self.find_fn(*callee) {
                    if !fn_def.type_params.is_empty() {
                        if args.is_empty() {
                            let concrete: Vec<HirType> = fn_def.type_params.iter().map(|_| HirType::Int).collect();
                            self.inferred_call_args.insert(*call_id, concrete.clone());
                            self.queue_fn_specialization(*callee, concrete);
                            return;
                        }
                        let arg_types: Vec<HirType> = args.iter().map(|a| self.expr_types.get(a.get_id().as_usize()).cloned().unwrap_or(HirType::Never)).collect();
                        let mut sub = HashMap::new();
                        for (param_idx, (_, param_ty)) in fn_def.params.iter().enumerate() {
                            if let HirType::Named(param_sym) = param_ty {
                                if fn_def.type_params.contains(param_sym) {
                                    if let Some(at) = arg_types.get(param_idx) {
                                        if *at != HirType::Never { sub.insert(*param_sym, at.clone()); }
                                    }
                                }
                            }
                        }
                        if sub.len() == fn_def.type_params.len() {
                            let concrete: Vec<HirType> = fn_def.type_params.iter().map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Int)).collect();
                            self.queue_fn_specialization(*callee, concrete);
                        }
                    }
                }
                for a in args { self.scan_expr_for_generic_calls(a); }
            }
            HirExpr::MethodCall { receiver, method_name, args, .. } => {
                let receiver_ty = self.expr_types.get(receiver.get_id().as_usize());
                if let Some(HirType::Generic(type_name, type_args)) = receiver_ty {
                    let mangled = format!("{}_{}", self.interner.resolve(*type_name), self.interner.resolve(*method_name));
                    let mangled_sym = self.interner.intern(&mangled);
                    let has_impl = self.find_fn(mangled_sym).is_some();
                    let concrete_args: Vec<HirType> = type_args.iter().map(|ta| {
                        let name_str = self.interner.resolve(match ta { HirType::Named(s) => *s, _ => return ta.clone() }).to_string();
                        if name_str.len() == 1 && name_str.chars().next().unwrap().is_uppercase() { HirType::Int } else { ta.clone() }
                    }).collect();
                    if has_impl && !concrete_args.is_empty() { self.queue_fn_specialization(mangled_sym, concrete_args); }
                }
                self.scan_expr_for_generic_calls(receiver);
                for a in args { self.scan_expr_for_generic_calls(a); }
            }
            HirExpr::Block { stmts, .. } => for s in stmts { match s {
                HirStmt::Expr(e) => self.scan_expr_for_generic_calls(e),
                HirStmt::Let { value, .. } | HirStmt::LetPat { value, .. } | HirStmt::Assign { value, .. } | HirStmt::AssignField { value, .. } => self.scan_expr_for_generic_calls(value),
                HirStmt::AssignDeref { target, value, .. } => { self.scan_expr_for_generic_calls(target); self.scan_expr_for_generic_calls(value); }
            }},
            HirExpr::If { condition, then_branch, else_branch, .. } => {
                self.scan_expr_for_generic_calls(condition);
                self.scan_expr_for_generic_calls(then_branch);
                if let Some(e) = else_branch { self.scan_expr_for_generic_calls(e); }
            }
            HirExpr::Match { scrutinee, arms, .. } => {
                self.scan_expr_for_generic_calls(scrutinee);
                for (_, guard, body) in arms {
                    if let Some(g) = guard { self.scan_expr_for_generic_calls(g); }
                    self.scan_expr_for_generic_calls(body);
                }
            }
            HirExpr::While { condition, body, .. } => { self.scan_expr_for_generic_calls(condition); self.scan_expr_for_generic_calls(body); }
            HirExpr::ForIn { iter, body, .. } => { self.scan_expr_for_generic_calls(iter); self.scan_expr_for_generic_calls(body); }
            HirExpr::Binary { lhs, rhs, .. } => { self.scan_expr_for_generic_calls(lhs); self.scan_expr_for_generic_calls(rhs); }
            HirExpr::Unary { operand, .. } | HirExpr::Deref { expr: operand, .. } | HirExpr::As { expr: operand, .. } => self.scan_expr_for_generic_calls(operand),
            HirExpr::Return { value: Some(v), .. } => self.scan_expr_for_generic_calls(v),
            HirExpr::StructLit { struct_name, fields, .. } => {
                if let Some(struct_def) = self.find_struct(*struct_name) {
                    for (field_sym, field_expr) in fields {
                        if let Some(field_def) = struct_def.fields.iter().find(|f| f.name == *field_sym) {
                            if let HirExpr::Call { id: call_id, callee, args, .. } = field_expr {
                                if args.is_empty() {
                                    if let Some(fn_def) = self.find_fn(*callee) {
                                        if !fn_def.type_params.is_empty() {
                                            let concrete: Vec<HirType> = match &field_def.ty {
                                                HirType::Generic(_, type_args) => type_args.clone(),
                                                _ => fn_def.type_params.iter().map(|_| HirType::Int).collect(),
                                            };
                                            if !concrete.is_empty() {
                                                self.inferred_call_args.insert(*call_id, concrete.clone());
                                                self.queue_fn_specialization(*callee, concrete);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        self.scan_expr_for_generic_calls(field_expr);
                    }
                } else {
                    for (_, field_expr) in fields { self.scan_expr_for_generic_calls(field_expr); }
                }
            }
            _ => {}
        }
    }
}
