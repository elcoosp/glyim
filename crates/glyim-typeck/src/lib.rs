pub mod typeck;
pub use typeck::{EnumInfo, StructInfo, TypeChecker, TypeError};

pub mod ty;
pub mod unify;
pub mod diagnostics;

#[cfg(test)]
mod tests;
