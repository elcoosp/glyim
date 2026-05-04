use crate::TypeChecker;
use crate::typeck::types::Scope;
use glyim_hir::types::HirType;
use glyim_interner::Symbol;

impl TypeChecker {
    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }
    pub(crate) fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    pub(crate) fn insert_binding(&mut self, name: Symbol, ty: HirType, mutable: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty, mutable);
        }
    }
    pub(crate) fn lookup_binding(&self, name: &Symbol) -> Option<HirType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.lookup(name) {
                return Some(ty.clone());
            }
        }
        None
    }
    pub(crate) fn lookup_binding_full(
        &self,
        name: &Symbol,
    ) -> Option<&crate::typeck::types::Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.lookup_binding(name) {
                return Some(b);
            }
        }
        None
    }
    /// Unify a concrete type with a generic type, extracting type parameter bindings.
    pub(crate) fn unify_types(
        concrete: &HirType,
        generic: &HirType,
        type_params: &[Symbol],
        sub: &mut std::collections::HashMap<Symbol, HirType>,
    ) {
        match (concrete, generic) {
            // Any concrete type matching a type parameter
            (_, HirType::Named(gen_name)) if type_params.contains(gen_name) => {
                sub.insert(*gen_name, concrete.clone());
            }
            // Primitive type -> named type param
            (HirType::Int, HirType::Named(gen_name)) if type_params.contains(gen_name) => {
                sub.insert(*gen_name, HirType::Int);
            }
            (HirType::Float, HirType::Named(gen_name)) if type_params.contains(gen_name) => {
                sub.insert(*gen_name, HirType::Float);
            }
            (HirType::Bool, HirType::Named(gen_name)) if type_params.contains(gen_name) => {
                sub.insert(*gen_name, HirType::Bool);
            }
            (HirType::Str, HirType::Named(gen_name)) if type_params.contains(gen_name) => {
                sub.insert(*gen_name, HirType::Str);
            }
            // Named -> Named (could be type param)
            (HirType::Named(conc_name), HirType::Named(gen_name))
                if type_params.contains(gen_name) =>
            {
                sub.insert(*gen_name, HirType::Named(*conc_name));
            }
            // Generic with same name and arity
            (HirType::Generic(conc_name, conc_args), HirType::Generic(gen_name, gen_args))
                if conc_name == gen_name && conc_args.len() == gen_args.len() =>
            {
                for (ca, ga) in conc_args.iter().zip(gen_args.iter()) {
                    Self::unify_types(ca, ga, type_params, sub);
                }
            }
            // Named concrete matching a generic param
            (HirType::Named(conc_name), HirType::Generic(gen_name, _))
                if type_params.contains(gen_name) =>
            {
                sub.insert(*gen_name, HirType::Named(*conc_name));
            }
            // Generic concrete matching a named param
            (HirType::Generic(conc_name, conc_args), HirType::Named(gen_name))
                if type_params.contains(gen_name) =>
            {
                sub.insert(*gen_name, HirType::Generic(*conc_name, conc_args.clone()));
            }
            // RawPtr recursion
            (HirType::RawPtr(conc_inner), HirType::RawPtr(gen_inner)) => {
                Self::unify_types(conc_inner, gen_inner, type_params, sub);
            }
            // Tuple recursion
            (HirType::Tuple(conc_elems), HirType::Tuple(gen_elems))
                if conc_elems.len() == gen_elems.len() =>
            {
                for (ca, ga) in conc_elems.iter().zip(gen_elems.iter()) {
                    Self::unify_types(ca, ga, type_params, sub);
                }
            }
            // Fallback: any concrete type matches a type param directly
            (_, HirType::Named(gen_name)) if type_params.contains(gen_name) => {
                sub.insert(*gen_name, concrete.clone());
            }
            _ => {}
        }
    }

    pub(crate) fn with_scope<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.push_scope();
        let result = f(self);
        self.pop_scope();
        result
    }
}
