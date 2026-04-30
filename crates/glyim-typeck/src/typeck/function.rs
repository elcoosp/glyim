use crate::typeck::error::TypeError;
use crate::TypeChecker;
use glyim_hir::node::HirFn;

impl TypeChecker {
    #[tracing::instrument(skip_all)]
pub(crate) fn check_fn(&mut self, f: &HirFn) {
        self.with_scope(|tc| {
            for (sym, ty) in &f.params {
                tc.insert_binding(*sym, ty.clone());
            }
            let body_type = tc.check_expr(&f.body);
            if let Some(ref expected) = f.ret {
                if let Some(ref actual) = body_type {
                    if expected != actual {
                        tc.errors.push(TypeError::InvalidReturnType {
                            expected: expected.clone(),
                            found: actual.clone(),
                        });
                    }
                }
            }
        });
    }
}
