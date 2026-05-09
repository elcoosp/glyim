//! Deterministic name mangling table for monomorphized items.
//!
//! Guarantees that mangled names are stable and reusable across the
//! entire monomorphization pipeline. The same (base, args) always
//! produces the same mangled symbol.

use crate::types::HirType;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

/// A table that maps `(base_symbol, type_args)` to a single mangled symbol.
///
/// Used for both type names and function names. The mangling scheme is:
///
///   `base__arg1_arg2_...`
///
/// For example:
///   - `Vec__i64`         (struct or enum)
///   - `id__i64`          (function)
///   - `HashMap__str_i64` (two type args)
///
/// Non-generic items (empty type args) keep their original name.
#[derive(Debug, Default)]
pub struct MangleTable {
    /// Cache: (base_symbol, type_args) → mangled_symbol
    map: HashMap<(Symbol, Vec<HirType>), Symbol>,
}

impl MangleTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the mangled symbol for `base` with the given concrete type arguments.
    ///
    /// Creates a new entry on first use using `glyim_hir::monomorphize::mangling`.
    /// Subsequent calls with the same (base, args) return the cached symbol.
    pub fn mangle(&mut self, base: Symbol, args: &[HirType], interner: &mut Interner) -> Symbol {
        let key = (base, args.to_vec());
        if let Some(&mangled) = self.map.get(&key) {
            return mangled;
        }
        let mangled = super::mangling::mangle_type_name(interner, base, args);
        self.map.insert(key, mangled);
        mangled
    }

    /// Mangle a function name with the given concrete type arguments.
    ///
    /// For non-generic functions (empty `args`), returns `base` unchanged.
    /// For generic functions, delegates to `mangle`.
    pub fn mangle_fn(&mut self, base: Symbol, args: &[HirType], interner: &mut Interner) -> Symbol {
        if args.is_empty() {
            return base;
        }
        self.mangle(base, args, interner)
    }

    /// Number of unique mangled names generated so far.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::HirType;
    use glyim_interner::Interner;

    #[test]
    fn mangle_table_is_deterministic() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let args = vec![HirType::Int];
        let mut table = MangleTable::new();

        let s1 = table.mangle(vec_sym, &args, &mut interner);
        let s2 = table.mangle(vec_sym, &args, &mut interner);
        assert_eq!(s1, s2, "same (base, args) must produce identical mangled symbol");
    }

    #[test]
    fn mangle_table_different_args_different_names() {
        let mut interner = Interner::new();
        let vec_sym = interner.intern("Vec");
        let mut table = MangleTable::new();

        let s_i64 = table.mangle(vec_sym, &[HirType::Int], &mut interner);
        let s_bool = table.mangle(vec_sym, &[HirType::Bool], &mut interner);
        assert_ne!(s_i64, s_bool, "different type args must produce distinct names");
    }

    #[test]
    fn mangle_fn_no_args_returns_original() {
        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        let mut table = MangleTable::new();

        let result = table.mangle_fn(foo, &[], &mut interner);
        assert_eq!(result, foo, "Non-generic fn should keep original name");
    }

    #[test]
    fn mangle_fn_with_args_mangles() {
        let mut interner = Interner::new();
        let id = interner.intern("id");
        let mut table = MangleTable::new();

        let result = table.mangle_fn(id, &[HirType::Int], &mut interner);
        assert_eq!(interner.resolve(result), "id__i64");
    }

    #[test]
    fn mangle_fn_deterministic() {
        let mut interner = Interner::new();
        let id = interner.intern("id");
        let mut table = MangleTable::new();

        let s1 = table.mangle_fn(id, &[HirType::Int], &mut interner);
        let s2 = table.mangle_fn(id, &[HirType::Int], &mut interner);
        assert_eq!(s1, s2);
    }

    #[test]
    fn mangle_table_handles_multiple_args() {
        let mut interner = Interner::new();
        let hashmap_sym = interner.intern("HashMap");
        let mut table = MangleTable::new();

        let s1 = table.mangle(hashmap_sym, &[HirType::Str, HirType::Int], &mut interner);
        let s2 = table.mangle(hashmap_sym, &[HirType::Str, HirType::Int], &mut interner);
        assert_eq!(s1, s2);

        let name = interner.resolve(s1);
        assert!(name.starts_with("HashMap__"), "expected HashMap__ prefix, got {}", name);
        assert!(name.contains("str") && name.contains("i64"), "expected str and i64, got {}", name);
    }
}
