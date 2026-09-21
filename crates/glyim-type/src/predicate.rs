use crate::region::Region;
use crate::substitution::Substitution;
use crate::ty::Ty;
use glyim_core::def_id::TraitDefId;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Predicate.
pub enum Predicate {
    #[allow(missing_docs)]
    Trait(TraitPredicate),
    #[allow(missing_docs)]
    RegionOutlives(RegionOutlivesPredicate),
    #[allow(missing_docs)]
    TypeOutlives(TypeOutlivesPredicate),
    #[allow(missing_docs)]
    WellFormed(Ty),
    #[allow(missing_docs)]
    Coerce(Ty, Ty),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// TraitPredicate.
pub struct TraitPredicate {
    /// Struct.
    pub trait_ref: TraitRef,
    /// Struct.
    pub polarity: ImplPolarity,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// TraitRef.
pub struct TraitRef {
    /// Struct.
    pub def_id: TraitDefId,
    /// Struct.
    pub substs: Substitution,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// ImplPolarity.
pub enum ImplPolarity {
    /// Variant.
    Positive,
    /// Variant.
    Negative,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// RegionOutlivesPredicate.
pub struct RegionOutlivesPredicate {
    /// Struct.
    pub a: Region,
    /// Struct.
    pub b: Region,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// TypeOutlivesPredicate.
pub struct TypeOutlivesPredicate {
    /// Struct.
    pub ty: Ty,
    /// Struct.
    pub region: Region,
}
