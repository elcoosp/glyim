//! Linked-editing-range request support (feature tag LSP-LINKED-EDIT).
//!
//! The server already lights up the occurrences of the cursor's symbol
//! (`textDocument/documentHighlight`) and can rename it across the
//! workspace (`textDocument/rename`) — but nothing answered the in-place
//! multi-cursor question editors ask before either:
//! `textDocument/linkedEditingRange` did not exist, so an editor cannot
//! offer "type the new name once, all occurrences follow" without a
//! round-trip through rename. This module closes that capability gap.
//!
//! The seam: this request reads the *same* occurrence facts through the
//! *same* [`crate::document_highlight::OccurrenceSource`] seam
//! document-highlight reads — the shared [`crate::reference_graph::ReferenceGraph`]
//! both find-references and rename read (Phase 8.2, unstub-5) — and its
//! *choice* is the document-highlight policy
//! ([`crate::document_highlight::compute_highlights`]) applied verbatim:
//! T129 [LSP-5] scope-owner filtering (two same-name locals in different
//! function bodies must never edit together), T053 [LSP-4] DUMMY-span
//! skip, per-document scope, deterministic order. A second
//! re-derivation of those rules would be exactly the drift the
//! LSP-DEAD-DEF audit removed, so the capability composes the seam
//! instead of duplicating it. The new decision here is the wire shape:
//! kinds are dropped (linked editing is kind-free by spec) and the
//! ranges become one edit group.
//!
//! Wire mapping (LSP spec): the response is a single
//! `LinkedEditingRanges` — one group of exact token extents sorted by
//! position, so a client's multi-cursor substitution is stable — with
//! `word_pattern` left unset: the client's default identifier pattern
//! covers the language's word shape, and the server's contract is the
//! ranges themselves. Off-symbol cursors and unopened documents answer
//! `null`, per the other engines' convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::document_highlight::{ReferenceGraphSource, compute_highlights};
use crate::navigation::identifier_at_offset;
use lsp_types::{LinkedEditingRangeParams, LinkedEditingRanges};
use url::Url;

/// `textDocument/linkedEditingRange` request entry point
/// (LSP-LINKED-EDIT).
///
/// Resolution order mirrors `document_highlights` (the request path this
/// capability composes): URI → open-file lookup → cursor offset
/// (byte-safe, INF-12) → identifier text → occurrence policy through the
/// shared seam → wire ranges. Returns `None` when the document is not
/// open, the cursor carries no identifier, or the identifier has no
/// recorded occurrence; callers answer `null` for that.
#[must_use]
pub fn linked_editing_ranges(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &LinkedEditingRangeParams,
) -> Option<LinkedEditingRanges> {
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

    let ranges = highlights
        .iter()
        .filter_map(|h| {
            // All highlights are in `file_id`, whose SourceMap is already
            // read; `?` drops an in-flight invalidation race (one
            // unmappable literal is dropped, the good ones survive).
            let ((start_line, start_col), (end_line, end_col)) =
                sm.span_to_position(h.span.lo.to_usize(), h.span.hi.to_usize())?;
            Some(lsp_types::Range {
                start: lsp_types::Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: lsp_types::Position {
                    line: end_line as u32,
                    character: end_col as u32,
                },
            })
        })
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        return None;
    }
    tracing::debug!(
        symbol = %identifier,
        ranges = ranges.len(),
        "linked editing ranges resolved"
    );
    Some(LinkedEditingRanges {
        ranges,
        // The client's default identifier pattern covers the language's
        // word shape; the server's contract is the ranges themselves.
        word_pattern: None,
    })
}
