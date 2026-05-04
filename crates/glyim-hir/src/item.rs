use crate::node::HirFn;
use crate::types::HirType;
use glyim_diag::Span;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub struct FnSig {
    pub params: Vec<HirType>,
    pub ret: HirType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: Symbol,
    pub ty: HirType,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub doc: Option<String>,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub fields: Vec<StructField>,
    pub span: Span,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HirItem {
    Fn(HirFn),
    Struct(StructDef),
    Enum(EnumDef),
    Impl(HirImplDef),
    Extern(ExternBlock),
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirVariant {
    pub name: Symbol,
    pub fields: Vec<StructField>,
    pub tag: u32,
    pub doc: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub doc: Option<String>,
    pub name: Symbol,
    pub type_params: Vec<Symbol>,
    pub variants: Vec<HirVariant>,
    pub span: Span,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HirImplDef {
    pub doc: Option<String>,
    pub target_name: Symbol,
    pub type_params: Vec<Symbol>,
    pub methods: Vec<HirFn>,
    pub span: Span,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternFn {
    pub name: Symbol,
    pub params: Vec<HirType>,
    pub ret: HirType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock {
    pub doc: Option<String>,
    pub functions: Vec<ExternFn>,
    pub span: Span,
}
