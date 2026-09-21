use glyim_core::def_id::{ConstDefId, FnDefId};
use glyim_core::{LocalDefId, Name};
use glyim_diag::GlyimDiagnostic;
use glyim_hir::Body;
use glyim_span::Span;
use glyim_type::*;
use glyim_typeck::thir;

#[derive(Clone, Debug)]
/// LowerResult.
pub struct LowerResult {
    /// Struct.
    pub body: glyim_mir::Body,
    /// Struct.
    pub diagnostics: Vec<GlyimDiagnostic>,
    /// Bodies of closures captured during this function's lowering. Each closure
    /// body owns the leading `captures...` arguments followed by the closure's
    /// own parameters, and is emitted as `__glyim_fn_{closure_id}` by codegen.
    pub closure_bodies: Vec<(
        glyim_core::def_id::ClosureId,
        glyim_type::Substitution,
        glyim_mir::Body,
    )>,
    /// Async v1 resume-dispatch plan (plan §Phase 3 / `ASYNC_V1_MIR_PLAN.md`).
    ///
    /// Computed by `async_state_transform::transform_async_body` after MIR
    /// generation. For non-async bodies and single-await bodies this is a
    /// trivial plan (`sites.len() <= 1`); for multi-await async `poll` methods
    /// it is the `Start`/`S0`..`S_{n-1}`/`Done` state-machine plan that the
    /// (currently tracked, host-unverifiable) M4 codegen would apply. It is
    /// stored here so the pipeline has the plan available without recomputing.
    pub async_transform: Option<crate::async_state_transform::AsyncTransformPlan>,
}

/// Pre-computed information about the `Iterator::next` method for a specific
/// iterator type. Used by for-loop lowering to generate the `next()` call
/// and `Option` switching.
///
/// All types are pre-constructed during the mutable type-context phase because
/// the lowering only has access to a frozen `TyCtx`.
#[derive(Clone, Debug)]
pub struct IteratorNextInfo {
    /// The `FnDefId` for the `Iterator::next` method.
    pub fn_def_id: FnDefId,
    /// The substitution for the `next()` method.
    pub fn_substs: Substitution,
    /// The type of the `next()` function reference (`TyKind::FnDef`).
    pub fn_ty: Ty,
    /// The return type of `next()`: `Option<elem_ty>`.
    pub option_ty: Ty,
    /// The discriminant type for the `Option` enum (typically `u8`).
    pub discr_ty: Ty,
    /// The type of `&mut I` — the argument passed to `next()`.
    pub ref_iter_ty: Ty,
}

/// Context trait provided by the caller to the THIR→MIR lowering.
///
/// Implementors provide type information, ADT definitions, and name-resolution
/// capabilities that the lowering needs but cannot access from THIR alone.
pub trait LowerCtx {
    /// Access the frozen type context.
    fn ty_ctx(&self) -> &TyCtx;

    /// Get the ADT definition for the given ADT ID.
    fn adt_def(&self, id: glyim_core::def_id::AdtId) -> AdtDef;

    /// Push a source span onto the span stack (for diagnostic context).
    fn push_span(&self, span: Span);

    /// Pop a source span from the span stack.
    fn pop_span(&self);

    /// Resolve a field by name within a specific variant of an ADT.
    ///
    /// Returns the `FieldIdx` of the field if found, or `None` if the field
    /// name is not present in the given variant.
    fn field_index_by_name(
        &self,
        _adt_id: glyim_core::def_id::AdtId,
        _variant_idx: u32,
        _name: Name,
    ) -> Option<FieldIdx> {
        None
    }
    /// Fetches the original HIR body for a given owner.
    /// This is required for evaluating `const { ... }` patterns at compile time.
    fn hir_body(&self, owner: LocalDefId) -> Option<&Body>;
    /// Resolve a variant by name within an ADT.
    ///
    /// Returns the variant index if found, or `None` if no variant with
    /// that name exists.
    fn variant_index_by_name(
        &self,
        _adt_id: glyim_core::def_id::AdtId,
        _name: Name,
    ) -> Option<u32> {
        None
    }

    /// Get the function signature for a function definition.
    fn fn_sig(&self, _def_id: FnDefId) -> Option<FnSig> {
        None
    }

    /// Get the constant value for a constant definition.
    fn const_value(
        &self,
        _def_id: ConstDefId,
        _substs: Substitution,
    ) -> Option<glyim_mir::MirConst> {
        None
    }

    /// Get information about the `Iterator::next` method for the given
    /// iterator type.
    ///
    /// Returns `None` if the iterator protocol is not available, in which
    /// case for-loop lowering uses a simplified model (loop without
    /// `next()` call).
    ///
    /// When `Some` is returned, for-loop lowering generates a full `Call`
    /// terminator for `next()` followed by a `SwitchInt` on the `Option`
    /// discriminant.
    fn iterator_next_fn(&self, _iter_ty: Ty, _elem_ty: Ty) -> Option<IteratorNextInfo> {
        None
    }
}

/// ADT definition used during lowering.
#[derive(Clone, Debug)]
pub struct AdtDef {
    /// Struct.
    pub variants: Vec<AdtVariant>,
    /// Struct.
    pub kind: AdtKind,
}

/// A single variant of an ADT (struct field list, enum variant, or union field).
#[derive(Clone, Debug)]
pub struct AdtVariant {
    /// Struct.
    pub fields: Vec<Ty>,
}

/// The kind of ADT.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdtKind {
    /// Variant.
    Struct,
    /// Variant.
    Enum,
    /// Variant.
    Union,
}

/// Lower a THIR body to MIR.
pub fn lower_body(ctx: &dyn LowerCtx, thir: &thir::Body) -> LowerResult {
    let mut builder = crate::builder::MirBuilder::new(ctx, thir);
    builder.lower_body(thir);

    let mut body = glyim_mir::Body::dummy(builder.owner);
    body.basic_blocks = builder.basic_blocks;
    body.locals = builder.locals;
    body.arg_count = builder.arg_count;
    body.return_ty = builder.return_ty;
    body.span = builder.span;

    // Async v1: compute the resume-dispatch plan for the lowered body. This is
    // a cheap analysis that is a no-op for non-async bodies (no `poll` Call
    // terminators => empty plan). The actual state-machine codegen (M4) applies
    // it after the future type is known at MIR. See `ASYNC_V1_MIR_PLAN.md`.
    let async_transform = crate::async_state_transform::transform_async_body(&body);

    LowerResult {
        body,
        diagnostics: builder.diagnostics,
        closure_bodies: builder.closure_bodies,
        async_transform: Some(async_transform),
    }
}
