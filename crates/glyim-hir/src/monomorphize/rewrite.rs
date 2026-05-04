// crates/glyim-hir/src/monomorphize/rewrite.rs
use super::*;
use crate::HirPattern;
use crate::MatchArm;
use crate::node::{HirExpr, HirStmt};
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    #[tracing::instrument(skip_all)]
    pub(crate) fn rewrite_expr(
        &mut self,
        expr: &HirExpr,
        fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        struct_map: &HashMap<Symbol, Symbol>,
        enum_spec_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        type_sub: &HashMap<Symbol, HirType>,
    ) -> HirExpr {
        match expr {
            HirExpr::Call {
                id,
                callee,
                args,
                span,
            } => {
                let type_args = self
                    .call_type_args
                    .get(id)
                    .map(|args| self.substitute_type_args(args, type_sub))
                    .or_else(|| self.call_type_args_overrides.get(id).cloned())
                    .unwrap_or_default();

                let new_callee = if !type_args.is_empty() {
                    fn_map.get(&(*callee, type_args.clone())).copied()
                } else {
                    fn_map.get(&(*callee, vec![])).copied()
                };

                // Fallback 1: check call_type_args_overrides
                let new_callee = new_callee.or_else(|| {
                    self.call_type_args_overrides
                        .get(id)
                        .and_then(|concrete| fn_map.get(&(*callee, concrete.clone())).copied())
                });

                // Fallback 2: if exactly one specialization exists for this callee, use it
                let new_callee = new_callee.unwrap_or_else(|| {
                    let matches: Vec<_> = fn_map
                        .iter()
                        .filter(|((sym, _), _)| sym == callee)
                        .collect();
                    if matches.len() == 1 {
                        *matches[0].1
                    } else {
                        *callee
                    }
                });

                HirExpr::Call {
                    id: *id,
                    callee: new_callee,
                    args: args
                        .iter()
                        .map(|a| self.rewrite_expr(a, fn_map, struct_map, enum_spec_map, type_sub))
                        .collect(),
                    span: *span,
                }
            }

            HirExpr::MethodCall {
                id,
                receiver,
                method_name,
                args,
                span,
                ..
            } => {
                let rewritten_receiver = Box::new(self.rewrite_expr(
                    receiver,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                ));
                let rewritten_args: Vec<HirExpr> = args
                    .iter()
                    .map(|a| self.rewrite_expr(a, fn_map, struct_map, enum_spec_map, type_sub))
                    .collect();

                let receiver_ty = self.get_expr_type(receiver.get_id());
                let concrete_receiver_ty = if !type_sub.is_empty() {
                    receiver_ty.map(|ty| crate::types::substitute_type(&ty, type_sub))
                } else {
                    receiver_ty
                };

                let inner_ty = match &concrete_receiver_ty {
                    Some(HirType::RawPtr(inner)) => Some(inner.as_ref().clone()),
                    Some(other) => Some(other.clone()),
                    None => None,
                };

                let _mangled_sym = match &inner_ty {
                    Some(HirType::Named(type_name)) => {
                        let mangled = format!(
                            "{}_{}",
                            self.interner.resolve(*type_name),
                            self.interner.resolve(*method_name)
                        );
                        Some(self.interner.intern(&mangled))
                    }
                    Some(HirType::Generic(type_name, type_args)) => {
                        let mangled = format!(
                            "{}_{}",
                            self.interner.resolve(*type_name),
                            self.interner.resolve(*method_name)
                        );
                        let mangled_sym = self.interner.intern(&mangled);
                        let receiver_type_args = type_args.clone();

                        let key = (mangled_sym, receiver_type_args);
                        if let Some(&mono_name) = fn_map.get(&key) {
                            let mut all_args = vec![*rewritten_receiver.clone()];
                            all_args.extend(rewritten_args);
                            return HirExpr::Call {
                                id: *id,
                                callee: mono_name,
                                args: all_args,
                                span: *span,
                            };
                        }
                        None
                    }
                    _ => None,
                };

                HirExpr::MethodCall {
                    id: *id,
                    receiver: rewritten_receiver,
                    method_name: *method_name,
                    resolved_callee: None,
                    args: rewritten_args,
                    span: *span,
                }
            }

            HirExpr::Match {
                id,
                scrutinee,
                arms,
                span,
            } => HirExpr::Match {
                id: *id,
                scrutinee: Box::new(self.rewrite_expr(
                    scrutinee,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                arms: arms
                    .iter()
                    .map(|arm| {
                        let pat = &arm.pattern;
                        let guard = &arm.guard;
                        let body = &arm.body;
                        let rewritten_guard = guard.as_ref().map(|g| {
                            self.rewrite_expr(g, fn_map, struct_map, enum_spec_map, type_sub)
                        });
                        let rewritten_pat = if let HirPattern::EnumVariant {
                            enum_name,
                            variant_name,
                            bindings,
                            span,
                        } = pat
                        {
                            // For patterns, we can't easily get the concrete args because patterns don't carry type info.
                            // We'll try to find the specialized enum from the scrutinee's type (which is available as the match scrutinee expr_type).
                            // But we don't have access to scrutinee type here. For now, try exact match and fallback to base.
                            // Better: the pattern rewriting is cosmetic; the codegen match logic already uses the rewritten EnumVariant expression.
                            // So just keep the original pattern name? Actually the codegen match looks at the pattern to determine tag.
                            // We need the pattern to match the concrete enum name.
                            // Since we already rewrote the scrutinee EnumVariant, the value has the correct tag layout.
                            // The pattern just needs to match the tag index of the concrete enum.
                            // We'll search enum_spec_map for any entry where the base matches.
                            let new_enum_name = enum_spec_map
                                .iter()
                                .find(|((base, _), _)| base == enum_name)
                                .map(|(_, mangled)| *mangled)
                                .unwrap_or(*enum_name);
                            HirPattern::EnumVariant {
                                enum_name: new_enum_name,
                                variant_name: *variant_name,
                                bindings: bindings.clone(),
                                span: *span,
                            }
                        } else {
                            pat.clone()
                        };
                        MatchArm {
                            pattern: rewritten_pat,
                            guard: rewritten_guard,
                            body: self.rewrite_expr(
                                body,
                                fn_map,
                                struct_map,
                                enum_spec_map,
                                type_sub,
                            ),
                        }
                    })
                    .collect(),
                span: *span,
            },

            HirExpr::Block { id, stmts, span } => HirExpr::Block {
                id: *id,
                stmts: stmts
                    .iter()
                    .map(|s| self.rewrite_stmt(s, fn_map, struct_map, enum_spec_map, type_sub))
                    .collect(),
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
                condition: Box::new(self.rewrite_expr(
                    condition,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                then_branch: Box::new(self.rewrite_expr(
                    then_branch,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                else_branch: else_branch.as_ref().map(|e| {
                    Box::new(self.rewrite_expr(e, fn_map, struct_map, enum_spec_map, type_sub))
                }),
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
                lhs: Box::new(self.rewrite_expr(lhs, fn_map, struct_map, enum_spec_map, type_sub)),
                rhs: Box::new(self.rewrite_expr(rhs, fn_map, struct_map, enum_spec_map, type_sub)),
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
                operand: Box::new(self.rewrite_expr(
                    operand,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                span: *span,
            },
            HirExpr::Return { id, value, span } => HirExpr::Return {
                id: *id,
                value: value.as_ref().map(|v| {
                    Box::new(self.rewrite_expr(v, fn_map, struct_map, enum_spec_map, type_sub))
                }),
                span: *span,
            },
            HirExpr::Deref { id, expr, span } => HirExpr::Deref {
                id: *id,
                expr: Box::new(self.rewrite_expr(
                    expr,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
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
                iter: Box::new(self.rewrite_expr(
                    iter,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                body: Box::new(self.rewrite_expr(
                    body,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                span: *span,
            },
            HirExpr::FieldAccess {
                id,
                object,
                field,
                span,
            } => HirExpr::FieldAccess {
                id: *id,
                object: Box::new(self.rewrite_expr(
                    object,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                field: *field,
                span: *span,
            },
            HirExpr::StructLit {
                id,
                struct_name,
                fields,
                span,
            } => {
                let new_name = self
                    .type_overrides
                    .get(id)
                    .and_then(|ty| {
                        if let HirType::Named(mangled) = ty {
                            Some(*mangled)
                        } else {
                            None
                        }
                    })
                    .or_else(|| struct_map.get(struct_name).copied())
                    .unwrap_or(*struct_name);
                HirExpr::StructLit {
                    id: *id,
                    struct_name: new_name,
                    fields: fields
                        .iter()
                        .map(|(s, e)| {
                            (
                                *s,
                                self.rewrite_expr(e, fn_map, struct_map, enum_spec_map, type_sub),
                            )
                        })
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
            } => {
                // Use the expression's concrete type to find the correct specialized enum name.
                let expr_type = self.get_expr_type(*id);
                let new_enum_name = match &expr_type {
                    Some(HirType::Generic(base_sym, concrete_args)) => {
                        let key = (*base_sym, concrete_args.clone());
                        enum_spec_map.get(&key).copied().unwrap_or(*enum_name)
                    }
                    Some(HirType::Named(base_sym)) => {
                        // Try to find specialization with empty args
                        let key = (*base_sym, vec![]);
                        enum_spec_map.get(&key).copied().unwrap_or(*base_sym)
                    }
                    _ => *enum_name,
                };
                HirExpr::EnumVariant {
                    id: *id,
                    enum_name: new_enum_name,
                    variant_name: *variant_name,
                    args: args
                        .iter()
                        .map(|a| self.rewrite_expr(a, fn_map, struct_map, enum_spec_map, type_sub))
                        .collect(),
                    span: *span,
                }
            }
            _ => expr.clone(),
        }
    }

    pub(crate) fn rewrite_stmt(
        &mut self,
        stmt: &HirStmt,
        fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        struct_map: &HashMap<Symbol, Symbol>,
        enum_spec_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        type_sub: &HashMap<Symbol, HirType>,
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
                value: self.rewrite_expr(value, fn_map, struct_map, enum_spec_map, type_sub),
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
                value: self.rewrite_expr(value, fn_map, struct_map, enum_spec_map, type_sub),
                ty: ty.as_ref().map(|t| {
                    let substituted = crate::types::substitute_type(t, type_sub);
                    let mut concretized = self.concretize_type(&substituted);
                    // Repeat until fully concretized (handles outer Generic)
                    let mut prev = concretized.clone();
                    loop {
                        concretized = self.concretize_type(&prev);
                        tracing::debug!(
                            "[rewrite_stmt LetPat] repeat concretize: {:?} -> {:?}",
                            prev,
                            concretized
                        );
                        if concretized == prev {
                            break;
                        }
                        prev = concretized.clone();
                    }
                    concretized
                }),
                span: *span,
            },
            HirStmt::Assign {
                target,
                value,
                span,
            } => HirStmt::Assign {
                target: *target,
                value: self.rewrite_expr(value, fn_map, struct_map, enum_spec_map, type_sub),
                span: *span,
            },
            HirStmt::AssignField {
                object,
                field,
                value,
                span,
            } => HirStmt::AssignField {
                object: Box::new(self.rewrite_expr(
                    object,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                field: *field,
                value: self.rewrite_expr(value, fn_map, struct_map, enum_spec_map, type_sub),
                span: *span,
            },
            HirStmt::AssignDeref {
                target,
                value,
                span,
            } => HirStmt::AssignDeref {
                target: Box::new(self.rewrite_expr(
                    target,
                    fn_map,
                    struct_map,
                    enum_spec_map,
                    type_sub,
                )),
                value: self.rewrite_expr(value, fn_map, struct_map, enum_spec_map, type_sub),
                span: *span,
            },
            HirStmt::Expr(e) => {
                HirStmt::Expr(self.rewrite_expr(e, fn_map, struct_map, enum_spec_map, type_sub))
            }
        }
    }

    pub(crate) fn rewrite_fn(
        &mut self,
        f: &HirFn,
        fn_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        struct_map: &HashMap<Symbol, Symbol>,
        enum_spec_map: &HashMap<(Symbol, Vec<HirType>), Symbol>,
        type_sub: &HashMap<Symbol, HirType>,
    ) -> HirFn {
        let mut mono = f.clone();
        mono.body = self.rewrite_expr(&f.body, fn_map, struct_map, enum_spec_map, type_sub);
        mono
    }
}
