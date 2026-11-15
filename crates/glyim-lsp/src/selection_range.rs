//! Selection-range request support (feature tag LSP-SELECTION).
//!
//! The server resolves names, occurrences, and types, but never answered
//! `textDocument/selectionRange` — an editor's expand-selection could only
//! guess from tokens, walking outward one character at a time instead of
//! jumping token → statement → item → file. This module closes that
//! capability gap on top of the parse trees the analysis driver already
//! builds.
//!
//! Until now the driver threw the rowan tree away after lowering to HIR;
//! the only syntax-tree consumer left in the request path was nothing at
//! all. The wire-up is the architectural seam here: `AnalysisDatabase`
//! gains a `syntaxes` store (one rowan root per open file, populated by
//! the driver next to `hirs`/`typeck`), and this module walks it on the
//! request path — no re-parse per request.
//!
//! The seam: ancestor *lookup* is split from selection *policy* by
//! [`AncestorSpanSource`], mirroring LSP-HIGHLIGHT's `OccurrenceSource`.
//! The production [`ParseTreeSource`] borrows the stored rowan root and
//! yields the raw ancestor chain (innermost non-trivia token first);
//! the policy ([`compute_selection_chains`]) normalizes each chain —
//! collapses consecutive duplicate extents (rowan wrapper nodes that
//! share their only child's range), validates the spec's "position
//! contained in range" contract, and keeps a `None` slot per unresolvable
//! position — while the wire function builds the parent-linked
//! [`SelectionRange`] response. The policy tests drive an in-memory
//! source, no database required (`tests/selection_range.rs`); the
//! parse-tree-backed path is covered end-to-end in
//! `tests/selection_range_tests.rs`.
//!
//! Wire mapping (LSP spec): one result per requested position, in order;
//! the innermost entry is the token's own extent, the outermost is the
//! whole document, and the `parent` links strictly widen. Positions that
//! do not resolve (closed file, out-of-bounds, empty tree) produce no
//! entry — the spec permits a server to omit ranges it cannot compute,
//! and a partial response beats failing every cursor on one bad one.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::database::SourceMap;
use glyim_span::{ByteIdx, FileId, Span, SyntaxContext};
use glyim_syntax::{GreenNode, SyntaxKind, SyntaxNode};
use lsp_types::{Position, Range, SelectionRange, SelectionRangeParams};
use rowan::{TextRange, TextSize, TokenAtOffset};
use url::Url;

/// The syntax-ancestor seam for selection-range requests (LSP-SELECTION).
///
/// The selection policy in [`compute_selection_chains`] must stay testable
/// without a live `AnalysisDatabase` (parse trees, locks, source maps), so
/// it reads ancestor chains through this trait. The production
/// [`ParseTreeSource`] walks the driver's stored parse tree; a future
/// partial-reparse or incremental-tree source would slot in behind the
/// same seam without touching the policy.
pub trait AncestorSpanSource {
    /// The raw ancestor chain of the innermost non-trivia token at
    /// `offset`: the token's own extent first, then every enclosing node
    /// out to the file root. Consecutive duplicate extents are *kept*
    /// here — collapsing them is policy (the tests pin that split).
    /// `None` when `offset` carries no tree (outside the root's range or
    /// an empty file).
    fn ancestors_at(&self, offset: usize) -> Option<Vec<Span>>;
}

/// Production [`AncestorSpanSource`]: borrows the driver's stored parse
/// tree for one file.
///
/// The store keeps the immutable [`GreenNode`] (rowan cursor nodes carry
/// raw parent pointers and are not `Send`, so they cannot live in the
/// shared database); each request materializes its own cursor root with
/// one `Arc` clone — the same trick the parser uses — instead of
/// re-parsing the file.
pub struct ParseTreeSource<'a> {
    green: &'a GreenNode,
    file_id: FileId,
}

impl<'a> ParseTreeSource<'a> {
    /// Borrows `green` as the ancestor source for one file's requests.
    #[must_use]
    pub fn new(green: &'a GreenNode, file_id: FileId) -> Self {
        Self { green, file_id }
    }
}

/// Whitespace and comments never open a selection: a cursor inside them
/// still selects the nearest *meaningful* token, not a run of spaces.
fn is_trivia(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Whitespace
            | SyntaxKind::LineComment
            | SyntaxKind::BlockComment
            | SyntaxKind::DocComment
    )
}

fn span_of(file_id: FileId, range: TextRange) -> Span {
    Span::new(
        file_id,
        ByteIdx::from_raw(u32::from(range.start())),
        ByteIdx::from_raw(u32::from(range.end())),
        SyntaxContext::ROOT,
    )
}

impl AncestorSpanSource for ParseTreeSource<'_> {
    fn ancestors_at(&self, offset: usize) -> Option<Vec<Span>> {
        // Materialize a cursor root for this request (one `Arc` clone of
        // the green tree; cursor nodes themselves are not `Send`, which
        // is why the store keeps the green form).
        let root = SyntaxNode::new_root(self.green.clone());
        // Offsets beyond u32 (or past the tree) cannot name a rowan byte.
        let at = TextSize::try_from(offset).ok()?;
        let root_range = root.text_range();
        if !(root_range.start()..=root_range.end()).contains(&at) {
            return None;
        }
        let token = match root.token_at_offset(at) {
            TokenAtOffset::None => return None,
            TokenAtOffset::Single(token) => token,
            TokenAtOffset::Between(left, right) => {
                // Deterministic boundary rule: prefer the token the cursor
                // is *inside* (a boundary point belongs to neither, so
                // fall back left — matching expand-selection habits where
                // the preceding token is what the user just typed).
                match (is_trivia(left.kind()), is_trivia(right.kind())) {
                    (true, false) => right,
                    (false, true) => left,
                    _ => left,
                }
            }
        };
        let mut spans = vec![span_of(self.file_id, token.text_range())];
        for node in token.parent_ancestors() {
            spans.push(span_of(self.file_id, node.text_range()));
        }
        Some(spans)
    }
}

/// Selection policy: normalize one raw ancestor chain per cursor offset.
///
/// The response keeps the request's shape — one slot per offset, in
/// order — because the LSP spec pins `result[i]` to `positions[i]`.
/// Per chain:
///
/// - Every span must contain the cursor (inclusive on both ends: a
///   boundary cursor selects the token it borders). A chain starting
///   elsewhere violates the spec's containment contract and is dropped.
/// - Consecutive duplicate extents collapse (rowan wrapper nodes that
///   span exactly their only child would otherwise make the user press
///   expand-selection twice with no visual change).
/// - An empty chain resolves to `None` (nothing to select).
#[must_use]
pub fn compute_selection_chains(
    source: &dyn AncestorSpanSource,
    offsets: &[usize],
) -> Vec<Option<Vec<Span>>> {
    offsets
        .iter()
        .map(|&offset| {
            let raw = source.ancestors_at(offset)?;
            let mut chain: Vec<Span> = Vec::with_capacity(raw.len());
            for span in raw {
                let (lo, hi) = (span.lo.to_usize(), span.hi.to_usize());
                if offset < lo || offset > hi {
                    return None;
                }
                if chain
                    .last()
                    .is_some_and(|last| last.lo.to_usize() == lo && last.hi.to_usize() == hi)
                {
                    continue;
                }
                chain.push(span);
            }
            if chain.is_empty() { None } else { Some(chain) }
        })
        .collect()
}

/// Build the parent-linked wire response for one chain, outermost first.
///
/// All spans belong to the file whose `SourceMap` the caller already
/// resolved; a `None` here can only mean an in-flight invalidation race
/// (T131-style), and aborts this one chain rather than the request.
fn chain_to_wire(sm: &SourceMap, chain: &[Span]) -> Option<SelectionRange> {
    let mut current: Option<SelectionRange> = None;
    for span in chain.iter().rev() {
        let ((start_line, start_col), (end_line, end_col)) =
            sm.span_to_position(span.lo.to_usize(), span.hi.to_usize())?;
        current = Some(SelectionRange {
            range: Range {
                start: Position {
                    line: start_line as u32,
                    character: start_col as u32,
                },
                end: Position {
                    line: end_line as u32,
                    character: end_col as u32,
                },
            },
            parent: current.map(Box::new),
        });
    }
    current
}

/// `textDocument/selectionRange` request entry point (LSP-SELECTION).
///
/// Resolution order mirrors `document_highlights`: URI → open-file
/// lookup → stored parse tree → per-position cursor offsets (byte-safe,
/// INF-11/INF-12) → policy chains → wire linked list. Returns `None` when
/// the document is not open, has no parse tree, or no position resolves;
/// unresolvable positions inside a resolvable document are skipped (see
/// the module docs for why partial answers are honest here).
#[must_use]
pub fn selection_ranges(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &SelectionRangeParams,
) -> Option<Vec<SelectionRange>> {
    if params.positions.is_empty() {
        return None;
    }
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let syntaxes = db.syntaxes.read();
    let root = syntaxes.get(&file_id)?;

    // Positions that fail the UTF-16 → byte mapping contribute an
    // offset no tree can contain, which the policy answers with `None`
    // (same shape the spec's per-position nullability describes).
    let offsets: Vec<usize> = params
        .positions
        .iter()
        .map(|position| {
            sm.line_col_to_offset(position.line as usize, position.character as usize)
                .unwrap_or(usize::MAX)
        })
        .collect();
    let chains = compute_selection_chains(&ParseTreeSource::new(root, file_id), &offsets);

    let mut wire = Vec::with_capacity(chains.len());
    for chain in chains.into_iter().flatten() {
        if let Some(selection) = chain_to_wire(sm, &chain) {
            wire.push(selection);
        }
    }
    if wire.is_empty() {
        None
    } else {
        tracing::debug!(
            positions = params.positions.len(),
            resolved = wire.len(),
            "selection ranges computed"
        );
        Some(wire)
    }
}
