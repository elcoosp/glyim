//! Type-hierarchy request support (feature tag LSP-TYPE-HIERARCHY).
//!
//! The server answers "what shape carries it" (goto-type-definition) and
//! "what implements it" (goto-implementation), but nothing answered the
//! *hierarchy* questions an editor's type-hierarchy view asks:
//! `textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, and
//! `typeHierarchy/subtypes` did not exist, even though every input they
//! need was already in the database. The HIR records every top-level
//! trait/struct/enum item with its byte-exact span, and every `impl` block
//! with its trait path and self type; only the wiring was missing. This
//! module closes that capability gap with no re-analysis on the request
//! path.
//!
//! The seam: *which recorded items and impl blocks count as hierarchy
//! members* is split from *how those facts are found* by
//! [`TypeHierarchySource`], mirroring LSP-IMPLEMENTATION's `ImplSource`
//! and LSP-CALL-HIERARCHY's `CallGraphSource`. The policy
//! ([`hierarchy_target`], [`compute_supertypes`], [`compute_subtypes`])
//! reads through the trait and stays testable without a HIR arena or
//! locks (`tests/type_hierarchy.rs`); the production
//! [`HirTypeHierarchySource`] borrows the driver's stored `CrateHir`s —
//! the same store the goto-implementation engine reads — and resolves
//! names through each crate's own lowering interner. A future
//! cross-crate or supertrait-aware index would slot in behind the same
//! seam without touching the policy.
//!
//! What the hierarchy is, over the recorded facts: the cursor's
//! identifier prepares a hierarchy item only when it names a top-level
//! trait, struct, or enum (functions, impls, locals, and misses answer
//! `null` — a function is call-hierarchy territory, not a type-hierarchy
//! root). Supertypes of a *type* are the traits implemented for it — one
//! member per trait impl site (inherent impls record no trait and are
//! not supertypes). Subtypes of a *trait* are the impl self types — one
//! member per impl site of the trait. The two honest blanks: a trait has
//! no recorded supertraits (the HIR does not carry a supertrait store,
//! so supertypes of a trait answer `null` rather than a guess), and a
//! struct/enum has no recorded sub-hierarchy (subtypes of a type answer
//! `null`). Zero-width extents are skipped (the same rule
//! [`crate::implementation::implementation_targets`] applies): an extent
//! that cannot name a jump target must not be answered as one.
//!
//! Wire mapping (LSP spec): `prepare` answers the one-item list for the
//! item under the cursor, `range` and `selection_range` both the whole
//! item extent (the same pin call-hierarchy makes — the recorded extent
//! is the jump target, not a guessed name slice). `supertypes` items are
//! `Interface`-kind at the impl block's extent; `subtypes` items carry
//! the self type's recorded classification (struct/enum/trait) or the
//! honest `Class` fallback for non-path self types that name nothing
//! recorded. The `supertypes`/`subtypes` echo is re-joined against the
//! recorded facts exactly like the LSP-CALL-HIERARCHY and code-lens
//! resolves: the echoed item's URI names the document, its
//! `selection_range` start maps back to the byte offset through the same
//! `SourceMap` the prepare mapped it from, and it must match a live
//! recorded item — a stale or fabricated echo answers `null`, never a
//! guessed hierarchy. Responses are ordered by (file id, span start) so
//! the answer is deterministic regardless of the database's per-file map
//! iteration order, and an empty walk answers `null`, per the
//! goto-implementation convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::implementation::{ImplSite, last_segment_name, type_ref_last_name};
use crate::navigation::identifier_at_offset;
use crate::symbol_index::DefinitionLocation;
use glyim_hir::{CrateHir, ItemKind};
use glyim_span::FileId;
use lsp_types::{
    SymbolKind, TypeHierarchyItem, TypeHierarchyPrepareParams, TypeHierarchySubtypesParams,
    TypeHierarchySupertypesParams,
};
use std::str::FromStr;
use url::Url;

/// The recorded classification of a top-level item the type hierarchy
/// can start from, wire-type-free: the trait/struct/enum distinction
/// decides both what prepares and which wire symbol kind the item maps
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HierarchyItemKind {
    /// A `trait` item — implemented-for facts (supertypes of its
    /// implementors) hang off it, and its own supertypes are not
    /// recorded (no supertrait store in the HIR).
    Trait,
    /// A `struct` item — its supertypes are the traits implemented for
    /// it.
    Struct,
    /// An `enum` item — same recorded facts as a struct.
    Enum,
}

/// One named top-level item the hierarchy can start from, wire-type-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedItem {
    /// The item's own name (the HIR record's interner-resolved name).
    pub name: String,
    /// The recorded classification (trait / struct / enum).
    pub kind: HierarchyItemKind,
    /// The whole item's extent, in the file the HIR was lowered for.
    pub location: DefinitionLocation,
}

/// One hierarchy member the supertypes/subtypes walk answers with,
/// wire-type-free: the name the wire item carries and the impl block
/// extent that anchors it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyMember {
    /// The member's own name — the trait's name for a supertype member,
    /// the self type's name for a subtype member.
    pub name: String,
    /// The whole `impl ... { ... }` block extent the member anchors at.
    pub location: DefinitionLocation,
}

/// The type-hierarchy fact seam (LSP-TYPE-HIERARCHY).
///
/// The policies in [`hierarchy_target`], [`compute_supertypes`], and
/// [`compute_subtypes`] must stay testable without a live
/// `AnalysisDatabase` (HIR arenas, interners, locks), so they read the
/// recorded facts through this trait. The production
/// [`HirTypeHierarchySource`] walks the driver's stored `CrateHir`s; a
/// future cross-crate or supertrait-aware index would slot in behind the
/// same seam without touching the policies.
pub trait TypeHierarchySource {
    /// The recorded top-level trait/struct/enum item named `name`, if any.
    /// When several recorded items share the name (cross-file same-name
    /// types), the first record in (file id, span start) order wins — a
    /// deterministic tie-break over the per-file map's iteration order.
    fn named_item(&self, name: &str) -> Option<NamedItem>;

    /// Every recorded trait impl site whose self type is `type_name` —
    /// the type's supertype facts. Inherent impls are not included (they
    /// record no trait); order is not significant, the policy sorts.
    fn impl_sites_for_type(&self, type_name: &str) -> Vec<ImplSite>;

    /// Every recorded impl site implementing `trait_name` — the trait's
    /// subtype facts. Order is not significant, the policy sorts.
    fn impl_sites_of_trait(&self, trait_name: &str) -> Vec<ImplSite>;
}

/// Production [`TypeHierarchySource`]: borrows the driver's stored
/// `CrateHir`s — one per analysed file — and resolves names through each
/// crate's own lowering interner (`CrateHir::interner`), so no
/// shared-interner assumption can drift between files lowered at
/// different times.
///
/// Borrowing (not cloning) keeps the walk allocation-free apart from the
/// returned vectors; the `db.hirs` read guard lives in the caller,
/// exactly like the goto-implementation handler.
pub struct HirTypeHierarchySource<'a> {
    files: Vec<(FileId, &'a CrateHir)>,
}

impl<'a> HirTypeHierarchySource<'a> {
    /// Bind the source to the session's `(file id, HIR)` pairs for one
    /// request.
    #[must_use]
    pub fn new(files: Vec<(FileId, &'a CrateHir)>) -> Self {
        Self { files }
    }

    /// The classification of one HIR item, or `None` for the kinds that
    /// are not type-hierarchy roots (functions, impls, aliases, consts,
    /// modules, uses).
    fn item_kind(kind: &ItemKind) -> Option<HierarchyItemKind> {
        match kind {
            ItemKind::Trait(_) => Some(HierarchyItemKind::Trait),
            ItemKind::Struct(_) => Some(HierarchyItemKind::Struct),
            ItemKind::Enum(_) => Some(HierarchyItemKind::Enum),
            _ => None,
        }
    }

    /// Every recorded impl block across the session's files, in the
    /// source's walk order — the shared fact walk both impl queries
    /// filter down from (the same shape [`crate::implementation`]'s
    /// production source walks).
    fn all_impl_sites(&self) -> Vec<ImplSite> {
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

impl TypeHierarchySource for HirTypeHierarchySource<'_> {
    fn named_item(&self, name: &str) -> Option<NamedItem> {
        // First record in (file id, span start) order: the per-file map's
        // iteration order is not deterministic, and the tie-break must be.
        self.files
            .iter()
            .flat_map(|(file_id, hir)| {
                hir.items.iter().filter_map(move |item| {
                    let kind = Self::item_kind(&item.kind)?;
                    let item_name = hir.interner.resolve(item.name).to_string();
                    (item_name == name).then_some(NamedItem {
                        name: item_name,
                        kind,
                        location: DefinitionLocation {
                            file_id: *file_id,
                            span: item.span,
                        },
                    })
                })
            })
            .min_by_key(|item| {
                (
                    item.location.file_id.to_raw(),
                    item.location.span.lo.to_usize(),
                )
            })
    }

    fn impl_sites_for_type(&self, type_name: &str) -> Vec<ImplSite> {
        self.all_impl_sites()
            .into_iter()
            .filter(|site| {
                site.self_ty_name.as_deref() == Some(type_name) && site.trait_name.is_some()
            })
            .collect()
    }

    fn impl_sites_of_trait(&self, trait_name: &str) -> Vec<ImplSite> {
        self.all_impl_sites()
            .into_iter()
            .filter(|site| site.trait_name.as_deref() == Some(trait_name))
            .collect()
    }
}

/// Prepare choice: the recorded trait/struct/enum item under the cursor's
/// identifier, or `None` when nothing qualifies.
///
/// A zero-width recorded extent is skipped (an extent that cannot name a
/// jump target must not be answered as one); a name the source records no
/// trait/struct/enum item for — a function, a local, or a total miss —
/// prepares nothing.
#[must_use]
pub fn hierarchy_target(source: &dyn TypeHierarchySource, name: &str) -> Option<NamedItem> {
    let item = source.named_item(name)?;
    if item.location.span.lo == item.location.span.hi {
        return None;
    }
    Some(item)
}

/// Supertypes policy: one member per trait impl site of the named type,
/// ordered by (file id, span start).
///
/// Inherent impls are excluded (they record no trait, so they are not
/// supertypes), sites whose trait path or extent names nothing usable
/// are skipped (a blank name or zero-width extent cannot be shown as a
/// jump target), and duplicates (same trait name at the same extent) are
/// collapsed.
#[must_use]
pub fn compute_supertypes(
    source: &dyn TypeHierarchySource,
    type_name: &str,
) -> Vec<HierarchyMember> {
    let mut members: Vec<HierarchyMember> = Vec::new();
    for site in source.impl_sites_for_type(type_name) {
        let Some(trait_name) = site.trait_name else {
            continue;
        };
        if trait_name.is_empty() || site.location.span.lo == site.location.span.hi {
            continue;
        }
        let member = HierarchyMember {
            name: trait_name,
            location: site.location,
        };
        if members.contains(&member) {
            continue;
        }
        members.push(member);
    }
    members.sort_by_key(|member| {
        (
            member.location.file_id.to_raw(),
            member.location.span.lo.to_usize(),
            member.location.span.hi.to_usize(),
        )
    });
    members
}

/// Subtypes policy: one member per impl self type of the named trait,
/// ordered by (file id, span start).
///
/// Sites whose self type names nothing usable are skipped (non-path self
/// types — slices, tuples, pointers — and zero-width extents cannot be
/// shown as jump targets), and duplicates (same self type at the same
/// extent) are collapsed.
#[must_use]
pub fn compute_subtypes(
    source: &dyn TypeHierarchySource,
    trait_name: &str,
) -> Vec<HierarchyMember> {
    let mut members: Vec<HierarchyMember> = Vec::new();
    for site in source.impl_sites_of_trait(trait_name) {
        let Some(self_ty_name) = site.self_ty_name else {
            continue;
        };
        if self_ty_name.is_empty() || site.location.span.lo == site.location.span.hi {
            continue;
        }
        let member = HierarchyMember {
            name: self_ty_name,
            location: site.location,
        };
        if members.contains(&member) {
            continue;
        }
        members.push(member);
    }
    members.sort_by_key(|member| {
        (
            member.location.file_id.to_raw(),
            member.location.span.lo.to_usize(),
            member.location.span.hi.to_usize(),
        )
    });
    members
}

/// The wire symbol kind a member maps to, resolved from the recorded
/// classification where one exists and the honest `Class` fallback where
/// none does (a non-path self type names nothing recorded).
fn member_symbol_kind(source: &dyn TypeHierarchySource, name: &str) -> SymbolKind {
    match source.named_item(name).map(|item| item.kind) {
        Some(HierarchyItemKind::Trait) => SymbolKind::INTERFACE,
        Some(HierarchyItemKind::Struct) => SymbolKind::STRUCT,
        Some(HierarchyItemKind::Enum) => SymbolKind::ENUM,
        None => SymbolKind::CLASS,
    }
}

/// Map one recorded location to the wire item the hierarchy answers
/// with. Returns `None` when the location's file is gone from the
/// session (each member maps independently — one unmappable member must
/// not erase the good ones).
fn member_wire_item(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    name: &str,
    kind: SymbolKind,
    location: &DefinitionLocation,
) -> Option<TypeHierarchyItem> {
    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&location.file_id)?;
    let ((start_line, start_col), (end_line, end_col)) =
        sm.span_to_position(location.span.lo.to_usize(), location.span.hi.to_usize())?;
    drop(source_maps);
    let target_path = file_map.path(location.file_id)?;
    let target_uri = Url::from_file_path(target_path).ok()?;
    let Ok(uri) = lsp_types::Uri::from_str(target_uri.as_str()) else {
        return None;
    };
    let range = lsp_types::Range {
        start: lsp_types::Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: lsp_types::Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    };
    Some(TypeHierarchyItem {
        name: name.to_string(),
        kind,
        tags: None,
        detail: None,
        uri,
        range,
        // The whole recorded extent is the jump target — the same pin
        // call-hierarchy's items make.
        selection_range: range,
        data: None,
    })
}

/// `textDocument/prepareTypeHierarchy` request entry point
/// (LSP-TYPE-HIERARCHY).
///
/// Resolution order mirrors `goto_implementation`: URI → open-file lookup
/// → cursor offset (byte-safe, INF-12) → identifier extraction →
/// recorded-item choice over *every* stored HIR (a type is named in one
/// file but can be implemented anywhere the driver analysed) → the
/// one-item wire list. Returns `None` when the document is not open, the
/// cursor carries no identifier, or the identifier names no top-level
/// trait/struct/enum item; callers answer `null` for that.
#[must_use]
pub fn prepare_type_hierarchy(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &TypeHierarchyPrepareParams,
) -> Option<Vec<TypeHierarchyItem>> {
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
    // The whole session's item facts, not just the cursor's file.
    let files: Vec<(FileId, &CrateHir)> =
        hirs.iter().map(|(file_id, hir)| (*file_id, hir)).collect();
    let target = hierarchy_target(&HirTypeHierarchySource::new(files), &symbol_name)?;
    drop(hirs);

    let item = member_wire_item(
        db,
        file_map,
        &target.name,
        match target.kind {
            HierarchyItemKind::Trait => SymbolKind::INTERFACE,
            HierarchyItemKind::Struct => SymbolKind::STRUCT,
            HierarchyItemKind::Enum => SymbolKind::ENUM,
        },
        &target.location,
    )?;
    tracing::debug!(name = %target.name, "type hierarchy prepared");
    Some(vec![item])
}

/// Re-join the echoed hierarchy item against the recorded facts (the
/// LSP-CALL-HIERARCHY echo discipline): the echo's URI and
/// `selection_range` start must resolve to the same recorded item prepare
/// answered with — the exact definition extent is what makes the echoed
/// item resolvable without trusting the client.
fn echo_target(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    item: &TypeHierarchyItem,
) -> Option<NamedItem> {
    let path = Url::parse(item.uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let lo = {
        let source_maps = db.source_maps.read();
        let sm = source_maps.get(&file_id)?;
        sm.line_col_to_offset(
            item.selection_range.start.line as usize,
            item.selection_range.start.character as usize,
        )?
    };

    let hirs = db.hirs.read();
    let files: Vec<(FileId, &CrateHir)> =
        hirs.iter().map(|(file_id, hir)| (*file_id, hir)).collect();
    let target = hierarchy_target(&HirTypeHierarchySource::new(files), &item.name)?;
    // The echo must name the same recorded item: same file, same start.
    (target.location.file_id == file_id && target.location.span.lo.to_usize() == lo)
        .then_some(target)
}

/// `typeHierarchy/supertypes` request entry point (LSP-TYPE-HIERARCHY).
///
/// The echoed item is re-joined against the recorded facts, then the
/// walk follows the echo's kind: a struct/enum answers one item per
/// trait impl site; a trait answers `null` (no supertrait facts are
/// recorded — nothing is guessed); a fabricated echo answers `null`.
#[must_use]
pub fn type_supertypes(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &TypeHierarchySupertypesParams,
) -> Option<Vec<TypeHierarchyItem>> {
    let target = echo_target(db, file_map, &params.item)?;
    if matches!(target.kind, HierarchyItemKind::Trait) {
        return None;
    }

    let hirs = db.hirs.read();
    let files: Vec<(FileId, &CrateHir)> =
        hirs.iter().map(|(file_id, hir)| (*file_id, hir)).collect();
    let source = HirTypeHierarchySource::new(files);
    let members = compute_supertypes(&source, &target.name);
    if members.is_empty() {
        return None;
    }
    let items = members
        .iter()
        .filter_map(|member| {
            member_wire_item(
                db,
                file_map,
                &member.name,
                SymbolKind::INTERFACE,
                &member.location,
            )
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    tracing::debug!(name = %target.name, supers = items.len(), "type supertypes resolved");
    Some(items)
}

/// `typeHierarchy/subtypes` request entry point (LSP-TYPE-HIERARCHY).
///
/// The echoed item is re-joined against the recorded facts, then the
/// walk follows the echo's kind: a trait answers one item per impl self
/// type (the self type's recorded classification maps to the wire kind);
/// a struct/enum answers `null` (types have no recorded sub-hierarchy);
/// a fabricated echo answers `null`.
#[must_use]
pub fn type_subtypes(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &TypeHierarchySubtypesParams,
) -> Option<Vec<TypeHierarchyItem>> {
    let target = echo_target(db, file_map, &params.item)?;
    if !matches!(target.kind, HierarchyItemKind::Trait) {
        return None;
    }

    let hirs = db.hirs.read();
    let files: Vec<(FileId, &CrateHir)> =
        hirs.iter().map(|(file_id, hir)| (*file_id, hir)).collect();
    let source = HirTypeHierarchySource::new(files);
    let members = compute_subtypes(&source, &target.name);
    if members.is_empty() {
        return None;
    }
    let items = members
        .iter()
        .filter_map(|member| {
            let kind = member_symbol_kind(&source, &member.name);
            member_wire_item(db, file_map, &member.name, kind, &member.location)
        })
        .collect::<Vec<_>>();
    if items.is_empty() {
        return None;
    }
    tracing::debug!(name = %target.name, subs = items.len(), "type subtypes resolved");
    Some(items)
}
