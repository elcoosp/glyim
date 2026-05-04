//! Deterministic name mangling table for monomorphized types.
//!
//! Ensures that mangled names are stable and reusable across the
//! discovery → specialization → rewriting pipeline.

use crate::types::HirType;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

/// A table that maps (base_symbol, type_args) to a single mangled symbol.
/// Guarantees deterministic naming: the same (base, args) always produces
/// the same mangled symbol.
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
    /// Creates a new entry on first use using `glyim_hir::monomorphize::mangling`.
    pub fn mangle(&mut self, base: Symbol, args: &[HirType], interner: &mut Interner) -> Symbol {
        let key = (base, args.to_vec());
        if let Some(&mangled) = self.map.get(&key) {
            return mangled;
        }
        let mangled = super::mangling::mangle_type_name(interner, base, args);
        self.map.insert(key, mangled);
        mangled
    }

    /// Number of unique mangled names generated so far.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}
