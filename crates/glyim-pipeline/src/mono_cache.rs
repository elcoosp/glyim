#![allow(clippy::single_match)]
//! Mono item caching for the pipeline.

use glyim_core::Mutability;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_diag::{DiagSink, GlyimDiagnostic};
use glyim_lower::mono::MonoItemData;
use glyim_mir::{
    BasicBlockData, BasicBlockIdx, Body, LocalDecl, LocalIdx, MirConst, MirConstKind, Operand,
    Place, ProjectionElem, Rvalue, SourceInfo, Statement, StatementKind, SwitchTargets, Terminator,
    TerminatorKind, VariantIdx,
};
use glyim_span::Span;
use glyim_type::{
    AdtDef, AdtKind, ConstKind, FieldIdx, GenericArg, ParamTy, Substitution, Ty, TyCtx, TyCtxMut,
    TyKind,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

pub(crate) struct PipelineMonoCache {
    symbols: Vec<String>,
}

impl PipelineMonoCache {
    pub(crate) fn from_items(items: &[MonoItemData]) -> Self {
        let symbols = items.iter().map(|d| d.symbol.clone()).collect();
        PipelineMonoCache { symbols }
    }

    pub(crate) fn symbols(&self) -> &[String] {
        &self.symbols
    }

    #[allow(dead_code)]
    pub(crate) fn len(&self) -> usize {
        self.symbols.len()
    }

    #[allow(dead_code)]
    pub(crate) fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }
}

/// Build a pipeline mono cache, applying polymorphization deduplication first.
///
/// Call this instead of `PipelineMonoCache::from_items` directly so that polymorphization
/// is actually integrated into the pipeline.
#[allow(dead_code)]
pub(crate) fn build_mono_cache(
    ctx: &mut glyim_lower::MonoCtx,
    ty_ctx: &mut TyCtxMut,
) -> PipelineMonoCache {
    ctx.polymorphize_and_deduplicate(ty_ctx);
    PipelineMonoCache::from_items(ctx.items())
}

pub(crate) fn substitute_body(body: &Body, substs: &Substitution, ty_ctx: &TyCtx) -> Body {
    // To perform recursive substitution, we need a mutable context to allocate new types.
    // We create a fresh `TyCtxMut` from the interner for this purpose.
    // This is safe because substitution only reads from the frozen `ty_ctx` and writes new types
    // to the mutable context.
    let mut sub_ctx = TyCtxMut::from_ty_ctx(ty_ctx);

    fn substitute_ty(ty: Ty, substs: &Substitution, ctx: &mut TyCtxMut, frozen: &TyCtx) -> Ty {
        match frozen.ty_kind(ty).clone() {
            TyKind::Param(ParamTy { index, .. }) => {
                let args = frozen.substitution_args(*substs);
                if let Some(GenericArg::Ty(t)) = args.get(index as usize) {
                    *t
                } else {
                    frozen.error_ty()
                }
            }
            TyKind::Ref(r, inner, m) => {
                let new_inner = substitute_ty(inner, substs, ctx, frozen);
                if new_inner == inner {
                    return ty;
                }
                ctx.mk_ref(r, new_inner, m)
            }
            TyKind::RawPtr(inner, m) => {
                let new_inner = substitute_ty(inner, substs, ctx, frozen);
                if new_inner == inner {
                    return ty;
                }
                ctx.mk_ty(TyKind::RawPtr(new_inner, m))
            }
            TyKind::Slice(inner) => {
                let new_inner = substitute_ty(inner, substs, ctx, frozen);
                if new_inner == inner {
                    return ty;
                }
                ctx.mk_ty(TyKind::Slice(new_inner))
            }
            TyKind::Array(inner, len) => {
                let new_inner = substitute_ty(inner, substs, ctx, frozen);
                // Substitute const length if it's a param
                let new_len = {
                    let mut new_len = len.clone();
                    if let ConstKind::Param(p) = &len.kind {
                        let args = frozen.substitution_args(*substs);
                        if let Some(GenericArg::Const(c)) = args.get(p.index as usize) {
                            new_len = c.clone();
                        }
                    }
                    new_len
                };
                if new_inner == inner && new_len == len {
                    return ty;
                }
                ctx.mk_ty(TyKind::Array(new_inner, new_len))
            }
            TyKind::Tuple(sub) => {
                let args = frozen.substitution_args(sub);
                let new_args: Vec<GenericArg> = args
                    .iter()
                    .map(|arg| {
                        if let GenericArg::Ty(t) = arg {
                            GenericArg::Ty(substitute_ty(*t, substs, ctx, frozen))
                        } else {
                            arg.clone()
                        }
                    })
                    .collect();
                if new_args == args.to_vec() {
                    return ty;
                }
                let new_sub = ctx.intern_substitution(new_args);
                ctx.mk_tuple(new_sub)
            }
            TyKind::Adt(id, sub) => {
                let args = frozen.substitution_args(sub);
                let new_args: Vec<GenericArg> = args
                    .iter()
                    .map(|arg| {
                        if let GenericArg::Ty(t) = arg {
                            GenericArg::Ty(substitute_ty(*t, substs, ctx, frozen))
                        } else {
                            arg.clone()
                        }
                    })
                    .collect();
                if new_args == args.to_vec() {
                    return ty;
                }
                let new_sub = ctx.intern_substitution(new_args);
                ctx.mk_adt(id, new_sub)
            }
            TyKind::FnPtr(sig) => {
                // Recurse into the signature: its input/output types may contain
                // `Param`s (e.g. a trait-method reference `fn(&mut F) -> Poll<F::Output>`).
                let inputs: Vec<GenericArg> = frozen
                    .substitution_args(sig.inputs)
                    .iter()
                    .map(|arg| match arg {
                        GenericArg::Ty(t) => {
                            GenericArg::Ty(substitute_ty(*t, substs, ctx, frozen))
                        }
                        other => other.clone(),
                    })
                    .collect();
                let new_output = substitute_ty(sig.output, substs, ctx, frozen);
                if inputs == frozen.substitution_args(sig.inputs).to_vec()
                    && new_output == sig.output
                {
                    return ty;
                }
                let new_inputs = ctx.intern_substitution(inputs);
                let new_sig = glyim_type::FnSig {
                    inputs: new_inputs,
                    output: new_output,
                    c_variadic: sig.c_variadic,
                    unsafety: sig.unsafety,
                    abi: sig.abi,
                };
                ctx.mk_ty(TyKind::FnPtr(new_sig))
            }
            TyKind::FnDef(def_id, sub) => {
                let args = frozen.substitution_args(sub);
                let new_args: Vec<GenericArg> = args
                    .iter()
                    .map(|arg| match arg {
                        GenericArg::Ty(t) => {
                            GenericArg::Ty(substitute_ty(*t, substs, ctx, frozen))
                        }
                        other => other.clone(),
                    })
                    .collect();
                if new_args == args.to_vec() {
                    return ty;
                }
                let new_sub = ctx.intern_substitution(new_args);
                ctx.mk_ty(TyKind::FnDef(def_id, new_sub))
            }
            TyKind::Projection(proj) => {
                // Associated-type projection (`F::Output`, `Self::Output`, ...):
                // normalize to the concrete defining type. Mirror
                // `glyim-codegen-llvm::types::llvm_type_for_ty`'s Projection arm.
                // First substitute the projection's self type (it may be a `Param`
                // referencing the instantiation substs), then resolve via the
                // projection table. Without this, `block_on<F: Future>`'s return
                // type `Poll<F::Output>` keeps a `Param` after monomorphization
                // and ICEs at codegen/layout with `UnknownType(Param)`.
                let proj_subst_args = frozen.substitution_args(proj.trait_ref.substs);
                let proj_self = proj_subst_args
                    .first()
                    .and_then(|a| match a {
                        GenericArg::Ty(t) => Some(*t),
                        _ => None,
                    });
                let concrete_self = match proj_self {
                    Some(st) => match frozen.ty_kind(st) {
                        TyKind::Param(p) => proj_subst_args
                            .get(p.index as usize)
                            .and_then(|a| match a {
                                GenericArg::Ty(t) => Some(*t),
                                _ => None,
                            })
                            .unwrap_or(st),
                        _ => st,
                    },
                    None => return ty,
                };
                // Recurse into the self type in case it still holds nested params.
                let concrete_self = substitute_ty(concrete_self, substs, ctx, frozen);
                let resolved = frozen.resolve_associated_type(
                    concrete_self,
                    proj.trait_ref.def_id,
                    proj.item_name,
                );
                match resolved {
                    Some(resolved) => resolved,
                    None => {
                        // Could not resolve (no matching impl); fall back to a
                        // structurally-substituted projection so we don't drop
                        // information, but this will still surface as a layout
                        // error if truly unresolvable.
                        let new_subst_args: Vec<GenericArg> = proj_subst_args
                            .iter()
                            .map(|a| match a {
                                GenericArg::Ty(t) => {
                                    GenericArg::Ty(substitute_ty(*t, substs, ctx, frozen))
                                }
                                other => other.clone(),
                            })
                            .collect();
                        let new_sub = ctx.intern_substitution(new_subst_args);
                        ctx.mk_ty(TyKind::Projection(glyim_type::ProjectionTy {
                            trait_ref: glyim_type::TraitRef {
                                def_id: proj.trait_ref.def_id,
                                substs: new_sub,
                            },
                            item_name: proj.item_name,
                        }))
                    }
                }
            }
            _ => ty,
        }
    }

    /// Substitute `Param` types inside an `Operand` (its `MirConst` `ty` and any
    /// generic `substs` carried by `MirConstKind::Fn`/`ConstRef`). `Copy`/`Move`
    /// operands reference locals whose types are substituted separately.
    fn substitute_operand(
        operand: Operand,
        substs: &Substitution,
        ctx: &mut TyCtxMut,
        frozen: &TyCtx,
    ) -> Operand {
        match operand {
            Operand::Constant(mut mir_const) => {
                mir_const.ty = substitute_ty(mir_const.ty, substs, ctx, frozen);
                match &mut mir_const.kind {
                    glyim_mir::MirConstKind::Fn(_, fn_substs) => {
                        *fn_substs = substitute_substitution(*fn_substs, substs, ctx, frozen);
                    }
                    glyim_mir::MirConstKind::ConstRef(_, const_substs) => {
                        *const_substs =
                            substitute_substitution(*const_substs, substs, ctx, frozen);
                    }
                    _ => {}
                }
                Operand::Constant(mir_const)
            }
            other => other,
        }
    }

    /// Substitute `Param` types inside a `Substitution` (used for the generic
    /// `substs` carried by `MirConstKind::Fn`/`ConstRef`).
    fn substitute_substitution(
        s: Substitution,
        substs: &Substitution,
        ctx: &mut TyCtxMut,
        frozen: &TyCtx,
    ) -> Substitution {
        let args: Vec<glyim_type::GenericArg> = frozen
            .substitution_args(s)
            .iter()
            .map(|arg| match arg {
                glyim_type::GenericArg::Ty(t) => {
                    glyim_type::GenericArg::Ty(substitute_ty(*t, substs, ctx, frozen))
                }
                other => other.clone(),
            })
            .collect();
        ctx.intern_substitution(args)
    }

    let mut new_locals = body.locals.clone();
    for local in new_locals.iter_mut() {
        local.ty = substitute_ty(local.ty, substs, &mut sub_ctx, ty_ctx);
    }

    let mut new_blocks = body.basic_blocks.clone();
    for block_data in new_blocks.iter_mut() {
        for stmt in &mut block_data.statements {
            if let StatementKind::Assign(_, rvalue) = &mut stmt.kind {
                match rvalue {
                    Rvalue::Cast(_, _, target_ty) => {
                        *target_ty = substitute_ty(*target_ty, substs, &mut sub_ctx, ty_ctx);
                    }
                    Rvalue::Repeat(_, const_val) => {
                        const_val.ty = substitute_ty(const_val.ty, substs, &mut sub_ctx, ty_ctx);
                    }
                    _ => {}
                }
            }
        }
        // Substitute types embedded in terminator operands (e.g. the `ty` of a
        // `MirConst` function/method reference, and the generic `substs` carried
        // by `MirConstKind::Fn`/`ConstRef`). Without this, a generic function's
        // body that calls a trait method (e.g. `block_on`'s `f.poll()`) keeps
        // the unsubstituted `Param` in the call operand and ICEs at codegen
        // with "TyKind::Param reached LLVM codegen".
        let terminator = &mut block_data.terminator;
        match &mut terminator.kind {
            TerminatorKind::Call { func, args, .. } => {
                *func = substitute_operand(func.clone(), substs, &mut sub_ctx, ty_ctx);
                for arg in args.iter_mut() {
                    *arg = substitute_operand(arg.clone(), substs, &mut sub_ctx, ty_ctx);
                }
            }
            TerminatorKind::SwitchInt { discr, switch_ty, .. } => {
                *discr = substitute_operand(discr.clone(), substs, &mut sub_ctx, ty_ctx);
                *switch_ty = substitute_ty(*switch_ty, substs, &mut sub_ctx, ty_ctx);
            }
            _ => {}
        }
    }

    let return_ty = substitute_ty(body.return_ty, substs, &mut sub_ctx, ty_ctx);
    Body {
        owner: body.owner,
        basic_blocks: new_blocks,
        locals: new_locals,
        arg_count: body.arg_count,
        return_ty,
        span: body.span,
        var_debug_info: body.var_debug_info.clone(),
    }
}

pub(crate) fn make_mir_body_provider<'a>(
    bodies: &'a std::collections::HashMap<DefId, Arc<Body>>,
    sink: &'a RefCell<DiagSink>,
    ty_ctx: &'a TyCtx,
) -> impl Fn(DefId, &Substitution) -> Arc<Body> + 'a {
    move |def_id: DefId, substs: &Substitution| -> Arc<Body> {
        if let Some(body) = bodies.get(&def_id) {
            if substs.is_empty() {
                body.clone()
            } else {
                let substituted = substitute_body(body, substs, ty_ctx);
                Arc::new(substituted)
            }
        } else {
            let diag = GlyimDiagnostic::internal_error(format!(
                "MIR body not found for DefId {:?}",
                def_id
            ));
            sink.borrow_mut().emit(diag);
            Arc::new(Body::dummy(DefId::new(
                CrateId::from_raw(0),
                LocalDefId::from_raw(0),
            )))
        }
    }
}

/// Create a drop-glue provider that can generate recursive drop glue for ADTs.
///
/// The provider only requires `&TyCtx` (immutable) so it fits the existing pipeline
/// call site.  Enum discriminant locals use the real tag type selected by
/// `discriminant_info` (a `u8`/`u16`/`u32`/`u64` sized to the variant count,
/// de-stubbing plan §7.1 / §13.1) — no placeholder type remains.
pub(crate) fn make_drop_glue_provider(ty_ctx: &TyCtx) -> impl Fn(Ty) -> Arc<Body> + '_ {
    move |ty: Ty| -> Arc<Body> { generate_drop_glue(ty, ty_ctx) }
}

pub(crate) fn generate_drop_glue(ty: Ty, ty_ctx: &TyCtx) -> Arc<Body> {
    let def_id = DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(0));
    let mut body = Body::dummy(def_id);
    body.return_ty = ty_ctx.unit_ty();

    let ptr_local = LocalIdx::from_raw(0);
    let place = Place::new(ptr_local);

    // Fast path: types that do not need dropping get a single `Return`.
    if !type_needs_drop(ty, ty_ctx, &mut HashSet::new()) {
        set_return_terminator(&mut body, BasicBlockIdx::from_raw(0));
        return Arc::new(body);
    }

    match ty_ctx.ty_kind(ty) {
        TyKind::Adt(adt_id, _) => {
            if let Some(adt_def) = ty_ctx.adt_def(*adt_id) {
                match adt_def.kind {
                    AdtKind::Struct => {
                        generate_struct_drop_glue(&mut body, &place, adt_def, ty_ctx);
                    }
                    AdtKind::Enum => {
                        generate_enum_drop_glue(&mut body, &place, adt_def, ty_ctx);
                    }
                    AdtKind::Union => {
                        // Unions do not have automatic drop glue; the user is responsible
                        // for unsafe union manipulation.
                        set_return_terminator(&mut body, BasicBlockIdx::from_raw(0));
                    }
                }
            } else {
                set_return_terminator(&mut body, BasicBlockIdx::from_raw(0));
            }
        }
        TyKind::Array(elem_ty, len) if type_needs_drop(*elem_ty, ty_ctx, &mut HashSet::new()) => {
            // For an `[T; N]` where `T` needs drop, drop every element in order.
            // The element type's own glue is registered by the collector (it scans
            // `Drop` terminators and enqueues the element type), so emitting a
            // `Drop` terminator per element — exactly as `generate_struct_drop_glue`
            // does per field — is sufficient; downstream elaboration turns each
            // into the element's recursive drop glue. A bare `Return` here is the
            // de-stubbing-plan §16.1 bug: it would skip every element's destructor.
            generate_array_drop_glue(&mut body, &place, *elem_ty, len, ty_ctx);
        }
        TyKind::Slice(elem_ty) if type_needs_drop(*elem_ty, ty_ctx, &mut HashSet::new()) => {
            // For a `[T]` slice, the element count is the runtime fat-pointer
            // length (read via `Rvalue::Len`); the same per-element drop loop is
            // driven by that length instead of a compile-time constant.
            generate_slice_drop_glue(&mut body, &place, *elem_ty, ty_ctx);
        }
        _ => {
            set_return_terminator(&mut body, BasicBlockIdx::from_raw(0));
        }
    }

    Arc::new(body)
}

fn set_return_terminator(body: &mut Body, block: BasicBlockIdx) {
    if let Some(block_data) = body.basic_blocks.get_mut(block) {
        block_data.terminator.kind = TerminatorKind::Return;
    }
}

// ---------------------------------------------------------------------------
// type_needs_drop
// ---------------------------------------------------------------------------

/// Determine whether a type needs drop glue.
///
/// Delegates to the single canonical `TyCtx::needs_drop` (de-stubbing plan
/// §0 rule 2 / §7.1) so there is exactly one source of truth for "does this type
/// need dropping". The `visited` set is accepted for call-site compatibility but
/// the canonical implementation performs its own cycle guard internally.
fn type_needs_drop(ty: Ty, ty_ctx: &TyCtx, _visited: &mut HashSet<Ty>) -> bool {
    ty_ctx.needs_drop(ty)
}

// ---------------------------------------------------------------------------
// Struct drop glue
// ---------------------------------------------------------------------------

fn generate_struct_drop_glue(body: &mut Body, place: &Place, adt_def: &AdtDef, ty_ctx: &TyCtx) {
    let fields_to_drop: Vec<_> = adt_def.variants[0]
        .fields
        .iter()
        .enumerate()
        .filter(|(_, f)| type_needs_drop(f.ty, ty_ctx, &mut HashSet::new()))
        .map(|(i, _)| i)
        .collect();

    if fields_to_drop.is_empty() {
        set_return_terminator(body, BasicBlockIdx::from_raw(0));
        return;
    }

    let return_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    let mut next_target = return_bb;

    // Build the tail of the chain in reverse order (last field -> second field).
    for field_idx in fields_to_drop.iter().skip(1).rev() {
        let mut proj = place.projection.to_vec();
        proj.push(ProjectionElem::Field(FieldIdx::from_raw(*field_idx as u32)));
        let field_place = Place {
            local: place.local,
            projection: proj.into_boxed_slice(),
        };

        let bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Drop {
                place: field_place,
                target: next_target,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        next_target = bb;
    }

    // The first field drop re-uses basic block 0.
    let first_field_idx = fields_to_drop[0];
    let mut proj = place.projection.to_vec();
    proj.push(ProjectionElem::Field(FieldIdx::from_raw(
        first_field_idx as u32,
    )));
    let field_place = Place {
        local: place.local,
        projection: proj.into_boxed_slice(),
    };

    if let Some(block0) = body.basic_blocks.get_mut(BasicBlockIdx::from_raw(0)) {
        block0.statements.clear();
        block0.terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: field_place,
                target: next_target,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
    }
}

// ---------------------------------------------------------------------------
// Enum drop glue
// ---------------------------------------------------------------------------

fn generate_enum_drop_glue(body: &mut Body, place: &Place, adt_def: &AdtDef, ty_ctx: &TyCtx) {
    let variants = &adt_def.variants;

    // Block that all variant-specific drop chains fall through to.
    let return_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    // Enum discriminant local: pick the smallest unsigned integer whose value range
    // covers every variant (de-stubbing plan §13.1 / §7.1). This matches
    // `glyim-layout::discriminant_info` and the `U8`/`U16`/`U32` tag scheme in
    // `glyim-codegen-llvm/abi.rs`, so the drop-glue discriminant type agrees with the
    // layout/codegen tag type instead of being an `error_ty()` placeholder.
    let n_variants = variants.len();
    let discr_ty = if n_variants <= 256 {
        glyim_type::Ty::U8
    } else if n_variants <= 65_536 {
        glyim_type::Ty::U16
    } else if n_variants <= 4_294_967_296 {
        glyim_type::Ty::U32
    } else {
        glyim_type::Ty::U64
    };

    let discr_local = body.locals.push(LocalDecl {
        ty: discr_ty,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let discr_place = Place::new(discr_local);

    // Build a drop-chain entry block for every variant.
    let mut variant_entry_blocks = Vec::new();

    for (variant_idx, variant) in variants.iter().enumerate() {
        let fields_to_drop: Vec<_> = variant
            .fields
            .iter()
            .enumerate()
            .filter(|(_, f)| type_needs_drop(f.ty, ty_ctx, &mut HashSet::new()))
            .map(|(i, _)| i)
            .collect();

        if fields_to_drop.is_empty() {
            variant_entry_blocks.push(return_bb);
            continue;
        }

        let mut next_target = return_bb;

        // Reverse chain for all fields except the first.
        for field_idx in fields_to_drop.iter().skip(1).rev() {
            let mut proj = place.projection.to_vec();
            proj.push(ProjectionElem::Downcast(VariantIdx::from_raw(
                variant_idx as u32,
            )));
            proj.push(ProjectionElem::Field(FieldIdx::from_raw(*field_idx as u32)));
            let field_place = Place {
                local: place.local,
                projection: proj.into_boxed_slice(),
            };

            let bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
                kind: TerminatorKind::Drop {
                    place: field_place,
                    target: next_target,
                    cleanup: None,
                },
                source_info: SourceInfo::new(Span::DUMMY),
            }));
            next_target = bb;
        }

        // First field for this variant gets its own block.
        let first_field_idx = fields_to_drop[0];
        let mut proj = place.projection.to_vec();
        proj.push(ProjectionElem::Downcast(VariantIdx::from_raw(
            variant_idx as u32,
        )));
        proj.push(ProjectionElem::Field(FieldIdx::from_raw(
            first_field_idx as u32,
        )));
        let field_place = Place {
            local: place.local,
            projection: proj.into_boxed_slice(),
        };

        let entry_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Drop {
                place: field_place,
                target: next_target,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        variant_entry_blocks.push(entry_bb);
    }

    // Assemble SwitchInt in block 0.
    let branches: Vec<_> = variant_entry_blocks
        .iter()
        .enumerate()
        .map(|(i, bb)| (i as u128, *bb))
        .collect();
    let otherwise = return_bb;
    let switch_targets = SwitchTargets::new(branches.into_boxed_slice(), otherwise);

    if let Some(block0) = body.basic_blocks.get_mut(BasicBlockIdx::from_raw(0)) {
        block0.statements.clear();
        block0.statements.push(Statement {
            kind: StatementKind::Assign(discr_place.clone(), Rvalue::Discriminant(place.clone())),
            source_info: SourceInfo::new(Span::DUMMY),
        });
        block0.terminator = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: Operand::Copy(discr_place),
                switch_ty: discr_ty,
                targets: switch_targets,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
    }
}

// ---------------------------------------------------------------------------
// Array drop glue
// ---------------------------------------------------------------------------

/// Generate drop glue for `[T; N]` where `T` needs drop: one `Drop` terminator
/// per element, chained in forward order (element 0 → element N-1), exactly like
/// `generate_struct_drop_glue` emits one `Drop` terminator per field. The
/// element type's own glue is registered by the collector from each `Drop`
/// terminator. A bare `Return` here (the previous behavior) is the de-stubbing
/// plan §16.1 bug: it would skip every element's destructor.
fn generate_array_drop_glue(
    body: &mut Body,
    place: &Place,
    _elem_ty: Ty,
    len: &glyim_type::Const,
    _ty_ctx: &TyCtx,
) {
    let n = match len.kind {
        glyim_type::ConstKind::Uint(n) => n,
        glyim_type::ConstKind::Int(n) => n as u128,
        _ => {
            // Phase 2c (GLYIM_DESTUB_PLAN): reaching here with a non-monomorphic
            // array length is a genuine compiler bug — monomorphization should
            // have resolved `ConstKind::Param` into a concrete length. The old
            // silent `Return` fallback skipped every element's destructor and
            // leaked memory without any observable failure. Panic instead so
            // the regression test catches it immediately.
            panic!(
                "internal error: array drop glue requested for `[T; {:?}]` with a \
                 non-monomorphic length after monomorphization — this means \
                 TyCtx::subst_ty or polymorphize's param-usage tracking has a bug. \
                 len = {:?}",
                len, len
            );
        }
    };

    let return_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    // Build the tail of the chain in reverse order so the first element re-uses
    // basic block 0 (same shape as the struct field-drop chain).
    let mut next_target = return_bb;
    for i in (1..n).rev() {
        let elem_place = element_place_at(place, i as u32);
        let bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
            kind: TerminatorKind::Drop {
                place: elem_place,
                target: next_target,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        }));
        next_target = bb;
    }

    let first_elem = element_place_at(place, 0);
    if let Some(block0) = body.basic_blocks.get_mut(BasicBlockIdx::from_raw(0)) {
        block0.statements.clear();
        block0.terminator = Terminator {
            kind: TerminatorKind::Drop {
                place: first_elem,
                target: next_target,
                cleanup: None,
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
    }
}

/// Build a place that indexes `base` by the compile-time constant `index`
/// (`base[`index`]`).
fn element_place_at(base: &Place, index: u32) -> Place {
    let idx_local = LocalIdx::from_raw(index);
    let mut proj = base.projection.to_vec();
    proj.push(ProjectionElem::Index(idx_local));
    Place {
        local: base.local,
        projection: proj.into_boxed_slice(),
    }
}

// ---------------------------------------------------------------------------
// Slice drop glue
// ---------------------------------------------------------------------------

/// Generate drop glue for `[T]` where `T` needs drop: a per-element drop loop
/// driven by the runtime length read via `Rvalue::Len(place)` (the fat-pointer
/// metadata word), since a slice's element count is not known at compile time
/// (de-stubbing plan §16.1).
fn generate_slice_drop_glue(body: &mut Body, place: &Place, _elem_ty: Ty, _ty_ctx: &TyCtx) {
    let idx_local = body.locals.push(LocalDecl {
        ty: Ty::USIZE,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });
    let len_local = body.locals.push(LocalDecl {
        ty: Ty::USIZE,
        mutability: Mutability::Not,
        source_info: SourceInfo::new(Span::DUMMY),
    });

    let exit_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Return,
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    let inc_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlockIdx::from_raw(0),
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));
    let body_bb = body.basic_blocks.push(BasicBlockData::new(Terminator {
        kind: TerminatorKind::Drop {
            place: element_place(place, idx_local),
            target: inc_bb,
            cleanup: None,
        },
        source_info: SourceInfo::new(Span::DUMMY),
    }));

    // Increment block: idx = idx + 1.
    if let Some(inc) = body.basic_blocks.get_mut(inc_bb) {
        inc.statements.push(Statement {
            kind: StatementKind::Assign(
                Place::new(idx_local),
                Rvalue::BinaryOp(
                    glyim_core::primitives::BinOp::Add,
                    Box::new((
                        Operand::Copy(Place::new(idx_local)),
                        Operand::Constant(MirConst {
                            kind: MirConstKind::Uint(1),
                            ty: Ty::USIZE,
                            span: Span::DUMMY,
                        }),
                    )),
                ),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
    }

    // Header (basic block 0): read len, set idx = 0, loop while idx < len.
    if let Some(block0) = body.basic_blocks.get_mut(BasicBlockIdx::from_raw(0)) {
        block0.statements.clear();
        block0.statements.push(Statement {
            kind: StatementKind::Assign(
                Place::new(len_local),
                Rvalue::Len(place.clone()),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
        block0.statements.push(Statement {
            kind: StatementKind::Assign(
                Place::new(idx_local),
                Rvalue::Use(Operand::Constant(MirConst {
                    kind: MirConstKind::Uint(0),
                    ty: Ty::USIZE,
                    span: Span::DUMMY,
                })),
            ),
            source_info: SourceInfo::new(Span::DUMMY),
        });
        block0.terminator = Terminator {
            kind: TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::new(idx_local)),
                switch_ty: Ty::USIZE,
                targets: SwitchTargets::new(
                    Box::new([(0, exit_bb)]),
                    body_bb,
                ),
            },
            source_info: SourceInfo::new(Span::DUMMY),
        };
    }
}

/// Build a place that indexes `base` by `idx_local` (`base[idx_local]`).
fn element_place(base: &Place, idx_local: LocalIdx) -> Place {
    let mut proj = base.projection.to_vec();
    proj.push(ProjectionElem::Index(idx_local));
    Place {
        local: base.local,
        projection: proj.into_boxed_slice(),
    }
}

pub(crate) fn compute_max_cgus() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    cores.clamp(1, 16)
}
