use crate::TypeChecker;
use crate::typeck::error::TypeError;
use glyim_hir::node::HirFn;
use glyim_hir::types::HirType;

impl TypeChecker {
    #[tracing::instrument(skip_all)]
    pub(crate) fn check_fn(&mut self, f: &HirFn) {
        self.call_type_args.clear();
        self.current_fn_type_params = f.type_params.clone();
        let _fn_name = self.interner.resolve(f.name).to_string();
        self.with_scope(|tc| {
            for (i, (sym, ty)) in f.params.iter().enumerate() {
                let mutable = f.param_mutability.get(i).copied().unwrap_or(false);
                tc.insert_binding(*sym, ty.clone(), mutable);
            }
            let body_type = tc.check_expr(&f.body);
            if let Some(expected) = &f.ret
                && let Some(actual) = body_type
            {
                let is_match = *expected == actual
                    || match (expected.clone(), actual.clone()) {
                        (HirType::Generic(s1, _), HirType::Named(s2)) => s1 == s2,
                        (HirType::Named(s1), HirType::Generic(s2, _)) => s1 == s2,
                        (HirType::Generic(s1, _), HirType::Generic(s2, _)) => s1 == s2,
                        _ => false,
                    };
                if !is_match {
                    tc.errors.push(TypeError::InvalidReturnType {
                        expected: expected.clone(),
                        found: actual.clone(),
                    });
                }
            }
        });
    }
}
