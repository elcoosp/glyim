use lasso::Spur;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Name.
pub struct Name {
    symbol: Spur,
}

impl Name {
    /// as_symbol.
    pub fn as_symbol(self) -> Spur {
        self.symbol
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Name({})", self.symbol.into_inner())
    }
}

/// Interner.
pub struct Interner {
    inner: Arc<lasso::ThreadedRodeo>,
}

impl Interner {
    /// new.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(lasso::ThreadedRodeo::new()),
        }
    }

    #[inline]
    /// intern.
    pub fn intern(&self, s: &str) -> Name {
        Name {
            symbol: self.inner.get_or_intern(s),
        }
    }

    #[inline]
    /// resolve.
    pub fn resolve(&self, name: Name) -> &str {
        self.inner.resolve(&name.symbol)
    }

    /// lookup.
    pub fn lookup(&self, s: &str) -> Option<Name> {
        self.inner.get(s).map(|symbol| Name { symbol })
    }
}

impl Clone for Interner {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl fmt::Debug for Interner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Interner").finish_non_exhaustive()
    }
}

impl Default for Interner {
    fn default() -> Self {
        Self::new()
    }
}
