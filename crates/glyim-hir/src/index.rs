use crate::item::*;
use crate::node::*;
use glyim_interner::Symbol;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: Symbol,
    pub fields: Vec<(Symbol, crate::types::HirType)>,
    pub field_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub name: Symbol,
    pub variants: Vec<HirVariant>,
    pub variant_map: HashMap<Symbol, usize>,
    pub type_params: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct FnInfo {
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub params: Vec<(Symbol, crate::types::HirType)>,
    pub ret: Option<crate::types::HirType>,
    pub is_generic: bool,
}

#[derive(Debug, Clone)]
pub struct HirIndex {
    pub structs: HashMap<Symbol, StructInfo>,
    pub enums: HashMap<Symbol, EnumInfo>,
    pub fns: HashMap<Symbol, FnInfo>,
    pub extern_fns: HashMap<Symbol, crate::item::ExternFn>,
    pub generic_fn_names: HashSet<Symbol>,
    pub generic_struct_names: HashSet<Symbol>,
    pub generic_enum_names: HashSet<Symbol>,
    // Maps type name -> set of mangled method names defined in impl blocks
    pub impl_methods: HashMap<Symbol, HashSet<Symbol>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IndexError {
    DuplicateStruct { name: Symbol },
    DuplicateEnum { name: Symbol },
    DuplicateFn { name: Symbol },
    DuplicateMethod { target: Symbol, method: Symbol },
}

impl HirIndex {
    pub fn build(hir: &crate::Hir) -> Result<Self, IndexError> {
        let mut idx = Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            fns: HashMap::new(),
            extern_fns: HashMap::new(),
            generic_fn_names: HashSet::new(),
            generic_struct_names: HashSet::new(),
            generic_enum_names: HashSet::new(),
            impl_methods: HashMap::new(),
        };

        for item in &hir.items {
            match item {
                HirItem::Struct(s) => idx.register_struct(s)?,
                HirItem::Enum(e) => idx.register_enum(e)?,
                HirItem::Fn(f) => idx.register_fn(f)?,
                HirItem::Extern(ext) => {
                    for ef in &ext.functions {
                        idx.extern_fns.insert(ef.name, ef.clone());
                    }
                }
                HirItem::Impl(imp) => idx.register_impl(imp)?,
            }
        }

        Ok(idx)
    }

    fn register_struct(&mut self, s: &StructDef) -> Result<(), IndexError> {
        if self.structs.contains_key(&s.name) {
            return Err(IndexError::DuplicateStruct { name: s.name });
        }
        let mut field_map = HashMap::new();
        for (i, f) in s.fields.iter().enumerate() {
            field_map.insert(f.name, i);
        }
        if !s.type_params.is_empty() {
            self.generic_struct_names.insert(s.name);
        }
        self.structs.insert(s.name, StructInfo {
            name: s.name,
            fields: s.fields.iter().map(|f| (f.name, f.ty.clone())).collect(),
            field_map,
            type_params: s.type_params.clone(),
        });
        Ok(())
    }

    fn register_enum(&mut self, e: &EnumDef) -> Result<(), IndexError> {
        if self.enums.contains_key(&e.name) {
            return Err(IndexError::DuplicateEnum { name: e.name });
        }
        let mut variant_map = HashMap::new();
        for (i, v) in e.variants.iter().enumerate() {
            variant_map.insert(v.name, i);
        }
        if !e.type_params.is_empty() {
            self.generic_enum_names.insert(e.name);
        }
        self.enums.insert(e.name, EnumInfo {
            name: e.name,
            variants: e.variants.clone(),
            variant_map,
            type_params: e.type_params.clone(),
        });
        Ok(())
    }

    fn register_fn(&mut self, f: &HirFn) -> Result<(), IndexError> {
        if self.fns.contains_key(&f.name) {
            return Err(IndexError::DuplicateFn { name: f.name });
        }
        let is_generic = !f.type_params.is_empty();
        if is_generic {
            self.generic_fn_names.insert(f.name);
        }
        self.fns.insert(f.name, FnInfo {
            name: f.name,
            type_params: f.type_params.clone(),
            params: f.params.clone(),
            ret: f.ret.clone(),
            is_generic,
        });
        Ok(())
    }

    fn register_impl(&mut self, imp: &HirImplDef) -> Result<(), IndexError> {
        // Collect method names first to avoid borrow conflicts
        let method_names: Vec<_> = imp.methods.iter().map(|m| m.name).collect();

        {
            let methods = self.impl_methods.entry(imp.target_name).or_default();
            for &name in &method_names {
                if methods.contains(&name) {
                    return Err(IndexError::DuplicateMethod { target: imp.target_name, method: name });
                }
                methods.insert(name);
            }
        }

        // Register each method as a top-level function
        for m in &imp.methods {
            self.register_fn(m)?;
        }
        Ok(())
    }

    pub fn find_fn(&self, name: Symbol) -> Option<&FnInfo> {
        self.fns.get(&name)
    }

    pub fn find_struct(&self, name: Symbol) -> Option<&StructInfo> {
        self.structs.get(&name)
    }

    pub fn find_enum(&self, name: Symbol) -> Option<&EnumInfo> {
        self.enums.get(&name)
    }

    pub fn lookup_method(&self, type_name: Symbol, method_name: Symbol) -> Option<&FnInfo> {
        let methods = self.impl_methods.get(&type_name)?;
        if methods.contains(&method_name) {
            self.fns.get(&method_name)
        } else {
            None
        }
    }
}
