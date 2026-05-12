pub mod item;
mod lower;
pub mod node;
pub mod passes;
pub mod types;
pub mod mangling;

pub use item::{
    EnumDef, ExternBlock, ExternFn, FnSig, HirImplDef, HirItem, HirVariant, StructDef, StructField,
};
pub use lower::attach_doc_comments;
pub use lower::desugar::desugar_method_calls;
pub use lower::lower;
pub use lower::lower_with_declarations;
pub use node::{Hir, HirBinOp, HirExpr, HirFn, HirStmt, HirUnOp, MatchArm};
pub use types::{
    ExprId, HirPattern, HirType, SubstitutionError, TypeVar, substitute_type, substitute_type_safe,
    substitute_type_with,
};
pub mod decl_table;
pub mod dependency_names;
pub mod index;
pub mod normalize;
pub mod remap_symbols;
pub mod semantic_hash;
pub use remap_symbols::{collect_symbols_from_type, remap_symbols_in_hir, remap_type};
pub mod effects;

#[cfg(test)]
mod tests;
