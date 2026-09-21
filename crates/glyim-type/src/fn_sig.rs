use crate::substitution::Substitution;
use crate::ty::Ty;
use glyim_core::primitives::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// FnSig.
pub struct FnSig {
    /// Struct.
    pub inputs: Substitution,
    /// Struct.
    pub output: Ty,
    /// Struct.
    pub c_variadic: bool,
    /// Struct.
    pub unsafety: Safety,
    /// Struct.
    pub abi: Abi,
}
