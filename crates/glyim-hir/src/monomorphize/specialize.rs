use super::*;
use crate::node::{HirExpr, HirStmt};
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn specialize_fn(&mut self, f: &HirFn, concrete: &[HirType]) -> HirFn {
        let mut sub = HashMap::new();
        for (i, tp) in f.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        for ct in concrete {
            self.ensure_struct_specialized(ct);
        }
        self.collect_type_overrides_for_expr(&f.body, &sub);
        let mut mono = f.clone();
        mono.type_params.clear();
        for (_, pt) in &mut mono.params {
            *pt = crate::types::substitute_type(pt, &sub);
        }
        if let Some(rt) = &mut mono.ret {
            *rt = crate::types::substitute_type(rt, &sub);
        }
        mono.body = self.substitute_expr_types(&mono.body, &sub);

        // Transitive specialization: re‑scan the substituted body for newly concrete calls.
        self.scan_expr_for_generic_calls(&mono.body);
        self.scan_expr_for_struct_instantiations(&mono.body);

        mono
    }

    pub(crate) fn ensure_struct_specialized(&mut self, ty: &HirType) {
        if let HirType::Generic(sym, args) = ty {
            if self.find_struct(*sym).is_some() {
                let concrete: Vec<HirType> = args.clone();
                let key = (*sym, concrete.clone());
                if !self.struct_specs.contains_key(&key)
                    && let Some(s) = self.find_struct(*sym)
                {
                    let specialized = self.specialize_struct(&s, &concrete);
                    self.struct_specs.insert(key, specialized);
                }
            }
            for arg in args {
                self.ensure_struct_specialized(arg);
            }
        }
    }

    pub(crate) fn collect_type_overrides_for_expr(
        &mut self,
        expr: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
    ) {
        let id = expr.get_id();
        if let Some(original_ty) = self.expr_types.get(id.as_usize()) {
            let new_ty = crate::types::substitute_type(original_ty, sub);
            if new_ty != *original_ty {
                self.type_overrides.insert(id, new_ty);
            }
        }
        match expr {
            HirExpr::Block { stmts, .. } => {
                for s in stmts {
                    self.collect_type_overrides_for_stmt(s, sub);
                }
            }
            HirExpr::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.collect_type_overrides_for_expr(condition, sub);
                self.collect_type_overrides_for_expr(then_branch, sub);
                if let Some(e) = else_branch {
                    self.collect_type_overrides_for_expr(e, sub);
                }
            }
            HirExpr::Match {
                scrutinee, arms, ..
            } => {
                self.collect_type_overrides_for_expr(scrutinee, sub);
                for (_, guard, body) in arms {
                    if let Some(g) = guard {
                        self.collect_type_overrides_for_expr(g, sub);
                    }
                    self.collect_type_overrides_for_expr(body, sub);
                }
            }
            HirExpr::Binary { lhs, rhs, .. } => {
                self.collect_type_overrides_for_expr(lhs, sub);
                self.collect_type_overrides_for_expr(rhs, sub);
            }
            HirExpr::Unary { operand, .. }
            | HirExpr::Deref { expr: operand, .. }
            | HirExpr::FieldAccess {
                object: operand, ..
            }
            | HirExpr::As { expr: operand, .. } => {
                self.collect_type_overrides_for_expr(operand, sub)
            }
            HirExpr::Return { value: Some(v), .. } => self.collect_type_overrides_for_expr(v, sub),
            HirExpr::While {
                condition, body, ..
            }
            | HirExpr::ForIn {
                iter: condition,
                body,
                ..
            } => {
                self.collect_type_overrides_for_expr(condition, sub);
                self.collect_type_overrides_for_expr(body, sub);
            }
            HirExpr::MethodCall { receiver, args, .. } => {
                self.collect_type_overrides_for_expr(receiver, sub);
                for a in args {
                    self.collect_type_overrides_for_expr(a, sub);
                }
            }
            HirExpr::Call { args, .. } => {
                for a in args {
                    self.collect_type_overrides_for_expr(a, sub);
                }
            }
            HirExpr::StructLit { fields, .. } => {
                for (_, f) in fields {
                    self.collect_type_overrides_for_expr(f, sub);
                }
            }
            HirExpr::EnumVariant { args, .. } | HirExpr::TupleLit { elements: args, .. } => {
                for a in args {
                    self.collect_type_overrides_for_expr(a, sub);
                }
            }
            HirExpr::Println { arg, .. } => self.collect_type_overrides_for_expr(arg, sub),
            HirExpr::Assert {
                condition, message, ..
            } => {
                self.collect_type_overrides_for_expr(condition, sub);
                if let Some(m) = message {
                    self.collect_type_overrides_for_expr(m, sub);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn collect_type_overrides_for_stmt(
        &mut self,
        stmt: &HirStmt,
        sub: &HashMap<Symbol, HirType>,
    ) {
        match stmt {
            HirStmt::Let { value, .. }
            | HirStmt::LetPat { value, .. }
            | HirStmt::Assign { value, .. } => self.collect_type_overrides_for_expr(value, sub),
            HirStmt::AssignField { object, value, .. } => {
                self.collect_type_overrides_for_expr(object, sub);
                self.collect_type_overrides_for_expr(value, sub);
            }
            HirStmt::AssignDeref { target, value, .. } => {
                self.collect_type_overrides_for_expr(target, sub);
                self.collect_type_overrides_for_expr(value, sub);
            }
            HirStmt::Expr(e) => self.collect_type_overrides_for_expr(e, sub),
        }
    }
}
