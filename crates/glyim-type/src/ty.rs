use crate::binder::Binder;
use crate::const_val::Const;
use crate::fn_sig::FnSig;
use crate::predicate::Predicate;
use crate::region::Region;
use crate::substitution::Substitution;
use glyim_core::def_id::*;
use glyim_core::interner::Name;
use glyim_core::primitives::*;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// `Ty`.
pub struct Ty {
    raw: u32,
}

impl Ty {
    /// Sanctioned production constructor: wraps a `usize` slot produced by
    /// the type arena's issuers (the interner and the builtin sentinels).
    /// Formerly `Ty::from_raw(u32)`; the CT-1 private-constructor ratchet
    /// (plan M3.1) replaces raw `u32` construction with the
    /// `from_usize(usize)` factory, mirroring `glyim-span`'s `FileId` and
    /// `glyim-core`'s macro-generated ID families. The `usize` -> `u32`
    /// narrowing is deliberate (handle widths are `u32` by construction)
    /// and is guarded by the arena invariant that type counts stay well
    /// below `u32::MAX`. `const` so the builtin sentinels below stay
    /// compile-time constants.
    #[inline]
    #[must_use]
    pub const fn from_usize(idx: usize) -> Self {
        Self { raw: idx as u32 }
    }
    #[inline]
    /// `to_raw`.
    #[must_use]
    pub fn to_raw(self) -> u32 {
        self.raw
    }
    #[inline]
    /// index.
    #[must_use]
    pub fn index(self) -> usize {
        self.raw as usize
    }

    /// `ERROR`.
    pub const ERROR: Ty = Ty::from_usize(0);
    /// `NEVER`.
    pub const NEVER: Ty = Ty::from_usize(1);
    /// `UNIT`.
    pub const UNIT: Ty = Ty::from_usize(2);
    /// `BOOL`.
    pub const BOOL: Ty = Ty::from_usize(3);
    /// `U8`.
    pub const U8: Ty = Ty::from_usize(4);
    /// `U16`.
    pub const U16: Ty = Ty::from_usize(5);
    /// `U32`.
    pub const U32: Ty = Ty::from_usize(6);
    /// `U64`.
    pub const U64: Ty = Ty::from_usize(7);
    /// `USIZE`.
    pub const USIZE: Ty = Ty::from_usize(8);
    /// `I8`.
    pub const I8: Ty = Ty::from_usize(9);
    /// `I16`.
    pub const I16: Ty = Ty::from_usize(10);
    /// `I32`.
    pub const I32: Ty = Ty::from_usize(11);
    /// `I64`.
    pub const I64: Ty = Ty::from_usize(12);
    /// `ISIZE`.
    pub const ISIZE: Ty = Ty::from_usize(13);
    /// `F32`.
    ///
    /// Script 452: the primitive float types now have concrete `Ty`
    /// constants. Previously only ints did, and `Infer(Float)` fallback
    /// could not return a "default f64" the way `Infer(Int)` returns
    /// `Ty::I32`. Adding F32/F64 at indices 14/15 keeps them adjacent to
    /// the other primitives (they are seeded by `TyCtxMut::new` right
    /// after `ISIZE`).
    pub const F32: Ty = Ty::from_usize(14);
    /// `F64`.
    pub const F64: Ty = Ty::from_usize(15);
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
/// `UniverseIndex`.
pub struct UniverseIndex(pub u32);

/// Represents a projection type like `<T as Iterator>::Item`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProjectionTy {
    /// `Struct`.
    pub trait_ref: crate::predicate::TraitRef,
    /// `Struct`.
    pub item_name: Name,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `TyKind`.
pub enum TyKind {
    /// `Variant`.
    Never,
    /// `Variant`.
    Unit,
    /// `Variant`.
    Bool,
    /// `Int`.
    Int(IntTy),
    /// `Uint`.
    Uint(UintTy),
    /// `Float`.
    Float(FloatTy),
    /// `Variant`.
    Char,
    /// `Variant`.
    String,
    /// `Infer`.
    Infer(InferVar),
    /// `Adt`.
    Adt(AdtId, Substitution),
    /// `FnDef`.
    FnDef(FnDefId, Substitution),
    /// `Closure`.
    Closure(ClosureId, Substitution),
    /// `FnPtr`.
    FnPtr(FnSig),
    /// `Ref`.
    Ref(Region, Ty, Mutability),
    /// `RawPtr`.
    RawPtr(Ty, Mutability),
    /// `Slice`.
    Slice(Ty),
    /// `Array`.
    Array(Ty, Const),
    /// `Tuple`.
    Tuple(Substitution),
    /// `Dynamic`.
    Dynamic(Binder<Box<[Predicate]>>, Region),
    /// `Opaque`.
    Opaque(OpaqueTyId, Substitution),
    /// `Projection`.
    Projection(ProjectionTy),
    /// `Param`.
    Param(ParamTy),
    /// `Bound`.
    Bound(u32, BoundTy),
    /// `Variant`.
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `InferVar`.
pub enum InferVar {
    /// `Ty`.
    Ty(TyVar),
    /// `Int`.
    Int(IntVar),
    /// `Float`.
    Float(FloatVar),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `ParamTy`.
pub struct ParamTy {
    /// `Struct`.
    pub index: u32,
    /// `Struct`.
    pub name: Name,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `BoundTy`.
pub struct BoundTy {
    /// `Struct`.
    pub var: u32,
    /// `Struct`.
    pub kind: BoundTyKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `BoundTyKind`.
pub enum BoundTyKind {
    /// `Variant`.
    Anon,
    /// `Param`.
    Param(Name),
}
