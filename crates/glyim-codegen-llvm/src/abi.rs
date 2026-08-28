use glyim_core::primitives::TargetInfo;
use glyim_layout::{
    Align, ArgAbi, CallConvention, FieldsShape, FnAbi, Layout, LayoutComputer, LayoutError,
    PassMode, SimpleLayoutComputer, Size, VariantsShape,
};
use glyim_type::{Ty, TyCtx, TyKind};

pub(crate) struct FullLayoutComputer<'a> {
    simple: SimpleLayoutComputer<'a>,
    ctx: &'a TyCtx,
}

impl<'a> FullLayoutComputer<'a> {
    pub fn new(ctx: &'a TyCtx, target: TargetInfo) -> Self {
        Self {
            simple: SimpleLayoutComputer::new(ctx, target),
            ctx,
        }
    }

    fn classify_arg(&self, ty: Ty, layout: &Layout) -> PassMode {
        let size = layout.size.0;
        if size == 0 {
            return PassMode::Ignore;
        }

        let kind = self.ctx.ty_kind(ty);
        let is_scalar = matches!(
            kind,
            TyKind::Bool
                | TyKind::Int(_)
                | TyKind::Uint(_)
                | TyKind::Float(_)
                | TyKind::Char
                | TyKind::Ref(..)
                | TyKind::RawPtr(..)
                | TyKind::FnPtr(_)
                | TyKind::FnDef(..)
        );

        if is_scalar {
            return PassMode::Direct;
        }

        // For aggregates, we need to consider the target ABI.
        let target_abi = self.simple.target_info().abi;

        match target_abi {
            glyim_core::primitives::TargetAbi::X86_64SystemV => {
                if size <= 16 && layout.align.0 <= 8 {
                    PassMode::Direct
                } else {
                    PassMode::Indirect { meta_attrs: false }
                }
            }
            glyim_core::primitives::TargetAbi::AArch64AAPCS => {
                if size <= 16 && layout.align.0 <= 8 {
                    PassMode::Direct
                } else {
                    PassMode::Indirect { meta_attrs: false }
                }
            }
            glyim_core::primitives::TargetAbi::X86_64Windows => {
                if size <= 8 && layout.align.0 <= 8 {
                    PassMode::Direct
                } else {
                    PassMode::Indirect { meta_attrs: false }
                }
            }
            glyim_core::primitives::TargetAbi::AArch64Windows => {
                if size <= 8 && layout.align.0 <= 8 {
                    PassMode::Direct
                } else {
                    PassMode::Indirect { meta_attrs: false }
                }
            }
            glyim_core::primitives::TargetAbi::Wasm32 => PassMode::Direct,
        }
    }
}

impl LayoutComputer for FullLayoutComputer<'_> {
    fn layout_of(&self, ty: Ty) -> Result<Layout, LayoutError> {
        match self.ctx.ty_kind(ty) {
            TyKind::Tuple(subst) => {
                let args = self.ctx.substitution_args(*subst);
                if args.is_empty() {
                    return Ok(Layout::unit());
                }
                let mut field_layouts = Vec::new();
                for arg in args {
                    if let glyim_type::GenericArg::Ty(t) = arg {
                        field_layouts.push(self.layout_of(*t)?);
                    }
                }
                if field_layouts.is_empty() {
                    return Ok(Layout::unit());
                }
                let mut size = Size::ZERO;
                let mut align = Align::ONE;
                let mut offsets = glyim_core::arena::IndexVec::new();
                for layout in &field_layouts {
                    let offset = size.align_to(layout.align);
                    offsets.push(offset);
                    size = offset + layout.size;
                    align = align.max(layout.align);
                }
                size = size.align_to(align);
                Ok(Layout {
                    size,
                    align,
                    fields: FieldsShape::Arbitrary { offsets },
                    variants: VariantsShape::Single { index: 0 },
                    is_unsized: false,
                })
            }
            TyKind::Array(elem, count) => {
                let elem_ty = *elem;
                let count = match &count.kind {
                    glyim_type::ConstKind::Uint(n) => *n as u64,
                    glyim_type::ConstKind::Int(n) => *n as u64,
                    _ => return Err(LayoutError::UnknownType(ty)),
                };
                let elem_layout = self.layout_of(elem_ty)?;
                let stride = elem_layout.size.align_to(elem_layout.align);
                let size = Size(stride.0.saturating_mul(count));
                Ok(Layout {
                    size,
                    align: elem_layout.align,
                    fields: FieldsShape::Array { stride, count },
                    variants: VariantsShape::Single { index: 0 },
                    is_unsized: false,
                })
            }
            TyKind::Adt(adt_id, substs) => {
                let substs = self.ctx.substitution_args(*substs);
                if let Some(adt_def) = self.ctx.adt_def(*adt_id) {
                    if adt_def.variants.len() > 1 {
                        let mut variant_layouts = Vec::with_capacity(adt_def.variants.len());
                        for variant in &adt_def.variants {
                            let mut field_layouts = Vec::with_capacity(variant.fields.len());
                            for field in variant.fields.iter() {
                                field_layouts.push(self.layout_of(self.subst_params(field.ty, substs))?);
                            }
                            let mut size = Size::ZERO;
                            let mut align = Align::ONE;
                            let mut offsets = glyim_core::arena::IndexVec::new();
                            for layout in &field_layouts {
                                let offset = size.align_to(layout.align);
                                offsets.push(offset);
                                size = offset + layout.size;
                                align = align.max(layout.align);
                            }
                            size = size.align_to(align);
                            variant_layouts.push(Layout {
                                size,
                                align,
                                fields: FieldsShape::Arbitrary { offsets },
                                variants: VariantsShape::Single { index: 0 },
                                is_unsized: false,
                            });
                        }

                        let max_size = variant_layouts.iter().map(|l| l.size.0).max().unwrap_or(0);
                        let max_align =
                            variant_layouts.iter().map(|l| l.align.0).max().unwrap_or(1);
                        let max_align = Align::from_bytes(max_align);

                        let n_variants = adt_def.variants.len() as u64;
                        let tag_bits = if n_variants <= 1 {
                            1
                        } else {
                            64 - (n_variants - 1).leading_zeros()
                        };
                        let tag_size_bytes = tag_bits.div_ceil(8) as u64;
                        let tag_size = Size(tag_size_bytes);
                        let tag_align = Align::from_bytes(tag_size_bytes);

                        let tag_ty = if n_variants <= 256 {
                            glyim_type::Ty::U8
                        } else if n_variants <= 65536 {
                            glyim_type::Ty::U16
                        } else {
                            glyim_type::Ty::U32
                        };

                        let data_start = tag_size.align_to(tag_align);
                        let mut untagged_offsets = glyim_core::arena::IndexVec::new();
                        if let Some(layout) = variant_layouts.first()
                            && let FieldsShape::Arbitrary { offsets } = &layout.fields
                        {
                            for offset in offsets.iter() {
                                untagged_offsets.push(*offset + data_start);
                            }
                        }

                        let total_size = max_size + data_start.0;
                        let total_size = Size(total_size).align_to(max_align);

                        let variants_shape = VariantsShape::Multiple {
                            tag: tag_ty,
                            tag_field: 0,
                            tag_encoding: glyim_layout::TagEncoding::Direct,
                            tag_size,
                            tag_align,
                            variants: variant_layouts,
                        };

                        return Ok(Layout {
                            size: total_size,
                            align: max_align,
                            fields: FieldsShape::Arbitrary {
                                offsets: untagged_offsets,
                            },
                            variants: variants_shape,
                            is_unsized: false,
                        });
                    } else if let Some(variant) = adt_def.variants.first() {
                        let mut field_layouts = Vec::with_capacity(variant.fields.len());
                        for field in variant.fields.iter() {
                            field_layouts.push(self.layout_of(self.subst_params(field.ty, substs))?);
                        }
                        if field_layouts.is_empty() {
                            return Ok(Layout::unit());
                        }
                        let mut size = Size::ZERO;
                        let mut align = Align::ONE;
                        let mut offsets = glyim_core::arena::IndexVec::new();
                        for layout in &field_layouts {
                            let offset = size.align_to(layout.align);
                            offsets.push(offset);
                            size = offset + layout.size;
                            align = align.max(layout.align);
                        }
                        size = size.align_to(align);
                        return Ok(Layout {
                            size,
                            align,
                            fields: FieldsShape::Arbitrary { offsets },
                            variants: VariantsShape::Single { index: 0 },
                            is_unsized: false,
                        });
                    }
                }
                self.simple.layout_of(ty)
            }
            TyKind::Projection(proj) => {
                let self_ty = self
                    .ctx
                    .substitution_args(proj.trait_ref.substs)
                    .first()
                    .and_then(|a| match a {
                        glyim_type::GenericArg::Ty(t) => Some(*t),
                        _ => None,
                    });
                if let Some(self_ty) = self_ty {
                    if let Some(resolved) =
                        self.ctx.resolve_associated_type(
                            self_ty,
                            proj.trait_ref.def_id,
                            proj.item_name,
                        )
                    {
                        return self.layout_of(resolved);
                    }
                }
                self.simple.layout_of(ty)
            }
            _ => self.simple.layout_of(ty),
        }
    }

    fn fn_abi_of(&self, sig: &glyim_type::FnSig) -> Result<FnAbi, LayoutError> {
        let ret_layout = self.layout_of(sig.output)?;
        let ret_mode = self.classify_arg(sig.output, &ret_layout);

        let args = self.ctx.substitution_args(sig.inputs);
        let mut arg_abis = Vec::with_capacity(args.len());
        for arg in args {
            if let glyim_type::GenericArg::Ty(t) = arg {
                let layout = self.layout_of(*t)?;
                let mode = self.classify_arg(*t, &layout);
                arg_abis.push(ArgAbi {
                    ty: *t,
                    layout,
                    mode,
                });
            }
        }
        Ok(FnAbi {
            args: arg_abis,
            ret: ArgAbi {
                ty: sig.output,
                layout: ret_layout,
                mode: ret_mode,
            },
            conv: CallConvention::from(sig.abi),
            c_variadic: sig.c_variadic,
        })
    }

    fn ptr_size(&self) -> Size {
        self.simple.ptr_size()
    }
    fn ptr_align(&self) -> Align {
        self.simple.ptr_align()
    }
    fn target_info(&self) -> &TargetInfo {
        self.simple.target_info()
    }
}

/// Recursively substitute generic `Param(i)` with `substs[i]` so that a
/// monomorphized ADT whose field/variant types still reference the generic
/// params (e.g. `Poll<T>`'s `Ready(T)` variant) lays out correctly instead of
/// ICEing on a bare `Param`. Shared with `SimpleLayoutComputer::subst_params`.
impl<'a> FullLayoutComputer<'a> {
    fn subst_params(&self, ty: Ty, substs: &[glyim_type::GenericArg]) -> Ty {
        if substs.is_empty() {
            return ty;
        }
        match self.ctx.ty_kind(ty) {
            TyKind::Param(p) => match substs.get(p.index as usize) {
                Some(glyim_type::GenericArg::Ty(t)) => *t,
                _ => ty,
            },
            TyKind::Ref(region, inner, mutbl) => {
                let new_inner = self.subst_params(*inner, substs);
                self.ctx.mk_ty(TyKind::Ref(region.clone(), new_inner, *mutbl))
            }
            TyKind::RawPtr(inner, mutbl) => {
                let new_inner = self.subst_params(*inner, substs);
                self.ctx.mk_ty(TyKind::RawPtr(new_inner, *mutbl))
            }
            TyKind::Adt(adt_id, s) => {
                let new_args: Vec<glyim_type::GenericArg> = self
                    .ctx
                    .substitution_args(*s)
                    .iter()
                    .map(|a| match a {
                        glyim_type::GenericArg::Ty(t) => {
                            glyim_type::GenericArg::Ty(self.subst_params(*t, substs))
                        }
                        other => other.clone(),
                    })
                    .collect();
                let new_s = self.ctx.intern_substitution(new_args);
                self.ctx.mk_ty(TyKind::Adt(*adt_id, new_s))
            }
            TyKind::Tuple(s) => {
                let new_args: Vec<glyim_type::GenericArg> = self
                    .ctx
                    .substitution_args(*s)
                    .iter()
                    .map(|a| match a {
                        glyim_type::GenericArg::Ty(t) => {
                            glyim_type::GenericArg::Ty(self.subst_params(*t, substs))
                        }
                        other => other.clone(),
                    })
                    .collect();
                let new_s = self.ctx.intern_substitution(new_args);
                self.ctx.mk_ty(TyKind::Tuple(new_s))
            }
            _ => ty,
        }
    }
}

#[cfg(test)]
mod abi_tests {
    use super::*;
    use glyim_core::interner::Interner;
    use glyim_core::primitives::TargetInfo;
    use glyim_type::{Ty, TyCtxMut, TyKind};

    // Shared test context.
    

    fn classify_with_ctx(ctx: &TyCtx, ty: Ty, target: TargetInfo) -> PassMode {
        let computer = FullLayoutComputer::new(ctx, target);
        let layout = computer.layout_of(ty).unwrap();
        computer.classify_arg(ty, &layout)
    }

    // Helper to create a target.
    fn target_for_abi(abi: glyim_core::primitives::TargetAbi) -> TargetInfo {
        let triple = match abi {
            glyim_core::primitives::TargetAbi::X86_64SystemV => "x86_64-unknown-linux-gnu",
            glyim_core::primitives::TargetAbi::AArch64AAPCS => "aarch64-unknown-linux-gnu",
            glyim_core::primitives::TargetAbi::X86_64Windows => "x86_64-pc-windows-msvc",
            glyim_core::primitives::TargetAbi::AArch64Windows => "aarch64-pc-windows-msvc",
            glyim_core::primitives::TargetAbi::Wasm32 => "wasm32-unknown-unknown",
        };
        TargetInfo::from_triple(triple)
    }

    

    #[test]
    fn test_classify_scalar_i32() {
        let ctx_mut = TyCtxMut::new(Interner::new());
        let ty = Ty::I32;
        let ctx = ctx_mut.freeze();
        let target = target_for_abi(glyim_core::primitives::TargetAbi::X86_64SystemV);
        let mode = classify_with_ctx(&ctx, ty, target);
        assert_eq!(mode, PassMode::Direct);
    }

    #[test]
    fn test_classify_struct_8_bytes() {
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let struct_ty = ctx_mut.mk_ty(TyKind::Tuple(substs));
        let ctx = ctx_mut.freeze();
        let target = target_for_abi(glyim_core::primitives::TargetAbi::X86_64SystemV);
        let mode = classify_with_ctx(&ctx, struct_ty, target);
        assert_eq!(mode, PassMode::Direct);
    }

    #[test]
    fn test_classify_struct_16_bytes() {
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let struct_ty = ctx_mut.mk_ty(TyKind::Tuple(substs));
        let ctx = ctx_mut.freeze();
        let target = target_for_abi(glyim_core::primitives::TargetAbi::X86_64SystemV);
        let mode = classify_with_ctx(&ctx, struct_ty, target);
        assert_eq!(mode, PassMode::Direct);
    }

    #[test]
    fn test_classify_struct_24_bytes() {
        let mut ctx_mut = TyCtxMut::new(Interner::new());
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::primitives::IntTy::I32));
        let substs = ctx_mut.intern_substitution(vec![
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
            glyim_type::GenericArg::Ty(i32_ty),
        ]);
        let struct_ty = ctx_mut.mk_ty(TyKind::Tuple(substs));
        let ctx = ctx_mut.freeze();
        let target = target_for_abi(glyim_core::primitives::TargetAbi::X86_64SystemV);
        let mode = classify_with_ctx(&ctx, struct_ty, target);
        assert_eq!(mode, PassMode::Indirect { meta_attrs: false });
    }
}
