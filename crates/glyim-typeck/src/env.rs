use glyim_hir::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct Binding {
    ty: HirType,
    mutable: bool,
    depth: usize,
}

pub struct TypeEnv {
    globals: HashMap<Symbol, Binding>,
    local_scopes: Vec<Vec<Symbol>>,
    local_bindings: HashMap<Symbol, Vec<Binding>>,
    current_depth: usize,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            globals: HashMap::new(),
            local_scopes: vec![vec![]],
            local_bindings: HashMap::new(),
            current_depth: 0,
        }
    }

    pub fn insert_global(&mut self, name: Symbol, ty: HirType, mutable: bool) {
        self.globals.insert(
            name,
            Binding {
                ty,
                mutable,
                depth: 0,
            },
        );
    }

    pub fn push_scope(&mut self) {
        self.local_scopes.push(vec![]);
        self.current_depth += 1;
    }

    pub fn pop_scope(&mut self) {
        if self.local_scopes.len() <= 1 {
            return;
        }
        let popped = self.local_scopes.pop().unwrap();
        self.current_depth -= 1;
        for name in popped {
            if let Some(stack) = self.local_bindings.get_mut(&name) {
                stack.retain(|b| b.depth < self.current_depth);
                if stack.is_empty() {
                    self.local_bindings.remove(&name);
                }
            }
        }
    }

    pub fn insert(&mut self, name: Symbol, ty: HirType, mutable: bool) {
        self.local_scopes.last_mut().unwrap().push(name);
        self.local_bindings.entry(name).or_default().push(Binding {
            ty,
            mutable,
            depth: self.current_depth,
        });
    }

    pub fn lookup(&self, name: Symbol) -> Option<&HirType> {
        self.local_bindings
            .get(&name)
            .and_then(|stack| stack.last().map(|b| &b.ty))
            .or_else(|| self.globals.get(&name).map(|b| &b.ty))
    }

    pub fn is_mutable(&self, name: Symbol) -> bool {
        self.local_bindings
            .get(&name)
            .and_then(|stack| stack.last().map(|b| b.mutable))
            .or_else(|| self.globals.get(&name).map(|b| b.mutable))
            .unwrap_or(false)
    }

    pub fn clear_locals(&mut self) {
        self.local_scopes = vec![vec![]];
        self.local_bindings.clear();
        self.current_depth = 0;
    }
}
