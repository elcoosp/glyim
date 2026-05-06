use glyim_hir::types::HirType;
use glyim_interner::Symbol;
use serde::{Serialize, Deserialize};

/// The public interface of a package, used by dependents for type-safe compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInterface {
    pub package_name: String,
    pub version: String,
    pub functions: Vec<InterfaceFn>,
    pub structs: Vec<InterfaceStruct>,
    pub enums: Vec<InterfaceEnum>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceFn {
    pub name: Symbol,
    pub mangled_name: String,
    pub params: Vec<HirType>,
    pub return_type: HirType,
    pub is_pub: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceStruct {
    pub name: Symbol,
    pub fields: Vec<(Symbol, HirType)>,
    pub type_params: Vec<Symbol>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceEnum {
    pub name: Symbol,
    pub variants: Vec<(Symbol, Vec<(Symbol, HirType)>)>,
    pub type_params: Vec<Symbol>,
}

impl DependencyInterface {
    /// Compute the interface from a fully type-checked HIR.
    pub fn from_hir(
        hir: &glyim_hir::Hir,
        package_name: &str,
        version: &str,
        interner: &glyim_interner::Interner,
    ) -> Self {
        use glyim_hir::item::HirItem;

        let mut functions = Vec::new();
        let mut structs = Vec::new();
        let mut enums = Vec::new();

        for item in &hir.items {
            match item {
                HirItem::Fn(f) if f.is_pub => {
                    let mangled_name = interner.resolve(f.name).to_string();
                    let params: Vec<HirType> = f.params.iter().map(|(_, ty)| ty.clone()).collect();
                    let return_type = f.ret.clone().unwrap_or(HirType::Unit);
                    functions.push(InterfaceFn {
                        name: f.name,
                        mangled_name,
                        params,
                        return_type,
                        is_pub: true,
                    });
                }
                HirItem::Struct(s) if s.is_pub => {
                    let fields: Vec<(Symbol, HirType)> = s.fields.iter()
                        .map(|f| (f.name, f.ty.clone()))
                        .collect();
                    structs.push(InterfaceStruct {
                        name: s.name,
                        fields,
                        type_params: s.type_params.clone(),
                    });
                }
                HirItem::Enum(e) if e.is_pub => {
                    let variants: Vec<(Symbol, Vec<(Symbol, HirType)>)> = e.variants.iter()
                        .map(|v| {
                            let fields: Vec<(Symbol, HirType)> = v.fields.iter()
                                .map(|f| (f.name, f.ty.clone()))
                                .collect();
                            (v.name, fields)
                        })
                        .collect();
                    enums.push(InterfaceEnum {
                        name: e.name,
                        variants,
                        type_params: e.type_params.clone(),
                    });
                }
                _ => {}
            }
        }

        Self {
            package_name: package_name.to_string(),
            version: version.to_string(),
            functions,
            structs,
            enums,
        }
    }

    /// Serialize this interface to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    /// Deserialize from bytes.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        postcard::from_bytes(data).ok()
    }

    /// Look up a function by its mangled name.
    pub fn find_function(&self, mangled_name: &str) -> Option<&InterfaceFn> {
        self.functions.iter().find(|f| f.mangled_name == mangled_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_interner::Interner;

    #[test]
    fn empty_interface_serializes() {
        let iface = DependencyInterface {
            package_name: "test".into(),
            version: "0.1.0".into(),
            functions: vec![],
            structs: vec![],
            enums: vec![],
        };
        let bytes = iface.to_bytes();
        let restored = DependencyInterface::from_bytes(&bytes).unwrap();
        assert_eq!(restored.package_name, "test");
    }
}
