//! O(1) lookup tables for HIR items.
//!
//! Built once from the input HIR before monomorphization begins.
//! All lookups are immutable — the index is never modified during BFS.

use crate::item::{EnumDef, HirImplDef, StructDef};
use crate::node::HirFn;
use crate::Hir;
use crate::HirItem;
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

/// Pre-built index over a Hir for O(1) lookups during monomorphization.
///
/// Every query method returns `Option<&T>` — no cloning, no linear scans.
/// The index owns clones of the original definitions so that the BFS
/// can freely consume and mutate them without aliasing the source Hir.
pub struct MonoIndex {
    /// Struct definitions indexed by name symbol.
    structs: HashMap<Symbol, StructDef>,

    /// Enum definitions indexed by name symbol.
    enums: HashMap<Symbol, EnumDef>,

    /// Top-level function definitions indexed by name symbol.
    fns: HashMap<Symbol, HirFn>,

    /// Impl methods indexed by their mangled name (e.g., `Vec_push`).
    /// These are the names produced by `lower/item.rs` during lowering.
    impl_methods: HashMap<Symbol, HirFn>,

    /// All impl blocks, indexed by target type name.
    impls: HashMap<Symbol, HirImplDef>,

    /// Pre-computed set of method names that need specialization because
    /// they belong to a generic struct or enum impl block.
    /// E.g., `Vec_new` has type_params=[] but belongs to impl Vec<T>,
    /// so it needs specialization with concrete type args for T.
    needs_specialization: HashSet<Symbol>,
}

impl MonoIndex {
    /// Build the index by scanning all items in the Hir exactly once.
    pub fn build(hir: &Hir) -> Self {
        let mut index = Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            fns: HashMap::new(),
            impl_methods: HashMap::new(),
            impls: HashMap::new(),
            needs_specialization: HashSet::new(),
        };

        for item in &hir.items {
            match item {
                HirItem::Struct(s) => {
                    index.structs.insert(s.name, s.clone());
                }
                HirItem::Enum(e) => {
                    index.enums.insert(e.name, e.clone());
                }
                HirItem::Fn(f) => {
                    index.fns.insert(f.name, f.clone());
                }
                HirItem::Impl(imp) => {
                    // Check if this impl's target type is generic.
                    // Methods on generic structs/enums need specialization
                    // even when the method itself has no type_params.
                    let is_generic_target = index.find_struct(imp.target_name)
                        .map(|s| !s.type_params.is_empty())
                        .unwrap_or(false)
                        || index.find_enum(imp.target_name)
                            .map(|e| !e.type_params.is_empty())
                            .unwrap_or(false)
                        || !imp.type_params.is_empty();

                    for m in &imp.methods {
                        // Impl methods are already mangled by the lower pass
                        // (e.g., `Vec_push`). Index them by that mangled name.
                        index.impl_methods.insert(m.name, m.clone());

                        // If the impl target is generic, mark this method as
                        // needing specialization. For example, Vec_new has
                        // type_params=[] but belongs to impl Vec<T>, so it
                        // must be specialized with concrete type args for T.
                        if is_generic_target {
                            index.needs_specialization.insert(m.name);
                        }
                    }
                    index.impls.insert(imp.target_name, imp.clone());
                }
                HirItem::Extern(_) => {
                    // Extern blocks don't need indexing — they're passed
                    // through unchanged.
                }
            }
        }

        index
    }

    // ── Lookup methods ──────────────────────────────────────────────

    /// Look up a struct definition by name.
    pub fn find_struct(&self, name: Symbol) -> Option<&StructDef> {
        self.structs.get(&name)
    }

    /// Look up an enum definition by name.
    pub fn find_enum(&self, name: Symbol) -> Option<&EnumDef> {
        self.enums.get(&name)
    }

    /// Look up a function by name.
    ///
    /// Checks top-level functions first, then impl methods.
    /// This matches the precedence of the old `find_fn` which
    /// scanned `HirItem::Fn` before `HirItem::Impl`.
    pub fn find_fn(&self, name: Symbol) -> Option<&HirFn> {
        self.fns.get(&name).or_else(|| self.impl_methods.get(&name))
    }

    /// Look up an impl block by target type name.
    pub fn find_impl(&self, target: Symbol) -> Option<&HirImplDef> {
        self.impls.get(&target)
    }

    // ── Generic queries ─────────────────────────────────────────────

    /// Returns true if the named function needs specialization.
    ///
    /// A function needs specialization if it has its own type parameters,
    /// OR if it is an impl method of a generic struct/enum. For example,
    /// `Vec_new` has `type_params: []` but belongs to `impl Vec<T>`,
    /// so it must be specialized with concrete type args for `T`.
    pub fn is_generic_fn(&self, name: Symbol) -> bool {
        self.find_fn(name).map(|f| !f.type_params.is_empty()).unwrap_or(false)
            || self.needs_specialization.contains(&name)
    }

    /// Returns true if the named struct has type parameters.
    pub fn is_generic_struct(&self, name: Symbol) -> bool {
        self.find_struct(name).map(|s| !s.type_params.is_empty()).unwrap_or(false)
    }

    /// Returns true if the named enum has type parameters.
    pub fn is_generic_enum(&self, name: Symbol) -> bool {
        self.find_enum(name).map(|e| !e.type_params.is_empty()).unwrap_or(false)
    }

    // ── Iterators for seeding the work queue ─────────────────────────

    /// All top-level function names (for seeding passthrough work items).
    pub fn fn_names(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.fns.keys().copied()
    }

    /// All impl method mangled names (for seeding passthrough work items).
    pub fn impl_method_names(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.impl_methods.keys().copied()
    }

    /// All struct names.
    pub fn struct_names(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.structs.keys().copied()
    }

    /// All enum names.
    pub fn enum_names(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.enums.keys().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{StructDef, StructField};
    use crate::node::HirFn;
    use crate::types::HirType;
    use glyim_diag::Span;
    use glyim_interner::Interner;

    fn make_hir_with_struct(interner: &mut Interner, name: &str, type_param_names: &[&str]) -> crate::Hir {
        let name_sym = interner.intern(name);
        let type_params: Vec<Symbol> = type_param_names.iter().map(|t| interner.intern(t)).collect();
        crate::Hir {
            items: vec![crate::HirItem::Struct(StructDef {
                doc: None,
                name: name_sym,
                type_params,
                fields: vec![StructField {
                    name: interner.intern("data"),
                    ty: HirType::Int,
                    doc: None,
                }],
                span: Span::new(0, 0),
                is_pub: false,
            })],
        }
    }

    #[test]
    fn index_finds_struct() {
        let mut interner = Interner::new();
        let hir = make_hir_with_struct(&mut interner, "Pair", &["T"]);
        let pair_sym = interner.intern("Pair");

        let index = MonoIndex::build(&hir);

        assert!(index.find_struct(pair_sym).is_some());
        assert!(index.is_generic_struct(pair_sym));

        let missing = interner.intern("Missing");
        assert!(index.find_struct(missing).is_none());
        assert!(!index.is_generic_struct(missing));
    }

    #[test]
    fn index_finds_non_generic_struct() {
        let mut interner = Interner::new();
        let hir = make_hir_with_struct(&mut interner, "Point", &[]);
        let point_sym = interner.intern("Point");

        let index = MonoIndex::build(&hir);

        assert!(index.find_struct(point_sym).is_some());
        assert!(!index.is_generic_struct(point_sym));
    }

    #[test]
    fn index_finds_fn_and_impl_method() {
        let mut interner = Interner::new();
        let id_sym = interner.intern("id");
        let push_sym = interner.intern("Vec_push");
        let vec_sym = interner.intern("Vec");

        let hir = crate::Hir {
            items: vec![
                crate::HirItem::Fn(HirFn {
                    doc: None,
                    name: id_sym,
                    type_params: vec![interner.intern("T")],
                    params: vec![(interner.intern("x"), HirType::Int)],
                    param_mutability: vec![false],
                    ret: None,
                    body: crate::node::HirExpr::IntLit {
                        id: crate::types::ExprId::new(0),
                        value: 0,
                        span: Span::new(0, 0),
                    },
                    span: Span::new(0, 0),
                    is_pub: false,
                    is_macro_generated: false,
                    is_extern_backed: false,
                    is_test: false,
                    test_config: None,
                }),
                crate::HirItem::Impl(crate::item::HirImplDef {
                    doc: None,
                    target_name: vec_sym,
                    type_params: vec![interner.intern("T")],
                    methods: vec![HirFn {
                        doc: None,
                        name: push_sym,
                        type_params: vec![],
                        params: vec![(interner.intern("self"), HirType::Int)],
                        param_mutability: vec![false],
                        ret: None,
                        body: crate::node::HirExpr::IntLit {
                            id: crate::types::ExprId::new(1),
                            value: 0,
                            span: Span::new(0, 0),
                        },
                        span: Span::new(0, 0),
                        is_pub: false,
                        is_macro_generated: false,
                        is_extern_backed: false,
                        is_test: false,
                        test_config: None,
                    }],
                    span: Span::new(0, 0),
                    is_pub: false,
                }),
            ],
        };

        let index = MonoIndex::build(&hir);

        // Top-level function
        assert!(index.find_fn(id_sym).is_some());
        assert!(index.is_generic_fn(id_sym));

        // Impl method (mangled name)
        assert!(index.find_fn(push_sym).is_some(), "Should find impl method by mangled name");

        // fn_names only returns top-level, impl_method_names returns impl methods
        let fn_name_set: Vec<Symbol> = index.fn_names().collect();
        assert!(fn_name_set.contains(&id_sym));
        assert!(!fn_name_set.contains(&push_sym), "impl methods not in fn_names");

        let impl_name_set: Vec<Symbol> = index.impl_method_names().collect();
        assert!(impl_name_set.contains(&push_sym));
    }

    #[test]
    fn index_finds_enum() {
        let mut interner = Interner::new();
        let opt_sym = interner.intern("Option");

        let hir = crate::Hir {
            items: vec![crate::HirItem::Enum(crate::item::EnumDef {
                doc: None,
                name: opt_sym,
                type_params: vec![interner.intern("T")],
                variants: vec![],
                span: Span::new(0, 0),
                is_pub: false,
            })],
        };

        let index = MonoIndex::build(&hir);

        assert!(index.find_enum(opt_sym).is_some());
        assert!(index.is_generic_enum(opt_sym));
    }

    #[test]
    fn index_empty_hir() {
        let hir = crate::Hir { items: vec![] };
        let index = MonoIndex::build(&hir);

        let mut interner = Interner::new();
        let foo = interner.intern("foo");
        assert!(index.find_fn(foo).is_none());
        assert!(index.find_struct(foo).is_none());
        assert!(index.find_enum(foo).is_none());
        assert!(!index.is_generic_fn(foo));
    }
}
