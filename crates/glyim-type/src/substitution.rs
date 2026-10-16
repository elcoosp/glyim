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

/// Mangle a `(FnDefId, Substitution)` pair into a stable, LLVM-safe symbol.
///
/// The `(index, len)` of the interned substitution uniquely identifies the
/// instantiation's concrete type arguments, so two call sites that resolve
/// to the same concrete arguments produce the same symbol, and two
/// different instantiations (e.g. `Option<i32>::unwrap` vs
/// `Option<Option<i32>>::unwrap`) produce different ones. An empty
/// substitution keeps the un-suffixed legacy name so non-generic functions
/// (`main`, etc.) don't churn.
pub fn mangle_fn_symbol(def_id: u32, substs: Substitution) -> String {
    if substs.is_empty() {
        format!("__glyim_fn_{}", def_id)
    } else {
        format!("__glyim_fn_{}_{}_{}", def_id, substs.index(), substs.len())
    }
}
