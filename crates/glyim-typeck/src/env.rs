//! Lexical-scope-aware local variable environment.
//!
//! Tracks variable bindings with proper scoping and shadowing support.
//! Variables are allocated sequential `LocalVarId`s (params first, then
//! let-bindings). When a scope is exited, all bindings added within it
//! are removed from name resolution, but their IDs are never reused.
//! Lexical-scope-aware local variable environment.

//! HIR `TypeRef` → `Ty` conversion.

//! Lexical-scope-aware local variable environment.

//! Lexical-scope-aware local variable environment.

//! Lexical-scope-aware local variable environment.

//! Lexical-scope-aware local variable environment.

use std::collections::HashMap;

use glyim_core::interner::Name;
use glyim_core::primitives::Mutability;
use glyim_type::Ty;

use crate::thir::LocalVarId;

/// Information about a single local variable binding.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct LocalVarInfo {
    /// The allocated ID for this variable.
    pub id: LocalVarId,
    /// The name as written in source.
    pub name: Name,
    /// The resolved type of the binding.
    pub ty: Ty,
    /// Whether the binding is mutable.
    pub mutability: Mutability,
}

/// A stack-based lexical variable environment.
#[derive(Clone, Debug)]
pub struct LocalEnv {
    name_map: HashMap<Name, Vec<LocalVarId>>,
    vars: Vec<LocalVarInfo>,
    scope_stack: Vec<usize>,
}

impl LocalEnv {
    #[inline]
    pub fn new() -> Self {
        Self {
            name_map: HashMap::new(),
            vars: Vec::new(),
            scope_stack: Vec::new(),
        }
    }

    #[inline]
    pub fn enter_scope(&mut self) {
        self.scope_stack.push(self.vars.len());
    }

    pub fn leave_scope(&mut self) {
        let base = self
            .scope_stack
            .pop()
            .expect("leave_scope without matching enter_scope");

        // Iterate over the variables added in this scope to update the name_map,
        // but do NOT pop them from self.vars so that LocalVarIds remain stable
        // and accessible for the duration of the function (e.g. after a block exits).
        for i in base..self.vars.len() {
            let var = &self.vars[i];
            if let Some(stack) = self.name_map.get_mut(&var.name) {
                stack.pop();
                if stack.is_empty() {
                    self.name_map.remove(&var.name);
                }
            }
        }
    }

    pub fn add_binding(&mut self, name: Name, ty: Ty, mutability: Mutability) -> LocalVarId {
        let id = LocalVarId::from_raw(self.vars.len() as u32);
        self.vars.push(LocalVarInfo {
            id,
            name,
            ty,
            mutability,
        });
        self.name_map.entry(name).or_default().push(id);
        id
    }

    /// Register an *additional* name for an already-bound `LocalVarId`
    /// (without allocating a new id). Used when a binding's name is visible
    /// under two interner namespaces (e.g. the HIR `Name` and the
    /// type-checker's remapped `Name`) so that lexical lookups via either key
    /// resolve to the same variable.
    pub fn add_alias(&mut self, name: Name, id: LocalVarId) {
        self.name_map.entry(name).or_default().push(id);
    }

    #[inline]
    pub fn lookup_by_name(&self, name: Name) -> Option<&LocalVarInfo> {
        self.name_map
            .get(&name)
            .and_then(|stack| stack.last())
            .and_then(|&id| self.vars.get(id.to_raw() as usize))
    }

    /// Debug helper: the set of names currently bound in the environment.
    #[allow(dead_code)]
    pub fn name_map_keys(&self) -> Vec<Name> {
        self.name_map.keys().copied().collect()
    }

    #[inline]
    #[allow(dead_code)]
    pub fn lookup_by_id(&self, id: LocalVarId) -> Option<&LocalVarInfo> {
        self.vars.get(id.to_raw() as usize)
    }

    /// Returns the `LocalVarId` that the *next* added binding will receive.
    ///
    /// `LocalVarId`s are allocated sequentially and never reused, so this is
    /// a stable "boundary": any `VarRef` resolved after this point belongs to
    /// an inner scope (e.g. a closure body or block), while any resolved
    /// before belongs to an enclosing scope (hence is a capture).
    #[inline]
    pub fn next_var_id(&self) -> LocalVarId {
        LocalVarId::from_raw(self.vars.len() as u32)
    }
}

impl Default for LocalEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::interner::Interner;
    use glyim_core::primitives::Mutability;

    /// Smoke-test: params, let-binding, shadowing, scope exit.
    #[test]
    fn env_scoping_and_shadowing() {
        let mut env = LocalEnv::new();
        let interner = Interner::new();

        let x = interner.intern("x");
        let y = interner.intern("y");
        let ty_i32 = Ty::UNIT; // placeholder — any Ty will do for this test

        // Parameter
        let x0 = env.add_binding(x, ty_i32, Mutability::Not);
        assert_eq!(env.lookup_by_name(x).unwrap().ty, ty_i32);

        // Enter inner scope, shadow x
        env.enter_scope();
        let x1 = env.add_binding(x, ty_i32, Mutability::Mut);
        assert_eq!(env.lookup_by_name(x).unwrap().mutability, Mutability::Mut);
        assert_ne!(x0, x1);

        // Add y in inner scope
        let _y0 = env.add_binding(y, ty_i32, Mutability::Not);
        assert!(env.lookup_by_name(y).is_some());

        // Leave inner scope — x shadow and y should disappear
        env.leave_scope();
        assert_eq!(env.lookup_by_name(x).unwrap().mutability, Mutability::Not);
        assert!(env.lookup_by_name(y).is_none());

        // IDs are stable after scope exit
        assert_eq!(env.lookup_by_id(x0).unwrap().name, x);
        assert_eq!(env.lookup_by_id(x1).unwrap().name, x); // still accessible by ID
    }

    #[test]
    #[should_panic(expected = "leave_scope without matching enter_scope")]
    fn env_unbalanced_leave_panics() {
        let mut env = LocalEnv::new();
        env.leave_scope();
    }
}
