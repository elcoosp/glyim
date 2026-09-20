use glyim_span::Span;

use crate::{Path, TypeRef};

/// A single where clause bound, e.g. `T: Clone + Copy`
#[derive(Clone, Debug)]
pub struct WhereClause {
/// Struct.
    pub ty: TypeRef,
/// Struct.
    pub bounds: Vec<TraitBound>,
/// Struct.
    pub span: Span,
}

/// A trait bound, e.g. `Clone`
#[derive(Clone, Debug)]
pub struct TraitBound {
/// Struct.
    pub trait_path: Path,
/// Struct.
    pub span: Span,
    /// For a parenthesized `Fn`-family bound (`F: FnOnce() -> R`) the
    /// lowered `TypeRef::Fn { params, ret }` shape, which `trait_path`
    /// cannot represent (the parser emits the params/arrow as siblings of
    /// the trait ident, and `lower_path_from_type` drops them).
    /// `None` for a plain trait bound.
    pub fn_shape: Option<TypeRef>,
}
