use crate::typeck::error::TypeError;
use crate::TypeChecker;
use glyim_hir::node::HirFn;
use glyim_hir::types::HirType;
impl TypeChecker {
    #[tracing::instrument(skip_all)]
    pub(crate) fn check_fn(&mut self, f: &HirFn) {
        // Clear call_type_args to prevent ExprId collisions across functions
        self.call_type_args.clear();
        self.current_fn_type_params = f.type_params.clone();
        let fn_name = self.interner.resolve(f.name).to_string();
        self.with_scope(|tc| {
            for (i, (sym, ty)) in f.params.iter().enumerate() {
                let mutable = f.param_mutability.get(i).copied().unwrap_or(false);
                tc.insert_binding(*sym, ty.clone(), mutable);
            }
            let body_type = tc.check_expr(&f.body);
            if let Some(ref expected) = f.ret {
                if let Some(ref actual) = body_type {
                    // Relaxed check: Generic<S, _> matches Named<S>
                    let is_match = expected == actual
                        || match (expected, actual) {
                            (HirType::Generic(s1, _), HirType::Named(s2)) => s1 == s2,
                            _ => false,
                        };
                    if !is_match {
                        eprintln!(
                            "[typeck] check_fn {}: expected {:?}, found {:?}",
                            fn_name, expected, actual
                        );
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
