//! Document-highlight request support (feature tag LSP-HIGHLIGHT).
//!
//! The server already exposed find-references and rename over the shared
//! `ReferenceGraph`, but no `textDocument/documentHighlight` handler — an
//! editor could not light up the occurrences of the symbol under the
//! cursor without issuing a full references request and filtering the
//! response by file. This module closes that capability gap.
//!
//! The seam: highlight *policy* (which occurrences qualify, and what kind
//! each one is) is split from occurrence *lookup* by
//! [`OccurrenceSource`]. The production [`ReferenceGraphSource`] borrows
//! the driver's shared `ReferenceGraph` — the same authoritative store
//! rename and find-references read (Phase 8.2, unstub-5) — while the
//! policy tests drive an in-memory source, no database required
//! (`tests/document_highlight.rs`). This mirrors how `rename` and
//! `find_references` already share the graph, and keeps the T129
//! scope-owner discipline in exactly one more place instead of being
//! re-derived a third time.
//!
//! Wire mapping (LSP spec): the definition site highlights as
//! `DocumentHighlightKind::TEXT`, read access as `READ`, write access as
//! `WRITE` — the classification `Reference::access` already carries, so
//! no re-analysis happens on the request path.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::navigation::identifier_at_offset;
use crate::reference_graph::{AccessKind, Reference, ReferenceGraph};
use glyim_span::FileId;
use lsp_types::{DocumentHighlight, DocumentHighlightKind, DocumentHighlightParams, Range};
use url::Url;

/// The occurrence-lookup seam for highlight requests (LSP-HIGHLIGHT).
///
/// The highlight policy in [`compute_highlights`] must stay testable
/// without a live `AnalysisDatabase` (locks, source maps, file ids), so it
/// reads occurrences through this trait. The production impl borrows the
/// shared [`ReferenceGraph`]; a future fast-path occurrence index would
/// slot in behind the same seam without touching the policy.
pub trait OccurrenceSource {
    /// Every recorded occurrence of `symbol`, across all open files.
    ///
    /// Order is not significant; the policy sorts the response by source
    /// position. Name-keyed like [`ReferenceGraph::find_references`], so
    /// scope filtering (T129 [LSP-5]) happens downstream, on the policy
    /// side.
    fn occurrences_of(&self, symbol: &str) -> Vec<Reference>;
}

/// Production [`OccurrenceSource`]: borrows the driver's shared
/// [`ReferenceGraph`].
///
/// Borrowing (not cloning) keeps the request allocation-free apart from
/// the returned vector; the guard lives in the caller, exactly like the
/// `find_references`/`rename` handlers.
pub struct ReferenceGraphSource<'a> {
    graph: &'a ReferenceGraph,
}

impl<'a> ReferenceGraphSource<'a> {
    /// Borrows `graph` as the occurrence source for one request.
    #[must_use]
    pub fn new(graph: &'a ReferenceGraph) -> Self {
        Self { graph }
    }
}

impl OccurrenceSource for ReferenceGraphSource<'_> {
    fn occurrences_of(&self, symbol: &str) -> Vec<Reference> {
        self.graph.find_references(symbol).to_vec()
    }
}

/// The highlight decision for one occurrence, wire-type-free.
///
/// Carries the raw [`glyim_span::Span`] so the policy tests assert on
/// byte offsets; the wire conversion in [`document_highlights`] maps it
/// to an LSP [`Range`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    /// The source extent that lights up.
    pub span: glyim_span::Span,
    /// Why it lights up (definition / read / write).
    pub kind: HighlightKind,
}

/// Why an occurrence is highlighted; mirrors LSP `DocumentHighlightKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    /// The definition site of the symbol (`DocumentHighlightKind::TEXT`).
    Text,
    /// A read access (`DocumentHighlightKind::READ`).
    Read,
    /// A write access (`DocumentHighlightKind::WRITE`).
    Write,
}

/// Highlight policy: the occurrences of the symbol under the cursor, in
/// the *current document only*, with per-occurrence kinds.
///
/// Returns `None` when the cursor is not on any recorded occurrence of
/// `identifier` (no graph entry, or the identifier text has none) — the
/// server answers such requests with `null`, matching how
/// `find_references` and `goto_definition` answer misses. Scope rules are
/// the ones rename and find-references already enforce:
///
/// - T129 [LSP-5]: if the cursor sits on a scoped (non-item-level)
///   occurrence, only occurrences sharing its `owner_item_id` qualify;
///   item-level names highlight globally.
/// - T053 [LSP-4]: DUMMY-span occurrences (pattern bindings, which HIR
///   `Pat` has no span for yet) are skipped — they would render as a
///   bogus highlight at file position 0:0.
/// - Per the LSP spec the response covers one document, so occurrences
///   in other files are dropped (unlike find-references).
#[must_use]
pub fn compute_highlights(
    source: &dyn OccurrenceSource,
    file_id: FileId,
    cursor_offset: usize,
    identifier: &str,
) -> Option<Vec<HighlightSpan>> {
    let occurrences = source.occurrences_of(identifier);

    // The cursor must land on a real occurrence in this file; that
    // occurrence decides whether the highlight is scope-filtered.
    let cursor_ref = occurrences.iter().find(|r| {
        !r.span.is_dummy()
            && r.file_id == file_id
            && r.span.lo.to_usize() <= cursor_offset
            && cursor_offset < r.span.hi.to_usize()
    })?;
    let scope_owner: Option<u32> = if cursor_ref.is_item_level {
        None
    } else {
        cursor_ref.owner_item_id
    };

    let mut highlights: Vec<HighlightSpan> = occurrences
        .iter()
        .filter(|r| r.file_id == file_id)
        .filter(|r| {
            if let Some(owner) = scope_owner {
                r.owner_item_id == Some(owner)
            } else {
                true
            }
        })
        .filter(|r| !r.span.is_dummy())
        .map(|r| HighlightSpan {
            span: r.span,
            kind: if r.is_definition {
                HighlightKind::Text
            } else if r.access == AccessKind::Write {
                HighlightKind::Write
            } else {
                HighlightKind::Read
            },
        })
        .collect();
    // Deterministic response order: by byte offset (then span width).
    highlights.sort_by_key(|h| (h.span.lo.to_usize(), h.span.hi.to_usize()));
    if highlights.is_empty() {
        None
    } else {
        tracing::debug!(
            symbol = %identifier,
            highlights = highlights.len(),
            "document highlight computed"
        );
        Some(highlights)
    }
}

/// `textDocument/documentHighlight` request entry point (LSP-HIGHLIGHT).
///
/// Resolution order mirrors `goto_definition`/`rename`: URI → open-file
/// lookup → cursor offset (byte-safe, INF-12) → identifier text →
/// occurrence policy → wire ranges. Returns `None` when the document is
/// not open, the cursor is off an identifier, or the identifier has no
/// recorded occurrence; callers answer `null` for that.
///
/// Cross-file occurrences of the same item-level name are excluded here
/// (document highlight is per-document) even though the underlying graph
/// is workspace-wide.
#[must_use]
pub fn document_highlights(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &DocumentHighlightParams,
) -> Option<Vec<DocumentHighlight>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let pos = params.text_document_position_params.position;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let source = sm.source();

    // Byte-offset, char-boundary-safe identifier extraction (INF-12).
    let identifier = identifier_at_offset(source, offset)?;

    let ref_graph = db.reference_graph.read();
    let occurrence_source = ReferenceGraphSource::new(&ref_graph);
    let highlights = compute_highlights(&occurrence_source, file_id, offset, &identifier)?;

    let mut wire = Vec::with_capacity(highlights.len());
    for h in highlights {
        // T131-style tolerance: a span whose SourceMap is gone (closed
        // file mid-request) must not abort the whole response. All
        // highlights are in `file_id`, whose SourceMap was already read,
        // so this only guards against an in-flight invalidation race.
        let Some(((start_line, start_col), (end_line, end_col))) =
            sm.span_to_position(h.span.lo.to_usize(), h.span.hi.to_usize())
        else {
            continue;
        };
        let kind = match h.kind {
            HighlightKind::Text => DocumentHighlightKind::TEXT,
            HighlightKind::Read => DocumentHighlightKind::READ,
            HighlightKind::Write => DocumentHighlightKind::WRITE,
        };
        wire.push(DocumentHighlight {
            range: Range {
                start: lsp_types::Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: lsp_types::Position {
                    line: end_line as u32,
                    character: end_col as u32,
                },
            },
            kind: Some(kind),
        });
    }
    if wire.is_empty() { None } else { Some(wire) }
}
