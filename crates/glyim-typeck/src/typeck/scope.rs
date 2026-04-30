use crate::typeck::types::Scope;
use crate::TypeChecker;
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
        if let Some(scope) = self.scopes.last_mut() { scope.insert(name, ty, mutable); }
    }
    pub(crate) fn lookup_binding(&self, name: &Symbol) -> Option<HirType> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.lookup(name) { return Some(ty.clone()); }
        }
        None
    }
    pub(crate) fn lookup_binding_full(&self, name: &Symbol) -> Option<&crate::typeck::types::Binding> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.lookup_binding(name) { return Some(b); }
        }
        None
    }
    pub(crate) fn with_scope<F, R>(&mut self, f: F) -> R
    where F: FnOnce(&mut Self) -> R {
        self.push_scope();
        let result = f(self);
        self.pop_scope();
        result
    }
}
