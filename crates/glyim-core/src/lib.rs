//! Foundation types: arena, primitives, def-id, abi, interner, path.

/// abi.
pub mod abi;
pub mod arena;
/// `def_id`.
pub mod def_id;
/// interner.
pub mod interner;
pub mod lint;
/// path.
pub mod path;
/// primitives.
pub mod primitives;
/// Stable identity hashes (M6.1): `StableHasher`, `DefPathHash`, `Fingerprint`,
/// `HirId`.
pub mod stable_hash;

pub use abi::*;
pub use arena::*;
pub use def_id::*;
pub use interner::*;
pub use lint::*;
pub use path::*;
pub use primitives::*;
pub use stable_hash::*;

#[cfg(test)]
mod tests;
