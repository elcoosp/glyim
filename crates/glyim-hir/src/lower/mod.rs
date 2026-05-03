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
pub fn lower(ast: &glyim_parse::Ast, interner: &mut Interner) -> Hir {
    let mut ctx = LoweringContext::new(interner);
    let items: Vec<_> = ast
        .items
        .iter()
        .filter_map(|item| item::lower_item(item, &mut ctx))
        .collect();
    Hir { items }
}

#[cfg(test)]
mod tests;


/// Re‑export from glyim‑parse so downstream crates can build DeclTable.
pub type DeclaredItems = glyim_parse::declarations::DeclaredItems;

/// Lower an AST to HIR, using a pre-built declaration table to resolve
/// forward references in type names and method calls.
pub fn lower_with_declarations(
    ast: &glyim_parse::Ast,
    interner: &mut Interner,
    decl_table: &crate::decl_table::DeclTable,
) -> Hir {
    let mut ctx = LoweringContext::with_decl_table(interner, decl_table);
    let items: Vec<_> = ast
        .items
        .iter()
        .filter_map(|item| item::lower_item(item, &mut ctx))
        .collect();
    Hir { items }
}
