//! Type-definition request support (feature tag LSP-TYPE-DEF).
//!
//! The server resolves goto-definition for the *name* under the cursor,
//! but not for the *shape*: hovering `p.y` and asking "where is the type
//! of this defined?" had no answer — the request
//! `textDocument/typeDefinition` did not exist, even though every input
//! it needs was already in the database. The type checker (Tier 6.4)
//! records each expression's resolved `Ty`, and the `TyCtx` can name the
//! ADT behind it; only the wiring was missing. This module closes that
//! capability gap.
//!
//! The seam: *which recorded symbol counts as the type's definition* is
//! split from *how the type name is found* by [`TypeSymbolSource`],
//! mirroring LSP-HIGHLIGHT's `OccurrenceSource`. The policy
//! ([`type_target`]) reads through the trait and stays testable without
//! a `TyCtx`, arena, or locks (`tests/type_definition.rs`); the
//! production [`SymbolIndexSource`] borrows the shared `SymbolIndex` —
//! the same authoritative store goto-definition and completions read.
//! Turning an expression's `Ty` into its type name is the production
//! helper [`named_type_of`] (peels `&T`/`*T`/`[T]`/`[T; N]` wrappers down
//! to the ADT and asks the `TyCtx` for its name); the type arena makes
//! mock-`TyKind` construction heavier than the choice logic it feeds, so
//! the peel is covered end-to-end instead (`tests/type_definition_tests.rs`).
//!
//! Wire mapping (LSP spec): an ADT-typed cursor answers a scalar
//! `Location` at the struct/enum definition site, byte-exact like
//! goto-definition (LSP-EXACT-EXTENTS). Primitives, tuples, function
//! pointers, and un-inferred types answer `null` — they have no
//! user-defined definition site — and so does a cursor the type checker
//! never reached (broken code above the cursor).

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::symbol_index::{DefinitionLocation, SymbolIndex, SymbolKind};
use glyim_type::{Ty, TyCtx, TyKind};
use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse};
use url::Url;

/// The type-symbol lookup seam for type-definition requests
/// (LSP-TYPE-DEF).
///
/// The choice policy in [`type_target`] must stay testable without a
/// live `AnalysisDatabase` (locks, indexes, source maps), so it reads
/// recorded symbols through this trait. The production impl borrows the
/// shared [`SymbolIndex`]; a future cross-file type index would slot in
/// behind the same seam without touching the policy.
pub trait TypeSymbolSource {
    /// Every recorded symbol named `type_name`, with its kind and
    /// definition site. Order is the index's recording order (the
    /// driver's analysis order); the policy applies its own preference
    /// rules on top.
    fn definitions_of(&self, type_name: &str) -> Vec<(SymbolKind, DefinitionLocation)>;
}

/// Production [`TypeSymbolSource`]: borrows the driver's shared
/// [`SymbolIndex`].
///
/// Borrowing (not cloning) keeps the request allocation-free apart from
/// the returned vector; the guard lives in the caller, exactly like the
/// goto-definition handler.
pub struct SymbolIndexSource<'a> {
    index: &'a SymbolIndex,
}

impl<'a> SymbolIndexSource<'a> {
    /// Borrows `index` as the type-symbol source for one request.
    #[must_use]
    pub fn new(index: &'a SymbolIndex) -> Self {
        Self { index }
    }
}

impl TypeSymbolSource for SymbolIndexSource<'_> {
    fn definitions_of(&self, type_name: &str) -> Vec<(SymbolKind, DefinitionLocation)> {
        self.index
            .lookup_by_name(type_name)
            .iter()
            .map(|sym| (sym.kind, sym.definition.clone()))
            .collect()
    }
}

/// Type-definition choice: the definition site of the struct or enum
/// recorded under `type_name`.
///
/// Only type-kind symbols qualify ([`SymbolKind::Struct`],
/// [`SymbolKind::Enum`]): a same-named function or local is *not* the
/// shape the cursor carries, and answering with it would be a lie. When
/// several type sites share the name (cross-file same-name ADTs), the
/// first recorded one wins — the index order is the driver's analysis
/// order, the same tie-break goto-definition applies. `None` when no
/// type is recorded under the name.
#[must_use]
pub fn type_target(source: &dyn TypeSymbolSource, type_name: &str) -> Option<DefinitionLocation> {
    source
        .definitions_of(type_name)
        .into_iter()
        .find(|(kind, _)| matches!(kind, SymbolKind::Struct | SymbolKind::Enum))
        .map(|(_, definition)| definition)
}

/// Peel an expression's resolved type down to its user-defined shape
/// name, if any.
///
/// References, raw pointers, slices, and arrays are transparent (the
/// shape behind `&Point` is `Point`); primitives, tuples, function
/// pointers, inference vars, and error types name nothing — a
/// `typeDefinition` request on them answers `null`, honestly, rather
/// than guessing a same-named symbol. The arena-backed `TyKind`s cannot
/// be constructed standalone, so this helper is exercised end-to-end
/// (`tests/type_definition_tests.rs`), while the *choice* it feeds is
/// unit-tested through the seam.
#[must_use]
pub fn named_type_of(ty_ctx: &TyCtx, ty: Ty) -> Option<String> {
    let kind = ty_ctx.ty_kind(ty);
    let adt_id = match kind {
        TyKind::Adt(adt_id, _) => *adt_id,
        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
            return named_type_of(ty_ctx, *inner);
        }
        TyKind::Slice(inner) | TyKind::Array(inner, _) => {
            return named_type_of(ty_ctx, *inner);
        }
        _ => return None,
    };
    let name = ty_ctx.adt_name_for_id(adt_id)?;
    Some(ty_ctx.resolver().resolve(name).to_string())
}

/// `textDocument/typeDefinition` request entry point (LSP-TYPE-DEF).
///
/// Resolution order mirrors `goto_definition`: URI → open-file lookup →
/// cursor offset (byte-safe, INF-12) → the type checker's expression
/// type at the offset (the Tier 6.4 database seam) → ADT name → symbol
/// choice through the shared seam → wire location. Returns `None` when
/// the document is not open, the cursor has no recorded expression type,
/// the type has no user-defined shape, or the shape is not in the symbol
/// index; callers answer `null` for that.
#[must_use]
pub fn type_definition(
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

    let ty = db.type_at_offset(file_id, offset)?;
    let ty_ctx = db.ty_ctx(file_id)?;
    let type_name = named_type_of(&ty_ctx, ty)?;

    let symbol_index = db.symbol_index.read();
    let target = type_target(&SymbolIndexSource::new(&symbol_index), &type_name)?;
    drop(symbol_index);

    let def = &target;
    let def_sm = source_maps.get(&def.file_id)?;
    // Exact token extents (LSP-EXACT-EXTENTS): the same two-tuple
    // mapping goto-definition answers with. An unmappable span answers
    // `null` (via `?`) rather than the old silent line-0/col-0 fallback,
    // which would aim every client at the top of the target file.
    let ((start_line, start_col), (end_line, end_col)) =
        def_sm.span_to_position(def.span.lo().to_usize(), def.span.hi().to_usize())?;
    let target_path = file_map.path(def.file_id)?;
    let target_uri = Url::from_file_path(target_path).ok()?;
    tracing::debug!(
        type_name = %type_name,
        "type definition resolved"
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
