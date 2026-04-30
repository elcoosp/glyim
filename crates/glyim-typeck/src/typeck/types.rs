use glyim_hir::item::{HirVariant, StructField};
use glyim_hir::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct StructInfo {
    pub fields: Vec<StructField>,
    pub field_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Clone, Debug)]
pub struct EnumInfo {
    pub variants: Vec<HirVariant>,
    pub variant_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Clone, Debug)]
pub(crate) struct Binding { pub ty: HirType, pub mutable: bool }

#[derive(Clone, Debug)]
pub(crate) struct Scope { pub bindings: HashMap<Symbol, Binding> }

impl Scope {
    pub fn new() -> Self { Self { bindings: HashMap::new() } }
    pub fn insert(&mut self, name: Symbol, ty: HirType, mutable: bool) {
        self.bindings.insert(name, Binding { ty, mutable });
    }
    pub fn lookup(&self, name: &Symbol) -> Option<&HirType> {
        self.bindings.get(name).map(|b| &b.ty)
    }
    pub fn lookup_binding(&self, name: &Symbol) -> Option<&Binding> {
        self.bindings.get(name)
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}
