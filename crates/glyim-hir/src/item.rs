use crate::types::HirType;
use crate::node::HirFn;
use glyim_interner::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub struct FnSig { pub params: Vec<HirType>, pub ret: HirType }

#[derive(Debug, Clone, PartialEq)]
pub struct StructField { pub name: Symbol, pub ty: HirType }

#[derive(Debug, Clone, PartialEq)]
pub struct StructDef { pub name: Symbol, pub type_params: Vec<Symbol>, pub fields: Vec<StructField> }

#[derive(Debug, Clone, PartialEq)]
pub enum HirItem { Fn(HirFn), Struct(StructDef), Enum(EnumDef), Impl(HirImplDef), Extern(ExternBlock) }

#[derive(Debug, Clone, PartialEq)]
pub struct HirVariant { pub name: Symbol, pub fields: Vec<StructField>, pub tag: u32 }

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef { pub name: Symbol, pub type_params: Vec<Symbol>, pub variants: Vec<HirVariant> }

#[derive(Debug, Clone, PartialEq)]
pub struct HirImplDef { pub target_name: Symbol, pub type_params: Vec<Symbol>, pub methods: Vec<HirFn>, pub is_pub: bool }

#[derive(Debug, Clone, PartialEq)]
pub struct ExternFn { pub name: Symbol, pub params: Vec<HirType>, pub ret: HirType }

#[derive(Debug, Clone, PartialEq)]
pub struct ExternBlock { pub functions: Vec<ExternFn> }
