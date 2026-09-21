use crate::ty::{RegionVid, UniverseIndex};
use glyim_core::interner::Name;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Region.
pub enum Region {
    /// Variant.
    Static,
    #[allow(missing_docs)]
    EarlyBound(EarlyBoundRegion),
    #[allow(missing_docs)]
    LateBound(DebruijnIndex, u32, BoundRegionKind),
    #[allow(missing_docs)]
    Var(RegionVid),
    /// A placeholder region created when instantiating higher-ranked trait bounds.
    /// For example, when checking `for<'a> T: Trait<'a>`, we replace the bound
    /// region `'a` with a `PlaceholderRegion` and verify the predicate holds
    /// for this arbitrary region.
    Placeholder(PlaceholderRegion),
    /// Variant.
    Erased,
    /// Variant.
    Error,
}

/// A placeholder region represents an abstract region created when checking
/// higher-ranked trait bounds (HRTB). It stands for "some region" in a given
/// universe — the solver must prove the predicate for *all* such regions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlaceholderRegion {
    /// The universe this placeholder was created in.
    pub universe: UniverseIndex,
    /// The bound region this placeholder replaces (for diagnostics).
    pub bound: BoundRegionKind,
    /// A unique index for this placeholder within its universe.
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// EarlyBoundRegion.
pub struct EarlyBoundRegion {
    /// Struct.
    pub index: u32,
    /// Struct.
    pub name: Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// DebruijnIndex.
pub struct DebruijnIndex(pub u32);

impl DebruijnIndex {
    /// INNERMOST.
    pub const INNERMOST: DebruijnIndex = DebruijnIndex(0);
    /// shifted_in.
    pub fn shifted_in(self) -> Self {
        DebruijnIndex(self.0 + 1)
    }
    /// shifted_out.
    pub fn shifted_out(self) -> Option<Self> {
        if self.0 > 0 {
            Some(DebruijnIndex(self.0 - 1))
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// BoundRegionKind.
pub enum BoundRegionKind {
    #[allow(missing_docs)]
    BrAnon(u32),
    #[allow(missing_docs)]
    BrNamed(Name),
    /// Variant.
    BrEnv,
}
