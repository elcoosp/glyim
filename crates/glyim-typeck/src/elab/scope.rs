use crate::ty::Ty;
use glyim_interner::Symbol;
use std::collections::HashMap;

/// Lexical scope: maps variable names to their types.
#[derive(Clone, Debug)]
pub struct Scope {
    bindings: HashMap<Symbol, Ty>,
    parent: Option<Box<Scope>>,
}

impl Scope {
    pub fn new() -> Self {
        Self {
            bindings: HashMap::new(),
            parent: None,
        }
    }

    pub fn child(parent: Scope) -> Self {
        Self {
            bindings: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    pub fn insert(&mut self, name: Symbol, ty: Ty) {
        self.bindings.insert(name, ty);
    }

    pub fn lookup(&self, name: Symbol) -> Option<Ty> {
        self.bindings.get(&name).copied().or_else(|| {
            self.parent.as_ref().and_then(|p| p.lookup(name))
        })
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}
