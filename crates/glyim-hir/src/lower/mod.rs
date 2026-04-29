//! AST to HIR lowering
//!
//! This module transforms the parsed AST into a higher-level intermediate representation (HIR)
//! that is easier to analyze and transform in subsequent compiler phases.

mod context;
mod expr;
mod item;
mod ops;
mod pattern;
mod types;

pub use context::LoweringContext;

use crate::Hir;
use glyim_interner::Interner;

/// Lower an AST to HIR.
///
/// This is the main entry point for the lowering phase. It iterates over all
/// top-level items in the AST and converts them to their HIR equivalents.
pub fn lower(ast: &glyim_parse::Ast, interner: &mut Interner) -> Hir {
    let mut ctx = LoweringContext::new(interner);
    let items: Vec<_> = ast.items
        .iter()
        .filter_map(|item| item::lower_item(item, &mut ctx))
        .collect();
    Hir { items }
}
