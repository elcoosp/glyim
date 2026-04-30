mod error;
mod expr;
mod function;
mod match_check;
mod register;
mod resolver;
mod scope;
mod stmt;
mod types;

pub use error::TypeError;
pub use types::{EnumInfo, StructInfo};

use glyim_hir::item::FnSig;
use glyim_hir::node::{Hir, HirFn};
use glyim_hir::types::{ExprId, HirType};
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

pub struct TypeChecker {
    pub interner: Interner,
    pub(crate) scopes: Vec<types::Scope>,
    pub structs: HashMap<Symbol, StructInfo>,
    pub enums: HashMap<Symbol, EnumInfo>,
    pub extern_fns: HashMap<Symbol, FnSig>,
    pub impl_methods: HashMap<Symbol, Vec<HirFn>>,
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    pub return_type: Option<HirType>,
    pub errors: Vec<TypeError>,
    fns: Vec<HirFn>,
}

impl TypeChecker {
    pub fn new(interner: Interner) -> Self {
        TypeChecker {
            interner,
            scopes: Vec::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
            extern_fns: HashMap::new(),
            impl_methods: HashMap::new(),
            expr_types: Vec::new(),
            call_type_args: HashMap::new(),
            return_type: None,
            errors: Vec::new(),
            fns: Vec::new(),
        }
    }

    fn register_builtin_enums(&mut self) {
        // Register Option<T> and Result<T, E> as known enums
        let opt_name = self.interner.intern("Option");
        let result_name = self.interner.intern("Result");
        let some = self.interner.intern("Some");
        let none = self.interner.intern("None");
        let ok = self.interner.intern("Ok");
        let err = self.interner.intern("Err");

        let opt_variants = [
            glyim_hir::item::HirVariant {
                name: some,
                fields: vec![],
                tag: 0,
            },
            glyim_hir::item::HirVariant {
                name: none,
                fields: vec![],
                tag: 1,
            },
        ];
        let res_variants = [
            glyim_hir::item::HirVariant {
                name: ok,
                fields: vec![],
                tag: 0,
            },
            glyim_hir::item::HirVariant {
                name: err,
                fields: vec![],
                tag: 1,
            },
        ];

        self.enums.insert(
            opt_name,
            EnumInfo {
                variants: opt_variants.to_vec(),
                variant_map: vec![(some, 0), (none, 1)].into_iter().collect(),
                type_params: vec![],
            },
        );
        self.enums.insert(
            result_name,
            EnumInfo {
                variants: res_variants.to_vec(),
                variant_map: vec![(ok, 0), (err, 1)].into_iter().collect(),
                type_params: vec![],
            },
        );
    }

    fn set_type(&mut self, id: ExprId, ty: HirType) {
        let idx = id.as_usize();
        if idx >= self.expr_types.len() {
            self.expr_types.resize(idx + 1, HirType::Never);
        }
        self.expr_types[idx] = ty;
    }

    fn dummy_symbol(&self) -> Symbol {
        glyim_interner::Interner::new().intern("__dummy")
    }

    #[tracing::instrument(skip_all)]
    #[tracing::instrument(skip_all)]
    pub fn check(&mut self, hir: &Hir) -> Result<(), Vec<TypeError>> {
        self.register_builtin_enums();
        self.register_items(hir);
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Fn(f) => {
                    self.check_fn(f);
                }
                glyim_hir::item::HirItem::Impl(imp) => {
                    for method in &imp.methods {
                        self.check_fn(method);
                    }
                }
                _ => {}
            }
        }
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    pub fn get_expr_type(&self, id: ExprId) -> Option<&HirType> {
        self.expr_types.get(id.as_usize())
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new(Interner::new())
    }
}

#[cfg(test)]
mod tests;
