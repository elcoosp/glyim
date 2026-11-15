//! Signature-help request support (feature tag LSP-SIGNATURE-HELP).
//!
//! The server's hover and completions already render function signatures
//! from the symbol index's `TypeSignature` (Tier 6.4), and the reference
//! graph records a `ReferenceKind::Call` occurrence for every name
//! invoked with `(` — but no `textDocument/signatureHelp` handler
//! existed, so an editor typing `add(` got no parameter hints. This
//! module closes that capability gap on top of the two existing stores,
//! with no re-analysis on the request path.
//!
//! The seams: *which call is the cursor inside* is split from the graph
//! by [`CallSiteSource`], and *what signature the callee has* is split
//! from the index by [`SignatureSource`] — the same one-trait-per-store
//! shape LSP-HIGHLIGHT (`OccurrenceSource`) and LSP-TYPE-DEF
//! (`TypeSymbolSource`) use. The production impls borrow the driver's
//! shared [`ReferenceGraph`] and [`SymbolIndex`]; the policy tests drive
//! in-memory doubles, no database required (`tests/signature_help.rs`).
//!
//! Wire mapping (LSP spec): the innermost recorded call containing the
//! cursor answers one [`SignatureInformation`] rendered `fn name(p0: T0,
//! p1: T1) -> R` from the index's `TypeSignature` (the same shape
//! hover renders), with per-parameter `ParameterInformation` labels —
//! the server carries exactly one candidate, so `active_signature` is
//! always `0`. `active_parameter` counts the top-level commas between
//! the call's `(` and the cursor (nested calls sit at depth > 0 and do
//! not count; the innermost-hit rule routes those requests to the nested
//! callee anyway), clamped to the recorded arity so an over-called site
//! still highlights a real parameter, and `None` for a zero-parameter
//! signature where there is nothing to highlight. A cursor outside
//! every recorded call, and a callee the index does not record (a
//! function-typed local, an unresolved name), answer `null` honestly.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use crate::reference_graph::{ReferenceGraph, ReferenceKind};
use crate::symbol_index::SymbolIndex;
use glyim_span::{FileId, Span};
use lsp_types::{
    ParameterInformation, ParameterLabel, SignatureHelp, SignatureHelpParams, SignatureInformation,
};
use url::Url;

/// The call-site lookup seam for signature-help requests
/// (LSP-SIGNATURE-HELP).
///
/// The active-parameter policy must stay testable without a live
/// `AnalysisDatabase`, so it reads recorded calls through this trait.
/// The production impl borrows the shared [`ReferenceGraph`]; a future
/// span-indexed call cache would slot in behind the same seam without
/// touching the policy.
pub trait CallSiteSource {
    /// The innermost recorded call in `file_id` whose span contains
    /// `offset`, if any. Innermost, because signature help is about the
    /// call the cursor's *argument* belongs to: inside `outer(inner(x))`
    /// the request must resolve to `inner`.
    fn call_site_at(&self, file_id: FileId, offset: usize) -> Option<CalledFunction>;
}

/// Production [`CallSiteSource`]: borrows the driver's shared
/// [`ReferenceGraph`].
pub struct ReferenceGraphCallSiteSource<'a> {
    graph: &'a ReferenceGraph,
}

impl<'a> ReferenceGraphCallSiteSource<'a> {
    /// Borrows `graph` as the call-site source for one request.
    #[must_use]
    pub fn new(graph: &'a ReferenceGraph) -> Self {
        Self { graph }
    }
}

impl CallSiteSource for ReferenceGraphCallSiteSource<'_> {
    fn call_site_at(&self, file_id: FileId, offset: usize) -> Option<CalledFunction> {
        self.graph
            .iter_with_names()
            .filter(|(_, r)| r.kind == ReferenceKind::Call && !r.span.is_dummy())
            .filter(|(_, r)| {
                r.file_id == file_id
                    && r.span.lo.to_usize() <= offset
                    && offset < r.span.hi.to_usize()
            })
            .min_by_key(|(_, r)| r.span.hi.to_usize() - r.span.lo.to_usize())
            .map(|(name, r)| CalledFunction {
                callee: name.to_string(),
                span: r.span,
            })
    }
}

/// One recorded call site the cursor sits inside, wire-type-free: the
/// callee name (the graph is name-keyed) and the whole call
/// expression's extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalledFunction {
    /// The name the callee was recorded under.
    pub callee: String,
    /// The call expression's extent (the whole `callee(args)` form).
    pub span: Span,
}

/// The signature lookup seam for signature-help requests
/// (LSP-SIGNATURE-HELP).
///
/// The production impl borrows the shared [`SymbolIndex`]; a future
/// signature cache or doc-augmented provider would slot in behind the
/// same seam without touching the policy.
pub trait SignatureSource {
    /// The rendered signature recorded for `callee`, if the source knows
    /// one. When several recorded symbols share the name (cross-file
    /// same-name functions), the first recorded one wins — the same
    /// tie-break goto-definition applies.
    fn signature_of(&self, callee: &str) -> Option<SignatureData>;
}

/// Production [`SignatureSource`]: borrows the driver's shared
/// [`SymbolIndex`].
pub struct SignatureIndexSource<'a> {
    index: &'a SymbolIndex,
}

impl<'a> SignatureIndexSource<'a> {
    /// Borrows `index` as the signature source for one request.
    #[must_use]
    pub fn new(index: &'a SymbolIndex) -> Self {
        Self { index }
    }
}

impl SignatureSource for SignatureIndexSource<'_> {
    fn signature_of(&self, callee: &str) -> Option<SignatureData> {
        // Only function-kind symbols carry call signatures: a same-named
        // struct is not what `callee(args)` invoked.
        self.index
            .lookup_by_name(callee)
            .iter()
            .find(|sym| {
                matches!(sym.kind, crate::symbol_index::SymbolKind::Function)
                    && sym.type_signature.is_some()
            })
            .map(|sym| {
                render_signature(
                    &sym.name,
                    sym.type_signature
                        .as_ref()
                        .expect("checked by the find above"),
                )
            })
    }
}

/// A rendered signature, wire-type-free: the display label, the
/// per-parameter labels, and the parameter count the active-parameter
/// policy clamps against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureData {
    /// The rendered `fn name(p0: T0, p1: T1) -> R` label.
    pub label: String,
    /// The per-parameter labels (`a: i32`), in declaration order.
    pub parameters: Vec<String>,
}

/// Render a symbol index `TypeSignature` (plus its symbol's name, which
/// the signature table does not re-store) into the `fn`-label shape
/// hover and completions already use: generic parameters (when
/// recorded), `name: Type` parameters, and the return type.
fn render_signature(name: &str, sig: &crate::symbol_index::TypeSignature) -> SignatureData {
    let generics = if sig.generic_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", sig.generic_params.join(", "))
    };
    let parameters = sig
        .params
        .iter()
        .map(|(param, ty)| format!("{param}: {ty}"))
        .collect::<Vec<_>>();
    let ret = sig
        .return_type
        .as_ref()
        .map(|ret| format!(" -> {ret}"))
        .unwrap_or_default();
    let params_joined = parameters.join(", ");
    let label = format!("fn {name}{generics}({params_joined}){ret}");
    SignatureData { label, parameters }
}

/// The policy result, wire-type-free: everything the wire layer needs to
/// emit one `SignatureHelp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignatureHelpPlan {
    /// The rendered `fn` label of the resolved callee.
    pub label: String,
    /// The per-parameter labels, in declaration order.
    pub parameters: Vec<String>,
    /// The parameter the cursor's position activates (`None` for a
    /// zero-parameter signature).
    pub active_parameter: Option<u32>,
}

/// Signature-help policy: the plan for the call under the cursor.
///
/// Resolution: innermost recorded call containing the cursor → the
/// callee's recorded signature → the active parameter counted from the
/// source text. Returns `None` when the cursor is outside every recorded
/// call or the callee has no recorded signature.
#[must_use]
pub fn compute_signature_help(
    call_sites: &dyn CallSiteSource,
    signatures: &dyn SignatureSource,
    source_text: &str,
    file_id: FileId,
    cursor_offset: usize,
) -> Option<SignatureHelpPlan> {
    let hit = call_sites.call_site_at(file_id, cursor_offset)?;
    let data = signatures.signature_of(&hit.callee)?;
    let active_parameter = active_parameter_at(source_text, &hit.span, cursor_offset, &data);
    Some(SignatureHelpPlan {
        label: data.label,
        parameters: data.parameters,
        active_parameter,
    })
}

/// Count the top-level commas between the call's `(` and the cursor,
/// clamp to the recorded arity, and map a zero-parameter signature to
/// `None`.
///
/// The scan is lexical: paren/bracket/brace depth decides which commas
/// belong to this call's argument list, so commas inside nested calls
/// (depth > 0) and grouped arguments (`(a, b), (c,)`) do not advance the
/// index. String literals containing commas are not skipped — the same
/// lexical honesty the reference graph's span records already have.
fn active_parameter_at(
    source_text: &str,
    call_span: &Span,
    cursor_offset: usize,
    data: &SignatureData,
) -> Option<u32> {
    if data.parameters.is_empty() {
        return None;
    }
    let lo = call_span.lo.to_usize();
    let hi = call_span.hi.to_usize();
    let text = source_text.get(lo..hi)?;
    // The callee name runs to the argument list's `(`; commas count from
    // there to the cursor (or the close paren, whichever comes first).
    let open = text.find('(')?;
    let args_start = lo + open + 1;
    let scan_end = cursor_offset.min(hi.saturating_sub(1)).max(args_start);
    let mut depth = 0usize;
    let mut commas = 0usize;
    for ch in source_text[args_start..scan_end].chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => commas += 1,
            _ => {}
        }
    }
    // Clamp: an over-called site (arity mismatch) still highlights a
    // real parameter instead of an index the signature does not have.
    let active = commas.min(data.parameters.len().saturating_sub(1));
    Some(active as u32)
}

/// `textDocument/signatureHelp` request entry point (LSP-SIGNATURE-HELP).
///
/// Resolution order mirrors `goto_definition`: URI → open-file lookup →
/// cursor offset (byte-safe, INF-12) → innermost call site through the
/// graph seam → callee signature through the index seam → active
/// parameter from the source text → wire shape. Returns `None` when the
/// document is not open, the cursor is outside every recorded call, or
/// the callee has no recorded signature; callers answer `null` for that.
pub fn signature_help(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &SignatureHelpParams,
) -> Option<SignatureHelp> {
    let uri = &params.text_document_position_params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;
    let pos = params.text_document_position_params.position;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let offset = sm.line_col_to_offset(pos.line as usize, pos.character as usize)?;
    let source_text = sm.source();

    let ref_graph = db.reference_graph.read();
    let call_sites = ReferenceGraphCallSiteSource::new(&ref_graph);
    let symbol_index = db.symbol_index.read();
    let signatures = SignatureIndexSource::new(&symbol_index);

    let plan = compute_signature_help(&call_sites, &signatures, source_text, file_id, offset)?;
    drop(symbol_index);
    drop(ref_graph);

    // Parameter labels as the plain-string form (the client resolves the
    // label text within the signature label); offsets would require
    // byte-exact label arithmetic for no capability gain.
    let parameters = if plan.parameters.is_empty() {
        None
    } else {
        Some(
            plan.parameters
                .iter()
                .map(|label| ParameterInformation {
                    label: ParameterLabel::Simple(label.clone()),
                    documentation: None,
                })
                .collect::<Vec<_>>(),
        )
    };
    tracing::debug!(
        label = %plan.label,
        active = ?plan.active_parameter,
        "signature help computed"
    );
    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: plan.label,
            documentation: None,
            parameters,
            // The help-level `active_parameter` below is the single
            // source of truth; the per-signature form stays unset.
            active_parameter: None,
        }],
        active_signature: Some(0),
        active_parameter: plan.active_parameter,
    })
}
