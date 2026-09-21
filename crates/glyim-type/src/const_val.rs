use crate::ty::{ConstVar, Ty};
use glyim_core::interner::Name;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Const.
pub struct Const {
    /// Struct.
    pub kind: ConstKind,
    /// Struct.
    pub ty: Ty,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// ConstKind.
pub enum ConstKind {
    #[allow(missing_docs)]
    Int(i128),
    #[allow(missing_docs)]
    Uint(u128),
    #[allow(missing_docs)]
    FloatBits(u64),
    #[allow(missing_docs)]
    Bool(bool),
    #[allow(missing_docs)]
    Char(char),
    #[allow(missing_docs)]
    String(Name),
    /// Variant.
    Unit,
    #[allow(missing_docs)]
    Infer(ConstVar),
    #[allow(missing_docs)]
    Param(ParamConst),
    /// Variant.
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// ParamConst.
pub struct ParamConst {
    /// Struct.
    pub index: u32,
    /// Struct.
    pub name: Name,
}
