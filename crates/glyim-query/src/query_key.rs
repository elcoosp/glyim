use crate::fingerprint::Fingerprint;

/// A key that uniquely identifies a memoizable computation.
///
/// Every query in the compiler pipeline implements this trait.
/// The `fingerprint` method must produce a deterministic hash
/// of all inputs that affect the query's output.
pub trait QueryKey: Clone + std::fmt::Debug + PartialEq + Eq + std::hash::Hash + Send + Sync + 'static {
    /// Compute a fingerprint from this key's data.
    /// Two keys that produce the same fingerprint MUST produce the same query result.
    fn fingerprint(&self) -> Fingerprint;
}
