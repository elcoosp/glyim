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
    #[allow(missing_docs)]
    Ref { frame: usize, local: usize },
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
            (InterpValue::Ref { frame: a, local: b }, InterpValue::Ref { frame: c, local: d }) => {
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
