//! Goto-declaration request support (feature tag LSP-DECLARATION).
//!
//! The server answered goto-definition for the *name* under the cursor
//! (the item, wherever it lives) and goto-type-definition for its
//! *shape*, but nothing answered "where is this name declared *for this
//! file*": `textDocument/declaration` had no engine, no advertisement,
//! and no router entry. The distinction matters on every import: jumping
//! from a use of `Point` to a far-away `struct Point` is a definition
//! answer, but the user auditing the file wants the `use` statement that
//! brings the name into scope. This module closes that capability gap.
//!
//! The seam: *which recorded declaration counts* is split from *how the
//! declarations are found* by [`DeclarationSource`], mirroring
//! LSP-TYPE-DEF's `TypeSymbolSource`. The policy ([`declaration_target`])
//! reads through the trait and stays testable without a parse tree, lock,
//! or index (`tests/goto_declaration.rs`); the production
//! [`ParseTreeDeclarationSource`] walks the parse trees the analysis
//! driver already stores in `db.syntaxes` — the same store selection-range
//! reads — plus the shared [`SymbolIndex`] the other goto requests read.
//! The policy order is the spec's semantics: an import declaration that
//! binds the name declares it (what the user wrote to bring the name into
//! scope), a locally-declared item declares itself (the spec's "declared
//! in the same file" case), and a name that is neither imported nor
//! declared in the file has no local declaration — it answers `null`
//! rather than masquerading as a second definition request.
//!
//! Wire mapping (LSP spec): the chosen declaration is a scalar
//! `Location` at the declaration extent, byte-exact like goto-definition
//! (LSP-EXACT-EXTENTS). Unopened documents, cursors with no identifier,
//! and names with no local declaration answer `null`.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::symbol_index::{DefinitionLocation, SymbolIndex, SymbolKind};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};
use url::Url;

/// The declaration-lookup seam for goto-declaration requests
/// (LSP-DECLARATION).
///
/// The choice policy in [`declaration_target`] must stay testable
/// without a live `AnalysisDatabase` (parse trees, locks, indexes), so
/// it reads recorded declarations through this trait. The production
/// [`ParseTreeDeclarationSource`] walks the driver's stored parse tree
/// and borrows the shared [`SymbolIndex`]; a future def-map-backed
/// import table would slot in behind the same seam without touching the
/// policy.
pub trait DeclarationSource {
    /// Spans of the import declarations in `file_id` that bind `name`
    /// (an `as` alias, or the path's last segment), in source order.
    /// Globs (`use foo::*`) and `_` aliases bind no single name and
    /// never appear here.
    fn import_declarations_of(&self, file_id: FileId, name: &str) -> Vec<Span>;

    /// Every definition site recorded for `name`, with its kind — the
    /// same recorded symbols goto-definition reads. Order is the
    /// index's recording order (the driver's analysis order); the
    /// policy applies its own preference rules on top.
    fn definitions_of(&self, name: &str) -> Vec<(SymbolKind, DefinitionLocation)>;
}

/// Production [`DeclarationSource`]: walks the driver's stored parse
/// tree for import declarations and borrows the shared [`SymbolIndex`]
/// for definitions.
///
/// The source is bound to one file (the parse tree is per-file), so a
/// query for any other file's import declarations answers empty — the
/// policy relies on that to keep per-file declarations per-file. The
/// store keeps the immutable [`GreenNode`] (rowan cursor nodes carry
/// raw parent pointers and are not `Send`, so they cannot live in the
/// shared database); each request materializes its own cursor root with
/// one `Arc` clone, the same trick selection-range uses.
pub struct ParseTreeDeclarationSource<'a> {
    green: &'a GreenNode,
    file_id: FileId,
    index: &'a SymbolIndex,
}

impl<'a> ParseTreeDeclarationSource<'a> {
    /// Bind the source to one file's parse tree and symbol index.
    #[must_use]
    pub fn new(green: &'a GreenNode, file_id: FileId, index: &'a SymbolIndex) -> Self {
        Self {
            green,
            file_id,
            index,
        }
    }
}

impl DeclarationSource for ParseTreeDeclarationSource<'_> {
    fn import_declarations_of(&self, file_id: FileId, name: &str) -> Vec<Span> {
        if file_id != self.file_id {
            return Vec::new();
        }
        import_bindings(self.green, self.file_id)
            .into_iter()
            .filter(|(bound, _)| bound == name)
            .map(|(_, span)| span)
            .collect()
    }

    fn definitions_of(&self, name: &str) -> Vec<(SymbolKind, DefinitionLocation)> {
        self.index
            .lookup_by_name(name)
            .iter()
            .map(|sym| (sym.kind, sym.definition.clone()))
            .collect()
    }
}

/// Extract every `(binding name, declaration span)` the parse tree's
/// `use` declarations record.
///
/// A `use` declaration binds a name through an `as` alias or through the
/// last path segment (`use crate::shapes::Point` binds `Point`); nested
/// import trees (`use foo::{a, b::c as d}`) contribute one binding per
/// leaf tree, all pointing at the enclosing statement's extent — that
/// whole statement is what an editor scrolls to and highlights. Globs
/// (`use foo::*`) and `_` aliases import names anonymously and bind no
/// single name to jump to, so they contribute nothing. Spans are
/// byte-exact over the statement (`Span` with [`SyntaxContext::ROOT`],
/// the same construction selection-range uses for parse-tree extents).
#[must_use]
pub fn import_bindings(green: &GreenNode, file_id: FileId) -> Vec<(String, Span)> {
    let root = SyntaxNode::new_root(green.clone());
    let mut out = Vec::new();
    for decl in root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::UseDecl)
    {
        let decl_span = span_of(file_id, decl.text_range());
        for tree in decl
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::UseTree)
        {
            // Leaf trees bind; a tree with a nested `{...}` delegates its
            // names to the leaves inside it.
            if tree
                .children()
                .any(|child| child.kind() == SyntaxKind::UseTree)
            {
                continue;
            }
            if let Some(bound) = binding_name_of_leaf(&tree) {
                out.push((bound, decl_span));
            }
        }
    }
    out
}

/// The name one leaf `use` tree binds, if any.
///
/// `as Alias` renames the binding (the alias wins over the path's last
/// segment); `as _` is an anonymous import and binds nothing. Without an
/// alias, the last identifier of the path is the binding; a path ending
/// in `self`/`super`/`crate` keywords names no importable item binding.
fn binding_name_of_leaf(tree: &SyntaxNode) -> Option<String> {
    // The path's identifiers live inside the `UsePath` child node, the
    // alias tokens directly in the tree — walk every token below the
    // leaf (a leaf has no nested `{...}` trees by construction).
    let mut saw_as = false;
    let mut alias: Option<String> = None;
    let mut last_ident: Option<String> = None;
    for token in tree
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        match token.kind() {
            SyntaxKind::KwAs => saw_as = true,
            SyntaxKind::Ident if saw_as => {
                alias = Some(token.text().to_string());
                break;
            }
            SyntaxKind::Ident => last_ident = Some(token.text().to_string()),
            _ => {}
        }
    }
    if saw_as {
        // `as` with no following identifier is a parse error the parser
        // already reported; an `as _` alias is anonymous. Either way the
        // alias slot stays `None` and nothing binds.
        return alias;
    }
    last_ident
}

fn span_of(file_id: FileId, range: rowan::TextRange) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_usize(u32::from(range.start()) as usize),
        ByteIdx::from_usize(u32::from(range.end()) as usize),
        SyntaxContext::ROOT,
    )
}

/// Goto-declaration choice: the declaration site of `name` *for*
/// `file_id`.
///
/// An import declaration that binds the name wins — it is what brought
/// the name into this file's scope, and the same tie-break order the
/// other engines apply (first recorded) resolves same-named imports. A
/// locally-declared item (a definition recorded in `file_id` itself) is
/// the declaration when no import exists. Definitions recorded only in
/// other files are *not* declarations of this file's name: answering
/// one would turn `declaration` into a second definition request.
/// Zero-width definition extents are skipped: the index records body
/// locals without usable extents today, and a zero-width extent cannot
/// name a jump target — answering it would aim the client at the top of
/// the file. `None` when nothing local declares the name.
#[must_use]
pub fn declaration_target(
    source: &dyn DeclarationSource,
    file_id: FileId,
    name: &str,
) -> Option<DefinitionLocation> {
    if let Some(span) = source
        .import_declarations_of(file_id, name)
        .into_iter()
        .next()
    {
        return Some(DefinitionLocation { file_id, span });
    }
    source
        .definitions_of(name)
        .into_iter()
        .find(|(_, definition)| {
            definition.file_id == file_id && definition.span.lo != definition.span.hi
        })
        .map(|(_, definition)| definition)
}

/// `textDocument/declaration` request entry point (LSP-DECLARATION).
///
/// Resolution order mirrors `goto_definition`: URI → open-file lookup →
/// cursor offset (byte-safe, INF-12) → identifier extraction → choice
/// through the shared seam → wire location. Returns `None` when the
/// document is not open, the cursor carries no identifier, or the name
/// has no local declaration; callers answer `null` for that.
#[must_use]
pub fn goto_declaration(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &GotoDefinitionParams,
) -> Option<GotoDefinitionResponse> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let pos = params.text_document_position_params.position;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let source = sm.source();

    // Byte-offset, char-boundary-safe identifier extraction (INF-12).
    let symbol_name = crate::navigation::identifier_at_offset(source, offset)?;
    let symbol_name = symbol_name.as_str();

    let syntaxes = db.syntaxes.read();
    let green = syntaxes.get(&file_id)?;
    let symbol_index = db.symbol_index.read();
    let target = declaration_target(
        &ParseTreeDeclarationSource::new(green, file_id, &symbol_index),
        file_id,
        symbol_name,
    )?;
    drop(symbol_index);
    drop(syntaxes);

    let def_sm = source_maps.get(&target.file_id)?;
    // Exact token extents (LSP-EXACT-EXTENTS): the same two-tuple
    // mapping the other goto requests answer with. An unmappable span
    // answers `null` (via `?`) rather than a silent line-0/col-0
    // fallback, which would aim every client at the top of the file.
    let ((start_line, start_col), (end_line, end_col)) =
        def_sm.span_to_position(target.span.lo.to_usize(), target.span.hi.to_usize())?;
    let target_path = file_map.path(target.file_id)?;
    let target_uri = Url::from_file_path(target_path).ok()?;
    tracing::debug!(
        name = %symbol_name,
        "goto declaration resolved"
    );
    use std::str::FromStr;
    Some(GotoDefinitionResponse::Scalar(lsp_types::Location {
        uri: lsp_types::Uri::from_str(target_uri.as_str()).ok()?,
        range: lsp_types::Range {
            start: lsp_types::Position {
                line: start_line as u32,
                character: start_col as u32,
            },
            end: lsp_types::Position {
                line: end_line as u32,
                character: end_col as u32,
            },
        },
    }))
}
