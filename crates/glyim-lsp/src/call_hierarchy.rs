//! Call-hierarchy request support (feature tag LSP-CALL-HIERARCHY).
//!
//! The reference graph records a `ReferenceKind::Call` occurrence for
//! every name invoked with `(` (free calls and method receivers alike),
//! each carrying the `ItemId` of the item that owns the call site — but
//! nothing answered the three `callHierarchy/*` requests: an editor could
//! not open "Show Call Hierarchy" on a function and walk its callers or
//! its callees. This module closes that capability gap on top of the
//! existing store, with no re-analysis on the request path.
//!
//! The seam: *where call edges and item definitions come from* is split
//! from *how a hierarchy is built* by [`CallGraphSource`]. The production
//! [`ReferenceGraphCallSource`] borrows the driver's shared [`ReferenceGraph`]
//! — the same authoritative store rename, find-references, and
//! document-highlight read — while the policy tests drive an in-memory
//! source, no database required (`tests/call_hierarchy.rs`). A future
//! owner-keyed call-site index would slot in behind the same seam without
//! touching the policy, exactly like LSP-HIGHLIGHT's `OccurrenceSource`.
//!
//! Wire mapping (LSP spec): `prepareCallHierarchy` answers the one-item
//! list for the function under the cursor — the cursor may sit on the
//! function's definition site or on a call site of it (the callee wins in
//! the second case, so invoking the hierarchy on a call walks the callee).
//! `incomingCalls`/`outgoingCalls` re-resolve the *echoed* item by its
//! name + definition extent (clients echo it back verbatim), group the
//! recorded call edges by the owner/callee item, and answer call-site
//! ranges as `from_ranges`. The graph records the whole `fn` item as the
//! definition extent — the same extent goto-definition answers with — so
//! `range` and `selection_range` coincide. Names that resolve to no
//! function item (function-typed locals, method receivers not in the fn
//! table) drop out of the responses instead of fabricating hierarchy
//! items, and a fabricated echoed item answers `null`.

use crate::AnalysisDatabase;
use crate::database::{FileMap, SourceMap};
use crate::navigation::identifier_at_offset;
use crate::reference_graph::{ReferenceGraph, ReferenceKind};
use glyim_span::{FileId, Span};
use lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyIncomingCallsParams, CallHierarchyItem,
    CallHierarchyOutgoingCall, CallHierarchyOutgoingCallsParams, CallHierarchyPrepareParams, Range,
    SymbolKind,
};
use std::collections::BTreeMap;
use url::Url;

/// The call-edge and item-definition lookup seam for call-hierarchy
/// requests (LSP-CALL-HIERARCHY).
///
/// The hierarchy policy in [`prepare_target`], [`compute_incoming_calls`],
/// and [`compute_outgoing_calls`] must stay testable without a live
/// `AnalysisDatabase` (locks, source maps, file ids), so it reads through
/// this trait. The production impl borrows the shared [`ReferenceGraph`].
pub trait CallGraphSource {
    /// The item-level definition of the function named `name`, if the
    /// source knows one (the production source only answers for names in
    /// the graph's fn table). When several recorded sites share the name
    /// (cross-file same-name functions), the first recorded one wins —
    /// the same tie-break goto-definition applies.
    fn function_definition(&self, name: &str) -> Option<ItemDefinition>;

    /// The item-level function definition owned by HIR item
    /// `owner_item_id` — the caller-side join: a call edge names its
    /// owner, and the owner's definition site is what the wire response
    /// shows.
    fn item_definition_by_owner(&self, owner_item_id: u32) -> Option<ItemDefinition>;

    /// The item-level function definition recorded in `file_id` covering
    /// the byte extent `lo..hi()` — the echo-resolution half of the seam:
    /// clients answer `incomingCalls` with the item `prepare` handed
    /// them, and the exact definition extent is what makes the echoed
    /// item resolvable without trusting the client.
    fn item_definition_at(&self, file_id: FileId, lo: usize, hi: usize) -> Option<ItemDefinition>;

    /// Every recorded call site whose callee is named `callee` (incoming
    /// edges). Order is not significant; the policy groups and sorts.
    fn incoming_call_sites(&self, callee: &str) -> Vec<CallSite>;

    /// Every recorded call site owned by HIR item `owner_item_id`
    /// (outgoing edges). Order is not significant; the policy groups and
    /// sorts.
    fn outgoing_call_sites(&self, owner_item_id: u32) -> Vec<CallSite>;
}

/// Production [`CallGraphSource`]: borrows the driver's shared
/// [`ReferenceGraph`].
///
/// Borrowing (not cloning) keeps the request allocation-free apart from
/// the returned vectors; the guard lives in the caller, exactly like the
/// `find_references`/`rename` handlers.
pub struct ReferenceGraphCallSource<'a> {
    graph: &'a ReferenceGraph,
}

impl<'a> ReferenceGraphCallSource<'a> {
    /// Borrows `graph` as the call-graph source for one request.
    #[must_use]
    pub fn new(graph: &'a ReferenceGraph) -> Self {
        Self { graph }
    }

    /// The item-level definition recorded for `name`'s references, gated
    /// on function-ness: struct/enum items are recorded in the same
    /// store with item-level definitions, and answering a call-hierarchy
    /// item for them would be a lie.
    fn function_definition_impl(&self, name: &str) -> Option<ItemDefinition> {
        if !self.graph.is_function_name(name) {
            return None;
        }
        self.graph
            .find_references(name)
            .iter()
            .find(|r| r.is_definition && r.is_item_level && !r.span.is_dummy())
            .and_then(|r| {
                Some(ItemDefinition {
                    name: name.to_string(),
                    owner_item_id: r.owner_item_id?,
                    file_id: r.file_id,
                    span: r.span,
                })
            })
    }
}

impl CallGraphSource for ReferenceGraphCallSource<'_> {
    fn function_definition(&self, name: &str) -> Option<ItemDefinition> {
        self.function_definition_impl(name)
    }

    fn item_definition_by_owner(&self, owner_item_id: u32) -> Option<ItemDefinition> {
        self.graph
            .iter_with_names()
            .find(|(_, r)| {
                r.is_definition
                    && r.is_item_level
                    && !r.span.is_dummy()
                    && r.owner_item_id == Some(owner_item_id)
            })
            .and_then(|(name, r)| {
                Some(ItemDefinition {
                    name: name.to_string(),
                    owner_item_id: r.owner_item_id?,
                    file_id: r.file_id,
                    span: r.span,
                })
            })
    }

    fn item_definition_at(&self, file_id: FileId, lo: usize, hi: usize) -> Option<ItemDefinition> {
        self.graph
            .iter_with_names()
            .find(|(_, r)| {
                r.is_definition
                    && r.is_item_level
                    && !r.span.is_dummy()
                    && r.file_id == file_id
                    && r.span.lo().to_usize() == lo
                    && r.span.hi().to_usize() == hi
            })
            .and_then(|(name, r)| {
                Some(ItemDefinition {
                    name: name.to_string(),
                    owner_item_id: r.owner_item_id?,
                    file_id: r.file_id,
                    span: r.span,
                })
            })
    }

    fn incoming_call_sites(&self, callee: &str) -> Vec<CallSite> {
        self.graph
            .find_references(callee)
            .iter()
            .filter(|r| r.kind == ReferenceKind::Call)
            .filter(|r| !r.span.is_dummy())
            .filter_map(|r| {
                Some(CallSite {
                    callee: callee.to_string(),
                    owner_item_id: r.owner_item_id?,
                    file_id: r.file_id,
                    span: r.span,
                })
            })
            .collect()
    }

    fn outgoing_call_sites(&self, owner_item_id: u32) -> Vec<CallSite> {
        self.graph
            .iter_with_names()
            .filter(|(_, r)| {
                r.kind == ReferenceKind::Call
                    && !r.span.is_dummy()
                    && r.owner_item_id == Some(owner_item_id)
            })
            .filter_map(|(name, r)| {
                Some(CallSite {
                    callee: name.to_string(),
                    owner_item_id: r.owner_item_id?,
                    file_id: r.file_id,
                    span: r.span,
                })
            })
            .collect()
    }
}

/// One recorded call edge, wire-type-free: the call expression's extent
/// in `file_id`, the HIR item that owns it, and the name the callee was
/// recorded under. Carries the raw [`glyim_span::Span`] so the policy
/// tests assert on byte offsets; the wire conversion maps it to an LSP
/// [`Range`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The name the callee was recorded under (the graph is name-keyed).
    pub callee: String,
    /// The HIR item whose body contains the call (`owner_item_id` of the
    /// recorded reference).
    pub owner_item_id: u32,
    /// The file the call site lives in.
    pub file_id: FileId,
    /// The call expression's extent (the whole `callee(args)` form).
    pub span: Span,
}

/// A function item's definition site, wire-type-free: the join key for
/// the hierarchy (`owner_item_id`), the display name, and the recorded
/// extent (the whole `fn` item, the same one goto-definition answers
/// with).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemDefinition {
    /// The function's name.
    pub name: String,
    /// The HIR `ItemId` of the function item (raw form).
    pub owner_item_id: u32,
    /// The file the item is defined in.
    pub file_id: FileId,
    /// The item's recorded extent.
    pub span: Span,
}

/// One incoming call, wire-type-free: the caller's item plus every call
/// site in it that targets the queried function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingCall {
    /// The function that makes the call.
    pub caller: ItemDefinition,
    /// The call-expression extents, sorted by byte offset.
    pub call_sites: Vec<Span>,
}

/// One outgoing call, wire-type-free: the callee's item plus every call
/// site in the queried function that targets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingCall {
    /// The function being called.
    pub callee: ItemDefinition,
    /// The call-expression extents, sorted by byte offset.
    pub call_sites: Vec<Span>,
}

/// Prepare policy: the call-hierarchy item for `identifier` under the
/// cursor in `file_id`.
///
/// Resolution order: (1) if the cursor sits inside a recorded *definition*
/// of `identifier` in this file and `identifier` is a function item, that
/// item is the target; (2) otherwise, if the cursor sits inside a recorded
/// *call site* of `identifier`, the callee's item is the target — invoking
/// the hierarchy on a call walks the callee (the caller side is what the
/// incoming view already answers). Returns `None` when the name resolves
/// to no function item, when the definition is in another file, or when
/// the cursor is off any recorded occurrence.
#[must_use]
pub fn prepare_target(
    source: &dyn CallGraphSource,
    file_id: FileId,
    cursor_offset: usize,
    identifier: &str,
) -> Option<ItemDefinition> {
    if let Some(def) = source.function_definition(identifier)
        && def.file_id == file_id
        && def.span.lo().to_usize() <= cursor_offset
        && cursor_offset < def.span.hi().to_usize()
    {
        return Some(def);
    }

    // Call-site rule: the cursor sits on a recorded call of `identifier`.
    // The callee must itself be a function item — `function_definition`
    // refuses names outside the fn table, so function-typed locals and
    // method receivers answer `None` honestly.
    let on_call_site = source.incoming_call_sites(identifier).iter().any(|s| {
        s.file_id == file_id
            && s.span.lo().to_usize() <= cursor_offset
            && cursor_offset < s.span.hi().to_usize()
    });
    if on_call_site {
        return source.function_definition(identifier);
    }
    None
}

/// Incoming-calls policy: the callers of `target`, grouped by caller item,
/// call sites sorted within each caller, callers sorted by name (then
/// owner item — the graph's recording order). Call edges whose owner
/// resolves to no item-level definition drop out (a stale edge after a
/// delete must not fabricate a caller); self edges are kept — recursion
/// is real.
#[must_use]
pub fn compute_incoming_calls(
    source: &dyn CallGraphSource,
    target: &ItemDefinition,
) -> Vec<IncomingCall> {
    let mut grouped: BTreeMap<(String, u32), (ItemDefinition, Vec<Span>)> = BTreeMap::new();
    for site in source.incoming_call_sites(&target.name) {
        let Some(caller) = source.item_definition_by_owner(site.owner_item_id) else {
            continue;
        };
        grouped
            .entry((caller.name.clone(), caller.owner_item_id))
            .or_insert_with(|| (caller, Vec::new()))
            .1
            .push(site.span);
    }
    grouped
        .into_values()
        .map(|(caller, mut sites)| {
            sites.sort_by_key(|s| (s.lo().to_usize(), s.hi().to_usize()));
            IncomingCall {
                caller,
                call_sites: sites,
            }
        })
        .collect()
}

/// Outgoing-calls policy: the callees of `target`, grouped by callee item,
/// call sites sorted within each callee, callees sorted by name (then
/// owner item — the graph's recording order). Call edges whose callee name
/// resolves to no function item (function-typed locals, method receivers
/// not in the fn table) drop out rather than fabricating a hierarchy item.
#[must_use]
pub fn compute_outgoing_calls(
    source: &dyn CallGraphSource,
    target: &ItemDefinition,
) -> Vec<OutgoingCall> {
    let mut grouped: BTreeMap<(String, u32), (ItemDefinition, Vec<Span>)> = BTreeMap::new();
    for site in source.outgoing_call_sites(target.owner_item_id) {
        let Some(callee) = source.function_definition(&site.callee) else {
            continue;
        };
        grouped
            .entry((callee.name.clone(), callee.owner_item_id))
            .or_insert_with(|| (callee, Vec::new()))
            .1
            .push(site.span);
    }
    grouped
        .into_values()
        .map(|(callee, mut sites)| {
            sites.sort_by_key(|s| (s.lo().to_usize(), s.hi().to_usize()));
            OutgoingCall {
                callee,
                call_sites: sites,
            }
        })
        .collect()
}

/// `textDocument/prepareCallHierarchy` request entry point
/// (LSP-CALL-HIERARCHY).
///
/// Resolution order mirrors `goto_definition`: URI → open-file lookup →
/// cursor offset (byte-safe, INF-12) → identifier text → [`prepare_target`]
/// policy → wire item. Returns `None` when the document is not open, the
/// cursor is off an identifier, or the identifier resolves to no function
/// item; callers answer `null` for that.
pub fn prepare_call_hierarchy(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CallHierarchyPrepareParams,
) -> Option<Vec<CallHierarchyItem>> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let pos = params.text_document_position_params.position;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;

    let identifier = identifier_at_offset(sm.source(), offset)?;

    let ref_graph = db.reference_graph.read();
    let source = ReferenceGraphCallSource::new(&ref_graph);
    let target = prepare_target(&source, file_id, offset, &identifier)?;
    tracing::debug!(name = %target.name, "call hierarchy prepared");

    Some(vec![item_to_wire(file_map, &source_maps, &target)?])
}

/// `callHierarchy/incomingCalls` request entry point (LSP-CALL-HIERARCHY).
///
/// The client echoes the item `prepare` handed it; the echoed item is
/// re-resolved by name + definition extent (`resolve_prepared_item`) — a
/// fabricated or stale item answers `null`, never a guessed hierarchy. A
/// caller whose file lost its `SourceMap` mid-request is skipped (the
/// T131 tolerance the highlight policy applies), not dropped wholesale.
pub fn incoming_calls(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CallHierarchyIncomingCallsParams,
) -> Option<Vec<CallHierarchyIncomingCall>> {
    let source_maps = db.source_maps.read();
    let target = resolve_prepared_item(db, file_map, &source_maps, &params.item)?;

    let ref_graph = db.reference_graph.read();
    let source = ReferenceGraphCallSource::new(&ref_graph);
    let calls = compute_incoming_calls(&source, &target);

    let wire = calls
        .into_iter()
        .filter_map(|call| {
            let from = item_to_wire(file_map, &source_maps, &call.caller)?;
            let from_ranges = call
                .call_sites
                .iter()
                .filter_map(|s| span_to_range(&source_maps, call.caller.file_id, *s))
                .collect::<Vec<_>>();
            if from_ranges.is_empty() {
                return None;
            }
            Some(CallHierarchyIncomingCall { from, from_ranges })
        })
        .collect::<Vec<_>>();
    tracing::debug!(callers = wire.len(), "incoming calls computed");
    Some(wire)
}

/// `callHierarchy/outgoingCalls` request entry point (LSP-CALL-HIERARCHY).
///
/// Same echo-resolution contract as [`incoming_calls`]; a resolved item
/// with no call edges answers an empty list — the honest "no callees" —
/// while `null` stays reserved for an unresolvable item.
pub fn outgoing_calls(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &CallHierarchyOutgoingCallsParams,
) -> Option<Vec<CallHierarchyOutgoingCall>> {
    let source_maps = db.source_maps.read();
    let target = resolve_prepared_item(db, file_map, &source_maps, &params.item)?;

    let ref_graph = db.reference_graph.read();
    let source = ReferenceGraphCallSource::new(&ref_graph);
    let calls = compute_outgoing_calls(&source, &target);

    let wire = calls
        .into_iter()
        .filter_map(|call| {
            let to = item_to_wire(file_map, &source_maps, &call.callee)?;
            let from_ranges = call
                .call_sites
                .iter()
                .filter_map(|s| span_to_range(&source_maps, target.file_id, *s))
                .collect::<Vec<_>>();
            if from_ranges.is_empty() {
                return None;
            }
            Some(CallHierarchyOutgoingCall { to, from_ranges })
        })
        .collect::<Vec<_>>();
    tracing::debug!(callees = wire.len(), "outgoing calls computed");
    Some(wire)
}

/// Re-resolve a client-echoed hierarchy item against the shared graph:
/// the item's URI must name an open file whose `SourceMap` can translate
/// the echoed `selection_range` back to the byte extent the graph
/// recorded for the item-level definition. Returns `None` for a
/// fabricated item — the requests never compute against a guess.
fn resolve_prepared_item(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    source_maps: &std::collections::HashMap<FileId, SourceMap>,
    item: &CallHierarchyItem,
) -> Option<ItemDefinition> {
    let path = Url::parse(item.uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let sm = source_maps.get(&file_id)?;
    let lo = sm.line_col_to_offset(
        item.selection_range.start.line as usize,
        item.selection_range.start.character as usize,
    )?;
    let hi = sm.line_col_to_offset(
        item.selection_range.end.line as usize,
        item.selection_range.end.character as usize,
    )?;

    let ref_graph = db.reference_graph.read();
    let source = ReferenceGraphCallSource::new(&ref_graph);
    source.item_definition_at(file_id, lo, hi)
}

/// Convert a policy-side definition into the wire `CallHierarchyItem`.
///
/// The kind is always `FUNCTION` (the seam only answers function items);
/// `range` and `selection_range` coincide because the graph records the
/// whole `fn` item as the definition extent — the same extent
/// goto-definition answers with (no finer name-token granularity exists
/// in the shared store). Returns `None` when the file's `SourceMap` is
/// gone (closed mid-request, the T131 tolerance: the individual item
/// drops out).
fn item_to_wire(
    file_map: &FileMap,
    source_maps: &std::collections::HashMap<FileId, SourceMap>,
    def: &ItemDefinition,
) -> Option<CallHierarchyItem> {
    let path = file_map.path(def.file_id)?;
    let uri = Url::from_file_path(path).ok()?;
    use std::str::FromStr;
    let uri = lsp_types::Uri::from_str(uri.as_str()).ok()?;
    let sm = source_maps.get(&def.file_id)?;
    let ((start_line, start_col), (end_line, end_col)) =
        sm.span_to_position(def.span.lo().to_usize(), def.span.hi().to_usize())?;
    let range = Range {
        start: lsp_types::Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: lsp_types::Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    };
    Some(CallHierarchyItem {
        name: def.name.clone(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri,
        range,
        selection_range: range,
        data: None,
    })
}

/// Map a raw span in `file_id` to a wire [`Range`], skipping spans whose
/// `SourceMap` disappeared mid-request (T131 tolerance).
fn span_to_range(
    source_maps: &std::collections::HashMap<FileId, SourceMap>,
    file_id: FileId,
    span: Span,
) -> Option<Range> {
    let sm = source_maps.get(&file_id)?;
    let ((start_line, start_col), (end_line, end_col)) =
        sm.span_to_position(span.lo().to_usize(), span.hi().to_usize())?;
    Some(Range {
        start: lsp_types::Position {
            line: start_line as u32,
            character: start_col as u32,
        },
        end: lsp_types::Position {
            line: end_line as u32,
            character: end_col as u32,
        },
    })
}
