use crate::ty::{ConstVar, Ty};
use glyim_core::interner::Name;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `Const`.
pub struct Const {
    /// `Struct`.
    pub kind: ConstKind,
    /// `Struct`.
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `ConstKind`.
pub enum ConstKind {
    /// `Int`.
    Int(i128),
    /// `Uint`.
    Uint(u128),
    /// `FloatBits`.
    FloatBits(u64),
    /// `Bool`.
    Bool(bool),
    /// `Char`.
    Char(char),
    /// `String`.
    String(Name),
    /// `Variant`.
    Unit,
    /// `Infer`.
    Infer(ConstVar),
    /// `Param`.
    Param(ParamConst),
    /// `Variant`.
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `ParamConst`.
pub struct ParamConst {
    /// `Struct`.
    pub index: u32,
    /// `Struct`.
    pub name: Name,
}
