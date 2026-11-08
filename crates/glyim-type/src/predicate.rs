use crate::region::Region;
use crate::substitution::Substitution;
use crate::ty::Ty;
use glyim_core::def_id::TraitDefId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `Predicate`.
pub enum Predicate {
    /// Trait item.
    Trait(TraitPredicate),
    /// `RegionOutlives`.
    RegionOutlives(RegionOutlivesPredicate),
    /// `TypeOutlives`.
    TypeOutlives(TypeOutlivesPredicate),
    /// `WellFormed`.
    WellFormed(Ty),
    /// `Coerce`.
    Coerce(Ty, Ty),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `TraitPredicate`.
pub struct TraitPredicate {
    /// `Struct`.
    pub trait_ref: TraitRef,
    /// `Struct`.
    pub polarity: ImplPolarity,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `TraitRef`.
pub struct TraitRef {
    /// `Struct`.
    pub def_id: TraitDefId,
    /// `Struct`.
    pub substs: Substitution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// `ImplPolarity`.
pub enum ImplPolarity {
    /// `Variant`.
    Positive,
    /// `Variant`.
    Negative,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `RegionOutlivesPredicate`.
pub struct RegionOutlivesPredicate {
    /// `Struct`.
    pub a: Region,
    /// `Struct`.
    pub b: Region,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// `TypeOutlivesPredicate`.
pub struct TypeOutlivesPredicate {
    /// `Struct`.
    pub ty: Ty,
    /// `Struct`.
    pub region: Region,
}
