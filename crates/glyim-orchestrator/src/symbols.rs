use glyim_interner::Symbol;
use glyim_macro_vfs::ContentHash;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

/// Maps exported symbols to their defining package and artifact hash.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageSymbolTable {
    /// (symbol_name) → (package_name, content_hash_of_object_code)
    exports: HashMap<Symbol, (String, ContentHash)>,
    /// (package_name) → Vec<Symbol>  — all symbols exported by a package
    package_exports: HashMap<String, Vec<Symbol>>,
}

impl PackageSymbolTable {
    pub fn new() -> Self {
        Self {
            exports: HashMap::new(),
            package_exports: HashMap::new(),
        }
    }

    /// Register a symbol exported by a package, along with its object code hash.
    pub fn register_export(&mut self, package: &str, symbol: Symbol, artifact_hash: ContentHash) {
        self.exports.insert(symbol, (package.to_string(), artifact_hash));
        self.package_exports
            .entry(package.to_string())
            .or_default()
            .push(symbol);
    }

    /// Resolve a symbol to its defining package and artifact hash.
    pub fn resolve(&self, symbol: Symbol) -> Option<(&str, &ContentHash)> {
        self.exports.get(&symbol).map(|(pkg, hash)| (pkg.as_str(), hash))
    }

    /// Return all symbols exported by a package.
    pub fn package_exports(&self, package: &str) -> &[Symbol] {
        self.package_exports
            .get(package)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Merge another symbol table into this one.
    pub fn merge(&mut self, other: &PackageSymbolTable) {
        for (sym, (pkg, hash)) in &other.exports {
            self.exports.insert(*sym, (pkg.clone(), *hash));
        }
        for (pkg, syms) in &other.package_exports {
            self.package_exports
                .entry(pkg.clone())
                .or_default()
                .extend_from_slice(syms);
        }
    }

    /// Number of exported symbols.
    pub fn len(&self) -> usize {
        self.exports.len()
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.exports.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    #[test]
    fn register_and_resolve_symbol() {
        let mut table = PackageSymbolTable::new();
        let mut interner = Interner::new();
        let sym = interner.intern("add");
        let hash = ContentHash::of(b"object_code");

        table.register_export("math-lib", sym, hash);
        let (pkg, h) = table.resolve(sym).unwrap();
        assert_eq!(pkg, "math-lib");
        assert_eq!(*h, hash);
    }

    #[test]
    fn unknown_symbol_returns_none() {
        let table = PackageSymbolTable::new();
        let mut interner = Interner::new();
        let sym = interner.intern("unknown");
        assert!(table.resolve(sym).is_none());
    }

    #[test]
    fn package_exports_lists_all_symbols() {
        let mut table = PackageSymbolTable::new();
        let mut interner = Interner::new();
        let sym1 = interner.intern("fn_a");
        let sym2 = interner.intern("fn_b");
        let hash = ContentHash::of(b"obj");

        table.register_export("pkg1", sym1, hash);
        table.register_export("pkg1", sym2, hash);

        let exports = table.package_exports("pkg1");
        assert_eq!(exports.len(), 2);
        assert!(exports.contains(&sym1));
        assert!(exports.contains(&sym2));
    }

    #[test]
    fn merge_combines_tables() {
        let mut table1 = PackageSymbolTable::new();
        let mut table2 = PackageSymbolTable::new();
        let mut interner = Interner::new();
        let sym1 = interner.intern("x");
        let sym2 = interner.intern("y");
        let hash = ContentHash::of(b"obj");

        table1.register_export("a", sym1, hash);
        table2.register_export("b", sym2, hash);
        table1.merge(&table2);

        assert_eq!(table1.len(), 2);
        assert!(table1.resolve(sym1).is_some());
        assert!(table1.resolve(sym2).is_some());
    }

    #[test]
    fn serialize_roundtrip() {
        let mut table = PackageSymbolTable::new();
        let mut interner = Interner::new();
        let sym = interner.intern("test_fn");
        let hash = ContentHash::of(b"data");
        table.register_export("test-pkg", sym, hash);

        let bytes = postcard::to_allocvec(&table).unwrap();
        let restored: PackageSymbolTable = postcard::from_bytes(&bytes).unwrap();

        let (pkg, h) = restored.resolve(sym).unwrap();
        assert_eq!(pkg, "test-pkg");
        assert_eq!(*h, hash);
    }
}
