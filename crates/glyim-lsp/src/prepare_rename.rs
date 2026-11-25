//! Prepare-rename request support (feature tag LSP-PREPARE-RENAME).
//!
//! The server advertised `rename_provider` as a plain boolean, so clients
//! sent `textDocument/rename` from any cursor position — including
//! keywords, literals, and whitespace — and the server answered with a
//! best-effort edit or `null`. The LSP protocol's prepare step exists for
//! exactly this: `textDocument/prepareRename` lets a client ask "is this
//! position renameable?" *before* opening the rename input, so the input
//! only appears on symbols the server can actually rename.
//!
//! The seam: the renameability decision reads occurrences through the
//! same [`OccurrenceSource`] trait the highlight engine introduced
//! (LSP-HIGHLIGHT), so prepare-rename and document-highlight agree on
//! what counts as a known symbol — they read the one authoritative
//! `ReferenceGraph` through one seam, and the policy side
//! ([`rename_target`]) stays testable without a database
//! (`tests/prepare_rename.rs`).
//!
//! Wire mapping: a graphed identifier answers
//! `PrepareRenameResponse::RangeWithPlaceholder` carrying the identifier's
//! exact token extent (LSP-EXACT-EXTENTS) and its text as the placeholder;
//! anything else (closed file, cursor off an identifier, ungraphed token)
//! answers `null`, which clients interpret as "not renameable here".

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::document_highlight::{OccurrenceSource, ReferenceGraphSource};
use glyim_span::{ByteIdx, Span, SyntaxContext};
use lsp_types::{PrepareRenameResponse, Range, TextDocumentPositionParams};
use url::Url;

/// The prepare-rename decision: when `identifier` has recorded
/// occurrences, its token extent and text; `None` otherwise.
///
/// `token` must be the identifier's own extent (the caller derives it
/// from the source text, so the wire response can carry the exact range
/// the client pre-selects). Graphed-but-DUMMY-span occurrences still
/// count — the *decision* is about renameability; DUMMY spans are a
/// rendering concern handled per-occurrence by the highlight and rename
/// paths (T053 [LSP-4]).
#[must_use]
pub fn rename_target(
    source: &dyn OccurrenceSource,
    identifier: &str,
    token: Span,
) -> Option<(Span, String)> {
    if source.occurrences_of(identifier).is_empty() {
        None
    } else {
        Some((token, identifier.to_string()))
    }
}

/// `textDocument/prepareRename` request entry point (LSP-PREPARE-RENAME).
///
/// Resolution order mirrors `document_highlights`: URI → open-file lookup
/// → cursor offset (byte-safe, INF-12) → identifier extent → occurrence
/// check through the shared seam → wire response. Returns `None` when the
/// document is not open, the cursor is off an identifier, or the
/// identifier has no recorded occurrences; the client disables rename
/// input for that position.
#[must_use]
pub fn prepare_rename(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &TextDocumentPositionParams,
) -> Option<PrepareRenameResponse> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let pos = params.position;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let source = sm.source();

    // Byte-offset, char-boundary-safe identifier extent (INF-12).
    let (start, end) = crate::navigation::identifier_extent_at_offset(source, offset)?;
    let identifier = &source[start..end];
    let token = Span::range(
        file_id,
        ByteIdx::from_usize(start),
        ByteIdx::from_usize(end),
        SyntaxContext::ROOT,
    );

    let ref_graph = db.reference_graph.read();
    let occurrence_source = ReferenceGraphSource::new(&ref_graph);
    let (_span, placeholder) = rename_target(&occurrence_source, identifier, token)?;

    // Exact token extents (LSP-EXACT-EXTENTS): the pre-selected range is
    // the identifier itself, so a client replacing the selection edits
    // precisely the symbol.
    let ((start_line, start_col), (end_line, end_col)) =
        sm.span_to_position(start, end).unwrap_or(((0, 0), (0, 0)));
    tracing::debug!(
        symbol = %placeholder,
        "prepare rename accepted"
    );
    Some(PrepareRenameResponse::RangeWithPlaceholder {
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
        placeholder,
    })
}
