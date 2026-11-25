//! Inline-value request support (feature tag LSP-INLINE-VALUE).
//!
//! The server already answers "what type is this binding" (inlay hints,
//! LSP-INLAY-HINT) and "where is this binding declared" (rename uses the
//! same recorded pattern spans), but nothing answered the *debugger's*
//! question: `textDocument/inlineValue` did not exist, so a debug adapter
//! stopped on a frame had no wire answer for "which variables in this
//! range should the editor render inline". Every fact the request needs
//! was already recorded — the HIR carries every `let` binding with the
//! bound identifier's byte-exact `pat_span` (the same spans rename edits
//! consume). This module closes that capability gap with no re-analysis
//! on the request path.
//!
//! The seam: *which recorded bindings count* is split from *how those
//! bindings are found* by [`InlineValueSource`], mirroring
//! LSP-INLAY-HINT's `BindingTypeSource`. The policy
//! ([`compute_inline_values`]) reads through the trait and stays testable
//! without a HIR arena or locks (`tests/inline_value.rs`); the production
//! [`HirInlineValueSource`] walks the driver's stored `CrateHir`s — the
//! same store the type-hierarchy engine reads — and resolves names
//! through the crate's own lowering interner. A future source over
//! MIR locals or runtime frames would slot in behind the same seam
//! without touching the policy.
//!
//! What the range decides, over the recorded facts: every binding whose
//! identifier extent strictly overlaps the requested range answers one
//! variable-lookup fact at that extent, ordered by (span start) so a
//! debugger frame shows them top-down. The honest skips: a binding the
//! source records with a zero-width extent overlaps no range and answers
//! nothing, a binding whose name is empty names no variable, and a range
//! over pure expression lines answers `null` — never a guessed value.
//! Unlike the inlay-hint policy, annotated bindings answer too: a
//! debugger may look up a variable regardless of whether its type was
//! written in the source, and the request carries no type payload. The
//! params' `context.stopped_location` is deliberately not consulted —
//! the spec computes values for `params.range`, and the server has no
//! frame store to refine the context against; refining it would be a
//! guess.
//!
//! Wire mapping (LSP spec): each fact becomes an `InlineValueVariableLookup`
//! with `case_sensitive_lookup` unset (the spec's default, `true` — glyim
//! identifiers are case-sensitive) and the range mapped from the recorded
//! `pat_span` through the same `SourceMap` the request offsets mapped
//! from, so the extent and the name can never disagree. Responses are
//! ordered by span start, and an empty walk answers `null`, per the
//! goto-implementation convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_hir::{CrateHir, Expr, Pat};
use glyim_span::Span;
use lsp_types::{InlineValue, InlineValueParams, InlineValueVariableLookup};
use url::Url;

/// One `let` binding the request can answer with, wire-type-free: the
/// bound name and the identifier's byte-exact extent (the HIR's
/// `pat_span` — the same spans rename edits consume).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedBinding {
    /// The bound identifier's name.
    pub name: String,
    /// The bound identifier's extent, in the file the HIR was lowered
    /// for.
    pub span: Span,
}

/// One variable-lookup fact the requested range answers with,
/// wire-type-free: the name and the extent the wire lookup maps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineValueFact {
    /// The variable the debug adapter should look up.
    pub name: String,
    /// The identifier extent the lookup renders at.
    pub span: Span,
}

/// The inline-value fact seam (LSP-INLINE-VALUE).
///
/// The policy in [`compute_inline_values`] must stay testable without a
/// live `AnalysisDatabase` (HIR arenas, interners, locks), so it reads
/// the recorded bindings through this trait. The production
/// [`HirInlineValueSource`] walks the driver's stored `CrateHir`s; a
/// future MIR-local or runtime-frame source would slot in behind the
/// same seam without touching the policy.
pub trait InlineValueSource {
    /// Every recorded `let` binding of one file, in the order the source
    /// walks it; the policy applies its own overlap, ordering, and skip
    /// rules on top.
    fn recorded_bindings(&self) -> Vec<RecordedBinding>;
}

/// Production [`InlineValueSource`]: walks one file's stored `CrateHir`
/// for its `let` expressions and resolves each pattern's bound name
/// through the crate's own lowering interner.
///
/// Borrowing (not cloning) keeps the walk allocation-free apart from the
/// returned vector; the `db.hirs` read guard lives in the caller,
/// exactly like the type-hierarchy handler.
pub struct HirInlineValueSource<'a> {
    hir: &'a CrateHir,
}

impl<'a> HirInlineValueSource<'a> {
    /// Bind the source to one file's HIR for one request.
    #[must_use]
    pub fn new(hir: &'a CrateHir) -> Self {
        Self { hir }
    }
}

impl InlineValueSource for HirInlineValueSource<'_> {
    fn recorded_bindings(&self) -> Vec<RecordedBinding> {
        let mut out = Vec::new();
        for (_body_id, body) in self.hir.bodies.iter_enumerated() {
            for (_expr_id, expr) in body.exprs.iter_enumerated() {
                let Expr::Let { pat, pat_span, .. } = expr else {
                    continue;
                };
                // The bound identifier's name lives in the pattern; the
                // `pat_span` is already exactly that identifier's extent.
                let Pat::Binding { name, .. } = &body.pats[*pat] else {
                    continue;
                };
                out.push(RecordedBinding {
                    name: self.hir.interner.resolve(*name).to_string(),
                    span: *pat_span,
                });
            }
        }
        out
    }
}

/// The inline-value policy: one variable-lookup fact per recorded
/// binding whose identifier extent strictly overlaps `[lo, hi)`, ordered
/// by (span start, span end).
///
/// A zero-width recorded extent (nothing to render a lookup at — the
/// same rule the other extent-consuming engines apply), an empty name
/// (names no variable), and duplicate recordings (same name at the same
/// extent) are skipped/collapsed honestly; a range that overlaps nothing
/// answers an empty walk.
#[must_use]
pub fn compute_inline_values(
    source: &dyn InlineValueSource,
    lo: usize,
    hi: usize,
) -> Vec<InlineValueFact> {
    let mut facts: Vec<InlineValueFact> = source
        .recorded_bindings()
        .into_iter()
        .filter(|binding| !binding.name.is_empty())
        // A zero-width recorded extent (nothing to render a lookup at)
        // answers nothing, the same rule the other extent-consuming
        // engines apply — even though it would arithmetically overlap a
        // containing range.
        .filter(|binding| binding.span.lo() != binding.span.hi())
        .filter(|binding| binding.span.lo().to_usize() < hi && lo < binding.span.hi().to_usize())
        .map(|binding| InlineValueFact {
            name: binding.name,
            span: binding.span,
        })
        .collect();
    facts.sort_by_key(|fact| (fact.span.lo().to_usize(), fact.span.hi().to_usize()));
    facts.dedup();
    facts
}

/// Map one fact to the wire variable lookup, with the span→position
/// mapping injected (`None` when the fact's file is gone from the
/// session — one unmappable fact must not erase the good ones, and the
/// caller filters `None`s).
#[must_use]
pub fn wire_inline_value(
    fact: &InlineValueFact,
    map: &dyn Fn(usize, usize) -> Option<((usize, usize), (usize, usize))>,
) -> Option<InlineValue> {
    let ((start_line, start_col), (end_line, end_col)) =
        map(fact.span.lo().to_usize(), fact.span.hi().to_usize())?;
    Some(InlineValue::VariableLookup(InlineValueVariableLookup {
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
        // lsp-types 0.97 carries the wire field as an optional string;
        // the fact's name is the whole payload, so it always carries
        // `Some` — the policy skips empty names before the mapping.
        variable_name: Some(fact.name.clone()),
        // The spec's default is `true`, and glyim identifiers are
        // case-sensitive — the explicit `true` is the honest value.
        case_sensitive_lookup: true,
    }))
}

/// `textDocument/inlineValue` request entry point (LSP-INLINE-VALUE).
///
/// Resolution order mirrors the other position requests: URI → open-file
/// lookup → range offsets (byte-safe, INF-12) → the recorded-binding
/// walk over the stored HIR → the wire lookups. Returns `None` when the
/// document is not open or the range overlaps no recorded binding;
/// callers answer `null` for that. The params' `context` is deliberately
/// not consulted (see the module docs).
#[must_use]
pub fn inline_values(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &InlineValueParams,
) -> Option<Vec<InlineValue>> {
    let path = Url::parse(params.text_document.uri.as_str())
        .ok()?
        .to_file_path()
        .ok()?;
    let file_id = file_map.get_by_path(&path)?;

    // The request range's byte extents (INF-11: the same UTF-16 mapping
    // the responses map back through, so the offsets and the wire ranges
    // can never disagree).
    let (lo, hi) = {
        let source_maps = db.source_maps.read();
        let sm = source_maps.get(&file_id)?;
        let lo = sm.line_col_to_offset(
            params.range.start.line as usize,
            params.range.start.character as usize,
        )?;
        let hi = sm.line_col_to_offset(
            params.range.end.line as usize,
            params.range.end.character as usize,
        )?;
        drop(source_maps);
        (lo, hi)
    };

    let hirs = db.hirs.read();
    let hir = hirs.get(&file_id)?;
    let source = HirInlineValueSource::new(hir);
    let facts = compute_inline_values(&source, lo, hi.max(lo));
    drop(hirs);

    if facts.is_empty() {
        return None;
    }

    // Map every fact through the same source map the request offsets
    // came from; a fact whose extent no longer maps is skipped, not a
    // guessed position.
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let items: Vec<InlineValue> = facts
        .iter()
        .filter_map(|fact| wire_inline_value(fact, &|lo, hi| sm.span_to_position(lo, hi)))
        .collect();
    drop(source_maps);
    if items.is_empty() {
        return None;
    }
    tracing::debug!(file = %file_id.to_raw(), values = items.len(), "inline values resolved");
    Some(items)
}
