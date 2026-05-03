// crates/glyim-hir/src/monomorphize/context.rs
use super::*;
use crate::item::HirItem;
use crate::node::{HirExpr, HirFn, HirStmt};
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    pub(crate) fn new(
        hir: &'a crate::Hir,
        interner: &'a mut Interner,
        expr_types: &'a [HirType],
        call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    ) -> Self {
        Self {
            hir,
            interner,
            expr_types,
            call_type_args,
            fn_specs: HashMap::new(),
            struct_specs: HashMap::new(),
            type_overrides: HashMap::new(),
            fn_work_queue: Vec::new(),
            fn_queued: HashSet::new(),
        }
    }

    /// Get the type for an expression, checking type_overrides first (specialized types)
    /// then falling back to expr_types (original types from typeck).
    pub(crate) fn get_expr_type(&self, id: ExprId) -> Option<HirType> {
        self.type_overrides
            .get(&id)
            .cloned()
            .or_else(|| self.expr_types.get(id.as_usize()).cloned())
    }

    pub(crate) fn find_fn(&mut self, name: Symbol) -> Option<HirFn> {
        let name_str = self.interner.resolve(name).to_string();

        // If it's a mangled name (contains __), strip to get base name
        let base_name = if let Some(pos) = name_str.find("__") {
            self.interner.intern(&name_str[..pos])
        } else {
            name
        };

        // Search top-level functions
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item
                && f.name == base_name
            {
                return Some(f.clone());
            }
        }

        // Search impl methods
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if m.name == base_name {
                        return Some(m.clone());
                    }
                }
            }
        }

        None
    }

    pub(crate) fn find_struct(&self, name: Symbol) -> Option<StructDef> {
        for item in &self.hir.items {
            if let HirItem::Struct(s) = item
                && s.name == name
            {
                return Some(s.clone());
            }
        }
        None
    }

    pub(crate) fn mangle_name(&mut self, base: Symbol, type_args: &[HirType]) -> Symbol {
        mangle_type_name(self.interner, base, type_args)
    }

    /// Check if a type contains any unresolved type parameters (single uppercase letters)
    pub(crate) fn has_unresolved_type_param(&self, ty: &HirType) -> bool {
        match ty {
            HirType::Named(sym) => {
                let s = self.interner.resolve(*sym);
                s.len() == 1 && s.chars().next().map_or(false, |c| c.is_uppercase())
            }
            HirType::Generic(_, args) => args.iter().any(|a| self.has_unresolved_type_param(a)),
            HirType::RawPtr(inner) => self.has_unresolved_type_param(inner.as_ref()),
            HirType::Option(inner) => self.has_unresolved_type_param(inner.as_ref()),
            HirType::Result(ok, err) => {
                self.has_unresolved_type_param(ok) || self.has_unresolved_type_param(err)
            }
            HirType::Tuple(elems) => elems.iter().any(|e| self.has_unresolved_type_param(e)),
            _ => false,
        }
    }

    /// Substitute unresolved type params in a type list using the given substitution
    pub(crate) fn substitute_type_args(
        &self,
        args: &[HirType],
        sub: &HashMap<Symbol, HirType>,
    ) -> Vec<HirType> {
        if sub.is_empty() {
            args.to_vec()
        } else {
            args.iter()
                .map(|ty| crate::types::substitute_type(ty, sub))
                .collect()
        }
    }

    pub(crate) fn specialize_struct(&mut self, s: &StructDef, concrete: &[HirType]) -> StructDef {
        let mut sub = HashMap::new();
        for (i, tp) in s.type_params.iter().enumerate() {
            if let Some(ct) = concrete.get(i) {
                sub.insert(*tp, ct.clone());
            }
        }
        let mut mono = s.clone();
        mono.type_params.clear();
        for field in &mut mono.fields {
            field.ty = crate::types::substitute_type(&field.ty, &sub);
        }
        mono
    }

    pub(crate) fn substitute_expr_types(
        &mut self,
        expr: &HirExpr,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
        match expr {
            HirExpr::SizeOf {
                id,
                target_type,
                span,
            } => HirExpr::SizeOf {
                id: *id,
                target_type: crate::types::substitute_type(target_type, sub),
                span: *span,
            },
            HirExpr::As {
                id,
                expr: inner,
                target_type,
                span,
            } => HirExpr::As {
                id: *id,
                expr: Box::new(self.substitute_expr_types(inner, sub)),
                target_type: crate::types::substitute_type(target_type, sub),
                span: *span,
            },
            HirExpr::Block { id, stmts, span } => HirExpr::Block {
                id: *id,
                stmts: stmts
                    .iter()
                    .map(|s| self.substitute_stmt_types(s, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::Binary {
                id,
                op,
                lhs,
                rhs,
                span,
            } => HirExpr::Binary {
                id: *id,
                op: op.clone(),
                lhs: Box::new(self.substitute_expr_types(lhs, sub)),
                rhs: Box::new(self.substitute_expr_types(rhs, sub)),
                span: *span,
            },
            HirExpr::If {
                id,
                condition,
                then_branch,
                else_branch,
                span,
            } => HirExpr::If {
                id: *id,
                condition: Box::new(self.substitute_expr_types(condition, sub)),
                then_branch: Box::new(self.substitute_expr_types(then_branch, sub)),
                else_branch: else_branch
                    .as_ref()
                    .map(|e| Box::new(self.substitute_expr_types(e, sub))),
                span: *span,
            },
            HirExpr::Match {
                id,
                scrutinee,
                arms,
                span,
            } => HirExpr::Match {
                id: *id,
                scrutinee: Box::new(self.substitute_expr_types(scrutinee, sub)),
                arms: arms
                    .iter()
                    .map(|(pat, guard, body)| {
                        (
                            pat.clone(),
                            guard.as_ref().map(|g| self.substitute_expr_types(g, sub)),
                            self.substitute_expr_types(body, sub),
                        )
                    })
                    .collect(),
                span: *span,
            },
            HirExpr::Call {
                id,
                callee,
                args,
                span,
            } => HirExpr::Call {
                id: *id,
                callee: *callee,
                args: args
                    .iter()
                    .map(|a| self.substitute_expr_types(a, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                span,
                ..
            } => HirExpr::MethodCall {
                id: *id,
                receiver: Box::new(self.substitute_expr_types(receiver, sub)),
                method_name: *method_name,
                resolved_callee: None,
                args: args
                    .iter()
                    .map(|a| self.substitute_expr_types(a, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::Unary {
                id,
                op,
                operand,
                span,
            } => HirExpr::Unary {
                id: *id,
                op: op.clone(),
                operand: Box::new(self.substitute_expr_types(operand, sub)),
                span: *span,
            },
            HirExpr::Return { id, value, span } => HirExpr::Return {
                id: *id,
                value: value
                    .as_ref()
                    .map(|v| Box::new(self.substitute_expr_types(v, sub))),
                span: *span,
            },
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                span,
            } => {
                let new_name = if let Some(struct_def) = self.find_struct(*struct_name) {
                    if !struct_def.type_params.is_empty() && !sub.is_empty() {
                        let concrete: Vec<HirType> = struct_def
                            .type_params
                            .iter()
                            .map(|tp| sub.get(tp).cloned().unwrap_or(HirType::Named(*tp)))
                            .collect();
                        let all_concrete =
                            concrete.iter().all(|a| !self.has_unresolved_type_param(a));
                        if all_concrete {
                            self.mangle_name(*struct_name, &concrete)
                        } else {
                            *struct_name
                        }
                    } else {
                        *struct_name
                    }
                } else {
                    *struct_name
                };
                HirExpr::StructLit {
                    id: *id,
                    struct_name: new_name,
                    fields: fields
                        .iter()
                        .map(|(s, e)| (*s, self.substitute_expr_types(e, sub)))
                        .collect(),
                    span: *span,
                }
            }
            HirExpr::EnumVariant {
                id,
                enum_name,
                variant_name,
                args,
                span,
            } => HirExpr::EnumVariant {
                id: *id,
                enum_name: *enum_name,
                variant_name: *variant_name,
                args: args
                    .iter()
                    .map(|a| self.substitute_expr_types(a, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::While {
                id,
                condition,
                body,
                span,
            } => HirExpr::While {
                id: *id,
                condition: Box::new(self.substitute_expr_types(condition, sub)),
                body: Box::new(self.substitute_expr_types(body, sub)),
                span: *span,
            },
            HirExpr::ForIn {
                id,
                pattern,
                iter,
                body,
                span,
            } => HirExpr::ForIn {
                id: *id,
                pattern: pattern.clone(),
                iter: Box::new(self.substitute_expr_types(iter, sub)),
                body: Box::new(self.substitute_expr_types(body, sub)),
                span: *span,
            },
            HirExpr::Deref { id, expr, span } => HirExpr::Deref {
                id: *id,
                expr: Box::new(self.substitute_expr_types(expr, sub)),
                span: *span,
            },
            HirExpr::FieldAccess {
                id,
                object,
                field,
                span,
            } => HirExpr::FieldAccess {
                id: *id,
                object: Box::new(self.substitute_expr_types(object, sub)),
                field: *field,
                span: *span,
            },
            HirExpr::TupleLit { id, elements, span } => HirExpr::TupleLit {
                id: *id,
                elements: elements
                    .iter()
                    .map(|e| self.substitute_expr_types(e, sub))
                    .collect(),
                span: *span,
            },
            HirExpr::Println { id, arg, span } => HirExpr::Println {
                id: *id,
                arg: Box::new(self.substitute_expr_types(arg, sub)),
                span: *span,
            },
            HirExpr::Assert {
                id,
                condition,
                message,
                span,
            } => HirExpr::Assert {
                id: *id,
                condition: Box::new(self.substitute_expr_types(condition, sub)),
                message: message
                    .as_ref()
                    .map(|m| Box::new(self.substitute_expr_types(m, sub))),
                span: *span,
            },
            _ => expr.clone(),
        }
    }

    pub(crate) fn substitute_stmt_types(
        &mut self,
        stmt: &HirStmt,
        sub: &HashMap<Symbol, HirType>,
    ) -> HirStmt {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                span,
            } => HirStmt::Let {
                name: *name,
                mutable: *mutable,
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                span,
                ty,
            } => HirStmt::LetPat {
                pattern: pattern.clone(),
                mutable: *mutable,
                value: self.substitute_expr_types(value, sub),
                ty: ty.clone(),
                span: *span,
            },
            HirStmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target: *target,
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::AssignField {
                object,
                field,
                value,
                span,
            } => HirStmt::AssignField {
                object: Box::new(self.substitute_expr_types(object, sub)),
                field: *field,
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::AssignDeref {
                target,
                value,
                span,
            } => HirStmt::AssignDeref {
                target: Box::new(self.substitute_expr_types(target, sub)),
                value: self.substitute_expr_types(value, sub),
                span: *span,
            },
            HirStmt::Expr(e) => HirStmt::Expr(self.substitute_expr_types(e, sub)),
        }
    }
}
