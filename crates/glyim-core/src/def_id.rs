use crate::arena::IdxLike;
use std::fmt;

macro_rules! define_def_id {
    ($($(#[$meta:meta])* $name:ident),* $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
            $(#[$meta])*
            pub struct $name(u32);

            impl $name {
                /// Wraps a raw `u32` index previously produced by
                /// [`Self::to_raw`].
                ///
                /// Panics-free by construction: the caller must have obtained
                /// the raw value from an ID of the same kind (the plan M3.1
                /// private-constructor ratchet tracks tightening this to
                /// arena-issued construction only).
                pub fn from_raw(raw: u32) -> Self { Self(raw) }
                /// The raw `u32` backing this ID, for interning tables that
                /// key on the plain integer.
                pub fn to_raw(self) -> u32 { self.0 }
                /// The ID as a `usize` index, for `IndexVec`-style indexing.
                pub fn index(self) -> usize { self.0 as usize }
            }

            impl IdxLike for $name {
                fn from_raw(raw: u32) -> Self { Self(raw) }
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
