pub mod item;
mod lower;
pub mod monomorphize;
pub mod node;
pub mod passes;
pub mod types;

pub use item::{
    EnumDef, ExternBlock, ExternFn, FnSig, HirImplDef, HirItem, HirVariant, StructDef, StructField,
};
pub use lower::attach_doc_comments;
pub use lower::desugar::desugar_method_calls;
pub use lower::lower;
pub use lower::lower_with_declarations;
pub use node::{Hir, HirBinOp, HirExpr, HirFn, HirStmt, HirUnOp, MatchArm};
pub use types::{ExprId, HirPattern, HirType};
pub mod decl_table;
pub mod dependency_names;
pub mod normalize;
pub mod semantic_hash;
pub mod remap_symbols;
pub use remap_symbols::remap_symbols_in_hir;
pub mod effects;

#[cfg(test)]
mod tests;
