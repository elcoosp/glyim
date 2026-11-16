//! Moniker request support (feature tag LSP-MONIKER).
//!
//! The server can jump to a symbol's definition, find its references, and
//! rename it, but nothing exported a symbol's *identity*: `textDocument/moniker`
//! did not exist, so an external indexer (LSIF, SCIP, ctags-style cross
//! references) had no wire answer for "what stable name does this symbol
//! carry". Every fact the request needs was already recorded — the shared
//! [`SymbolIndex`] holds each top-level item with its byte-exact extent and
//! its qualified module path (the §22.6 auto-import store), and the parse
//! trees hold the `use` statements spelling where an imported name comes
//! from. This module closes that capability gap with no re-analysis on the
//! request path.
//!
//! The seam: *which recorded facts count as identities* is split from *how
//! those facts are found* by [`MonikerSource`], mirroring
//! LSP-DECLARATION's `DeclarationSource` and LSP-TYPE-HIERARCHY's
//! `TypeHierarchySource`. The policy ([`monikers_at`]) reads through the
//! trait and stays testable without a parse tree, lock, or index
//! (`tests/moniker.rs`); the production [`SessionMonikerSource`] borrows
//! the driver's stored [`SymbolIndex`] — the same store goto-definition
//! reads — and the file's stored parse tree for the spelled import paths.
//! A future project-wide path store would slot in behind the same seam
//! without touching the policy.
//!
//! What the cursor's position decides, over the recorded facts: parked on
//! a `use` binding of the name, the request answers *Import* with the
//! exact path the source spells (`use crate::shapes::Point as Pt` carries
//! the identity `crate::shapes::Point`, not the alias); parked on a
//! recorded definition extent, it answers *Export* with the recorded
//! qualified path; a bare usage resolves by how the file binds the name —
//! the import when one binds it (the usage refers to the imported symbol),
//! the export identity otherwise. The two honest skips: a definition the
//! index records with a zero-width extent (body locals are recorded
//! without usable extents today) cannot be vouched as an identity, and a
//! definition with no recorded qualified path (impl methods, for instance)
//! carries no module-level identity to answer. Globs (`use foo::*`) and
//! anonymous aliases (`use foo::Bar as _`) bind no single name and never
//! answer an import.
//!
//! Wire mapping (LSP spec): every fact becomes a `Moniker` under the
//! `glyim` scheme with `unique: "document"` — each session file carries
//! its own module tree, so the qualified path is the identity the server
//! can vouch for *within its document*, and an LSIF consumer keys on
//! (scheme, identifier, unique) rather than joining bare paths across
//! documents. Upgrading the uniqueness level is a one-line change behind
//! the same seam once a project-wide path store exists. A cursor whose
//! name no fact identifies answers `null`, never a guessed identity.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::navigation::identifier_at_offset;
use crate::symbol_index::{DefinitionLocation, SymbolIndex};
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{Moniker, MonikerKind, MonikerParams, UniquenessLevel};
use url::Url;

/// The moniker scheme every identity this server answers carries: one
/// scheme, so a consumer can tell a glyim identifier from a tsc or .Net
/// one without sniffing the shape.
pub const MONIKER_SCHEME: &str = "glyim";

/// One `use`-statement binding the request can answer as an *Import*:
/// the path the source spells (not the alias) and the whole
/// `use ...;` statement's extent, which decides whether the cursor is
/// parked on the binding itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpelledImport {
    /// The path as spelled, e.g. `crate::shapes::Point` — the alias a
    /// binding renames it to is not part of the identity.
    pub path: String,
    /// The whole `use ...;` statement's extent, in the file it binds.
    pub span: Span,
}

/// One recorded definition the request can answer as an *Export*: the
/// symbol's name, the qualified module path the index recorded for it
/// (`None` for definitions that carry no module-level identity — impl
/// methods, for instance), and its byte-exact extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSymbol {
    /// The recorded symbol's name.
    pub name: String,
    /// The qualified module path the index recorded for the defining
    /// file (a crate-root item records its bare name; a nested item
    /// records `crate::mod1::Name`).
    pub qualified_path: Option<String>,
    /// The whole definition's extent, in the file that records it.
    pub location: DefinitionLocation,
}

/// The moniker fact seam (LSP-MONIKER).
///
/// The policy in [`monikers_at`] must stay testable without a live
/// `AnalysisDatabase` (parse trees, locks, indexes), so it reads the
/// recorded identities through this trait. The production
/// [`SessionMonikerSource`] borrows the driver's stored [`SymbolIndex`]
/// and the file's stored parse tree; a future project-wide path store
/// would slot in behind the same seam without touching the policy.
pub trait MonikerSource {
    /// Every recorded definition of `name` across the session, in the
    /// index's recording order; the policy applies its own ordering and
    /// skip rules on top.
    fn recorded_symbols(&self, name: &str) -> Vec<RecordedSymbol>;

    /// The `use` bindings of `name` in `file_id`, with the paths the
    /// source spells. Globs and `as _` aliases bind no single name and
    /// never appear here. Order is not significant; the policy sorts.
    fn spelled_imports_of(&self, file_id: FileId, name: &str) -> Vec<SpelledImport>;
}

/// The wire-free kind of a moniker fact: which side of the import/export
/// edge the cursor's symbol sits on decides the wire `MonikerKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactKind {
    /// The symbol is imported into this project (a `use` binding).
    Import,
    /// The symbol is defined and exported by this project (a recorded
    /// definition).
    Export,
}

/// One identity the cursor's symbol answers with, wire-type-free: the
/// kind and the identifier that the wire `Moniker` carries verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonikerFact {
    /// Which side of the import/export edge the symbol sits on.
    pub kind: FactKind,
    /// The identity string: the spelled path for an import, the recorded
    /// qualified path for an export.
    pub identifier: String,
}

/// Production [`MonikerSource`]: borrows the driver's stored
/// [`SymbolIndex`] — the same store goto-definition and hover read — and
/// the cursor's file's stored parse tree, from which the spelled import
/// paths are walked. Borrowing (not cloning) keeps the walk
/// allocation-free apart from the returned vectors; the read guards live
/// in the caller, exactly like the goto-implementation handler.
pub struct SessionMonikerSource<'a> {
    index: &'a SymbolIndex,
    green: Option<&'a GreenNode>,
    file_id: FileId,
}

impl<'a> SessionMonikerSource<'a> {
    /// Bind the source to one request's session state: the shared symbol
    /// index, the cursor's file's parse tree (when the file was analysed),
    /// and the file id the request came from.
    #[must_use]
    pub fn new(index: &'a SymbolIndex, green: Option<&'a GreenNode>, file_id: FileId) -> Self {
        Self {
            index,
            green,
            file_id,
        }
    }
}

impl MonikerSource for SessionMonikerSource<'_> {
    fn recorded_symbols(&self, name: &str) -> Vec<RecordedSymbol> {
        self.index
            .lookup_by_name(name)
            .into_iter()
            .map(|sym| RecordedSymbol {
                name: sym.name.clone(),
                qualified_path: self
                    .index
                    .import_path_for(sym.definition.file_id, &sym.name)
                    .cloned(),
                location: sym.definition.clone(),
            })
            .collect()
    }

    fn spelled_imports_of(&self, file_id: FileId, name: &str) -> Vec<SpelledImport> {
        // The request path only ever asks for the cursor's own file, and
        // only that file's parse tree is borrowed here.
        if file_id != self.file_id {
            return Vec::new();
        }
        let Some(green) = self.green else {
            return Vec::new();
        };
        spelled_import_bindings(green, file_id)
            .into_iter()
            .filter(|(bound, _, _)| bound == name)
            .map(|(_, path, span)| SpelledImport { path, span })
            .collect()
    }
}

/// The path-segment tokens one `UseTree`'s own `UsePath` child spells, in
/// source order: the identifier and keyword segments (`crate`, `self`,
/// `super`) that name the path's parts.
fn path_segments_of(tree: &SyntaxNode) -> Vec<String> {
    tree.children()
        .find(|child| child.kind() == SyntaxKind::UsePath)
        .map(|path| {
            path.descendants_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| {
                    matches!(
                        token.kind(),
                        SyntaxKind::Ident
                            | SyntaxKind::KwCrate
                            | SyntaxKind::KwSelf
                            | SyntaxKind::KwSuper
                    )
                })
                .map(|token| token.text().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The `(bound name, spelled path)` one leaf `use` tree carries, if any.
///
/// `as Alias` renames the binding (the alias wins over the path's last
/// segment, while the identity stays the spelled path); `as _` is an
/// anonymous import and binds nothing. Globs (`use foo::*`) bind no
/// single name and are skipped. A path ending in `self`/`super`/`crate`
/// keywords names no importable item binding.
fn spelled_binding_of_leaf(tree: &SyntaxNode) -> Option<(String, String)> {
    let mut saw_as = false;
    let mut saw_glob = false;
    let mut alias: Option<String> = None;
    let mut parts: Vec<String> = Vec::new();
    for token in tree
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
    {
        match token.kind() {
            SyntaxKind::Star => saw_glob = true,
            SyntaxKind::KwAs => saw_as = true,
            SyntaxKind::Ident if saw_as => {
                alias = Some(token.text().to_string());
                break;
            }
            // The guarded `Ident` arm above already claimed the alias
            // case, so this arm only ever sees pre-`as` path segments.
            SyntaxKind::Ident | SyntaxKind::KwCrate | SyntaxKind::KwSelf | SyntaxKind::KwSuper => {
                parts.push(token.text().to_string());
            }
            _ => {}
        }
    }
    if saw_glob {
        return None;
    }
    let path = parts.join("::");
    let bound = if saw_as {
        // `as` with no following identifier is a parse error the parser
        // already reported; an `as _` alias is anonymous. Either way the
        // alias slot stays `None` and nothing binds.
        alias?
    } else {
        parts.pop()?
    };
    if path.is_empty() || matches!(bound.as_str(), "self" | "super" | "crate") {
        return None;
    }
    Some((bound, path))
}

/// Every leaf `use` binding in the parse tree, with the path the source
/// spells: `(bound name, spelled path, statement extent)` in source
/// order. The spelled path is the leaf's own segments prefixed by the
/// ancestor trees' paths (`use a::{b}` binds `b` under the identity
/// `a::b`), so nested shapes answer the full path, not the bare segment.
fn spelled_import_bindings(green: &GreenNode, file_id: FileId) -> Vec<(String, String, Span)> {
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
            let Some((bound, leaf_path)) = spelled_binding_of_leaf(&tree) else {
                continue;
            };
            // The ancestor trees' own path segments, innermost first;
            // reversed into source order and prefixed onto the leaf's
            // segments.
            let mut parts: Vec<String> = tree
                .ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor.kind() == SyntaxKind::UseTree)
                .flat_map(|ancestor| path_segments_of(&ancestor))
                .collect();
            parts.reverse();
            parts.extend(
                leaf_path
                    .split("::")
                    .filter(|segment| !segment.is_empty())
                    .map(str::to_string),
            );
            out.push((bound, parts.join("::"), decl_span));
        }
    }
    out
}

fn span_of(file_id: FileId, range: rowan::TextRange) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_raw(u32::from(range.start())),
        ByteIdx::from_raw(u32::from(range.end())),
        SyntaxContext::ROOT,
    )
}

/// Map one fact to the wire `Moniker`: the `glyim` scheme, the fact's
/// identifier verbatim, and `document` uniqueness — each session file
/// carries its own module tree, so the qualified path is the identity
/// the server can vouch for within its document (see the module docs).
#[must_use]
pub fn wire_moniker(fact: &MonikerFact) -> Moniker {
    Moniker {
        scheme: MONIKER_SCHEME.to_string(),
        identifier: fact.identifier.clone(),
        unique: UniquenessLevel::Document,
        kind: Some(match fact.kind {
            FactKind::Import => MonikerKind::Import,
            FactKind::Export => MonikerKind::Export,
        }),
    }
}

/// Collapse facts that carry the same identity — two session files
/// recording the same qualified path (or two use statements spelling the
/// same path) answer one moniker, not a duplicate list.
fn dedupe(facts: Vec<MonikerFact>) -> Vec<MonikerFact> {
    let mut out: Vec<MonikerFact> = Vec::new();
    for fact in facts {
        if !out.contains(&fact) {
            out.push(fact);
        }
    }
    out
}

/// Whether `offset` sits inside `span` (half-open, the span map's own
/// convention for extents).
fn contains(span: &Span, offset: usize) -> bool {
    span.lo.to_usize() <= offset && offset < span.hi.to_usize()
}

/// One import fact per distinct spelled path, ordered by (span start,
/// span end) so the answer is deterministic regardless of the source's
/// walk order.
fn import_facts(imports: &[SpelledImport]) -> Vec<MonikerFact> {
    let mut facts: Vec<(Span, MonikerFact)> = imports
        .iter()
        .filter(|import| !import.path.is_empty())
        .map(|import| {
            let span = import.span;
            (
                span,
                MonikerFact {
                    kind: FactKind::Import,
                    identifier: import.path.clone(),
                },
            )
        })
        .collect();
    facts.sort_by_key(|(span, _)| (span.lo.to_usize(), span.hi.to_usize()));
    dedupe(facts.into_iter().map(|(_, fact)| fact).collect())
}

/// The moniker choice: the identities the cursor's symbol answers with,
/// wire-type-free.
///
/// Resolution order mirrors the recorded facts' semantics: the cursor
/// parked on a `use` binding of the name answers that binding's spelled
/// path; parked on a recorded definition extent it answers the export
/// identity; a bare usage resolves by how the file binds the name — the
/// import when one binds it, the export identity otherwise. Definitions
/// the index records with zero-width extents (body locals today) or with
/// no qualified path are skipped honestly; a name no fact identifies
/// answers an empty walk.
#[must_use]
pub fn monikers_at(
    source: &dyn MonikerSource,
    file_id: FileId,
    offset: usize,
    name: &str,
) -> Vec<MonikerFact> {
    let imports = source.spelled_imports_of(file_id, name);

    // 1. The cursor on the import binding itself: the spelled path wins.
    let cursor_imports: Vec<MonikerFact> = import_facts(
        &imports
            .iter()
            .filter(|import| contains(&import.span, offset))
            .cloned()
            .collect::<Vec<_>>(),
    );
    if !cursor_imports.is_empty() {
        return cursor_imports;
    }

    // 2. The cursor on a recorded definition extent: the export identity.
    let cursor_exports: Vec<MonikerFact> = source
        .recorded_symbols(name)
        .into_iter()
        .filter(|sym| {
            sym.qualified_path
                .as_ref()
                .is_some_and(|path| !path.is_empty())
        })
        .filter(|sym| contains(&sym.location.span, offset))
        .map(|sym| MonikerFact {
            kind: FactKind::Export,
            identifier: sym.qualified_path.unwrap_or_default(),
        })
        .collect();
    if !cursor_exports.is_empty() {
        return dedupe(cursor_exports);
    }

    // 3. A bare usage of an imported name: the usage refers to the
    //    imported symbol, and every spelling the file binds is an answer.
    let bare_imports = import_facts(&imports);
    if !bare_imports.is_empty() {
        return bare_imports;
    }

    // 4. A bare usage of a recorded name: the symbol's export identity,
    //    one per distinct qualified path, ordered by (file id, span
    //    start) so the answer is deterministic regardless of the index's
    //    per-file recording order.
    let mut bare_exports: Vec<(DefinitionLocation, MonikerFact)> = source
        .recorded_symbols(name)
        .into_iter()
        .filter(|sym| {
            sym.qualified_path
                .as_ref()
                .is_some_and(|path| !path.is_empty())
        })
        .filter(|sym| sym.location.span.lo != sym.location.span.hi)
        .map(|sym| {
            let location = sym.location.clone();
            (
                location,
                MonikerFact {
                    kind: FactKind::Export,
                    identifier: sym.qualified_path.unwrap_or_default(),
                },
            )
        })
        .collect();
    bare_exports.sort_by_key(|(location, _)| {
        (
            location.file_id.to_raw(),
            location.span.lo.to_usize(),
            location.span.hi.to_usize(),
        )
    });
    dedupe(bare_exports.into_iter().map(|(_, fact)| fact).collect())
}

/// `textDocument/moniker` request entry point (LSP-MONIKER).
///
/// Resolution order mirrors the goto requests: URI → open-file lookup →
/// cursor offset (byte-safe, INF-12) → identifier extraction → the
/// cursor-position fact choice over the session's recorded identities.
/// Returns `None` when the document is not open, the cursor carries no
/// identifier, or no fact identifies the name; callers answer `null` for
/// that.
#[must_use]
pub fn monikers(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &MonikerParams,
) -> Option<Vec<Moniker>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let pos = params.text_document_position_params.position;

    // Offset + source snapshot (INF-12): the source map is read once for
    // both the wire-position mapping and the identifier extraction.
    let (source, offset) = {
        let source_maps = db.source_maps.read();
        let sm = source_maps.get(&file_id)?;
        let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
        (sm.source().to_string(), offset)
    };
    // Byte-offset, char-boundary-safe identifier extraction (INF-12).
    let name = identifier_at_offset(&source, offset)?;

    let index_guard = db.symbol_index.read();
    let syntax_guard = db.syntaxes.read();
    let green = syntax_guard.get(&file_id);
    let session = SessionMonikerSource::new(&index_guard, green, file_id);
    let facts = monikers_at(&session, file_id, offset, &name);
    drop(index_guard);
    drop(syntax_guard);

    if facts.is_empty() {
        return None;
    }
    tracing::debug!(name = %name, monikers = facts.len(), "monikers resolved");
    Some(facts.iter().map(wire_moniker).collect())
}
