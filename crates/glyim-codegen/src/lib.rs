//! Crate root.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]
use glyim_core::primitives::{BinOp, UnOp};
use glyim_core::{FnDefId, IndexVec, TargetInfo};
use glyim_diag::{CompResult, GlyimDiagnostic};
use glyim_layout::{FieldsShape, LayoutComputer, SimpleLayoutComputer, TagEncoding, VariantsShape};
use glyim_mir::*;
use glyim_type::{ConstKind, TyKind};
use glyim_type::{FieldIdx, Substitution, Ty, TyCtx};
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;

/// CodegenBackend.
pub trait CodegenBackend {
    /// name.
    fn name(&self) -> &'static str;
    /// generate.
    fn generate(&self, bodies: &[Arc<Body>], output: &Path) -> CompResult<()>;
    /// generate_function.
    fn generate_function(&self, body: &Arc<Body>) -> CompResult<Vec<u8>>;
}

/// Layout provider for computing field offsets and sizes.
pub trait LayoutProvider {
    /// field_offset.
    fn field_offset(&self, ty: Ty, field_idx: FieldIdx) -> u64;
    /// size_of.
    fn size_of(&self, ty: Ty) -> u64;
    /// variant_type.
    fn variant_type(&self, enum_ty: Ty, variant_idx: VariantIdx) -> Ty;
    /// Byte offset of a downcasted enum variant's data payload from the start
    /// of the enum value. For enums using a *direct* discriminant tag, the tag
    /// occupies the leading bytes and variant data begins at `tag_size`; for
    /// single-variant types and niche-encoded enums the data overlaps the tag,
    /// so the offset is 0. Used by `Downcast` projections (plan §20.1).
    fn tag_offset(&self, ty: Ty) -> u64;
}

/// Shared handle to the pipeline-published `TyCtx`. Matches
/// `glyim_db::TyCtxHandle` structurally; declared locally so this crate does
/// not take a dependency on `glyim-db` (which would invert the crate layering
/// and risk a cycle, since the pipeline builds both). Any
/// `Arc<RwLock<Option<Arc<TyCtx>>>>` the pipeline publishes is assignment-
/// compatible.
pub type TyCtxHandle = std::sync::Arc<std::sync::RwLock<Option<std::sync::Arc<TyCtx>>>>;

/// Real layout provider using glyim-layout.
///
/// Holds a `TyCtxHandle` (a shared, pipeline-published `RwLock<Option<Arc<TyCtx>>>`)
/// rather than an owned `Arc<TyCtx>`. The handle lets the provider resolve the
/// *live* type context at each layout query, so a backend constructed before
/// the pipeline has run still sees the real (post-typeck) `TyCtx` by the time
/// `generate` is invoked. Without this, an early-constructed backend captured
/// an empty `TyCtxMut::new(..).freeze()` and every `layout_of` read past the
/// end of the empty arena (`type_arena::ty_kind` OOB).
struct GlyimLayoutProvider {
    ty_ctx: TyCtxHandle,
    target: TargetInfo,
}

impl GlyimLayoutProvider {
    /// Resolve the current `Arc<TyCtx>` from the handle, or `None` if the
    /// pipeline has not yet published one.
    fn ctx(&self) -> Option<Arc<TyCtx>> {
        self.ty_ctx.read().ok().and_then(|g| g.clone())
    }
}

impl LayoutProvider for GlyimLayoutProvider {
    fn field_offset(&self, ty: Ty, field_idx: FieldIdx) -> u64 {
        let Some(ctx) = self.ctx() else {
            tracing::warn!("Layout computation failed: TyCtx not published");
            return 0;
        };
        let computer = SimpleLayoutComputer::new(ctx.as_ref(), self.target.clone());
        if let Ok(layout) = computer.layout_of(ty) {
            match layout.fields {
                FieldsShape::Arbitrary { ref offsets } => {
                    if field_idx.index() < offsets.len() {
                        offsets[field_idx].0
                    } else {
                        0
                    }
                }
                FieldsShape::Primitive => 0,
                FieldsShape::Array { stride, count: _ } => (field_idx.to_raw() as u64) * stride.0,
            }
        } else {
            tracing::warn!("Layout computation failed for field offset");
            0
        }
    }

    fn size_of(&self, ty: Ty) -> u64 {
        let Some(ctx) = self.ctx() else {
            tracing::warn!("Layout computation failed: TyCtx not published");
            return 0;
        };
        let computer = SimpleLayoutComputer::new(ctx.as_ref(), self.target.clone());
        if let Ok(layout) = computer.layout_of(ty) {
            layout.size.0
        } else {
            tracing::warn!("Layout computation failed for size");
            0
        }
    }

    fn variant_type(&self, enum_ty: Ty, variant_idx: VariantIdx) -> Ty {
        use glyim_type::TyKind;
        let Some(ctx) = self.ctx() else { return Ty::ERROR };
        match ctx.ty_kind(enum_ty) {
            TyKind::Adt(adt_id, _substs) => ctx.variant_type(*adt_id, variant_idx.to_raw()),
            _ => Ty::ERROR,
        }
    }

    fn tag_offset(&self, ty: Ty) -> u64 {
        let Some(ctx) = self.ctx() else {
            tracing::warn!("Layout computation failed: TyCtx not published");
            return 0;
        };
        let computer = SimpleLayoutComputer::new(ctx.as_ref(), self.target.clone());
        if let Ok(layout) = computer.layout_of(ty) {
            if let VariantsShape::Multiple {
                tag_encoding: TagEncoding::Direct,
                tag_size,
                ..
            } = &layout.variants
            {
                tag_size.0
            } else {
                0
            }
        } else {
            0
        }
    }
}

/// Optimization level for the bytecode backend.
///
/// The default is `O0`, which emits exactly what the lowering produces (no
/// transforms) so existing byte-exact tests remain stable. Levels `O1` and
/// above additionally run the peephole pass (see [`BytecodeBackend::peephole`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptLevel {
    /// No optimization: emit lowering output verbatim.
    #[default]
    O0,
    /// Peephole optimizations: constant folding and trivial dead-code removal.
    O1,
    /// Same passes as `O1` (higher levels reserved for future pipeline stages).
    O2,
    /// Same passes as `O1` (highest reserved level).
    O3,
}

/// BytecodeBackend.
pub struct BytecodeBackend {
    string_table: RefCell<Vec<String>>,
    fn_table: RefCell<Vec<(FnDefId, Substitution)>>,
    layout_provider: Box<dyn LayoutProvider>,
    /// Shared handle to the pipeline-published `TyCtx`. Reading through the
    /// handle (rather than holding an owned `Arc<TyCtx>`) lets a backend
    /// constructed *before* the pipeline runs still see the real, post-typeck
    /// context by the time `generate_function` executes. See
    /// [`GlyimLayoutProvider`] for the failure mode this avoids.
    ty_ctx: Option<TyCtxHandle>,
    /// Optimization level (default `O0`). Higher levels run the peephole pass.
    opt_level: OptLevel,
    /// Per emitted `OP_LOAD_CONST`, whether the pushed value is an integer
    /// (Int/Uint/Bool/Char). Recorded in emission order so the peephole pass
    /// can fold integer binary ops without mis-folding float/string constants.
    const_is_int: RefCell<Vec<bool>>,
}

impl BytecodeBackend {
    /// Construct with a shared pipeline `TyCtxHandle`. Preferred entry point:
    /// the backend resolves the live `TyCtx` at each call, so it works
    /// regardless of whether the pipeline has published it yet.
    pub fn with_ty_ctx_handle(handle: TyCtxHandle, target: TargetInfo) -> Self {
        Self {
            string_table: RefCell::new(Vec::new()),
            fn_table: RefCell::new(Vec::new()),
            layout_provider: Box::new(GlyimLayoutProvider {
                ty_ctx: handle.clone(),
                target: target.clone(),
            }),
            ty_ctx: Some(handle),
            opt_level: OptLevel::O0,
            const_is_int: RefCell::new(Vec::new()),
        }
    }

    /// Construct with an owned `Arc<TyCtx>` (test convenience). The backend
    /// installs the context into a private handle internally, so `ty_ctx`
    /// reads behave identically to the `with_ty_ctx_handle` path.
    pub fn with_ty_ctx(ctx: Arc<TyCtx>, target: TargetInfo) -> Self {
        let handle: TyCtxHandle = std::sync::Arc::new(std::sync::RwLock::new(Some(ctx)));
        Self::with_ty_ctx_handle(handle, target)
    }

    /// Resolve the current `Arc<TyCtx>` from the handle, or `None` if the
    /// pipeline has not published it yet.
    fn resolve_ty_ctx(&self) -> Option<Arc<TyCtx>> {
        self.ty_ctx
            .as_ref()
            .and_then(|h| h.read().ok().and_then(|g| g.clone()))
    }

    /// with_opt_level.
    pub fn with_opt_level(mut self, level: OptLevel) -> Self {
        self.opt_level = level;
        self
    }

    /// with_layout_provider.
    pub fn with_layout_provider(mut self, provider: Box<dyn LayoutProvider>) -> Self {
        self.layout_provider = provider;
        self
    }

    fn emit_place_address(
        &self,
        bc: &mut Vec<u8>,
        place: &Place,
        local_tys: &IndexVec<LocalIdx, LocalDecl>,
    ) -> CompResult<()> {
        if place.projection.is_empty() {
            bc.push(OP_LOAD_LOCAL_ADDR);
            bc.extend_from_slice(&place.local.to_raw().to_le_bytes());
            return Ok(());
        }

        bc.push(OP_LOAD_LOCAL_ADDR);
        bc.extend_from_slice(&place.local.to_raw().to_le_bytes());

        let local_idx = place.local.to_raw() as usize;
        if local_idx >= local_tys.len() {
            panic!(
                "local index out of bounds: {} (len={})",
                local_idx,
                local_tys.len()
            );
        }
        let mut current_ty = local_tys[place.local].ty;

        for proj in place.projection.iter() {
            match proj {
                ProjectionElem::Deref => {
                    bc.push(OP_DEREF);
                    current_ty = if let Some(ctx) = self.resolve_ty_ctx() {
                        match ctx.ty_kind(current_ty) {
                            glyim_type::TyKind::Ref(_, inner, _)
                            | glyim_type::TyKind::RawPtr(inner, _) => *inner,
                            _ => Ty::ERROR,
                        }
                    } else {
                        Ty::ERROR
                    };
                }
                ProjectionElem::Field(idx) => {
                    let offset = self.layout_provider.field_offset(current_ty, *idx);
                    bc.push(OP_LOAD_CONST);
                    bc.extend_from_slice(&(offset as i64).to_le_bytes());
                    bc.push(OP_ADD);
                }
                ProjectionElem::Index(local) => {
                    let elem_size = self.layout_provider.size_of(current_ty);
                    if elem_size == 0 {
                        // ZST array/slice element: every element aliases the same
                        // base address, so the byte offset is always 0. This is
                        // *deliberately* correct, not an accident: adding 0 to the
                        // base pointer is the right address for any index.
                        //
                        // However, indexing must still *bounds-check* and panic on an
                        // out-of-range access, matching non-ZST semantics — a ZST
                        // index must not silently accept `a[999]` on `[Z; 3]`.
                        // Emit `index < len` and trap when it fails (the trap
                        // sentinel block is the same one used by `OP_JUMP_IF` /
                        // `Assert` for an unreachable/panic target).
                        bc.push(OP_LOAD_LOCAL);
                        bc.extend_from_slice(&local.to_raw().to_le_bytes());
                        let kind_owned: Option<TyKind> = self
                            .resolve_ty_ctx()
                            .map(|ctx| ctx.ty_kind(current_ty).clone());
                        match kind_owned.as_ref() {
                            Some(TyKind::Array(_, len_const)) => {
                                let n = match &len_const.kind {
                                    ConstKind::Uint(n) => *n as i64,
                                    ConstKind::Int(n) => *n as i64,
                                    _ => 0,
                                };
                                bc.push(OP_LOAD_CONST);
                                bc.extend_from_slice(&n.to_le_bytes());
                            }
                            Some(TyKind::Slice(_)) => {
                                bc.push(OP_LEN);
                                bc.extend_from_slice(&place.local.to_raw().to_le_bytes());
                            }
                            _ => {
                                bc.push(OP_LOAD_CONST);
                                bc.extend_from_slice(&0i64.to_le_bytes());
                            }
                        }
                        bc.push(OP_LT);
                        bc.push(OP_ASSERT);
                        bc.push(1u8); // expected: index < len must hold
                        bc.extend_from_slice(
                            &BasicBlockIdx::from_raw(u32::MAX).to_raw().to_le_bytes(),
                        );
                        // Address is base + 0 for a ZST element.
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&0i64.to_le_bytes());
                        bc.push(OP_ADD);
                    } else {
                        bc.push(OP_LOAD_LOCAL);
                        bc.extend_from_slice(&local.to_raw().to_le_bytes());
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(elem_size as i64).to_le_bytes());
                        bc.push(OP_MUL);
                        bc.push(OP_ADD);
                    }
                    current_ty = if let Some(ctx) = self.resolve_ty_ctx() {
                        match ctx.ty_kind(current_ty) {
                            glyim_type::TyKind::Array(elem, _)
                            | glyim_type::TyKind::Slice(elem) => *elem,
                            _ => Ty::ERROR,
                        }
                    } else {
                        Ty::ERROR
                    };
                }
                ProjectionElem::Downcast(_) => {
                    // Downcast to an enum variant. For a *direct*-tagged enum
                    // the discriminant tag occupies the leading `tag_size`
                    // bytes, so the variant's data payload begins at that
                    // offset; subsequent `Field` projections must be relative
                    // to the data region. Niche-encoded enums and
                    // single-variant types overlap the tag, so the offset is 0
                    // (plan §20.1).
                    let tag_off = self.layout_provider.tag_offset(current_ty);
                    if tag_off != 0 {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(tag_off as i64).to_le_bytes());
                        bc.push(OP_ADD);
                    }
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    min_length: _,
                    from_end,
                } => {
                    let elem_size = self.layout_provider.size_of(current_ty);
                    let index_val = if *from_end {
                        // For arrays the length is known at compile time, so the
                        // from-end index can be resolved to a constant offset.
                        if let Some(ctx) = self.resolve_ty_ctx() {
                            if let TyKind::Array(_, const_val) = ctx.ty_kind(current_ty) {
                                let n = match &const_val.kind {
                                    ConstKind::Uint(n) => *n as u64,
                                    ConstKind::Int(n) => *n as u64,
                                    _ => 0,
                                };
                                n.saturating_sub(*offset)
                            } else {
                                // For slices the length is only known at runtime
                                // (the `len` field of the fat pointer). Emit a
                                // runtime `actual_offset = runtime_len - offset`
                                // scaled by the element size, matching the
                                // established Index(local) accumulator idiom:
                                // the base address is already on the stack, we
                                // push the slice value, read its len, subtract
                                // the offset, scale by elem_size, and add to the
                                // base. (No bytecode VM exists in the repo, but
                                // golden-pattern tests assert the emitted
                                // opcodes, which is this backend's verification
                                // convention.)
                                let slice_local = place.local;
                                bc.push(OP_LOAD_LOCAL);
                                bc.extend_from_slice(&slice_local.to_raw().to_le_bytes());
                                bc.push(OP_LEN);
                                bc.push(OP_LOAD_CONST);
                                bc.extend_from_slice(&(*offset as i64).to_le_bytes());
                                bc.push(OP_SUB);
                                bc.push(OP_LOAD_CONST);
                                bc.extend_from_slice(&(elem_size as i64).to_le_bytes());
                                bc.push(OP_MUL);
                                bc.push(OP_ADD);
                                current_ty = if let Some(ctx) = self.resolve_ty_ctx() {
                                    match ctx.ty_kind(current_ty) {
                                        TyKind::Slice(elem) => *elem,
                                        _ => Ty::ERROR,
                                    }
                                } else {
                                    Ty::ERROR
                                };
                                continue;
                            }
                        } else {
                            *offset
                        }
                    } else {
                        *offset
                    };
                    let byte_offset = index_val * elem_size;
                    bc.push(OP_LOAD_CONST);
                    bc.extend_from_slice(&(byte_offset as i64).to_le_bytes());
                    bc.push(OP_ADD);
                    current_ty = Ty::ERROR;
                }
                ProjectionElem::Subslice {
                    from,
                    to: _,
                    from_end: _,
                } => {
                    let elem_size = self.layout_provider.size_of(current_ty);
                    let byte_offset = *from * elem_size;
                    bc.push(OP_LOAD_CONST);
                    bc.extend_from_slice(&(byte_offset as i64).to_le_bytes());
                    bc.push(OP_ADD);
                }
            }
        }
        Ok(())
    }

    fn intern_string(&self, s: &str) -> u32 {
        let mut table = self.string_table.borrow_mut();
        for (i, existing) in table.iter().enumerate() {
            if existing == s {
                return i as u32;
            }
        }
        table.push(s.to_string());
        (table.len() - 1) as u32
    }

    fn intern_fn(&self, def_id: FnDefId, substs: Substitution) -> u32 {
        let mut table = self.fn_table.borrow_mut();
        for (i, (id, s)) in table.iter().enumerate() {
            if *id == def_id && *s == substs {
                return i as u32;
            }
        }
        table.push((def_id, substs));
        (table.len() - 1) as u32
    }
}

pub(crate) const OP_LOAD_CONST: u8 = 0x01;
pub(crate) const OP_ADD: u8 = 0x02;
pub(crate) const OP_SUB: u8 = 0x03;
pub(crate) const OP_MUL: u8 = 0x04;
pub(crate) const OP_DIV: u8 = 0x05;
pub(crate) const OP_REM: u8 = 0x06;
pub(crate) const OP_EQ: u8 = 0x07;
pub(crate) const OP_NE: u8 = 0x08;
pub(crate) const OP_LT: u8 = 0x09;
pub(crate) const OP_GT: u8 = 0x0A;
pub(crate) const OP_LE: u8 = 0x0B;
pub(crate) const OP_GE: u8 = 0x0C;
pub(crate) const OP_AND: u8 = 0x0D;
pub(crate) const OP_OR: u8 = 0x0E;
pub(crate) const OP_NOT: u8 = 0x0F;
pub(crate) const OP_NEG: u8 = 0x10;
pub(crate) const OP_BITAND: u8 = 0x11;
pub(crate) const OP_BITOR: u8 = 0x12;
pub(crate) const OP_BITXOR: u8 = 0x13;
pub(crate) const OP_SHL: u8 = 0x14;
pub(crate) const OP_SHR: u8 = 0x15;
pub(crate) const OP_LOAD_LOCAL: u8 = 0x16;
pub(crate) const OP_STORE_LOCAL: u8 = 0x17;
pub(crate) const OP_RETURN: u8 = 0x18;
pub(crate) const OP_JUMP_IF: u8 = 0x19;
pub(crate) const OP_JUMP: u8 = 0x1A;
pub(crate) const OP_CALL: u8 = 0x1B;
pub(crate) const OP_CAST: u8 = 0x1C;
pub(crate) const OP_AGGREGATE: u8 = 0x1D;
pub(crate) const OP_DISCRIMINANT: u8 = 0x1E;
pub(crate) const OP_LEN: u8 = 0x1F;
pub(crate) const OP_SWITCH_INT: u8 = 0x20;
pub(crate) const OP_ASSERT: u8 = 0x21;
pub(crate) const OP_CALL_INDIRECT: u8 = 0x22;
pub(crate) const OP_LOAD_LOCAL_ADDR: u8 = 0x29;
pub(crate) const OP_STORE_FIELD: u8 = 0x2A;
pub(crate) const OP_DEREF: u8 = 0x2B;
pub(crate) const OP_DROP: u8 = 0x2C;
pub(crate) const OP_REPEAT: u8 = 0x2D;
pub(crate) const OP_TRAP: u8 = 0xFF;

/// Fold two integer constants through an integer binary opcode, returning the
/// wrapped result if the opcode is a foldable integer arithmetic/logic op.
///
/// Wrapping arithmetic matches the bytecode runtime's `i64` semantics, so the
/// folded constant is exactly what a runtime `OP_BINOP` would have produced.
/// Division by zero is not foldable (returns `None`) because the runtime traps
/// on it rather than returning a wrapped value.
fn fold_int_binop(op: u8, a: i64, b: i64) -> Option<i64> {
    use std::ops::{BitAnd, BitOr, BitXor, Rem};
    let r = match op {
        OP_ADD => a.wrapping_add(b),
        OP_SUB => a.wrapping_sub(b),
        OP_MUL => a.wrapping_mul(b),
        OP_DIV => a.checked_div(b)?,
        OP_REM => a.rem(b),
        OP_EQ => (a == b) as i64,
        OP_NE => (a != b) as i64,
        OP_LT => (a < b) as i64,
        OP_GT => (a > b) as i64,
        OP_LE => (a <= b) as i64,
        OP_GE => (a >= b) as i64,
        OP_AND => (a != 0 && b != 0) as i64,
        OP_OR => (a != 0 || b != 0) as i64,
        OP_BITAND => a.bitand(b),
        OP_BITOR => a.bitor(b),
        OP_BITXOR => a.bitxor(b),
        OP_SHL => a.wrapping_shl(b as u32),
        OP_SHR => a.wrapping_shr(b as u32),
        _ => return None,
    };
    Some(r)
}

impl CodegenBackend for BytecodeBackend {
    fn name(&self) -> &'static str {
        "bytecode"
    }

    fn generate(&self, bodies: &[Arc<Body>], output: &Path) -> CompResult<()> {
        let mut module_bytes: Vec<u8> = Vec::new();
        for body in bodies {
            let fn_bytes = self.generate_function(body)?;
            module_bytes.extend_from_slice(&fn_bytes);
        }
        std::fs::write(output, &module_bytes).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "failed to write bytecode output to {}: {}",
                output.display(),
                e
            ))]
        })?;
        Ok(())
    }

    fn generate_function(&self, body: &Arc<Body>) -> CompResult<Vec<u8>> {
        let mut bc = Vec::new();
        // Reset the per-constant int-tracking record; it is rebuilt during this
        // function's emission and consumed by the peephole pass below.
        self.const_is_int.borrow_mut().clear();
        for block in body.basic_blocks.iter() {
            for stmt in &block.statements {
                self.emit_statement(&mut bc, &stmt.kind, &body.locals)?;
            }
            self.emit_terminator(&mut bc, &block.terminator.kind, &body.locals)?;
        }
        if self.opt_level != OptLevel::O0 {
            self.peephole(&mut bc);
        }
        Ok(bc)
    }
}

impl BytecodeBackend {
    /// Run the peephole optimization pass over the emitted bytecode stream.
    ///
    /// Currently implements two always-semantics-preserving rules:
    /// 1. **Integer constant folding**: `OP_LOAD_CONST a; OP_LOAD_CONST b; OP_BINOP`
    ///    where `a`/`b` are integer-typed constants and `OP_BINOP` is one of the
    ///    arithmetic/logic ops, is collapsed to `OP_LOAD_CONST (a OP b)`.
    /// Run the peephole optimization pass over the emitted bytecode stream.
    ///
    /// Implemented on a width-aware decoded form (see [`decode_bytecode`]) so it
    /// never misreads operand bytes as opcodes.
    ///
    /// Two always-semantics-preserving rules:
    /// 1. **Integer constant folding**: `LOAD_CONST a; LOAD_CONST b; BINOP` where
    ///    `a`/`b` are integer-typed constants is collapsed to `LOAD_CONST (a OP b)`.
    /// 2. **Double-negation cancellation**: `OP_NEG; OP_NEG` is removed (identity
    ///    for all numeric types).
    ///
    /// The `const_is_int` record (populated during emission) ensures float and
    /// string constants are never folded, so this pass is type-safe.
    fn peephole(&self, bc: &mut Vec<u8>) {
        let instrs = decode_bytecode(bc);
        let ints = self.const_is_int.borrow();
        let mut int_iter = ints.iter().copied();
        let mut out: Vec<Instr> = Vec::with_capacity(instrs.len());
        let mut k = 0usize;
        while k < instrs.len() {
            let op = instrs[k].op;
            if op == OP_LOAD_CONST {
                let v = i64::from_le_bytes(instrs[k].operand[..8].try_into().unwrap());
                let is_int = int_iter.next().unwrap_or(false);
                // LOAD_CONST(int a) ; LOAD_CONST(int b) ; foldable BINOP
                if is_int
                    && k + 2 < instrs.len()
                    && instrs[k + 1].op == OP_LOAD_CONST
                    && int_iter.clone().next() == Some(true)
                    && is_foldable_binop(instrs[k + 2].op)
                {
                    let v2 = i64::from_le_bytes(instrs[k + 1].operand[..8].try_into().unwrap());
                    if let Some(folded) = fold_int_binop(instrs[k + 2].op, v, v2) {
                        int_iter.next(); // consume the second const's int flag
                        out.push(Instr {
                            op: OP_LOAD_CONST,
                            operand: folded.to_le_bytes().to_vec(),
                        });
                        k += 3;
                        continue;
                    }
                }
                out.push(instrs[k].clone());
                k += 1;
            } else if op == OP_NEG && k + 1 < instrs.len() && instrs[k + 1].op == OP_NEG {
                // Double negation cancels (identity for all numeric types).
                k += 2;
            } else {
                out.push(instrs[k].clone());
                k += 1;
            }
        }
        *bc = encode_bytecode(&out);
    }
}

/// A single decoded bytecode instruction: an opcode plus its raw operand bytes.
#[derive(Debug, Clone)]
struct Instr {
    op: u8,
    operand: Vec<u8>,
}

/// Decode a raw bytecode stream into a sequence of [`Instr`], using the
/// per-opcode operand-width table (including variable-length `CALL`/`SWITCH_INT`
/// /`AGGREGATE` whose inner instructions are decoded recursively). This is the
/// inverse of [`encode_bytecode`].
fn decode_bytecode(bc: &[u8]) -> Vec<Instr> {
    let mut instrs = Vec::new();
    let mut i = 0usize;
    while i < bc.len() {
        let op = bc[i];
        i += 1;
        let (operand, consumed) = decode_operand(bc, i, op);
        i += consumed;
        instrs.push(Instr { op, operand });
    }
    instrs
}

/// Decode the operand bytes for `op` starting at `i`, returning the operand and
/// the number of bytes consumed.
fn decode_operand(bc: &[u8], i: usize, op: u8) -> (Vec<u8>, usize) {
    let take = |n: usize| {
        let end = (i + n).min(bc.len());
        (bc[i..end].to_vec(), end - i)
    };
    match op {
        OP_LOAD_CONST => take(8),
        OP_LOAD_LOCAL | OP_STORE_LOCAL | OP_JUMP | OP_JUMP_IF | OP_LEN | OP_DISCRIMINANT => take(4),
        OP_CAST => take(1),
        OP_ASSERT => {
            // 1-byte expected + 4-byte target.
            let (a, c1) = take(1);
            let (b, c2) = take(4);
            let mut v = a;
            v.extend(b);
            (v, c1 + c2)
        }
        OP_AGGREGATE => {
            // 4-byte count followed by that many inner instructions.
            let (cnt_bytes, _) = take(4);
            let mut v = cnt_bytes.clone();
            let mut consumed = 4;
            if cnt_bytes.len() == 4 {
                let count = u32::from_le_bytes(cnt_bytes.try_into().unwrap()) as usize;
                for _ in 0..count {
                    if i + consumed >= bc.len() {
                        break;
                    }
                    let inner = decode_bytecode(&bc[i + consumed..]);
                    if inner.is_empty() {
                        break;
                    }
                    let used = inner_encoded_len(&inner[0]);
                    v.extend_from_slice(&bc[i + consumed..i + consumed + used]);
                    consumed += used;
                }
            }
            (v, consumed)
        }
        OP_CALL | OP_CALL_INDIRECT => {
            // 4-byte argc, then argc inner instructions, then 4-byte dest + 4 target.
            let (cnt_bytes, _) = take(4);
            let mut v = cnt_bytes.clone();
            let mut consumed = 4;
            if cnt_bytes.len() == 4 {
                let count = u32::from_le_bytes(cnt_bytes.try_into().unwrap()) as usize;
                for _ in 0..count {
                    if i + consumed >= bc.len() {
                        break;
                    }
                    let inner = decode_bytecode(&bc[i + consumed..]);
                    if inner.is_empty() {
                        break;
                    }
                    let used = inner_encoded_len(&inner[0]);
                    v.extend_from_slice(&bc[i + consumed..i + consumed + used]);
                    consumed += used;
                }
                let (rest, c) = take(8); // dest local + target
                v.extend(rest);
                consumed += c;
            }
            (v, consumed)
        }
        OP_SWITCH_INT => {
            // 4-byte count, then count*(8-byte value + 4-byte target), then 4 otherwise.
            let (cnt_bytes, _) = take(4);
            let mut v = cnt_bytes.clone();
            let mut consumed = 4;
            if cnt_bytes.len() == 4 {
                let count = u32::from_le_bytes(cnt_bytes.try_into().unwrap()) as usize;
                for _ in 0..count {
                    let (pair, c) = take(12);
                    v.extend(pair);
                    consumed += c;
                }
                let (otherwise, c) = take(4);
                v.extend(otherwise);
                consumed += c;
            }
            (v, consumed)
        }
        _ => (Vec::new(), 0),
    }
}

/// Encoded length of a single instruction (opcode + operand) so the variable
/// length decoders can advance by exactly one inner instruction.
fn inner_encoded_len(instr: &Instr) -> usize {
    1 + instr.operand.len()
}

/// Encode a sequence of [`Instr`] back into a raw bytecode stream.
fn encode_bytecode(instrs: &[Instr]) -> Vec<u8> {
    let mut bc = Vec::new();
    for instr in instrs {
        bc.push(instr.op);
        bc.extend_from_slice(&instr.operand);
    }
    bc
}

/// Whether an opcode is an integer binary op that can be constant-folded.
fn is_foldable_binop(op: u8) -> bool {
    matches!(
        op,
        OP_ADD
            | OP_SUB
            | OP_MUL
            | OP_DIV
            | OP_REM
            | OP_EQ
            | OP_NE
            | OP_LT
            | OP_GT
            | OP_LE
            | OP_GE
            | OP_AND
            | OP_OR
            | OP_BITAND
            | OP_BITOR
            | OP_BITXOR
            | OP_SHL
            | OP_SHR
    )
}

impl BytecodeBackend {
    fn emit_statement(
        &self,
        bc: &mut Vec<u8>,
        kind: &StatementKind,
        local_tys: &IndexVec<LocalIdx, LocalDecl>,
    ) -> CompResult<()> {
        match kind {
            StatementKind::Assign(place, rvalue) => {
                self.emit_rvalue(bc, rvalue, local_tys)?;
                if place.projection.is_empty() {
                    bc.push(OP_STORE_LOCAL);
                    bc.extend_from_slice(&place.local.to_raw().to_le_bytes());
                } else {
                    self.emit_place_address(bc, place, local_tys)?;
                    bc.push(OP_STORE_FIELD);
                }
                Ok(())
            }
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) | StatementKind::Nop => {
                Ok(())
            }
        }
    }

    fn emit_rvalue(
        &self,
        bc: &mut Vec<u8>,
        rvalue: &Rvalue,
        local_tys: &IndexVec<LocalIdx, LocalDecl>,
    ) -> CompResult<()> {
        match rvalue {
            Rvalue::Use(operand) => self.emit_operand(bc, operand, local_tys),
            Rvalue::BinaryOp(op, operands_box) => {
                let (left, right) = operands_box.as_ref();
                self.emit_operand(bc, left, local_tys)?;
                self.emit_operand(bc, right, local_tys)?;
                let opcode = match op {
                    BinOp::Add => OP_ADD,
                    BinOp::Sub => OP_SUB,
                    BinOp::Mul => OP_MUL,
                    BinOp::Div => OP_DIV,
                    BinOp::Rem => OP_REM,
                    BinOp::Eq => OP_EQ,
                    BinOp::Ne => OP_NE,
                    BinOp::Lt => OP_LT,
                    BinOp::Gt => OP_GT,
                    BinOp::LtEq => OP_LE,
                    BinOp::GtEq => OP_GE,
                    BinOp::And => OP_AND,
                    BinOp::Or => OP_OR,
                    BinOp::BitAnd => OP_BITAND,
                    BinOp::BitOr => OP_BITOR,
                    BinOp::BitXor => OP_BITXOR,
                    BinOp::Shl => OP_SHL,
                    BinOp::Shr => OP_SHR,
                };
                bc.push(opcode);
                Ok(())
            }
            Rvalue::UnaryOp(op, operand) => {
                self.emit_operand(bc, operand, local_tys)?;
                bc.push(match op {
                    UnOp::Not => OP_NOT,
                    UnOp::Neg => OP_NEG,
                    UnOp::Deref => OP_DEREF,
                });
                Ok(())
            }
            Rvalue::Ref(place, _borrow_kind) => {
                // `&T` and `&mut T` both lower to the same address-taking
                // opcode: in this address-only bytecode model the reference is
                // just a pointer, and mutability is enforced by the
                // borrow-checker (glyim-borrowck), not the codegen backend. The
                // borrow kind is bound (not silently discarded) to document
                // that the distinction lives at the borrow-check layer (plan
                // §20.2).
                self.emit_place_address(bc, place, local_tys)
            }
            Rvalue::Aggregate(_, operands) => {
                bc.push(OP_AGGREGATE);
                bc.extend_from_slice(&(operands.len() as u32).to_le_bytes());
                for o in operands {
                    self.emit_operand(bc, o, local_tys)?;
                }
                Ok(())
            }
            Rvalue::Discriminant(place) => {
                self.emit_operand(bc, &Operand::Copy(place.clone()), local_tys)?;
                bc.push(OP_DISCRIMINANT);
                Ok(())
            }
            Rvalue::Len(place) => {
                self.emit_operand(bc, &Operand::Copy(place.clone()), local_tys)?;
                bc.push(OP_LEN);
                Ok(())
            }
            Rvalue::Cast(kind, operand, _) => {
                self.emit_operand(bc, operand, local_tys)?;
                bc.push(OP_CAST);
                bc.push(match kind {
                    CastKind::IntToInt => 0,
                    CastKind::FloatToInt => 1,
                    CastKind::IntToFloat => 2,
                    CastKind::PtrToPtr => 3,
                    CastKind::FnPtrToPtr => 4,
                    CastKind::PtrToInt => 5,
                    CastKind::IntToPtr => 6,
                    CastKind::FloatToFloat => 7,
                });
                Ok(())
            }
            Rvalue::Repeat(operand, mir_const) => {
                bc.push(OP_REPEAT);
                self.emit_operand(bc, operand, local_tys)?;
                self.emit_operand(bc, &Operand::Constant(mir_const.clone()), local_tys)
            }
        }
    }

    fn emit_operand(
        &self,
        bc: &mut Vec<u8>,
        operand: &Operand,
        local_tys: &IndexVec<LocalIdx, LocalDecl>,
    ) -> CompResult<()> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                if place.projection.is_empty() {
                    bc.push(OP_LOAD_LOCAL);
                    bc.extend_from_slice(&place.local.to_raw().to_le_bytes());
                    Ok(())
                } else {
                    self.emit_place_address(bc, place, local_tys)?;
                    bc.push(OP_DEREF);
                    Ok(())
                }
            }
            Operand::Constant(mir_const) => {
                match &mir_const.kind {
                    MirConstKind::Int(v) => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(*v as i64).to_le_bytes());
                        self.const_is_int.borrow_mut().push(true);
                    }
                    MirConstKind::Uint(v) => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(*v as i64).to_le_bytes());
                        self.const_is_int.borrow_mut().push(true);
                    }
                    MirConstKind::Bool(b) => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(if *b { 1i64 } else { 0i64 }).to_le_bytes());
                        self.const_is_int.borrow_mut().push(true);
                    }
                    MirConstKind::Char(c) => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(*c as i64).to_le_bytes());
                        self.const_is_int.borrow_mut().push(true);
                    }
                    MirConstKind::FloatBits(b) => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&b.to_le_bytes());
                        self.const_is_int.borrow_mut().push(false);
                    }
                    MirConstKind::String(_name) => {
                        // Resolve the interner through the live TyCtx. Hold the
                        // `Arc` in a local so the `&str` we copy out of it
                        // outlives the closure (a bare `.map(|ctx| ...)` would
                        // borrow from a temporary owned by the map closure).
                        let str_content = match self.resolve_ty_ctx() {
                            Some(ctx) => ctx.name_str(*_name).to_string(),
                            None => "string_payload".to_string(),
                        };
                        let idx = self.intern_string(&str_content);
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(idx as i64).to_le_bytes());
                        self.const_is_int.borrow_mut().push(false);
                    }
                    MirConstKind::Fn(def_id, substs) => {
                        bc.push(OP_LOAD_CONST);
                        let idx = self.intern_fn(*def_id, *substs);
                        bc.extend_from_slice(&(idx as i64).to_le_bytes());
                        self.const_is_int.borrow_mut().push(false);
                    }
                    MirConstKind::ConstRef(def_id, _) => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&(def_id.to_raw() as i64).to_le_bytes());
                        self.const_is_int.borrow_mut().push(false);
                    }
                    MirConstKind::Aggregate(elems) => {
                        // Emit each element constant in order; the bytecode
                        // runtime reconstructs the aggregate from the pushed
                        // values (plan §15.3).
                        for e in elems {
                            self.emit_operand(bc, &Operand::Constant(e.clone()), local_tys)?;
                        }
                    }
                    MirConstKind::Unit | MirConstKind::Error => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&0i64.to_le_bytes());
                        self.const_is_int.borrow_mut().push(true);
                    }
                    // Devirtualized by monomorphization before codegen; should
                    // not survive to this point. Treated as a unit placeholder.
                    MirConstKind::VirtualMethod { .. } => {
                        bc.push(OP_LOAD_CONST);
                        bc.extend_from_slice(&0i64.to_le_bytes());
                        self.const_is_int.borrow_mut().push(true);
                    }
                }
                Ok(())
            }
        }
    }

    fn emit_terminator(
        &self,
        bc: &mut Vec<u8>,
        kind: &TerminatorKind,
        local_tys: &IndexVec<LocalIdx, LocalDecl>,
    ) -> CompResult<()> {
        match kind {
            TerminatorKind::Return => {
                bc.push(OP_RETURN);
                Ok(())
            }
            TerminatorKind::SwitchInt {
                discr,
                switch_ty,
                targets,
            } => {
                if *switch_ty == Ty::BOOL {
                    self.emit_operand(bc, discr, local_tys)?;
                    let false_target = targets
                        .iter()
                        .next()
                        .map(|(_, t)| t)
                        .unwrap_or_else(|| targets.otherwise());
                    let true_target = targets.otherwise();
                    bc.push(OP_JUMP_IF);
                    bc.extend_from_slice(&true_target.to_raw().to_le_bytes());
                    bc.push(OP_JUMP);
                    bc.extend_from_slice(&false_target.to_raw().to_le_bytes());
                } else {
                    self.emit_operand(bc, discr, local_tys)?;
                    bc.push(OP_SWITCH_INT);
                    let count = targets.iter().count() as u32;
                    bc.extend_from_slice(&count.to_le_bytes());
                    for (v, t) in targets.iter() {
                        bc.extend_from_slice(&v.to_le_bytes());
                        bc.extend_from_slice(&t.to_raw().to_le_bytes());
                    }
                    bc.extend_from_slice(&targets.otherwise().to_raw().to_le_bytes());
                }
                Ok(())
            }
            TerminatorKind::Goto { target } => {
                bc.push(OP_JUMP);
                bc.extend_from_slice(&target.to_raw().to_le_bytes());
                Ok(())
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
                ..
            } => {
                let is_indirect = matches!(func, Operand::Copy(_) | Operand::Move(_));
                self.emit_operand(bc, func, local_tys)?;
                for arg in args {
                    match arg {
                        Operand::Copy(_) | Operand::Move(_) => {
                            // Bytecode backend is a stack machine; always pass by value if possible
                            self.emit_operand(bc, arg, local_tys)?;
                        }
                        _ => self.emit_operand(bc, arg, local_tys)?,
                    }
                }
                bc.push(if is_indirect {
                    OP_CALL_INDIRECT
                } else {
                    OP_CALL
                });
                bc.extend_from_slice(&(args.len() as u32).to_le_bytes());
                bc.extend_from_slice(&destination.local.to_raw().to_le_bytes());
                let t = target.unwrap_or_else(|| BasicBlockIdx::from_raw(u32::MAX));
                bc.extend_from_slice(&t.to_raw().to_le_bytes());
                Ok(())
            }
            TerminatorKind::Unreachable => {
                bc.push(OP_TRAP);
                Ok(())
            }
            TerminatorKind::Assert {
                cond,
                expected,
                target,
                ..
            } => {
                self.emit_operand(bc, cond, local_tys)?;
                bc.push(OP_ASSERT);
                bc.push(if *expected { 1u8 } else { 0u8 });
                bc.extend_from_slice(&target.to_raw().to_le_bytes());
                Ok(())
            }
            TerminatorKind::Drop { place, target, .. } => {
                self.emit_place_address(bc, place, local_tys)?;
                bc.push(OP_DROP);
                bc.push(OP_JUMP);
                bc.extend_from_slice(&target.to_raw().to_le_bytes());
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests;

pub mod vtable;
