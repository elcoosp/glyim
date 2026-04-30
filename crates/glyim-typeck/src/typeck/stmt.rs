use crate::TypeChecker;
use crate::TypeError;
use glyim_hir::node::HirStmt;
use glyim_hir::types::ExprId;
use glyim_hir::types::HirType;
use glyim_hir::HirPattern;

impl TypeChecker {
    #[tracing::instrument(skip_all)]
    pub(crate) fn check_stmt(&mut self, stmt: &HirStmt) -> Option<HirType> {
        match stmt {
            HirStmt::Let { name, mutable, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.insert_binding(*name, ty, *mutable);
                None
            }
            HirStmt::LetPat { pattern, value, .. } => {
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.bind_pattern(pattern, &ty);
                None
            }
            HirStmt::Assign { target, value, .. } => {
                let immutable = self.lookup_binding_full(target)
                    .map(|b| !b.mutable).unwrap_or(false);
                if immutable {
                    self.errors.push(TypeError::AssignToImmutable {
                        name: *target, expr_id: ExprId::new(0),
                    });
                }
                let ty = self.check_expr(value).unwrap_or(HirType::Int);
                self.insert_binding(*target, ty.clone(), true);
                Some(ty)
            }
            HirStmt::Expr(e) => self.check_expr(e),
        }
    }

    pub(crate) fn bind_pattern(&mut self, pattern: &HirPattern, value_ty: &HirType) {
        match pattern {
            HirPattern::Var(sym) => {
                self.insert_binding(*sym, value_ty.clone(), false);
            }
            HirPattern::Wild => {}
            HirPattern::Tuple { elements, .. } => {
                if let HirType::Tuple(elem_types) = value_ty {
                    for (pat, ty) in elements.iter().zip(elem_types.iter()) {
                        self.bind_pattern(pat, ty);
                    }
                }
            }
            _ => {}
        }
    }
}
