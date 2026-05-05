pub mod ast;
pub mod ast_to_cst;
pub mod cst_builder;
pub mod error;
mod expr;
mod item;
mod parser;
pub mod recovery;

pub use ast::{
    Ast, BinOp, BlockItem, EnumVariantRepr as EnumVariant, ExprKind, ExprNode, ExternFn, Item,
    MatchArm, Pattern, StmtKind, StmtNode, TypeExpr, UnOp, UseItem, VariantKind,
};
pub use error::ParseError;
pub use parser::{ParseOutput, parse};
pub mod declarations;
pub mod doc_comment;

#[cfg(test)]
mod tests;
