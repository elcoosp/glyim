pub mod typeck;
pub use typeck::{EnumInfo, StructInfo, TypeChecker, TypeError, unify as legacy_unify};

pub mod ty;
pub mod unify;
pub mod diagnostics;
pub mod chr;
pub mod freeze;
pub mod staging;
pub mod rep;
pub mod reflect;
pub mod elab;
pub mod comptime;
pub mod queries;
pub mod db;

use std::collections::HashMap;
use glyim_hir::types::{ExprId, HirType};

/// The output expected by `glyim-compiler::pipeline` (Phase 5 / Chunk 11 integration).
pub struct TypeCheckOutput {
    pub expr_types: Vec<HirType>,
    pub call_type_args: HashMap<ExprId, Vec<HirType>>,
    pub reflect_metadata: Vec<()>,  // placeholder
    pub generated_items: Vec<()>,   // placeholder
}

#[cfg(test)]
mod tests;
