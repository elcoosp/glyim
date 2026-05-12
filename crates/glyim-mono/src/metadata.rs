use glyim_hir::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum TypeStructure {
    Generic { base: Symbol, args: Vec<HirType> },
    Plain { base: Symbol },
}

#[derive(Debug, Default)]
pub struct TypeMetadata {
    map: HashMap<Symbol, TypeStructure>,
}

impl TypeMetadata {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, name: Symbol, structure: TypeStructure) {
        self.map.insert(name, structure);
    }

    pub fn get(&self, name: Symbol) -> Option<&TypeStructure> {
        self.map.get(&name)
    }

    pub fn get_base_symbol(&self, name: Symbol) -> Option<Symbol> {
        self.map.get(&name).map(|s| match s {
            TypeStructure::Generic { base, .. } => *base,
            TypeStructure::Plain { base } => *base,
        })
    }
}
