use crate::arena::IdxLike;
use std::fmt;

macro_rules! define_def_id {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Struct.
            pub struct $name(u32);

            impl $name {
/// from_raw.
                pub fn from_raw(raw: u32) -> Self { Self(raw) }
/// to_raw.
                pub fn to_raw(self) -> u32 { self.0 }
/// index.
                pub fn index(self) -> usize { self.0 as usize }
            }

            impl IdxLike for $name {
                fn from_raw(raw: u32) -> Self { Self(raw) }
                fn to_raw(self) -> u32 { self.0 }
            }
        )*
    };
}

define_def_id!(CrateId, LocalDefId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// DefId.
pub struct DefId {
    /// Struct.
    pub krate: CrateId,
    /// Struct.
    pub local_id: LocalDefId,
}

impl DefId {
    /// new.
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
    AdtId,
    FnDefId,
    ClosureId,
    TraitDefId,
    ImplDefId,
    OpaqueTyId,
    TypeAliasId,
    ConstDefId,
    StaticDefId,
    VariantIdx
);
