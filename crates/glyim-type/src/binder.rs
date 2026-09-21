use crate::region::BoundRegionKind;
use crate::ty::BoundTyKind;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// Binder.
pub struct Binder<T> {
    /// Struct.
    pub value: T,
    /// Struct.
    pub bound_vars: Box<[BoundVariableKind]>,
}

impl<T> Binder<T> {
    /// bind.
    pub fn bind(value: T, bound_vars: Box<[BoundVariableKind]>) -> Self {
        Self { value, bound_vars }
    }
    /// skip_binder.
    pub fn skip_binder(self) -> T {
        self.value
    }
    /// as_ref.
    pub fn as_ref(&self) -> Binder<&T> {
        Binder {
            value: &self.value,
            bound_vars: self.bound_vars.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
/// BoundVariableKind.
pub enum BoundVariableKind {
    #[allow(missing_docs)]
    Ty(BoundTyKind),
    #[allow(missing_docs)]
    Region(BoundRegionKind),
    /// Variant.
    Const,
}
