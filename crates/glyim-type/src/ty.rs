use crate::binder::*;
use crate::const_val::*;
use crate::fn_sig::*;
use crate::predicate::*;
use crate::region::*;
use crate::substitution::*;
use glyim_core::def_id::*;
use glyim_core::interner::Name;
use glyim_core::primitives::*;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Ty.
pub struct Ty {
    raw: u32,
}

impl Ty {
    #[inline]
    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self { raw }
    }
    #[inline]
    /// to_raw.
    pub fn to_raw(self) -> u32 {
        self.raw
    }
    #[inline]
    /// index.
    pub fn index(self) -> usize {
        self.raw as usize
    }

    /// ERROR.
    pub const ERROR: Ty = Ty::from_raw(0);
    /// NEVER.
    pub const NEVER: Ty = Ty::from_raw(1);
    /// UNIT.
    pub const UNIT: Ty = Ty::from_raw(2);
    /// BOOL.
    pub const BOOL: Ty = Ty::from_raw(3);
    /// U8.
    pub const U8: Ty = Ty::from_raw(4);
    /// U16.
    pub const U16: Ty = Ty::from_raw(5);
    /// U32.
    pub const U32: Ty = Ty::from_raw(6);
    /// U64.
    pub const U64: Ty = Ty::from_raw(7);
    /// USIZE.
    pub const USIZE: Ty = Ty::from_raw(8);
    /// I8.
    pub const I8: Ty = Ty::from_raw(9);
    /// I16.
    pub const I16: Ty = Ty::from_raw(10);
    /// I32.
    pub const I32: Ty = Ty::from_raw(11);
    /// I64.
    pub const I64: Ty = Ty::from_raw(12);
    /// ISIZE.
    pub const ISIZE: Ty = Ty::from_raw(13);
}

impl fmt::Debug for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Ty({})", self.raw)
    }
}

glyim_core::define_idx!(TyVar);
glyim_core::define_idx!(IntVar);
glyim_core::define_idx!(FloatVar);
glyim_core::define_idx!(RegionVid);
glyim_core::define_idx!(ConstVar);
glyim_core::define_idx!(FieldIdx);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// UniverseIndex.
pub struct UniverseIndex(pub u32);

/// Represents a projection type like `<T as Iterator>::Item`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectionTy {
    /// Struct.
    pub trait_ref: crate::predicate::TraitRef,
    /// Struct.
    pub item_name: Name,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// TyKind.
pub enum TyKind {
    /// Variant.
    Never,
    /// Variant.
    Unit,
    /// Variant.
    Bool,
    #[allow(missing_docs)]
    Int(IntTy),
    #[allow(missing_docs)]
    Uint(UintTy),
    #[allow(missing_docs)]
    Float(FloatTy),
    /// Variant.
    Char,
    /// Variant.
    String,
    #[allow(missing_docs)]
    Infer(InferVar),
    #[allow(missing_docs)]
    Adt(AdtId, Substitution),
    #[allow(missing_docs)]
    FnDef(FnDefId, Substitution),
    #[allow(missing_docs)]
    Closure(ClosureId, Substitution),
    #[allow(missing_docs)]
    FnPtr(FnSig),
    #[allow(missing_docs)]
    Ref(Region, Ty, Mutability),
    #[allow(missing_docs)]
    RawPtr(Ty, Mutability),
    #[allow(missing_docs)]
    Slice(Ty),
    #[allow(missing_docs)]
    Array(Ty, Const),
    #[allow(missing_docs)]
    Tuple(Substitution),
    #[allow(missing_docs)]
    Dynamic(Binder<Box<[Predicate]>>, Region),
    #[allow(missing_docs)]
    Opaque(OpaqueTyId, Substitution),
    #[allow(missing_docs)]
    Projection(ProjectionTy),
    #[allow(missing_docs)]
    Param(ParamTy),
    #[allow(missing_docs)]
    Bound(u32, BoundTy),
    /// Variant.
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// InferVar.
pub enum InferVar {
    #[allow(missing_docs)]
    Ty(TyVar),
    #[allow(missing_docs)]
    Int(IntVar),
    #[allow(missing_docs)]
    Float(FloatVar),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// ParamTy.
pub struct ParamTy {
    /// Struct.
    pub index: u32,
    /// Struct.
    pub name: Name,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// BoundTy.
pub struct BoundTy {
    /// Struct.
    pub var: u32,
    /// Struct.
    pub kind: BoundTyKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// BoundTyKind.
pub enum BoundTyKind {
    /// Variant.
    Anon,
    #[allow(missing_docs)]
    Param(Name),
}
