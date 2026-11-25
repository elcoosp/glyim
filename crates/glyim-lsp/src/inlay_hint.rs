//! Inlay-hint request support (feature tag LSP-INLAY-HINT).
//!
//! The server already names types in hover and resolves shapes in
//! type-definition, but an editor *reading* code had no inline answer:
//! `let total = price * count;` shows its inferred type only if the
//! reader hovers the binding, and `textDocument/inlayHint` was not
//! implemented at all. This module closes that capability gap on top of
//! the state the analysis driver already stores (the Tier 6.4 HIR +
//! typeck results the same way LSP-TYPE-DEF reads them), so a hint is a
//! cheap walk of recorded facts — no re-analysis on the request path.
//!
//! The seam: *which bindings are hintable and what their types render
//! as* is split from the database by [`BindingTypeSource`], mirroring
//! LSP-TYPE-DEF's `TypeSymbolSource`. The policy ([`compute_inlay_hints`])
//! reads pre-resolved facts and stays testable without a HIR arena,
//! typeck map, or lock (`tests/inlay_hint.rs`); the production
//! [`HirBindingTypeSource`] walks the driver's stored `CrateHir` +
//! `TypeckResult` and renders types through [`render_type_label`]. A
//! future cross-file or macro-expanded source would slot in behind the
//! same seam without touching the policy.
//!
//! What gets a hint: `let` bindings **without** a written annotation
//! (an annotated `let x: i32 = ..;` already says its type — restating
//! it inline is noise, so the HIR's `ty: Option<TypeRef>` field is the
//! filter). The hint attaches right after the binding name (the HIR
//! records exactly that span for rename, INF-13) and renders `: Ty`.
//!
//! Label honesty: [`render_type_label`] renders only shapes it can
//! name as a user would write them — primitives, `&T`/`&mut T`,
//! `*const T`/`*mut T`, `[T]`/`[T; _]`, tuples, function pointers, and
//! ADTs with resolved names (recursively, so `&Point` and
//! `Vec<Point>` render fully). Inference variables, error types,
//! closures, opaque/projection types, and anonymous bounds have no
//! stable user-facing spelling — they answer `None` and the binding is
//! skipped, degrading to no hint rather than a wrong one. This is the
//! same honesty rule LSP-SEMANTIC-TOKENS applies to unclassifiable
//! tokens.
//!
//! Wire mapping (LSP spec): one `InlayHint` per binding — position at
//! the attachment offset (UTF-16 columns, INF-11), a string label
//! `: Ty`, kind `Type`, and no text edits (display-only: the server
//! never proposes an annotation the user did not ask for). A binding
//! whose attachment falls outside the requested screen range is
//! filtered, matching the range-shape behavior of semantic tokens.
//! Documents that are not open, have no recorded typeck results, or
//! yield no hints answer `null`, per the other engines' convention.

use crate::AnalysisDatabase;
use crate::database::FileMap;
use glyim_core::primitives::Mutability;
use glyim_hir::Expr;
use glyim_type::{GenericArg, Substitution, Ty, TyCtx, TyKind};
use glyim_typeck::TypeckResult;
use lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, InlayHintParams};
use url::Url;

/// One hintable binding, wire-type-free: the byte offset the hint
/// attaches after (the end of the binding name) and the rendered type
/// label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnannotatedBinding {
    /// Byte offset one past the last byte of the binding name.
    pub after: usize,
    /// The user-facing type spelling (`Point`, `&Point`, `Vec<i32>`).
    pub type_label: String,
}

/// The binding-type seam for inlay-hint requests (LSP-INLAY-HINT).
///
/// The hint policy in [`compute_inlay_hints`] must stay testable
/// without a live `AnalysisDatabase` (HIR arenas, typeck maps, locks),
/// so it reads recorded bindings through this trait. The production
/// [`HirBindingTypeSource`] walks the driver's stored HIR and typeck
/// results; a future macro-expanded or cross-file source would slot in
/// behind the same seam without touching the policy.
pub trait BindingTypeSource {
    /// Every un-annotated binding of one file, in the order the source
    /// walks it. The policy applies its own normalization on top.
    fn unannotated_bindings(&self) -> Vec<UnannotatedBinding>;
}

/// Production [`BindingTypeSource`]: walks one file's stored HIR and
/// typeck results, borrowing the shared `TyCtx` for name resolution.
///
/// Borrowing (not cloning) keeps the walk allocation-free apart from
/// the returned vector; the guards live in the caller, exactly like
/// the goto-definition handler. Body ids translate through
/// [`TypeckResult::body_owner`] (TCK-SEAM: the HIR and typeck
/// `LocalDefId` counters are independent).
pub struct HirBindingTypeSource<'a> {
    hir: &'a glyim_hir::CrateHir,
    typeck: &'a TypeckResult,
    ty_ctx: &'a TyCtx,
}

impl<'a> HirBindingTypeSource<'a> {
    /// Borrows one file's analysis state as the binding source for one
    /// request.
    #[must_use]
    pub fn new(hir: &'a glyim_hir::CrateHir, typeck: &'a TypeckResult, ty_ctx: &'a TyCtx) -> Self {
        Self {
            hir,
            typeck,
            ty_ctx,
        }
    }
}

impl BindingTypeSource for HirBindingTypeSource<'_> {
    fn unannotated_bindings(&self) -> Vec<UnannotatedBinding> {
        let mut out = Vec::new();
        for (body_id, body) in self.hir.bodies.iter_enumerated() {
            let Some(owner) = self.typeck.body_owner(body_id) else {
                continue;
            };
            for (_expr_id, expr) in body.exprs.iter_enumerated() {
                let Expr::Let {
                    pat_span,
                    ty,
                    value,
                    ..
                } = expr
                else {
                    continue;
                };
                // An annotated binding already carries its type in the
                // source; restating it inline is noise, not information.
                if ty.is_some() {
                    continue;
                }
                let Some(init_ty) = self.typeck.expr_ty(owner, value.to_raw() as usize) else {
                    continue;
                };
                let Some(type_label) = render_type_label(self.ty_ctx, init_ty) else {
                    continue;
                };
                out.push(UnannotatedBinding {
                    after: pat_span.hi().to_usize(),
                    type_label,
                });
            }
        }
        // A flat walk over `exprs` yields declaration order per body but
        // body order is the HIR's, not the file's; sort so the policy's
        // monotonic invariant holds for the wire layer.
        out.sort_by_key(|binding| binding.after);
        out
    }
}

/// Inlay-hint policy over a whole file: filter the source's bindings
/// through the normalization invariants and keep the output strictly
/// monotonic.
///
/// Normalization (the part tests pin): empty labels are dropped (an
/// empty `: ` hint is wire-visible garbage), a binding that rewinds
/// before the previous kept attachment is dropped (a sort bug upstream
/// must degrade to a missing hint, never a scrambled hint order — the
/// same failure class [`crate::semantic_tokens::classify_tokens`]
/// chose), and duplicate attachment offsets keep the first (one hint
/// per site).
#[must_use]
pub fn compute_inlay_hints(source: &dyn BindingTypeSource) -> Vec<UnannotatedBinding> {
    let mut kept: Vec<UnannotatedBinding> = Vec::new();
    for binding in source.unannotated_bindings() {
        if binding.type_label.is_empty() {
            continue;
        }
        // Strictly monotonic in attachments: rewinds and same-site
        // duplicates are dropped; the first binding at offset 0 is kept
        // (the empty `kept` guard makes the first hint exempt from the
        // rewind check).
        if kept.last().is_some_and(|prev| binding.after <= prev.after) {
            continue;
        }
        kept.push(binding);
    }
    kept
}

/// Render a resolved type as a user would write it, or `None` when the
/// type has no stable user-facing spelling.
///
/// References, raw pointers, slices, arrays, tuples, and function
/// pointers render recursively (the shape behind `&Point` is spelled
/// `&Point`); ADTs resolve their name through the same `TyCtx` →
/// interner path goto-definition's [`crate::type_definition::named_type_of`]
/// uses, with generic arguments rendered in angle brackets. Primitives
/// take their surface names. Inference variables, error types,
/// closures, `dyn Trait`, opaque (`impl Trait`) and projection types,
/// and anonymous bound vars are skipped — the request degrades to no
/// hint rather than a `?ty3`-style compiler-internal spelling leaking
/// into an editor.
#[must_use]
pub fn render_type_label(ty_ctx: &TyCtx, ty: Ty) -> Option<String> {
    let kind = ty_ctx.ty_kind(ty);
    match kind {
        TyKind::Bool => Some("bool".to_string()),
        TyKind::Char => Some("char".to_string()),
        TyKind::String => Some("str".to_string()),
        TyKind::Never => Some("!".to_string()),
        TyKind::Unit => Some("()".to_string()),
        TyKind::Int(i) => Some(i.name().to_string()),
        TyKind::Uint(u) => Some(u.name().to_string()),
        TyKind::Float(fl) => Some(fl.name().to_string()),
        TyKind::Ref(_, inner, mutability) => {
            let label = render_type_label(ty_ctx, *inner)?;
            Some(match mutability {
                Mutability::Mut => format!("&mut {label}"),
                Mutability::Not => format!("&{label}"),
            })
        }
        TyKind::RawPtr(inner, mutability) => {
            let label = render_type_label(ty_ctx, *inner)?;
            Some(match mutability {
                Mutability::Mut => format!("*mut {label}"),
                Mutability::Not => format!("*const {label}"),
            })
        }
        TyKind::Slice(inner) => {
            let label = render_type_label(ty_ctx, *inner)?;
            Some(format!("[{label}]"))
        }
        TyKind::Array(inner, _) => {
            // The array length is a `Const` whose surface spelling the
            // frozen context does not render; `[T; _]` mirrors
            // `glyim_type`'s `PrintTy` honesty.
            let label = render_type_label(ty_ctx, *inner)?;
            Some(format!("[{label}; _]"))
        }
        TyKind::Tuple(substs) => {
            let args: Vec<String> = substitution_labels(ty_ctx, *substs)?;
            Some(format!("({})", args.join(", ")))
        }
        TyKind::FnPtr(sig) => {
            let inputs: Vec<String> = substitution_labels(ty_ctx, sig.inputs)?;
            let output = render_type_label(ty_ctx, sig.output)?;
            Some(format!("fn({}) -> {output}", inputs.join(", ")))
        }
        TyKind::Adt(adt_id, substs) => {
            let name = ty_ctx.adt_name_for_id(*adt_id)?;
            let name = ty_ctx.resolver().resolve(name).to_string();
            if substitution_args_is_empty(ty_ctx, *substs) {
                Some(name)
            } else {
                let args: Vec<String> = substitution_labels(ty_ctx, *substs)?;
                Some(format!("{name}<{}>", args.join(", ")))
            }
        }
        TyKind::Param(param) => Some(ty_ctx.name_str(param.name).to_string()),
        // Named bound vars (a generic's canonical parameter) render by
        // name; anonymous ones have no spelling the user wrote.
        TyKind::Bound(_, bound) => match bound.kind {
            glyim_type::BoundTyKind::Param(name) => Some(ty_ctx.name_str(name).to_string()),
            _ => None,
        },
        TyKind::Infer(_)
        | TyKind::Error
        | TyKind::FnDef(_, _)
        | TyKind::Closure(_, _)
        | TyKind::Dynamic(_, _)
        | TyKind::Opaque(_, _)
        | TyKind::Projection(_) => None,
    }
}

/// Render a substitution's type arguments (the label renderer's shared
/// recursion for tuples, function pointers, and ADT arguments).
/// Lifetime and const arguments render as `_` (the frozen context does
/// not spell them); a substitution the context cannot read fails the
/// whole label.
fn substitution_labels(ty_ctx: &TyCtx, substs: Substitution) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for arg in ty_ctx.substitution_args(substs) {
        match arg {
            GenericArg::Ty(t) => out.push(render_type_label(ty_ctx, *t)?),
            GenericArg::Lifetime(_) | GenericArg::Const(_) => out.push("_".to_string()),
        }
    }
    Some(out)
}

/// `Substitution` has no public emptiness check on the frozen context;
/// `substitution_args` is the only read path, so emptiness reads
/// through it.
fn substitution_args_is_empty(ty_ctx: &TyCtx, substs: Substitution) -> bool {
    ty_ctx.substitution_args(substs).is_empty()
}

/// `textDocument/inlayHint` request entry point (LSP-INLAY-HINT).
///
/// Resolution order mirrors `semantic_tokens_range`: URI → open-file
/// lookup → stored analysis state → policy (normalized bindings) →
/// range filter → wire positions. Returns `None` when the document is
/// not open, has no recorded typeck results, has no map-able range, or
/// yields no hints (an empty response would be legal, but `null`
/// matches the other engines' "nothing to answer" convention and saves
/// the wire round trip on files the policy skips entirely).
#[must_use]
pub fn inlay_hints(
    db: &AnalysisDatabase,
    file_map: &FileMap,
    params: &InlayHintParams,
) -> Option<Vec<InlayHint>> {
    let uri = &params.text_document.uri;
    let path = Url::parse(uri.as_str()).ok()?.to_file_path().ok()?;
    let file_id = file_map.get_by_path(&path)?;

    let source_maps = db.source_maps.read();
    let sm = source_maps.get(&file_id)?;
    let hirs = db.hirs.read();
    let hir = hirs.get(&file_id)?;
    let typeck_map = db.typeck.read();
    let (ty_ctx, typeck_result) = typeck_map.get(&file_id)?;

    let bindings = compute_inlay_hints(&HirBindingTypeSource::new(hir, typeck_result, ty_ctx));
    if bindings.is_empty() {
        return None;
    }

    // The wire range is (line, utf16 col) pairs; map both ends back to
    // byte offsets so the attachment offsets filter directly. An
    // unmappable endpoint (a line past EOF, a column past its line)
    // leaves nothing to answer.
    let range_lo = sm.line_col_to_offset(
        params.range.start.line as usize,
        params.range.start.character as usize,
    )?;
    let range_hi = sm.line_col_to_offset(
        params.range.end.line as usize,
        params.range.end.character as usize,
    )?;
    if range_hi <= range_lo {
        return None;
    }

    let mut wire = Vec::with_capacity(bindings.len());
    for binding in bindings {
        // Attach-only filter: a hint belongs to the range its
        // attachment offset falls in (an editor hints the visible
        // lines, and a hint outside them would be dead payload).
        if binding.after < range_lo || binding.after > range_hi {
            continue;
        }
        let (line, col) = sm.span_to_position(binding.after, binding.after)?.0;
        wire.push(InlayHint {
            position: lsp_types::Position {
                line: line as u32,
                character: col as u32,
            },
            label: InlayHintLabel::String(format!(": {}", binding.type_label)),
            kind: Some(InlayHintKind::TYPE),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: None,
            data: None,
        });
    }
    if wire.is_empty() {
        return None;
    }
    tracing::debug!(hints = wire.len(), "inlay hints computed");
    Some(wire)
}
