//! Goto-implementation request support (feature tag LSP-IMPLEMENTATION).
//!
//! The server answers "where is this name defined" (goto-definition), "what
//! declares it for this file" (goto-declaration), and "what shape carries
//! it" (goto-type-definition) — but nothing answered the fourth navigation
//! question, the one editors bind to "go to implementation":
//! `textDocument/implementation` did not exist, even though every input it
//! needs was already in the database. The HIR records every `impl` block
//! with its trait path (`impl Greet for P`) and its self type, and each
//! item carries a byte-exact span; only the wiring was missing. This
//! module closes that capability gap.
//!
//! The seam: *which recorded impl blocks count as implementations of the
//! cursor's symbol* is split from *how the impl blocks are found* by
//! [`ImplSource`], mirroring LSP-DECLARATION's `DeclarationSource` and
//! LSP-TYPE-DEF's `TypeSymbolSource`. The policy ([`implementation_targets`])
//! reads through the trait and stays testable without a HIR arena or locks
//! (`tests/implementation.rs`); the production [`HirImplSource`] borrows
//! the driver's stored `CrateHir`s — the same store the inlay-hint engine
//! reads — and resolves names through each crate's own lowering interner.
//! A future cross-crate impl index would slot in behind the same seam
//! without touching the policy.
//!
//! What counts as an implementation: the cursor's identifier matches the
//! *last path segment* of an impl's trait reference (cursor on a trait
//! symbol → its impl blocks) or of an impl's self type (cursor on a type
//! symbol → the trait and inherent impls implemented for it). Method and
//! local names match nothing — answering a method's enclosing impl would
//! turn the request into a second goto-definition, so those cursors
//! answer `null` honestly. Zero-width spans are skipped (the same rule
//! `declaration_target` applies): an extent that cannot name a jump
//! target must not be answered as one.
//!
//! Wire mapping (LSP spec): the response is an *array* of `Location`s —
//! a symbol can have several impls — ordered by (file id, span start) so
//! the answer is deterministic regardless of the database's per-file map
//! iteration order. Unmappable spans are dropped (each target is
//! independent; one bad span must not erase the good ones), an all-dropped
//! or empty answer is `null`, per the other engines' convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::navigation::identifier_at_offset;
use crate::symbol_index::DefinitionLocation;
use glyim_hir::{CrateHir, ItemKind, Path, TypeRef};
use glyim_span::FileId;
use lsp_types::GotoDefinitionResponse;
use lsp_types::request::GotoImplementationParams;
use url::Url;

/// One recorded impl block, wire-type-free: the names the choice policy
/// matches against and the byte-exact location of the block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplSite {
    /// Last path segment of the impl's trait reference — `Some("Greet")`
    /// for `impl Greet for P`. Inherent impls (`impl P`) have none.
    pub trait_name: Option<String>,
    /// Last path segment of the impl's self type — `Some("P")` for
    /// `impl Greet for P`. Non-path self types (slices, tuples, pointers)
    /// have no single user-written name and carry none.
    pub self_ty_name: Option<String>,
    /// The whole `impl ... { ... }` block extent, in the file the HIR was
    /// lowered for.
    pub location: DefinitionLocation,
}

/// The impl-site seam for goto-implementation requests
/// (LSP-IMPLEMENTATION).
///
/// The choice policy in [`implementation_targets`] must stay testable
/// without a live `AnalysisDatabase` (HIR arenas, locks, source maps), so
/// it reads recorded impl blocks through this trait. The production
/// [`HirImplSource`] walks the driver's stored `CrateHir`s; a future
/// cross-crate or workspace-wide impl index would slot in behind the same
/// seam without touching the policy.
pub trait ImplSource {
    /// Every recorded impl block across the session's files, in the
    /// source's walk order. The policy applies its own ordering and
    /// filtering rules on top.
    fn impl_sites(&self) -> Vec<ImplSite>;
}

/// Production [`ImplSource`]: borrows the driver's stored `CrateHir`s —
/// one per analysed file — and resolves names through each crate's own
/// lowering interner (`CrateHir::interner`), so no shared-interner
/// assumption can drift between files lowered at different times.
///
/// Borrowing (not cloning) keeps the walk allocation-free apart from the
/// returned vector; the `db.hirs` read guard lives in the caller, exactly
/// like the inlay-hint handler.
pub struct HirImplSource<'a> {
    files: Vec<(FileId, &'a CrateHir)>,
}

impl<'a> HirImplSource<'a> {
    /// Bind the source to the session's `(file id, HIR)` pairs for one
    /// request.
    #[must_use]
    pub fn new(files: Vec<(FileId, &'a CrateHir)>) -> Self {
        Self { files }
    }
}

impl ImplSource for HirImplSource<'_> {
    fn impl_sites(&self) -> Vec<ImplSite> {
        let mut out = Vec::new();
        for (file_id, hir) in &self.files {
            for item in hir.items.iter() {
                let ItemKind::Impl(impl_item) = &item.kind else {
                    continue;
                };
                out.push(ImplSite {
                    trait_name: impl_item
                        .trait_ref
                        .as_ref()
                        .and_then(|path| last_segment_name(path, &hir.interner)),
                    self_ty_name: type_ref_last_name(&impl_item.self_ty, &hir.interner),
                    location: DefinitionLocation {
                        file_id: *file_id,
                        span: item.span,
                    },
                });
            }
        }
        out
    }
}

/// The last path segment's resolved name, if the path carries one.
///
/// Empty paths (a parse-recovery shape the lowering can produce) name
/// nothing.
fn last_segment_name(path: &Path, interner: &glyim_core::Interner) -> Option<String> {
    path.segments
        .last()
        .map(|segment| interner.resolve(segment.name).to_string())
}

/// Extract the single user-written name a `TypeRef` spells as a path, if
/// any. `impl Greet for P` spells both sides as paths; slices, tuples,
/// and pointers have no single name to match a cursor against, so they
/// answer `None` (the impl site stays recorded, but no symbol claims it).
fn type_ref_last_name(ty: &TypeRef, interner: &glyim_core::Interner) -> Option<String> {
    match ty {
        TypeRef::Path(path) => last_segment_name(path, interner),
        _ => None,
    }
}

/// Goto-implementation choice: every impl block recorded for `name`.
///
/// A site qualifies when its trait name **or** its self-type name equals
/// the cursor's identifier (a site where the two names coincide — an
/// `impl Greet for Greet` shape — matches both filters and answers once:
/// a duplicated location would render as two identical highlights).
/// Zero-width extents are skipped, the same rule
/// [`crate::goto_declaration::declaration_target`] applies. The result is
/// ordered by (file id, span start): the production source walks a
/// per-file map whose iteration order is not deterministic, and the wire
/// order must be.
#[must_use]
pub fn implementation_targets(source: &dyn ImplSource, name: &str) -> Vec<DefinitionLocation> {
    let mut kept: Vec<DefinitionLocation> = Vec::new();
    for site in source.impl_sites() {
        let matches =
            site.trait_name.as_deref() == Some(name) || site.self_ty_name.as_deref() == Some(name);
        if !matches {
            continue;
        }
        if site.location.span.lo == site.location.span.hi {
            continue;
        }
        if kept.contains(&site.location) {
            continue;
        }
        kept.push(site.location);
    }
    kept.sort_by_key(|location| {
        (
            location.file_id.to_raw(),
            location.span.lo.to_usize(),
            location.span.hi.to_usize(),
        )
    });
    kept
}

/// `textDocument/implementation` request entry point
/// (LSP-IMPLEMENTATION).
///
/// Resolution order mirrors `goto_declaration`: URI → open-file lookup →
/// cursor offset (byte-safe, INF-12) → identifier extraction → choice
/// through the shared seam over *every* stored HIR (implementations live
/// in other files too) → wire locations. Returns `None` when the document
/// is not open, the cursor carries no identifier, or no impl block names
/// the symbol; callers answer `null` for that.
#[must_use]
pub fn goto_implementation(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &GotoImplementationParams,
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
    let symbol_name = identifier_at_offset(source, offset)?;

    let hirs = db.hirs.read();
    // The whole session's impl facts, not just the cursor's file: a trait
    // declared here can be implemented anywhere the driver analysed.
    let files: Vec<(FileId, &CrateHir)> =
        hirs.iter().map(|(file_id, hir)| (*file_id, hir)).collect();
    let targets = implementation_targets(&HirImplSource::new(files), &symbol_name);
    if targets.is_empty() {
        return None;
    }

    let mut locations = Vec::with_capacity(targets.len());
    for target in &targets {
        // Each target maps independently: one unmappable span (a span in
        // a file whose map is gone) must not erase the good answers, so
        // it is dropped instead of failing the whole request.
        let Some(target_sm) = source_maps.get(&target.file_id) else {
            continue;
        };
        let Some(((start_line, start_col), (end_line, end_col))) =
            target_sm.span_to_position(target.span.lo.to_usize(), target.span.hi.to_usize())
        else {
            continue;
        };
        let Some(target_path) = file_map.path(target.file_id) else {
            continue;
        };
        let Some(target_uri) = Url::from_file_path(target_path).ok() else {
            continue;
        };
        use std::str::FromStr;
        let Ok(uri) = lsp_types::Uri::from_str(target_uri.as_str()) else {
            continue;
        };
        locations.push(lsp_types::Location {
            uri,
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
        });
    }
    if locations.is_empty() {
        return None;
    }
    tracing::debug!(
        name = %symbol_name,
        impls = locations.len(),
        "goto implementation resolved"
    );
    Some(GotoDefinitionResponse::Array(locations))
}
