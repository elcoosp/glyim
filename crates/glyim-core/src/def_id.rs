use crate::arena::IdxLike;
use std::fmt;

macro_rules! define_def_id {
    ($($(#[$meta:meta])* $name:ident),* $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
            $(#[$meta])*
            pub struct $name(u32);

            impl $name {
                // No raw constructor exists, not even `#[cfg(test)]`-gated
                // (see `arena::define_idx!`); [`IdxLike::from_u32`] —
                // `IndexVec` machinery — is the only raw wrap in this
                // crate (plan M3.1 / CT-1).
                /// Sanctioned production constructor: wraps a `usize` slot
                /// produced by an arena issuer (`IndexVec::push`, an
                /// interner, or an explicit allocator). The `usize` ->
                /// `u32` narrowing is deliberate (handle widths are `u32`
                /// by construction) and is guarded by the `IndexVec::push`
                /// invariant that lengths stay well below `u32::MAX`.
                #[inline]
                #[must_use]
                pub fn from_usize(idx: usize) -> Self { Self(idx as u32) }
                /// The raw `u32` backing this ID, for interning tables that
                /// key on the plain integer.
                pub fn to_raw(self) -> u32 { self.0 }
                /// The ID as a `usize` index, for `IndexVec`-style indexing.
                pub fn index(self) -> usize { self.0 as usize }
            }

            impl IdxLike for $name {
                /// Wraps a raw `u32` index (`IndexVec` machinery; see
                /// [`IdxLike`] docs).
                fn from_u32(raw: u32) -> Self { Self(raw) }
                fn to_raw(self) -> u32 { self.0 }
            }
        )*
    };
}

define_def_id!(
    /// The interning ID of a crate in the compiler session's crate table.
    CrateId,
    /// The def-map-local ID of a definition within one crate.
    LocalDefId,
);

/// A globally unique definition reference: which crate, which local def.
///
/// `DefId`s survive across crates because the `krate` half namespaces the
/// crate-local [`LocalDefId`]; within one compilation session the crate
/// table is stable, so the pair is a valid long-lived key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DefId {
    /// The crate the definition belongs to.
    pub krate: CrateId,
    /// The definition's ID within that crate's def map.
    pub local_id: LocalDefId,
}

impl DefId {
    /// Pairs a crate with its crate-local definition ID into a global one.
    #[must_use]
    pub fn new(krate: CrateId, local_id: LocalDefId) -> Self {
        Self { krate, local_id }
    }
}

impl fmt::Display for CrateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "crate[{}]", self.0)
    }
}
impl fmt::Display for DefId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.krate, self.local_id.0)
    }
}

define_def_id!(
    /// The ID of an ADT (struct or enum) definition.
    AdtId,
    /// The ID of a function definition (free function or method).
    FnDefId,
    /// The ID of a closure definition.
    ClosureId,
    /// The ID of a trait definition.
    TraitDefId,
    /// The ID of an impl block.
    ImplDefId,
    /// The ID of an opaque type (`impl Trait`) definition.
    OpaqueTyId,
    /// The ID of a type alias definition.
    TypeAliasId,
    /// The ID of a `const` item definition.
    ConstDefId,
    /// The ID of a `static` item definition.
    StaticDefId,
    /// The index of one enum variant within its ADT.
    VariantIdx
);
