pub mod typeck;
pub use typeck::{EnumInfo, StructInfo, TypeChecker, TypeError, unify};

pub mod ty;

#[cfg(test)]
mod tests;
