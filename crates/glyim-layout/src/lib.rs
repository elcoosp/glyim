//! Type layout computation — sizes, alignments, ABI details.

use glyim_core::abi::ALIGN_MAX;
use glyim_core::arena::IndexVec;
use glyim_core::def_id::TraitDefId;
use glyim_core::primitives::{Abi, TargetInfo};
use glyim_type::adt_def::{AdtDef, AdtKind};
use glyim_type::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Size.
pub struct Size(pub u64);

impl Size {
/// ZERO.
    pub const ZERO: Size = Size(0);
/// bytes.
    pub fn bytes(b: u64) -> Self {
        Size(b)
    }
/// bits.
    pub fn bits(&self) -> u64 {
        self.0.saturating_mul(8)
    }
/// align_to.
    pub fn align_to(&self, align: Align) -> Self {
        debug_assert!(align.0 > 0, "alignment must be non-zero");
        let mask = align.0 - 1;
        Size((self.0 + mask) & !mask)
    }
/// checked_mul.
    pub fn checked_mul(self, rhs: u64) -> Option<Size> {
        self.0.checked_mul(rhs).map(Size)
    }
}

impl std::ops::Add for Size {
    type Output = Size;
    fn add(self, rhs: Size) -> Size {
        Size(self.0.saturating_add(rhs.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
/// Align.
pub struct Align(pub u64);

impl Align {
/// ONE.
    pub const ONE: Align = Align(1);
/// EIGHT.
    pub const EIGHT: Align = Align(8);
/// from_bytes.
    pub fn from_bytes(bytes: u64) -> Self {
        debug_assert!(
            bytes.is_power_of_two(),
            "alignment must be a power of two, got {bytes}"
        );
        Align(bytes)
    }
/// max.
    pub fn max(self, other: Self) -> Self {
        Align(self.0.max(other.0))
    }
}

#[derive(Clone, Debug)]
/// Layout.
pub struct Layout {
/// Struct.
    pub size: Size,
/// Struct.
    pub align: Align,
/// Struct.
    pub fields: FieldsShape,
/// Struct.
    pub variants: VariantsShape,
/// Struct.
    pub is_unsized: bool,
}

impl Layout {
/// scalar.
    pub fn scalar(size: Size, align: Align) -> Self {
        Self {
            size,
            align,
            fields: FieldsShape::Primitive,
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        }
    }
/// unit.
    pub fn unit() -> Self {
        Self {
            size: Size::ZERO,
            align: Align::ONE,
            fields: FieldsShape::Arbitrary {
                offsets: IndexVec::new(),
            },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        }
    }
}

#[derive(Clone, Debug)]
/// FieldsShape.
pub enum FieldsShape {
/// Variant.
    Primitive,
/// Variant.
    Array {
        /// stride field.
        stride: Size,
        /// count field.
        count: u64,
    },
/// Struct.
    Arbitrary {
        /// offsets field.
        offsets: IndexVec<FieldIdx, Size>,
    },
}

#[derive(Clone, Debug)]
/// VariantsShape.
pub enum VariantsShape {
/// Variant.
    Single {
/// Struct.
        index: u32,
    },
/// Variant.
    Multiple {
/// Struct.
        tag: Ty,
/// Struct.
        tag_field: u32,
/// Struct.
        tag_encoding: TagEncoding,
/// Struct.
        variants: Vec<Layout>,
/// Struct.
        tag_size: Size,   // ← ADDED
/// Struct.
        tag_align: Align, // ← ADDED
    },
}

#[derive(Clone, Debug)]
/// TagEncoding.
pub enum TagEncoding {
/// Variant.
    Direct,
/// Variant.
    Niche {
/// Struct.
        untagged_variant: u32,
/// Struct.
        niche_variants: std::ops::RangeInclusive<u32>,
/// Struct.
        niche_start: u128,
    },
}

#[derive(Clone, Debug)]
/// FnAbi.
pub struct FnAbi {
/// Struct.
    pub args: Vec<ArgAbi>,
/// Struct.
    pub ret: ArgAbi,
/// Struct.
    pub conv: CallConvention,
/// Struct.
    pub c_variadic: bool,
}

#[derive(Clone, Debug)]
/// ArgAbi.
pub struct ArgAbi {
/// Struct.
    pub ty: Ty,
/// Struct.
    pub layout: Layout,
/// Struct.
    pub mode: PassMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// PassMode.
pub enum PassMode {
/// Variant.
    Direct,
/// Variant.
    Indirect {
        /// meta_attrs field.
        meta_attrs: bool,
    },
/// Variant.
    Ignore,
/// Struct.
    Cast {
        /// to field.
        to: Ty,
        /// cast_int field.
        cast_int: bool,
    },
/// Struct.
    HomogeneousAggregate {
        /// element_ty field.
        element_ty: Ty,
        /// count field.
        count: u32,
    },
/// Variant.
    Split {
        /// pieces field.
        pieces: Vec<PassMode>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// CallConvention.
pub enum CallConvention {
/// Variant.
    Glyim,
/// Variant.
    C,
/// Variant.
    System,
}

impl From<Abi> for CallConvention {
    fn from(abi: Abi) -> Self {
        match abi {
            Abi::C => CallConvention::C,
            Abi::Glyim => CallConvention::Glyim,
            Abi::System => CallConvention::System,
        }
    }
}

/// LayoutComputer.
pub trait LayoutComputer {
/// layout_of.
    fn layout_of(&self, ty: Ty) -> Result<Layout, LayoutError>;
/// fn_abi_of.
    fn fn_abi_of(&self, sig: &FnSig) -> Result<FnAbi, LayoutError>;
/// ptr_size.
    fn ptr_size(&self) -> Size;
/// ptr_align.
    fn ptr_align(&self) -> Align;
/// target_info.
    fn target_info(&self) -> &TargetInfo;
}

#[derive(Clone, Debug)]
/// LayoutError.
pub enum LayoutError {
#[allow(missing_docs)]
    UnknownType(Ty),
#[allow(missing_docs)]
    SizeOverflow(Ty),
#[allow(missing_docs)]
    Unsized(Ty),
#[allow(missing_docs)]
    Cycle(Ty),
/// Struct.
    AlignmentExceedsRuntime {
        /// ty field.
        ty: Ty,
        /// align field.
        align: u64,
        /// max field.
        max: u64,
    },
    /// A trait object's trait definition could not be resolved during vtable
    /// construction. This indicates a pipeline-ordering bug: an unresolvable
    /// trait should have been caught by typeck's object-safety check long
    /// before layout runs, so layout surfaces it as a hard error rather than
    /// emitting a silently-empty vtable that would miscall through null slots.
    UnknownTrait(TraitDefId),
}

/// SimpleLayoutComputer.
pub struct SimpleLayoutComputer<'a> {
    ctx: &'a TyCtx,
    target: TargetInfo,
}

impl<'a> SimpleLayoutComputer<'a> {
/// new.
    pub fn new(ctx: &'a TyCtx, target: TargetInfo) -> Self {
        const _: () = assert!(ALIGN_MAX >= 8, "ALIGN_MAX must be at least 8");
        Self { ctx, target }
    }

    fn layout_tuple(&self, substs: Substitution) -> Result<Layout, LayoutError> {
        let args = self.ctx.substitution_args(substs);
        if args.is_empty() {
            return Ok(Layout::unit());
        }
        let mut offsets = IndexVec::new();
        let mut struct_align = Align::ONE;
        let mut current_offset = Size::ZERO;
        for arg in args {
            if let GenericArg::Ty(field_ty) = arg {
                let field_layout = self.layout_of(*field_ty)?;
                struct_align = struct_align.max(field_layout.align);
                current_offset = current_offset.align_to(field_layout.align);
                offsets.push(current_offset);
                current_offset = current_offset + field_layout.size;
            }
        }
        let size = current_offset.align_to(struct_align);
        Ok(Layout {
            size,
            align: struct_align,
            fields: FieldsShape::Arbitrary { offsets },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        })
    }

    fn layout_array(&self, inner: Ty, count: &Const, outer_ty: Ty) -> Result<Layout, LayoutError> {
        let inner_layout = self.layout_of(inner)?;
        let count_val: u64 = match &count.kind {
            ConstKind::Uint(n) => {
                u64::try_from(*n).map_err(|_| LayoutError::SizeOverflow(outer_ty))?
            }
            ConstKind::Int(n) => {
                if *n < 0 {
                    return Err(LayoutError::SizeOverflow(outer_ty));
                }
                u64::try_from(*n).map_err(|_| LayoutError::SizeOverflow(outer_ty))?
            }
            _ => return Err(LayoutError::UnknownType(outer_ty)),
        };
        let stride = inner_layout.size.align_to(inner_layout.align);
        let size = stride
            .checked_mul(count_val)
            .ok_or(LayoutError::SizeOverflow(outer_ty))?;
        Ok(Layout {
            size,
            align: inner_layout.align,
            fields: FieldsShape::Array {
                stride,
                count: count_val,
            },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        })
    }

    fn layout_struct(&self, adt_def: &AdtDef, substs: &[GenericArg]) -> Result<Layout, LayoutError> {
        let mut offsets = IndexVec::new();
        let mut struct_align = Align::ONE;
        let mut current_offset = Size::ZERO;
        for field in adt_def.fields.iter() {
            let field_layout = self.layout_of(self.subst_params(field.ty, substs))?;
            struct_align = struct_align.max(field_layout.align);
            current_offset = current_offset.align_to(field_layout.align);
            offsets.push(current_offset);
            current_offset = current_offset + field_layout.size;
        }
        let size = current_offset.align_to(struct_align);
        Ok(Layout {
            size,
            align: struct_align,
            fields: FieldsShape::Arbitrary { offsets },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        })
    }

    fn layout_union(&self, adt_def: &AdtDef, substs: &[GenericArg]) -> Result<Layout, LayoutError> {
        let mut union_size = Size::ZERO;
        let mut union_align = Align::ONE;
        let mut offsets = IndexVec::new();
        for field in adt_def.fields.iter() {
            let field_layout = self.layout_of(self.subst_params(field.ty, substs))?;
            union_align = union_align.max(field_layout.align);
            union_size = union_size.max(field_layout.size);
            offsets.push(Size::ZERO);
        }
        let size = union_size.align_to(union_align);
        Ok(Layout {
            size,
            align: union_align,
            fields: FieldsShape::Arbitrary { offsets },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        })
    }

    fn layout_enum(&self, adt_def: &AdtDef, outer_ty: Ty, substs: &[GenericArg]) -> Result<Layout, LayoutError> {
        let variant_count = adt_def.variants.len();
        if variant_count == 0 {
            return Err(LayoutError::UnknownType(outer_ty));
        }
        if variant_count == 1 {
            return self.layout_single_variant_enum(adt_def, substs);
        }
        let variant_layouts: Vec<Layout> = adt_def
            .variants
            .iter()
            .map(|v| self.layout_variant_data(v, substs))
            .collect::<Result<Vec<_>, LayoutError>>()?;
        if let Some(result) = self.try_niche_encoding(adt_def, &variant_layouts)? {
            return Ok(result);
        }
        self.direct_tag_encoding(adt_def, &variant_layouts)
    }

    fn layout_single_variant_enum(&self, adt_def: &AdtDef, substs: &[GenericArg]) -> Result<Layout, LayoutError> {
        let variant_fields = &adt_def.variants[0].fields;
        let mut offsets = IndexVec::new();
        let mut enum_align = Align::ONE;
        let mut current_offset = Size::ZERO;
        for field in variant_fields.iter() {
            let field_layout = self.layout_of(self.subst_params(field.ty, substs))?;
            enum_align = enum_align.max(field_layout.align);
            current_offset = current_offset.align_to(field_layout.align);
            offsets.push(current_offset);
            current_offset = current_offset + field_layout.size;
        }
        let size = current_offset.align_to(enum_align);
        Ok(Layout {
            size,
            align: enum_align,
            fields: FieldsShape::Arbitrary { offsets },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        })
    }

    fn layout_variant_data(
        &self,
        variant: &glyim_type::adt_def::VariantDef,
        substs: &[GenericArg],
    ) -> Result<Layout, LayoutError> {
        let mut offsets = IndexVec::new();
        let mut var_align = Align::ONE;
        let mut current_offset = Size::ZERO;
        for field in variant.fields.iter() {
            let field_layout = self.layout_of(self.subst_params(field.ty, substs))?;
            var_align = var_align.max(field_layout.align);
            current_offset = current_offset.align_to(field_layout.align);
            offsets.push(current_offset);
            current_offset = current_offset + field_layout.size;
        }
        let size = current_offset.align_to(var_align);
        Ok(Layout {
            size,
            align: var_align,
            fields: FieldsShape::Arbitrary { offsets },
            variants: VariantsShape::Single { index: 0 },
            is_unsized: false,
        })
    }

    fn try_niche_encoding(
        &self,
        adt_def: &AdtDef,
        variant_layouts: &[Layout],
    ) -> Result<Option<Layout>, LayoutError> {
        let variant_count = adt_def.variants.len();
        let niche_variants_needed =
            u128::try_from(variant_count.saturating_sub(1)).unwrap_or(u128::MAX);
        for (vi, variant) in adt_def.variants.iter().enumerate() {
            for (fi, field) in variant.fields.iter().enumerate() {
                if let Some((niche_start, niche_count)) = self.niche_info(field.ty)
                    && niche_count >= niche_variants_needed
                {
                    return Ok(Some(self.build_niche_layout(
                        variant_layouts,
                        vi,
                        fi,
                        field.ty,
                        niche_start,
                        niche_count,
                        variant_count,
                    )?));
                }
            }
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn build_niche_layout(
        &self,
        variant_layouts: &[Layout],
        niche_variant_idx: usize,
        niche_field_idx: usize,
        niche_field_ty: Ty,
        niche_start: u128,
        _niche_count: u128,
        variant_count: usize,
    ) -> Result<Layout, LayoutError> {
        let data_layout = &variant_layouts[niche_variant_idx];
        let niche_variants = if niche_variant_idx == 0 {
            1..=u32::try_from(variant_count - 1).unwrap_or(u32::MAX)
        } else {
            0..=u32::try_from(niche_variant_idx - 1).unwrap_or(0)
        };
        let mut max_size = Size::ZERO;
        let mut max_align = Align::ONE;
        for vl in variant_layouts {
            max_size = max_size.max(vl.size);
            max_align = max_align.max(vl.align);
        }
        let size = max_size.align_to(max_align);
        let output_variants: Vec<Layout> = variant_layouts.to_vec();
        let niche_layout = self.layout_of(niche_field_ty)?; // ← Compute tag size/align
        Ok(Layout {
            size,
            align: max_align,
            fields: FieldsShape::Arbitrary {
                offsets: match &data_layout.fields {
                    FieldsShape::Arbitrary { offsets } => offsets.clone(),
                    FieldsShape::Primitive => {
                        let mut o = IndexVec::new();
                        o.push(Size::ZERO);
                        o
                    }
                    FieldsShape::Array { stride, count } => {
                        let mut o = IndexVec::new();
                        let mut off = Size::ZERO;
                        for _ in 0..*count {
                            o.push(off);
                            off = off + *stride;
                        }
                        o
                    }
                },
            },
            variants: VariantsShape::Multiple {
                tag: niche_field_ty,
                tag_field: u32::try_from(niche_field_idx).unwrap_or(0),
                tag_encoding: TagEncoding::Niche {
                    untagged_variant: u32::try_from(niche_variant_idx).unwrap_or(0),
                    niche_variants,
                    niche_start,
                },
                variants: output_variants,
                tag_size: niche_layout.size,   // ← ADDED
                tag_align: niche_layout.align, // ← ADDED
            },
            is_unsized: false,
        })
    }

    fn niche_info(&self, ty: Ty) -> Option<(u128, u128)> {
        self.niche_info_inner(ty, 0)
    }

    /// Available niche of `ty`: the `(start, count)` of bit-patterns `ty` provably
    /// *cannot* represent, which an enclosing enum may reuse as its discriminant niche.
    ///
    /// Built recursively (depth-guarded): a nested enum whose own layout already
    /// niche-encodes exposes its *free* discriminant values, which is what makes
    /// `Option<Option<bool>>` collapse to a single byte.
    fn niche_info_inner(&self, ty: Ty, depth: usize) -> Option<(u128, u128)> {
        if depth >= 8 {
            return None;
        }
        match self.ctx.ty_kind(ty) {
            TyKind::Bool => Some((2, 254)),
            TyKind::Ref(_, _, _) | TyKind::RawPtr(_, _) => Some((0, 1)),
            TyKind::Int(int_ty) => {
                let bw = int_ty.bit_width(&self.target);
                match bw {
                    8 => Some((0x80, 1)),
                    16 => Some((0x8000, 1)),
                    32 => Some((0x8000_0000, 1)),
                    64 => Some((0x8000_0000_0000_0000, 1)),
                    _ => None,
                }
            }
            TyKind::Char => Some((0x110000, u128::MAX - 0x110000 + 1)),
            TyKind::Adt(adt_id, _) => {
                let adt_def = self.ctx.adt_def(*adt_id)?;
                if adt_def.kind != AdtKind::Enum {
                    return None;
                }
                let layout = self.layout_of(ty).ok()?;
                let VariantsShape::Multiple { tag, tag_encoding, .. } = &layout.variants else {
                    return None;
                };
                let full = self.tag_value_count(*tag)?;
                // Sound available niche: every used discriminant value is strictly
                // below `first_free`, so the tail `[first_free, full)` is provably
                // unused and safe for an enclosing enum to reuse.
                let (start, count) = match tag_encoding {
                    TagEncoding::Direct => {
                        let used = adt_def.variants.len() as u128;
                        if full > used {
                            (used, full - used)
                        } else {
                            return None;
                        }
                    }
                    TagEncoding::Niche { niche_variants, .. } => {
                        let sv_start = *niche_variants.start();
                        let sv_end = *niche_variants.end();
                        if sv_start > 0 {
                            // Free contiguous block `[0, sv_start)`.
                            (0, sv_start as u128)
                        } else {
                            // Free contiguous block `[sv_end + 1, niche_start)`.
                            let start = (sv_end as u128) + 1;
                            let count = full.saturating_sub(start);
                            (start, count)
                        }
                    }
                };
                if count > 0 {
                    Some((start, count))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Number of distinct values the discriminant `tag` type can hold.
    fn tag_value_count(&self, tag_ty: Ty) -> Option<u128> {
        match self.ctx.ty_kind(tag_ty) {
            TyKind::Bool => Some(256),
            TyKind::Int(i) => {
                let bw = i.bit_width(&self.target) as u128;
                1u128.checked_shl(bw as u32)
            }
            TyKind::Uint(i) => {
                let bw = i.bit_width(&self.target) as u128;
                1u128.checked_shl(bw as u32)
            }
            _ => None,
        }
    }

    /// Discriminant width selection (de-stubbing plan §7.1 / §13.1).
    ///
    /// Previously this always returned `bool_ty()`, which is wrong for any enum with
    /// more than two variants: an `i64`-tagged enum or a >256-variant enum needs a
    /// real integer tag whose width covers the variant count. `glyim-codegen-llvm`
    /// already emits `U8`/`U16`/`U32` for the tag (see `abi.rs`), so this must agree
    /// with that scheme or the codegen-side tag type would silently disagree with the
    /// layout-side tag type — a latent ABI mismatch. We therefore pick the smallest
    /// unsigned integer whose value range covers `variant_count` discriminants.
    fn discriminant_info(&self, variant_count: usize) -> (Size, Align, Ty) {
        if variant_count <= 256 {
            (Size::bytes(1), Align::ONE, Ty::U8)
        } else if variant_count <= 65_536 {
            (Size::bytes(2), Align::from_bytes(2), Ty::U16)
        } else if variant_count <= 4_294_967_296 {
            (Size::bytes(4), Align::from_bytes(4), Ty::U32)
        } else {
            (Size::bytes(8), Align::EIGHT, Ty::U64)
        }
    }

    fn direct_tag_encoding(
        &self,
        adt_def: &AdtDef,
        variant_layouts: &[Layout],
    ) -> Result<Layout, LayoutError> {
        let variant_count = adt_def.variants.len();
        let (tag_size, tag_align, tag_ty) = self.discriminant_info(variant_count);
        let mut max_variant_size = Size::ZERO;
        let mut overall_align = tag_align;
        let mut tagged_variant_layouts: Vec<Layout> = Vec::with_capacity(variant_count);
        for variant_data in variant_layouts {
            overall_align = overall_align.max(variant_data.align);
            let data_start = tag_size.align_to(variant_data.align);
            let variant_end = data_start + variant_data.size;
            max_variant_size = max_variant_size.max(variant_end);
            tagged_variant_layouts.push(variant_data.clone());
        }
        let size = max_variant_size.align_to(overall_align);
        let mut offsets = IndexVec::new();
        offsets.push(Size::ZERO);
        let data_start = tag_size.align_to(
            variant_layouts
                .iter()
                .map(|v| v.align)
                .max()
                .unwrap_or(Align::ONE),
        );
        offsets.push(data_start);
        Ok(Layout {
            size,
            align: overall_align,
            fields: FieldsShape::Arbitrary { offsets },
            variants: VariantsShape::Multiple {
                tag: tag_ty,
                tag_field: 0,
                tag_encoding: TagEncoding::Direct,
                variants: tagged_variant_layouts,
                tag_size,  // ← ADDED
                tag_align, // ← ADDED
            },
            is_unsized: false,
        })
    }

    /// Substitute generic `Param(i)` types in `ty` with the corresponding
    /// entry of `substs` (used when laying out a monomorphized ADT whose
    /// field/variant types still reference the generic params, e.g. the
    /// `T` in `Poll<T>`'s `Ready(T)` variant). Without this, `layout_of`
    /// reaches a bare `Param` and ICEs. The substitution is recursive over
    /// `Ref`/`RawPtr`/`Adt`/`Tuple` so nested generics resolve too.
    fn subst_params(&self, ty: Ty, substs: &[GenericArg]) -> Ty {
        if substs.is_empty() {
            return ty;
        }
        match self.ctx.ty_kind(ty) {
            TyKind::Param(p) => match substs.get(p.index as usize) {
                Some(GenericArg::Ty(t)) => *t,
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
                let new_args: Vec<GenericArg> = self
                    .ctx
                    .substitution_args(*s)
                    .iter()
                    .map(|a| match a {
                        GenericArg::Ty(t) => GenericArg::Ty(self.subst_params(*t, substs)),
                        other => other.clone(),
                    })
                    .collect();
                let new_s = self.ctx.intern_substitution(new_args);
                self.ctx.mk_ty(TyKind::Adt(*adt_id, new_s))
            }
            TyKind::Tuple(s) => {
                let new_args: Vec<GenericArg> = self
                    .ctx
                    .substitution_args(*s)
                    .iter()
                    .map(|a| match a {
                        GenericArg::Ty(t) => GenericArg::Ty(self.subst_params(*t, substs)),
                        other => other.clone(),
                    })
                    .collect();
                let new_s = self.ctx.intern_substitution(new_args);
                self.ctx.mk_ty(TyKind::Tuple(new_s))
            }
            _ => ty,
        }
    }

    fn layout_adt(
        &self,
        adt_id: glyim_core::AdtId,
        substs: Substitution,
        outer_ty: Ty,
    ) -> Result<Layout, LayoutError> {
        let adt_def = match self.ctx.adt_def(adt_id) {
            Some(def) => def,
            None => {
                if let Some(repr) = self.ctx.adt_repr(adt_id) {
                    let mut offsets = IndexVec::new();
                    let mut struct_align = Align::ONE;
                    let mut current_offset = Size::ZERO;
                    for field_ty in &repr.field_tys {
                        let field_layout = self.layout_of(*field_ty)?;
                        struct_align = struct_align.max(field_layout.align);
                        current_offset = current_offset.align_to(field_layout.align);
                        offsets.push(current_offset);
                        current_offset = current_offset + field_layout.size;
                    }
                    let size = current_offset.align_to(struct_align);
                    return Ok(Layout {
                        size,
                        align: struct_align,
                        fields: FieldsShape::Arbitrary { offsets },
                        variants: VariantsShape::Single { index: 0 },
                        is_unsized: false,
                    });
                }
                return Err(LayoutError::UnknownType(outer_ty));
            }
        };
        let _args = self.ctx.substitution_args(substs);
        let args = _args;
        match adt_def.kind {
            AdtKind::Struct => self.layout_struct(adt_def, args),
            AdtKind::Enum => self.layout_enum(adt_def, outer_ty, args),
            AdtKind::Union => self.layout_union(adt_def, args),
        }
    }

    fn pass_mode_for(&self, ty: Ty, layout: &Layout, conv: CallConvention) -> PassMode {
        if matches!(self.ctx.ty_kind(ty), TyKind::Never) {
            return PassMode::Ignore;
        }
        if matches!(self.ctx.ty_kind(ty), TyKind::Unit) {
            return PassMode::Ignore;
        }
        if layout.size == Size::ZERO && !layout.is_unsized {
            return PassMode::Ignore;
        }
        match conv {
            CallConvention::C | CallConvention::System => {
                if layout.size.0 > 16 {
                    PassMode::Indirect { meta_attrs: false }
                } else {
                    PassMode::Direct
                }
            }
            CallConvention::Glyim => PassMode::Direct,
        }
    }
}

impl LayoutComputer for SimpleLayoutComputer<'_> {
    fn layout_of(&self, ty: Ty) -> Result<Layout, LayoutError> {
        let ptr_size = Size::bytes(self.target.pointer_size());
        let ptr_align = Align::from_bytes(self.target.pointer_align());
        let layout = match self.ctx.ty_kind(ty) {
            TyKind::Bool => Layout::scalar(Size::bytes(1), Align::ONE),
            TyKind::Int(i) => {
                let bw = i.bit_width(&self.target);
                let byte_width = bw as u64 / 8;
                Layout::scalar(Size::bytes(byte_width), Align::from_bytes(byte_width))
            }
            TyKind::Uint(u) => {
                let bw = u.bit_width(&self.target);
                let byte_width = bw as u64 / 8;
                Layout::scalar(Size::bytes(byte_width), Align::from_bytes(byte_width))
            }
            TyKind::Float(f) => {
                let bw = f.bit_width();
                let byte_width = bw as u64 / 8;
                Layout::scalar(Size::bytes(byte_width), Align::from_bytes(byte_width))
            }
            TyKind::Char => Layout::scalar(Size::bytes(4), Align::from_bytes(4)),
            TyKind::Never => Layout::scalar(Size::ZERO, Align::ONE),
            TyKind::Unit => Layout::unit(),
            TyKind::Ref(_, _, _) | TyKind::RawPtr(_, _) => Layout::scalar(ptr_size, ptr_align),
            TyKind::FnPtr(_) => Layout::scalar(ptr_size, ptr_align),
            TyKind::Dynamic(_binder, _region) => {
                let raw_size = self.target.pointer_size();
                Layout {
                    size: Size::bytes(raw_size.saturating_mul(2)),
                    align: ptr_align,
                    fields: FieldsShape::Arbitrary {
                        offsets: {
                            let mut off = IndexVec::new();
                            off.push(Size::ZERO);
                            off.push(Size::bytes(raw_size));
                            off
                        },
                    },
                    variants: VariantsShape::Single { index: 0 },
                    is_unsized: false,
                }
            }
            TyKind::Slice(_) => return Err(LayoutError::Unsized(ty)),
            TyKind::String => {
                let raw_size = self.target.pointer_size();
                Layout {
                    size: Size::bytes(raw_size.saturating_mul(2)),
                    align: ptr_align,
                    fields: FieldsShape::Arbitrary {
                        offsets: {
                            let mut off = IndexVec::new();
                            off.push(Size::ZERO);
                            off.push(Size::bytes(raw_size));
                            off
                        },
                    },
                    variants: VariantsShape::Single { index: 0 },
                    is_unsized: false,
                }
            }
            TyKind::Tuple(substs) => return self.layout_tuple(*substs),
            TyKind::Array(inner, count) => return self.layout_array(*inner, count, ty),
            TyKind::Adt(adt_id, substs) => return self.layout_adt(*adt_id, *substs, ty),
            TyKind::Infer(_) => { return Err(LayoutError::UnknownType(ty)); }
            TyKind::Error => { return Err(LayoutError::UnknownType(ty)); }
            TyKind::Param(_) | TyKind::Bound(_, _) => { return Err(LayoutError::UnknownType(ty)); }
            TyKind::Opaque(_, _) => { return Err(LayoutError::UnknownType(ty)); }
            TyKind::Projection(_) => { return Err(LayoutError::UnknownType(ty)); }
            TyKind::Closure(_, substs) => {
                let args = self.ctx.substitution_args(*substs);
                if args.is_empty() {
                    return Ok(Layout::unit());
                }
                let mut field_layouts = Vec::with_capacity(args.len());
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
                Layout {
                    size,
                    align,
                    fields: FieldsShape::Arbitrary { offsets },
                    variants: VariantsShape::Single { index: 0 },
                    is_unsized: false,
                }
            }
            // TyKind::String is handled above alongside TyKind::Slice
            TyKind::FnDef(_, _) => return Err(LayoutError::UnknownType(ty)),
        };
        if layout.align.0 > ALIGN_MAX {
            return Err(LayoutError::AlignmentExceedsRuntime {
                ty,
                align: layout.align.0,
                max: ALIGN_MAX,
            });
        }
        Ok(layout)
    }

    fn fn_abi_of(&self, sig: &FnSig) -> Result<FnAbi, LayoutError> {
        let conv = CallConvention::from(sig.abi);
        let args = self.ctx.substitution_args(sig.inputs);
        let arg_abis: Vec<ArgAbi> = args
            .iter()
            .filter_map(|arg| {
                if let GenericArg::Ty(t) = arg {
                    let layout = self.layout_of(*t).ok()?;
                    let mode = self.pass_mode_for(*t, &layout, conv);
                    Some(ArgAbi {
                        ty: *t,
                        layout,
                        mode,
                    })
                } else {
                    None
                }
            })
            .collect();
        let ret_layout = self.layout_of(sig.output)?;
        let ret_mode = self.pass_mode_for(sig.output, &ret_layout, conv);
        Ok(FnAbi {
            args: arg_abis,
            ret: ArgAbi {
                ty: sig.output,
                layout: ret_layout,
                mode: ret_mode,
            },
            conv,
            c_variadic: sig.c_variadic,
        })
    }

    fn ptr_size(&self) -> Size {
        Size::bytes(self.target.pointer_size())
    }
    fn ptr_align(&self) -> Align {
        Align::from_bytes(self.target.pointer_align())
    }
    fn target_info(&self) -> &TargetInfo {
        &self.target
    }
}

#[cfg(test)]
mod tests;

pub mod vtable;

impl crate::vtable::VTableComputer for SimpleLayoutComputer<'_> {
    fn vtable_of(
        &self,
        trait_def_id: glyim_core::TraitDefId,
        concrete_ty: glyim_type::Ty,
    ) -> Result<crate::vtable::VTableLayout, crate::LayoutError> {
        // Propagate any real layout error (e.g. unsized/concrete type issues)
        // directly; it is independent of trait resolution.
        let concrete_layout = self.layout_of(concrete_ty)?;

        // Plan §10.1: a missing trait definition is a hard error, not a
        // silently-empty vtable. An unresolvable trait should have been caught
        // by typeck's object-safety check (§9.4) long before layout runs.
        let trait_def = self
            .ctx
            .trait_def(trait_def_id)
            .ok_or(crate::LayoutError::UnknownTrait(trait_def_id))?;

        let methods = trait_def
            .methods
            .iter()
            .map(|method| crate::vtable::VTableEntry {
                name: method.name,
                sig: method.sig.clone(),
                fn_def_id: method
                    .fn_def_id
                    .unwrap_or_else(|| glyim_core::def_id::FnDefId::from_raw(u32::MAX)),
            })
            .collect::<Vec<_>>();

        Ok(crate::vtable::VTableLayout {
            trait_def_id,
            concrete_ty,
            size: concrete_layout.size,
            align: concrete_layout.align,
            drop_fn: None,
            methods,
        })
    }
}
