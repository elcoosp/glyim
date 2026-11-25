//! Code-lens request support (feature tag LSP-CODE-LENS).
//!
//! Every navigation request in this server answers a *cursor* question —
//! the editor already knows where the user is looking. Nothing answered
//! the *inventory* question: which top-level items in this file are
//! worth attention before the user scrolls, and how much. The editor
//! idiom for that is `textDocument/codeLens` — the above-the-line
//! annotations ("N references") editors render without a cursor on the
//! line. This module closes that capability gap with the full two-request
//! contract: `textDocument/codeLens` enumerates the lenses,
//! `codeLens/resolve` attaches the live command.
//!
//! The seam: *which items are lensable and how often each name is used*
//! is split from *where those facts come from* by [`LensSource`],
//! mirroring LSP-DOC-LINK's `LinkLexSource`. The policy
//! ([`compute_code_lenses`]) reads through the trait and stays testable
//! without a reference-graph lock (`tests/code_lens.rs`); the production
//! [`GraphLensSource`] reads the driver's shared [`ReferenceGraph`] —
//! the same store find-references, rename, document-highlight,
//! linked-editing and call-hierarchy read, so no re-analysis happens on
//! the request path and the capability works on whatever the session has
//! analysed, including partially broken code.
//!
//! What counts as a lens site: exactly the graph's *item-level*
//! definitions (`Reference::is_item_level`, T129-PATCHED [LSP-5]) of the
//! requested document. Locals, params, fields, and variants are scoped
//! to one item and never lens — a lens above `let x = …` would be
//! noise. The use count is the same global name-keyed count
//! find-references answers for an item-level name (item-level names are
//! cross-module by definition; the graph's `owner_item_id` filter
//! deliberately does not apply to them), minus the definition records
//! themselves — "2 references" never counts the item's own name token.
//!
//! Wire mapping (LSP spec): each [`ItemLens`] becomes one `CodeLens`
//! whose range anchors the lens on the item's first line — the graph's
//! item-level definition records carry the *whole item's* extent (that
//! is what `build_from_hir` marks), so the wire range is that extent
//! clamped to its first line (the LSP spec asks a lens to span a single
//! line, and the anchor editors render above is `range.start`; a
//! single-line extent answers exactly, a multi-line item clamps
//! honestly). The lens carries `command: None` and a `data` payload
//! naming the document (`{"uri": …}`). The resolve re-derives the answer
//! the way LSP-CALL-HIERARCHY does: the client's echoed range start maps
//! back to the byte offset through the same `SourceMap` the lens request
//! mapped it from, and it must match a live item-level definition on the
//! graph — a stale or fabricated echo answers `null`, never a guessed
//! count. A resolved lens carries the command `glyim.showReferences`
//! with the title rendered from the live count (`0 references`,
//! `1 reference`, `N references`) — the count is recomputed at resolve
//! time, so a session that analysed more callers between the two
//! requests answers with the current number, not the stale one.
//!
//! Responses come in source order (the definition walk is sorted by
//! extent) and the response is `null` when the document is not open or
//! carries no item-level definition, per the other engines' convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::reference_graph::ReferenceGraph;
use glyim_span::FileId;
use lsp_types::{CodeLens, CodeLensParams, Command, Position, Range};
use serde_json::json;
use url::Url;

/// The command id a resolved lens carries. The title is the visible
/// contract ("N references"); the id is namespaced per-server so a client
/// that reacts to it does so deliberately.
pub const SHOW_REFERENCES_COMMAND: &str = "glyim.showReferences";

/// One lensable item, wire-type-free: name and the item-level
/// definition record's extent (the whole item, as the graph records it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LensItem {
    /// The item's own name (the graph's key for it).
    pub name: String,
    /// Byte offset of the item's first byte (the lens line anchor).
    pub lo: usize,
    /// Byte offset one past the item's last byte.
    pub hi: usize,
}

/// The item-lens seam for code-lens requests (LSP-CODE-LENS).
///
/// The lens policy in [`compute_code_lenses`] must stay testable without
/// a live `AnalysisDatabase` (reference-graph locks, source maps), so it
/// reads item facts through this trait. The production
/// [`GraphLensSource`] reads the driver's shared reference graph; a
/// future source over un-analysed syntax trees (names only, zero counts)
/// would slot in behind the same seam without touching the policy.
pub trait LensSource {
    /// Every lensable item of one document in source order.
    fn lens_items(&self) -> Vec<LensItem>;
    /// How many uses (non-definition references) the item-level name
    /// `name` has across the analysed session.
    fn use_count(&self, name: &str) -> usize;
}

/// One item lens with its live use count, wire-type-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLens {
    /// The item's own name (echoed in the resolve payload).
    pub name: String,
    /// Byte offset of the item's first byte (the lens line anchor).
    pub lo: usize,
    /// Byte offset one past the item's last byte.
    pub hi: usize,
    /// Uses (non-definition references) of the name across the session.
    pub uses: usize,
}

/// Code-lens policy: one lens per lensable item, in source order, with
/// its use count joined per name.
///
/// A name the seam reports as empty produces no lens — there is nothing
/// to resolve against, and a lens on an unnamed extent would be a lie.
/// Every named item answers a lens, including a `uses == 0` one: an
/// unused top-level item is exactly what a code lens is for.
#[must_use]
pub fn compute_code_lenses(source: &dyn LensSource) -> Vec<ItemLens> {
    source
        .lens_items()
        .into_iter()
        .filter(|item| !item.name.is_empty())
        .map(|item| ItemLens {
            uses: source.use_count(&item.name),
            name: item.name,
            lo: item.lo,
            hi: item.hi,
        })
        .collect()
}

/// Production [`LensSource`]: borrows the driver's shared reference
/// graph for one file.
///
/// Lens sites are the graph's item-level definitions of that file (the
/// `is_item_level` post-pass in `ReferenceGraph::build_from_hir` marks
/// exactly the top-level items' own name tokens); dummy-span records the
/// graph keeps for synthesized shapes are skipped, and the walk is
/// sorted by extent because the underlying store is a name-keyed map
/// with no source order.
pub struct GraphLensSource<'a> {
    graph: &'a ReferenceGraph,
    file_id: FileId,
}

impl<'a> GraphLensSource<'a> {
    /// Reads item facts for `file_id` out of `graph`.
    #[must_use]
    pub fn new(graph: &'a ReferenceGraph, file_id: FileId) -> Self {
        Self { graph, file_id }
    }
}

impl LensSource for GraphLensSource<'_> {
    fn lens_items(&self) -> Vec<LensItem> {
        let mut items: Vec<LensItem> = self
            .graph
            .iter_with_names()
            .filter(|(_, reference)| {
                reference.file_id == self.file_id
                    && reference.is_definition
                    && reference.is_item_level
                    && !reference.span.is_dummy()
            })
            .map(|(name, reference)| LensItem {
                name: name.to_string(),
                lo: reference.span.lo().to_usize(),
                hi: reference.span.hi().to_usize(),
            })
            .collect();
        items.sort_by_key(|item| (item.lo, item.hi));
        items
    }

    fn use_count(&self, name: &str) -> usize {
        self.graph
            .find_references(name)
            .iter()
            .filter(|reference| !reference.is_definition && !reference.span.is_dummy())
            .count()
    }
}

/// Render the resolve command's title from a live use count (the grammar
/// the editors' own "references" lenses use).
#[must_use]
fn references_title(uses: usize) -> String {
    match uses {
        0 => "0 references".to_string(),
        1 => "1 reference".to_string(),
        n => format!("{n} references"),
    }
}

/// `textDocument/codeLens` request entry point (LSP-CODE-LENS).
///
/// Resolution order mirrors `document_links`: URI → open-file lookup →
/// the shared reference graph (no re-analysis) → lens policy → wire
/// lenses. Returns `None` when the document is not open, its source map
/// is not stored, or it carries no item-level definition; callers answer
/// `null` for that.
#[must_use]
pub fn code_lenses(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CodeLensParams,
) -> Option<Vec<CodeLens>> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let lenses = {
        let graph = db.reference_graph.read();
        compute_code_lenses(&GraphLensSource::new(&graph, file_id))
    };
    if lenses.is_empty() {
        return None;
    }

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let mut wire = Vec::with_capacity(lenses.len());
    for lens in lenses {
        // The extent was read from this same document's graph, so it maps
        // cleanly; the guard keeps a mid-request invalidation race from
        // aborting the whole response (one unmappable lens is dropped,
        // the good ones survive).
        let Some(((start_line, start_col), _)) = sm.span_to_position(lens.lo, lens.hi) else {
            continue;
        };
        // The graph's item-level records carry the whole item's extent;
        // the LSP spec asks a lens to span a single line, so a multi-line
        // item clamps its range to the item's first line (the anchor
        // editors render above is `range.start`). A single-line extent —
        // one-liner items, the synthetic test graphs — answers exactly.
        let content = sm.source();
        let line_end = content[lens.lo..]
            .find('\n')
            .map_or(content.len(), |index| lens.lo + index);
        let hi = if lens.hi <= line_end {
            lens.hi
        } else {
            line_end
        };
        let Some((_, end_col)) = sm.span_to_position(hi, hi).map(|(start, _)| start) else {
            continue;
        };
        wire.push(CodeLens {
            range: Range {
                start: Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: start_line as u32,
                    character: end_col as u32,
                },
            },
            command: None,
            data: Some(json!({ "uri": uri.as_str() })),
        });
    }
    if wire.is_empty() {
        return None;
    }
    tracing::debug!(lenses = wire.len(), "code lenses resolved");
    Some(wire)
}

/// `codeLens/resolve` request entry point (LSP-CODE-LENS).
///
/// The client echoes the lens it is about to render; the resolve
/// re-derives the answer from the recorded facts instead of trusting the
/// echo: the payload's URI names the document, the echoed range's start
/// maps back to the byte offset through the same `SourceMap` the lens
/// request mapped it from, and it must match a live item-level
/// definition on the reference graph (the LSP-CALL-HIERARCHY join — the
/// start offset identifies the item; the echoed end is the lens line the
/// wire mapping clamped to, not the item's full extent). Returns `None`
/// — the caller answers with a JSON-RPC error, the result type is not
/// optional — when the echo carries no payload, the document is not
/// open, the start matches no lensable definition, or the source map is
/// gone.
#[must_use]
pub fn resolve_code_lens(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CodeLens,
) -> Option<CodeLens> {
    // The payload is the only carrier of the document URI on this
    // request (the resolve params have no `textDocument`); a client that
    // strips `data` cannot be answered honestly.
    let uri = params.data.as_ref()?.get("uri")?.as_str()?;
    let path = Url::parse(uri).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    // The echoed wire range's start maps back to a byte offset through
    // the same `SourceMap` the lens request mapped it from.
    let lo = {
        let source_maps = db.source_maps.read();
        let sm = source_maps.get(&file_id)?;
        sm.line_col_to_offset(
            params.range.start.line as usize,
            params.range.start.character as usize,
        )?
    };

    // Re-join against the graph: the start offset must be a live
    // item-level definition of this document. The count is read at
    // resolve time, so analysis that landed between the two requests is
    // reflected.
    let (name, uses) = {
        let graph = db.reference_graph.read();
        let source = GraphLensSource::new(&graph, file_id);
        let item = source.lens_items().into_iter().find(|item| item.lo == lo)?;
        let uses = source.use_count(&item.name);
        (item.name, uses)
    };

    tracing::debug!(name = %name, uses, "code lens resolved");
    Some(CodeLens {
        range: params.range,
        command: Some(Command {
            title: references_title(uses),
            command: SHOW_REFERENCES_COMMAND.to_string(),
            arguments: Some(vec![json!(uri), json!(params.range)]),
        }),
        data: params.data.clone(),
    })
}
