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


/// After lowering, scan the original tokens and attach doc comments
/// to items based on source position (Go‑style).
pub fn attach_doc_comments(
    hir: &mut Hir,
    tokens: &[glyim_lex::Token],
) {
    for item in &mut hir.items {
        let span = match item {
            crate::item::HirItem::Fn(f) => f.span,
            crate::item::HirItem::Struct(s) => s.span,
            crate::item::HirItem::Enum(e) => e.span,
            crate::item::HirItem::Impl(i) => i.span,
            crate::item::HirItem::Extern(e) => e.span,
        };

        // Find the token matching the HIR span.start (usually the item name)
        let name_token_index = tokens.iter()
            .position(|t| t.start == span.start && !t.kind.is_trivia());

        // Walk backwards from the name token to find the keyword token
        // (fn, struct, enum, impl, extern) that starts the declaration.
        // Doc comments precede the keyword, not just the name.
        let keyword_index = name_token_index.and_then(|idx| {
            (0..idx).rev().find(|&i| {
                let t = &tokens[i];
                !t.kind.is_trivia() && t.kind.is_keyword()
            })
        });

        let search_index = keyword_index.or(name_token_index);

        if let Some(idx) = search_index
            && let Some(doc) = glyim_parse::doc_comment::collect_doc_comments(tokens, idx) {
                match item {
                    crate::item::HirItem::Fn(f) => f.doc = Some(doc),
                    crate::item::HirItem::Struct(s) => s.doc = Some(doc),
                    crate::item::HirItem::Enum(e) => e.doc = Some(doc),
                    crate::item::HirItem::Impl(i) => i.doc = Some(doc),
                    crate::item::HirItem::Extern(e) => e.doc = Some(doc),
                }
            }
    }
}
