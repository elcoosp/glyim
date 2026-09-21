use glyim_borrowck::BorrowckCtx;
use glyim_core::def_id::{AdtId, ConstDefId, FnDefId, LocalDefId};
use glyim_core::interner::Name;
use glyim_core::primitives::{Mutability, UintTy};
use glyim_hir::{CrateHir, ItemKind, TypeRef};
use glyim_lower::{AdtDef, AdtKind, AdtVariant, LowerCtx};
use glyim_mir::{Body, LocalDecl, LocalIdx, MirConst, MirConstKind};
use glyim_span::Span;
use glyim_type::{FieldIdx, GenericArg, Region, Ty, TyCtx, TyKind};
use std::cell::RefCell;
use std::collections::HashMap;
use tracing::warn;

pub(crate) struct PipelineLowerCtx<'a> {
    ty_ctx: &'a TyCtx,
    hir: &'a CrateHir,
    /// Evaluated const values (Part C: const value materialization), owned so
    /// the context does not borrow `TypeckResult` for its whole lifetime (the
    /// result is also moved/consumed elsewhere in the pipeline).
    const_values: HashMap<ConstDefId, glyim_const_eval::ConstValue>,
    span_stack: RefCell<Vec<Span>>,
}

impl<'a> PipelineLowerCtx<'a> {
    pub(crate) fn new(
        ty_ctx: &'a TyCtx,
        hir: &'a CrateHir,
        const_values: HashMap<ConstDefId, glyim_const_eval::ConstValue>,
    ) -> Self {
        PipelineLowerCtx {
            ty_ctx,
            hir,
            const_values,
            span_stack: RefCell::new(Vec::new()),
        }
    }
}

impl<'a> LowerCtx for PipelineLowerCtx<'a> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }
    fn hir_body(&self, owner: glyim_core::def_id::LocalDefId) -> Option<&glyim_hir::Body> {
        // Iterate through body owners to find the one matching the given LocalDefId
        for (body_id, body_owner) in self.hir.body_owners.iter_enumerated() {
            if *body_owner == owner {
                return self.hir.bodies.get(body_id);
            }
        }
        None
    }
    fn adt_def(&self, id: AdtId) -> AdtDef {
        if let Some(adt_def) = self.ty_ctx.adt_def(id) {
            let variants = adt_def
                .variants
                .iter()
                .map(|variant| AdtVariant {
                    fields: variant.fields.iter().map(|field| field.ty).collect(),
                })
                .collect();
            let kind = match adt_def.kind {
                glyim_type::AdtKind::Struct => AdtKind::Struct,
                glyim_type::AdtKind::Enum => AdtKind::Enum,
                glyim_type::AdtKind::Union => AdtKind::Union,
            };
            return AdtDef { variants, kind };
        }

        let def_id = glyim_core::def_id::DefId::new(
            glyim_core::def_id::CrateId::from_raw(0),
            glyim_core::def_id::LocalDefId::from_raw(id.to_raw()),
        );
        let item_id = glyim_hir::ItemId::from_raw(def_id.local_id.to_raw());

        match self.hir.items.get(item_id) {
            Some(item) => match &item.kind {
                ItemKind::Struct(s) => {
                    let fields = s
                        .fields
                        .iter()
                        .map(|_field| self.ty_ctx.error_ty())
                        .collect();
                    AdtDef {
                        variants: vec![AdtVariant { fields }],
                        kind: AdtKind::Struct,
                    }
                }
                ItemKind::Enum(e) => {
                    let variants = e
                        .variants
                        .iter()
                        .map(|variant| AdtVariant {
                            fields: variant
                                .fields
                                .iter()
                                .map(|_f| self.ty_ctx.error_ty())
                                .collect(),
                        })
                        .collect();
                    AdtDef {
                        variants,
                        kind: AdtKind::Enum,
                    }
                }
                _ => {
                    warn!("ADT id {:?} resolved to non-struct/enum item", id);
                    AdtDef {
                        variants: Vec::new(),
                        kind: AdtKind::Struct,
                    }
                }
            },
            None => {
                warn!("ADT id {:?} not found in HIR items", id);
                AdtDef {
                    variants: Vec::new(),
                    kind: AdtKind::Struct,
                }
            }
        }
    }

    fn field_index_by_name(&self, adt_id: AdtId, variant_idx: u32, name: Name) -> Option<FieldIdx> {
        let adt = self.ty_ctx.adt_def(adt_id)?;
        // Structs store their fields at the ADT level; enums store them per
        // variant. Search both: the struct-level `fields` first (covers
        // structs and guards against a zero-variant edge case), then the
        // requested variant's own field list. This is what makes enum variant
        // pattern destructuring (`match e { V { x, y } => .. }`) resolve field
        // indices at MIR lowering — previously `TyCtx::field_index` only
        // consulted the struct-level `fields`, which is empty for enums, so
        // every variant field binding fell back to `None` and never got a MIR
        // local (breaking the async state-machine desugar's
        // `match self.state { Start { f0 } => .. }`, M4/M5).
        if let Some(idx) = adt.fields.iter_enumerated().find(|(_, f)| f.name == name) {
            return Some(FieldIdx::from_raw(idx.0.index() as u32));
        }
        if let Some(variant) = adt.variants.get(variant_idx as usize)
            && let Some(idx) = variant
                .fields
                .iter_enumerated()
                .find(|(_, f)| f.name == name)
        {
            return Some(FieldIdx::from_raw(idx.0.index() as u32));
        }
        None
    }

    fn push_span(&self, span: Span) {
        self.span_stack.borrow_mut().push(span);
    }

    fn pop_span(&self) {
        self.span_stack.borrow_mut().pop();
    }

    fn const_value(
        &self,
        def_id: glyim_core::def_id::ConstDefId,
        _substs: glyim_type::Substitution,
    ) -> Option<glyim_mir::MirConst> {
        // Part C: const value materialization. Fold a `ConstRef` into a
        // concrete `MirConst` from the const-evaluated value produced by
        // typeck (stored in `TypeckResult::const_values`). Scalar constants
        // (integers, floats, bool, char, str, unit) and aggregate constants
        // (tuple/array/struct) fold fully; range constants have no `MirConst`
        // representation yet and fall back to `None` so the caller emits a
        // `ConstRef` (zero-initialized global) as before.
        let value = self.const_values.get(&def_id)?;
        let ty = self
            .ty_ctx
            .const_ty(def_id)
            .unwrap_or_else(|| self.ty_ctx.error_ty());
        let kind = self.cv_const(value, ty)?;
        Some(MirConst {
            kind,
            ty,
            span: Span::DUMMY,
        })
    }

    /// Phase 1 (GLYIM_DESTUB_PLAN): resolve `Iterator::next` for a for-loop's
    /// iterable type. Typeck threads this through the THIR `For.next` node when
    /// it can resolve the `impl Iterator` via its def-map; this method is the
    /// lowering-context fallback that resolves it directly from the program's
    /// HIR `impl Iterator for <iter_ty>` (read-only over the frozen `TyCtx` +
    /// `CrateHir`). Without it, every for-loop in a production build fell back
    /// to the one-iteration path. Mirrors typeck's `resolve_trait_method_fn`
    /// scan but needs no `InferCtx`/`def_map`: the iterable type is concrete.
    fn iterator_next_fn(&self, iter_ty: Ty, _elem_ty: Ty) -> Option<glyim_lower::IteratorNextInfo> {
        let iter_name = self.ty_ctx.resolver().intern("Iterator");
        let next_name = self.ty_ctx.resolver().intern("next");
        for (_id, item) in self.hir.items.iter_enumerated() {
            let ItemKind::Impl(impl_item) = &item.kind else {
                continue;
            };
            // The impl must be for the `Iterator` trait.
            let Some(trait_path) = &impl_item.trait_ref else {
                continue;
            };
            let Some(trait_last) = trait_path.segments.last() else {
                continue;
            };
            if trait_last.name != iter_name {
                continue;
            }
            // The impl's `Self` type must match the for-loop's iterable type.
            let Some(self_ty) = self.resolve_type_ref_to_ty(&impl_item.self_ty) else {
                continue;
            };
            if !self.ty_struct_eq(self_ty, iter_ty) {
                continue;
            }
            // Find the `next` method and its body's `FnDefId`.
            for method in &impl_item.methods {
                if method.name != next_name {
                    continue;
                }
                let Some(body_id) = method.body else {
                    continue;
                };
                let local: LocalDefId = self.hir.body_owners[body_id];
                let fn_def_id = FnDefId::from_raw(local.to_raw());
                let fn_substs = self.ty_ctx.intern_substitution(vec![]);
                let fn_ty = self.ty_ctx.mk_ty(TyKind::FnDef(fn_def_id, fn_substs));
                // The `next` body's return type is the real `Option<elem_ty>`.
                let option_ty = self
                    .ty_ctx
                    .fn_sig(fn_def_id)
                    .map(|s| s.output)
                    .unwrap_or_else(|| self.ty_ctx.error_ty());
                let ref_iter_ty = self.ty_ctx.mk_ref(Region::Erased, iter_ty, Mutability::Mut);
                let discr_ty = self.ty_ctx.mk_ty(TyKind::Uint(UintTy::U8));
                return Some(glyim_lower::IteratorNextInfo {
                    fn_def_id,
                    fn_substs,
                    fn_ty,
                    option_ty,
                    discr_ty,
                    ref_iter_ty,
                });
            }
        }
        None
    }
}

impl<'a> PipelineLowerCtx<'a> {
    /// Resolve a HIR `TypeRef` to a `Ty` read-only, using the frozen `TyCtx`'s
    /// by-name ADT table. Supports the shapes a for-loop iterable's `Self` can
    /// take: plain ADTs (`Counter`, `Range<T>`) and references (`&mut T`).
    fn resolve_type_ref_to_ty(&self, tr: &TypeRef) -> Option<Ty> {
        match tr {
            TypeRef::Path(path) => {
                let seg = path.segments.last()?;
                let name = seg.name;
                let adt_id = self.ty_ctx.adt_id_by_name(name)?;
                let substs = match &seg.generic_args {
                    Some(args) => {
                        let gen_args: Vec<GenericArg> = args
                            .iter()
                            .filter_map(|a| self.resolve_type_ref_to_ty(a).map(GenericArg::Ty))
                            .collect();
                        self.ty_ctx.intern_substitution(gen_args)
                    }
                    None => self.ty_ctx.intern_substitution(vec![]),
                };
                Some(self.ty_ctx.mk_ty(TyKind::Adt(adt_id, substs)))
            }
            TypeRef::Ref { inner, mutability } => {
                let inner_ty = self.resolve_type_ref_to_ty(inner)?;
                Some(self.ty_ctx.mk_ref(Region::Erased, inner_ty, *mutability))
            }
            _ => None,
        }
    }

    /// Structural type equality for concrete types (the for-loop iterable is
    /// always concrete). Compares by ADT identity + recursive substitution
    /// args, so a fresh `mk_ty` handle for the same logical type still matches
    /// the typeck-resolved `iter_ty` handle.
    fn ty_struct_eq(&self, a: Ty, b: Ty) -> bool {
        let ka = self.ty_ctx.ty_kind(a);
        let kb = self.ty_ctx.ty_kind(b);
        match (ka, kb) {
            (TyKind::Adt(a_id, a_sub), TyKind::Adt(b_id, b_sub)) => {
                if a_id != b_id {
                    return false;
                }
                let a_args = self.ty_ctx.substitution_args(*a_sub);
                let b_args = self.ty_ctx.substitution_args(*b_sub);
                if a_args.len() != b_args.len() {
                    return false;
                }
                a_args.iter().zip(b_args.iter()).all(|(x, y)| match (x, y) {
                    (GenericArg::Ty(xt), GenericArg::Ty(yt)) => self.ty_struct_eq(*xt, *yt),
                    _ => x == y,
                })
            }
            (TyKind::Ref(_, a_inner, a_mut), TyKind::Ref(_, b_inner, b_mut)) => {
                a_mut == b_mut && self.ty_struct_eq(*a_inner, *b_inner)
            }
            (TyKind::Slice(a_inner), TyKind::Slice(b_inner)) => {
                self.ty_struct_eq(*a_inner, *b_inner)
            }
            (TyKind::Array(a_inner, _), TyKind::Array(b_inner, _)) => {
                self.ty_struct_eq(*a_inner, *b_inner)
            }
            (TyKind::Tuple(a_sub), TyKind::Tuple(b_sub)) => {
                let a_args = self.ty_ctx.substitution_args(*a_sub);
                let b_args = self.ty_ctx.substitution_args(*b_sub);
                a_args.len() == b_args.len()
                    && a_args.iter().zip(b_args.iter()).all(|(x, y)| match (x, y) {
                        (GenericArg::Ty(xt), GenericArg::Ty(yt)) => self.ty_struct_eq(*xt, *yt),
                        _ => x == y,
                    })
            }
            (TyKind::Bool, TyKind::Bool)
            | (TyKind::Unit, TyKind::Unit)
            | (TyKind::Never, TyKind::Never)
            | (TyKind::Char, TyKind::Char) => true,
            (TyKind::Int(x), TyKind::Int(y)) => x == y,
            (TyKind::Uint(x), TyKind::Uint(y)) => x == y,
            (TyKind::Float(x), TyKind::Float(y)) => x == y,
            _ => a == b,
        }
    }
}

impl<'a> PipelineLowerCtx<'a> {
    /// Recursively convert a `ConstValue` into a `MirConst`, deriving each
    /// element's type from the value's declared `Ty` (`ty`):
    /// - tuple element types come from `TyKind::Tuple`'s substitution;
    /// - array element types are the array's element type;
    /// - struct field types come from the ADT definition via `field_ty`.
    fn cv_const(
        &self,
        value: &glyim_const_eval::ConstValue,
        ty: glyim_type::Ty,
    ) -> Option<glyim_mir::MirConstKind> {
        use glyim_const_eval::ConstValue;
        let kind = match value {
            ConstValue::Int(v, _) => MirConstKind::Int(*v),
            ConstValue::Uint(v, _) => MirConstKind::Uint(*v),
            ConstValue::FloatBits(b, _) => MirConstKind::FloatBits(*b),
            ConstValue::Bool(b) => MirConstKind::Bool(*b),
            ConstValue::Char(c) => MirConstKind::Char(*c),
            ConstValue::String(n) => MirConstKind::String(*n),
            ConstValue::Unit => MirConstKind::Unit,
            ConstValue::Tuple(vals) => {
                let elem_tys: Vec<glyim_type::Ty> = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Tuple(substs) => self
                        .ty_ctx
                        .substitution_args(*substs)
                        .iter()
                        .filter_map(|a| match a {
                            glyim_type::GenericArg::Ty(t) => Some(*t),
                            _ => None,
                        })
                        .collect(),
                    _ => return None,
                };
                let elems = vals
                    .iter()
                    .zip(elem_tys.iter())
                    .map(|(v, &et)| {
                        Some(MirConst {
                            kind: self.cv_const(v, et)?,
                            ty: et,
                            span: Span::DUMMY,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                MirConstKind::Aggregate(elems)
            }
            ConstValue::Array(vals) => {
                let elem_ty: glyim_type::Ty = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Array(inner, _) => *inner,
                    _ => return None,
                };
                let elems = vals
                    .iter()
                    .map(|v| {
                        Some(MirConst {
                            kind: self.cv_const(v, elem_ty)?,
                            ty: elem_ty,
                            span: Span::DUMMY,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                MirConstKind::Aggregate(elems)
            }
            ConstValue::Struct(vals) => {
                let adt_id = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Adt(adt_id, _) => *adt_id,
                    _ => return None,
                };
                let elems = vals
                    .iter()
                    .enumerate()
                    .map(|(i, (_, v))| {
                        let field_ty = self.ty_ctx.field_ty(adt_id, i);
                        Some(MirConst {
                            kind: self.cv_const(v, field_ty)?,
                            ty: field_ty,
                            span: Span::DUMMY,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                MirConstKind::Aggregate(elems)
            }
            // Phase 6 (GLYIM_DESTUB_PLAN §16.x): `Range<T>` is a 2-field struct
            // `{ start: T, end: T }` at the ABI level. Fold both bounds (when
            // present) into an aggregate constant, reusing the existing
            // `MirConstKind::Aggregate` backend lowering — no new const kind
            // or backend arm needed. `inclusive` is a compile-time property of
            // the range literal, not stored in the value, so it is dropped here.
            ConstValue::Range(start, end, _inclusive) => {
                let elem_ty: glyim_type::Ty = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Adt(_, substs) => self
                        .ty_ctx
                        .substitution_args(*substs)
                        .iter()
                        .filter_map(|a| match a {
                            glyim_type::GenericArg::Ty(t) => Some(*t),
                            _ => None,
                        })
                        .next()
                        .unwrap_or_else(|| self.ty_ctx.error_ty()),
                    _ => return None,
                };
                let bound = |b: &Option<Box<ConstValue>>| -> Option<glyim_mir::MirConstKind> {
                    let v = b.as_ref()?;
                    self.cv_const(v.as_ref(), elem_ty)
                };
                let start_v = bound(start)?;
                let end_v = bound(end)?;
                MirConstKind::Aggregate(vec![
                    glyim_mir::MirConst {
                        kind: start_v,
                        ty: elem_ty,
                        span: Span::DUMMY,
                    },
                    glyim_mir::MirConst {
                        kind: end_v,
                        ty: elem_ty,
                        span: Span::DUMMY,
                    },
                ])
            }
        };
        Some(kind)
    }
}

pub(crate) struct PipelineBorrowckCtx<'a> {
    ty_ctx: &'a TyCtx,
    body: &'a Body,
}

impl<'a> PipelineBorrowckCtx<'a> {
    pub(crate) fn new(ty_ctx: &'a TyCtx, body: &'a Body) -> Self {
        PipelineBorrowckCtx { ty_ctx, body }
    }
}

impl<'a> BorrowckCtx for PipelineBorrowckCtx<'a> {
    fn ty_ctx(&self) -> &TyCtx {
        self.ty_ctx
    }

    fn local_decl(&self, idx: LocalIdx) -> &LocalDecl {
        &self.body.locals[idx]
    }

    fn local_name(&self, idx: LocalIdx) -> String {
        format!("local_{}", idx.to_raw())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_const_eval::ConstValue;
    use glyim_core::arena::IndexVec;
    use glyim_core::interner::Interner;
    use glyim_core::primitives::IntTy;
    use glyim_type::{GenericArg, TyCtxMut};

    /// Phase 6 (GLYIM_DESTUB_PLAN): `cv_const` must fold a `Range` const into a
    /// real `MirConstKind::Aggregate([start, end])` instead of falling back to
    /// `None` (which forced a zero-init `ConstRef`). This unit test drives
    /// `cv_const` directly (the public `let r: Range<i32> = 0..10;` path is
    /// currently blocked by unrelated pre-existing gaps: `const` items are not
    /// lowered in HIR, and `Range<Idx>` field access / `println!` are not wired
    /// in a bare project).
    #[test]
    fn cv_const_range_folds_to_aggregate() {
        let mut tcx = TyCtxMut::new(Interner::default());
        let i32_ty = tcx.mk_ty(TyKind::Int(IntTy::I32));
        let substs = tcx.intern_substitution(vec![GenericArg::Ty(i32_ty)]);
        // `Range<T>` is registered as ADT 1000 by the `TyCtxBuilder`-style
        // default setup; `cv_const` only reads the substitution's element type,
        // so we build the `Range<i32>` type directly without re-registering it.
        let range_ty = tcx.mk_ty(TyKind::Adt(AdtId::from_raw(1000), substs));
        let frozen = tcx.freeze();

        let hir = CrateHir {
            items: IndexVec::new(),
            bodies: IndexVec::new(),
            body_owners: IndexVec::new(),
            interner: Interner::default(),
        };

        let ctx = PipelineLowerCtx::new(&frozen, &hir, Default::default());

        let value = ConstValue::Range(
            Some(Box::new(ConstValue::Int(0, IntTy::I32))),
            Some(Box::new(ConstValue::Int(10, IntTy::I32))),
            false,
        );

        let result = ctx.cv_const(&value, range_ty);
        match result {
            Some(MirConstKind::Aggregate(elems)) => {
                assert_eq!(elems.len(), 2, "range aggregate must have 2 fields");
                match (&elems[0].kind, &elems[1].kind) {
                    (MirConstKind::Int(start), MirConstKind::Int(end)) => {
                        assert_eq!(*start, 0, "range start must fold to 0");
                        assert_eq!(*end, 10, "range end must fold to 10");
                    }
                    other => panic!("range fields must be Int, got {other:?}"),
                }
            }
            other => panic!("expected Aggregate([Int(0), Int(10)]), got {other:?}"),
        }
    }
}
