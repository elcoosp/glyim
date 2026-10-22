#[derive(Debug, Clone)]
/// InterpValue.
pub enum InterpValue {
    #[allow(missing_docs)]
    Int(i128),
    #[allow(missing_docs)]
    Uint(u128),
    #[allow(missing_docs)]
    Bool(bool),
    /// Variant.
    Unit,
    #[allow(missing_docs)]
    Aggregate(Vec<InterpValue>),
    /// A reference to a local (or a sub-place of one) in the current
    /// frame or an ancestor frame.
    ///
    /// `projection` records the place's projection chain at the point
    /// `Rvalue::Ref` was evaluated, so a projection like
    /// `&mut _5.field0` can still resolve to `_5.field0` (not the whole
    /// `_5`) when the ref is later dereferenced. Before this field
    /// existed, projections were discarded at ref-creation, so
    /// `w.v.len()` (where `v` is a struct field of `w`) derefed to the
    /// whole `W` and read the wrong length. See KNOWN_GAPS §8.
    #[allow(missing_docs)]
    Ref {
        frame: usize,
        local: usize,
        projection: Box<[glyim_mir::ProjectionElem]>,
    },
    #[allow(missing_docs)]
    Float(f64),
    #[allow(missing_docs)]
    String(String),
    #[allow(missing_docs)]
    Fn(glyim_core::DefId),
    #[allow(missing_docs)]
    ConstRef(glyim_core::DefId),
    /// Opaque pointer to a `static` item (an address into the
    /// interpreter's `globals` map). Produced by evaluating
    /// `MirConstKind::StaticRef` and consumed by `Deref` projections in
    /// `read_place` / `write_place` to access the global's value.
    StaticPtr {
        /// The static's def id (the key into the interpreter's `globals`).
        def_id: glyim_core::def_id::StaticDefId,
    },
}

impl PartialEq for InterpValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (InterpValue::Int(a), InterpValue::Int(b)) => a == b,
            (InterpValue::Uint(a), InterpValue::Uint(b)) => a == b,
            (InterpValue::Bool(a), InterpValue::Bool(b)) => a == b,
            (InterpValue::Unit, InterpValue::Unit) => true,
            (InterpValue::Aggregate(a), InterpValue::Aggregate(b)) => a == b,
            (
                InterpValue::Ref { frame: a, local: b, .. },
                InterpValue::Ref { frame: c, local: d, .. },
            ) => {
                // Address identity: two Refs are "equal" iff they point at
                // the same (frame, local) base. Projection is not compared
                // (that would be structural equality, not address
                // equality); matching the pre-projection-field behaviour.
                a == c && b == d
            }
            (InterpValue::Float(a), InterpValue::Float(b)) => a.to_bits() == b.to_bits(),
            (InterpValue::String(a), InterpValue::String(b)) => a == b,
            (InterpValue::Fn(a), InterpValue::Fn(b)) => a == b,
            (InterpValue::ConstRef(a), InterpValue::ConstRef(b)) => a == b,
            _ => false,
        }
    }
}
