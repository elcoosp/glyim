use crate::abi::FullLayoutComputer;
use crate::debug::DebugInfoCtx;
use crate::types::llvm_type_for_ty;
use glyim_core::TargetInfo;
use glyim_core::arena::IndexVec;
use glyim_core::primitives::*;
use glyim_core::primitives::{Abi, Safety};
use glyim_diag::{CompResult, GlyimDiagnostic};
use glyim_layout::{FieldsShape, LayoutComputer, PassMode, Size, TagEncoding, VariantsShape};
use glyim_mir::VariantIdx;
use glyim_mir::{
    AggregateKind, BasicBlockIdx, Body, CastKind, LocalIdx, MirConst, MirConstKind, Operand, Place,
    ProjectionElem, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use glyim_span::HygieneCtx;
use glyim_span::{FileId, Span};
use glyim_type::{
    ConstKind, FieldIdx, FnSig, GenericArg, ProjectionTy, TraitRef, Ty, TyCtx, TyCtxMut, TyKind,
};
use inkwell::AddressSpace;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::BasicType;
use inkwell::values::{
    AnyValue, AnyValueEnum, BasicMetadataValueEnum, BasicValue, BasicValueEnum, PointerValue,
};
use std::collections::HashMap;
use std::num::NonZeroU32;

use llvm_sys::prelude::LLVMValueRef;

/// Plan §19.1 / §19.2: personality-function selection is a proper three-way
/// choice driven by target ABI, instead of the previous implicit
/// "Windows or Unix, nothing else" binary. `None` means the target/profile has
/// no unwinding support, in which case `Call` terminators with a cleanup target
/// must lower to a plain `call` (no `invoke`/landingpad) — see `lower_call`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Personality {
    /// Itanium C++ ABI personality (`__gcc_personality_v0` / `__gxx_personality_v0`)
    /// used on Linux/glibc, macOS, and other Unix-like targets.
    Itanium,
    /// Microsoft SEH personality (`__CxxFrameHandler3`) used on Windows targets.
    /// Requires funclet-based landingpads (`cleanuppad`/`catchpad`) rather than
    /// the Itanium `landingpad`/`resume` pair (deferred — see §19.1).
    Seh,
    /// No unwinding support (e.g. `panic = "abort"` or a bare-metal target with
    /// no unwinder). No personality function is emitted.
    None,
}

/// Select the personality kind for the given target, given whether the body
/// has any cleanup (unwind) blocks. Without cleanup blocks there is nothing to
/// unwind into, so we emit no personality function regardless of target.
pub fn select_personality(target: &TargetInfo, has_cleanup: bool) -> Personality {
    if !has_cleanup {
        return Personality::None;
    }
    match target.abi {
        TargetAbi::X86_64Windows | TargetAbi::AArch64Windows => Personality::Seh,
        _ => Personality::Itanium,
    }
}

#[allow(unused_imports)]
fn local_ty(body: &Body, local: LocalIdx) -> Ty {
    body.locals[local].ty
}

struct LoweringCtx<'ctx, 'a> {
    context: &'ctx Context,
    builder: Builder<'ctx>,
    function: inkwell::values::FunctionValue<'ctx>,
    module: &'a Module<'ctx>,
    body: &'a Body,
    target_info: TargetInfo,
    ty_ctx: &'a TyCtx,
    locals: IndexVec<LocalIdx, Option<PointerValue<'ctx>>>,
    bb_map: HashMap<BasicBlockIdx, inkwell::basic_block::BasicBlock<'ctx>>,
    _personality_fn: Option<inkwell::values::FunctionValue<'ctx>>,
    /// Unwinding personality selected for this target body. Drives whether the
    /// cleanup block lowers to an Itanium `landingpad`/`resume` pair or an SEH
    /// funclet `cleanuppad`/`cleanupret` pair.
    personality: Personality,
    debug_ctx: Option<DebugInfoCtx<'ctx>>,
    /// The landingpad result value for the cleanup block currently being
    /// lowered, if any. Set at the top of `lower_body`'s block loop (via
    /// `emit_landingpad`) and consulted by `TerminatorKind::Unreachable` to
    /// decide whether to emit `resume` instead of `unreachable`.
    current_landingpad: Option<BasicValueEnum<'ctx>>,
    /// The SEH `cleanuppad` token for the cleanup block currently being
    /// lowered, if the personality is `Seh`. Consumed by
    /// `TerminatorKind::Unreachable`, which emits `cleanupret` to the
    /// function's unwind continuation (the caller's pad) instead of a
    /// `resume`.
    current_seh_pad: Option<LLVMValueRef>,
    /// Whether this function returns its result via a hidden sret pointer
    /// (composite return values that the target ABI cannot pass in registers).
    /// When true, the caller passes a hidden pointer as the first argument and
    /// the callee stores its return value there instead of returning it.
    is_sret: bool,
}
impl<'ctx, 'a> LoweringCtx<'ctx, 'a> {
    fn llvm_int_type(&self, bits: u32) -> inkwell::types::IntType<'ctx> {
        let non_zero = NonZeroU32::new(bits).unwrap_or_else(|| NonZeroU32::new(64).unwrap());
        self.context.custom_width_int_type(non_zero).unwrap()
    }

    /// Build an LLVM function type from a Glyim `FnSig`, applying ABI rules.
    fn llvm_fn_type_from_sig(
        &self,
        sig: &glyim_type::FnSig,
    ) -> CompResult<inkwell::types::FunctionType<'ctx>> {
        let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
        llvm_fn_type_from_sig_inner(
            self.ty_ctx,
            &self.target_info,
            self.context,
            &layout_computer,
            sig,
        )
        .map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "fn type lowering failed: {e:?}"
            ))]
        })
    }
}

/// Free-function form of `llvm_fn_type_from_sig` so it can be used before a
/// `LoweringCtx` exists (e.g. when constructing the LLVM `FunctionValue`).
fn llvm_fn_type_from_sig_inner<'ctx>(
    ty_ctx: &TyCtx,
    target_info: &TargetInfo,
    context: &'ctx Context,
    layout_computer: &FullLayoutComputer,
    sig: &glyim_type::FnSig,
) -> CompResult<inkwell::types::FunctionType<'ctx>> {
    let fn_abi = match layout_computer.fn_abi_of(sig) {
        Ok(a) => a,
        Err(e) => {
            return Err(vec![GlyimDiagnostic::internal_error(format!(
                "fn_abi_of failed: {:?}",
                e
            ))]);
        }
    };

    let is_sret = matches!(fn_abi.ret.mode, PassMode::Indirect { .. });
    let mut param_types = Vec::new();
    if is_sret {
        param_types.push(context.ptr_type(AddressSpace::default()).into());
    }
    for arg_abi in &fn_abi.args {
        let llvm_ty = match arg_abi.mode {
            PassMode::Direct => llvm_type_for_ty(ty_ctx, target_info, context, arg_abi.ty)?,
            PassMode::Indirect { .. } => context.ptr_type(AddressSpace::default()).into(),
            PassMode::Ignore => continue,
            _ => llvm_type_for_ty(ty_ctx, target_info, context, arg_abi.ty)?,
        };
        param_types.push(llvm_ty);
    }
    let ret_type: Option<inkwell::types::BasicTypeEnum<'ctx>> = if is_sret {
        None
    } else {
        match fn_abi.ret.mode {
            PassMode::Ignore => None,
            _ => Some(llvm_type_for_ty(
                ty_ctx,
                target_info,
                context,
                fn_abi.ret.ty,
            )?),
        }
    };
    let metadata_param_types: Vec<inkwell::types::BasicMetadataTypeEnum<'ctx>> =
        param_types.iter().map(|t| (*t).into()).collect();
    if let Some(ret) = ret_type {
        Ok(ret.fn_type(&metadata_param_types, sig.c_variadic))
    } else {
        Ok(context
            .void_type()
            .fn_type(&metadata_param_types, sig.c_variadic))
    }
}

impl<'ctx, 'a> LoweringCtx<'ctx, 'a> {
    fn llvm_type_for_ty(&self, ty: Ty) -> CompResult<inkwell::types::BasicTypeEnum<'ctx>> {
        match llvm_type_for_ty(self.ty_ctx, &self.target_info, self.context, ty) {
            Ok(t) => Ok(t),
            Err(e) => Err(vec![GlyimDiagnostic::internal_error(format!(
                "Codegen error: {e:?}"
            ))]),
        }
    }

    fn set_debug_location(&self, span: Span) {
        if span.is_dummy() {
            return;
        }
        if let Some(ref debug_ctx) = self.debug_ctx
            && let Some(loc) = debug_ctx.location_for_span(self.context, &span)
        {
            self.builder.set_current_debug_location(loc);
        }
    }

    fn alloc_local(&mut self, local: LocalIdx) -> CompResult<()> {
        // Idempotent: a local may already have a slot from the prologue/alloca
        // loop (e.g. a function param whose value was just stored by the
        // prologue). `StorageLive` is allowed to fire again for an already
        // live local and must NOT re-allocate — re-allocating would hand the
        // body a fresh, uninitialized slot and silently drop the prologue's
        // stored value (the `%local_11`/`%local_22` shadow-slot bug).
        if self.locals[local].is_some() {
            return Ok(());
        }
        let ty = local_ty(self.body, local);
        let llvm_ty = self.llvm_type_for_ty(ty)?;
        let name = format!("local_{}", local.index());
        let is_zero_sized = ty == Ty::UNIT
            || ty == Ty::NEVER
            || (if let inkwell::types::BasicTypeEnum::StructType(st) = llvm_ty {
                st.get_field_types().is_empty()
            } else {
                false
            });
        if is_zero_sized {
            let ptr = self
                .context
                .ptr_type(inkwell::AddressSpace::default())
                .const_null();
            self.locals[local] = Some(ptr);
        } else {
            let alloca = self
                .builder
                .build_alloca(llvm_ty, &name)
                .expect("alloca failed");
            // Plan §19.3: enforce the *real* computed alignment at every alloca
            // site unconditionally (not only above a 16-byte threshold). The
            // opaque sized type cannot carry alignment > 16 through its type
            // alone, and even <= 16 types must get their true alignment so the
            // backend never silently under-aligns (e.g. `#[repr(align(64))]`).
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            if let Ok(layout) = layout_computer.layout_of(ty) {
                let align = layout.align.0.max(1) as u32;
                if let Some(alloca_inst) = alloca.as_instruction_value() {
                    let _ = alloca_inst.set_alignment(align);
                }
            }
            self.locals[local] = Some(alloca);
        }
        Ok(())
    }

    fn get_local_ptr(&self, local: LocalIdx) -> PointerValue<'ctx> {
        self.locals[local].unwrap_or_else(|| panic!("local {} not allocated", local.index()))
    }

    fn lower_operand(&self, operand: &Operand) -> CompResult<BasicValueEnum<'ctx>> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                let ptr = self.place_ptr(place)?;
                let ty = self.place_ty(place);
                let llvm_ty = self.llvm_type_for_ty(ty)?;
                Ok(self
                    .builder
                    .build_load(llvm_ty, ptr, "load")
                    .expect("load failed"))
            }
            Operand::Constant(c) => self.lower_const(c),
        }
    }

    fn lower_const(&self, c: &MirConst) -> CompResult<BasicValueEnum<'ctx>> {
        match &c.kind {
            MirConstKind::Int(v) => {
                let ty = self.llvm_type_for_ty(c.ty)?;
                let int_ty = ty.into_int_type();
                Ok(int_ty.const_int(*v as u64, true).into())
            }
            MirConstKind::Uint(v) => {
                let ty = self.llvm_type_for_ty(c.ty)?;
                let int_ty = ty.into_int_type();
                Ok(int_ty.const_int(*v as u64, false).into())
            }
            MirConstKind::Bool(b) => Ok(self
                .llvm_int_type(1)
                .const_int(if *b { 1 } else { 0 }, false)
                .into()),
            MirConstKind::FloatBits(bits) => {
                let ty = self.llvm_type_for_ty(c.ty)?;
                let float_ty = ty.into_float_type();
                Ok(float_ty.const_float(f64::from_bits(*bits)).into())
            }
            MirConstKind::Char(ch) => {
                Ok(self.llvm_int_type(32).const_int(*ch as u64, false).into())
            }
            MirConstKind::Unit => {
                let unit_ty = self.context.struct_type(&[], false);
                Ok(unit_ty.const_zero().as_basic_value_enum())
            }
            MirConstKind::String(name) => {
                let str_content = self.ty_ctx.name_str(*name);
                let module = self.module;
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                str_content.hash(&mut hasher);
                let hash = hasher.finish();
                let global_name = format!("__glyim_str_{:016x}", hash);
                let global = if let Some(existing) = module.get_global(&global_name) {
                    existing
                } else {
                    let const_str = self.context.const_string(str_content.as_bytes(), true);
                    let i8_type = self.context.i8_type();
                    let str_type = i8_type.array_type(str_content.len() as u32 + 1);
                    let global = module.add_global(
                        str_type,
                        Some(inkwell::AddressSpace::default()),
                        &global_name,
                    );
                    global.set_initializer(&const_str);
                    global.set_constant(true);
                    global.set_linkage(inkwell::module::Linkage::Private);
                    global
                };
                let ptr_type = self.context.ptr_type(inkwell::AddressSpace::default());
                let i64_type = self.context.i64_type();
                let struct_ty = self
                    .context
                    .struct_type(&[ptr_type.into(), i64_type.into()], false);
                let str_ptr = self
                    .builder
                    .build_bit_cast(global.as_pointer_value(), ptr_type, "str_ptr")
                    .expect("bitcast for string constant failed")
                    .into_pointer_value();
                let len_val = i64_type.const_int(str_content.len() as u64, false);
                let agg = struct_ty.const_zero();
                let inserted_ptr = self
                    .builder
                    .build_insert_value(agg, str_ptr, 0, "str_ptr_insert")
                    .expect("insert ptr failed");
                let inserted_ptr = match inserted_ptr {
                    inkwell::values::AggregateValueEnum::StructValue(s) => s,
                    _ => unreachable!(),
                };
                let inserted_len = self
                    .builder
                    .build_insert_value(inserted_ptr, len_val, 1, "str_len_insert")
                    .expect("insert len failed");
                match inserted_len {
                    inkwell::values::AggregateValueEnum::StructValue(s) => {
                        Ok(s.as_basic_value_enum())
                    }
                    _ => unreachable!(),
                }
            }
            MirConstKind::Fn(fn_def_id, _substs) => {
                let fn_name = match self.ty_ctx.extern_fn_name(*fn_def_id) {
                    Some(ext) => ext.to_string(),
                    None => format!("__glyim_fn_{}", fn_def_id.to_raw()),
                };
                let module = self.module;
                let callee = match module.get_function(&fn_name) {
                    Some(f) => f,
                    None => {
                        let fn_type = if let Some(sig) = self.ty_ctx.fn_sig(*fn_def_id) {
                            self.llvm_fn_type_from_sig(sig)?
                        } else {
                            self.context.void_type().fn_type(&[], false)
                        };
                        module.add_function(&fn_name, fn_type, None)
                    }
                };
                Ok(callee
                    .as_global_value()
                    .as_pointer_value()
                    .as_basic_value_enum())
            }
            MirConstKind::ConstRef(const_def_id, _substs) => {
                // T114-PATCHED-LLVM [LL-19]: a named `const` that survived
                // const-eval folding to this point cannot be materialised
                // correctly here. The previous body allocated a global with
                // a zero initializer and never wrote the real value, so
                // every non-folded const context silently read `0`. Rather
                // than emit wrong code, produce an internal-error diagnostic
                // that names the const id and asks for the const evaluator
                // to run first. If const-eval is guaranteed to fold every
                // ConstRef before codegen, this arm is unreachable and the
                // diagnostic simply documents the invariant.
                Err(vec![GlyimDiagnostic::internal_error(format!(
                    "ConstRef({}) reached LLVM codegen: const evaluation must \
                     fold named constants into concrete MirConstKind values \
                     before codegen. Context type: {:?}.",
                    const_def_id.to_raw(),
                    c.ty,
                ))])
            }
            MirConstKind::Aggregate(elems) => {
                // Emit a constant aggregate (tuple/array/struct) by recursively
                // lowering each constant element and inserting it into a zero
                // aggregate at its index (plan §15.3). `build_insert_value`
                // works uniformly for both struct and array LLVM types. Any
                // element that fails to lower falls back to a zero constant so
                // callers never observe an error.
                let llvm_ty = self.llvm_type_for_ty(c.ty)?;
                let zero = llvm_ty.const_zero();
                let agg: inkwell::values::AggregateValueEnum = match llvm_ty {
                    inkwell::types::BasicTypeEnum::StructType(st) => st.const_zero().into(),
                    inkwell::types::BasicTypeEnum::ArrayType(at) => at.const_zero().into(),
                    // Scalars/others: nothing to aggregate; return the zero.
                    _ => return Ok(zero.as_basic_value_enum()),
                };
                let mut cur = agg;
                for (i, e) in elems.iter().enumerate() {
                    let val = self
                        .lower_const(e)
                        .unwrap_or_else(|_| zero.as_basic_value_enum());
                    cur = self
                        .builder
                        .build_insert_value(cur, val, i as u32, "agg_elem")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "aggregate constant insert failed: {:?}",
                                e
                            ))]
                        })?;
                }
                Ok(match cur {
                    inkwell::values::AggregateValueEnum::StructValue(s) => s.as_basic_value_enum(),
                    inkwell::values::AggregateValueEnum::ArrayValue(a) => a.as_basic_value_enum(),
                })
            }
            MirConstKind::Error => {
                // T140-PATCHED-LLVM-ERROR: `MirConstKind::Error` is emitted
                // by lowering for compiler-synthetic builtin calls whose
                // `FnDefId` is a typeck sentinel (`u32::MAX - 4 ..=
                // u32::MAX - 1`; see `check_expr.rs:2574/2579/2587` and
                // `unify.rs:94`) — `into`/`to_string`/`clone`/`drop`. There
                // is no `FnSig` for these ids, so a real `Call` ICEs with
                // "no signature registered". Lowering instead stores an
                // `Error` constant into the destination.
                //
                // These constants appear only on runtime-dead error paths
                // (e.g. the error arm of `Result::unwrap` calling
                // `E::to_string()` for a generic `E`), but LLVM still has to
                // emit code for them. Yielding a zero value of the
                // destination type keeps the generated function well-formed
                // and linkable; the bytecode backend has always done exactly
                // this (`glyim-codegen/src/lib.rs` maps `Unit | Error` to a
                // zero const). The follow-up (session handoff) is to replace
                // the sentinel with a real virtual-method reference so the
                // error branch is *correct*, not merely compilable.
                let ty = self.llvm_type_for_ty(c.ty)?;
                Ok(ty.const_zero().as_basic_value_enum())
            }
            // Devirtualized by monomorphization before LLVM codegen; should not
            // survive to this point.
            MirConstKind::VirtualMethod { .. } => Err(vec![GlyimDiagnostic::internal_error(
                "internal compiler error: MirConstKind::VirtualMethod reached LLVM codegen (not devirtualized)",
            )]),
        }
    }

    fn place_ptr(&self, place: &Place) -> CompResult<PointerValue<'ctx>> {
        let base = self.get_local_ptr(place.local);
        if place.projection.is_empty() {
            return Ok(base);
        }
        let mut ptr = base;
        let mut current_ty = local_ty(self.body, place.local);
        let mut downcast_variant: Option<VariantIdx> = None;
        for elem in place.projection.iter() {
            match elem {
                ProjectionElem::Deref => {
                    // A fat pointer (`&[T]` / `&str`, represented as
                    // `{data_ptr, i64 len}`) dereferences to the unsized
                    // `[T]` / `str`, which is laid out *at* the fat pointer
                    // itself. Do **not** load and advance `ptr` in that case:
                    // downstream projections (`Field(0)` = data pointer,
                    // `Field(1)` = length, and `Rvalue::Len`) expect `ptr` to
                    // still address the fat-pointer struct. Advancing `ptr`
                    // to the data address (an earlier, incorrect fix) made
                    // `Rvalue::Len` read `{ptr, i64}` from the *data* — which
                    // returned garbage lengths and is what produced the
                    // empty `println` output under `--emit=exec`.
                    //
                    // A thin pointer (`&T` where `T` is sized) dereferences to
                    // a value at the loaded address, so `ptr` becomes the
                    // loaded pointer.
                    let pointee_is_fat = match self.ty_ctx.ty_kind(current_ty) {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => matches!(
                            self.ty_ctx.ty_kind(*inner),
                            TyKind::Slice(_) | TyKind::String
                        ),
                        _ => false,
                    };
                    if !pointee_is_fat {
                        let llvm_ty = self.llvm_type_for_ty(current_ty)?;
                        let loaded = self
                            .builder
                            .build_load(llvm_ty, ptr, "deref_load")
                            .expect("deref load failed");
                        ptr = loaded.into_pointer_value();
                    }
                    current_ty = match self.ty_ctx.ty_kind(current_ty) {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                        _ => {
                            return Err(vec![GlyimDiagnostic::internal_error(format!(
                                "internal compiler error: Deref on non-pointer type {:?} – MIR is invalid",
                                current_ty
                            ))]);
                        }
                    };
                }
                ProjectionElem::Field(idx) => {
                    let layout_computer =
                        FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
                    let field_offset_bytes = if let Ok(layout) =
                        layout_computer.layout_of(current_ty)
                    {
                        match &layout.variants {
                            VariantsShape::Multiple { variants, .. } => {
                                // Two offsets contribute to reading a
                                // field of a multi-variant enum:
                                //
                                //   1. `tag_prefix` — the byte distance
                                //      from the enum base to the start of
                                //      the variant payload. For a
                                //      `Downcast`-prefixed projection,
                                //      the `Downcast` arm already
                                //      advanced `ptr` past the tag, so
                                //      this is 0. For a bare `Field`
                                //      (common in patterns like
                                //      `match e { E::A(v) => v }`, where
                                //      the pattern binder reads the
                                //      payload without an explicit
                                //      `Downcast` in the MIR), `ptr`
                                //      still points at the enum base and
                                //      we must add the tag-prefix size.
                                //      That value is `data_start`, which
                                //      `direct_tag_encoding` stores as
                                //      `offsets[tag_field + 1]` (always
                                //      index 1: it writes exactly
                                //      `[0, data_start]`).
                                //
                                //   2. `variant_field_offset` — the byte
                                //      distance from the variant payload
                                //      base to the field. Read from the
                                //      per-variant layout
                                //      `variants[v].fields` — the SAME
                                //      table `build_layout_aggregate`
                                //      writes through, so reader and
                                //      writer agree. (An earlier attempt
                                //      to fold the tag prefix into a
                                //      single `offsets[tag_field + 1 +
                                //      idx]` lookup read past the outer
                                //      layout's 2-entry offsets array
                                //      for `idx >= 1` and panicked with
                                //      an OOB.)
                                let tag_prefix: u64 = if downcast_variant.is_some() {
                                    0
                                } else {
                                    match &layout.fields {
                                        // `offsets.as_slice().get(..)`
                                        // (not `offsets.get(FieldIdx)`)
                                        // because `IndexVec::get` carries
                                        // a `debug_assert!` that fires on
                                        // an out-of-range index — the
                                        // `.unwrap_or(0)` fallback here is
                                        // *intentional* and must not
                                        // panic when `data_start` is
                                        // absent (a single-variant or
                                        // niche-tagged layout). The slice
                                        // `get` returns `None` cleanly.
                                        FieldsShape::Arbitrary { offsets } => {
                                            offsets.as_slice().get(1).map(|s| s.0).unwrap_or(0)
                                        }
                                        _ => 0,
                                    }
                                };
                                // Search the selected variant first, then
                                // every other variant. The state-machine
                                // enums the async desugar emits have
                                // variants with different field counts
                                // (e.g. `S0 { f0, f1, fut0 }` vs
                                // `S1 { f0, f1, v0, fut1 }`), so a bare
                                // `Field(n)` with `n` beyond the selected
                                // variant's field list must fall through
                                // to a variant that has it.
                                let start =
                                    downcast_variant.map(|v| v.to_raw() as usize).unwrap_or(0);
                                let ordered: Vec<usize> = std::iter::once(start)
                                    .chain((0..variants.len()).filter(|i| *i != start))
                                    .collect();
                                let idx_raw = idx.to_raw() as usize;
                                let mut found = None;
                                for vi in ordered {
                                    // `IndexVec::get` carries a
                                    // `debug_assert!` that fires *before*
                                    // returning `None`; guard with an
                                    // explicit length check so a shorter
                                    // variant's field list doesn't panic
                                    // in debug builds during the search.
                                    if let Some(variant) = variants.get(vi) {
                                        if let FieldsShape::Arbitrary { offsets } = &variant.fields
                                        {
                                            if idx_raw < offsets.len() {
                                                let per_variant =
                                                    offsets[FieldIdx::from_raw(idx_raw as u32)].0;
                                                found = Some(tag_prefix + per_variant);
                                                break;
                                            }
                                        }
                                    }
                                }
                                found
                            }
                            VariantsShape::Single { .. } => match &layout.fields {
                                FieldsShape::Arbitrary { offsets } => {
                                    let i = idx.to_raw() as usize;
                                    if i < offsets.len() {
                                        Some(offsets[*idx].0)
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            },
                        }
                    } else {
                        None
                    };
                    ptr = if let Some(offset) = field_offset_bytes {
                        let i8_ptr = self.context.ptr_type(inkwell::AddressSpace::default());
                        let base_i8 = self
                            .builder
                            .build_bit_cast(ptr, i8_ptr, "field_base_i8")
                            .expect("bitcast to i8* failed")
                            .into_pointer_value();
                        let offset_val = self.llvm_int_type(64).const_int(offset, false);
                        unsafe {
                            self.builder
                                .build_in_bounds_gep(
                                    self.context.i8_type(),
                                    base_i8,
                                    &[offset_val],
                                    "field_offset",
                                )
                                .expect("field offset GEP failed")
                        }
                    } else {
                        let field_idx = idx.to_raw() as u64;
                        let i32_type = self.llvm_int_type(32);
                        let zero = i32_type.const_zero();
                        let field_index = i32_type.const_int(field_idx, false);
                        let llvm_ty = self.llvm_type_for_ty(current_ty)?;
                        unsafe {
                            self.builder
                                .build_in_bounds_gep(
                                    llvm_ty,
                                    ptr,
                                    &[zero, field_index],
                                    "field_gep",
                                )
                                .expect("field gep failed")
                        }
                    };
                    let field_ty = match self.ty_ctx.ty_kind(current_ty) {
                        // A slice `[T]` / `str` (`TyKind::String`) is the
                        // unsized fat-pointer *pointee*; `lower_dynamic_range_slice`
                        // derefs a `&[T]` (yielding `[T]`) and then projects
                        // `Field(0)` to read the data pointer. Resolve the two
                        // fat-pointer slots: `Field(0)` = data pointer
                        // (`&T` / `*const u8`), `Field(1)` = `usize` length.
                        TyKind::Slice(elem) if idx.to_raw() == 0 => self.ty_ctx.mk_ref(
                            glyim_type::Region::Static,
                            *elem,
                            glyim_core::primitives::Mutability::Not,
                        ),
                        TyKind::Slice(_) => self
                            .ty_ctx
                            .mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize)),
                        TyKind::String if idx.to_raw() == 0 => {
                            let u8_ty = self
                                .ty_ctx
                                .mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U8));
                            self.ty_ctx.mk_ref(
                                glyim_type::Region::Static,
                                u8_ty,
                                glyim_core::primitives::Mutability::Not,
                            )
                        }
                        TyKind::String => self
                            .ty_ctx
                            .mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize)),
                        TyKind::Tuple(subst) => {
                            let args = self.ty_ctx.substitution_args(*subst);
                            args.get(idx.to_raw() as usize)
                                .and_then(|arg| {
                                    if let glyim_type::GenericArg::Ty(t) = arg {
                                        Some(*t)
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(Ty::ERROR)
                        }
                        TyKind::Adt(adt_id, substs) => {
                            let adt_substs = self.ty_ctx.substitution_args(*substs);
                            let field_idx = idx.to_raw() as usize;
                            // Structs carry their fields in `AdtDef.fields`; enums
                            // carry them per-variant in `AdtDef.variants[v].fields`.
                            // Resolve struct fields directly, and for enums prefer the
                            // downcast variant (if the place went through `Downcast`),
                            // else the first variant that actually has a field at
                            // `idx` (enums lay the value field out at a fixed offset
                            // regardless of which variant is active, so a bare `Field`
                            // on the enum local still reads the right slot).

                            if let Some(adt_def) = self.ty_ctx.adt_def(*adt_id) {
                                let struct_field =
                                    adt_def.fields.as_slice().get(field_idx).map(|f| f.ty);
                                let field_raw = struct_field
                                    .or_else(|| {
                                        let variant = downcast_variant
                                            .and_then(|vi| {
                                                adt_def.variants.get(vi.to_raw() as usize)
                                            })
                                            .or_else(|| {
                                                adt_def
                                                    .variants
                                                    .iter()
                                                    .find(|v| v.fields.len() > field_idx)
                                            })
                                            .or_else(|| adt_def.variants.first());
                                        variant.and_then(|v| {
                                            v.fields.as_slice().get(field_idx).map(|f| f.ty)
                                        })
                                    })
                                    .unwrap_or_else(|| self.ty_ctx.field_ty(*adt_id, field_idx));
                                // Monomorphization: a generic ADT's field type may
                                // reference `Param(i)`, which must be substituted with
                                // the ADT's own `substs[i]` so that e.g. `Poll<T>`'s
                                // `Ready(T)` field resolves to the concrete `T`
                                // (e.g. `i32`) instead of a bare `Param`.
                                match self.ty_ctx.ty_kind(field_raw) {
                                    TyKind::Param(p) => adt_substs
                                        .get(p.index as usize)
                                        .and_then(|a| {
                                            if let glyim_type::GenericArg::Ty(t) = a {
                                                Some(*t)
                                            } else {
                                                None
                                            }
                                        })
                                        .unwrap_or(field_raw),
                                    _ => field_raw,
                                }
                            } else {
                                self.ty_ctx.field_ty(*adt_id, field_idx)
                            }
                        }
                        _ => Ty::ERROR,
                    };
                    if field_ty == Ty::ERROR {
                        tracing::error!(
                            "place_ptr: field type unresolved for current_ty={:?} idx={:?}",
                            self.ty_ctx.ty_kind(current_ty),
                            idx
                        );
                    }
                    if field_ty != Ty::ERROR {
                        let _field_llvm_ty = self.llvm_type_for_ty(field_ty)?;
                        let field_ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        ptr = self
                            .builder
                            .build_bit_cast(ptr, field_ptr_ty, "field_ptr")
                            .expect("bitcast to field type failed")
                            .into_pointer_value();
                        current_ty = field_ty;
                    }
                }
                ProjectionElem::Index(local_idx) => {
                    let index_ptr = self.get_local_ptr(*local_idx);
                    // LL-5: load the index local at its DECLARED width, then
                    // extend to i64. Loading 8 bytes from a 4-byte `i32` slot
                    // pulled adjacent stack bytes into the high bits and drove
                    // the GEP out of bounds.
                    let local_ty = local_ty(self.body, *local_idx);
                    let local_llvm_ty = self.llvm_type_for_ty(local_ty)?;
                    let raw = self
                        .builder
                        .build_load(local_llvm_ty, index_ptr, "index_load")
                        .expect("index load failed")
                        .into_int_value();
                    let i64_ty = self.llvm_int_type(64);
                    let index_val = if local_llvm_ty == i64_ty.as_basic_type_enum() {
                        raw
                    } else if self.is_signed_int_ty(local_ty) {
                        self.builder
                            .build_int_s_extend(raw, i64_ty, "idx_sext")
                            .expect("sext failed")
                    } else {
                        self.builder
                            .build_int_z_extend(raw, i64_ty, "idx_zext")
                            .expect("zext failed")
                    };
                    let elem_ty = match self.ty_ctx.ty_kind(current_ty) {
                        TyKind::Array(elem, _) => *elem,
                        TyKind::Slice(elem) => *elem,
                        _ => {
                            return Err(vec![GlyimDiagnostic::internal_error(format!(
                                "internal compiler error: Index projection on non-array/slice type {:?}",
                                current_ty
                            ))]);
                        }
                    };
                    if let TyKind::Slice(_) = self.ty_ctx.ty_kind(current_ty) {
                        let i8_ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                        let zero_i64 = i64_ty.const_zero();
                        let field0_ptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(
                                    self.llvm_type_for_ty(current_ty)?,
                                    ptr,
                                    &[zero_i64, zero_i64],
                                    "slice_field0_gep",
                                )
                                .expect("slice field0 gep failed")
                        };
                        let data_ptr = self
                            .builder
                            .build_load(i8_ptr_ty, field0_ptr, "slice_data_ptr")
                            .expect("load data ptr failed")
                            .into_pointer_value();
                        let elem_llvm_ty = self.llvm_type_for_ty(elem_ty)?;
                        let elem_ptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(
                                    elem_llvm_ty,
                                    data_ptr,
                                    &[index_val],
                                    "slice_elem_gep",
                                )
                                .expect("slice elem gep failed")
                        };
                        ptr = elem_ptr;
                        current_ty = elem_ty;
                    } else {
                        let llvm_ty = self.llvm_type_for_ty(current_ty)?;
                        let zero_i64 = i64_ty.const_zero();
                        ptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(
                                    llvm_ty,
                                    ptr,
                                    &[zero_i64, index_val],
                                    "array_index_gep",
                                )
                                .expect("array index gep failed")
                        };
                        current_ty = elem_ty;
                    }
                }
                ProjectionElem::Downcast(variant_idx) => {
                    let layout_computer =
                        FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
                    let layout = layout_computer
                        .layout_of(current_ty)
                        .expect("layout_of failed for downcast");
                    if let VariantsShape::Multiple {
                        tag_size,
                        tag_align,
                        tag_encoding,
                        variants,
                        ..
                    } = &layout.variants
                    {
                        let vi = variant_idx.to_raw() as usize;
                        if vi >= variants.len() {
                            return Err(vec![GlyimDiagnostic::internal_error(format!(
                                "Variant index {vi} out of bounds for {current_ty:?}"
                            ))]);
                        }
                        // For `Direct` tag encoding the variant's data starts
                        // *after* the tag (aligned to the variant's own
                        // alignment). For `Niche` encoding there is no
                        // separate tag prefix at all -- every variant's
                        // fields (including the "untagged" storage variant)
                        // live at the same offsets, see `build_niche_layout`
                        // in glyim-layout, which clones the untagged
                        // variant's own field offsets directly onto the
                        // outer enum layout.
                        let data_start = match tag_encoding {
                            TagEncoding::Direct => {
                                Size(tag_size.0)
                                    .align_to(*tag_align)
                                    .align_to(variants[vi].align)
                                    .0
                            }
                            TagEncoding::Niche { .. } => 0,
                        };
                        let i8_ptr = self.context.ptr_type(inkwell::AddressSpace::default());
                        let base_i8 = self
                            .builder
                            .build_bit_cast(ptr, i8_ptr, "downcast_base_i8")
                            .expect("bitcast failed")
                            .into_pointer_value();
                        let offset_val = self.llvm_int_type(64).const_int(data_start, false);
                        ptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(
                                    self.context.i8_type(),
                                    base_i8,
                                    &[offset_val],
                                    "variant_data_ptr",
                                )
                                .expect("downcast GEP failed")
                        };
                        downcast_variant = Some(*variant_idx);
                    }
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    min_length: _,
                    from_end,
                } => {
                    let index_val = if *from_end {
                        let len = match self.ty_ctx.ty_kind(current_ty) {
                            TyKind::Array(_, const_val) => {
                                let n = match &const_val.kind {
                                    ConstKind::Uint(n) => *n as u64,
                                    ConstKind::Int(n) => *n as u64,
                                    _ => 0,
                                };
                                self.llvm_int_type(64).const_int(n, false)
                            }
                            TyKind::Slice(_) => {
                                let llvm_ty = self.llvm_type_for_ty(current_ty)?;
                                let val = self
                                    .builder
                                    .build_load(llvm_ty, ptr, "slice_len_load")
                                    .expect("load failed");
                                let struct_val = match val {
                                    BasicValueEnum::StructValue(s) => s,
                                    _ => panic!("slice value not a struct"),
                                };

                                self.builder
                                    .build_extract_value(struct_val, 1, "slice_len_extract")
                                    .expect("extract failed")
                                    .into_int_value()
                            }
                            _ => {
                                return Err(vec![GlyimDiagnostic::internal_error(format!(
                                    "internal compiler error: ConstantIndex on non-array/slice type {:?}",
                                    current_ty
                                ))]);
                            }
                        };
                        let offset_val = self.llvm_int_type(64).const_int(*offset, false);
                        self.builder
                            .build_int_sub(len, offset_val, "sub_index")
                            .expect("sub failed")
                    } else {
                        self.llvm_int_type(64).const_int(*offset, false)
                    };
                    let data_ptr = if let TyKind::Slice(_) = self.ty_ctx.ty_kind(current_ty) {
                        let llvm_ty = self.llvm_type_for_ty(current_ty)?;
                        let val = self
                            .builder
                            .build_load(llvm_ty, ptr, "slice_load")
                            .expect("load failed");
                        let struct_val = match val {
                            BasicValueEnum::StructValue(s) => s,
                            _ => panic!("slice value not a struct"),
                        };

                        self.builder
                            .build_extract_value(struct_val, 0, "slice_data_ptr")
                            .expect("extract failed")
                            .into_pointer_value()
                    } else {
                        ptr
                    };
                    let elem_ty = match self.ty_ctx.ty_kind(current_ty) {
                        TyKind::Array(elem, _) | TyKind::Slice(elem) => *elem,
                        _ => {
                            return Err(vec![GlyimDiagnostic::internal_error(format!(
                                "internal compiler error: ConstantIndex on non-array/slice type {:?}",
                                current_ty
                            ))]);
                        }
                    };
                    let elem_llvm_ty = self.llvm_type_for_ty(elem_ty)?;
                    ptr = unsafe {
                        self.builder
                            .build_in_bounds_gep(
                                elem_llvm_ty,
                                data_ptr,
                                &[index_val],
                                "const_index_gep",
                            )
                            .expect("const index gep failed")
                    };
                    current_ty = elem_ty;
                }
                ProjectionElem::Subslice { from, to, from_end } => {
                    // The result of a Subslice projection is itself a
                    // *slice value* `{ data_ptr, len }` -- even when the
                    // base being sliced was a fixed-size array. Since
                    // `place_ptr` can only ever hand back a single
                    // `PointerValue` (not a pointer+length pair), we
                    // materialize that `{ ptr, len }` value into a fresh
                    // stack temporary here and return a pointer to *that*,
                    // so every downstream consumer of the resulting place
                    // (`lower_operand`'s load, further projections, etc.)
                    // sees a real, correctly-typed slice value at the
                    // address we return -- exactly as if it had always
                    // lived in memory.
                    //
                    // INVARIANT: `Subslice` must be the terminal element of
                    // a place's projection list. This matches how slice
                    // patterns (`[a, b, .., z]`) produce it during pattern
                    // lowering -- the subslice binding (`..rest`) is always
                    // read out whole, never projected into further within
                    // the same `Place`. `slice_desugar` (see glyim-opt)
                    // removes every `Subslice` from MIR, and
                    // `glyim_opt::validate::validate_no_subslice` (de-stubbing
                    // plan §8.7) asserts none survive past that pass --
                    // wired as a debug-gated panic in `optimize()`. So
                    // reaching this code with a `Subslice` is true by
                    // construction (proven by the validator), not by hope.
                    let (data_ptr, base_len) = match self.ty_ctx.ty_kind(current_ty) {
                        TyKind::Slice(_) => {
                            let llvm_ty = self.llvm_type_for_ty(current_ty)?;
                            let val = self
                                .builder
                                .build_load(llvm_ty, ptr, "subslice_base_load")
                                .expect("load failed");
                            let struct_val = match val {
                                BasicValueEnum::StructValue(s) => s,
                                _ => panic!("slice value not a struct"),
                            };
                            let dp = self
                                .builder
                                .build_extract_value(struct_val, 0, "subslice_data_ptr")
                                .expect("extract failed")
                                .into_pointer_value();
                            let ln = self
                                .builder
                                .build_extract_value(struct_val, 1, "subslice_len")
                                .expect("extract failed")
                                .into_int_value();
                            (dp, ln)
                        }
                        TyKind::Array(_, const_val) => {
                            let n = match &const_val.kind {
                                ConstKind::Uint(n) => *n as u64,
                                ConstKind::Int(n) => *n as u64,
                                _ => 0,
                            };
                            (ptr, self.llvm_int_type(64).const_int(n, false))
                        }
                        other => {
                            return Err(vec![GlyimDiagnostic::internal_error(format!(
                                "internal compiler error: Subslice projection on non-array/slice type {:?}",
                                other
                            ))]);
                        }
                    };
                    let elem_ty = match self.ty_ctx.ty_kind(current_ty) {
                        TyKind::Array(elem, _) | TyKind::Slice(elem) => *elem,
                        _ => unreachable!(),
                    };
                    let i64_ty = self.llvm_int_type(64);
                    let from_val = i64_ty.const_int(*from, false);
                    let to_val = i64_ty.const_int(*to, false);
                    let new_len = if *from_end {
                        // `to` counts back from the end: subslice is
                        // `[from .. base_len - to]`.
                        let end_val = self
                            .builder
                            .build_int_sub(base_len, to_val, "subslice_end")
                            .expect("sub failed");
                        self.builder
                            .build_int_sub(end_val, from_val, "subslice_len")
                            .expect("sub failed")
                    } else {
                        // `to` is an absolute index: subslice is `[from .. to]`.
                        self.builder
                            .build_int_sub(to_val, from_val, "subslice_len")
                            .expect("sub failed")
                    };
                    let elem_llvm_ty = self.llvm_type_for_ty(elem_ty)?;
                    let new_data_ptr = unsafe {
                        self.builder
                            .build_in_bounds_gep(
                                elem_llvm_ty,
                                data_ptr,
                                &[from_val],
                                "subslice_data_ptr",
                            )
                            .expect("gep failed")
                    };
                    let ptr_ty = self.context.ptr_type(AddressSpace::default());
                    let slice_struct_ty = self
                        .context
                        .struct_type(&[ptr_ty.into(), i64_ty.into()], false);
                    let tmp = self
                        .builder
                        .build_alloca(slice_struct_ty, "subslice_tmp")
                        .expect("alloca failed");
                    let agg = slice_struct_ty.const_zero();
                    let agg = self
                        .builder
                        .build_insert_value(agg, new_data_ptr, 0, "subslice_ptr_insert")
                        .expect("insert failed");
                    let agg = self
                        .builder
                        .build_insert_value(agg, new_len, 1, "subslice_len_insert")
                        .expect("insert failed");
                    self.builder
                        .build_store(tmp, agg.as_basic_value_enum())
                        .expect("store failed");
                    ptr = tmp;
                }
            }
        }
        Ok(ptr)
    }

    fn place_ty(&self, place: &Place) -> Ty {
        place.ty(self.ty_ctx, &self.body.locals)
    }

    fn lower_statement(&mut self, stmt: &Statement) -> CompResult<()> {
        self.set_debug_location(stmt.source_info.span);
        match &stmt.kind {
            StatementKind::Assign(place, rvalue) => {
                let expected_ty = self.place_ty(place);
                let val = self.lower_rvalue(rvalue, expected_ty)?;
                let ptr = self.place_ptr(place)?;
                self.builder.build_store(ptr, val).expect("store failed");
                Ok(())
            }
            StatementKind::StorageLive(local) => {
                self.alloc_local(*local)?;
                Ok(())
            }
            StatementKind::StorageDead(_local) => Ok(()),
            StatementKind::Nop => Ok(()),
        }
    }

    /// Lower an rvalue to a value.
    ///
    /// `expected_ty` is the type of the place this rvalue is being assigned
    /// into (`Place::ty`). It is authoritative for `Aggregate` construction:
    /// unlike deriving a struct type ad hoc from the operands' LLVM types,
    /// `expected_ty` lets us go through `glyim-layout` and build the value
    /// with the *exact* field offsets (and, for enums, discriminant/niche
    /// tag) that `llvm_type_for_ty(expected_ty)` and every other place that
    /// reads this type already assume.
    fn lower_rvalue(&self, rvalue: &Rvalue, expected_ty: Ty) -> CompResult<BasicValueEnum<'ctx>> {
        match rvalue {
            Rvalue::Use(operand) => Ok(self.lower_operand(operand)?),
            Rvalue::BinaryOp(op, operands) => {
                let (left, right) = operands.as_ref();
                let l = self.lower_operand(left)?;
                let r = self.lower_operand(right)?;
                let operand_ty = self.operand_ty(left);
                self.lower_binop(*op, l, r, operand_ty)
            }
            Rvalue::UnaryOp(op, operand) => {
                let val = self.lower_operand(operand)?;
                self.lower_unop(*op, operand, val)
            }
            Rvalue::Ref(place, _borrow_kind) => {
                let ptr = self.place_ptr(place)?;
                // A reference to an *unsized* place (`str` / `[T]`) is itself
                // a fat pointer `{ data_ptr, len }`, not a bare address. The
                // place is materialized on the stack as a `{ ptr, i64 }`
                // struct, so `place_ptr` returns the address of that struct.
                // Returning it directly and letting the caller store it as
                // `&str` wrote only the 8-byte pointer into a 16-byte slot
                // (with garbage in the length half) — the cause of the
                // `str::trim()` cluster returning wrong lengths and of
                // `let t: &str = "abc".trim(); t.len()` segfaulting.
                // Load the whole fat pointer instead.
                let pointee_ty = self.place_ty(place);
                let is_unsized = matches!(
                    self.ty_ctx.ty_kind(pointee_ty),
                    glyim_type::TyKind::Slice(_) | glyim_type::TyKind::String
                );
                if is_unsized {
                    let llvm_pointee_ty = self.llvm_type_for_ty(pointee_ty)?;
                    let val = self
                        .builder
                        .build_load(llvm_pointee_ty, ptr, "fat_ref_load")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "load fat reference failed: {:?}",
                                e
                            ))]
                        })?;
                    return Ok(val);
                }
                Ok(ptr.as_basic_value_enum())
            }
            Rvalue::Aggregate(kind, operands) => {
                let mut vals = Vec::new();
                for op in operands {
                    vals.push(self.lower_operand(op)?);
                }
                match kind {
                    AggregateKind::Array(elem_ty) => {
                        let llvm_elem_ty = self.llvm_type_for_ty(*elem_ty)?;
                        let array_ty = llvm_elem_ty.array_type(vals.len() as u32);
                        let mut agg: inkwell::values::AggregateValueEnum<'ctx> =
                            array_ty.const_zero().into();
                        for (i, val) in vals.into_iter().enumerate() {
                            let val = if val.get_type() == llvm_elem_ty {
                                val
                            } else {
                                self.builder
                                    .build_bit_cast(val, llvm_elem_ty, "array_elem_cast")
                                    .expect("bitcast failed")
                            };
                            agg = self
                                .builder
                                .build_insert_value(agg, val, i as u32, "array_insert")
                                .expect("insert value failed");
                        }
                        Ok(agg.as_basic_value_enum())
                    }
                    AggregateKind::Tuple => self.build_layout_aggregate(expected_ty, None, &vals),
                    AggregateKind::Adt(_adt_id, variant, _substs) => {
                        self.build_layout_aggregate(expected_ty, Some(*variant), &vals)
                    }
                    AggregateKind::Closure(_closure_id, _substs) => {
                        self.build_layout_aggregate(expected_ty, None, &vals)
                    }
                }
            }
            Rvalue::Discriminant(place) => {
                // `lower_discriminant` always returns the tag as `i64`, but the
                // place this rvalue is stored into may be a narrower integer
                // (e.g. `i8` for a 2-variant enum). Storing the `i64` value
                // directly would write 8 bytes into a 1-byte `alloca`,
                // clobbering adjacent stack (a silent miscompile:
                // `match E::A(42) { E::A(v) => v }` returned 0). Cast to the
                // destination type's width before returning.
                let val = self.lower_discriminant(place)?;
                let dest_llvm_ty = self.llvm_type_for_ty(expected_ty)?;
                if val.get_type() != dest_llvm_ty {
                    let src_int = val.into_int_value();
                    let dest_int = dest_llvm_ty.into_int_type();
                    let src_bits = src_int.get_type().get_bit_width();
                    let dest_bits = dest_int.get_bit_width();
                    let cast = if dest_bits < src_bits {
                        self.builder
                            .build_int_truncate(src_int, dest_int, "discr_trunc")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "trunc discr failed: {:?}",
                                    e
                                ))]
                            })?
                    } else {
                        self.builder
                            .build_int_z_extend(src_int, dest_int, "discr_zext_dest")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "zext discr failed: {:?}",
                                    e
                                ))]
                            })?
                    };
                    Ok(cast.as_basic_value_enum())
                } else {
                    Ok(val)
                }
            }
            Rvalue::Len(place) => {
                let ptr = self.place_ptr(place)?;
                let ty = self.place_ty(place);
                let len = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Array(_, const_val) => {
                        let n = match &const_val.kind {
                            ConstKind::Uint(n) => *n as u64,
                            ConstKind::Int(n) => *n as u64,
                            _ => 0,
                        };
                        self.llvm_int_type(64).const_int(n, false)
                    }
                    // Slices and `str` are both lowered to the same
                    // `{data_ptr, len}` fat-pointer struct, so `len` is
                    // field 1 of that struct. The interpreter already
                    // handles `TyKind::String` this way
                    // (`InterpValue::String` exposes field 1 as byte
                    // length); codegen previously only matched `Slice`,
                    // so any `str::trim_*` (which lowers `s.len()` on a
                    // `str`) panicked with `Len on non-array/slice type`.
                    TyKind::Slice(_) | TyKind::String => {
                        let llvm_ty = self.llvm_type_for_ty(ty)?;
                        let val = self
                            .builder
                            .build_load(llvm_ty, ptr, "slice_len_load")
                            .expect("load failed");
                        let struct_val = match val {
                            BasicValueEnum::StructValue(s) => s,
                            _ => {
                                return Err(vec![GlyimDiagnostic::internal_error(
                                    "slice/str value not a struct",
                                )]);
                            }
                        };
                        let len_val = self
                            .builder
                            .build_extract_value(struct_val, 1, "slice_len_extract")
                            .expect("extract value failed");
                        len_val.into_int_value()
                    }
                    other => {
                        return Err(vec![GlyimDiagnostic::internal_error(format!(
                            "Len on non-array/slice/str type: {:?}",
                            other
                        ))]);
                    }
                };
                Ok(len.as_basic_value_enum())
            }
            Rvalue::Cast(kind, operand, target_ty) => {
                let val = self.lower_operand(operand)?;
                self.lower_cast(*kind, val, self.operand_ty(operand), *target_ty)
            }
            Rvalue::Repeat(operand, count_const) => {
                let val = self.lower_operand(operand)?;
                let count = match &count_const.kind {
                    MirConstKind::Uint(n) => *n as u32,
                    MirConstKind::Int(n) => *n as u32,
                    _ => 0,
                };
                let elem_ty = val.get_type();
                let array_ty = elem_ty.array_type(count);
                let mut agg: inkwell::values::AggregateValueEnum<'ctx> =
                    array_ty.const_zero().into();
                for i in 0..count {
                    agg = self
                        .builder
                        .build_insert_value(agg, val, i, "repeat_insert")
                        .expect("insert value failed");
                }
                Ok(agg.as_basic_value_enum())
            }
        }
    }

    /// Construct an ADT (struct/enum variant) or closure aggregate value
    /// using real layout information from `glyim-layout`, rather than an
    /// ad hoc struct built purely from the operands' own LLVM types.
    ///
    /// This matters for two reasons:
    /// 1. The ad hoc struct type built from `vals.iter().map(|v|
    ///    v.get_type())` is a completely different (and differently laid
    ///    out) LLVM type than `llvm_type_for_ty(expected_ty)`, which is
    ///    what the destination place's memory was allocated/typed as.
    ///    Storing one shape into memory typed for the other is undefined
    ///    behavior.
    /// 2. For enums, it silently dropped the discriminant/niche tag
    ///    entirely -- constructing *any* multi-variant enum value produced
    ///    a value with no tag and the wrong size, guaranteed to be
    ///    misread by anything that later matches on it (`SwitchInt` over
    ///    `Rvalue::Discriminant`) or downcasts it.
    ///
    /// We build the value by allocating a temporary of the *canonical*
    /// LLVM type, storing each field (and the tag, if this is an enum) at
    /// its correct byte offset via `i8`-GEP, then loading the whole thing
    /// back out as the SSA value `lower_rvalue` is expected to return.
    fn build_layout_aggregate(
        &self,
        agg_ty: Ty,
        variant: Option<VariantIdx>,
        vals: &[BasicValueEnum<'ctx>],
    ) -> CompResult<BasicValueEnum<'ctx>> {
        let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
        let layout = layout_computer.layout_of(agg_ty).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "layout error building aggregate: {:?}",
                e
            ))]
        })?;
        let llvm_ty = self.llvm_type_for_ty(agg_ty)?;
        let tmp = self.builder.build_alloca(llvm_ty, "agg_tmp").map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "alloca failed: {:?}",
                e
            ))]
        })?;
        let i8_ptr_ty = self.context.ptr_type(AddressSpace::default());
        let base_i8 = self
            .builder
            .build_bit_cast(tmp, i8_ptr_ty, "agg_base_i8")
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "bitcast failed: {:?}",
                    e
                ))]
            })?
            .into_pointer_value();

        // Resolve which field-offset table to use for storing operands, and
        // (for enums) write the discriminant/niche tag.
        let (field_shape, base_offset): (FieldsShape, u64) = match &layout.variants {
            VariantsShape::Single { .. } => (layout.fields.clone(), 0),
            VariantsShape::Multiple {
                tag_field,
                tag_encoding,
                variants,
                tag_size,
                tag_align,
                ..
            } => {
                let vidx = variant.map(|v| v.to_raw() as usize).unwrap_or(0);
                let variant_layout = variants.get(vidx).ok_or_else(|| {
                    vec![GlyimDiagnostic::internal_error(
                        "variant index out of bounds building aggregate",
                    )]
                })?;
                match tag_encoding {
                    TagEncoding::Direct => {
                        // Tag lives at offset 0; variant data starts right
                        // after it, aligned to the variant's own alignment
                        // (mirrors `direct_tag_encoding` in glyim-layout).
                        let tag_bits = (tag_size.0 * 8).max(8) as u32;
                        let tag_llvm_ty = self.llvm_int_type(tag_bits);
                        let tag_ptr_raw = unsafe {
                            self.builder.build_in_bounds_gep(
                                self.context.i8_type(),
                                base_i8,
                                &[self.llvm_int_type(64).const_int(0, false)],
                                "tag_ptr_raw",
                            )
                        }
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "GEP failed: {:?}",
                                e
                            ))]
                        })?;
                        let discr_value = tag_llvm_ty.const_int(vidx as u64, false);
                        self.builder
                            .build_store(tag_ptr_raw, discr_value)
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "store tag failed: {:?}",
                                    e
                                ))]
                            })?;
                        let data_start = Size(tag_size.0)
                            .align_to(*tag_align)
                            .align_to(variant_layout.align)
                            .0;
                        (variant_layout.fields.clone(), data_start)
                    }
                    TagEncoding::Niche {
                        untagged_variant,
                        niche_variants,
                        niche_start,
                    } => {
                        if vidx != *untagged_variant as usize {
                            // Write `niche_start + (vidx - niche_variants.start())`
                            // into the tag field's slot. That slot's offset
                            // comes from the *outer* enum layout's own
                            // `fields`, which `build_niche_layout` (in
                            // glyim-layout) sets to the untagged variant's
                            // field offsets directly -- there is no separate
                            // tag prefix for niche encoding.
                            let rel = vidx as u128 - *niche_variants.start() as u128;
                            let niche_value = niche_start.wrapping_add(rel);
                            let tag_offset = match &layout.fields {
                                FieldsShape::Arbitrary { offsets } => offsets
                                    .as_slice()
                                    .get(*tag_field as usize)
                                    .map(|s| s.0)
                                    .unwrap_or(0),
                                _ => 0,
                            };
                            let tag_bits = (tag_size.0 * 8).max(8) as u32;
                            let tag_llvm_ty = self.llvm_int_type(tag_bits);
                            let tag_ptr_raw = unsafe {
                                self.builder.build_in_bounds_gep(
                                    self.context.i8_type(),
                                    base_i8,
                                    &[self.llvm_int_type(64).const_int(tag_offset, false)],
                                    "niche_ptr_raw",
                                )
                            }
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "GEP failed: {:?}",
                                    e
                                ))]
                            })?;
                            // `niche_value`/`niche_start` are u128 so that
                            // pointer- or char-sized niches can be encoded
                            // without loss; truncate to the tag's actual
                            // width when storing.
                            let niche_lo = (niche_value & u128::from(u64::MAX)) as u64;
                            let discr_value = tag_llvm_ty.const_int(niche_lo, false);
                            self.builder
                                .build_store(tag_ptr_raw, discr_value)
                                .map_err(|e| {
                                    vec![GlyimDiagnostic::internal_error(format!(
                                        "store niche failed: {:?}",
                                        e
                                    ))]
                                })?;
                        }
                        // Data fields for every variant (including the
                        // untagged one) live at the same offsets -- no
                        // separate tag prefix.
                        (variant_layout.fields.clone(), 0)
                    }
                }
            }
        };

        let offsets: Vec<u64> = match &field_shape {
            FieldsShape::Arbitrary { offsets } => offsets.iter().map(|s| s.0).collect(),
            FieldsShape::Primitive => vec![0],
            FieldsShape::Array { stride, count } => (0..*count).map(|i| i * stride.0).collect(),
        };
        for (i, val) in vals.iter().enumerate() {
            let byte_offset = base_offset + offsets.get(i).copied().unwrap_or(0);
            let field_ptr_raw = unsafe {
                self.builder.build_in_bounds_gep(
                    self.context.i8_type(),
                    base_i8,
                    &[self.llvm_int_type(64).const_int(byte_offset, false)],
                    "agg_field_raw",
                )
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "GEP failed: {:?}",
                    e
                ))]
            })?;
            let field_ptr = self
                .builder
                .build_bit_cast(
                    field_ptr_raw,
                    self.context.ptr_type(AddressSpace::default()),
                    "agg_field_ptr",
                )
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "bitcast failed: {:?}",
                        e
                    ))]
                })?
                .into_pointer_value();
            self.builder.build_store(field_ptr, *val).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "store field failed: {:?}",
                    e
                ))]
            })?;
        }
        self.builder
            .build_load(llvm_ty, tmp, "agg_load")
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "load failed: {:?}",
                    e
                ))]
            })
    }

    /// Read the discriminant of an enum place using its real layout,
    /// handling both `Direct` and niche-optimized tag encodings.
    ///
    /// The previous implementation loaded the *whole* place as a struct and
    /// blindly extracted field 0, which happened to work only for `Direct`
    /// encoding where `llvm_type_for_ty` puts the tag first, and was
    /// simply wrong for niche encoding (there is no explicit tag field to
    /// extract at all -- the discriminant must be *computed* from the
    /// niche field's value).
    fn lower_discriminant(&self, place: &Place) -> CompResult<BasicValueEnum<'ctx>> {
        let ptr = self.place_ptr(place)?;
        let ty = self.place_ty(place);
        let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
        let layout = layout_computer.layout_of(ty).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "layout error for discriminant: {:?}",
                e
            ))]
        })?;
        let i64_ty = self.llvm_int_type(64);
        match &layout.variants {
            VariantsShape::Single { index } => {
                Ok(i64_ty.const_int(*index as u64, false).as_basic_value_enum())
            }
            VariantsShape::Multiple {
                tag_field,
                tag_encoding,
                tag_size,
                ..
            } => {
                let tag_offset = match &layout.fields {
                    FieldsShape::Arbitrary { offsets } => offsets
                        .as_slice()
                        .get(*tag_field as usize)
                        .map(|s| s.0)
                        .unwrap_or(0),
                    _ => 0,
                };
                let i8_ptr_ty = self.context.ptr_type(AddressSpace::default());
                let base_i8 = self
                    .builder
                    .build_bit_cast(ptr, i8_ptr_ty, "discr_base_i8")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "bitcast failed: {:?}",
                            e
                        ))]
                    })?
                    .into_pointer_value();
                let offset_val = i64_ty.const_int(tag_offset, false);
                let tag_ptr = unsafe {
                    self.builder.build_in_bounds_gep(
                        self.context.i8_type(),
                        base_i8,
                        &[offset_val],
                        "discr_tag_ptr",
                    )
                }
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "GEP failed: {:?}",
                        e
                    ))]
                })?;
                let tag_bits = (tag_size.0 * 8).max(8) as u32;
                let tag_llvm_ty = self.llvm_int_type(tag_bits);
                let tag_val = self
                    .builder
                    .build_load(tag_llvm_ty, tag_ptr, "discr_tag_load")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "load failed: {:?}",
                            e
                        ))]
                    })?
                    .into_int_value();
                let tag64 = if tag_bits < 64 {
                    self.builder
                        .build_int_z_extend(tag_val, i64_ty, "discr_zext")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "zext failed: {:?}",
                                e
                            ))]
                        })?
                } else {
                    tag_val
                };
                match tag_encoding {
                    TagEncoding::Direct => Ok(tag64.as_basic_value_enum()),
                    TagEncoding::Niche {
                        untagged_variant,
                        niche_variants,
                        niche_start,
                    } => {
                        // Map the raw niche-field value back to a variant
                        // ordinal:
                        //   in [niche_start, niche_start + niche_len) -> that variant
                        //   otherwise                                 -> untagged_variant
                        let niche_len = (*niche_variants.end() as u128
                            - *niche_variants.start() as u128
                            + 1) as u64;
                        let start_lo = (*niche_start & u128::from(u64::MAX)) as u64;
                        let start_val = i64_ty.const_int(start_lo, false);
                        let end_val = i64_ty.const_int(start_lo.wrapping_add(niche_len), false);
                        let ge_start = self
                            .builder
                            .build_int_compare(
                                inkwell::IntPredicate::UGE,
                                tag64,
                                start_val,
                                "niche_ge",
                            )
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "cmp failed: {:?}",
                                    e
                                ))]
                            })?;
                        let lt_end = self
                            .builder
                            .build_int_compare(
                                inkwell::IntPredicate::ULT,
                                tag64,
                                end_val,
                                "niche_lt",
                            )
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "cmp failed: {:?}",
                                    e
                                ))]
                            })?;
                        let in_range = self
                            .builder
                            .build_and(ge_start, lt_end, "niche_in_range")
                            .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "and failed: {:?}",
                                e
                            ))]
                        })?;
                        let rel = self
                            .builder
                            .build_int_sub(tag64, start_val, "niche_rel")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "sub failed: {:?}",
                                    e
                                ))]
                            })?;
                        let variant_from_niche = self
                            .builder
                            .build_int_add(
                                rel,
                                i64_ty.const_int(*niche_variants.start() as u64, false),
                                "niche_variant",
                            )
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "add failed: {:?}",
                                    e
                                ))]
                            })?;
                        let untagged_val = i64_ty.const_int(*untagged_variant as u64, false);
                        let result = self
                            .builder
                            .build_select(
                                in_range,
                                variant_from_niche,
                                untagged_val,
                                "discr_select",
                            )
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "select failed: {:?}",
                                    e
                                ))]
                            })?;
                        Ok(result)
                    }
                }
            }
        }
    }

    fn is_signed_int_ty(&self, ty: Ty) -> bool {
        matches!(self.ty_ctx.ty_kind(ty), TyKind::Int(_))
    }

    fn lower_binop(
        &self,
        op: BinOp,
        l: BasicValueEnum<'ctx>,
        r: BasicValueEnum<'ctx>,
        operand_ty: Ty,
    ) -> CompResult<BasicValueEnum<'ctx>> {
        use inkwell::FloatPredicate;
        use inkwell::IntPredicate;

        match op {
            BinOp::Add => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_int_add(l.into_int_value(), r.into_int_value(), "add")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "add failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_add(l.into_float_value(), r.into_float_value(), "add")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "add failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "add: mismatched types",
                    )])
                }
            }
            BinOp::Sub => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_int_sub(l.into_int_value(), r.into_int_value(), "sub")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "sub failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_sub(l.into_float_value(), r.into_float_value(), "sub")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "sub failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "sub: mismatched types",
                    )])
                }
            }
            BinOp::Mul => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_int_mul(l.into_int_value(), r.into_int_value(), "mul")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "mul failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_mul(l.into_float_value(), r.into_float_value(), "mul")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "mul failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "mul: mismatched types",
                    )])
                }
            }
            BinOp::Div => {
                if l.is_int_value() && r.is_int_value() {
                    let result = if self.is_signed_int_ty(operand_ty) {
                        self.builder.build_int_signed_div(
                            l.into_int_value(),
                            r.into_int_value(),
                            "div",
                        )
                    } else {
                        self.builder.build_int_unsigned_div(
                            l.into_int_value(),
                            r.into_int_value(),
                            "div",
                        )
                    };
                    result.map(|v| v.as_basic_value_enum()).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "div failed: {:?}",
                            e
                        ))]
                    })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_div(l.into_float_value(), r.into_float_value(), "div")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "div failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "div: mismatched types",
                    )])
                }
            }
            BinOp::Rem => {
                if l.is_int_value() && r.is_int_value() {
                    let result = if self.is_signed_int_ty(operand_ty) {
                        self.builder.build_int_signed_rem(
                            l.into_int_value(),
                            r.into_int_value(),
                            "rem",
                        )
                    } else {
                        self.builder.build_int_unsigned_rem(
                            l.into_int_value(),
                            r.into_int_value(),
                            "rem",
                        )
                    };
                    result.map(|v| v.as_basic_value_enum()).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "rem failed: {:?}",
                            e
                        ))]
                    })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_rem(l.into_float_value(), r.into_float_value(), "rem")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "rem failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "rem: mismatched types",
                    )])
                }
            }
            BinOp::Eq => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            l.into_int_value(),
                            r.into_int_value(),
                            "eq",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "eq failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OEQ,
                            l.into_float_value(),
                            r.into_float_value(),
                            "eq",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "eq failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "eq: mismatched types",
                    )])
                }
            }
            BinOp::Ne => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_int_compare(
                            IntPredicate::NE,
                            l.into_int_value(),
                            r.into_int_value(),
                            "ne",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "ne failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_compare(
                            FloatPredicate::ONE,
                            l.into_float_value(),
                            r.into_float_value(),
                            "ne",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "ne failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "ne: mismatched types",
                    )])
                }
            }
            BinOp::Lt => {
                if l.is_int_value() && r.is_int_value() {
                    // Use the *unsigned* predicate when the operand type is
                    // unsigned (LL-1): a `u32`/`u64`/`usize` comparison of a
                    // value >= 2^63 was miscompiled as signed.
                    let cmp_pred = if self.is_signed_int_ty(operand_ty) {
                        IntPredicate::SLT
                    } else {
                        IntPredicate::ULT
                    };
                    self.builder
                        .build_int_compare(cmp_pred, l.into_int_value(), r.into_int_value(), "lt")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "lt failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OLT,
                            l.into_float_value(),
                            r.into_float_value(),
                            "lt",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "lt failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "lt: mismatched types",
                    )])
                }
            }
            BinOp::Gt => {
                if l.is_int_value() && r.is_int_value() {
                    // Use the *unsigned* predicate when the operand type is
                    // unsigned (LL-1): a `u32`/`u64`/`usize` comparison of a
                    // value >= 2^63 was miscompiled as signed.
                    let cmp_pred = if self.is_signed_int_ty(operand_ty) {
                        IntPredicate::SGT
                    } else {
                        IntPredicate::UGT
                    };
                    self.builder
                        .build_int_compare(cmp_pred, l.into_int_value(), r.into_int_value(), "gt")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "gt failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OGT,
                            l.into_float_value(),
                            r.into_float_value(),
                            "gt",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "gt failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "gt: mismatched types",
                    )])
                }
            }
            BinOp::LtEq => {
                if l.is_int_value() && r.is_int_value() {
                    // Use the *unsigned* predicate when the operand type is
                    // unsigned (LL-1): a `u32`/`u64`/`usize` comparison of a
                    // value >= 2^63 was miscompiled as signed.
                    let cmp_pred = if self.is_signed_int_ty(operand_ty) {
                        IntPredicate::SLE
                    } else {
                        IntPredicate::ULE
                    };
                    self.builder
                        .build_int_compare(cmp_pred, l.into_int_value(), r.into_int_value(), "le")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "le failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OLE,
                            l.into_float_value(),
                            r.into_float_value(),
                            "le",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "le failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "le: mismatched types",
                    )])
                }
            }
            BinOp::GtEq => {
                if l.is_int_value() && r.is_int_value() {
                    // Use the *unsigned* predicate when the operand type is
                    // unsigned (LL-1): a `u32`/`u64`/`usize` comparison of a
                    // value >= 2^63 was miscompiled as signed.
                    let cmp_pred = if self.is_signed_int_ty(operand_ty) {
                        IntPredicate::SGE
                    } else {
                        IntPredicate::UGE
                    };
                    self.builder
                        .build_int_compare(cmp_pred, l.into_int_value(), r.into_int_value(), "ge")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "ge failed: {:?}",
                                e
                            ))]
                        })
                } else if l.is_float_value() && r.is_float_value() {
                    self.builder
                        .build_float_compare(
                            FloatPredicate::OGE,
                            l.into_float_value(),
                            r.into_float_value(),
                            "ge",
                        )
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "ge failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "ge: mismatched types",
                    )])
                }
            }
            BinOp::And => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_and(l.into_int_value(), r.into_int_value(), "and")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "and failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "and: expected integer types",
                    )])
                }
            }
            BinOp::Or => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_or(l.into_int_value(), r.into_int_value(), "or")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "or failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "or: expected integer types",
                    )])
                }
            }
            BinOp::BitAnd => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_and(l.into_int_value(), r.into_int_value(), "bitand")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "bitand failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "bitand: expected integer types",
                    )])
                }
            }
            BinOp::BitOr => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_or(l.into_int_value(), r.into_int_value(), "bitor")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "bitor failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "bitor: expected integer types",
                    )])
                }
            }
            BinOp::BitXor => {
                if l.is_int_value() && r.is_int_value() {
                    self.builder
                        .build_xor(l.into_int_value(), r.into_int_value(), "bitxor")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "bitxor failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "bitxor: expected integer types",
                    )])
                }
            }
            BinOp::Shl => {
                if l.is_int_value() && r.is_int_value() {
                    let lv = l.into_int_value();
                    let rv = r.into_int_value();
                    // T113-PATCHED [LL-18]: the language defines over-shifts
                    // as wrapping (the VM/interp use `wrapping_shl`), so we
                    // mask the shift amount to `bits - 1` before calling
                    // LLVM `shl`. Without the mask, LLVM treats a shift of
                    // >= bitwidth as poison — UB — and different targets
                    // produce different results (x86 masks by accident,
                    // AArch64 yields 0). The `and` mask matches the VM.
                    let bits = lv.get_type().get_bit_width();
                    let mask = rv.get_type().const_int((bits - 1) as u64, false);
                    let masked = self.builder.build_and(rv, mask, "shl_mask").map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "shl mask failed: {:?}",
                            e
                        ))]
                    })?;
                    self.builder
                        .build_left_shift(lv, masked, "shl")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "shl failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "shl: expected integer types",
                    )])
                }
            }
            BinOp::Shr => {
                if l.is_int_value() && r.is_int_value() {
                    let signed = self.is_signed_int_ty(operand_ty);
                    let lv = l.into_int_value();
                    let rv = r.into_int_value();
                    // T113-PATCHED [LL-18]: same masking as Shl above.
                    let bits = lv.get_type().get_bit_width();
                    let mask = rv.get_type().const_int((bits - 1) as u64, false);
                    let masked = self.builder.build_and(rv, mask, "shr_mask").map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "shr mask failed: {:?}",
                            e
                        ))]
                    })?;
                    self.builder
                        .build_right_shift(lv, masked, signed, "shr")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "shr failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "shr: expected integer types",
                    )])
                }
            }
        }
    }
    fn lower_unop(
        &self,
        op: UnOp,
        operand: &Operand,
        val: BasicValueEnum<'ctx>,
    ) -> CompResult<BasicValueEnum<'ctx>> {
        match op {
            UnOp::Not => {
                if val.is_int_value() {
                    self.builder
                        .build_not(val.into_int_value(), "not")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "not failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "not: expected integer type",
                    )])
                }
            }
            UnOp::Neg => {
                if val.is_int_value() {
                    self.builder
                        .build_int_neg(val.into_int_value(), "neg")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "neg failed: {:?}",
                                e
                            ))]
                        })
                } else if val.is_float_value() {
                    self.builder
                        .build_float_neg(val.into_float_value(), "neg")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "neg failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "neg: expected numeric type",
                    )])
                }
            }
            UnOp::Deref => {
                let ty = self.operand_ty(operand);
                let inner_ty = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                    _ => Ty::ERROR,
                };
                if val.is_pointer_value() && inner_ty != Ty::ERROR {
                    let llvm_ty = self.llvm_type_for_ty(inner_ty)?;
                    self.builder
                        .build_load(llvm_ty, val.into_pointer_value(), "deref_load")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "deref load failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Ok(val)
                }
            }
        }
    }

    fn lower_cast(
        &self,
        kind: CastKind,
        val: BasicValueEnum<'ctx>,
        src_ty: Ty,
        target_ty: Ty,
    ) -> CompResult<BasicValueEnum<'ctx>> {
        let target_llvm_ty = self.llvm_type_for_ty(target_ty)?;
        match kind {
            CastKind::IntToInt => {
                if val.get_type() == target_llvm_ty {
                    Ok(val)
                } else if val.is_int_value() && target_llvm_ty.is_int_type() {
                    let int_val = val.into_int_value();
                    let target_bits = target_llvm_ty.into_int_type().get_bit_width();
                    let src_bits = int_val.get_type().get_bit_width();
                    if target_bits == src_bits {
                        Ok(int_val.as_basic_value_enum())
                    } else if target_bits > src_bits {
                        // Sign-extend a signed source, zero-extend otherwise
                        // (LL-2). Widening `i32(-1)` to `i64` must be sext.
                        if self.is_signed_int_ty(src_ty) {
                            self.builder.build_int_s_extend(
                                int_val,
                                target_llvm_ty.into_int_type(),
                                "sext",
                            )
                        } else {
                            self.builder.build_int_z_extend(
                                int_val,
                                target_llvm_ty.into_int_type(),
                                "zext",
                            )
                        }
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "zext failed: {:?}",
                                e
                            ))]
                        })
                    } else {
                        self.builder
                            .build_int_truncate(int_val, target_llvm_ty.into_int_type(), "trunc")
                            .map(|v| v.as_basic_value_enum())
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "trunc failed: {:?}",
                                    e
                                ))]
                            })
                    }
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "IntToInt cast on non-integer",
                    )])
                }
            }
            CastKind::FloatToInt => {
                if val.is_float_value() && target_llvm_ty.is_int_type() {
                    let float_val = val.into_float_value();
                    // `fptoui` for an unsigned target, `fptosi` otherwise
                    // (LL-3): `1e19 as u64` was UB / wrong via fptosi.
                    if matches!(self.ty_ctx.ty_kind(target_ty), TyKind::Uint(_)) {
                        self.builder.build_float_to_unsigned_int(
                            float_val,
                            target_llvm_ty.into_int_type(),
                            "fptoui",
                        )
                    } else {
                        self.builder.build_float_to_signed_int(
                            float_val,
                            target_llvm_ty.into_int_type(),
                            "fptosi",
                        )
                    }
                    .map(|v| v.as_basic_value_enum())
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "fptosi failed: {:?}",
                            e
                        ))]
                    })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "FloatToInt cast on non-float",
                    )])
                }
            }
            CastKind::IntToFloat => {
                if val.is_int_value() && target_llvm_ty.is_float_type() {
                    let int_val = val.into_int_value();
                    // `uitofp` for an unsigned source, `sitofp` otherwise
                    // (LL-4): a `u64` >= 2^63 converted via sitofp gave a
                    // negative float.
                    if matches!(self.ty_ctx.ty_kind(src_ty), TyKind::Uint(_)) {
                        self.builder.build_unsigned_int_to_float(
                            int_val,
                            target_llvm_ty.into_float_type(),
                            "uitofp",
                        )
                    } else {
                        self.builder.build_signed_int_to_float(
                            int_val,
                            target_llvm_ty.into_float_type(),
                            "sitofp",
                        )
                    }
                    .map(|v| v.as_basic_value_enum())
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "sitofp failed: {:?}",
                            e
                        ))]
                    })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "IntToFloat cast on non-int",
                    )])
                }
            }
            CastKind::FloatToFloat => {
                if val.is_float_value() && target_llvm_ty.is_float_type() {
                    let float_val = val.into_float_value();
                    let target_float_ty = target_llvm_ty.into_float_type();
                    let src_is_f64 = float_val.get_type() == self.context.f64_type();
                    let target_is_f64 = target_float_ty == self.context.f64_type();
                    if src_is_f64 == target_is_f64 {
                        Ok(float_val.as_basic_value_enum())
                    } else if !src_is_f64 && target_is_f64 {
                        self.builder
                            .build_float_ext(float_val, target_float_ty, "fpext")
                            .map(|v| v.as_basic_value_enum())
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "fpext failed: {:?}",
                                    e
                                ))]
                            })
                    } else {
                        self.builder
                            .build_float_trunc(float_val, target_float_ty, "fptrunc")
                            .map(|v| v.as_basic_value_enum())
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "fptrunc failed: {:?}",
                                    e
                                ))]
                            })
                    }
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "FloatToFloat cast on non-float",
                    )])
                }
            }
            CastKind::PtrToPtr | CastKind::FnPtrToPtr => {
                if val.is_pointer_value() && target_llvm_ty.is_pointer_type() {
                    self.builder
                        .build_bit_cast(val, target_llvm_ty, "bitcast")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "bitcast failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "PtrToPtr cast on non-pointer",
                    )])
                }
            }
            CastKind::PtrToInt => {
                if val.is_pointer_value() && target_llvm_ty.is_int_type() {
                    let ptr_val = val.into_pointer_value();
                    self.builder
                        .build_ptr_to_int(ptr_val, target_llvm_ty.into_int_type(), "ptrtoint")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "ptrtoint failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "PtrToInt cast on non-pointer",
                    )])
                }
            }
            CastKind::IntToPtr => {
                if val.is_int_value() && target_llvm_ty.is_pointer_type() {
                    let int_val = val.into_int_value();
                    self.builder
                        .build_int_to_ptr(int_val, target_llvm_ty.into_pointer_type(), "inttoptr")
                        .map(|v| v.as_basic_value_enum())
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "inttoptr failed: {:?}",
                                e
                            ))]
                        })
                } else {
                    Err(vec![GlyimDiagnostic::internal_error(
                        "IntToPtr cast on non-int",
                    )])
                }
            }
        }
    }

    fn lower_terminator(&mut self, terminator: &Terminator) -> CompResult<()> {
        self.set_debug_location(terminator.source_info.span);
        match &terminator.kind {
            TerminatorKind::Goto { target } => {
                let target_bb = self.bb_map.get(target).unwrap();
                self.builder
                    .build_unconditional_branch(*target_bb)
                    .expect("branch failed");
                Ok(())
            }
            TerminatorKind::SwitchInt {
                discr,
                switch_ty,
                targets,
            } => {
                let discr_val = self.lower_operand(discr)?;
                let discr_int = discr_val.into_int_value();
                let otherwise = *self.bb_map.get(&targets.otherwise()).unwrap();
                let case_ty = self.llvm_type_for_ty(*switch_ty)?.into_int_type();

                if targets.iter().count() == 0 {
                    self.builder
                        .build_unconditional_branch(otherwise)
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "branch failed: {:?}",
                                e
                            ))]
                        })?;
                } else {
                    let mut branches = Vec::new();
                    for (val, bb) in targets.iter() {
                        let const_val = case_ty.const_int(val as u64, false);
                        let bb_val = *self.bb_map.get(&bb).unwrap();
                        branches.push((const_val, bb_val));
                    }
                    self.builder
                        .build_switch(discr_int, otherwise, &branches)
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "switch failed: {:?}",
                                e
                            ))]
                        })?;
                }
                Ok(())
            }
            TerminatorKind::Return => {
                let ret_ty = self.body.return_ty;
                if matches!(self.ty_ctx.ty_kind(ret_ty), TyKind::Never | TyKind::Unit)
                    || ret_ty == Ty::NEVER
                    || ret_ty == Ty::UNIT
                {
                    self.builder.build_return(None).expect("return failed");
                } else if self.is_sret {
                    // Composite return via hidden sret pointer (first parameter).
                    let ret_op =
                        glyim_mir::Operand::Move(glyim_mir::Place::new(LocalIdx::from_raw(0)));
                    let val = self.lower_operand(&ret_op)?;
                    let params = self.function.get_params();
                    let sret_ptr = params
                        .first()
                        .expect("sret parameter missing")
                        .into_pointer_value();
                    self.builder
                        .build_store(sret_ptr, val)
                        .expect("store sret failed");
                    self.builder.build_return(None).expect("return failed");
                } else {
                    let ret_op =
                        glyim_mir::Operand::Move(glyim_mir::Place::new(LocalIdx::from_raw(0)));
                    let val = self.lower_operand(&ret_op)?;
                    self.builder
                        .build_return(Some(&val))
                        .expect("return failed");
                }
                Ok(())
            }
            TerminatorKind::Unreachable => {
                // If we're at the end of a cleanup block that has an active
                // landingpad, "falling off the end" means "nothing local
                // wanted to catch/handle this unwind" -- the correct action
                // is to resume unwinding, not to assert unreachable (which
                // would be undefined behavior the very first time a
                // destructor actually runs during a panic).
                if let Some(lp) = self.current_landingpad {
                    self.builder.build_resume(lp).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "resume failed: {:?}",
                            e
                        ))]
                    })?;
                } else if let Some(pad) = self.current_seh_pad {
                    // SEH funclet path: terminate the cleanup block with a
                    // `cleanupret` to the function's unwind continuation. A
                    // null unwind continuation means "unwind to the caller's
                    // pad" (the standard behavior for a top-level cleanuppad
                    // that doesn't catch -- it forwards the exception outward).
                    use crate::seh_ffi::LLVMBuildCleanupRet;
                    unsafe {
                        LLVMBuildCleanupRet(self.builder.as_mut_ptr(), pad, std::ptr::null_mut());
                    }
                } else {
                    self.builder
                        .build_unreachable()
                        .expect("unreachable failed");
                }
                Ok(())
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                cleanup,
            } => self.lower_call(func, args, destination, target, cleanup),
            TerminatorKind::Assert {
                cond,
                expected,
                target,
                cleanup,
                msg,
            } => {
                let cond_int = self.lower_operand(cond)?.into_int_value();
                let expected_val = self
                    .llvm_int_type(1)
                    .const_int(if *expected { 1 } else { 0 }, false);
                let cmp = self
                    .builder
                    .build_int_compare(
                        inkwell::IntPredicate::EQ,
                        cond_int,
                        expected_val,
                        "assert_check",
                    )
                    .expect("assert cmp failed");
                let target_bb = *self.bb_map.get(target).unwrap();
                let fail_bb = self
                    .context
                    .append_basic_block(self.function, "assert_fail");
                self.builder
                    .build_conditional_branch(cmp, target_bb, fail_bb)
                    .expect("assert br failed");
                self.builder.position_at_end(fail_bb);

                // Route assertion failure through the runtime panic entry
                // point rather than a bare `llvm.trap`, so that (a) it goes
                // through the same path as any other panic, and (b) when a
                // cleanup block exists we can `invoke` it, giving unwinding
                // builds a real unwind edge to run destructors on before the
                // process aborts. `glyim_panic` is `-> !`, so the "normal"
                // continuation of the invoke is never actually reached, but
                // LLVM's `invoke` still requires one.
                let module = self.module;
                let i8_ptr_ty = self.context.ptr_type(AddressSpace::default());
                let i64_ty = self.llvm_int_type(64);
                let panic_fn = module.get_function("glyim_panic").unwrap_or_else(|| {
                    let fn_ty = self
                        .context
                        .void_type()
                        .fn_type(&[i8_ptr_ty.into(), i64_ty.into()], false);
                    module.add_function("glyim_panic", fn_ty, None)
                });
                let msg_str = format!("assertion failed: {:?}", msg);
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                msg_str.hash(&mut hasher);
                let hash = hasher.finish();
                let global_name = format!("__glyim_assert_{:016x}", hash);
                let msg_global = module.get_global(&global_name).unwrap_or_else(|| {
                    let const_str = self.context.const_string(msg_str.as_bytes(), true);
                    let i8_type = self.context.i8_type();
                    let str_type = i8_type.array_type(msg_str.len() as u32 + 1);
                    let global = module.add_global(
                        str_type,
                        Some(inkwell::AddressSpace::default()),
                        &global_name,
                    );
                    global.set_initializer(&const_str);
                    global.set_constant(true);
                    global.set_linkage(inkwell::module::Linkage::Private);
                    global
                });
                let msg_ptr = self
                    .builder
                    .build_bit_cast(msg_global.as_pointer_value(), i8_ptr_ty, "msg_ptr")
                    .expect("bitcast failed")
                    .into_pointer_value();
                let msg_len = i64_ty.const_int(msg_str.len() as u64, false);
                let panic_args: Vec<BasicMetadataValueEnum<'ctx>> =
                    vec![msg_ptr.into(), msg_len.into()];

                if let Some(cleanup_bb_idx) = cleanup {
                    let cleanup_bb = *self.bb_map.get(cleanup_bb_idx).unwrap();
                    let unreachable_cont = self
                        .context
                        .append_basic_block(self.function, "assert_panic_cont");
                    let panic_args_vals: Vec<BasicValueEnum<'ctx>> =
                        vec![msg_ptr.as_basic_value_enum(), msg_len.as_basic_value_enum()];
                    self.builder
                        .build_invoke(
                            panic_fn,
                            &panic_args_vals,
                            unreachable_cont,
                            cleanup_bb,
                            "panic_call",
                        )
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "invoke failed: {:?}",
                                e
                            ))]
                        })?;
                    self.builder.position_at_end(unreachable_cont);
                    self.builder.build_unreachable().map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "unreachable failed: {:?}",
                            e
                        ))]
                    })?;
                } else {
                    self.builder
                        .build_call(panic_fn, &panic_args, "panic_call")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "call failed: {:?}",
                                e
                            ))]
                        })?;
                    self.builder.build_unreachable().map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "unreachable failed: {:?}",
                            e
                        ))]
                    })?;
                }
                Ok(())
            }
            TerminatorKind::Drop {
                place,
                target,
                cleanup,
            } => {
                let target_bb = *self.bb_map.get(target).unwrap();
                let cleanup_bb = cleanup.map(|c| *self.bb_map.get(&c).unwrap());
                // A `Drop` on a place whose *base local* is `()` or
                // `Ty::ERROR` is spurious. Check the base local's declared
                // type directly (before `place_ty`, which would log
                // `Field projection on non-tuple/ADT type` for e.g.
                // `_0.Field(0)` when `_0: ()`), so the skip is silent.
                let base_local_ty = self.body.locals[place.local].ty;
                if base_local_ty == Ty::ERROR || base_local_ty == Ty::UNIT {
                    self.builder
                        .build_unconditional_branch(target_bb)
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "branch failed: {:?}",
                                e
                            ))]
                        })?;
                    return Ok(());
                }
                let ty = self.place_ty(place);
                if ty == Ty::ERROR || ty == Ty::UNIT {
                    self.builder
                        .build_unconditional_branch(target_bb)
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "branch failed: {:?}",
                                e
                            ))]
                        })?;
                    return Ok(());
                }
                let needs_drop = self.type_needs_drop(ty);
                let is_owning = self.type_is_owning_pointer(ty);

                if needs_drop || is_owning {
                    // T005-PATCHED-DECL [LL-15]: the runtime symbol takes
                    // `(ptr: *mut u8, drop_fn: Option<DropFn>)`; the second
                    // parameter is niche-optimised to a raw function
                    // pointer. Declaring the symbol with only one argument
                    // let the callee read a garbage register as `drop_fn`
                    // — a call through an arbitrary pointer (SIGSEGV/UB)
                    // or a silent no-drop. Both the decl and the call site
                    // now pass a null second argument until per-type drop
                    // glue is wired end-to-end.
                    let drop_fn = self
                        .module
                        .get_function("glyim_drop_in_place")
                        .unwrap_or_else(|| {
                            let ptr_ty = self.context.ptr_type(AddressSpace::default());
                            let fn_type = self
                                .context
                                .void_type()
                                .fn_type(&[ptr_ty.into(), ptr_ty.into()], false);
                            self.module
                                .add_function("glyim_drop_in_place", fn_type, None)
                        });

                    let drop_arg = if matches!(
                        self.ty_ctx.ty_kind(ty),
                        TyKind::Ref(_, _, _) | TyKind::RawPtr(_, _)
                    ) {
                        let val = self.lower_operand(&Operand::Move(place.clone()))?;
                        // A fat pointer (`&str` / `&[T]`) loads as a
                        // `{data_ptr, len}` struct; `into_pointer_value()`
                        // would panic on it. Extract field 0 (the data
                        // pointer) so `glyim_drop_in_place` receives the
                        // pointee address, exactly like the thin-pointer
                        // case below. This mirrors the identical extract in
                        // `place_ptr`'s `ProjectionElem::Deref` arm.
                        let ptr_val = if val.is_struct_value() {
                            self.builder
                                .build_extract_value(val.into_struct_value(), 0, "fat_drop_ptr")
                                .map_err(|e| {
                                    vec![GlyimDiagnostic::internal_error(format!(
                                        "fat-pointer data extract for drop failed: {:?}",
                                        e
                                    ))]
                                })?
                                .into_pointer_value()
                        } else {
                            val.into_pointer_value()
                        };
                        ptr_val.into()
                    } else {
                        self.place_ptr(place)?.into()
                    };
                    // T005-PATCHED-CALL [LL-15]: pass the null `drop_fn`
                    // the runtime expects as its second argument.
                    let null_drop_fn = self.context.ptr_type(AddressSpace::default()).const_null();
                    let args_vals: Vec<BasicValueEnum<'ctx>> = vec![drop_arg, null_drop_fn.into()];

                    if let Some(cleanup_bb) = cleanup_bb {
                        let normal_bb = self.context.append_basic_block(self.function, "drop_cont");
                        self.builder
                            .build_invoke(drop_fn, &args_vals, normal_bb, cleanup_bb, "drop_call")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "invoke failed: {:?}",
                                    e
                                ))]
                            })?;

                        self.builder.position_at_end(normal_bb);
                        if is_owning {
                            let dealloc_fn = self
                                .module
                                .get_function("glyim_dealloc")
                                .unwrap_or_else(|| {
                                    let fn_type = self.context.void_type().fn_type(
                                        &[
                                            self.context.ptr_type(AddressSpace::default()).into(),
                                            self.llvm_int_type(64).into(),
                                            self.llvm_int_type(64).into(),
                                        ],
                                        false,
                                    );
                                    self.module.add_function("glyim_dealloc", fn_type, None)
                                });
                            let val = self.lower_operand(&Operand::Move(place.clone()))?;
                            // A `&str` / `&[T]` fat pointer loads as a
                            // `{data_ptr, len}` struct; extract field 0 so
                            // `glyim_dealloc` receives the pointee address.
                            let ptr = if val.is_struct_value() {
                                self.builder
                                    .build_extract_value(
                                        val.into_struct_value(),
                                        0,
                                        "fat_dealloc_ptr",
                                    )
                                    .map_err(|e| {
                                        vec![GlyimDiagnostic::internal_error(format!(
                                            "fat-pointer data extract for dealloc failed: {:?}",
                                            e
                                        ))]
                                    })?
                                    .into_pointer_value()
                            } else {
                                val.into_pointer_value()
                            };
                            let pointee_ty = match self.ty_ctx.ty_kind(ty) {
                                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                                _ => unreachable!(),
                            };
                            let layout_computer =
                                FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
                            if let Ok(layout) = layout_computer.layout_of(pointee_ty) {
                                let size_val =
                                    self.llvm_int_type(64).const_int(layout.size.0, false);
                                let align_val =
                                    self.llvm_int_type(64).const_int(layout.align.0, false);
                                let dealloc_args: Vec<BasicMetadataValueEnum<'ctx>> =
                                    vec![ptr.into(), size_val.into(), align_val.into()];
                                self.builder
                                    .build_call(dealloc_fn, &dealloc_args, "dealloc_call")
                                    .map_err(|e| {
                                        vec![GlyimDiagnostic::internal_error(format!(
                                            "dealloc call failed: {:?}",
                                            e
                                        ))]
                                    })?;
                            }
                        }
                        self.builder
                            .build_unconditional_branch(target_bb)
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "branch failed: {:?}",
                                    e
                                ))]
                            })?;
                    } else {
                        let args_meta: Vec<BasicMetadataValueEnum<'ctx>> =
                            args_vals.iter().map(|&v| v.into()).collect();
                        self.builder
                            .build_call(drop_fn, &args_meta, "drop_call")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "call failed: {:?}",
                                    e
                                ))]
                            })?;

                        if is_owning {
                            let dealloc_fn = self
                                .module
                                .get_function("glyim_dealloc")
                                .unwrap_or_else(|| {
                                    let fn_type = self.context.void_type().fn_type(
                                        &[
                                            self.context.ptr_type(AddressSpace::default()).into(),
                                            self.llvm_int_type(64).into(),
                                            self.llvm_int_type(64).into(),
                                        ],
                                        false,
                                    );
                                    self.module.add_function("glyim_dealloc", fn_type, None)
                                });
                            let val = self.lower_operand(&Operand::Move(place.clone()))?;
                            // A `&str` / `&[T]` fat pointer loads as a
                            // `{data_ptr, len}` struct; extract field 0 so
                            // `glyim_dealloc` receives the pointee address.
                            let ptr = if val.is_struct_value() {
                                self.builder
                                    .build_extract_value(
                                        val.into_struct_value(),
                                        0,
                                        "fat_dealloc_ptr",
                                    )
                                    .map_err(|e| {
                                        vec![GlyimDiagnostic::internal_error(format!(
                                            "fat-pointer data extract for dealloc failed: {:?}",
                                            e
                                        ))]
                                    })?
                                    .into_pointer_value()
                            } else {
                                val.into_pointer_value()
                            };
                            let pointee_ty = match self.ty_ctx.ty_kind(ty) {
                                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                                _ => unreachable!(),
                            };
                            let layout_computer =
                                FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
                            if let Ok(layout) = layout_computer.layout_of(pointee_ty) {
                                let size_val =
                                    self.llvm_int_type(64).const_int(layout.size.0, false);
                                let align_val =
                                    self.llvm_int_type(64).const_int(layout.align.0, false);
                                let dealloc_args: Vec<BasicMetadataValueEnum<'ctx>> =
                                    vec![ptr.into(), size_val.into(), align_val.into()];
                                self.builder
                                    .build_call(dealloc_fn, &dealloc_args, "dealloc_call")
                                    .map_err(|e| {
                                        vec![GlyimDiagnostic::internal_error(format!(
                                            "dealloc call failed: {:?}",
                                            e
                                        ))]
                                    })?;
                            }
                        }
                        self.builder
                            .build_unconditional_branch(target_bb)
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "branch failed: {:?}",
                                    e
                                ))]
                            })?;
                    }
                } else {
                    self.builder
                        .build_unconditional_branch(target_bb)
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "branch failed: {:?}",
                                e
                            ))]
                        })?;
                }
                Ok(())
            }
        }
    }

    /// Recursively concretize a `FnSig` whose generic `Param(i)` slots reference
    /// the call-site substitution. `param_map[i]` is the concrete type for
    /// `Param(i)`. Associated-type projections (`F::Output`, `Self::Output`) are
    /// normalized via the projection table. This is what lets generic/async
    /// callees lower without an `UnknownType(Param)` ICE: a synthetic
    /// `Poll<Self::Output>` return type becomes a concrete `Poll<i32>`.
    fn concretize_fn_sig(sig: &FnSig, param_map: &[GenericArg], ty_ctx: &TyCtx) -> FnSig {
        let mut ctx = TyCtxMut::from_ty_ctx(ty_ctx);
        fn subst(ty: Ty, map: &[GenericArg], ctx: &mut TyCtxMut, frozen: &TyCtx) -> Ty {
            match frozen.ty_kind(ty).clone() {
                TyKind::Param(p) => match map.get(p.index as usize) {
                    Some(GenericArg::Ty(t)) => *t,
                    _ => ty,
                },
                TyKind::Ref(r, inner, m) => {
                    let ni = subst(inner, map, ctx, frozen);
                    if ni == inner {
                        ty
                    } else {
                        ctx.mk_ty(TyKind::Ref(r, ni, m))
                    }
                }
                TyKind::RawPtr(inner, m) => {
                    let ni = subst(inner, map, ctx, frozen);
                    if ni == inner {
                        ty
                    } else {
                        ctx.mk_ty(TyKind::RawPtr(ni, m))
                    }
                }
                TyKind::Adt(id, sub) => {
                    let args = frozen.substitution_args(sub);
                    let mut new_args: Vec<GenericArg> = Vec::new();
                    for a in args.iter() {
                        match a {
                            GenericArg::Ty(t) => {
                                new_args.push(GenericArg::Ty(subst(*t, map, ctx, frozen)))
                            }
                            other => new_args.push(other.clone()),
                        }
                    }
                    if new_args == args.to_vec() {
                        ty
                    } else {
                        let sub = ctx.intern_substitution(new_args);
                        ctx.mk_ty(TyKind::Adt(id, sub))
                    }
                }
                TyKind::Tuple(sub) => {
                    let args = frozen.substitution_args(sub);
                    let mut new_args: Vec<GenericArg> = Vec::new();
                    for a in args.iter() {
                        match a {
                            GenericArg::Ty(t) => {
                                new_args.push(GenericArg::Ty(subst(*t, map, ctx, frozen)))
                            }
                            other => new_args.push(other.clone()),
                        }
                    }
                    if new_args == args.to_vec() {
                        ty
                    } else {
                        let sub = ctx.intern_substitution(new_args);
                        ctx.mk_ty(TyKind::Tuple(sub))
                    }
                }
                TyKind::Projection(proj) => {
                    // Substitute the projection's self type (may be a `Param`
                    // referencing the instantiation substs), then resolve.
                    let proj_subst_args = frozen.substitution_args(proj.trait_ref.substs);
                    let proj_self = proj_subst_args.first().and_then(|a| match a {
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
                    let concrete_self = subst(concrete_self, map, ctx, frozen);
                    // `self` may be wrapped in `&mut`/`*` (e.g. `poll`'s
                    // `&mut self`), but the impl table is keyed by the bare ADT.
                    let bare_self = match frozen.ty_kind(concrete_self) {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                        _ => concrete_self,
                    };
                    match frozen.resolve_associated_type(
                        concrete_self,
                        proj.trait_ref.def_id,
                        proj.item_name,
                    ) {
                        Some(resolved) => resolved,
                        None => {
                            // The registered `fn_sig` (e.g. an async `poll`'s
                            // `Poll<F::Output>` return type) may carry a
                            // cross-arena-stale `trait_ref.def_id`, so the
                            // trait-keyed lookup above misses even though a valid
                            // impl exists. Retry by self-type + assoc-name alone,
                            // which ignores the trait id (correct for the common
                            // single-impl `Future::Output` case).
                            match frozen
                                .resolve_associated_type_by_self_ty(bare_self, proj.item_name)
                            {
                                Some(resolved) => resolved,
                                None => {
                                    let mut new_subst_args: Vec<GenericArg> = Vec::new();
                                    for a in proj_subst_args.iter() {
                                        match a {
                                            GenericArg::Ty(t) => new_subst_args
                                                .push(GenericArg::Ty(subst(*t, map, ctx, frozen))),
                                            other => new_subst_args.push(other.clone()),
                                        }
                                    }
                                    let new_sub = ctx.intern_substitution(new_subst_args);
                                    ctx.mk_ty(TyKind::Projection(ProjectionTy {
                                        trait_ref: TraitRef {
                                            def_id: proj.trait_ref.def_id,
                                            substs: new_sub,
                                        },
                                        item_name: proj.item_name,
                                    }))
                                }
                            }
                        }
                    }
                }
                _ => ty,
            }
        }
        let mut input_args: Vec<GenericArg> = Vec::new();
        for a in ty_ctx.substitution_args(sig.inputs).iter() {
            match a {
                GenericArg::Ty(t) => {
                    input_args.push(GenericArg::Ty(subst(*t, param_map, &mut ctx, ty_ctx)))
                }
                other => input_args.push(other.clone()),
            }
        }
        let inputs = ctx.intern_substitution(input_args);
        let output = subst(sig.output, param_map, &mut ctx, ty_ctx);
        // The registered `fn_sig` for an async `poll` may carry a
        // cross-arena-stale `Poll<F::Output>` whose inner substitution reads as
        // empty in the shared arena (the `F::Output` projection was lost during
        // stdlib pre-compilation). Recover it from the call's concrete `F`
        // (`param_map[0]`): resolve `F::Output` via the `Future` lang-item trait
        // and rebuild `Poll<Output>`. This is exact for the single-impl
        // `Future::Output` case and only triggers when the substitution is empty.
        let output = match ty_ctx.ty_kind(output) {
            TyKind::Adt(poll_adt, poll_sub)
                if *poll_adt == glyim_core::def_id::AdtId::from_raw(1) =>
            {
                if let Some(GenericArg::Ty(self_ty)) = param_map.first() {
                    // `Self` is `&mut Future` here; the impl table is keyed by the bare ADT.
                    let bare_self: Ty = match ty_ctx.ty_kind(*self_ty) {
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => *inner,
                        _ => *self_ty,
                    };
                    // The registered `poll` `fn_sig` may carry a cross-arena
                    // stale `Future`/`Output` handle. Resolve `Future::Output`
                    // by self-ADT + the *name string* "Output", which is immune
                    // to `Name`-handle corruption across arenas.
                    let r = ty_ctx.resolve_associated_type_by_self_ty_name(bare_self, "Output");
                    if let Some(resolved) = r {
                        // `resolved` is a `Ty` handle from the impl table,
                        // which may live in a *different* (leaked) arena than
                        // the codegen `ty_ctx` — re-interning its *kind* here
                        // yields a fresh, valid handle in the codegen arena
                        // (otherwise the dedup lands back on the stale
                        // `Poll<Subst(4)>` and `fn_abi_of` still fails).
                        let resolved_fresh = ctx.mk_ty(ty_ctx.ty_kind(resolved).clone());
                        let new_poll_sub =
                            ctx.intern_substitution(vec![GenericArg::Ty(resolved_fresh)]);
                        ctx.mk_ty(TyKind::Adt(*poll_adt, new_poll_sub))
                    } else {
                        output
                    }
                } else {
                    output
                }
            }
            _ => output,
        };
        FnSig {
            inputs,
            output,
            c_variadic: sig.c_variadic,
            unsafety: sig.unsafety,
            abi: sig.abi,
        }
    }

    /// Emit a builtin-intrinsic method inline. Returns `Ok(true)` if `fn_id`
    /// names a builtin this dispatcher knows how to lower; `Ok(false)`
    /// otherwise (the caller falls through to the ordinary call lowering).
    ///
    /// `str`, `String`'s inner `Vec<u8>`, and `[u8]` all lower to a fat
    /// pointer `{ ptr, i64 }`. The `as_bytes` / `as_ptr` / `as_mut_ptr`
    /// family is therefore a layout-level pass-through or a data-pointer
    /// field extraction:
    /// - `str::as_bytes(&self) -> &[u8]` — fat pointer in, fat pointer out.
    /// - `str::as_ptr(&self) -> *const u8` — first field of the fat pointer.
    /// - `String::as_bytes(&self) -> &[u8]` — `String`'s value is a
    ///   `{ptr, i64}`-shaped struct; its first field is the data pointer.
    ///
    /// Anything else returns `Ok(false)` so future builtins can be added
    /// incrementally.
    /// Declare (or fetch) an external helper with a void return and the
    /// given parameter types. Used by String-builtin lowering.
    fn declare_void_helper(
        &self,
        name: &str,
        param_tys: &[inkwell::types::BasicMetadataTypeEnum<'ctx>],
    ) -> inkwell::values::FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function(name) {
            return f;
        }
        let fn_ty = self.context.void_type().fn_type(param_tys, false);
        self.module.add_function(name, fn_ty, None)
    }

    /// Resolve the receiver of a builtin method to a pointer to its
    /// in-memory header. A receiver that arrives as a pointer is used
    /// directly; one that arrives as a struct value is resolved to the
    /// caller's place pointer (so mutations are visible), falling back to a
    /// fresh alloca only when there is no place. Returns `Ok(None)` for any
    /// other shape, letting the caller decline the lowering.
    fn receiver_header_ptr(
        &self,
        recv: &Operand,
        recv_val: BasicValueEnum<'ctx>,
        tag: &str,
    ) -> CompResult<Option<PointerValue<'ctx>>> {
        if recv_val.is_pointer_value() {
            return Ok(Some(recv_val.into_pointer_value()));
        }
        if recv_val.is_struct_value() {
            if let Operand::Copy(p) | Operand::Move(p) = recv {
                return Ok(Some(self.place_ptr(p)?));
            }
            let alloca = self
                .builder
                .build_alloca(recv_val.get_type(), &format!("{tag}_hdr"))
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "{tag} hdr alloca: {e:?}"
                    ))]
                })?;
            self.builder.build_store(alloca, recv_val).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "{tag} hdr store: {e:?}"
                ))]
            })?;
            return Ok(Some(alloca));
        }
        Ok(None)
    }

    fn try_lower_builtin_intrinsic(
        &mut self,
        fn_id: glyim_core::def_id::FnDefId,
        args: &[Operand],
        destination: &Place,
        target: &Option<BasicBlockIdx>,
    ) -> CompResult<bool> {
        let Some((_recv_adt, method_name)) = self.ty_ctx.builtin_fn_id(fn_id) else {
            return Ok(false);
        };
        let method = self.ty_ctx.name_str(method_name).to_string();
        if !matches!(
            method.as_str(),
            "as_bytes"
                | "as_ptr"
                | "as_mut_ptr"
                | "len"
                | "is_empty"
                | "to_string"
                | "as_str"
                | "new"
                | "push_str"
                | "push"
                | "clear"
                | "into_bytes"
                | "from"
                | "remove"
                | "to_vec"
                | "as_slice"
                | "as_ref"
                | "from_utf8_lossy"
                | "capacity"
                | "set_len"
                | "truncate"
                | "copy_from_slice"
                | "split_at"
                | "pop"
                | "insert"
                | "reserve"
                | "is_some"
                | "is_none"
                | "is_ok"
                | "is_err"
                | "unwrap"
                | "expect"
                | "unwrap_err"
                | "expect_err"
                | "unwrap_or"
                | "unwrap_or_default"
        ) {
            return Ok(false);
        }
        // BC-NEW-ZEROARG: `String::new()` / `Vec::new()` take no arguments,
        // so the `args.first()` guard below would bail before any per-method
        // arm. Handle the zero-arg constructors first. `dest` is the
        // `{ptr, len}` fat pointer; an empty collection is all-zero.
        if method == "new" {
            let dest_ty0 = self
                .body
                .locals
                .get(destination.local)
                .map(|d| d.ty)
                .unwrap_or(glyim_type::Ty::UNIT);
            let dest_llvm_ty0 = self.llvm_type_for_ty(dest_ty0)?;
            let dp = self.place_ptr(destination)?;
            self.builder
                .build_store(dp, dest_llvm_ty0.const_zero())
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("store new: {e:?}"))])?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("new target");
                self.builder
                    .build_unconditional_branch(tb)
                    .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("branch: {e:?}"))])?;
            }
            return Ok(true);
        }
        let Some(recv) = args.first() else {
            return Ok(false);
        };
        let recv_val = self.lower_operand(recv)?;
        let dest_ptr = self.place_ptr(destination)?;
        let dest_ty = self
            .body
            .locals
            .get(destination.local)
            .map(|d| d.ty)
            .unwrap_or(glyim_type::Ty::UNIT);
        let dest_llvm_ty = self.llvm_type_for_ty(dest_ty)?;

        // BC-STR-ARMS: `as_str` is identity (`String` and `&str` are both
        // `{ptr, i64}`); `String::new` is all-zero; `push_str` mutates the
        // receiver's `{ptr, len}` header in place via the runtime helper.
        if method == "as_str" {
            let stored = if recv_val.get_type() == dest_llvm_ty
                || (recv_val.is_struct_value() && dest_llvm_ty.is_struct_type())
            {
                recv_val
            } else {
                return Ok(false);
            };
            self.builder.build_store(dest_ptr, stored).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "store as_str: {e:?}"
                ))]
            })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("as_str target");
                self.builder
                    .build_unconditional_branch(tb)
                    .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("branch: {e:?}"))])?;
            }
            return Ok(true);
        }
        if method == "push_str" {
            // Receiver is `&mut String`; its LLVM value is the `{ptr, i64}`
            // struct OR a pointer to it. The helper needs a pointer to the
            // header.
            let header_ptr = if recv_val.is_pointer_value() {
                recv_val.into_pointer_value()
            } else if recv_val.is_struct_value() {
                // Struct-value receiver: the helper mutates the header
                // through the pointer we pass, so hand it the caller's place.
                let place = match recv {
                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                    _ => None,
                };
                if let Some(p) = place {
                    self.place_ptr(p)?
                } else {
                    let alloca = self
                        .builder
                        .build_alloca(recv_val.get_type(), "ps_hdr")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!("alloca: {e:?}"))]
                        })?;
                    self.builder.build_store(alloca, recv_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!("store: {e:?}"))]
                    })?;
                    alloca
                }
            } else {
                return Ok(false);
            };
            let Some(add) = args.get(1) else {
                return Ok(false);
            };
            let add_val = self.lower_operand(add)?;
            if !add_val.is_struct_value() {
                return Ok(false);
            }
            let av = add_val.into_struct_value();
            let ap = self
                .builder
                .build_extract_value(av, 0, "ps_p")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("extract: {e:?}"))])?;
            let al = self
                .builder
                .build_extract_value(av, 1, "ps_l")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("extract: {e:?}"))])?;
            let helper = self.declare_void_helper(
                "glyim_string_push_str",
                &[
                    self.context
                        .ptr_type(inkwell::AddressSpace::default())
                        .into(),
                    self.context
                        .ptr_type(inkwell::AddressSpace::default())
                        .into(),
                    self.llvm_int_type(64).into(),
                ],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> =
                vec![header_ptr.into(), ap.into(), al.into()];
            self.builder
                .build_call(helper, &cargs, "push_str_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "push_str call: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("push_str target");
                self.builder
                    .build_unconditional_branch(tb)
                    .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("branch: {e:?}"))])?;
            }
            return Ok(true);
        }

        // BC-VEC-PUSH: `Vec::<T>::push(&mut self, value: T)`. Inline lowering
        // via the `glyim_vec_push` runtime helper. Vec's LLVM layout is
        // `{buf: {ptr, cap}, len}` == `{ptr, cap, len}` at bytes 0/8/16 —
        // the same shape as `String`, so the same header-pointer path used
        // by `push_str` applies. Gated on Vec's AdtId (1020) so it does not
        // shadow `String::push(u8)` / `str::push(u8)`.
        if method == "push" && _recv_adt.to_raw() == 1020 {
            let header_ptr = if recv_val.is_pointer_value() {
                recv_val.into_pointer_value()
            } else if recv_val.is_struct_value() {
                let place = match recv {
                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                    _ => None,
                };
                if let Some(p) = place {
                    self.place_ptr(p)?
                } else {
                    let alloca = self
                        .builder
                        .build_alloca(recv_val.get_type(), "vp_hdr")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "vec push hdr alloca: {e:?}"
                            ))]
                        })?;
                    self.builder.build_store(alloca, recv_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "vec push hdr store: {e:?}"
                        ))]
                    })?;
                    alloca
                }
            } else {
                return Ok(false);
            };
            let Some(elem_op) = args.get(1) else {
                return Ok(false);
            };
            let elem_val = self.lower_operand(elem_op)?;
            let elem_ty = elem_val.get_type();
            let elem_alloca = self.builder.build_alloca(elem_ty, "vp_elem").map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "vec push elem alloca: {e:?}"
                ))]
            })?;
            self.builder
                .build_store(elem_alloca, elem_val)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec push elem store: {e:?}"
                    ))]
                })?;
            // Size of the element type via the layout computer, not
            // `LLVMType::size_of()` — the latter returns a `ConstantExpr`
            // (not a `ConstantInt`) without an attached target data layout
            // on the inkwell context, and `.get_zero_extended_constant()`
            // then yields `None`. The Ty-based layout is authoritative
            // anyway.
            let elem_ty_rust = self.operand_ty(elem_op);
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size_u64 = layout_computer
                .layout_of(elem_ty_rust)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec push: layout_of element failed: {:?}",
                        e
                    ))]
                })?;
            let helper = self.declare_void_helper(
                "glyim_vec_push",
                &[
                    self.context
                        .ptr_type(inkwell::AddressSpace::default())
                        .into(),
                    self.context
                        .ptr_type(inkwell::AddressSpace::default())
                        .into(),
                    self.llvm_int_type(64).into(),
                ],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> = vec![
                header_ptr.into(),
                elem_alloca.into(),
                self.llvm_int_type(64)
                    .const_int(elem_size_u64, false)
                    .into(),
            ];
            self.builder
                .build_call(helper, &cargs, "vec_push_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec push call: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("vec push target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec push branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-CLEAR: `Vec::<T>::clear` / `String::clear` / `str::clear`.
        // All three share the same 24-byte `{ptr, cap, len}` header, so
        // clearing is a single store of zero at byte offset 16 (the length
        // word). Capacity and buffer pointer are retained, matching the
        // "keeps capacity" contract.
        // BC-AS-SLICE: `Vec::<T>::as_slice(&self) -> &[T]` (and
        // `Vec::as_ref`, same shape). Vec's 24-byte `{ptr, cap, len}`
        // header → the `{ptr, len}` fat pointer: read word 0 (ptr) and
        // word 2 (len). Gated on Vec's AdtId (1020).
        if matches!(method.as_str(), "as_slice" | "as_ref")
            && _recv_adt.to_raw() == 1020
            && dest_llvm_ty.is_struct_type()
            && dest_llvm_ty.into_struct_type().count_fields() >= 2
        {
            let header_ptr = if recv_val.is_pointer_value() {
                recv_val.into_pointer_value()
            } else if recv_val.is_struct_value() {
                let place = match recv {
                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                    _ => None,
                };
                if let Some(p) = place {
                    self.place_ptr(p)?
                } else {
                    let alloca = self
                        .builder
                        .build_alloca(recv_val.get_type(), "as_hdr")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "as_slice hdr alloca: {e:?}"
                            ))]
                        })?;
                    self.builder.build_store(alloca, recv_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_slice hdr store: {e:?}"
                        ))]
                    })?;
                    alloca
                }
            } else {
                return Ok(false);
            };
            let i8_ptr = self.context.ptr_type(AddressSpace::default());
            let base = self
                .builder
                .build_bit_cast(header_ptr, i8_ptr, "as_base")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "as_slice bitcast: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let zero = self.llvm_int_type(64).const_zero();
            let six = self.llvm_int_type(64).const_int(16, false);
            let data_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[zero], "as_dp")
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "as_slice dp: {e:?}"
                ))]
            })?;
            let len_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[six], "as_lp")
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "as_slice lp: {e:?}"
                ))]
            })?;
            let p_loaded = self
                .builder
                .build_load(i8_ptr, data_ptr, "as_p")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "as_slice pload: {e:?}"
                    ))]
                })?;
            let l_loaded = self
                .builder
                .build_load(self.llvm_int_type(64), len_ptr, "as_l")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "as_slice lload: {e:?}"
                    ))]
                })?;
            let dst = dest_llvm_ty.into_struct_type();
            let mut agg = dst.get_undef();
            agg = self
                .builder
                .build_insert_value(agg, p_loaded, 0, "as_i0")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "as_slice i0: {e:?}"
                    ))]
                })?
                .into_struct_value();
            agg = self
                .builder
                .build_insert_value(agg, l_loaded, 1, "as_i1")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "as_slice i1: {e:?}"
                    ))]
                })?
                .into_struct_value();
            self.builder.build_store(dest_ptr, agg).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "as_slice store: {e:?}"
                ))]
            })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("as_slice target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "as_slice branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-CAPACITY / BC-SET-LEN: `capacity()` reads word 1 (byte 8) of
        // the 24-byte `{ptr, cap, len}` header; `set_len(n)` writes word 2
        // (byte 16). Both are shared by `Vec`, `String` and `str`, so they
        // are gated on all three AdtIds.
        if method == "capacity" && matches!(_recv_adt.to_raw(), 1020 | 1050 | 1061) {
            let header_ptr = match self.receiver_header_ptr(recv, recv_val, "cap")? {
                Some(p) => p,
                None => return Ok(false),
            };
            let base = self
                .builder
                .build_bit_cast(
                    header_ptr,
                    self.context.ptr_type(AddressSpace::default()),
                    "cap_base",
                )
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "capacity bitcast: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let eight = self.llvm_int_type(64).const_int(8, false);
            let cap_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[eight], "cap_ptr")
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "capacity gep: {e:?}"
                ))]
            })?;
            let cap = self
                .builder
                .build_load(self.llvm_int_type(64), cap_ptr, "cap_load")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "capacity load: {e:?}"
                    ))]
                })?;
            self.builder.build_store(dest_ptr, cap).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "capacity store: {e:?}"
                ))]
            })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("capacity target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "capacity branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        if method == "set_len" && matches!(_recv_adt.to_raw(), 1020 | 1050 | 1061) {
            let header_ptr = match self.receiver_header_ptr(recv, recv_val, "sl")? {
                Some(p) => p,
                None => return Ok(false),
            };
            let Some(n_op) = args.get(1) else {
                return Ok(false);
            };
            let n_val = self.lower_operand(n_op)?.into_int_value();
            let i64_ty = self.llvm_int_type(64);
            let n64 = if n_val.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(n_val, i64_ty, "sl_zext")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "set_len zext: {e:?}"
                        ))]
                    })?
            } else {
                n_val
            };
            let base = self
                .builder
                .build_bit_cast(
                    header_ptr,
                    self.context.ptr_type(AddressSpace::default()),
                    "sl_base",
                )
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "set_len bitcast: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let off = i64_ty.const_int(16, false);
            let len_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[off], "sl_ptr")
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "set_len gep: {e:?}"
                ))]
            })?;
            self.builder.build_store(len_ptr, n64).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "set_len store: {e:?}"
                ))]
            })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("set_len target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "set_len branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-TRUNCATE: `Vec::<T>::truncate(&mut self, n)` clamps the length
        // word to `n` (a no-op when `len <= n`). Capacity and buffer are
        // retained, matching `Vec::truncate`. Gated on Vec (1020) — the
        // method is not registered for `String`/`str`.
        if method == "truncate" && _recv_adt.to_raw() == 1020 {
            let header_ptr = match self.receiver_header_ptr(recv, recv_val, "tr")? {
                Some(p) => p,
                None => return Ok(false),
            };
            let Some(n_op) = args.get(1) else {
                return Ok(false);
            };
            let n_val = self.lower_operand(n_op)?.into_int_value();
            let i64_ty = self.llvm_int_type(64);
            let n64 = if n_val.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(n_val, i64_ty, "tr_zext")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "truncate zext: {e:?}"
                        ))]
                    })?
            } else {
                n_val
            };
            let base = self
                .builder
                .build_bit_cast(
                    header_ptr,
                    self.context.ptr_type(AddressSpace::default()),
                    "tr_base",
                )
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "truncate bitcast: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let off = i64_ty.const_int(16, false);
            let len_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[off], "tr_ptr")
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "truncate gep: {e:?}"
                ))]
            })?;
            let len = self
                .builder
                .build_load(i64_ty, len_ptr, "tr_len")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "truncate load: {e:?}"
                    ))]
                })?
                .into_int_value();
            let gt = self
                .builder
                .build_int_compare(inkwell::IntPredicate::UGT, len, n64, "tr_gt")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "truncate cmp: {e:?}"
                    ))]
                })?;
            let clamped = self
                .builder
                .build_select(gt, n64, len, "tr_clamp")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "truncate select: {e:?}"
                    ))]
                })?;
            self.builder.build_store(len_ptr, clamped).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "truncate store: {e:?}"
                ))]
            })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("truncate target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "truncate branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-VEC-POP: `Vec::<T>::pop(&mut self) -> Option<T>`. Reads the
        // length word (before mutating), calls `glyim_vec_pop` to write the
        // last element to a temp and decrement the length, then selects
        // between `None` (variant 0) and `Some(payload)` (variant 1) built
        // via `build_layout_aggregate` (Option's LLVM type is an
        // opaque-sized struct, not `{tag, payload}`).
        if method == "pop" && _recv_adt.to_raw() == 1020 {
            let header_ptr = match self.receiver_header_ptr(recv, recv_val, "pop")? {
                Some(p) => p,
                None => return Ok(false),
            };
            let recv_ty_rust = match recv {
                Operand::Copy(p) | Operand::Move(p) => self.place_ty(p),
                _ => return Ok(false),
            };
            // Element type: prefer the receiver's `Vec<T>` argument, but
            // fall back to the *destination*'s `Option<T>` payload — the
            // receiver's substitution can be an un-substituted `ERROR`
            // placeholder at this point, while the destination type is
            // always concrete (typeck resolved it). Both must agree.
            let elem_ty =
                {
                    let from_recv = {
                        let inner = match self.ty_ctx.ty_kind(recv_ty_rust) {
                            TyKind::Ref(_, i, _) | TyKind::RawPtr(i, _) => *i,
                            _ => recv_ty_rust,
                        };
                        match self.ty_ctx.ty_kind(inner) {
                            TyKind::Adt(_, s) => self
                                .ty_ctx
                                .substitution_args(*s)
                                .first()
                                .and_then(|a| match a {
                                    GenericArg::Ty(t) => Some(*t),
                                    _ => None,
                                }),
                            _ => None,
                        }
                    };
                    let concrete = |t: Option<Ty>| match t {
                        Some(t)
                            if !matches!(
                                self.ty_ctx.ty_kind(t),
                                TyKind::Error | TyKind::Infer(_) | TyKind::Param(_)
                            ) =>
                        {
                            Some(t)
                        }
                        _ => None,
                    };
                    let from_dest = {
                        let dty = self.place_ty(destination);
                        match self.ty_ctx.ty_kind(dty) {
                            TyKind::Adt(_, s) => self
                                .ty_ctx
                                .substitution_args(*s)
                                .first()
                                .and_then(|a| match a {
                                    GenericArg::Ty(t) => Some(*t),
                                    _ => None,
                                }),
                            _ => None,
                        }
                    };
                    concrete(from_recv).or_else(|| concrete(from_dest))
                };
            let Some(elem_ty) = elem_ty else {
                return Ok(false);
            };
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec pop layout: {e:?}"
                    ))]
                })?;
            let elem_llvm_ty = self.llvm_type_for_ty(elem_ty)?;
            let i64_ty = self.llvm_int_type(64);
            let i8_ptr = self.context.ptr_type(AddressSpace::default());
            let base = self
                .builder
                .build_bit_cast(header_ptr, i8_ptr, "pop_base")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "pop bitcast: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let off16 = i64_ty.const_int(16, false);
            let len_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[off16], "pop_lp")
            }
            .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("pop gep: {e:?}"))])?;
            let len = self
                .builder
                .build_load(i64_ty, len_ptr, "pop_len")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("pop lload: {e:?}"))])?
                .into_int_value();
            let is_empty = self
                .builder
                .build_int_compare(
                    inkwell::IntPredicate::EQ,
                    len,
                    i64_ty.const_zero(),
                    "pop_empty",
                )
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("pop cmp: {e:?}"))])?;
            let temp = self
                .builder
                .build_alloca(elem_llvm_ty, "pop_tmp")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "pop tmp alloca: {e:?}"
                    ))]
                })?;
            let helper = self.declare_void_helper(
                "glyim_vec_pop",
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into()],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> = vec![
                header_ptr.into(),
                i64_ty.const_int(elem_size, false).into(),
                temp.into(),
            ];
            self.builder
                .build_call(helper, &cargs, "pop_call")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("pop call: {e:?}"))])?;
            let payload = self
                .builder
                .build_load(elem_llvm_ty, temp, "pop_payload")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("pop pload: {e:?}"))])?;
            let dest_ty = self.place_ty(destination);
            let none_val =
                self.build_layout_aggregate(dest_ty, Some(VariantIdx::from_raw(0)), &[])?;
            let some_val =
                self.build_layout_aggregate(dest_ty, Some(VariantIdx::from_raw(1)), &[payload])?;
            let result = self
                .builder
                .build_select(is_empty, none_val, some_val, "pop_sel")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "pop select: {e:?}"
                    ))]
                })?;
            self.builder
                .build_store(dest_ptr, result)
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("pop store: {e:?}"))])?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("pop target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "pop branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-VEC-INSERT: `Vec::<T>::insert(&mut self, index, value)`.
        // Delegates to `glyim_vec_insert` (grow + memmove tail + store).
        if method == "insert" && _recv_adt.to_raw() == 1020 {
            let header_ptr = match self.receiver_header_ptr(recv, recv_val, "ins")? {
                Some(p) => p,
                None => return Ok(false),
            };
            let Some(idx_op) = args.get(1) else {
                return Ok(false);
            };
            let idx_val = self.lower_operand(idx_op)?.into_int_value();
            let i64_ty = self.llvm_int_type(64);
            let idx64 = if idx_val.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(idx_val, i64_ty, "ins_zext")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "insert zext: {e:?}"
                        ))]
                    })?
            } else {
                idx_val
            };
            let Some(elem_op) = args.get(2) else {
                return Ok(false);
            };
            let elem_val = self.lower_operand(elem_op)?;
            let elem_llvm_ty = elem_val.get_type();
            let temp = self
                .builder
                .build_alloca(elem_llvm_ty, "ins_tmp")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "insert tmp alloca: {e:?}"
                    ))]
                })?;
            self.builder.build_store(temp, elem_val).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "insert tmp store: {e:?}"
                ))]
            })?;
            let elem_ty_rust = self.operand_ty(elem_op);
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty_rust)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "insert layout: {e:?}"
                    ))]
                })?;
            let i8_ptr = self.context.ptr_type(AddressSpace::default());
            let helper = self.declare_void_helper(
                "glyim_vec_insert",
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into()],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> = vec![
                header_ptr.into(),
                idx64.into(),
                temp.into(),
                i64_ty.const_int(elem_size, false).into(),
            ];
            self.builder
                .build_call(helper, &cargs, "insert_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "insert call: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("insert target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "insert branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-VEC-RESERVE: `Vec::<T>::reserve(&mut self, additional)`.
        // Grows capacity to hold at least `len + additional`; a no-op when
        // capacity already suffices.
        if method == "reserve" && _recv_adt.to_raw() == 1020 {
            let header_ptr = match self.receiver_header_ptr(recv, recv_val, "res")? {
                Some(p) => p,
                None => return Ok(false),
            };
            let Some(n_op) = args.get(1) else {
                return Ok(false);
            };
            let n_val = self.lower_operand(n_op)?.into_int_value();
            let i64_ty = self.llvm_int_type(64);
            let n64 = if n_val.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(n_val, i64_ty, "res_zext")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "reserve zext: {e:?}"
                        ))]
                    })?
            } else {
                n_val
            };
            let recv_ty_rust = match recv {
                Operand::Copy(p) | Operand::Move(p) => self.place_ty(p),
                _ => return Ok(false),
            };
            let elem_ty = {
                let inner = match self.ty_ctx.ty_kind(recv_ty_rust) {
                    TyKind::Ref(_, i, _) | TyKind::RawPtr(i, _) => *i,
                    _ => recv_ty_rust,
                };
                match self.ty_ctx.ty_kind(inner) {
                    TyKind::Adt(_, s) => {
                        self.ty_ctx
                            .substitution_args(*s)
                            .first()
                            .and_then(|a| match a {
                                GenericArg::Ty(t) => Some(*t),
                                _ => None,
                            })
                    }
                    _ => None,
                }
            };
            let Some(elem_ty) = elem_ty else {
                return Ok(false);
            };
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "reserve layout: {e:?}"
                    ))]
                })?;
            let i8_ptr = self.context.ptr_type(AddressSpace::default());
            let helper = self.declare_void_helper(
                "glyim_vec_reserve",
                &[i8_ptr.into(), i64_ty.into(), i64_ty.into()],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> = vec![
                header_ptr.into(),
                n64.into(),
                i64_ty.const_int(elem_size, false).into(),
            ];
            self.builder
                .build_call(helper, &cargs, "reserve_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "reserve call: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("reserve target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "reserve branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-SLICE-COPY-FROM-SLICE: `[T]::copy_from_slice(&mut self, src: &[T])`.
        // Both sides are `{ptr, i64}` fat pointers; memcpy
        // `min(dst_len, src_len) * elem_size` bytes. Gated on slice (1060).
        if method == "copy_from_slice" && _recv_adt.to_raw() == 1060 {
            if !recv_val.is_struct_value() {
                return Ok(false);
            }
            let dst_sv = recv_val.into_struct_value();
            if dst_sv.get_type().count_fields() < 2 {
                return Ok(false);
            }
            let Some(src_op) = args.get(1) else {
                return Ok(false);
            };
            let src_val = self.lower_operand(src_op)?;
            if !src_val.is_struct_value() {
                return Ok(false);
            }
            let src_sv = src_val.into_struct_value();
            if src_sv.get_type().count_fields() < 2 {
                return Ok(false);
            }
            let dst_p = self
                .builder
                .build_extract_value(dst_sv, 0, "cfs_dp")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs dp: {e:?}"))])?
                .into_pointer_value();
            let dst_l = self
                .builder
                .build_extract_value(dst_sv, 1, "cfs_dl")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs dl: {e:?}"))])?
                .into_int_value();
            let src_p = self
                .builder
                .build_extract_value(src_sv, 0, "cfs_sp")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs sp: {e:?}"))])?
                .into_pointer_value();
            let src_l = self
                .builder
                .build_extract_value(src_sv, 1, "cfs_sl")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs sl: {e:?}"))])?
                .into_int_value();
            let recv_ty_rust = match recv {
                Operand::Copy(p) | Operand::Move(p) => self.place_ty(p),
                _ => return Ok(false),
            };
            let elem_ty = match self.ty_ctx.ty_kind(recv_ty_rust) {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
                    match self.ty_ctx.ty_kind(*inner) {
                        TyKind::Slice(e) => *e,
                        _ => return Ok(false),
                    }
                }
                TyKind::Slice(e) => *e,
                _ => return Ok(false),
            };
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "copy_from_slice layout: {e:?}"
                    ))]
                })?;
            let i64_ty = self.llvm_int_type(64);
            let lt = self
                .builder
                .build_int_compare(inkwell::IntPredicate::ULT, dst_l, src_l, "cfs_lt")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs cmp: {e:?}"))])?;
            let n = self
                .builder
                .build_select(lt, dst_l, src_l, "cfs_n")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs sel: {e:?}"))])?
                .into_int_value();
            let bytes = self
                .builder
                .build_int_mul(n, i64_ty.const_int(elem_size, false), "cfs_bytes")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("cfs mul: {e:?}"))])?;
            self.builder
                .build_memcpy(dst_p, 1, src_p, 1, bytes)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "copy_from_slice memcpy: {e:?}"
                    ))]
                })?;
            self.builder
                .build_store(dest_ptr, dest_llvm_ty.const_zero())
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "copy_from_slice store: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("copy_from_slice target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "copy_from_slice branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-SLICE-SPLIT-AT: `[T]::split_at(&self, mid) -> (&[T], &[T])`.
        // Writes four i64 words [ptr0, len0, ptr1, len1] at dest+0/8/16/24.
        // Gated on slice (1060).
        if method == "split_at" && _recv_adt.to_raw() == 1060 {
            if !recv_val.is_struct_value() {
                return Ok(false);
            }
            let sv = recv_val.into_struct_value();
            if sv.get_type().count_fields() < 2 {
                return Ok(false);
            }
            let Some(mid_op) = args.get(1) else {
                return Ok(false);
            };
            let mid_raw = self.lower_operand(mid_op)?.into_int_value();
            let i64_ty = self.llvm_int_type(64);
            let mid = if mid_raw.get_type().get_bit_width() < 64 {
                self.builder
                    .build_int_z_extend(mid_raw, i64_ty, "sa_zext")
                    .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa zext: {e:?}"))])?
            } else {
                mid_raw
            };
            let p = self
                .builder
                .build_extract_value(sv, 0, "sa_p")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa p: {e:?}"))])?
                .into_pointer_value();
            let len = self
                .builder
                .build_extract_value(sv, 1, "sa_l")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa l: {e:?}"))])?
                .into_int_value();
            let recv_ty_rust = match recv {
                Operand::Copy(pl) | Operand::Move(pl) => self.place_ty(pl),
                _ => return Ok(false),
            };
            let elem_ty = match self.ty_ctx.ty_kind(recv_ty_rust) {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
                    match self.ty_ctx.ty_kind(*inner) {
                        TyKind::Slice(e) => *e,
                        _ => return Ok(false),
                    }
                }
                TyKind::Slice(e) => *e,
                _ => return Ok(false),
            };
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "split_at layout: {e:?}"
                    ))]
                })?;
            let mid_bytes = self
                .builder
                .build_int_mul(mid, i64_ty.const_int(elem_size, false), "sa_mb")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa mul: {e:?}"))])?;
            let p1 = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), p, &[mid_bytes], "sa_p1")
            }
            .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa gep: {e:?}"))])?;
            let len1 = self
                .builder
                .build_int_sub(len, mid, "sa_l1")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa sub: {e:?}"))])?;
            let i8_ptr = self.context.ptr_type(AddressSpace::default());
            let base = self
                .builder
                .build_bit_cast(dest_ptr, i8_ptr, "sa_base")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa bc: {e:?}"))])?
                .into_pointer_value();
            let p0_as_i = self
                .builder
                .build_ptr_to_int(p, i64_ty, "sa_p0i")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa p2i: {e:?}"))])?;
            let p1_as_i = self
                .builder
                .build_ptr_to_int(p1, i64_ty, "sa_p1i")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa p2i1: {e:?}"))])?;
            let words: [(u64, inkwell::values::IntValue<'ctx>); 4] =
                [(0, p0_as_i), (8, mid), (16, p1_as_i), (24, len1)];
            for (off, val) in words {
                let slot = unsafe {
                    self.builder.build_in_bounds_gep(
                        self.context.i8_type(),
                        base,
                        &[i64_ty.const_int(off, false)],
                        "sa_slot",
                    )
                }
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("sa sgep: {e:?}"))])?;
                self.builder.build_store(slot, val).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!("sa sstore: {e:?}"))]
                })?;
            }
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("split_at target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "split_at branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        if method == "clear" && matches!(_recv_adt.to_raw(), 1020 | 1050 | 1061) {
            let header_ptr = if recv_val.is_pointer_value() {
                recv_val.into_pointer_value()
            } else if recv_val.is_struct_value() {
                let place = match recv {
                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                    _ => None,
                };
                if let Some(p) = place {
                    self.place_ptr(p)?
                } else {
                    let alloca = self
                        .builder
                        .build_alloca(recv_val.get_type(), "vc_hdr")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "vec clear hdr alloca: {e:?}"
                            ))]
                        })?;
                    self.builder.build_store(alloca, recv_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "vec clear hdr store: {e:?}"
                        ))]
                    })?;
                    alloca
                }
            } else {
                return Ok(false);
            };
            let i8_ptr = self.context.ptr_type(AddressSpace::default());
            let base = self
                .builder
                .build_bit_cast(header_ptr, i8_ptr, "vc_base")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec clear bitcast: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let off = self.llvm_int_type(64).const_int(16, false);
            let len_ptr = unsafe {
                self.builder
                    .build_in_bounds_gep(self.context.i8_type(), base, &[off], "vc_len_ptr")
            }
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "vec clear gep: {e:?}"
                ))]
            })?;
            self.builder
                .build_store(len_ptr, self.llvm_int_type(64).const_zero())
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec clear store: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("vec clear target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec clear branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-INTO-BYTES: `String::into_bytes(self) -> Vec<u8>`. Both are
        // the 24-byte `{ptr, cap, len}` header (see `BC-CLEAR`), so this is
        // a pure reinterpret: `memcpy` the header to the destination. The
        // receiver is consumed (`self` by value), so ownership transfers
        // cleanly — no double-free. Gated on String's AdtId (1050).
        if method == "into_bytes" && _recv_adt.to_raw() == 1050 {
            let src_ptr = if recv_val.is_pointer_value() {
                recv_val.into_pointer_value()
            } else if recv_val.is_struct_value() {
                let place = match recv {
                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                    _ => None,
                };
                if let Some(p) = place {
                    self.place_ptr(p)?
                } else {
                    let alloca = self
                        .builder
                        .build_alloca(recv_val.get_type(), "ib_src")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "into_bytes src alloca: {e:?}"
                            ))]
                        })?;
                    self.builder.build_store(alloca, recv_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "into_bytes src store: {e:?}"
                        ))]
                    })?;
                    alloca
                }
            } else {
                return Ok(false);
            };
            let size = self.llvm_int_type(64).const_int(24, false);
            self.builder
                .build_memcpy(dest_ptr, 8, src_ptr, 8, size)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "into_bytes memcpy: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("into_bytes target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "into_bytes branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-STRING-FROM: `String::from(&str)` and
        // `String::from_utf8_lossy(&[u8])`. Both take a `{ptr, i64}` fat
        // pointer and produce a `String` (a 24-byte `{ptr, cap, len}`
        // header), so both lower to `glyim_string_from_bytes` — allocate +
        // copy the source bytes. The "lossy" validity check is a no-op for
        // the byte-exact payloads this compiler carries. Gated on String's
        // AdtId (1050).
        if matches!(method.as_str(), "from" | "from_utf8_lossy") && _recv_adt.to_raw() == 1050 {
            if !recv_val.is_struct_value() {
                return Ok(false);
            }
            let sv = recv_val.into_struct_value();
            if sv.get_type().count_fields() < 2 {
                return Ok(false);
            }
            let p = self
                .builder
                .build_extract_value(sv, 0, "from_p")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("from ptr: {e:?}"))])?
                .into_pointer_value();
            let l = self
                .builder
                .build_extract_value(sv, 1, "from_l")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("from len: {e:?}"))])?;
            let helper = self.declare_void_helper(
                "glyim_string_from_bytes",
                &[
                    self.context.ptr_type(AddressSpace::default()).into(),
                    self.llvm_int_type(64).into(),
                    self.context.ptr_type(AddressSpace::default()).into(),
                ],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> =
                vec![p.into(), l.into(), dest_ptr.into()];
            self.builder
                .build_call(helper, &cargs, "from_call")
                .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("from call: {e:?}"))])?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("from target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "from branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-VEC-REMOVE: `Vec::<T>::remove(&mut self, index) -> T`. Gated
        // on Vec's AdtId (1020). The runtime helper shifts the tail down
        // and returns the removed element via an out-pointer.
        if method == "remove" && _recv_adt.to_raw() == 1020 {
            let header_ptr = if recv_val.is_pointer_value() {
                recv_val.into_pointer_value()
            } else if recv_val.is_struct_value() {
                let place = match recv {
                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                    _ => None,
                };
                if let Some(p) = place {
                    self.place_ptr(p)?
                } else {
                    let alloca = self
                        .builder
                        .build_alloca(recv_val.get_type(), "vr_hdr")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "vec remove hdr alloca: {e:?}"
                            ))]
                        })?;
                    self.builder.build_store(alloca, recv_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "vec remove hdr store: {e:?}"
                        ))]
                    })?;
                    alloca
                }
            } else {
                return Ok(false);
            };
            let Some(idx_op) = args.get(1) else {
                return Ok(false);
            };
            let idx_val = self.lower_operand(idx_op)?;
            let i64_ty = self.llvm_int_type(64);
            let idx_i64 = if idx_val.get_type() == i64_ty.into() {
                idx_val.into_int_value()
            } else if idx_val.is_int_value() {
                let iv = idx_val.into_int_value();
                let from_bits = iv.get_type().get_bit_width();
                if from_bits < 64 {
                    self.builder
                        .build_int_z_extend(iv, i64_ty, "vr_idx_zext")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "vec remove idx zext: {e:?}"
                            ))]
                        })?
                } else {
                    iv
                }
            } else {
                return Ok(false);
            };
            // Element Rust type / size.
            let recv_ty_rust = match recv {
                Operand::Copy(p) | Operand::Move(p) => self.place_ty(p),
                _ => return Ok(false),
            };
            let vec_elem_ty = match self.ty_ctx.ty_kind(recv_ty_rust) {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
                    match self.ty_ctx.ty_kind(*inner) {
                        TyKind::Adt(_, substs) => self
                            .ty_ctx
                            .substitution_args(*substs)
                            .first()
                            .and_then(|a| match a {
                                GenericArg::Ty(t) => Some(*t),
                                _ => None,
                            }),
                        _ => None,
                    }
                }
                TyKind::Adt(_, substs) => {
                    self.ty_ctx
                        .substitution_args(*substs)
                        .first()
                        .and_then(|a| match a {
                            GenericArg::Ty(t) => Some(*t),
                            _ => None,
                        })
                }
                _ => None,
            };
            let Some(elem_ty) = vec_elem_ty else {
                return Ok(false);
            };
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec remove layout: {e:?}"
                    ))]
                })?;
            // Out pointer: allocate a slot of dest type.
            let out_alloca = self
                .builder
                .build_alloca(dest_llvm_ty, "vr_out")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec remove out alloca: {e:?}"
                    ))]
                })?;
            let helper = self.declare_void_helper(
                "glyim_vec_remove",
                &[
                    self.context.ptr_type(AddressSpace::default()).into(),
                    i64_ty.into(),
                    i64_ty.into(),
                    self.context.ptr_type(AddressSpace::default()).into(),
                ],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> = vec![
                header_ptr.into(),
                idx_i64.into(),
                i64_ty.const_int(elem_size, false).into(),
                out_alloca.into(),
            ];
            self.builder
                .build_call(helper, &cargs, "vec_remove_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec remove call: {e:?}"
                    ))]
                })?;
            let out_val = self
                .builder
                .build_load(dest_llvm_ty, out_alloca, "vr_load")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec remove load: {e:?}"
                    ))]
                })?;
            self.builder.build_store(dest_ptr, out_val).map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "vec remove store: {e:?}"
                ))]
            })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("vec remove target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "vec remove branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-SLICE-TO-VEC: `[T]::to_vec(&self) -> Vec<T>`. Gated on slice's
        // AdtId (1060). The receiver is a `{ptr, i64}` fat pointer; the
        // helper allocates capacity == length and memcpys.
        if method == "to_vec" && _recv_adt.to_raw() == 1060 {
            if !recv_val.is_struct_value() {
                return Ok(false);
            }
            let sv = recv_val.into_struct_value();
            if sv.get_type().count_fields() < 2 {
                return Ok(false);
            }
            let p = self
                .builder
                .build_extract_value(sv, 0, "tv_p")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "to_vec ptr: {e:?}"
                    ))]
                })?
                .into_pointer_value();
            let l = self
                .builder
                .build_extract_value(sv, 1, "tv_l")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "to_vec len: {e:?}"
                    ))]
                })?;
            // Element size from the slice's Rust element type.
            let recv_ty_rust = match recv {
                Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
                _ => return Ok(false),
            };
            let elem_ty = match self.ty_ctx.ty_kind(recv_ty_rust) {
                TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
                    match self.ty_ctx.ty_kind(*inner) {
                        TyKind::Slice(e) => *e,
                        _ => return Ok(false),
                    }
                }
                TyKind::Slice(e) => *e,
                _ => return Ok(false),
            };
            let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
            let elem_size = layout_computer
                .layout_of(elem_ty)
                .map(|l| l.size.0)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "to_vec layout: {e:?}"
                    ))]
                })?;
            let i64_ty = self.llvm_int_type(64);
            let helper = self.declare_void_helper(
                "glyim_slice_to_vec",
                &[
                    self.context.ptr_type(AddressSpace::default()).into(),
                    i64_ty.into(),
                    i64_ty.into(),
                    self.context.ptr_type(AddressSpace::default()).into(),
                ],
            );
            let cargs: Vec<inkwell::values::BasicMetadataValueEnum> = vec![
                p.into(),
                l.into(),
                i64_ty.const_int(elem_size, false).into(),
                dest_ptr.into(),
            ];
            self.builder
                .build_call(helper, &cargs, "to_vec_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "to_vec call: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("to_vec target");
                self.builder.build_unconditional_branch(tb).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "to_vec branch: {e:?}"
                    ))]
                })?;
            }
            return Ok(true);
        }

        // BC-TOSTRING-LLVM: `str::to_string(&self) -> String`. In this
        // compiler's LLVM type model both `str` (a `&str`) and `String` lower
        // to the fat pointer `{ptr, i64}`, so `to_string` is a copy: read the
        // receiver's (ptr, len) and hand them to the runtime helper, which
        // allocates and copies. Without this arm the call emitted an undefined
        // `__glyim_fn_9106`.
        if method == "to_string" {
            if !recv_val.is_struct_value() {
                return Ok(false);
            }
            let sv = recv_val.into_struct_value();
            let sty = sv.get_type();
            // Produce `(ptr, len)` regardless of which shape the receiver has.
            let (p, l) = if sty.count_fields() >= 2 {
                // A `&str` fat pointer: `{ ptr, i64 }` — field 0 is already a
                // pointer, field 1 is the length.
                let p = self
                    .builder
                    .build_extract_value(sv, 0, "ts_p")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "to_string ptr: {e:?}"
                        ))]
                    })?
                    .into_pointer_value();
                let l = self
                    .builder
                    .build_extract_value(sv, 1, "ts_l")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "to_string len: {e:?}"
                        ))]
                    })?;
                (p, l)
            } else if sty.count_fields() == 1 {
                // `String` is `{ [3 x i64] }` — field 0 is the `[ptr, cap, len]`
                // array. Extract the array, then its elements 0 and 2. No
                // struct-to-struct bitcast (which hung LLVM's verifier).
                let arr = self
                    .builder
                    .build_extract_value(sv, 0, "ts_arr")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "to_string arr: {e:?}"
                        ))]
                    })?
                    .into_array_value();
                let p_i = self
                    .builder
                    .build_extract_value(arr, 0, "ts_pi")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "to_string pi: {e:?}"
                        ))]
                    })?
                    .into_int_value();
                let l = self
                    .builder
                    .build_extract_value(arr, 2, "ts_li")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "to_string li: {e:?}"
                        ))]
                    })?;
                let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                let p = self
                    .builder
                    .build_int_to_ptr(p_i, ptr_ty, "ts_inttoptr")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "to_string inttoptr: {e:?}"
                        ))]
                    })?;
                (p, l)
            } else {
                return Ok(false);
            };
            let helper = self.declare_void_helper(
                "glyim_string_from_bytes",
                &[
                    self.context
                        .ptr_type(inkwell::AddressSpace::default())
                        .into(),
                    self.llvm_int_type(64).into(),
                    self.context
                        .ptr_type(inkwell::AddressSpace::default())
                        .into(),
                ],
            );
            let args: Vec<inkwell::values::BasicMetadataValueEnum> =
                vec![p.into(), l.into(), dest_ptr.into()];
            self.builder
                .build_call(helper, &args, "to_string_call")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "to_string call: {e:?}"
                    ))]
                })?;
            if let Some(target_bb) = target {
                let tb = *self.bb_map.get(target_bb).expect("to_string target");
                self.builder
                    .build_unconditional_branch(tb)
                    .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("branch: {e:?}"))])?;
            }
            return Ok(true);
        }

        // `len` / `is_empty` on a fat pointer `{data_ptr, len}` extract field
        // 1 (the length). `is_empty` further compares it to 0.
        if matches!(method.as_str(), "len" | "is_empty") {
            // The receiver may be a fat pointer directly, or a thin pointer to
            // a local that holds one. Handle the struct value first; if the
            // operand loaded a thin pointer, we currently do not chase it
            // (only fat-pointer receivers reach this arm in practice).
            if !recv_val.is_struct_value() {
                return Ok(false);
            }
            // The receiver is either `{ ptr, i64 }` (a `&str`/`[T]` fat
            // pointer — length at field 1) or `{ [3 x i64] }` (a `String`,
            // its `{ptr, capacity, len}` inner flattened — length at word 2).
            // A fall-through for the `String` case emitted an undefined
            // `__glyim_fn_9069`; extract the field directly instead.
            let sv = recv_val.into_struct_value();
            let sty = sv.get_type();
            let len_val = if sty.count_fields() >= 2 {
                self.builder
                    .build_extract_value(sv, 1, "fat_len")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "fat-pointer len extract failed: {:?}",
                            e
                        ))]
                    })?
            } else {
                let arr = self
                    .builder
                    .build_extract_value(sv, 0, "str_len_arr")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "string len array extract failed: {:?}",
                            e
                        ))]
                    })?
                    .into_array_value();
                self.builder
                    .build_extract_value(arr, 2, "str_len_word")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "string len extract failed: {:?}",
                            e
                        ))]
                    })?
            };
            let stored = if method == "is_empty" {
                let zero = self.llvm_int_type(64).const_int(0, false);
                let is_empty = self
                    .builder
                    .build_int_compare(
                        inkwell::IntPredicate::EQ,
                        len_val.into_int_value(),
                        zero,
                        "is_empty",
                    )
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "is_empty compare failed: {:?}",
                            e
                        ))]
                    })?
                    .as_basic_value_enum();
                if is_empty.get_type() != dest_llvm_ty {
                    self.builder
                        .build_int_z_extend(
                            is_empty.into_int_value(),
                            dest_llvm_ty.into_int_type(),
                            "is_empty_ext",
                        )
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "is_empty extend failed: {:?}",
                                e
                            ))]
                        })?
                        .as_basic_value_enum()
                } else {
                    is_empty
                }
            } else {
                // `len` returns `usize` (64-bit here); truncate if the dest is
                // narrower for robustness.
                if len_val.get_type() != dest_llvm_ty {
                    self.builder
                        .build_int_truncate(
                            len_val.into_int_value(),
                            dest_llvm_ty.into_int_type(),
                            "len_trunc",
                        )
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "len truncate failed: {:?}",
                                e
                            ))]
                        })?
                        .as_basic_value_enum()
                } else {
                    len_val
                }
            };
            self.builder
                .build_store(dest_ptr, stored)
                .expect("store len/is_empty failed");
            if let Some(target_bb) = target {
                let target_block = *self
                    .bb_map
                    .get(target_bb)
                    .expect("builtin intrinsic target block not found");
                self.builder
                    .build_unconditional_branch(target_block)
                    .expect("branch after len/is_empty failed");
            }
            return Ok(true);
        }
        // Enum-payload builtins (`Option` / `Result`). The receiver is the
        // enum value; `_recv_adt.to_raw()` is 1010 for `Option`, 1011 for
        // `Result`. The enum LLVM type is an *opaque sized struct*
        // (`opaque_sized_type(size, align)`), NOT `{tag, payload}` — so we
        // must not use `build_extract_value` on the SSA value. Instead we
        // reuse the existing layout-aware machinery:
        //   * `lower_discriminant(recv_place)` reads the tag word at its
        //     real byte offset (see `abi.rs`'s enum layout);
        //   * a projected place `[Downcast(v), Field(0)]` reaches the
        //     selected variant's payload using the same offsets
        //     `build_layout_aggregate` writes through.
        //
        // Tag convention (mirrors `glyim-mir-interp`):
        //   Option: None = 0, Some = 1
        //   Result: Ok   = 0, Err  = 1
        if matches!(
            method.as_str(),
            "is_some"
                | "is_none"
                | "is_ok"
                | "is_err"
                | "unwrap"
                | "expect"
                | "unwrap_err"
                | "expect_err"
                | "unwrap_or"
                | "unwrap_or_default"
        ) {
            use inkwell::IntPredicate;
            let recv_place = match recv {
                Operand::Copy(p) | Operand::Move(p) => p,
                Operand::Constant(_) => return Ok(false),
            };
            let is_option = _recv_adt.to_raw() == 1010;
            // The tag value that means "the payload we want is present":
            //   Some = 1 for `Option`; Ok = 0 for `Result`.
            let present_tag: u64 = if is_option { 1 } else { 0 };
            let err_tag: u64 = if is_option { 0 } else { 1 };

            let tag_i64 = {
                let tag = self.lower_discriminant(recv_place)?.into_int_value();
                let i64_ty = self.llvm_int_type(64);
                if tag.get_type() == i64_ty {
                    tag
                } else {
                    self.builder
                        .build_int_z_extend(tag, i64_ty, "enum_tag64")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "enum tag zext: {e:?}"
                            ))]
                        })?
                }
            };

            // ------------------------------------------------------------------
            // Boolean tests: `is_some` / `is_none` / `is_ok` / `is_err`.
            // ------------------------------------------------------------------
            if matches!(method.as_str(), "is_some" | "is_none" | "is_ok" | "is_err") {
                let want: u64 = match method.as_str() {
                    // `is_some` / `is_ok` test the payload-present variant.
                    "is_some" | "is_ok" => present_tag,
                    // `is_none` / `is_err` test the other variant.
                    "is_none" | "is_err" => err_tag,
                    _ => unreachable!(),
                };
                let i64_ty = self.llvm_int_type(64);
                let want_v = i64_ty.const_int(want, false);
                let eq = self
                    .builder
                    .build_int_compare(IntPredicate::EQ, tag_i64, want_v, "is_eq")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!("is_* cmp: {e:?}"))]
                    })?;
                let stored: BasicValueEnum = if dest_llvm_ty.is_int_type() {
                    let dty = dest_llvm_ty.into_int_type();
                    if dty.get_bit_width() == 1 {
                        eq.into()
                    } else {
                        self.builder
                            .build_int_z_extend(eq, dty, "is_zext")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!("is_* zext: {e:?}"))]
                            })?
                            .into()
                    }
                } else {
                    return Ok(false);
                };
                self.builder.build_store(dest_ptr, stored).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "is_* store: {e:?}"
                    ))]
                })?;
                if let Some(t) = target {
                    let tb = *self.bb_map.get(t).expect("is_* target");
                    self.builder.build_unconditional_branch(tb).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!("is_* br: {e:?}"))]
                    })?;
                }
                return Ok(true);
            }

            // ------------------------------------------------------------------
            // Payload-extracting methods:
            //   unwrap / expect          -> present variant
            //   unwrap_err / expect_err  -> error variant (Result only)
            //   unwrap_or / _default     -> present variant, alt on miss
            // ------------------------------------------------------------------
            let (want_tag, take_err, store_default_on_miss) = match method.as_str() {
                "unwrap" | "expect" => (present_tag, false, false),
                "unwrap_err" | "expect_err" => {
                    if is_option {
                        // Not a real method on `Option`; bail to the normal
                        // path (which will emit a proper diagnostic).
                        return Ok(false);
                    }
                    (err_tag, true, false)
                }
                "unwrap_or" | "unwrap_or_default" => (present_tag, false, true),
                _ => unreachable!(),
            };

            let Some(target_idx) = target else {
                return Ok(false);
            };
            let target_bb = *self.bb_map.get(target_idx).expect("payload target");
            let Some(current_fn) = self.builder.get_insert_block().and_then(|b| b.get_parent())
            else {
                return Err(vec![GlyimDiagnostic::internal_error(
                    "payload: no current function",
                )]);
            };
            let ok_bb = self.context.append_basic_block(current_fn, "payload_ok");
            let bad_bb = self.context.append_basic_block(current_fn, "payload_bad");

            let i64_ty = self.llvm_int_type(64);
            let want_v = i64_ty.const_int(want_tag, false);
            let eq = self
                .builder
                .build_int_compare(IntPredicate::EQ, tag_i64, want_v, "payload_eq")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "payload cmp: {e:?}"
                    ))]
                })?;
            self.builder
                .build_conditional_branch(eq, ok_bb, bad_bb)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "payload br: {e:?}"
                    ))]
                })?;

            // --- Present branch: extract payload, store into destination.
            self.builder.position_at_end(ok_bb);
            let variant_for_payload: u32 = if take_err {
                err_tag as u32
            } else {
                present_tag as u32
            };
            let mut proj: Vec<ProjectionElem> = recv_place.projection.to_vec();
            proj.push(ProjectionElem::Downcast(VariantIdx::from_raw(
                variant_for_payload,
            )));
            proj.push(ProjectionElem::Field(FieldIdx::from_raw(0)));
            let payload_place = Place {
                local: recv_place.local,
                projection: proj.into_boxed_slice(),
            };
            let payload_ptr = self.place_ptr(&payload_place)?;
            let payload_val = self
                .builder
                .build_load(dest_llvm_ty, payload_ptr, "payload_load")
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "payload load: {e:?}"
                    ))]
                })?;
            self.builder
                .build_store(dest_ptr, payload_val)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "payload store: {e:?}"
                    ))]
                })?;
            self.builder
                .build_unconditional_branch(target_bb)
                .map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "payload br2: {e:?}"
                    ))]
                })?;

            // --- Miss branch: abort (unwrap/expect/_err) or store alternate.
            self.builder.position_at_end(bad_bb);
            if store_default_on_miss {
                let alt_val: BasicValueEnum = if method == "unwrap_or" {
                    match args.get(1) {
                        Some(op) => self.lower_operand(op)?,
                        None => return Ok(false),
                    }
                } else {
                    // `unwrap_or_default`: `Default::default()` of the
                    // destination type. `const_zero` is correct for the
                    // primitive and opaque-aggregate types this compiler
                    // emits.
                    dest_llvm_ty.const_zero()
                };
                // Coerce the alternate value's type to the destination type
                // when the mismatch is only integer width/signedness.
                let alt_val = if alt_val.get_type() == dest_llvm_ty {
                    alt_val
                } else if alt_val.is_int_value() && dest_llvm_ty.is_int_type() {
                    let from = alt_val.into_int_value();
                    let to = dest_llvm_ty.into_int_type();
                    let from_bits = from.get_type().get_bit_width();
                    let to_bits = to.get_bit_width();
                    if from_bits == to_bits {
                        from.as_basic_value_enum()
                    } else if from_bits < to_bits {
                        self.builder
                            .build_int_z_extend(from, to, "alt_zext")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!("alt zext: {e:?}"))]
                            })?
                            .into()
                    } else {
                        self.builder
                            .build_int_truncate(from, to, "alt_trunc")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!("alt trunc: {e:?}"))]
                            })?
                            .into()
                    }
                } else {
                    // Unsupported coercion shape; let the normal path
                    // diagnose.
                    return Ok(false);
                };
                self.builder.build_store(dest_ptr, alt_val).map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!("alt store: {e:?}"))]
                })?;
                self.builder
                    .build_unconditional_branch(target_bb)
                    .map_err(|e| vec![GlyimDiagnostic::internal_error(format!("alt br: {e:?}"))])?;
            } else {
                let abort_fn = self
                    .module
                    .get_function("glyim_process_abort")
                    .unwrap_or_else(|| {
                        let fn_ty = self.context.void_type().fn_type(&[], false);
                        self.module.add_function("glyim_process_abort", fn_ty, None)
                    });
                self.builder.build_call(abort_fn, &[], "").map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!(
                        "abort call: {e:?}"
                    ))]
                })?;
                self.builder.build_unreachable().map_err(|e| {
                    vec![GlyimDiagnostic::internal_error(format!("abort un: {e:?}"))]
                })?;
            }
            return Ok(true);
        }

        // Coerce the receiver value to the destination LLVM type. Three
        // shapes are possible for the receiver and three for the destination:
        //
        //   receiver  |  destination  |  action
        //   ----------+---------------+---------------------------------------
        //   T         |  T            |  pass through
        //   `{ptr,i64}` | `{ptr,i64}` |  pass through (same type)
        //   `{ptr,i64}` |  thin ptr    |  extract field 0
        //   thin ptr  |  thin ptr    |  bitcast
        //   `{[3xi64]}` (String) | `{ptr,i64}` |  unpack inner array words 0,2
        //   `{[3xi64]}` (String) |  thin ptr    |  unpack word 0 → inttoptr
        let mut val = recv_val;
        if val.get_type() != dest_llvm_ty {
            // `String` → fat pointer (e.g. `String::as_bytes`, `String::as_str`
            // via the same coercion path).
            let is_string_struct =
                val.is_struct_value() && val.into_struct_value().get_type().count_fields() == 1;
            if is_string_struct && dest_llvm_ty.is_struct_type() {
                let sv = val.into_struct_value();
                let arr = self
                    .builder
                    .build_extract_value(sv, 0, "as_bytes_arr")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_bytes array extract failed: {:?}",
                            e
                        ))]
                    })?
                    .into_array_value();
                let p_i = self
                    .builder
                    .build_extract_value(arr, 0, "as_bytes_pi")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_bytes ptr extract failed: {:?}",
                            e
                        ))]
                    })?
                    .into_int_value();
                let l = self
                    .builder
                    .build_extract_value(arr, 2, "as_bytes_li")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_bytes len extract failed: {:?}",
                            e
                        ))]
                    })?;
                let ptr_ty = self.context.ptr_type(inkwell::AddressSpace::default());
                let p = self
                    .builder
                    .build_int_to_ptr(p_i, ptr_ty, "as_bytes_p")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_bytes inttoptr failed: {:?}",
                            e
                        ))]
                    })?;
                let dst_sty = dest_llvm_ty.into_struct_type();
                let mut agg = dst_sty.get_undef();
                agg = self
                    .builder
                    .build_insert_value(agg, p, 0, "as_bytes_i0")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_bytes insert 0 failed: {:?}",
                            e
                        ))]
                    })?
                    .into_struct_value();
                agg = self
                    .builder
                    .build_insert_value(agg, l, 1, "as_bytes_i1")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "as_bytes insert 1 failed: {:?}",
                            e
                        ))]
                    })?
                    .into_struct_value();
                val = agg.as_basic_value_enum();
            }
            if val.get_type() != dest_llvm_ty
                && val.is_struct_value()
                && dest_llvm_ty.is_pointer_type()
            {
                val = self
                    .builder
                    .build_extract_value(val.into_struct_value(), 0, "fat_data_ptr")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "fat-ptr extract failed: {:?}",
                            e
                        ))]
                    })?;
            }
            if val.get_type() != dest_llvm_ty {
                if val.is_pointer_value() && dest_llvm_ty.is_pointer_type() {
                    val = self
                        .builder
                        .build_bit_cast(val, dest_llvm_ty, "intrinsic_cast")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "intrinsic bitcast failed: {:?}",
                                e
                            ))]
                        })?
                        .as_basic_value_enum();
                } else {
                    return Err(vec![GlyimDiagnostic::internal_error(format!(
                        "unsupported intrinsic `{}` receiver/destination shape (recv={:?}, dst={:?})",
                        method,
                        val.get_type(),
                        dest_llvm_ty
                    ))]);
                }
            }
        }
        self.builder
            .build_store(dest_ptr, val)
            .expect("store intrinsic result failed");
        if let Some(target_bb) = target {
            let target_block = *self
                .bb_map
                .get(target_bb)
                .expect("builtin intrinsic target block not found");
            self.builder
                .build_unconditional_branch(target_block)
                .expect("branch after builtin intrinsic failed");
        }
        Ok(true)
    }

    fn lower_call(
        &mut self,
        func: &Operand,
        args: &[Operand],
        destination: &Place,
        target: &Option<BasicBlockIdx>,
        cleanup: &Option<BasicBlockIdx>,
    ) -> CompResult<()> {
        // Builtin intrinsic methods (`str::as_bytes`, `String::as_bytes`,
        // `str::as_ptr`, …) have synthetic FnDefIds in the 9_000+ range with
        // NO MIR body. Before this dispatcher existed, `lower_call` fell
        // through to the FnDef arm below, which emitted a *call* to a symbol
        // `__glyim_fn_{id}` that was never defined — so `nm h.o` reported
        // `___glyim_fn_9102` / `___glyim_fn_9104` as undefined and the linker
        // failed. Handle the handful of builtins hello world needs by
        // emitting the intrinsic inline.
        if let Operand::Constant(c) = func
            && let MirConstKind::Fn(fn_id, _) = &c.kind
            && fn_id.to_raw() >= 9_000
            && self.try_lower_builtin_intrinsic(*fn_id, args, destination, target)?
        {
            return Ok(());
        }
        // Determine the call target. A function pointer is one form; a closure
        // is carried as an aggregate of its captures with the closure id encoded
        // in the (synthetic ADT) type. For a closure we reconstruct the callee
        // signature as `[captures..., explicit params] -> return_ty` and pull the
        // capture values out of the aggregate.
        let (fn_sig, direct_fn_val, closure_captures) = match self.operand_ty(func) {
            ty if matches!(self.ty_ctx.ty_kind(ty), TyKind::FnDef(_, _)) => {
                // A direct call to a named (top-level) function, e.g. `inc(5)`
                // or `block_on(f)`. The callee is a `MirConstKind::Fn(def_id,
                // substs)` constant; resolve it to the canonical
                // `__glyim_fn_{def_id}` symbol that `lower_body` emits for each
                // monomorphized function. (Named-function calls were previously
                // unhandled here, surfacing as "expected function pointer or
                // closure type for call operand" — the root of the M5
                // single-await runtime gap once the `TyKind::Error` ICE was
                // closed.)
                let (def_id, substs) = match self.ty_ctx.ty_kind(ty) {
                    TyKind::FnDef(d, s) => (*d, *s),
                    _ => unreachable!(),
                };
                let generic_sig = match self.ty_ctx.fn_sig(def_id) {
                    Some(s) => s.clone(),
                    None => {
                        return Err(vec![GlyimDiagnostic::internal_error(format!(
                            "no signature registered for function {:?}",
                            def_id
                        ))]);
                    }
                };
                // For generic callees the registered `FnSig` still carries the
                // `Param` types; the call's concrete argument types live in the
                // operand's `substs`. We must substitute them before computing
                // the call ABI, otherwise `fn_abi_of` calls `layout_of` on a
                // `Param` and ICEs with `UnknownType` (this is the
                // generic-`Future`/`block_on` resolution half of the M5 gap).
                // `intern_substitution` is `&mut`-only, but the call operand's
                // `substs` is already an interned `Substitution` of concrete
                // `Ty`s, so for the supported identity-`Param` shape we reuse it
                // directly without interning.
                // Concretize the callee signature by substituting the call-site
                // generic `Param`s with their concrete arguments. This handles
                // nested associated-type projections (e.g. `Poll<Self::Output>`
                // returned by a trait method like `Future::poll`) that the
                // previous top-level-only handling missed. For non-generic
                // callees `substs` is empty and the generic signature is already
                // concrete, so `concretize_fn_sig` is a no-op.
                // The call-site `substs` occasionally still carries
                // unresolved inference vars even for a builtin method whose
                // receiver is fully concrete (`Vec::<i32>::push` with
                // `substs = [Infer(TyVar(0))]` — the value that would have
                // become `i32`). Substituting `Param(0)` with that `Infer`
                // leaves the sig with an `Infer` input and ICEs
                // `fn_abi_of` with `UnknownType`. Treat any `Infer`/`Param`
                // in the call-site substs as "not yet concrete" and derive
                // the substitution map from the receiver's own ADT substs
                // instead. The receiver is always `args[0]` for an inherent
                // method.
                let call_substs_usable = {
                    let args_slice = self.ty_ctx.substitution_args(substs);
                    !args_slice.is_empty()
                        && args_slice.iter().all(|a| match a {
                            GenericArg::Ty(t) => !matches!(
                                self.ty_ctx.ty_kind(*t),
                                TyKind::Infer(_) | TyKind::Param(_)
                            ),
                            _ => true,
                        })
                };
                let infer_from_receiver = |this: &Self| -> Option<Vec<GenericArg>> {
                    let recv_ty = args.first().map(|a| this.operand_ty(a))?;
                    match this.ty_ctx.ty_kind(recv_ty) {
                        TyKind::Adt(_, s) => Some(this.ty_ctx.substitution_args(*s).to_vec()),
                        TyKind::Ref(_, inner, _) | TyKind::RawPtr(inner, _) => {
                            match this.ty_ctx.ty_kind(*inner) {
                                TyKind::Adt(_, s) => {
                                    Some(this.ty_ctx.substitution_args(*s).to_vec())
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                };
                let sig = if call_substs_usable {
                    let param_map = self.ty_ctx.substitution_args(substs).to_vec();
                    Self::concretize_fn_sig(&generic_sig, &param_map, self.ty_ctx)
                } else {
                    match infer_from_receiver(self) {
                        Some(map) if !map.is_empty() => {
                            Self::concretize_fn_sig(&generic_sig, &map, self.ty_ctx)
                        }
                        _ => generic_sig,
                    }
                };
                // Builtin trait-impl methods (`Option::<T>::unwrap`,
                // `Result::<T,E>::unwrap`, …) are registered with an
                // *unresolved inference var* as the return type (e.g.
                // `Infer(TyVar(2))`), not a `Param`. `concretize_fn_sig`
                // only substitutes `Param`, so the sig output stays
                // `Infer` and `fn_abi_of` panics with `UnknownType`.
                //
                // The concrete result type is known: it is exactly the
                // type of the destination place, which typeck resolved
                // (`Option::unwrap` on `Option<i32>` → `i32`). Override
                // the sig output with it whenever the registered output
                // is still an unresolved inference var.
                let mut sig = sig;
                if matches!(self.ty_ctx.ty_kind(sig.output), TyKind::Infer(_)) {
                    let dest_ty = self.place_ty(destination);
                    if !matches!(self.ty_ctx.ty_kind(dest_ty), TyKind::Infer(_)) {
                        sig.output = dest_ty;
                    }
                }
                // Same class of bug as the output override above: a call to a
                // generic function whose registered `FnSig` still carries a
                // not-yet-concrete `Infer` in an *input* (or nested inside
                // the output's ADT substitution — `MaybeUninit<i32>::new(5)`
                // registers `fn(Infer) -> MaybeUninit<Infer>`). `fn_abi_of`
                // then panics with `UnknownType`. The concrete type is the
                // corresponding argument's own type, which typeck resolved.
                //
                // Build an `Infer → concrete` map by aligning the generic
                // signature's still-`Infer` input Tys with the operands'
                // concrete Tys, then rewrite both `sig.inputs` and
                // `sig.output` through it. `concretize_fn_sig` only
                // substitutes `Param`, not `Infer`, so it cannot do this.
                {
                    let arg_tys: Vec<Ty> = args.iter().map(|a| self.operand_ty(a)).collect();
                    let gen_inputs = self.ty_ctx.substitution_args(sig.inputs).to_vec();
                    let mut infer_map: Vec<(u32, Ty)> = Vec::new();
                    for (i, g) in gen_inputs.iter().enumerate() {
                        if let GenericArg::Ty(t) = g {
                            if let TyKind::Infer(glyim_type::InferVar::Ty(var)) =
                                self.ty_ctx.ty_kind(*t)
                            {
                                if let Some(conc) = arg_tys.get(i) {
                                    if !matches!(self.ty_ctx.ty_kind(*conc), TyKind::Infer(_)) {
                                        infer_map.push((var.to_raw(), *conc));
                                    }
                                }
                            }
                        }
                    }
                    if !infer_map.is_empty() {
                        fn subst_infer(
                            ty: Ty,
                            map: &[(u32, Ty)],
                            ctx: &mut TyCtxMut,
                            frozen: &TyCtx,
                        ) -> Ty {
                            match frozen.ty_kind(ty).clone() {
                                TyKind::Infer(glyim_type::InferVar::Ty(v)) => map
                                    .iter()
                                    .find(|(k, _)| *k == v.to_raw())
                                    .map(|(_, c)| *c)
                                    .unwrap_or(ty),
                                TyKind::Infer(_) => ty,
                                TyKind::Adt(id, sub) => {
                                    let args = frozen.substitution_args(sub);
                                    let mut new_args: Vec<GenericArg> = Vec::new();
                                    let mut changed = false;
                                    for a in args.iter() {
                                        match a {
                                            GenericArg::Ty(t) => {
                                                let nt = subst_infer(*t, map, ctx, frozen);
                                                if nt != *t {
                                                    changed = true;
                                                }
                                                new_args.push(GenericArg::Ty(nt));
                                            }
                                            other => new_args.push(other.clone()),
                                        }
                                    }
                                    if changed {
                                        let s = ctx.intern_substitution(new_args);
                                        ctx.mk_ty(TyKind::Adt(id, s))
                                    } else {
                                        ty
                                    }
                                }
                                TyKind::Ref(r, inner, m) => {
                                    let ni = subst_infer(inner, map, ctx, frozen);
                                    if ni == inner {
                                        ty
                                    } else {
                                        ctx.mk_ty(TyKind::Ref(r, ni, m))
                                    }
                                }
                                TyKind::RawPtr(inner, m) => {
                                    let ni = subst_infer(inner, map, ctx, frozen);
                                    if ni == inner {
                                        ty
                                    } else {
                                        ctx.mk_ty(TyKind::RawPtr(ni, m))
                                    }
                                }
                                TyKind::Tuple(sub) => {
                                    let args = frozen.substitution_args(sub);
                                    let mut new_args: Vec<GenericArg> = Vec::new();
                                    let mut changed = false;
                                    for a in args.iter() {
                                        match a {
                                            GenericArg::Ty(t) => {
                                                let nt = subst_infer(*t, map, ctx, frozen);
                                                if nt != *t {
                                                    changed = true;
                                                }
                                                new_args.push(GenericArg::Ty(nt));
                                            }
                                            other => new_args.push(other.clone()),
                                        }
                                    }
                                    if changed {
                                        let s = ctx.intern_substitution(new_args);
                                        ctx.mk_ty(TyKind::Tuple(s))
                                    } else {
                                        ty
                                    }
                                }
                                _ => ty,
                            }
                        }
                        let mut ctx = TyCtxMut::from_ty_ctx(self.ty_ctx);
                        let mut new_inputs: Vec<GenericArg> = Vec::new();
                        for g in gen_inputs.iter() {
                            match g {
                                GenericArg::Ty(t) => {
                                    new_inputs.push(GenericArg::Ty(subst_infer(
                                        *t,
                                        &infer_map,
                                        &mut ctx,
                                        self.ty_ctx,
                                    )));
                                }
                                other => new_inputs.push(other.clone()),
                            }
                        }
                        let new_output = subst_infer(sig.output, &infer_map, &mut ctx, self.ty_ctx);
                        sig.inputs = ctx.intern_substitution(new_inputs);
                        sig.output = new_output;
                    }
                }
                // Companion pass for the *`Param`* case (assoc-fn calls with
                // empty call-site substs, e.g. `MaybeUninit::<i32>::new(5)`
                // where the MIR gives no substitution but the registered sig
                // is `fn(Param(0)) -> MaybeUninit<Param(0)>`). Build a
                // partial `Param(idx) -> concrete arg type` map, then let
                // `concretize_fn_sig` do the (recursive, ADT-aware) rewrite.
                {
                    let arg_tys: Vec<Ty> = args.iter().map(|a| self.operand_ty(a)).collect();
                    let gen_inputs = self.ty_ctx.substitution_args(sig.inputs).to_vec();
                    let mut pmap: Vec<Option<Ty>> = Vec::new();
                    for (i, g) in gen_inputs.iter().enumerate() {
                        if let GenericArg::Ty(t) = g {
                            if let TyKind::Param(p) = self.ty_ctx.ty_kind(*t) {
                                let idx = p.index as usize;
                                if pmap.len() <= idx {
                                    pmap.resize(idx + 1, None);
                                }
                                if pmap[idx].is_none() {
                                    if let Some(conc) = arg_tys.get(i) {
                                        if !matches!(
                                            self.ty_ctx.ty_kind(*conc),
                                            TyKind::Infer(_) | TyKind::Param(_)
                                        ) {
                                            pmap[idx] = Some(*conc);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if pmap.iter().any(|x| x.is_some()) {
                        let map_args: Vec<GenericArg> = pmap
                            .into_iter()
                            .map(|x| match x {
                                Some(t) => GenericArg::Ty(t),
                                None => GenericArg::Ty(Ty::ERROR),
                            })
                            .collect();
                        sig = Self::concretize_fn_sig(&sig, &map_args, self.ty_ctx);
                    }
                }
                let fn_name = match self.ty_ctx.extern_fn_name(def_id) {
                    Some(ext) => ext.to_string(),
                    None => format!("__glyim_fn_{}", def_id.to_raw()),
                };
                let fn_val = match self.module.get_function(&fn_name) {
                    Some(v) => v,
                    None => {
                        // Forward-declare the callee so the call can target it;
                        // `lower_body` will later `add_function` the same symbol
                        // (inkwell reuses the existing declaration) and define
                        // its body. This mirrors the `FnPtr` arm's tolerance
                        // for call-site-before-definition ordering.
                        let fn_type = self.llvm_fn_type_from_sig(&sig)?;
                        self.module.add_function(&fn_name, fn_type, None)
                    }
                };
                (sig, Some(fn_val), None)
            }
            ty if matches!(self.ty_ctx.ty_kind(ty), TyKind::FnPtr(_)) => {
                let sig = match self.ty_ctx.ty_kind(ty) {
                    TyKind::FnPtr(sig) => sig.clone(),
                    _ => unreachable!(),
                };
                (sig, None, None)
            }
            ty if matches!(self.ty_ctx.ty_kind(ty), TyKind::Closure(_, _)) => {
                // A closure is carried as a `TyKind::Closure` value whose raw id
                // is its ClosureId (see `register_closure`). Recover the closure
                // id and its precomputed full signature
                // `[captures..., explicit params] -> ret`.
                let closure_id = match self.ty_ctx.ty_kind(ty) {
                    TyKind::Closure(closure_id, _) => *closure_id,
                    _ => unreachable!(),
                };
                let fn_sig = match self.ty_ctx.closure_sig(closure_id) {
                    Some(sig) => sig.clone(),
                    None => {
                        return Err(vec![GlyimDiagnostic::internal_error(format!(
                            "no signature registered for closure {}",
                            closure_id.to_raw()
                        ))]);
                    }
                };
                let capture_count = (fn_sig.inputs.len() as usize) - args.len();
                let fn_name = format!("__glyim_fn_{}", closure_id.to_raw());
                let direct_fn_val = self.module.get_function(&fn_name);
                let func_val = self.lower_operand(func)?;
                let struct_val = func_val.into_struct_value();
                let mut captures: Vec<BasicValueEnum<'ctx>> = Vec::new();
                for i in 0..capture_count {
                    let field_val = self
                        .builder
                        .build_extract_value(struct_val, i as u32, "capture")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "extract closure capture failed: {:?}",
                                e
                            ))]
                        })?;
                    captures.push(field_val);
                }
                (fn_sig, direct_fn_val, Some(captures))
            }
            _ => {
                return Err(vec![GlyimDiagnostic::internal_error(
                    "expected function pointer or closure type for call operand",
                )]);
            }
        };
        let layout_computer = FullLayoutComputer::new(self.ty_ctx, self.target_info.clone());
        let fn_abi = layout_computer.fn_abi_of(&fn_sig).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "Layout error: {:?}",
                e
            ))]
        })?;
        let is_sret = matches!(fn_abi.ret.mode, PassMode::Indirect { .. });
        let fn_type = self.llvm_fn_type_from_sig(&fn_sig)?;
        let mut llvm_args: Vec<inkwell::values::BasicValueEnum<'ctx>> = Vec::new();
        let mut sret_alloca = None;
        if is_sret {
            let sret_llvm_ty = self.llvm_type_for_ty(fn_abi.ret.ty)?;
            let sret_ptr = self
                .builder
                .build_alloca(sret_llvm_ty, "sret")
                .expect("alloca sret failed");
            llvm_args.push(sret_ptr.as_basic_value_enum());
            sret_alloca = Some(sret_ptr);
        }
        // T112-PATCHED [LL-17]: `PassMode::Ignore` args (ZSTs like `()`)
        // are dropped from the LLVM parameter list but are still present in
        // MIR's `args`. The previous code `continue`d *before* the arg_idx
        // increment, so the next ABI arg was read from `args[arg_idx]` with
        // `arg_idx` still pointing at the ignored MIR arg — the caller fed
        // the ZST operand to the following non-ZST parameter. Advance the
        // index even for ignored args.
        let mut arg_idx = 0;
        for arg_abi in &fn_abi.args {
            if matches!(arg_abi.mode, PassMode::Ignore) {
                arg_idx += 1;
                continue;
            }
            if arg_idx >= args.len() + closure_captures.as_ref().map(|c| c.len()).unwrap_or(0) {
                return Err(vec![GlyimDiagnostic::internal_error(
                    "argument count mismatch",
                )]);
            }
            // For a closure call the leading ABI args are the captured
            // environment values (already extracted from the aggregate); the
            // trailing args are the explicit call arguments.
            let arg_val = if let Some(captures) = closure_captures.as_ref() {
                if arg_idx < captures.len() {
                    captures[arg_idx]
                } else {
                    self.lower_operand(&args[arg_idx - captures.len()])?
                }
            } else {
                self.lower_operand(&args[arg_idx])?
            };
            match arg_abi.mode {
                PassMode::Direct => llvm_args.push(arg_val),
                PassMode::Indirect { .. } => {
                    let llvm_ty = self.llvm_type_for_ty(arg_abi.ty)?;
                    let tmp_ptr = self
                        .builder
                        .build_alloca(llvm_ty, "arg")
                        .expect("alloca arg failed");
                    self.builder
                        .build_store(tmp_ptr, arg_val)
                        .expect("store arg failed");
                    llvm_args.push(tmp_ptr.as_basic_value_enum());
                }
                PassMode::Ignore => unreachable!(),
                PassMode::Cast { .. } => {
                    let layout = layout_computer.layout_of(arg_abi.ty).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "Layout error in Cast: {:?}",
                            e
                        ))]
                    })?;
                    let size_bits = layout.size.0 * 8;
                    let cast_ty = self.llvm_int_type(size_bits as u32);
                    let cast_ptr = self
                        .builder
                        .build_alloca(cast_ty, "arg_cast")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "alloca failed: {:?}",
                                e
                            ))]
                        })?;
                    self.builder.build_store(cast_ptr, arg_val).map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "store failed: {:?}",
                            e
                        ))]
                    })?;
                    let casted_val = self
                        .builder
                        .build_load(cast_ty, cast_ptr, "arg_casted")
                        .map_err(|e| {
                            vec![GlyimDiagnostic::internal_error(format!(
                                "load failed: {:?}",
                                e
                            ))]
                        })?;
                    llvm_args.push(casted_val);
                }
                PassMode::HomogeneousAggregate { .. } | PassMode::Split { .. } => {
                    let struct_val = arg_val.into_struct_value();
                    let num_fields = struct_val.get_type().count_fields();
                    for i in 0..num_fields {
                        let field_val = self
                            .builder
                            .build_extract_value(struct_val, i, "agg_field")
                            .map_err(|e| {
                                vec![GlyimDiagnostic::internal_error(format!(
                                    "extract failed: {:?}",
                                    e
                                ))]
                            })?;
                        llvm_args.push(field_val);
                    }
                }
            }
            arg_idx += 1;
        }
        let metadata_args: Vec<inkwell::values::BasicMetadataValueEnum<'ctx>> =
            llvm_args.iter().map(|v| (*v).into()).collect();
        let use_invoke = cleanup.is_some();
        let call_result = if let Some(fn_val) = direct_fn_val {
            if use_invoke {
                let normal_bb = if let Some(target_bb) = target {
                    *self.bb_map.get(target_bb).expect("target block not found")
                } else {
                    return Err(vec![GlyimDiagnostic::internal_error(
                        "invoke requires a target block",
                    )]);
                };
                let cleanup_bb = if let Some(cleanup_bb_idx) = cleanup {
                    *self
                        .bb_map
                        .get(cleanup_bb_idx)
                        .expect("cleanup block not found")
                } else {
                    return Err(vec![GlyimDiagnostic::internal_error(
                        "invoke requires a cleanup block",
                    )]);
                };
                self.builder
                    .build_invoke(fn_val, &llvm_args, normal_bb, cleanup_bb, "call")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "invoke failed: {:?}",
                            e
                        ))]
                    })?
            } else {
                self.builder
                    .build_call(fn_val, &metadata_args, "call")
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "call failed: {:?}",
                            e
                        ))]
                    })?
            }
        } else {
            if closure_captures.is_some() {
                return Err(vec![GlyimDiagnostic::internal_error(
                    "closure body function was not emitted before the call site",
                )]);
            }
            let func_val = self.lower_operand(func)?.into_pointer_value();
            if use_invoke {
                let normal_bb = if let Some(target_bb) = target {
                    *self.bb_map.get(target_bb).expect("target block not found")
                } else {
                    return Err(vec![GlyimDiagnostic::internal_error(
                        "invoke requires a target block",
                    )]);
                };
                let cleanup_bb = if let Some(cleanup_bb_idx) = cleanup {
                    *self
                        .bb_map
                        .get(cleanup_bb_idx)
                        .expect("cleanup block not found")
                } else {
                    return Err(vec![GlyimDiagnostic::internal_error(
                        "invoke requires a cleanup block",
                    )]);
                };
                self.builder
                    .build_indirect_invoke(
                        fn_type, func_val, &llvm_args, normal_bb, cleanup_bb, "call",
                    )
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "invoke failed: {:?}",
                            e
                        ))]
                    })?
            } else {
                self.builder
                    .build_indirect_call(fn_type, func_val, &metadata_args, "call")
                    .inspect(|call_site| {
                        // T115-PATCHED [LL-20]: set the call-site convention
                        // to match the callee's declared ABI. Direct calls
                        // inherit the callee's convention automatically,
                        // but indirect calls default to `ccc`, while our
                        // `Abi::Glyim` functions are emitted with fastcc
                        // (8). Calling fastcc through a ccc site silently
                        // drops the return value on AArch64.
                        call_site.set_call_convention(8u32);
                    })
                    .map_err(|e| {
                        vec![GlyimDiagnostic::internal_error(format!(
                            "call failed: {:?}",
                            e
                        ))]
                    })?
            }
        };
        let mut param_idx = 0;
        if is_sret {
            let sret_attr = self.context.create_enum_attribute(
                inkwell::attributes::Attribute::get_named_enum_kind_id("sret"),
                0,
            );
            call_result.add_attribute(
                inkwell::attributes::AttributeLoc::Param(param_idx),
                sret_attr,
            );
            param_idx += 1;
        }
        for arg_abi in &fn_abi.args {
            if let PassMode::Indirect { .. } = arg_abi.mode {
                let llvm_ty = self.llvm_type_for_ty(arg_abi.ty)?;
                let any_ty = match llvm_ty {
                    inkwell::types::BasicTypeEnum::ArrayType(t) => {
                        inkwell::types::AnyTypeEnum::ArrayType(t)
                    }
                    inkwell::types::BasicTypeEnum::FloatType(t) => {
                        inkwell::types::AnyTypeEnum::FloatType(t)
                    }
                    inkwell::types::BasicTypeEnum::IntType(t) => {
                        inkwell::types::AnyTypeEnum::IntType(t)
                    }
                    inkwell::types::BasicTypeEnum::PointerType(t) => {
                        inkwell::types::AnyTypeEnum::PointerType(t)
                    }
                    inkwell::types::BasicTypeEnum::StructType(t) => {
                        inkwell::types::AnyTypeEnum::StructType(t)
                    }
                    inkwell::types::BasicTypeEnum::VectorType(t) => {
                        inkwell::types::AnyTypeEnum::VectorType(t)
                    }
                    inkwell::types::BasicTypeEnum::ScalableVectorType(t) => {
                        inkwell::types::AnyTypeEnum::ScalableVectorType(t)
                    }
                };
                let byval_attr = self.context.create_type_attribute(
                    inkwell::attributes::Attribute::get_named_enum_kind_id("byval"),
                    any_ty,
                );
                call_result.add_attribute(
                    inkwell::attributes::AttributeLoc::Param(param_idx),
                    byval_attr,
                );
            }
            if !matches!(arg_abi.mode, PassMode::Ignore) {
                param_idx += 1;
            }
        }
        if use_invoke && let Some(target_bb) = target {
            let target_block = self.bb_map.get(target_bb).unwrap();
            self.builder.position_at_end(*target_block);
        }
        if is_sret {
            let sret_ptr = sret_alloca.unwrap();
            let sret_ty = self.llvm_type_for_ty(fn_abi.ret.ty)?;
            let sret_val = self
                .builder
                .build_load(sret_ty, sret_ptr, "sret_load")
                .expect("load sret failed");
            let dest_ptr = self.place_ptr(destination)?;
            self.builder
                .build_store(dest_ptr, sret_val)
                .expect("store sret failed");
        } else if !matches!(fn_abi.ret.mode, PassMode::Ignore) {
            let ret_val = match call_result.as_any_value_enum() {
                AnyValueEnum::IntValue(v) => BasicValueEnum::IntValue(v),
                AnyValueEnum::FloatValue(v) => BasicValueEnum::FloatValue(v),
                AnyValueEnum::PointerValue(v) => BasicValueEnum::PointerValue(v),
                AnyValueEnum::StructValue(v) => BasicValueEnum::StructValue(v),
                AnyValueEnum::ArrayValue(v) => BasicValueEnum::ArrayValue(v),
                AnyValueEnum::VectorValue(v) => BasicValueEnum::VectorValue(v),
                AnyValueEnum::ScalableVectorValue(v) => BasicValueEnum::ScalableVectorValue(v),
                _ => {
                    return Err(vec![GlyimDiagnostic::internal_error(
                        "unexpected call return kind",
                    )]);
                }
            };
            let dest_ptr = self.place_ptr(destination)?;
            self.builder
                .build_store(dest_ptr, ret_val)
                .expect("store ret failed");
        }
        if !use_invoke {
            if let Some(target_bb) = target {
                let target_block = self.bb_map.get(target_bb).expect("target block not found");
                self.builder
                    .build_unconditional_branch(*target_block)
                    .expect("branch failed");
            } else {
                self.builder
                    .build_unreachable()
                    .expect("unreachable failed");
            }
        }
        Ok(())
    }

    fn operand_ty(&self, operand: &Operand) -> Ty {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_ty(place),
            Operand::Constant(c) => c.ty,
        }
    }

    /// Emit a landingpad instruction for the cleanup block currently being
    /// positioned at, and record its value in `self.current_landingpad`.
    ///
    /// Glyim has no user-visible `catch` construct (panics are always fatal
    /// -- see `glyim_panic` in glyim-runtime), so every landingpad we emit
    /// is a pure *cleanup* landingpad: it doesn't filter by exception type
    /// at all (`clauses = &[]`, `is_cleanup = true`), it just gives us a
    /// legal place for the unwinder to hand control back to us so that any
    /// `Drop` terminators already present in this cleanup block (emitted by
    /// `glyim-opt::drop_elaboration`) get to run before we `resume` the
    /// unwind (see `TerminatorKind::Unreachable` above).
    fn emit_landingpad(&mut self) -> CompResult<()> {
        let Some(personality_fn) = self._personality_fn else {
            self.current_landingpad = None;
            self.current_seh_pad = None;
            return Ok(());
        };
        match self.personality {
            Personality::Seh => self.emit_seh_cleanuppad(personality_fn),
            Personality::Itanium | Personality::None => {
                self.emit_itanium_landingpad(personality_fn)
            }
        }
    }

    /// Emit an SEH-style `cleanuppad` token for the current cleanup block.
    ///
    /// Glyim has no user-visible `catch`; panics are always fatal
    /// (`glyim_panic` in glyim-runtime), so the pad carries no filter args and
    /// just gives the unwinder a legal place to resume so that any `Drop`
    /// terminators already present in this cleanup block (emitted by
    /// `glyim-opt::drop_elaboration`) run before we `cleanupret` back to the
    /// caller's pad. A top-level cleanuppad has no parent pad (null token);
    /// nested cleanups within a cleanup (e.g. a destructor that itself panics)
    /// would pass the enclosing pad instead.
    fn emit_seh_cleanuppad(
        &mut self,
        _personality_fn: inkwell::values::FunctionValue<'ctx>,
    ) -> CompResult<()> {
        use crate::seh_ffi::LLVMBuildCleanupPad;
        let name = std::ffi::CString::new("cleanuppad").unwrap();
        let cleanuppad = unsafe {
            LLVMBuildCleanupPad(
                self.builder.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                name.as_ptr(),
            )
        };
        if cleanuppad.is_null() {
            return Err(vec![GlyimDiagnostic::internal_error(
                "LLVMBuildCleanupPad returned null -- SEH funclet emission failed",
            )]);
        }
        self.current_seh_pad = Some(cleanuppad);
        // No `current_landingpad`: SEH resumes via `cleanupret`, not `resume`.
        self.current_landingpad = None;
        Ok(())
    }

    /// Emit an Itanium-style `landingpad` instruction for the cleanup block
    /// currently being positioned at, recording its value in
    /// `self.current_landingpad`.
    ///
    /// Glyim has no user-visible `catch` construct (panics are always fatal
    /// -- see `glyim_panic` in glyim-runtime), so every landingpad we emit
    /// is a pure *cleanup* landingpad: it doesn't filter by exception type
    /// at all (`clauses = &[]`, `is_cleanup = true`), it just gives us a
    /// legal place for the unwinder to hand control back to us so that any
    /// `Drop` terminators already present in this cleanup block (emitted by
    /// `glyim-opt::drop_elaboration`) get to run before we `resume` the
    /// unwind (see `TerminatorKind::Unreachable` above).
    fn emit_itanium_landingpad(
        &mut self,
        personality_fn: inkwell::values::FunctionValue<'ctx>,
    ) -> CompResult<()> {
        let i8_ptr_ty = self.context.ptr_type(AddressSpace::default());
        let i32_ty = self.context.i32_type();
        let landingpad_ty = self
            .context
            .struct_type(&[i8_ptr_ty.into(), i32_ty.into()], false);
        let landingpad = self
            .builder
            .build_landing_pad(landingpad_ty, personality_fn, &[], true, "lpad")
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "landingpad failed: {:?}",
                    e
                ))]
            })?;

        self.current_landingpad = Some(landingpad);
        Ok(())
    }
    fn type_needs_drop(&self, ty: Ty) -> bool {
        match self.ty_ctx.ty_kind(ty) {
            TyKind::Ref(_, _, _) | TyKind::RawPtr(_, _) => false,
            TyKind::Array(inner, _) => self.type_needs_drop(*inner),
            TyKind::Tuple(substs) => self.ty_ctx.substitution_args(*substs).iter().any(|arg| {
                if let glyim_type::GenericArg::Ty(t) = arg {
                    self.type_needs_drop(*t)
                } else {
                    false
                }
            }),
            TyKind::Adt(adt_id, _) => {
                if let Some(adt_def) = self.ty_ctx.adt_def(*adt_id) {
                    adt_def
                        .variants
                        .iter()
                        .any(|v| v.fields.iter().any(|f| self.type_needs_drop(f.ty)))
                } else {
                    false
                }
            }
            TyKind::Closure(_, substs) => {
                self.ty_ctx.substitution_args(*substs).iter().any(|arg| {
                    if let glyim_type::GenericArg::Ty(t) = arg {
                        self.type_needs_drop(*t)
                    } else {
                        false
                    }
                })
            }
            TyKind::String => true,
            TyKind::Bool
            | TyKind::Int(_)
            | TyKind::Uint(_)
            | TyKind::Float(_)
            | TyKind::Char
            | TyKind::Never
            | TyKind::Unit
            | TyKind::FnDef(_, _)
            | TyKind::FnPtr(_) => false,
            // Unreachable after TASK-P1-6, but conservative fallback is true.
            TyKind::Dynamic(..)
            | TyKind::Opaque(..)
            | TyKind::Projection(_)
            | TyKind::Param(_)
            | TyKind::Bound(_, _)
            | TyKind::Infer(_) => true,
            _ => false,
        }
    }

    fn type_is_owning_pointer(&self, ty: Ty) -> bool {
        match self.ty_ctx.ty_kind(ty) {
            // INTENTIONAL: only Ref(Mut)/RawPtr(Mut) are owning-pointer-like today;
            // extend this match when an owned-box type is added to glyim-type.
            TyKind::Ref(_, inner, glyim_core::primitives::Mutability::Mut)
            | TyKind::RawPtr(inner, glyim_core::primitives::Mutability::Mut) => {
                !self.ty_ctx.is_copy(*inner)
            }
            _ => false,
        }
    }
}

/// Map an unsized type (slice `[T]`, `str`/`String`) to its fat-pointer twin
/// (`{ ptr, len }`) so it can be passed as a function argument / used in an
/// `FnSig` without tripping `layout_of`'s `Unsized` error. Sized types are
/// returned unchanged.
fn lower_unsized_to_fat_ptr(ty_ctx: &TyCtx, ty: Ty) -> Ty {
    let mk_fat_ptr = |elem: Ty| -> Ty {
        let ptr_ty = ty_ctx.mk_ref(
            glyim_type::Region::Static,
            elem,
            glyim_core::primitives::Mutability::Not,
        );
        let len_ty = ty_ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::Usize));
        // A 2-field tuple `{ ptr, usize }` is the fat-pointer representation
        // `llvm_type_for_ty` produces for slices (struct { ptr, len }).
        let substs =
            ty_ctx.intern_substitution(vec![GenericArg::Ty(ptr_ty), GenericArg::Ty(len_ty)]);
        ty_ctx.mk_ty(TyKind::Tuple(substs))
    };
    match ty_ctx.ty_kind(ty) {
        TyKind::Slice(elem) => mk_fat_ptr(*elem),
        TyKind::String => {
            mk_fat_ptr(ty_ctx.mk_ty(TyKind::Uint(glyim_core::primitives::UintTy::U8)))
        }
        _ => ty,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn lower_body<'ctx>(
    context: &'ctx Context,
    module: &Module<'ctx>,
    body: &Body,
    target_info: TargetInfo,
    ty_ctx: &TyCtx,
    debug_info: bool,
    source_map: HashMap<FileId, (String, String)>,
    hygiene: Option<HygieneCtx>,
    entry_main: Option<u32>,
) -> CompResult<()> {
    let fn_name = format!("__glyim_fn_{}", body.owner.local_id.to_raw());
    let layout_computer = FullLayoutComputer::new(ty_ctx, target_info.clone());
    let fn_def_id = glyim_core::def_id::FnDefId::from_raw(body.owner.local_id.to_raw());
    // Build the FnSig from the (already-monomorphized) body so that generic /
    // async functions like `block_on<F>` or `Future::poll` lower without an
    // `UnknownType(Param)` ICE in `fn_abi_of`. The LLVM function *type* below is
    // derived from the same `fn_sig` so the sret hidden-pointer parameter and the
    // function's declared type stay in agreement.
    let fn_sig = {
        let mut input_args: Vec<GenericArg> = Vec::new();
        for i in 1..=body.arg_count {
            let local_idx = LocalIdx::from_raw(i as u32);
            if let Some(local_decl) = body.locals.get(local_idx) {
                // Unsized types (slice, str) cannot be passed by value — the
                // ABI layout of an unsized `Ty` is an error (`Unsized(Ty)`).
                // The MIR represents such a parameter as its fat-pointer type
                // (ptr + len), which is what `llvm_type_for_ty` lowers it to.
                // Map the unsized local type to its fat-pointer twin here so
                // `fn_abi_of`/`layout_of` never receive an unsized argument.
                let arg_ty = lower_unsized_to_fat_ptr(ty_ctx, local_decl.ty);
                input_args.push(GenericArg::Ty(arg_ty));
            }
        }
        let inputs = ty_ctx.intern_substitution(input_args);
        let abi = ty_ctx
            .fn_sig(fn_def_id)
            .map(|s| s.abi)
            .unwrap_or(Abi::Glyim);
        let param_map: Vec<GenericArg> = body
            .locals
            .iter()
            .skip(1)
            .map(|l| GenericArg::Ty(l.ty))
            .collect();
        let concretized = LoweringCtx::concretize_fn_sig(
            &FnSig {
                inputs,
                output: body.return_ty,
                c_variadic: false,
                unsafety: Safety::Safe,
                abi,
            },
            &param_map,
            ty_ctx,
        );
        FnSig {
            inputs: concretized.inputs,
            output: concretized.output,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi,
        }
    };
    // Derive the LLVM function type (including any sret hidden pointer) from the
    // same `fn_sig` so the declared type matches the ABI attributes applied below.
    let fn_type =
        llvm_fn_type_from_sig_inner(ty_ctx, &target_info, context, &layout_computer, &fn_sig)
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "fn type lowering failed: {:?}",
                    e
                ))]
            })?;
    let function = module
        .get_function(&fn_name)
        .unwrap_or_else(|| module.add_function(&fn_name, fn_type, None));
    // Emit the calling convention for FFI functions (unstub-5 Phase 4.3).
    // `extern "C" fn` / `extern fn` compile to the platform C calling
    // convention so the symbol is callable from C; the default Glyim ABI uses
    // the LLVM `Fast` convention (not externally callable). `system` maps to
    // the target's system convention (Win64 on Windows, C elsewhere).
    // inkwell 0.10 exposes this via `set_call_conventions(u32)` using the raw
    // LLVM calling-convention IDs (C=0, Fast=8, Win64=64).
    let cc = match fn_sig.abi {
        Abi::Glyim => 8u32, // LLVMCCallConv::Fast
        Abi::C => 0u32,     // LLVMCCallConv::C
        Abi::System => match target_info.abi {
            TargetAbi::X86_64Windows | TargetAbi::AArch64Windows => 64u32, // Win64
            _ => 0u32,                                                     // C
        },
    };
    function.set_call_conventions(cc);
    // The crate entry `main` body is called by the generated C-convention
    // `main` wrapper (and by the C shim). On AArch64, calling a `fastcc` fn
    // from a `ccc` site silently drops the return value (different return
    // register), so force the entry-main body to the C convention as well.
    if entry_main == Some(body.owner.local_id.to_raw()) {
        function.set_call_conventions(0u32); // LLVMCCallConv::C
    }
    let mut is_sret_ret = false;
    if let Ok(fn_abi) = layout_computer.fn_abi_of(&fn_sig) {
        is_sret_ret = matches!(fn_abi.ret.mode, glyim_layout::PassMode::Indirect { .. });
        let mut param_idx = 0;
        if matches!(fn_abi.ret.mode, glyim_layout::PassMode::Indirect { .. }) {
            let sret_attr = context.create_enum_attribute(
                inkwell::attributes::Attribute::get_named_enum_kind_id("sret"),
                0,
            );
            function.add_attribute(
                inkwell::attributes::AttributeLoc::Param(param_idx),
                sret_attr,
            );
            param_idx += 1;
        }
        for arg_abi in &fn_abi.args {
            if let glyim_layout::PassMode::Indirect { .. } = arg_abi.mode {
                let llvm_ty = llvm_type_for_ty(ty_ctx, &target_info, context, arg_abi.ty)
                    .unwrap_or(context.i8_type().into());
                let any_ty = match llvm_ty {
                    inkwell::types::BasicTypeEnum::ArrayType(t) => {
                        inkwell::types::AnyTypeEnum::ArrayType(t)
                    }
                    inkwell::types::BasicTypeEnum::FloatType(t) => {
                        inkwell::types::AnyTypeEnum::FloatType(t)
                    }
                    inkwell::types::BasicTypeEnum::IntType(t) => {
                        inkwell::types::AnyTypeEnum::IntType(t)
                    }
                    inkwell::types::BasicTypeEnum::PointerType(t) => {
                        inkwell::types::AnyTypeEnum::PointerType(t)
                    }
                    inkwell::types::BasicTypeEnum::StructType(t) => {
                        inkwell::types::AnyTypeEnum::StructType(t)
                    }
                    inkwell::types::BasicTypeEnum::VectorType(t) => {
                        inkwell::types::AnyTypeEnum::VectorType(t)
                    }
                    inkwell::types::BasicTypeEnum::ScalableVectorType(t) => {
                        inkwell::types::AnyTypeEnum::ScalableVectorType(t)
                    }
                };
                let byval_attr = context.create_type_attribute(
                    inkwell::attributes::Attribute::get_named_enum_kind_id("byval"),
                    any_ty,
                );
                function.add_attribute(
                    inkwell::attributes::AttributeLoc::Param(param_idx),
                    byval_attr,
                );
            }
            param_idx += 1;
        }
    }

    let mut debug_ctx = if debug_info {
        Some(DebugInfoCtx::new(
            context, module, source_map, true, hygiene,
        ))
    } else {
        None
    };
    if let Some(ref mut di) = debug_ctx {
        di.set_function(context, &function, &fn_name, FileId::from_raw(0), 1);
    }
    let entry_block = context.append_basic_block(function, "entry");
    let builder = context.create_builder();
    builder.position_at_end(entry_block);
    let mut bb_map: HashMap<BasicBlockIdx, inkwell::basic_block::BasicBlock<'ctx>> = HashMap::new();
    bb_map.insert(BasicBlockIdx::from_raw(0), entry_block);
    for (bb_idx, _bb_data) in body.basic_blocks.iter_enumerated() {
        if bb_idx != BasicBlockIdx::from_raw(0) {
            let bb_name = format!("bb_{}", bb_idx.index());
            let llvm_bb = context.append_basic_block(function, &bb_name);
            bb_map.insert(bb_idx, llvm_bb);
        }
    }
    let num_locals = body.locals.len();
    let mut locals: IndexVec<LocalIdx, Option<PointerValue<'ctx>>> =
        IndexVec::with_capacity(num_locals);
    for _ in 0..num_locals {
        locals.push(None);
    }
    let _ptr_type = context.ptr_type(AddressSpace::default());
    let _i64_type = context
        .custom_width_int_type(NonZeroU32::new(64).unwrap())
        .unwrap();
    let has_cleanup = body.basic_blocks.iter().any(|bb| bb.is_cleanup);
    let personality = select_personality(&target_info, has_cleanup);
    let personality_fn = match personality {
        Personality::None => None,
        Personality::Itanium | Personality::Seh => {
            // `__gcc_personality_v0` is the Itanium-ABI personality routine
            // provided by libgcc on Linux/glibc (and the `__gxx_personality_v0`
            // family on macOS) targets, which is what the build's linker (see
            // glyim-cli/src/linker.rs, which shells out to `cc`) resolves at
            // link time without extra runtime support. This is declared, not
            // defined, here — exactly like a C/C++ program compiled with
            // `-fexceptions` would.
            //
            // On Windows (`Seh`) the personality is `__CxxFrameHandler3`; the
            // SEH funclet-based landingpad lowering that consumes it is tracked
            // under §19.1 and is not yet wired (the landingpad path below is the
            // Itanium one). For now we still emit the correct personality name
            // so the symbol is declared for the linker.
            let personality_name = match target_info.abi {
                TargetAbi::X86_64Windows | TargetAbi::AArch64Windows => "__CxxFrameHandler3",
                _ => "__gcc_personality_v0",
            };
            let personality_fn_type = context.i32_type().fn_type(&[], true);
            let personality_fn = module.get_function(personality_name).unwrap_or_else(|| {
                module.add_function(personality_name, personality_fn_type, None)
            });
            function.set_personality_function(personality_fn);
            Some(personality_fn)
        }
    };
    let mut lowering_ctx = LoweringCtx {
        context,
        builder,
        function,
        module,
        body,
        target_info,
        ty_ctx,
        locals,
        bb_map,
        _personality_fn: personality_fn,
        personality,
        debug_ctx,
        current_landingpad: None,
        current_seh_pad: None,
        is_sret: is_sret_ret,
    };
    for (local_idx, _local_decl) in body.locals.iter_enumerated() {
        lowering_ctx.alloc_local(local_idx)?;
    }
    // Prologue: copy each incoming LLVM function parameter into its local
    // slot. The MIR uses locals 1..=arg_count as the function's formal
    // parameters; if we don't materialize the LLVM `%0..%n` params into those
    // slots, every read of a parameter reads uninitialized stack garbage
    // (silent miscompile of *every* function call in real-llvm codegen).
    let fn_abi = layout_computer.fn_abi_of(&fn_sig).ok();
    // T112-PATCHED [LL-17]: map LLVM params to MIR locals by iterating the
    // ABI classification, not by assuming `param_idx == mir_idx`. ZST
    // (`PassMode::Ignore`) params are absent from the LLVM signature but
    // still present in MIR; the previous `param_idx = i - 1` mapping read
    // the wrong LLVM parameter (or nothing) whenever a ZST appeared before
    // a real argument. Each MIR local `1..=arg_count` is matched to its
    // `fn_abi.args[i - 1]` entry; when that entry is Ignore, the local
    // stays uninitialized (it holds a ZST anyway, so nothing to store).
    let params = function.get_params();
    // Offset of the first non-hidden LLVM parameter. sret prepends a
    // pointer to the return slot.
    let llvm_param_offset: u32 = if lowering_ctx.is_sret { 1 } else { 0 };
    // Walk the ABI arg list, tracking the LLVM param index as we go and
    // skipping `Ignore` entries (which contribute no LLVM param).
    let mut next_llvm_param: u32 = llvm_param_offset;
    // MIR arg locals are numbered sequentially from 1, so the loop index
    // doubles as the local counter.
    for (arg_pos, arg_abi) in fn_abi
        .as_ref()
        .map(|abi| abi.args.as_slice())
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        let local_idx = LocalIdx::from_raw(1 + arg_pos as u32);
        if matches!(arg_abi.mode, glyim_layout::PassMode::Ignore) {
            // No LLVM param for this ABI entry; skip.
            continue;
        }
        let param_idx = next_llvm_param;
        next_llvm_param += 1;
        if let Some(param_val) = params.get(param_idx as usize) {
            let local_ptr = lowering_ctx.get_local_ptr(local_idx);
            let arg_mode = matches!(arg_abi.mode, glyim_layout::PassMode::Indirect { .. });
            match arg_mode {
                true => {
                    let llvm_ty = lowering_ctx
                        .llvm_type_for_ty(body.locals.get(local_idx).map(|l| l.ty).unwrap())?;
                    let loaded = lowering_ctx
                        .builder
                        .build_load(llvm_ty, param_val.into_pointer_value(), "param_load")
                        .expect("failed to load byval param");
                    lowering_ctx
                        .builder
                        .build_store(local_ptr, loaded)
                        .expect("failed to store param into local");
                }
                false => {
                    lowering_ctx
                        .builder
                        .build_store(local_ptr, *param_val)
                        .expect("failed to store param into local");
                }
            }
        }
    }
    for (bb_idx, bb_data) in body.basic_blocks.iter_enumerated() {
        let llvm_bb = lowering_ctx.bb_map.get(&bb_idx).unwrap();
        lowering_ctx.builder.position_at_end(*llvm_bb);
        // Reset per-block: a landingpad value (or SEH pad) from a previous
        // cleanup block must never leak into a block that isn't itself a fresh
        // cleanup landing pad.
        lowering_ctx.current_landingpad = None;
        lowering_ctx.current_seh_pad = None;
        if bb_data.is_cleanup {
            lowering_ctx.emit_landingpad()?;
        }
        for stmt in &bb_data.statements {
            lowering_ctx.lower_statement(stmt)?;
        }
        lowering_ctx.lower_terminator(&bb_data.terminator)?;
    }
    if let Some(di) = lowering_ctx.debug_ctx {
        di.finalize();
    }

    // Phase: native executable (`--emit=exec`). When this body is the crate's
    // entry `main`, also emit a C-ABI `main` symbol (the OS/libc entry point)
    // that calls the glyim `main` body (`__glyim_fn_{id}`) and returns 0. The
    // linker looks for `main`; without it the produced object cannot link into
    // a runnable binary. The wrapper uses the platform C calling convention so
    // libc can invoke it directly.
    if entry_main == Some(body.owner.local_id.to_raw()) {
        // Glyim `main` has the *real* return type of `fn main() -> R` (e.g. i32
        // for `fn main() -> i32`). The OS/libc `main` entry point must use the
        // platform C calling convention so the host (and the C shim) can read
        // its return value; glyim fns are `fastcc`, and calling a `fastcc` fn
        // directly from C on AArch64 silently loses the return (wrong reg).
        // So this `main` (ccc) calls the glyim `main` body and propagates its
        // value. Functions with `Never`/`Unit` return are mapped to `0`.
        let i32_type = context.i32_type();
        let main_fn_type = i32_type.fn_type(&[], false);
        let main_fn = module.add_function("main", main_fn_type, None);
        main_fn.set_call_conventions(0u32); // LLVMCCallConv::C
        let main_entry = context.append_basic_block(main_fn, "entry");
        let main_builder = context.create_builder();
        main_builder.position_at_end(main_entry);
        let callee = module.get_function(&fn_name).unwrap_or(function);
        // The glyim `main` body is normally `fastcc` (conv 8), but the `main`
        // wrapper is `ccc` (conv 0). Calling a `fastcc` fn from a `ccc` call
        // site on AArch64 silently drops the return value (different return
        // reg), so force the entry-main body to the C convention as well.
        callee.set_call_conventions(0u32); // LLVMCCallConv::C
        let call = main_builder
            .build_call(callee, &[], "call_glyim_main")
            .expect("failed to build main call");
        let retval = if matches!(ty_ctx.ty_kind(body.return_ty), TyKind::Never | TyKind::Unit) {
            BasicValueEnum::IntValue(i32_type.const_int(0, false))
        } else {
            call.try_as_basic_value()
                .basic()
                .unwrap_or_else(|| BasicValueEnum::IntValue(i32_type.const_int(0, false)))
        };
        main_builder
            .build_return(Some(&retval))
            .expect("failed to build main return");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glyim_core::primitives::{TargetAbi, TargetInfo};

    // Plan §19.1: personality selection must be a proper three-way choice
    // driven by target ABI — Windows targets use SEH, everyone else Itanium,
    // and a body with no cleanup blocks uses `None` (no unwinding).
    #[test]
    fn select_personality_windows_uses_seh() {
        let win = TargetInfo::from_triple("x86_64-pc-windows-msvc");
        assert_eq!(win.abi, TargetAbi::X86_64Windows);
        assert_eq!(
            select_personality(&win, true),
            Personality::Seh,
            "Windows target with cleanup must select SEH personality"
        );
    }

    #[test]
    fn select_personality_linux_uses_itanium() {
        let linux = TargetInfo::from_triple("x86_64-unknown-linux-gnu");
        assert_eq!(
            select_personality(&linux, true),
            Personality::Itanium,
            "Linux/Unix target with cleanup must select Itanium personality"
        );
    }

    #[test]
    fn select_personality_no_cleanup_is_none() {
        let linux = TargetInfo::from_triple("x86_64-unknown-linux-gnu");
        assert_eq!(
            select_personality(&linux, false),
            Personality::None,
            "a body with no cleanup blocks needs no personality function"
        );
    }

    #[test]
    fn seh_target_lowers_cleanup_landingpad_green() {
        use crate::LlvmBackend;
        // Plan §19.1 / Phase 7: a `-msvc` Windows target must lower a MIR body
        // with a cleanup block into real SEH funclet IR (`cleanuppad` +
        // `cleanupret`), not the Itanium `landingpad`/`resume` form. The
        // toolchain DOES export the funclet C-API (`LLVMBuildCleanupPad` /
        // `LLVMBuildCleanupRet` are present in `libLLVM.dylib`), so raw FFI via
        // `seh_ffi` produces genuine funclet IR. This test locks that in.
        use glyim_core::arena::IndexVec;
        use glyim_core::{Abi, CrateId, DefId, Interner, LocalDefId, Mutability, Safety};
        use glyim_mir::*;
        use glyim_type::{FnSig, TyCtxMut, TyKind};

        let mut ctx_mut = TyCtxMut::new(Interner::default());
        let i32_ty = ctx_mut.mk_ty(TyKind::Int(glyim_core::IntTy::I32));
        let fn_sig = FnSig {
            inputs: ctx_mut.intern_substitution(vec![]),
            output: i32_ty,
            c_variadic: false,
            unsafety: Safety::Safe,
            abi: Abi::Glyim,
        };
        let fn_ptr_ty = ctx_mut.mk_ty(TyKind::FnPtr(fn_sig.clone()));

        let mut locals: IndexVec<LocalIdx, LocalDecl> = IndexVec::new();
        locals.push(LocalDecl {
            ty: i32_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });
        locals.push(LocalDecl {
            ty: fn_ptr_ty,
            mutability: Mutability::Not,
            source_info: SourceInfo::new(glyim_span::Span::DUMMY),
        });

        let bb0 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Call {
                    func: Operand::Copy(Place::new(LocalIdx::from_raw(1))),
                    args: vec![],
                    destination: Place::new(LocalIdx::from_raw(0)),
                    target: Some(BasicBlockIdx::from_raw(1)),
                    cleanup: Some(BasicBlockIdx::from_raw(2)),
                },
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let bb1 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Return,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: false,
        };
        let bb2 = BasicBlockData {
            statements: vec![],
            terminator: Terminator {
                kind: TerminatorKind::Unreachable,
                source_info: SourceInfo::new(glyim_span::Span::DUMMY),
            },
            is_cleanup: true,
        };
        let body = Body {
            owner: DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(4)),
            basic_blocks: IndexVec::from_raw(vec![bb0, bb1, bb2]),
            locals,
            arg_count: 1,
            return_ty: i32_ty,
            span: glyim_span::Span::DUMMY,
            var_debug_info: vec![],
        };
        let ctx = ctx_mut.freeze();

        let backend = LlvmBackend::new()
            .with_target("x86_64-pc-windows-msvc")
            .with_ty_ctx(ctx);
        let inkwell_ctx = inkwell::context::Context::create();
        let result = backend.lower_body_to_module(&inkwell_ctx, &body);
        assert!(
            result.is_ok(),
            "SEH cleanup lowering must succeed (not reject with a toolchain error): {:?}",
            result.err()
        );
        let module = result.unwrap();
        let ir = module.print_to_string().to_string();
        assert!(
            ir.contains("cleanuppad") && ir.contains("cleanupret"),
            "SEH target must emit cleanuppad + cleanupret funclet IR: {}",
            ir
        );
        assert!(
            ir.contains("__CxxFrameHandler3"),
            "SEH target must declare the __CxxFrameHandler3 personality: {}",
            ir
        );
    }
}
