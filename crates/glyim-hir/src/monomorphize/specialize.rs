// crates/glyim-hir/src/monomorphize/specialize.rs
use super::*;
use crate::node::{HirExpr, HirStmt};
use crate::types::HirType;
use std::collections::HashMap;
use tracing::debug;

impl<'a> MonoContext<'a> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn specialize_fn(&mut self, f: &HirFn, concrete: &[HirType]) -> HirFn {
        let mut sub = HashMap::new();

        tracing::debug!(
            "[specialize_fn] ENTER fn={} type_params=[{}] concrete={:?}",
            self.interner.resolve(f.name),
            f.type_params
                .iter()
                .map(|s| self.interner.resolve(*s))
                .collect::<Vec<_>>()
                .join(", "),
            concrete
                .iter()
                .map(|t| format!("{:?}", t))
                .collect::<Vec<_>>()
        );

        // Use explicit type parameters of the function if present
        for (i, tp) in f.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }

        // If no type params are on the function itself (e.g., struct methods),
        // derive the substitution from the self parameter's generic type.
        if sub.is_empty() && !f.params.is_empty() {
            if let (_first_sym, HirType::Generic(_, param_type_args)) = &f.params[0] {
                for (i, formal) in param_type_args.iter().enumerate() {
                    if let HirType::Named(formal_name) = formal {
                        if let Some(ct) = concrete.get(i) {
                            sub.insert(*formal_name, ct.clone());
                        }
                    }
                }
            }
        }

        // Fallback: if sub is still empty but we have concrete args and
        // function type params, map them directly.
        if sub.is_empty() && !concrete.is_empty() && !f.type_params.is_empty() {
            for (i, tp) in f.type_params.iter().enumerate() {
                if let Some(ct) = concrete.get(i) {
                    sub.insert(*tp, ct.clone());
                }
            }
        }

        // If the function is already fully specialized and sub is empty,
        // return it unchanged to avoid incorrect substitutions.
        if f.type_params.is_empty() && sub.is_empty() {
            return f.clone();
        }

        for ct in concrete {
            self.ensure_struct_specialized(ct);
        }
        self.collect_type_overrides_for_expr(&f.body, &sub);

        self.scan_expr_for_generic_calls(&f.body, &sub);
        self.scan_expr_for_struct_instantiations(&f.body, &sub);

        let mut mono = f.clone();
        mono.type_params.clear();
        for (_, pt) in &mut mono.params {
            *pt = crate::types::substitute_type(pt, &sub);
        }
        if let Some(rt) = &mut mono.ret {
            *rt = crate::types::substitute_type(rt, &sub);
        }
        mono.body = self.substitute_expr_types(&mono.body, &sub);

        // Brute-force pass: walk the body and replace any As target type
        // that matches a type parameter with its concrete type.
        if !sub.is_empty() {
            tracing::debug!("[specialize_fn] force sub map: {:?}", sub);
            tracing::debug!(
                "[specialize_fn] body before As substitution: {:#?}",
                mono.body
            );
            mono.body = Self::force_substitute_as_targets(mono.body, &sub);
            tracing::debug!(
                "[specialize_fn] body after As substitution: {:#?}",
                mono.body
            );
        }

        self.scan_expr_for_generic_calls(&mono.body, &sub);
        self.scan_expr_for_struct_instantiations(&mono.body, &sub);

        mono
    }

    fn force_substitute_as_targets(expr: HirExpr, sub: &HashMap<Symbol, HirType>) -> HirExpr {
        match expr {
            HirExpr::As {
                id,
                expr: inner,
                target_type,
                span,
            } => {
                let new_target = crate::types::substitute_type(&target_type, sub);
                tracing::debug!(
                    "[force_sub] As id={:?} old_target={:?} new_target={:?}",
                    id,
                    target_type,
                    new_target
                );
                HirExpr::As {
                    id,
                    expr: Box::new(Self::force_substitute_as_targets(*inner, sub)),
                    target_type: new_target,
                    span,
                }
            }
            HirExpr::Block { id, stmts, span } => HirExpr::Block {
                id,
                stmts: stmts
                    .into_iter()
                    .map(|s| Self::force_substitute_stmt_as(s, sub))
                    .collect(),
                span,
            },
            HirExpr::If {
                id,
                condition,
                then_branch,
                else_branch,
                span,
            } => HirExpr::If {
                id,
                condition: Box::new(Self::force_substitute_as_targets(*condition, sub)),
                then_branch: Box::new(Self::force_substitute_as_targets(*then_branch, sub)),
                else_branch: else_branch
                    .map(|e| Box::new(Self::force_substitute_as_targets(*e, sub))),
                span,
            },
            HirExpr::Match {
                id,
                scrutinee,
                arms,
                span,
            } => HirExpr::Match {
                id,
                scrutinee: Box::new(Self::force_substitute_as_targets(*scrutinee, sub)),
                arms: arms
                    .into_iter()
                    .map(|(p, g, b)| {
                        (
                            p,
                            g.map(|g| Self::force_substitute_as_targets(g, sub)),
                            Self::force_substitute_as_targets(b, sub),
                        )
                    })
                    .collect(),
                span,
            },
            HirExpr::Binary {
                id,
                op,
                lhs,
                rhs,
                span,
            } => HirExpr::Binary {
                id,
                op,
                lhs: Box::new(Self::force_substitute_as_targets(*lhs, sub)),
                rhs: Box::new(Self::force_substitute_as_targets(*rhs, sub)),
                span,
            },
            HirExpr::Unary {
                id,
                op,
                operand,
                span,
            } => HirExpr::Unary {
                id,
                op,
                operand: Box::new(Self::force_substitute_as_targets(*operand, sub)),
                span,
            },
            HirExpr::Return { id, value, span } => HirExpr::Return {
                id,
                value: value.map(|v| Box::new(Self::force_substitute_as_targets(*v, sub))),
                span,
            },
            HirExpr::While {
                id,
                condition,
                body,
                span,
            } => HirExpr::While {
                id,
                condition: Box::new(Self::force_substitute_as_targets(*condition, sub)),
                body: Box::new(Self::force_substitute_as_targets(*body, sub)),
                span,
            },
            HirExpr::ForIn {
                id,
                pattern,
                iter,
                body,
                span,
            } => HirExpr::ForIn {
                id,
                pattern,
                iter: Box::new(Self::force_substitute_as_targets(*iter, sub)),
                body: Box::new(Self::force_substitute_as_targets(*body, sub)),
                span,
            },
            HirExpr::Deref { id, expr: e, span } => HirExpr::Deref {
                id,
                expr: Box::new(Self::force_substitute_as_targets(*e, sub)),
                span,
            },
            HirExpr::FieldAccess {
                id,
                object,
                field,
                span,
            } => HirExpr::FieldAccess {
                id,
                object: Box::new(Self::force_substitute_as_targets(*object, sub)),
                field,
                span,
            },
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                span,
            } => HirExpr::StructLit {
                id,
                struct_name,
                fields: fields
                    .into_iter()
                    .map(|(s, e)| (s, Self::force_substitute_as_targets(e, sub)))
                    .collect(),
                span,
            },
            HirExpr::EnumVariant {
                id,
                enum_name,
                variant_name,
                args,
                span,
            } => HirExpr::EnumVariant {
                id,
                enum_name,
                variant_name,
                args: args
                    .into_iter()
                    .map(|a| Self::force_substitute_as_targets(a, sub))
                    .collect(),
                span,
            },
            HirExpr::TupleLit { id, elements, span } => HirExpr::TupleLit {
                id,
                elements: elements
                    .into_iter()
                    .map(|a| Self::force_substitute_as_targets(a, sub))
                    .collect(),
                span,
            },
            HirExpr::Call {
                id,
                callee,
                args,
                span,
            } => HirExpr::Call {
                id,
                callee,
                args: args
                    .into_iter()
                    .map(|a| Self::force_substitute_as_targets(a, sub))
                    .collect(),
                span,
            },
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                resolved_callee,
                args,
                span,
            } => HirExpr::MethodCall {
                id,
                receiver: Box::new(Self::force_substitute_as_targets(*receiver, sub)),
                method_name,
                resolved_callee,
                args: args
                    .into_iter()
                    .map(|a| Self::force_substitute_as_targets(a, sub))
                    .collect(),
                span,
            },
            HirExpr::Println { id, arg, span } => HirExpr::Println {
                id,
                arg: Box::new(Self::force_substitute_as_targets(*arg, sub)),
                span,
            },
            HirExpr::Assert {
                id,
                condition,
                message,
                span,
            } => HirExpr::Assert {
                id,
                condition: Box::new(Self::force_substitute_as_targets(*condition, sub)),
                message: message.map(|m| Box::new(Self::force_substitute_as_targets(*m, sub))),
                span,
            },
            other => other,
        }
    }

    fn force_substitute_stmt_as(stmt: HirStmt, sub: &HashMap<Symbol, HirType>) -> HirStmt {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                span,
            } => HirStmt::Let {
                name,
                mutable,
                value: Self::force_substitute_as_targets(value, sub),
                span,
            },
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                ty,
                span,
            } => HirStmt::LetPat {
                pattern,
                mutable,
                value: Self::force_substitute_as_targets(value, sub),
                ty,
                span,
            },
            HirStmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target,
                value: Self::force_substitute_as_targets(value, sub),
                span,
            },
            HirStmt::AssignField {
                object,
                field,
                value,
                span,
            } => HirStmt::AssignField {
                object: Box::new(Self::force_substitute_as_targets(*object, sub)),
                field,
                value: Self::force_substitute_as_targets(value, sub),
                span,
            },
            HirStmt::AssignDeref {
                target,
                value,
                span,
            } => HirStmt::AssignDeref {
                target: Box::new(Self::force_substitute_as_targets(*target, sub)),
                value: Self::force_substitute_as_targets(value, sub),
                span,
            },
            HirStmt::Expr(e) => HirStmt::Expr(Self::force_substitute_as_targets(e, sub)),
        }
    }

    pub(crate) fn ensure_struct_specialized(&mut self, ty: &HirType) {
        if let HirType::Generic(sym, args) = ty {
            if self.find_struct(*sym).is_some() {
                let concrete: Vec<HirType> = args.clone();
                let key = (*sym, concrete.clone());
                if !self.struct_specs.contains_key(&key) {
                    if let Some(s) = self.find_struct(*sym) {
                        let specialized = self.specialize_struct(&s, &concrete);
                        self.struct_specs.insert(key, specialized);
                    }
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
