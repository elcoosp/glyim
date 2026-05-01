use crate::TypeChecker;
use crate::TypeError;
use glyim_hir::node::HirStmt;
use glyim_hir::types::{ExprId, HirType};
use glyim_hir::{HirExpr, HirPattern};
impl TypeChecker {
    #[tracing::instrument(skip_all)]
    pub(crate) fn check_stmt(&mut self, stmt: &HirStmt) -> Option<HirType> {
        match stmt {
            HirStmt::Let {
                name,
                mutable,
                value,
                ..
            } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.insert_binding(*name, ty, *mutable);
                None
            }
            HirStmt::LetPat {
                pattern,
                mutable,
                value,
                ty: annotation,
                ..
            } => {
                let inferred = self.check_expr(value).unwrap_or(HirType::Int);
                // If there's an annotation, use it as the binding type (overriding inference)
                let ty = if let Some(annotated) = annotation {
                    let resolved_annotated =
                        crate::typeck::resolver::resolve_named_type(&self.interner, annotated);
                    let resolved_inferred =
                        crate::typeck::resolver::resolve_named_type(&self.interner, &inferred);
                    // Accept Generic<->Named compatibility (same base type)
                    let is_compat = resolved_annotated == resolved_inferred
                        || match (&resolved_annotated, &resolved_inferred) {
                            (HirType::Generic(a, _), HirType::Named(b)) => a == b,
                            (HirType::Named(a), HirType::Generic(b, _)) => a == b,
                            (HirType::Generic(a, _), HirType::Generic(b, _)) => a == b,
                            _ => false,
                        };
                    if !is_compat {
                        self.errors.push(TypeError::MismatchedTypes {
                            expected: annotated.clone(),
                            found: inferred,
                            expr_id: value.get_id(),
                        });
                    }
                    // CRITICAL: use the annotation type, not inferred, for the binding
                    annotated.clone()
                } else {
                    inferred
                };
                // Propagate type args to monomorphizer for MethodCall/Call expressions
                if let HirPattern::Var(_) = pattern {
                    if let HirType::Generic(_, type_args) = &ty {
                        let value_id = value.get_id();
                        if !type_args.is_empty() {
                            self.call_type_args.insert(value_id, type_args.clone());
                        }
                    }
                }
                self.bind_pattern(pattern, &ty, *mutable);
                None
            }
            HirStmt::Assign { target, value, .. } => {
                let binding = self.lookup_binding_full(target);
                let immutable = binding.map(|b| !b.mutable).unwrap_or(false);
                if immutable {
                    self.errors.push(TypeError::AssignToImmutable {
                        name: *target,
                        expr_id: ExprId::new(0),
                    });
                }
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.insert_binding(*target, ty.clone(), true);
                Some(ty)
            }
            HirStmt::AssignDeref { target, value, .. } => {
                let pointer_expr = if let HirExpr::Deref { expr, .. } = target.as_ref() {
                    expr.as_ref()
                } else {
                    target
                };
                let ptr_ty = self.check_expr(pointer_expr).unwrap_or(HirType::Never);
                let value_ty = self.check_expr(value).unwrap_or(HirType::Int);
                match ptr_ty {
                    HirType::RawPtr(inner) => {
                        if *inner != value_ty {
                            self.errors.push(TypeError::MismatchedTypes {
                                expected: *inner,
                                found: value_ty,
                                expr_id: ExprId::new(0),
                            });
                        }
                    }
                    _ => {
                        self.errors.push(TypeError::AssignThroughNonPointer {
                            found: ptr_ty,
                            expr_id: ExprId::new(0),
                        });
                    }
                }
                Some(HirType::Unit)
            }
            HirStmt::AssignField {
                object,
                field: _,
                value,
                ..
            } => {
                let _obj_ty = self.check_expr(object).unwrap_or(HirType::Int);
                let val_ty = self.check_expr(value).unwrap_or(HirType::Int);
                Some(val_ty)
            }
            HirStmt::Expr(e) => self.check_expr(e),
        }
    }
    pub(crate) fn bind_pattern(&mut self, pattern: &HirPattern, value_ty: &HirType, mutable: bool) {
        match pattern {
            HirPattern::Var(sym) => {
                self.insert_binding(*sym, value_ty.clone(), mutable);
            }
            HirPattern::Wild => {}
            HirPattern::Tuple { elements, .. } => {
                if let HirType::Tuple(elem_types) = value_ty {
                    for (pat, ty) in elements.iter().zip(elem_types.iter()) {
                        self.bind_pattern(pat, ty, mutable);
                    }
                }
            }
            HirPattern::Struct { name, bindings, .. } => {
                let field_tys: Vec<HirType> = {
                    if let Some(info) = self.structs.get(name) {
                        bindings
                            .iter()
                            .filter_map(|(f_sym, _)| {
                                info.field_map
                                    .get(f_sym)
                                    .and_then(|&idx| info.fields.get(idx).map(|f| f.ty.clone()))
                            })
                            .collect()
                    } else {
                        vec![]
                    }
                };
                for ((_, field_pat), field_ty) in bindings.iter().zip(field_tys.iter()) {
                    self.bind_pattern(field_pat, field_ty, mutable);
                }
            }
            _ => {}
        }
    }
}
