use crate::const_val::Const;
use crate::region::Region;
use crate::ty::Ty;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Substitution.
pub struct Substitution {
    index: u32,
    len: u16,
}

impl Substitution {
    pub(crate) fn from_raw(index: u32, len: u16) -> Self {
        Self { index, len }
    }

    /// Create an empty substitution with no generic arguments.
    pub fn empty() -> Self {
        Self { index: 0, len: 0 }
    }
    /// index.
    pub fn index(self) -> u32 {
        self.index
    }
    /// len.
    pub fn len(self) -> u16 {
        self.len
    }
    /// is_empty.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
}

impl fmt::Debug for Substitution {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Substitution(index={}, len={})", self.index, self.len)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// GenericArg.
pub enum GenericArg {
    #[allow(missing_docs)]
    Ty(Ty),
    #[allow(missing_docs)]
    Lifetime(Region),
    #[allow(missing_docs)]
    Const(Const),
}
