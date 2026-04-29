use glyim_interner::Symbol;
use crate::types::HirType;

/// A function signature: parameters + return type.
#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub params: Vec<HirType>,
    pub ret: HirType,
}

/// Information about a struct field.
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: Symbol,
    pub ty: HirType,
}

/// A struct definition.
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub name: Symbol,
    pub fields: Vec<StructField>,
}

/// Top-level HIR item.
#[derive(Debug, Clone, PartialEq)]
pub enum HirItem {
    Fn(crate::node::HirFn),
    Struct(StructDef),
    Enum(EnumDef),
}

/// An enum variant definition in HIR.
#[derive(Debug, Clone, PartialEq)]
pub struct HirVariant {
    pub name: Symbol,
    pub fields: Vec<StructField>, // fields for this variant
    pub tag: u32,                 // discriminant value
}

/// An enum definition in HIR.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: Symbol,
    pub variants: Vec<HirVariant>,
}
