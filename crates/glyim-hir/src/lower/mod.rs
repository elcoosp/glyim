mod context;
pub mod desugar;
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
pub fn attach_doc_comments(hir: &mut Hir, tokens: &[glyim_lex::Token]) {
    fn attach_doc_for_span(span: glyim_diag::Span, tokens: &[glyim_lex::Token]) -> Option<String> {
        let name_token_index = tokens
            .iter()
            .position(|t| t.start == span.start && !t.kind.is_trivia());

        let keyword_index = name_token_index.and_then(|idx| {
            (0..idx).rev().find(|&i| {
                let t = &tokens[i];
                !t.kind.is_trivia() && t.kind.is_keyword()
            })
        });

        let search_index = keyword_index.or(name_token_index);

        search_index.and_then(|idx| glyim_parse::doc_comment::collect_doc_comments(tokens, idx))
    }

    for item in &mut hir.items {
        match item {
            crate::item::HirItem::Fn(f) => {
                f.doc = attach_doc_for_span(f.span, tokens);
            }
            crate::item::HirItem::Struct(s) => {
                s.doc = attach_doc_for_span(s.span, tokens);
                for _field in &mut s.fields {
                    // field span is at its name? We don't have a span for each field currently.
                    // We'll use the field's name token position. We'll get it from tokens.
                    // Actually we don't store individual field spans in StructField.
                    // We'll skip for now and just leave doc as None.
                }
            }
            crate::item::HirItem::Enum(e) => {
                e.doc = attach_doc_for_span(e.span, tokens);
                for _variant in &mut e.variants {
                    // variant span info is not stored; skip for now.
                }
            }
            crate::item::HirItem::Impl(i) => {
                i.doc = attach_doc_for_span(i.span, tokens);
                for method in &mut i.methods {
                    method.doc = attach_doc_for_span(method.span, tokens);
                }
            }
            crate::item::HirItem::Extern(e) => {
                e.doc = attach_doc_for_span(e.span, tokens);
            }
        }
    }
}
