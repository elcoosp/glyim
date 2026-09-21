//! Foundation types: arena, primitives, def-id, abi, interner, path.

/// abi.
pub mod abi;
pub mod arena;
/// def_id.
pub mod def_id;
/// interner.
pub mod interner;
/// path.
pub mod path;
/// primitives.
pub mod primitives;

pub use abi::*;
pub use arena::*;
pub use def_id::*;
pub use interner::*;
pub use path::*;
pub use primitives::*;

#[cfg(test)]
mod tests;
