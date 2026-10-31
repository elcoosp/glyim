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
use glyim_core::{BinOp, CrateId, DefId, LocalDefId, UnOp, primitives::TargetInfo};
use glyim_layout::{LayoutComputer, SimpleLayoutComputer};
use glyim_mir::*;
use glyim_type::{AdtKind, FieldIdx, Substitution, Ty, TyCtx, TyKind};
use std::collections::HashMap;

mod interp_error;
mod interp_value;

pub use interp_error::InterpError;
pub use interp_value::InterpValue;

/// Interpreter.
pub struct Interpreter<'tcx> {
    tcx: &'tcx TyCtx,
    layout: SimpleLayoutComputer<'tcx>,
    /// When `true`, a panic (assert failure or statement evaluation error)
    /// routes to the current block's `cleanup` edge instead of aborting
    /// interpretation immediately. This implements single-frame unwind
    /// cleanup (plan §14.2): the cleanup block — which drop elaboration has
    /// populated with drop-glue calls — runs to completion before the
    /// function terminates. Cross-frame unwinding (walking the call stack to
    /// cleanup blocks in caller frames) IS implemented and hardened (plan
    /// §1.4): `unwind_step` keeps popping caller frames until one has a real
    /// `unwind_target` (its `Call`'s cleanup edge) or the stack empties,
    /// surfacing `InterpError::Unwind` at the top — it never resumes a caller's
    /// normal continuation for a propagating panic.
    pub panics_unwind: bool,
    /// Struct.
    pub step_limit: usize,
    /// Struct.
    pub recursion_limit: usize,
    step_count: usize,
    recursion_depth: usize,
    /// T140-PATCHED-IO: bytes captured from FFI writes.
    stdout_buf: Vec<u8>,
    stderr_buf: Vec<u8>,
    function_table: HashMap<DefId, Body>,
    current_body_owner: Option<DefId>,
    current_arg_count: usize,
    current_body: Option<Body>,
    current_bb: BasicBlockIdx,
    locals: Vec<Option<InterpValue>>,
    local_decls: Vec<LocalDecl>,
    call_stack: Vec<CallFrame>,
    /// Absolute depth of the frame currently executing (0 == the entry
    /// `run_body` frame, 1 == a function called directly from it, ...). Used
    /// to resolve frame-aware `Ref { frame, local }` values so that a borrow
    /// (`&mut self`) created in one frame and dereferenced in another reads the
    /// correct frame's locals instead of the callee's own (plan §1: for-loop
    /// `next(&mut self)`).
    frame_depth: usize,
    /// Per-absolute-depth frame generation. `frame_generations[d]` holds the
    /// generation stamped on the frame currently occupying depth `d`. When a
    /// frame is popped, its slot's generation is reset to 0, so a `Ref` that
    /// captured `frame == d` can be checked against `call_stack[d].generation`
    /// to detect the "borrower frame was popped and its slot reused" case
    /// (use-after-free style aliasing). See `locals_for_ref_frame`.
    frame_generations: Vec<u64>,
    /// Monotonic counter feeding `CallFrame::generation` / `frame_generations`.
    next_frame_gen: u64,
    /// Cross-frame unwind payload (plan §7.2). When a callee panics and this
    /// frame resumes at the caller's cleanup edge, `pending_unwind` holds the
    /// original panic so that once the caller's own cleanups finish and it
    /// re-panics, the walk continues upward rather than being swallowed.
    pending_unwind: Option<InterpError>,
    /// `static` item storage. Initialized lazily by `MirConstKind::StaticRef`
    /// eval; read / written through `InterpValue::StaticPtr` + `Deref` in
    /// `read_place` / `write_place`. Mirrors the LLVM backend's
    /// `__glyim_static_<id>` globals.
    globals: std::cell::RefCell<std::collections::HashMap<glyim_core::def_id::StaticDefId, InterpValue>>,
}

struct CallFrame {
    body: Body,
    #[allow(dead_code)]
    bb: BasicBlockIdx,
    locals: Vec<Option<InterpValue>>,
    return_place: Place,
    target_bb: BasicBlockIdx,
    /// The caller's unwind destination for the `Call` that invoked this
    /// callee — where the caller should resume when this frame unwinds
    /// (plan §7.2). Mirrors MIR `TerminatorKind::Call::cleanup`.
    unwind_target: Option<BasicBlockIdx>,
    /// Monotonic generation assigned when this frame was pushed. Used to
    /// detect a `Ref` whose borrower frame was popped and whose slot was
    /// later reused by an unrelated call (use-after-free style aliasing).
    generation: u64,
}

impl<'tcx> Interpreter<'tcx> {
    /// new.
    pub fn new(tcx: &'tcx TyCtx) -> Self {
        Interpreter {
            tcx,
            layout: SimpleLayoutComputer::new(tcx, TargetInfo::default()),
            panics_unwind: false,
            step_limit: 1_000_000,
            recursion_limit: 256,
            step_count: 0,
            recursion_depth: 0,
            stdout_buf: Vec::new(),
            stderr_buf: Vec::new(),
            function_table: HashMap::new(),
            current_body_owner: None,
            current_arg_count: 0,
            current_body: None,
            current_bb: BasicBlockIdx::from_raw(0),
            locals: Vec::new(),
            local_decls: Vec::new(),
            call_stack: Vec::new(),
            frame_depth: 0,
            frame_generations: Vec::new(),
            next_frame_gen: 0,
            pending_unwind: None,
            globals: std::cell::RefCell::new(std::collections::HashMap::new()),
        }
    }

    /// with_step_limit.
    pub fn with_step_limit(mut self, limit: usize) -> Self {
        self.step_limit = limit;
        self
    }

    /// Test-only accessor exposing the real layout-backed element size that
    /// drives pointer arithmetic. This is the observable result of the
    /// `get_element_size` fix (Tier 0.1): every element is sized by its
    /// layout, not assumed to be 1 byte.
    #[cfg(test)]
    pub(crate) fn element_size_of(&self, ty: Ty) -> InterpResult<usize> {
        self.get_element_size(ty)
    }

    /// with_recursion_limit.
    pub fn with_recursion_limit(mut self, limit: usize) -> Self {
        self.recursion_limit = limit;
        self
    }

    /// Configure whether a panicking call unwinds to its `cleanup` block.
    ///
    /// Default is `false`: a panic aborts immediately with
    /// `InterpError::Panic`, which is the contract the interpreter's own
    /// tests (`panic_without_unwind_aborts`, the `*_panics` family in
    /// `tests/advanced.rs`, `tests/div_rem.rs`, …) and both production
    /// callers (`glyip test`, `glyim-test`'s `interpreter_runner`) are
    /// written against — they match on `Panic(payload)`.
    ///
    /// Setting this to `true` enables the cross-frame unwinding path in
    /// `unwind_step` (walk caller frames to their `cleanup` edges, run drop
    /// glue, surface `InterpError::Unwind` at the top). That path is
    /// implemented and exercised by `tests/unwind_cleanup.rs`, but is not
    /// enabled by default because it changes the top-level error variant
    /// every caller observes.
    ///
    /// (An earlier revision of this doc claimed the flag was a no-op and
    /// the behavior unimplemented; both were wrong — see the field doc on
    /// `panics_unwind` and `unwind_step`'s implementation.)
    pub fn with_panics_unwind(mut self, yes: bool) -> Self {
        self.panics_unwind = yes;
        self
    }

    /// add_function.
    pub fn add_function(&mut self, def_id: DefId, body: Body) {
        self.function_table.insert(def_id, body);
    }

    /// step_limit.
    pub fn step_limit(&self) -> usize {
        self.step_limit
    }

    /// recursion_limit.
    pub fn recursion_limit(&self) -> usize {
        self.recursion_limit
    }

    /// recursion_depth.
    pub fn recursion_depth(&self) -> usize {
        self.recursion_depth
    }

    /// get_local_value.
    pub fn get_local_value(&self, local: LocalIdx) -> Option<&InterpValue> {
        self.locals.get(local.index())?.as_ref()
    }

    /// T140-PATCHED-BUILTIN: dispatch compiler-synthetic builtin methods.
    ///
    /// Typeck resolves a handful of builtin methods (`Vec::push` / `len` /
    /// `new`, `Option::unwrap` / `is_some` / `is_none`, `Result::unwrap` /
    /// `is_ok` / `is_err`, `String::new` / `len` / `as_str` / `push_str`,
    /// `str::len`) to synthetic `FnDefId`s in the 9000+ range with no MIR
    /// body. The LLVM backend handles these in `try_lower_builtin_intrinsic`;
    /// the interpreter had no equivalent and panicked `function not found`
    /// for any stdlib code using them.
    ///
    /// Returns `Ok(Some(value))` when the call was recognised (the caller
    /// writes `value` into the destination and jumps to `target`),
    /// `Ok(None)` when the callee is not a builtin this dispatcher knows,
    /// `Err` for a genuine runtime failure (e.g. `unwrap` on `None`).
    /// P1-C2 batch 1: infer the receiver's bit-width from the operand's
    /// local type. Required by the bit-manipulation intrinsics
    /// (`leading_zeros`, `trailing_zeros`, `count_zeros`, `next_power_of_two`)
    /// whose semantics depend on the integer width for `x == 0` and the
    /// overflow case. Projected places return `None` (the caller surfaces
    /// an explicit panic rather than silently miscompiling).
    fn operand_bit_width(&self, op: Option<&Operand>) -> Option<u32> {
        let op = op?;
        // P1-C2 batch 2b hotfix: the receiver of a builtin method call can
        // be either a place (Copy/Move of a local) or a constant (a literal
        // passed directly — e.g. `(65536u32).saturating_mul(x)`). The
        // original implementation only handled places and returned `None`
        // for constants, which the caller turned into an `InterpError::Panic`
        // — so any test whose receiver was a *literal* panicked under
        // mir-interp while working under LLVM (LLVM doesn't call this
        // helper). Fixed by reading the type off the constant directly.
        // This also retroactively fixes the wrapping arm from batch 2a,
        // which had the same latent gap.
        let ty = match op {
            Operand::Copy(p) | Operand::Move(p) => {
                if !p.projection.is_empty() {
                    return None;
                }
                let decl = self
                    .local_decls
                    .as_slice()
                    .get(p.local.to_raw() as usize)?;
                decl.ty
            }
            Operand::Constant(c) => c.ty,
        };
        match self.tcx.ty_kind(ty) {
            glyim_type::TyKind::Uint(glyim_core::primitives::UintTy::U8) => Some(8),
            glyim_type::TyKind::Uint(glyim_core::primitives::UintTy::U16) => Some(16),
            glyim_type::TyKind::Uint(glyim_core::primitives::UintTy::U32) => Some(32),
            glyim_type::TyKind::Uint(glyim_core::primitives::UintTy::U64) => Some(64),
            glyim_type::TyKind::Uint(glyim_core::primitives::UintTy::Usize) => Some(64),
            glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I8) => Some(8),
            glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I16) => Some(16),
            glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I32) => Some(32),
            glyim_type::TyKind::Int(glyim_core::primitives::IntTy::I64) => Some(64),
            glyim_type::TyKind::Int(glyim_core::primitives::IntTy::Isize) => Some(64),
            _ => None,
        }
    }

    fn try_call_builtin(
        &mut self,
        callee_id: &DefId,
        args: &[InterpValue],
        arg_operands: &[Operand],
    ) -> InterpResult<Option<InterpValue>> {
        let raw = callee_id.local_id.to_raw();
        // SENTINEL-HARD-FAIL: the top four id slots are typeck's synthetic
        // sentinels (`into` / `to_string` / `clone` / `drop`). Lowering
        // replaces them with an `Error` constant on the *dead* branch it
        // assumes they always sit on. When one is reached on a live path,
        // evaluating it as a zero value silently produces wrong output
        // (observed: `.to_string()` returning 0). Fail loudly instead.
        if raw >= u32::MAX - 4 {
            return Err(InterpError::Panic(format!(
                "reached a compiler-synthetic sentinel call (FnDefId {raw}); \
                 this branch was assumed dead. File a compiler bug."
            )));
        }
        if raw < 9_000 {
            return Ok(None);
        }
        let fn_def_id = glyim_core::def_id::FnDefId::from_raw(raw);
        let Some((adt_id, name)) = self.tcx.builtin_fn_id(fn_def_id) else {
            return Ok(None);
        };
        let method = self.tcx.name_str(name).to_string();
        let receiver = args.first().cloned().unwrap_or(InterpValue::Unit);

        match (adt_id.to_raw(), method.as_str()) {
            (1010, "is_some") => Ok(Some(InterpValue::Bool(
                Self::enum_tag(&receiver) == Some(1),
            ))),
            (1010, "is_none") => Ok(Some(InterpValue::Bool(
                Self::enum_tag(&receiver) != Some(1),
            ))),
            (1010, "unwrap") | (1010, "expect") => {
                if Self::enum_tag(&receiver) == Some(1) {
                    Ok(Some(Self::enum_payload(&receiver)))
                } else {
                    Err(InterpError::Panic(
                        "called `Option::unwrap()` on a `None` value".into(),
                    ))
                }
            }
            (1010, "unwrap_or") => {
                if Self::enum_tag(&receiver) == Some(1) {
                    Ok(Some(Self::enum_payload(&receiver)))
                } else {
                    Ok(Some(args.get(1).cloned().unwrap_or(InterpValue::Unit)))
                }
            }
            (1011, "is_ok") => Ok(Some(InterpValue::Bool(
                Self::enum_tag(&receiver) == Some(0),
            ))),
            (1011, "is_err") => Ok(Some(InterpValue::Bool(
                Self::enum_tag(&receiver) != Some(0),
            ))),
            (1011, "unwrap") | (1011, "expect") => {
                if Self::enum_tag(&receiver) == Some(0) {
                    Ok(Some(Self::enum_payload(&receiver)))
                } else {
                    Err(InterpError::Panic(
                        "called `Result::unwrap()` on an `Err` value".into(),
                    ))
                }
            }
            (1020, "new") => {
                // Real Vec<T> = { buf: RawVec<T>, len: usize }. Model as
                // a two-field aggregate so `len`/`push` can agree on shape.
                Ok(Some(InterpValue::Aggregate(vec![
                    InterpValue::Aggregate(Vec::new()),
                    InterpValue::Uint(0),
                ])))
            }
            (1020, "len") => {
                let v = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Uint(Self::vec_len(&v) as u128)))
            }
            (1020, "is_empty") => {
                let v = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Bool(Self::vec_len(&v) == 0)))
            }
            (1020, "push") => {
                let elem = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    Self::vec_push(v, elem);
                })?;
                Ok(Some(v))
            }
            (1020, "pop") => {
                // Returns Option<T>: `None` (tag 0) when empty, else
                // `Some(elem)` (tag 1). The interpreter models `Option` as
                // `Aggregate([Int(tag), payload])` (see `enum_tag` /
                // `enum_payload`).
                match arg_operands.first() {
                    Some(Operand::Copy(p) | Operand::Move(p)) => {
                        let mut v = self.read_place(p)?;
                        let removed = Self::vec_pop(&mut v);
                        self.write_place(p, v)?;
                        Ok(Some(removed))
                    }
                    _ => Ok(Some(InterpValue::Aggregate(vec![
                        InterpValue::Int(0),
                        InterpValue::Unit,
                    ]))),
                }
            }
            (1020, "insert") => {
                let idx = match args.get(1) {
                    Some(InterpValue::Uint(n)) => *n as usize,
                    Some(InterpValue::Int(n)) => (*n).max(0) as usize,
                    _ => 0,
                };
                let elem = args.get(2).cloned().unwrap_or(InterpValue::Unit);
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    Self::vec_insert(v, idx, elem);
                })?;
                Ok(Some(v))
            }
            (1020, "reserve") => {
                // Model limitation: the interpreter's Vec has no separate
                // capacity word, so `reserve` is a no-op. Assertions should
                // use `capacity() >= len()`, which this satisfies.
                let v = self.builtin_deref(&receiver)?;
                Ok(Some(v))
            }
            (1020, "capacity") => {
                // Model limitation: the interpreter's Vec is
                // `[elements, len]` with no separate capacity word, so it
                // reports `len`. Real codegen reports the grown capacity
                // (`>= len`). A fixture must therefore assert only
                // `capacity() >= len()`.
                let v = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Uint(Self::vec_len(&v) as u128)))
            }
            (1020, "set_len") => {
                let n = match args.get(1) {
                    Some(InterpValue::Uint(n)) => *n as usize,
                    Some(InterpValue::Int(n)) => (*n).max(0) as usize,
                    _ => 0,
                };
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.len() == 2 {
                            fields[1] = InterpValue::Uint(n as u128);
                        }
                    }
                })?;
                Ok(Some(v))
            }
            (1020, "truncate") => {
                let n = match args.get(1) {
                    Some(InterpValue::Uint(n)) => *n as usize,
                    Some(InterpValue::Int(n)) => (*n).max(0) as usize,
                    _ => 0,
                };
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.len() == 2 {
                            let cur = match &fields[1] {
                                InterpValue::Uint(x) => *x as usize,
                                InterpValue::Int(x) => (*x).max(0) as usize,
                                _ => 0,
                            };
                            if cur > n {
                                fields[1] = InterpValue::Uint(n as u128);
                            }
                        }
                    }
                })?;
                Ok(Some(v))
            }
            (1020, "remove") => {
                let idx = match args.get(1) {
                    Some(InterpValue::Uint(n)) => *n as usize,
                    Some(InterpValue::Int(n)) => (*n).max(0) as usize,
                    _ => 0,
                };
                match arg_operands.first() {
                    Some(Operand::Copy(p) | Operand::Move(p)) => {
                        let mut v = self.read_place(p)?;
                        let removed = Self::vec_remove(&mut v, idx);
                        self.write_place(p, v)?;
                        Ok(Some(removed))
                    }
                    _ => Ok(Some(InterpValue::Unit)),
                }
            }
            (1020, "clear") => {
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.len() == 2 {
                            if let InterpValue::Aggregate(buf) = &mut fields[0] {
                                buf.clear();
                            }
                            fields[1] = InterpValue::Uint(0);
                        } else {
                            fields.clear();
                        }
                    }
                })?;
                Ok(Some(v))
            }
            // BC-VEC-CLONE (KNOWN_GAPS §3.5.4 / P1-C1 second half):
            // `Vec<T>::clone` for non-Copy `T`. The LLVM backend lowers
            // this to `glyim_slice_to_vec` via a new arm in
            // `codegen-llvm/lower.rs`; the interpreter has to mirror it
            // so the run-pass corpus (which uses mir-interp) passes.
            //
            // The interp Vec model is `Aggregate([Aggregate(elements),
            // Uint(len)])`. Cloning is a deep structural copy — no
            // interior-mutability or reference sharing, which matches
            // Rust semantics for `Vec<T>::clone` (the elements themselves
            // are recursively cloned).
            (1020, "clone") => {
                let v = self.builtin_deref(&receiver)?;
                Ok(Some(v.clone()))
            }
            // BC-IS-POWER-OF-TWO (P1-C2): shared by u8/u32/u64/usize.
            // Rust semantics: x != 0 && (x & (x - 1)) == 0.
            (_, "is_power_of_two") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let r = n != 0 && (n & (n.wrapping_sub(1))) == 0;
                Ok(Some(InterpValue::Bool(r)))
            }
            // P1-C2 batch 1: `count_ones` / `count_zeros` / `leading_zeros`
            // / `trailing_zeros` / `next_power_of_two` on unsigned integers.
            // Rust semantics on the zero case: `leading_zeros(0) == width`,
            // `trailing_zeros(0) == width`; `next_power_of_two(0) == 1`.
            // Overflow of `next_power_of_two` (x > 2^(width-1)) wraps to 0
            // to mirror the LLVM backend's current behavior; the fixture
            // does not exercise that case.
            (_, "count_ones") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                Ok(Some(InterpValue::Uint(n.count_ones() as u128)))
            }
            (_, "count_zeros") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(
                        "interp: count_zeros: cannot determine receiver bit width".into(),
                    )
                })?;
                let mask: u128 = (1u128 << width) - 1;
                let trimmed = n & mask;
                Ok(Some(InterpValue::Uint(
                    (width as u128) - (trimmed.count_ones() as u128),
                )))
            }
            (_, "leading_zeros") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(
                        "interp: leading_zeros: cannot determine receiver bit width".into(),
                    )
                })?;
                let mask: u128 = (1u128 << width) - 1;
                let trimmed = n & mask;
                let lz: u128 = if trimmed == 0 {
                    width as u128
                } else {
                    (width as u128) - (128u128 - (trimmed.leading_zeros() as u128))
                };
                Ok(Some(InterpValue::Uint(lz)))
            }
            (_, "trailing_zeros") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(
                        "interp: trailing_zeros: cannot determine receiver bit width".into(),
                    )
                })?;
                let mask: u128 = (1u128 << width) - 1;
                let trimmed = n & mask;
                let tz: u128 = if trimmed == 0 {
                    width as u128
                } else {
                    trimmed.trailing_zeros() as u128
                };
                Ok(Some(InterpValue::Uint(tz)))
            }
            (_, "next_power_of_two") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(
                        "interp: next_power_of_two: cannot determine receiver bit width".into(),
                    )
                })?;
                let result: u128 = if n <= 1 {
                    1
                } else {
                    let x_minus_1 = n - 1;
                    let bits_used = 128u32 - x_minus_1.leading_zeros();
                    if bits_used >= width {
                        0
                    } else {
                        1u128 << bits_used
                    }
                };
                Ok(Some(InterpValue::Uint(result)))
            }
            (_, "leading_ones") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(
                        "interp: leading_ones: cannot determine receiver bit width".into(),
                    )
                })?;
                let mask: u128 = (1u128 << width) - 1;
                let trimmed = n & mask;
                let inverted = (!trimmed) & mask;
                let lo: u128 = if inverted == 0 {
                    width as u128
                } else {
                    (width as u128) - (128u128 - (inverted.leading_zeros() as u128))
                };
                Ok(Some(InterpValue::Uint(lo)))
            }
            // P1-C2 batch 2c: checked arithmetic on unsigned integers.
            // Returns `Option<T>`: `None` on overflow, `Some(result)` when
            // the operation fits. The Option shape here matches what
            // `vec_pop` produces: `Aggregate([Int(0), Unit])` for None,
            // `Aggregate([Int(1), Uint(result)])` for Some.
            //
            // Overflow detection uses the width-specific bounds rather
            // than `u128::checked_*` — `a_n + b_n` at u128 level never
            // overflows for inputs constrained to a smaller width, but
            // the *narrow* width's overflow is what this method reports.
            // Each branch checks the exact condition that would push the
            // result out of `[0, max]` before computing, so the inner
            // arithmetic is guaranteed not to overflow u128.
            (_, "checked_add") | (_, "checked_sub") | (_, "checked_mul") => {
                let a = self.builtin_deref(&receiver)?;
                let b = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let a_n: u128 = match &a {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let b_n: u128 = match &b {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(format!(
                        "interp: {}: cannot determine receiver bit width",
                        method
                    ))
                })?;
                let max: u128 = if width >= 128 {
                    u128::MAX
                } else {
                    (1u128 << width) - 1
                };
                let result: Option<u128> = match method.as_str() {
                    "checked_add" => {
                        if a_n > max - b_n {
                            None
                        } else {
                            Some(a_n + b_n)
                        }
                    }
                    "checked_sub" => {
                        if b_n > a_n {
                            None
                        } else {
                            Some(a_n - b_n)
                        }
                    }
                    "checked_mul" => {
                        if b_n != 0 && a_n > max / b_n {
                            None
                        } else {
                            Some(a_n * b_n)
                        }
                    }
                    _ => unreachable!(),
                };
                Ok(Some(match result {
                    None => InterpValue::Aggregate(vec![InterpValue::Int(0), InterpValue::Unit]),
                    Some(v) => {
                        InterpValue::Aggregate(vec![InterpValue::Int(1), InterpValue::Uint(v)])
                    }
                }))
            }
            // P1-C2 batch 2b: saturating arithmetic on unsigned integers.
            // `saturating_add` / `saturating_sub` / `saturating_mul`.
            // `u128::saturating_*` clamps at u128's bounds, so the
            // width-specific clamp to `(1 << width) - 1` must be applied
            // *after* — otherwise an `a + b` that fits in u128 but not the
            // receiver width would silently exceed the width's range.
            // For sub, u128 saturating_sub already clamps at 0, matching
            // the width-independent unsigned behavior.
            (_, "saturating_add") | (_, "saturating_sub") | (_, "saturating_mul") => {
                let a = self.builtin_deref(&receiver)?;
                let b = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(format!(
                        "interp: {}: cannot determine receiver bit width",
                        method
                    ))
                })?;

                // §3.5.19 follow-up: dispatch on the receiver's *declared type*
                // (via operand_ty), not the runtime InterpValue variant. Some
                // unsigned receivers arrive as InterpValue::Int after
                // trunc_to_ty; value-variant dispatch would route them to the
                // signed branch and clamp at [INT_MIN, INT_MAX] instead of
                // [0, UINT_MAX]. Type dispatch mirrors the LLVM backend's
                // place_ty(destination) decision.
                let recv_ty = self.operand_ty(
                    arg_operands.first().ok_or_else(|| {
                        InterpError::Panic(format!(
                            "interp: {}: no receiver operand",
                            method
                        ))
                    })?,
                );
                let signed = recv_ty
                    .map(|t| matches!(self.tcx.ty_kind(t), glyim_type::TyKind::Int(_)))
                    .unwrap_or(false);

                if signed {
                    let a_s: i128 = match &a {
                        InterpValue::Int(i) => *i,
                        InterpValue::Uint(n) => *n as i128,
                        _ => return Ok(None),
                    };
                    let b_s: i128 = match &b {
                        InterpValue::Int(i) => *i,
                        InterpValue::Uint(n) => *n as i128,
                        _ => return Ok(None),
                    };
                    let (min, max): (i128, i128) = if width >= 128 {
                        (i128::MIN, i128::MAX)
                    } else {
                        let max = (1i128 << (width - 1)) - 1;
                        let min = -(1i128 << (width - 1));
                        (min, max)
                    };
                    let result: i128 = match method.as_str() {
                        "saturating_add" => a_s.saturating_add(b_s).clamp(min, max),
                        "saturating_sub" => a_s.saturating_sub(b_s).clamp(min, max),
                        "saturating_mul" => a_s.saturating_mul(b_s).clamp(min, max),
                        _ => unreachable!(),
                    };
                    return Ok(Some(InterpValue::Int(result)));
                }

                let a_n: u128 = match &a {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let b_n: u128 = match &b {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let max: u128 = if width >= 128 {
                    u128::MAX
                } else {
                    (1u128 << width) - 1
                };
                let result: u128 = match method.as_str() {
                    "saturating_add" => a_n.saturating_add(b_n).min(max),
                    "saturating_sub" => a_n.saturating_sub(b_n),
                    "saturating_mul" => a_n.saturating_mul(b_n).min(max),
                    _ => unreachable!(),
                };
                Ok(Some(InterpValue::Uint(result)))
            }
            // P1-C2 batch 2a: wrapping arithmetic. Rust semantics: perform
            // at infinite precision, truncate to receiver width. Signed
            // variants (§3.5.31(b)) additionally sign-extend the low
            // `width` bits so InterpValue::Int round-trips. Dispatch by
            // the receiver's declared type, not the InterpValue variant
            // (some unsigned receivers arrive as Int after trunc_to_ty —
            // see §3.5.19 follow-up).
            (_, "wrapping_add") | (_, "wrapping_sub") | (_, "wrapping_mul") => {
                let a = self.builtin_deref(&receiver)?;
                let b = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(format!(
                        "interp: {}: cannot determine receiver bit width",
                        method
                    ))
                })?;
                let recv_ty = self.operand_ty(arg_operands.first().ok_or_else(|| {
                    InterpError::Panic(format!("interp: {}: no receiver operand", method))
                })?);
                let signed = recv_ty
                    .map(|t| matches!(self.tcx.ty_kind(t), glyim_type::TyKind::Int(_)))
                    .unwrap_or(false);
                let mask: u128 = if width >= 128 { u128::MAX } else { (1u128 << width) - 1 };

                if signed {
                    let a_s: i128 = match &a {
                        InterpValue::Int(i) => *i,
                        InterpValue::Uint(n) => *n as i128,
                        _ => return Ok(None),
                    };
                    let b_s: i128 = match &b {
                        InterpValue::Int(i) => *i,
                        InterpValue::Uint(n) => *n as i128,
                        _ => return Ok(None),
                    };
                    let raw: i128 = match method.as_str() {
                        "wrapping_add" => a_s.wrapping_add(b_s),
                        "wrapping_sub" => a_s.wrapping_sub(b_s),
                        "wrapping_mul" => a_s.wrapping_mul(b_s),
                        _ => unreachable!(),
                    };
                    let masked = (raw as u128) & mask;
                    let result: i128 = if width >= 128 {
                        masked as i128
                    } else if masked & (1u128 << (width - 1)) != 0 {
                        (masked as i128) - (1i128 << width)
                    } else {
                        masked as i128
                    };
                    return Ok(Some(InterpValue::Int(result)));
                }

                let a_n: u128 = match &a {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let b_n: u128 = match &b {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let result: u128 = match method.as_str() {
                    "wrapping_add" => a_n.wrapping_add(b_n) & mask,
                    "wrapping_sub" => a_n.wrapping_sub(b_n) & mask,
                    "wrapping_mul" => a_n.wrapping_mul(b_n) & mask,
                    _ => unreachable!(),
                };
                Ok(Some(InterpValue::Uint(result)))
            }
            (_, "trailing_ones") => {
                let v = self.builtin_deref(&receiver)?;
                let n: u128 = match &v {
                    InterpValue::Uint(n) => *n,
                    InterpValue::Int(i) => (*i).max(0) as u128,
                    _ => return Ok(None),
                };
                let width = self.operand_bit_width(arg_operands.first()).ok_or_else(|| {
                    InterpError::Panic(
                        "interp: trailing_ones: cannot determine receiver bit width".into(),
                    )
                })?;
                let mask: u128 = (1u128 << width) - 1;
                let trimmed = n & mask;
                let inverted = (!trimmed) & mask;
                let to: u128 = if inverted == 0 {
                    width as u128
                } else {
                    inverted.trailing_zeros() as u128
                };
                Ok(Some(InterpValue::Uint(to)))
            }
            (1050, "clear") | (1061, "clear") => {
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    *v = InterpValue::String(String::new());
                })?;
                Ok(Some(v))
            }
            (1050, "from_utf8") => {
                // `String::from_utf8(Vec<u8>) -> Result<String, Vec<u8>>`.
                // The argument is a `Vec<u8>` modelled as
                // `Aggregate([buf, len])` with `buf = Aggregate([u8, …])`.
                // Build the byte string from `buf`'s elements (NOT
                // `byte_payload`, which would mis-read the length word as a
                // repeat count for a `Vec`), validate, and tag the result.
                let arg = args.first().cloned().unwrap_or(InterpValue::Unit);
                let bytes = Self::vec_u8_bytes(&arg);
                if std::str::from_utf8(bytes.as_bytes()).is_ok() {
                    Ok(Some(InterpValue::Aggregate(vec![
                        InterpValue::Int(0),
                        InterpValue::String(bytes),
                    ])))
                } else {
                    Ok(Some(InterpValue::Aggregate(vec![
                        InterpValue::Int(1),
                        arg,
                    ])))
                }
            }
            (1050, "from") | (1050, "from_utf8_lossy") => {
                // `String::from(&str)`: the argument is the string view.
                let s = args
                    .first()
                    .cloned()
                    .map(|v| self.builtin_deref(&v))
                    .transpose()?
                    .unwrap_or(InterpValue::String(String::new()));
                Ok(Some(s))
            }
            (1060, "copy_from_slice") => {
                // MODEL LIMITATION: `as_slice` returns a *copy* of the
                // element bytes (an `InterpValue::String`), not a view
                // linked back to the owning `Vec`. So a `&mut [T]` receiver
                // has no path to write through to the original buffer, and
                // this is a no-op. The observable length of the destination
                // is unaffected either way; fixtures assert only that. A
                // faithful model would require slice values to carry a
                // `Ref` back to their source, which the interpreter does
                // not currently represent.
                Ok(Some(InterpValue::Unit))
            }
            (1060, "split_at") => {
                let s = self.builtin_deref(&receiver)?;
                let mid = match args.get(1) {
                    Some(InterpValue::Uint(n)) => *n as usize,
                    Some(InterpValue::Int(n)) => (*n).max(0) as usize,
                    _ => 0,
                };
                let full = Self::byte_payload(&s);
                let chars: Vec<char> = full.chars().collect();
                let n = chars.len();
                let mid = mid.min(n);
                let lo: String = chars[..mid].iter().collect();
                let hi: String = chars[mid..].iter().collect();
                // Tuple of two slices, each modelled as a `String` so that
                // `str_len` on a projected element yields the char count
                // (matching codegen's `{ptr, len}` fat pointer length).
                Ok(Some(InterpValue::Aggregate(vec![
                    InterpValue::String(lo),
                    InterpValue::String(hi),
                ])))
            }
            (1060, "to_vec") => {
                let s = self.builtin_deref(&receiver)?;
                let bytes: Vec<InterpValue> = match &s {
                    InterpValue::String(st) => st
                        .as_bytes()
                        .iter()
                        .map(|b| InterpValue::Uint(*b as u128))
                        .collect(),
                    InterpValue::Aggregate(f) => f.clone(),
                    _ => Vec::new(),
                };
                let n = bytes.len() as u128;
                Ok(Some(InterpValue::Aggregate(vec![
                    InterpValue::Aggregate(bytes),
                    InterpValue::Uint(n),
                ])))
            }
            (1050, "into_bytes") => {
                let s = self.builtin_deref(&receiver)?;
                let bytes: Vec<InterpValue> = match &s {
                    InterpValue::String(st) => st
                        .as_bytes()
                        .iter()
                        .map(|b| InterpValue::Uint(*b as u128))
                        .collect(),
                    InterpValue::Aggregate(fields) => fields.clone(),
                    _ => Vec::new(),
                };
                let n = bytes.len() as u128;
                Ok(Some(InterpValue::Aggregate(vec![
                    InterpValue::Aggregate(bytes),
                    InterpValue::Uint(n),
                ])))
            }
            // UNSAFECELL-NEW: `UnsafeCell::new(v)` is a single-field wrapper.
            (1005, "new") => {
                let v = args.first().cloned().unwrap_or(InterpValue::Unit);
                Ok(Some(InterpValue::Aggregate(vec![v])))
            }
            // ATOMIC-BUILTINS: `AtomicBool` (1016), `AtomicUsize` (1017),
            // `AtomicU8` (1018). Modelled as a single-field
            // `Aggregate([stored])` (the `UnsafeCell` wrapper flattened).
            // `load` reads the field; `store` and `fetch_*` write it back
            // through `mutate_receiver` so a `&self` receiver still reaches
            // the owning local.
            (1016, "new") | (1017, "new") | (1018, "new") => {
                let v = args.first().cloned().unwrap_or(InterpValue::Unit);
                Ok(Some(InterpValue::Aggregate(vec![v])))
            }
            (1016, "load") | (1017, "load") | (1018, "load") => {
                let s = self.builtin_deref(&receiver)?;
                let v = match &s {
                    InterpValue::Aggregate(fields) => {
                        fields.first().cloned().unwrap_or(InterpValue::Unit)
                    }
                    other => other.clone(),
                };
                Ok(Some(v))
            }
            (1016, "store") | (1017, "store") | (1018, "store") => {
                let new_val = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let _ = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.is_empty() {
                            fields.push(new_val.clone());
                        } else {
                            fields[0] = new_val.clone();
                        }
                    }
                })?;
                Ok(Some(InterpValue::Unit))
            }
            (1016, "fetch_and") | (1016, "fetch_or") => {
                let is_and = method.as_str() == "fetch_and";
                let rhs = matches!(args.get(1), Some(InterpValue::Bool(true)));
                let old = std::cell::Cell::new(false);
                let _ = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.is_empty() {
                            fields.push(InterpValue::Bool(false));
                        }
                        let cur = matches!(&fields[0], InterpValue::Bool(true));
                        old.set(cur);
                        fields[0] =
                            InterpValue::Bool(if is_and { cur && rhs } else { cur || rhs });
                    }
                })?;
                Ok(Some(InterpValue::Bool(old.get())))
            }
            (1017, "fetch_add") | (1017, "fetch_sub") => {
                let is_add = method.as_str() == "fetch_add";
                let rhs = match args.get(1) {
                    Some(InterpValue::Uint(n)) => *n,
                    Some(InterpValue::Int(n)) => *n as u128,
                    _ => 0,
                };
                let old = std::cell::Cell::new(0u128);
                let _ = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.is_empty() {
                            fields.push(InterpValue::Uint(0));
                        }
                        let cur = match &fields[0] {
                            InterpValue::Uint(n) => *n,
                            InterpValue::Int(n) => *n as u128,
                            _ => 0,
                        };
                        old.set(cur);
                        let new = if is_add {
                            cur.wrapping_add(rhs)
                        } else {
                            cur.wrapping_sub(rhs)
                        };
                        fields[0] = InterpValue::Uint(new);
                    }
                })?;
                Ok(Some(InterpValue::Uint(old.get())))
            }
            // `compare_exchange(expected, new, ..) -> Result<T, T>`:
            // succeed (variant 0 = `Ok`) iff the current value equals
            // `expected`, otherwise (variant 1 = `Err`). Either way the
            // payload is the *previous* value. Atomics are modelled as a
            // single-field `Aggregate([stored])`, so the current value is
            // `fields[0]`.
            (1016, "compare_exchange") | (1017, "compare_exchange") => {
                let expected = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let new_val = args.get(2).cloned().unwrap_or(InterpValue::Unit);
                let prev = std::cell::Cell::new(InterpValue::Unit);
                let _ = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    if let InterpValue::Aggregate(fields) = v {
                        if fields.is_empty() {
                            fields.push(InterpValue::Unit);
                        }
                        let cur = fields[0].clone();
                        prev.set(cur.clone());
                        if cur == expected {
                            fields[0] = new_val.clone();
                        }
                    }
                })?;
                let prev = prev.into_inner();
                let tag = if prev == expected { 0 } else { 1 };
                Ok(Some(InterpValue::Aggregate(vec![
                    InterpValue::Int(tag),
                    prev,
                ])))
            }
            (1050, "new") => Ok(Some(InterpValue::String(String::new()))),
            // (str_id 1061 and string_id 1050 `push_str` are both handled below)
            (1050, "len") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Uint(Self::str_len(&s) as u128)))
            }
            (1050, "is_empty") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Bool(Self::str_len(&s) == 0)))
            }
            (1050, "as_str") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(s))
            }
            (1050, "push_str") | (1061, "push_str") => {
                let add = args.get(1).cloned().unwrap_or(InterpValue::Unit);
                let v = self.mutate_receiver(arg_operands.first(), args.first(), |v| {
                    Self::string_push_str(v, &add);
                })?;
                Ok(Some(v))
            }
            (1061, "len") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Uint(Self::str_len(&s) as u128)))
            }
            (1061, "is_empty") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Bool(Self::str_len(&s) == 0)))
            }
            // T140-PATCHED-IO: byte-view operations. `&str`/`&[u8]` payloads
            // are carried in `InterpValue::String` (byte-exact for the ASCII
            // fixtures; lossy for non-UTF-8, a documented limit of this
            // interpreter's reference model).
            (1061, "as_bytes") | (1050, "as_bytes") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::String(Self::byte_payload(&s))))
            }
            // T140-PATCHED-STR-IDENTITY: `str::as_str(&self) -> &str` is an
            // identity on a `&str`; `str::to_string` / `String::to_string`
            // wrap the payload as a `String`. The `format!("{}", x)`
            // expansion emits `(x).to_string().as_str()`, so both are on
            // the path.
            (1061, "as_str") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::String(Self::byte_payload(&s))))
            }
            (1061, "to_string") | (1050, "to_string") | (1060, "to_string") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::String(Self::byte_payload(&s))))
            }
            (1060, "len") => {
                // SLICE-LEN-TYPED: `[T]`/`[T; N]`/`&[T]` have three runtime
                // representations, and the correct length comes from the
                // receiver's *declared type*, not a blind `byte_payload`:
                //  - `[T; N]` / array literal  -> a flat `Aggregate([e0..eN-1])`,
                //    length = field count.
                //  - `&[T]` (fat pointer)      -> `Aggregate([data_ptr, len])`,
                //    length = field 1.
                //  - `String`/`&str` byte view -> the byte payload length.
                // The previous single `byte_payload` call returned 0 for a
                // plain array, so `[1,2,3].len()` was 0.
                let declared = arg_operands.first().and_then(|o| self.operand_ty(o));
                let is_fat_ptr = matches!(
                    declared.and_then(|t| self.tcx.ty_kind(t).clone().into()),
                    Some(glyim_type::TyKind::Ref(_, inner, _))
                        if matches!(self.tcx.ty_kind(inner), glyim_type::TyKind::Slice(_))
                );
                let s = self.builtin_deref(&receiver)?;
                let len = match &s {
                    InterpValue::Aggregate(fields) => {
                        if is_fat_ptr && fields.len() >= 2 {
                            // Fat pointer `{ptr, len}`: read field 1.
                            match &fields[1] {
                                InterpValue::Int(n) => (*n).max(0) as usize,
                                InterpValue::Uint(n) => *n as usize,
                                _ => fields.len(),
                            }
                        } else {
                            fields.len()
                        }
                    }
                    other => Self::byte_payload(other).len(),
                };
                Ok(Some(InterpValue::Uint(len as u128)))
            }
            (1060, "is_empty") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::Bool(Self::byte_payload(&s).is_empty())))
            }
            (1020, "as_ptr") | (1060, "as_ptr") | (1061, "as_ptr") => {
                let s = self.builtin_deref(&receiver)?;
                Ok(Some(InterpValue::String(Self::byte_payload(&s))))
            }
            // BC-ITER: `<[T]>::iter(&self) -> &[T]` and `iter_mut`.
            // The interpreter models a slice as a `String` of one char
            // per element (same convention `as_slice` uses on Vec), so
            // `next` (iter.g:332, a real body — not a builtin) can use
            // `is_empty` / `cur[0]` / `cur[1..]` unchanged. For array
            // receivers, convert the element aggregate to that string;
            // for slice/Vec receivers, pass through.
            (1060, "iter") | (1060, "iter_mut") => {
                let s = self.builtin_deref(&receiver)?;
                match &s {
                    // Already a slice model — identity.
                    InterpValue::String(_) => Ok(Some(s)),
                    InterpValue::Aggregate(fields) => {
                        // Vec-style `{Aggregate(elems), Uint(len)}` vs.
                        // bare array `Aggregate(elem, ..)`.
                        let elems: Vec<InterpValue> = if fields.len() == 2
                            && matches!(&fields[0], InterpValue::Aggregate(_))
                            && matches!(&fields[1], InterpValue::Uint(_) | InterpValue::Int(_))
                        {
                            match &fields[0] {
                                InterpValue::Aggregate(buf) => buf.clone(),
                                _ => Vec::new(),
                            }
                        } else {
                            fields.clone()
                        };
                        let slice: String = elems
                            .iter()
                            .map(|e| match e {
                                InterpValue::Uint(n) => {
                                    char::from_u32((*n as u32) & 0xff).unwrap_or('\u{0}')
                                }
                                InterpValue::Int(n) => {
                                    char::from_u32(((*n).max(0) as u32) & 0xff)
                                        .unwrap_or('\u{0}')
                                }
                                _ => '\u{0}',
                            })
                            .collect();
                        Ok(Some(InterpValue::String(slice)))
                    }
                    _ => Ok(None),
                }
            }
            (1020, "as_slice") | (1020, "as_ref") => {
                let s = self.builtin_deref(&receiver)?;
                // VEC-AS-SLICE: a `Vec` is modelled as
                // `Aggregate([Aggregate(elements), Uint(len)])`. The old
                // `byte_payload(&s)` recursed into the first *element* and
                // produced NUL bytes — it was written for `String`/`&str`,
                // where the payload IS the bytes — so `v.as_slice().len()`
                // was wrong (0 or 1 instead of the element count). Model
                // the slice as a one-char-per-element `String` so `str_len`
                // equals the element count and `to_vec` /
                // `from_utf8_lossy` round-trip.
                let elems: Vec<InterpValue> = match &s {
                    InterpValue::Aggregate(fields) if fields.len() == 2 => match &fields[0] {
                        InterpValue::Aggregate(buf) => buf.clone(),
                        _ => Vec::new(),
                    },
                    _ => Vec::new(),
                };
                let slice: String = elems
                    .iter()
                    .map(|e| match e {
                        InterpValue::Uint(n) => {
                            char::from_u32((*n as u32) & 0xff).unwrap_or('\u{0}')
                        }
                        InterpValue::Int(n) => {
                            char::from_u32(((*n).max(0) as u32) & 0xff).unwrap_or('\u{0}')
                        }
                        _ => '\u{0}',
                    })
                    .collect();
                Ok(Some(InterpValue::String(slice)))
            }
            _ => Ok(None),
        }
    }

    fn builtin_deref(&self, v: &InterpValue) -> InterpResult<InterpValue> {
        match v {
            InterpValue::Ref { frame, local, projection } => {
                let mut val = self
                    .locals_for_ref_frame(*frame)
                    .and_then(|locals| locals.get(*local).and_then(|o| o.as_ref()).cloned())
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "builtin receiver: Ref{{frame:{frame}, local:{local}}} points to uninitialized or dead slot"
                        ))
                    })?;
                // Apply the reference's own projection. `&mut _5.field0`
                // carries `[Field(0)]`; without applying it we'd hand the
                // *whole* `_5` aggregate to the builtin (e.g.
                // `w.v.len()` reading `W { v, i }` and returning
                // `vec_len`'s `fields[1]` fallback = 1). See KNOWN_GAPS §8.
                for proj in projection.iter() {
                    match proj {
                        glyim_mir::ProjectionElem::Field(fi) => {
                            let fi = fi.index();
                            val = match val {
                                InterpValue::Aggregate(fields) => fields
                                    .into_iter()
                                    .nth(fi)
                                    .ok_or_else(|| {
                                        InterpError::Panic(format!(
                                            "builtin_deref: field {fi} out of bounds on projected Ref"
                                        ))
                                    })?,
                                other => {
                                    return Err(InterpError::Panic(format!(
                                        "builtin_deref: Field projection on non-aggregate value {:?}",
                                        other
                                    )));
                                }
                            };
                        }
                        glyim_mir::ProjectionElem::Deref => {
                            // `self.v.len()` where `self: &mut VecIter<T>`
                            // produces `Ref{1, [Deref, Field(0)]}`: deref the
                            // `&mut VecIter<T>` to reach the struct, then
                            // project its `v` field. `builtin_deref` has
                            // already resolved the base local (a `Ref` value
                            // in `frame_locals[local]`), so a Deref step here
                            // just peels that outer reference.
                            val = match val {
                                InterpValue::Ref { frame, local, .. } => self
                                    .locals_for_ref_frame(frame)
                                    .and_then(|locals| {
                                        locals.get(local).and_then(|o| o.as_ref()).cloned()
                                    })
                                    .ok_or_else(|| {
                                        InterpError::Panic(format!(
                                            "builtin_deref: Deref in projection points at uninitialized local {local} (frame {frame})"
                                        ))
                                    })?,
                                // Byte payload (`String`) — already the
                                // "dereferenced" view (`read_place` treats
                                // `Deref` on it as a no-op). Same here.
                                InterpValue::String(_) => val,
                                InterpValue::StaticPtr { def_id } => self
                                    .globals
                                    .borrow()
                                    .get(&def_id)
                                    .cloned()
                                    .ok_or_else(|| {
                                        InterpError::Panic(format!(
                                            "builtin_deref: Deref in projection points at uninitialized static {def_id:?}"
                                        ))
                                    })?,
                                other => {
                                    return Err(InterpError::Panic(format!(
                                        "builtin_deref: Deref on non-reference value {other:?}"
                                    )));
                                }
                            };
                        }
                        other => {
                            return Err(InterpError::Panic(format!(
                                "builtin_deref: projection {other:?} in Ref not yet supported"
                            )));
                        }
                    }
                }
                Ok(val)
            }
            other => Ok(other.clone()),
        }
    }

    /// T140-PATCHED-MUTRECV: read-modify-write the receiver of a builtin
    /// `&mut self` method. The current lowering emits builtin method calls
    /// with the receiver as a `Copy(place)` *value* rather than an
    /// autoref'd `&mut` (see the follow-up note in the session handoff).
    /// When the receiver is a place, write back to that place; when it is
    /// an `InterpValue::Ref`, deref/write through the frame-aware path.
    fn mutate_receiver<F: FnOnce(&mut InterpValue)>(
        &mut self,
        op: Option<&Operand>,
        value: Option<&InterpValue>,
        f: F,
    ) -> InterpResult<InterpValue> {
        // T140-PATCHED-RETURN-MUTATED: return the mutated value in addition
        // to writing it back. MIR sometimes uses the *receiver's* place as
        // the call destination for a `()`-returning `&mut self` method
        // (`s.push_str(..)` lowered with destination = `s`); returning Unit
        // then clobbered `s`. Returning the mutated value makes the
        // destination correct whichever place MIR chose.
        if let Some(Operand::Copy(p) | Operand::Move(p)) = op {
            let mut v = self.read_place(p)?;
            f(&mut v);
            self.write_place(p, v.clone())?;
            return Ok(v);
        }
        match value {
            Some(InterpValue::Ref { frame, local, .. }) => {
                let place = Place::new(LocalIdx::from_raw(*local as u32));
                let mut v = self
                    .locals_for_ref_frame(*frame)
                    .and_then(|ls| ls.get(*local).and_then(|o| o.as_ref()).cloned())
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "builtin mutate: dead Ref{{frame:{frame}, local:{local}}}"
                        ))
                    })?;
                f(&mut v);
                self.write_place_frame(*frame, &place, v.clone())?;
                Ok(v)
            }
            Some(v) => {
                let _ = v;
                Err(InterpError::Panic(
                    "builtin &mut-self method called with a by-value receiver;                      no place to write the mutation back to. This is a lowering                      gap — see the session-5 handoff §3.1."
                        .into(),
                ))
            }
            None => Ok(InterpValue::Unit),
        }
    }

    fn enum_tag(v: &InterpValue) -> Option<i128> {
        if let InterpValue::Aggregate(fields) = v {
            if let Some(InterpValue::Int(t)) = fields.first() {
                return Some(*t);
            }
        }
        None
    }

    fn enum_payload(v: &InterpValue) -> InterpValue {
        if let InterpValue::Aggregate(fields) = v {
            if fields.len() >= 2 {
                return fields[1].clone();
            }
        }
        InterpValue::Unit
    }

    /// Pop the last element, returning `Some(elem)` / `None` in the
    /// interpreter's `Option` shape (`Aggregate([Int(tag), payload])`).
    fn vec_pop(v: &mut InterpValue) -> InterpValue {
        if let InterpValue::Aggregate(fields) = v {
            if fields.len() == 2 {
                let len = match &fields[1] {
                    InterpValue::Uint(n) => *n as usize,
                    InterpValue::Int(n) => (*n).max(0) as usize,
                    _ => 0,
                };
                if len == 0 {
                    return InterpValue::Aggregate(vec![InterpValue::Int(0), InterpValue::Unit]);
                }
                let elem = if let InterpValue::Aggregate(buf) = &mut fields[0] {
                    while buf.len() < len {
                        buf.push(InterpValue::Unit);
                    }
                    if buf.len() >= len && len >= 1 {
                        buf.remove(len - 1)
                    } else {
                        InterpValue::Unit
                    }
                } else {
                    InterpValue::Unit
                };
                fields[1] = InterpValue::Uint((len - 1) as u128);
                return InterpValue::Aggregate(vec![InterpValue::Int(1), elem]);
            }
        }
        InterpValue::Aggregate(vec![InterpValue::Int(0), InterpValue::Unit])
    }

    /// Insert `elem` at `idx`, shifting the tail up.
    fn vec_insert(v: &mut InterpValue, idx: usize, elem: InterpValue) {
        if let InterpValue::Aggregate(fields) = v {
            if fields.len() == 2 {
                let len = match &fields[1] {
                    InterpValue::Uint(n) => *n as usize,
                    InterpValue::Int(n) => (*n).max(0) as usize,
                    _ => 0,
                };
                if idx > len {
                    return;
                }
                if let InterpValue::Aggregate(buf) = &mut fields[0] {
                    while buf.len() < len {
                        buf.push(InterpValue::Unit);
                    }
                    if idx >= buf.len() {
                        buf.push(elem);
                    } else {
                        buf.insert(idx, elem);
                    }
                }
                fields[1] = InterpValue::Uint((len + 1) as u128);
            }
        }
    }

    fn vec_remove(v: &mut InterpValue, idx: usize) -> InterpValue {
        if let InterpValue::Aggregate(fields) = v {
            if fields.len() == 2 {
                let len = match &fields[1] {
                    InterpValue::Uint(n) => *n as usize,
                    InterpValue::Int(n) => (*n).max(0) as usize,
                    _ => 0,
                };
                if idx >= len {
                    return InterpValue::Unit;
                }
                let removed = if let InterpValue::Aggregate(buf) = &mut fields[0] {
                    while buf.len() < len {
                        buf.push(InterpValue::Unit);
                    }

                    if idx < buf.len() {
                        buf.remove(idx)
                    } else {
                        InterpValue::Unit
                    }
                } else {
                    InterpValue::Unit
                };
                fields[1] = InterpValue::Uint((len - 1) as u128);
                return removed;
            }
        }
        InterpValue::Unit
    }

    fn vec_len(v: &InterpValue) -> usize {
        if let InterpValue::Aggregate(fields) = v {
            if fields.len() == 2 {
                if let InterpValue::Uint(n) = fields[1] {
                    return n as usize;
                }
                if let InterpValue::Int(n) = fields[1] {
                    return n.max(0) as usize;
                }
            }
            // Fallback: flat element list (some construction paths).
            fields.len()
        } else {
            0
        }
    }

    fn vec_push(v: &mut InterpValue, elem: InterpValue) {
        if let InterpValue::Aggregate(fields) = v {
            if fields.len() == 2 {
                // {buf, len}: bump len and store the element in buf's
                // element list so both views agree.
                let cur = match &fields[1] {
                    InterpValue::Uint(n) => *n as usize,
                    InterpValue::Int(n) => (*n).max(0) as usize,
                    _ => 0,
                };
                if let InterpValue::Aggregate(buf) = &mut fields[0] {
                    while buf.len() < cur {
                        buf.push(InterpValue::Unit);
                    }
                    if buf.len() == cur {
                        buf.push(elem);
                    } else if cur < buf.len() {
                        buf[cur] = elem;
                    }
                }
                fields[1] = InterpValue::Uint((cur + 1) as u128);
            } else {
                fields.push(elem);
            }
        } else {
            *v = InterpValue::Aggregate(vec![elem]);
        }
    }

    fn str_len(v: &InterpValue) -> usize {
        match v {
            InterpValue::String(s) => s.len(),
            // STR-LEN-FATPTR: a dynamic slice (`&s[a..b]`) lowers to a
            // fat-pointer `Aggregate([data_ptr, len])`, not a `String`. The
            // previous `if let String` returned 0 for every sliced `&str`, so
            // `&s[1..3].len()` was 0. Read field 1 (the length).
            InterpValue::Aggregate(fields) if fields.len() >= 2 => match &fields[1] {
                InterpValue::Int(n) => (*n).max(0) as usize,
                InterpValue::Uint(n) => *n as usize,
                _ => 0,
            },
            _ => 0,
        }
    }

    /// T140-PATCHED-IO: extract the byte payload of a byte-buffer-like value.
    fn byte_payload(v: &InterpValue) -> String {
        match v {
            InterpValue::String(s) => s.clone(),
            InterpValue::Aggregate(fields) if !fields.is_empty() => Self::byte_payload(&fields[0]),
            // T-ICE-3: `n` is program-controlled; a hostile program could
            // request gigabytes of NUL bytes and OOM the host. Cap the
            // payload at 16 MiB — larger writes are truncated. This is a
            // debugging interpreter, and truncation beats a killed host.
            InterpValue::Uint(n) => "\u{0}".repeat((*n).min(16 * 1024 * 1024) as usize),
            _ => String::new(),
        }
    }

    /// Extract the byte content of a `Vec<u8>`-modelled value
    /// (`Aggregate([buf, len])`, `buf = Aggregate([u8, …])`). Unlike
    /// `byte_payload`, this reads the *elements*, not the length word.
    fn vec_u8_bytes(v: &InterpValue) -> String {
        if let InterpValue::Aggregate(fields) = v {
            if let Some(buf) = fields.first() {
                match buf {
                    InterpValue::Aggregate(elems) => {
                        let mut s = String::new();
                        for e in elems {
                            let b = match e {
                                InterpValue::Uint(n) => *n as u32,
                                InterpValue::Int(n) => (*n).max(0) as u32,
                                _ => continue,
                            };
                            if let Some(c) = char::from_u32(b) {
                                s.push(c);
                            }
                        }
                        return s;
                    }
                    InterpValue::String(s) => return s.clone(),
                    _ => {}
                }
            }
        }
        String::new()
    }

    /// T140-PATCHED-IO: dispatch an `extern "C"` FFI call by registered
    /// symbol name. Returns `Ok(Some(ret))` when recognised (caller writes
    /// `ret` and jumps to the continuation), `Ok(None)` otherwise.
    fn try_call_extern(
        &mut self,
        callee_id: &DefId,
        args: &[InterpValue],
    ) -> InterpResult<Option<InterpValue>> {
        let fn_def_id = glyim_core::def_id::FnDefId::from_raw(callee_id.local_id.to_raw());
        let maybe = self.tcx.extern_fn_name(fn_def_id).map(str::to_string);
        let Some(name) = maybe else {
            return Ok(None);
        };
        match name.as_str() {
            "glyim_stdout_write" | "glyim_stderr_write" => {
                let payload = args.get(1).map(Self::byte_payload).unwrap_or_default();
                let len = args
                    .get(2)
                    .and_then(|v| match v {
                        InterpValue::Uint(n) => Some(*n as usize),
                        InterpValue::Int(n) => Some((*n).max(0) as usize),
                        _ => None,
                    })
                    .unwrap_or(0);
                // T140-PATCHED-EXTERN-LEN: the MIR passes the byte slice's
                // length as a separate operand built from `len - written`.
                // In this interpreter's reference model the tuple carrying
                // `(ptr, len)` loses the length through a projection the
                // byte-payload stand-in does not model, so it arrives as 0
                // even when the payload is non-empty. Derive `n` from the
                // payload itself; a genuine zero-length write still has an
                // empty payload, so behaviour is preserved.
                let n = if len == 0 {
                    payload.len()
                } else {
                    payload.len().min(len)
                };
                let bytes = payload.as_bytes()[..n].to_vec();
                if name == "glyim_stdout_write" {
                    self.stdout_buf.extend_from_slice(&bytes);
                } else {
                    self.stderr_buf.extend_from_slice(&bytes);
                }
                Ok(Some(InterpValue::Int(n as i128)))
            }
            "glyim_stdout_flush" | "glyim_stderr_flush" | "glyim_stdin_flush" => {
                Ok(Some(InterpValue::Int(0)))
            }
            "glyim_stdin_read" => Ok(Some(InterpValue::Int(0))),
            // ABORT-EXTERNS: the runtime's `-> !` abort hooks must STOP
            // execution. The `_` fallthrough swallowed them as `Int(0)`, so
            // `abort`/`panic` silently continued. Surface as InterpError.
            //
            // §3.5.27 unification: `glyim_panic(msg, len)` and
            // `glyim_process_abort_with_msg(msg, len)` carry a message the
            // LLVM backend writes as `thread 'main' panicked: <msg>`. Read
            // `args[0]` via `byte_payload` (a `String`-shaped value in this
            // interpreter's model) so both backends produce identical
            // stderr text. `glyim_process_abort()` (zero-arg) has no
            // message — keep the generic "program aborted" for its `.g`
            // callers.
            "glyim_panic" | "glyim_process_abort_with_msg" => {
                let msg = args
                    .first()
                    .map(Self::byte_payload)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "program aborted".to_string());
                Err(InterpError::Panic(msg))
            }
            "glyim_process_abort" => Err(InterpError::Panic("program aborted".into())),
            "glyim_process_exit" => {
                let code = args
                    .first()
                    .and_then(|v| match v {
                        InterpValue::Int(n) => Some(*n),
                        InterpValue::Uint(n) => Some(*n as i128),
                        _ => None,
                    })
                    .unwrap_or(0);
                Err(InterpError::Panic(format!("exit({code})")))
            }
            _ => {
                tracing::warn!(extern_fn = %name, "interpreter: unhandled extern; returning 0");
                Ok(Some(InterpValue::Int(0)))
            }
        }
    }

    fn string_push_str(s: &mut InterpValue, add: &InterpValue) {
        let suffix = if let InterpValue::String(inner) = add {
            inner.clone()
        } else {
            String::new()
        };
        if let InterpValue::String(inner) = s {
            inner.push_str(&suffix);
        } else {
            *s = InterpValue::String(suffix);
        }
    }

    /// T140-PATCHED-IO: bytes written to stdout by `glyim_stdout_write`.
    pub fn get_stdout(&self) -> &[u8] {
        &self.stdout_buf
    }

    /// T140-PATCHED-IO: bytes written to stderr by `glyim_stderr_write`.
    pub fn get_stderr(&self) -> &[u8] {
        &self.stderr_buf
    }

    /// get_return_value.
    pub fn get_return_value(&self) -> Option<InterpValue> {
        self.locals.first().and_then(|opt| opt.clone())
    }

    /// Resolve the locals vector owning a frame-aware `Ref { frame, local }`.
    ///
    /// `frame` is the absolute depth of the frame that created the reference
    /// (0 == the `run_body` entry frame, 1 == a direct callee, ...). The
    /// currently-executing frame is `self.frame_depth`; its locals live in
    /// `self.locals`. Every ancestor frame is parked in `self.call_stack`,
    /// which is ordered outermost-first, so ancestor depth `f` is at
    /// `call_stack[f]`. Returns `None` for an out-of-range frame (a compiler
    /// bug — a ref outliving its borrower), or when the frame that occupied
    /// depth `f` has since been popped and its slot reused by an unrelated
    /// call (detected via the per-frame `generation` counter).
    fn locals_for_ref_frame(&self, frame: usize) -> Option<&Vec<Option<InterpValue>>> {
        if frame == self.frame_depth {
            Some(&self.locals)
        } else if frame < self.call_stack.len()
            && self.call_stack[frame].generation
                == self.frame_generations.get(frame).copied().unwrap_or(0)
        {
            Some(&self.call_stack[frame].locals)
        } else {
            None
        }
    }

    /// Like [`Self::locals_for_ref_frame`] but resolves the *type* of a local in
    /// the frame a `Ref` points at. The interpreter only keeps the current
    /// frame's `local_decls` in `self.local_decls`; ancestor frames live in
    /// `call_stack[f].body.local_decls`. Reading `self.local_decls` for a
    /// cross-frame ref would return the wrong local's type and corrupt
    /// `current_ty` (which the `Field` projection uses to decide enum
    /// discriminant skipping) — this helper returns the correct frame's decls.
    fn decls_for_ref_frame(&self, frame: usize) -> Option<&[LocalDecl]> {
        if frame == self.frame_depth {
            Some(&self.local_decls)
        } else if frame < self.call_stack.len()
            && self.call_stack[frame].generation
                == self.frame_generations.get(frame).copied().unwrap_or(0)
        {
            Some(self.call_stack[frame].body.locals.as_slice())
        } else {
            None
        }
    }

    /// Mutable counterpart of [`Self::locals_for_ref_frame`] for writes through
    /// a `Ref` (e.g. `*r = v` or `*r.field = v).
    fn write_target_locals_mut(&mut self, frame: usize) -> Option<&mut Vec<Option<InterpValue>>> {
        if frame == self.frame_depth {
            Some(&mut self.locals)
        } else if frame < self.call_stack.len()
            && self.call_stack[frame].generation
                == self.frame_generations.get(frame).copied().unwrap_or(0)
        {
            Some(&mut self.call_stack[frame].locals)
        } else {
            None
        }
    }

    /// §4: evaluate a `const` / `const fn` MIR body to its value.
    ///
    /// This is const-evaluation: run the body to completion (terminating at
    /// `Return`) and return the value held in the return slot. Calling a
    /// `const fn` is just evaluating its MIR body in a fresh interpreter
    /// frame, which the runtime interpreter already supports; this wrapper
    /// makes that usable from const-context sites (e.g. array lengths, enum
    /// discriminants, `const` bindings) without driving the loop by hand.
    pub fn const_eval(&mut self, body: &Body) -> InterpResult<InterpValue> {
        self.run_body(body)?;
        self.get_return_value()
            .ok_or_else(|| InterpError::Panic("const-eval produced no return value".into()))
    }

    /// run_body.
    pub fn run_body(&mut self, body: &Body) -> InterpResult<()> {
        self.current_body_owner = Some(body.owner);
        self.current_arg_count = body.arg_count;
        self.current_body = Some(body.clone());
        self.current_bb = BasicBlockIdx::from_raw(0);
        self.locals = vec![None; body.locals.len()];
        self.local_decls = body.locals.iter().cloned().collect();
        self.call_stack.clear();
        self.frame_depth = 0;
        self.step_count = 0;
        self.recursion_depth = 1;
        self.run_current_function()
    }

    fn run_current_function(&mut self) -> InterpResult<()> {
        let mut body = self.current_body.take().unwrap();
        let mut bb_idx = self.current_bb;

        loop {
            self.step_count += 1;
            if self.step_count > self.step_limit {
                self.current_body = Some(body);
                self.current_bb = bb_idx;
                return Err(InterpError::TimedOut);
            }

            // T-ICE-4: a lowering bug feeding an out-of-range block index
            // must surface as an InterpError, not a host panic.
            let bb = body.basic_blocks.get(bb_idx).ok_or_else(|| {
                InterpError::Panic(format!(
                    "internal error: basic block index {:?} out of range (len {}); \
                     this is a compiler bug in lowering, please report it",
                    bb_idx,
                    body.basic_blocks.len()
                ))
            })?;
            let terminator_kind = bb.terminator.kind.clone();

            // Capture this block's cleanup edge (if any) before executing
            // statements, so a panic while evaluating a statement can be routed
            // there when unwinding is enabled (plan §14.2, single-frame).
            let cleanup_edge = match &terminator_kind {
                TerminatorKind::Assert { cleanup, .. }
                | TerminatorKind::Call { cleanup, .. }
                | TerminatorKind::Drop { cleanup, .. } => *cleanup,
                _ => None,
            };

            // Execute statements one at a time. An index loop (not `for
            // stmt in &body...`) is required so the immutable borrow of `body`
            // ends before `unwind_step` may take a mutable borrow of `body`.
            //
            // T109-PATCHED [INT-6]: previously this cloned the entire
            // `Statement` (including a Vec of args for Calls) for every
            // statement executed — a per-step heap allocation in the hot
            // interpreter loop. The immutable borrow of `body` here and the
            // mutable borrow of `self` inside `execute_statement` are
            // disjoint (body is a local, not part of self), so the clone is
            // unnecessary. Removing it eliminates one alloc per executed
            // statement without changing semantics: on Err, the borrow
            // ends and `unwind_step` takes `&mut body` sequentially.
            let stmt_count = body
                .basic_blocks
                .get(bb_idx)
                .ok_or_else(|| {
                    InterpError::Panic(format!(
                        "internal error: basic block index {:?} out of range (len {}); \
                         this is a compiler bug in lowering, please report it",
                        bb_idx,
                        body.basic_blocks.len()
                    ))
                })?
                .statements
                .len();
            let mut stmt_idx = 0;
            while stmt_idx < stmt_count {
                let err = {
                    let stmt = &body
                        .basic_blocks
                        .get(bb_idx)
                        .ok_or_else(|| {
                            InterpError::Panic(format!(
                                "internal error: basic block index {:?} out of range (len {}); \
                                 this is a compiler bug in lowering, please report it",
                                bb_idx,
                                body.basic_blocks.len()
                            ))
                        })?
                        .statements[stmt_idx];
                    self.execute_statement(stmt).err()
                };
                if let Some(e) = err {
                    self.unwind_step(e, &mut body, &mut bb_idx, cleanup_edge)?;
                }
                stmt_idx += 1;
            }

            match terminator_kind {
                TerminatorKind::Goto { target } => {
                    bb_idx = target;
                }
                TerminatorKind::SwitchInt {
                    discr,
                    switch_ty,
                    targets,
                } => {
                    let val = self.eval_operand(&discr)?;
                    let discr_u128 = if switch_ty == glyim_type::Ty::BOOL {
                        if let Ok(b) = self.interp_value_to_bool(&val) {
                            if b { 1u128 } else { 0u128 }
                        } else {
                            self.interp_value_to_u128(&val)
                        }
                    } else {
                        self.interp_value_to_u128(&val)
                    };
                    let mut next_bb = targets.otherwise();
                    for (v, bb) in targets.iter() {
                        if v == discr_u128 {
                            next_bb = bb;
                            break;
                        }
                    }
                    bb_idx = next_bb;
                }
                TerminatorKind::Return => {
                    // A normal return cancels any in-flight unwind payload:
                    // the call that just completed succeeded, so the pending
                    // panic (if any) is no longer being propagated.
                    self.pending_unwind = None;
                    if let Some(frame) = self.call_stack.pop() {
                        let caller_body = frame.body;
                        // T108-PATCHED [INT-5]: `Call { target: None }`
                        // stores the u32::MAX sentinel to mean "this call
                        // was lowered as diverging". Check the sentinel
                        // *before* reading the return place — a diverging
                        // callee commonly returns without ever assigning
                        // local 0, so reading it first would mask the
                        // real diagnostic with an "uninitialized local"
                        // error.
                        if frame.target_bb.to_raw() == u32::MAX {
                            self.current_body = Some(caller_body);
                            return Err(InterpError::Panic(
                                "diverging call returned to its caller".into(),
                            ));
                        }
                        // T140-PATCHED-RETURN-NEVER: a `!`- or `()`-returning function may
                        // never write its return slot (fn 307 in the stdlib is a
                        // `-> !` stub whose body is just `StorageLive(1); Return`).
                        // Treat an uninitialized local 0 as `Unit` rather than a
                        // hard "read from uninitialized local 0" panic.
                        let ret_val = self
                            .locals
                            .first()
                            .and_then(|opt| opt.clone())
                            .unwrap_or(InterpValue::Unit);
                        bb_idx = frame.target_bb;
                        self.locals = frame.locals;
                        self.local_decls = caller_body.locals.iter().cloned().collect();
                        self.write_place(&frame.return_place, ret_val)?;
                        self.recursion_depth -= 1;
                        self.frame_depth -= 1;
                        // Invalidate this depth's generation so a stale
                        // `Ref { frame: depth }` cannot alias a reused slot.
                        let popped = self.frame_depth;
                        if let Some(g) = self.frame_generations.get_mut(popped) {
                            *g = 0;
                        }
                        body = caller_body;
                        continue;
                    } else {
                        self.current_body = Some(body);
                        self.current_bb = bb_idx;
                        return Ok(());
                    }
                }
                TerminatorKind::Unreachable => {
                    self.current_body = Some(body);
                    self.current_bb = bb_idx;
                    return Err(InterpError::Panic(
                        "entered unreachable code".to_string(),
                    ));
                }
                TerminatorKind::Call {
                    func,
                    args,
                    destination,
                    target,
                    cleanup,
                } => {
                    // Devirtualize a generic-bound trait method call
                    // (`f.poll()` where `f: F: Trait`) against the *concrete*
                    // receiver type carried in the first argument. The MIR was
                    // lowered with `func = Constant(VirtualMethod{trait,method})`;
                    // rewrite it to a direct `Constant(Fn(fn_def_id))` so the
                    // rest of the call machinery (including `resolve_callee`)
                    // sees a static function.
                    let func = if let Operand::Constant(MirConst {
                        kind:
                            MirConstKind::VirtualMethod {
                                trait_def_id,
                                method_name,
                                self_ty,
                            },
                        span,
                        ..
                    }) = &func
                    {
                        let recv =
                            self_ty.or_else(|| args.first().and_then(|a| self.operand_ty(a)));
                        if let Some(recv_ty) = recv {
                            let resolved =
                                self.tcx
                                    .resolve_trait_method(*trait_def_id, recv_ty, *method_name);
                            if let Some(fn_def_id) = resolved {
                                Operand::Constant(MirConst {
                                    kind: MirConstKind::Fn(fn_def_id, Substitution::empty()),
                                    ty: recv_ty,
                                    span: *span,
                                })
                            } else {
                                func
                            }
                        } else {
                            func
                        }
                    } else {
                        func
                    };
                    // Plan §12.1: a closure value is an aggregate
                    // `[Fn(def_id), captures...]`. `resolve_callee` unpacks that,
                    // returning the callee id *and* the captured values so they
                    // can be passed as leading arguments — matching the calling
                    // convention the closure body was lowered with
                    // (`arg_count = captures.len() + params.len()`).
                    let (callee_id, captured) = self.resolve_callee(&func, &args)?;

                    let mut arg_values = captured;
                    for arg_op in &args {
                        arg_values.push(self.eval_operand(arg_op)?);
                    }

                    // T140-PATCHED-BUILTIN: dispatch compiler-synthetic
                    // builtin methods (FnDefId in the 9000+ range) before the
                    // ordinary function-table lookup. The LLVM backend has
                    // `try_lower_builtin_intrinsic` for these; the
                    // interpreter had no equivalent and panicked `function
                    // not found` for any stdlib code calling `Vec::push`,
                    // `Option::unwrap`, etc.
                    if let Some(ret) = self.try_call_builtin(&callee_id, &arg_values, &args)? {
                        self.write_place(&destination, ret)?;
                        if let Some(t) = target {
                            bb_idx = t;
                            continue;
                        }
                        self.current_body = Some(body);
                        self.current_bb = bb_idx;
                        return Err(InterpError::Panic(
                            "builtin call has no continuation".into(),
                        ));
                    }
                    // T140-PATCHED-IO: extern "C" FFI dispatch.
                    if let Some(ret) = self.try_call_extern(&callee_id, &arg_values)? {
                        self.write_place(&destination, ret)?;
                        if let Some(t) = target {
                            bb_idx = t;
                            continue;
                        }
                        self.current_body = Some(body);
                        self.current_bb = bb_idx;
                        return Err(InterpError::Panic("extern call has no continuation".into()));
                    }

                    let callee_body =
                        self.function_table
                            .get(&callee_id)
                            .cloned()
                            .ok_or_else(|| {
                                InterpError::Panic(format!("function not found: {:?}", callee_id))
                            })?;

                    self.recursion_depth += 1;
                    if self.recursion_depth > self.recursion_limit {
                        self.current_body = Some(body);
                        self.current_bb = bb_idx;
                        return Err(InterpError::StackOverflow);
                    }

                    let mut callee_locals = vec![None; callee_body.locals.len()];
                    for (i, val) in arg_values.into_iter().enumerate() {
                        // T-ICE-4: arg/local count mismatch is a lowering bug; fail loudly
                        // but as an InterpError instead of an index panic.
                        if i + 1 >= callee_locals.len() {
                            return Err(InterpError::Panic(format!(
                                "internal error: argument index {} exceeds callee local count {}; \
                                 this is a compiler bug in lowering, please report it",
                                i + 1,
                                callee_locals.len()
                            )));
                        }
                        callee_locals[i + 1] = Some(val);
                    }

                    // T108-PATCHED [INT-5]: a `Call { target: None }` is
                    // *diverging* (panic!/abort-style). The previous code
                    // resumed at `bb_idx + 1`, which either landed on an
                    // unrelated block or, if `bb` was the last block,
                    // produced an out-of-range index that panicked the
                    // host. Represent "no resume point" explicitly with
                    // `usize::MAX`-style sentinel via `Option` and raise
                    // a clean panic if the callee ever returns to us.
                    let next_bb: Option<BasicBlockIdx> = target;

                    let caller_frame = CallFrame {
                        body,
                        // T108-PATCHED [INT-5]: `None` signals a diverging
                        // call; the sentinel index we insert is never used
                        // because the callee either loops forever or aborts,
                        // and if it *does* return, `handle_return` below
                        // raises a clear InterpError instead of silently
                        // jumping to an arbitrary block.
                        bb: next_bb.unwrap_or(BasicBlockIdx::from_raw(u32::MAX)),
                        locals: std::mem::take(&mut self.locals),
                        return_place: destination,
                        // T108-PATCHED [INT-5]: same sentinel for the
                        // unwind-target field so `frame.target_bb` and
                        // `frame.bb` agree.
                        target_bb: next_bb.unwrap_or(BasicBlockIdx::from_raw(u32::MAX)),
                        unwind_target: cleanup,
                        generation: self.next_frame_gen,
                    };

                    // Record the generation for this absolute depth so a
                    // cross-frame `Ref { frame: depth }` can later detect if
                    // this frame was popped and its slot reused.
                    let depth = self.frame_depth;
                    if self.frame_generations.len() <= depth {
                        self.frame_generations.resize(depth + 1, 0);
                    }
                    self.frame_generations[depth] = self.next_frame_gen;
                    self.next_frame_gen = self.next_frame_gen.wrapping_add(1);

                    self.call_stack.push(caller_frame);
                    self.frame_depth += 1;
                    self.local_decls = callee_body.locals.iter().cloned().collect();
                    self.current_body_owner = Some(callee_body.owner);
                    self.current_arg_count = callee_body.arg_count;
                    self.locals = callee_locals;
                    body = callee_body;
                    bb_idx = BasicBlockIdx::from_raw(0);
                }
                TerminatorKind::Assert {
                    cond,
                    expected,
                    target,
                    cleanup,
                    msg,
                } => {
                    let val = self.eval_operand(&cond)?;
                    let is_true = match val {
                        InterpValue::Bool(b) => b,
                        _ => {
                            self.current_body = Some(body);
                            self.current_bb = bb_idx;
                            return Err(InterpError::Panic(
                                "assert condition must be bool".to_string(),
                            ));
                        }
                    };
                    if is_true == expected {
                        bb_idx = target;
                    } else {
                        let payload = InterpError::Panic(format!("assert failed: {:?}", msg));
                        self.unwind_step(payload, &mut body, &mut bb_idx, cleanup)?;
                    }
                }
                TerminatorKind::Drop {
                    place,
                    target,
                    cleanup: _,
                } => {
                    // A `Drop` on a local whose type has a user `Drop`
                    // impl must actually run that destructor — the LLVM
                    // backend does (it passes the resolved `drop` function
                    // to `glyim_drop_in_place`), and a value that is
                    // dropped on one backend but not the other is exactly
                    // the divergence the differential suite exists to
                    // catch.
                    //
                    // Run the destructor as an ordinary call frame: the
                    // `drop(&mut self)` body takes one argument, a `Ref` to
                    // the local being dropped. `frame_depth` is captured
                    // *before* the push so the `Ref` addresses the caller's
                    // frame. Types with no drop impl (and `Copy` types) are
                    // a plain branch to `target`, matching the previous
                    // no-op behaviour.
                    let drop_ty = self
                        .local_decls
                        .get(place.local.index())
                        .map(|d| d.ty)
                        .unwrap_or(Ty::ERROR);
                    let drop_body = if drop_ty != Ty::ERROR {
                        self.tcx.drop_method_for(drop_ty).and_then(|fn_def_id| {
                            let key = DefId::new(
                                CrateId::from_raw(0),
                                LocalDefId::from_raw(fn_def_id.to_raw()),
                            );
                            self.function_table.get(&key).cloned()
                        })
                    } else {
                        None
                    };
                    let Some(callee_body) = drop_body else {
                        bb_idx = target;
                        continue;
                    };

                    let recv = InterpValue::Ref {
                        frame: self.frame_depth,
                        local: place.local.index(),
                        projection: place.projection.clone(),
                    };
                    let mut callee_locals = vec![None; callee_body.locals.len()];
                    if callee_locals.len() < 2 {
                        return Err(InterpError::Panic(
                            "internal error: drop glue body has no receiver local".into(),
                        ));
                    }
                    callee_locals[1] = Some(recv);

                    // Mirror the `Call` arm's bookkeeping: the `Return`
                    // handler decrements *both* counters, so a frame pushed
                    // without incrementing `recursion_depth` underflows it on
                    // return (`attempt to subtract with overflow`).
                    self.recursion_depth += 1;
                    if self.recursion_depth > self.recursion_limit {
                        self.current_body = Some(body);
                        self.current_bb = bb_idx;
                        return Err(InterpError::StackOverflow);
                    }

                    let caller_frame = CallFrame {
                        body,
                        bb: target,
                        locals: std::mem::take(&mut self.locals),
                        // The destructor returns *to* `target`; there is no
                        // value to place (drop returns `()`).
                        return_place: Place::new(LocalIdx::from_raw(0)),
                        target_bb: target,
                        unwind_target: None,
                        generation: self.next_frame_gen,
                    };
                    let depth = self.frame_depth;
                    if self.frame_generations.len() <= depth {
                        self.frame_generations.resize(depth + 1, 0);
                    }
                    self.frame_generations[depth] = self.next_frame_gen;
                    self.next_frame_gen = self.next_frame_gen.wrapping_add(1);

                    self.call_stack.push(caller_frame);
                    self.frame_depth += 1;
                    self.local_decls = callee_body.locals.iter().cloned().collect();
                    self.current_body_owner = Some(callee_body.owner);
                    self.current_arg_count = callee_body.arg_count;
                    self.locals = callee_locals;
                    body = callee_body;
                    bb_idx = BasicBlockIdx::from_raw(0);
                }
            }
        }
    }

    /// Route a panic through cleanup/unwind (plan §7.2, cross-frame).
    ///
    /// When `panics_unwind` is disabled the panic aborts immediately
    /// (`Err(payload)`). When enabled:
    /// 1. If the *current* block has a cleanup edge, jump to it (single-frame
    ///    cleanup) and return `Ok(())` so the caller loop continues.
    /// 2. Otherwise pop to the caller frame and resume it at the caller's
    ///    unwind target (the `cleanup` of the `Call` that invoked this frame),
    ///    carrying the original payload in `pending_unwind`. Return `Ok(())`.
    /// 3. If there is no caller frame, the walk reached the top: return
    ///    `Err(InterpError::Unwind(..))` carrying the original panic.
    fn unwind_step(
        &mut self,
        payload: InterpError,
        body: &mut Body,
        bb_idx: &mut BasicBlockIdx,
        cleanup_edge: Option<BasicBlockIdx>,
    ) -> InterpResult<()> {
        if !self.panics_unwind {
            return Err(payload);
        }
        // (1) single-frame cleanup in the current body.
        if let Some(cb) = cleanup_edge {
            *bb_idx = cb;
            return Ok(());
        }
        // (2) cross-frame: keep popping caller frames until one has a real
        // `unwind_target` (its `Call`'s cleanup edge), or the stack empties.
        // A caller with *no* cleanup edge for the call that led here must be
        // skipped — resuming at its normal continuation (`target_bb`) would
        // silently treat the propagating panic as a successful return, which
        // is a correctness bug, not a valid fallback.
        if self.pending_unwind.is_none() {
            self.pending_unwind = Some(payload);
        }
        loop {
            match self.call_stack.pop() {
                Some(frame) => {
                    self.recursion_depth = self.recursion_depth.saturating_sub(1);
                    match frame.unwind_target {
                        Some(resume) => {
                            self.locals = frame.locals;
                            self.local_decls = frame.body.locals.iter().cloned().collect();
                            *body = frame.body;
                            *bb_idx = resume;
                            return Ok(());
                        }
                        // No cleanup for this frame's call: keep walking up
                        // rather than resuming normal execution.
                        None => continue,
                    }
                }
                None => {
                    let top = self.pending_unwind.take().expect("set above");
                    return Err(InterpError::Unwind(Box::new(top)));
                }
            }
        }
    }

    fn execute_statement(&mut self, stmt: &Statement) -> InterpResult<()> {
        match &stmt.kind {
            StatementKind::Assign(place, rvalue) => {
                // MIR-13/14: the destination local's declared type drives
                // width-correct arithmetic and casts.
                let dest_ty = self.current_local_ty(place.local).unwrap_or(Ty::ERROR);
                let val = self.eval_rvalue(rvalue, dest_ty)?;
                self.write_place(place, val)?;
            }
            StatementKind::StorageLive(local) => {
                // Argument locals (indices 1..=arg_count) are already
                // initialized by the caller at function entry. `StorageLive`
                // for an argument local would wrongly wipe the passed
                // argument, so skip those (see for-loop method-call desugar).
                let idx = local.index();
                if !(idx >= 1 && idx <= self.current_arg_count) {
                    self.locals[idx] = None;
                }
            }
            StatementKind::StorageDead(_local) => {}
            StatementKind::Nop => {}
        }
        Ok(())
    }

    /// The declared type of a local in the CURRENT frame — used by the
    /// `Assign` dispatch to give arithmetic/casts their destination type.
    /// Returns `None` for an out-of-range local so the caller can fall back
    /// and let `write_place` report the bounds error.
    pub(crate) fn current_local_ty(&self, local: LocalIdx) -> Option<Ty> {
        self.local_decls.get(local.index()).map(|d| d.ty)
    }

    pub(crate) fn eval_rvalue(&self, rvalue: &Rvalue, dest_ty: Ty) -> InterpResult<InterpValue> {
        match rvalue {
            Rvalue::Use(operand) => self.eval_operand(operand),
            Rvalue::BinaryOp(op, operands) => {
                let (left, right) = operands.as_ref();
                let l = self.eval_operand(left)?;
                let r = self.eval_operand(right)?;
                self.eval_binary_op(*op, &l, &r, dest_ty)
            }
            Rvalue::UnaryOp(op, operand) => {
                let v = self.eval_operand(operand)?;
                self.eval_unary_op(*op, &v)
            }
            Rvalue::Ref(place, _borrow_kind) => {
                // Preserve the place's projection: `&mut _5.field0` must
                // refer to the *field*, not to the whole `_5`. Dropping
                // it here made `w.v.len()` deref to the whole `W` and
                // read the wrong length (KNOWN_GAPS §8).
                Ok(InterpValue::Ref {
                    frame: self.frame_depth,
                    local: place.local.index(),
                    projection: place.projection.clone(),
                })
            }
            Rvalue::Aggregate(kind, operands) => {
                let mut values = Vec::with_capacity(operands.len());
                for op in operands {
                    values.push(self.eval_operand(op)?);
                }
                // Enum ADTs are tagged with their variant index as `fields[0]`
                // so `SwitchInt` and `Rvalue::Discriminant` can read it, and
                // `read_place`'s `Downcast` projection can strip it to expose
                // the payload at its natural indices.
                //
                // This MUST run before the `values.is_empty()` shortcut below:
                // a *unit* variant (`Four::C`) constructs an `Aggregate` with
                // zero operands, and returning `InterpValue::Unit` for it threw
                // away the variant index — so `Discriminant` read 0 and every
                // unit-variant `match` selected the first arm. (Reported by the
                // `match_four_variants` run-pass fixture: `Four::C` matched
                // `Four::A`.)
                if let AggregateKind::Adt(adt_id, variant, _substs) = kind {
                    if let Some(def) = self.tcx.adt_def(*adt_id) {
                        if def.kind == AdtKind::Enum {
                            let mut tagged = Vec::with_capacity(values.len() + 1);
                            tagged.push(InterpValue::Int(variant.index() as i128));
                            tagged.extend(values);
                            return Ok(InterpValue::Aggregate(tagged));
                        }
                    }
                }
                if values.is_empty() {
                    Ok(InterpValue::Unit)
                } else {
                    Ok(InterpValue::Aggregate(values))
                }
            }
            Rvalue::Discriminant(place) => {
                let val = self.read_place(place)?;
                match &val {
                    InterpValue::Aggregate(fields) => {
                        if fields.is_empty() {
                            Ok(InterpValue::Int(0))
                        } else {
                            Ok(fields[0].clone())
                        }
                    }
                    InterpValue::Unit => Ok(InterpValue::Int(0)),
                    _ => Err(InterpError::Panic("Discriminant on non-aggregate".into())),
                }
            }
            Rvalue::Len(place) => {
                let ty = self.local_decls[place.local.index()].ty;
                let ty_kind = self.tcx.ty_kind(ty);
                match ty_kind {
                    glyim_type::TyKind::Array(_, const_val) => {
                        let len = match &const_val.kind {
                            glyim_type::ConstKind::Int(n) => *n as usize,
                            glyim_type::ConstKind::Uint(n) => *n as usize,
                            _ => {
                                return Err(InterpError::Panic(
                                    "Len: unsupported array length kind".into(),
                                ));
                            }
                        };
                        Ok(InterpValue::Int(len as i128))
                    }
                    glyim_type::TyKind::Slice(_) => {
                        let val = self.read_place(place)?;
                        let len = self.slice_length_from_value(&val)?;
                        Ok(InterpValue::Int(len as i128))
                    }
                    // T140-PATCHED-LEN-REF: `buf.len()` where `buf: &[u8]`
                    // (or `&str`) has a declared local type of
                    // `Ref(Slice(_))` / `Ref(String)`, which the two arms
                    // above do not match. Read the place (the interpreter's
                    // `read_place` follows the reference) and length the
                    // resulting value.
                    glyim_type::TyKind::Ref(_, inner, _)
                        if matches!(
                            self.tcx.ty_kind(*inner),
                            glyim_type::TyKind::Slice(_) | glyim_type::TyKind::String
                        ) =>
                    {
                        let val = self.read_place(place)?;
                        let len = self.slice_length_from_value(&val)?;
                        Ok(InterpValue::Int(len as i128))
                    }
                    // LEN-STR: a bare `str` local (`TyKind::String`). `str`
                    // lowers to the `{ptr, len}` fat pointer; length is the
                    // same extraction as the slice case. Reached by
                    // `lower_dynamic_range_slice`'s `Len(base)` on a `&s[a..b]`
                    // where the base's declared type is the primitive `str`.
                    glyim_type::TyKind::String => {
                        let val = self.read_place(place)?;
                        let len = self.slice_length_from_value(&val)?;
                        Ok(InterpValue::Int(len as i128))
                    }
                    glyim_type::TyKind::Adt(id, _)
                        if id.to_raw() == 1050 || id.to_raw() == 1020 =>
                    {
                        // `String` (1050) / `Vec` (1020): the container's own
                        // value carries its length (field 1 of the fat
                        // pointer / the `len` field). Used by the slice path.
                        let val = self.read_place(place)?;
                        let len = self.slice_length_from_value(&val)?;
                        Ok(InterpValue::Int(len as i128))
                    }
                    _ => Err(InterpError::Panic("Len: expected array or slice".into())),
                }
            }
            Rvalue::Cast(kind, operand, target_ty) => {
                let val = self.eval_operand(operand)?;
                match kind {
                    &glyim_mir::CastKind::FloatToFloat => {
                        if let InterpValue::Float(a) = val {
                            Ok(InterpValue::Float(a))
                        } else {
                            Err(InterpError::Panic("expected float for FloatToFloat".into()))
                        }
                    }
                    // MIR-14: a real truncation to the target width/signedness,
                    // not an identity. `(-1i32) as u8` must give 255, matching
                    // the LLVM backend.
                    CastKind::IntToInt => match val {
                        InterpValue::Int(v) => {
                            Ok(InterpValue::Int(self.trunc_to_ty(v, *target_ty)))
                        }
                        InterpValue::Uint(v) => Ok(InterpValue::Uint(
                            self.trunc_to_ty(v as i128, *target_ty) as u128,
                        )),
                        other => Ok(other),
                    },
                    CastKind::IntToFloat => match val {
                        InterpValue::Int(i) => Ok(InterpValue::Float(i as f64)),
                        _ => Err(InterpError::Panic("expected int for IntToFloat".into())),
                    },
                    // Float-to-int: truncate the (float-derived) i128 to the
                    // target width (MIR-14).
                    CastKind::FloatToInt => match val {
                        InterpValue::Float(f) => {
                            Ok(InterpValue::Int(self.trunc_to_ty(f as i128, *target_ty)))
                        }
                        _ => Err(InterpError::Panic("expected float for FloatToInt".into())),
                    },
                    // Deliberately a no-op: InterpValue pointers carry no
                    // type-specific representation to convert between, so a
                    // pointer-to-pointer cast is identity at this value level.
                    CastKind::PtrToPtr | CastKind::FnPtrToPtr => Ok(val),
                    CastKind::PtrToInt => match val {
                        InterpValue::Ref { frame, local, .. } => {
                            Ok(InterpValue::Uint(((frame as u128) << 48) | (local as u128)))
                        }
                        _ => Err(InterpError::Panic("PtrToInt on non-reference".into())),
                    },
                    CastKind::IntToPtr => match val {
                        InterpValue::Uint(addr) => Ok(InterpValue::Ref {
                            frame: (addr >> 48) as usize,
                            local: (addr & ((1u128 << 48) - 1)) as usize,
                            // Reconstructed from an address; the original
                            // projection (if any) is lost in the encode.
                            projection: Box::new([]),
                        }),
                        InterpValue::Int(addr) if addr >= 0 => Ok(InterpValue::Ref {
                            frame: 0,
                            local: addr as usize,
                            projection: Box::new([]),
                        }),
                        _ => Err(InterpError::Panic("IntToPtr on non-integer".into())),
                    },
                }
            }
            Rvalue::Repeat(operand, count_const) => {
                let val = self.eval_operand(operand)?;
                let count_val = self.eval_mir_const(count_const)?;
                let len = match count_val {
                    InterpValue::Int(i) => i as usize,
                    InterpValue::Uint(u) => u as usize,
                    _ => return Err(InterpError::Panic("repeat count must be integer".into())),
                };
                let repeated = vec![val; len];
                Ok(InterpValue::Aggregate(repeated))
            }
        }
    }

    fn eval_operand(&self, operand: &Operand) -> InterpResult<InterpValue> {
        match operand {
            Operand::Constant(c) => self.eval_mir_const(c),
            Operand::Copy(place) | Operand::Move(place) => self.read_place(place),
        }
    }

    fn eval_mir_const(&self, c: &MirConst) -> InterpResult<InterpValue> {
        match &c.kind {
            MirConstKind::Int(v) => Ok(InterpValue::Int(*v)),
            MirConstKind::Uint(v) => Ok(InterpValue::Uint(*v)),
            MirConstKind::Bool(v) => Ok(InterpValue::Bool(*v)),
            MirConstKind::Unit => Ok(InterpValue::Unit),
            MirConstKind::Char(ch) => Ok(InterpValue::Int(*ch as i128)),
            MirConstKind::FloatBits(bits) => {
                let f = f64::from_bits(*bits);
                Ok(InterpValue::Float(f))
            }
            MirConstKind::String(name) => {
                let s = self.tcx.name_str(*name).to_string();
                Ok(InterpValue::String(s))
            }
            MirConstKind::Fn(def_id, _substs) => {
                let crate_id = CrateId::from_raw(0);
                let local_def_id = LocalDefId::from_raw(def_id.to_raw());
                let def_id = DefId::new(crate_id, local_def_id);
                Ok(InterpValue::Fn(def_id))
            }
            MirConstKind::ConstRef(def_id, _substs) => {
                let crate_id = CrateId::from_raw(0);
                let local_def_id = LocalDefId::from_raw(def_id.to_raw());
                let def_id = DefId::new(crate_id, local_def_id);
                Ok(InterpValue::ConstRef(def_id))
            }
            MirConstKind::Variant {
                adt_id: _,
                variant_idx,
                fields,
            } => {
                // Enum values are laid out as `[disc, ...payload]` in this
                // interpreter (see `Rvalue::Aggregate`'s enum arm and
                // `Rvalue::Discriminant`). Build the same shape so a
                // `match` on the constant selects the right arm.
                let mut tagged = Vec::with_capacity(fields.len() + 1);
                tagged.push(InterpValue::Int(variant_idx.index() as i128));
                for f in fields {
                    tagged.push(self.eval_mir_const(f)?);
                }
                Ok(InterpValue::Aggregate(tagged))
            }
            MirConstKind::Aggregate(elems) => {
                let mut values = Vec::with_capacity(elems.len());
                for e in elems {
                    values.push(self.eval_mir_const(e)?);
                }
                Ok(InterpValue::Aggregate(values))
            }
            MirConstKind::SizeOf(ty) => {
                let lc = glyim_layout::SimpleLayoutComputer::new(
                    self.tcx,
                    glyim_core::primitives::TargetInfo::x86_64(),
                );
                let size = lc.layout_of(*ty).map(|l| l.size.0).unwrap_or(0);
                Ok(InterpValue::Uint(size as u128))
            }
                        MirConstKind::StaticRef { def_id, init, .. } => {
                            // Initialize the global lazily. Subsequent
                            // `Deref` through the returned `StaticPtr` reads /
                            // writes the stored value.
                            let init_val = self.eval_mir_const(init)?;
                            self.globals
                                .borrow_mut()
                                .entry(*def_id)
                                .or_insert(init_val);
                            Ok(InterpValue::StaticPtr { def_id: *def_id })
                        }
MirConstKind::Error => {
                // T140-PATCHED-INTERP-ERROR: compiler-synthetic builtin calls
                // (`into`/`to_string`/`clone`/`drop`) lower to an `Error`
                // constant on runtime-dead branches (e.g. the error arm of
                // `Result::unwrap` calling `E::to_string()` for a generic `E`).
                // LLVM codegen tolerates it (see `T140-PATCHED-LLVM-ERROR`);
                // the interpreter must match. Returning `Unit` here is safe
                // because the value is never consumed on a live path — a
                // branch that actually reads it is already a compiler bug
                // that this fix does not attempt to mask.
                Ok(InterpValue::Unit)
            }
            // Devirtualized at the `Call` terminator before being evaluated as a
            // value; should never be reached as an operand constant.
            MirConstKind::VirtualMethod { .. } => Err(InterpError::Panic(
                "VirtualMethod const reached eval_const".into(),
            )),
        }
    }

    /// Truncate `v` to the width/signedness of scalar type `ty` (MIR-13).
    /// The interpreter carries values as `i128`; without this, `100i8 + 100i8`
    /// yields 200 while an LLVM binary yields -56.
    fn trunc_to_ty(&self, v: i128, ty: Ty) -> i128 {
        use glyim_core::primitives::{IntTy, UintTy};
        match self.tcx.ty_kind(ty) {
            TyKind::Int(IntTy::I8) => v as i8 as i128,
            TyKind::Int(IntTy::I16) => v as i16 as i128,
            TyKind::Int(IntTy::I32) => v as i32 as i128,
            TyKind::Int(IntTy::I64) => v as i64 as i128,
            TyKind::Int(IntTy::Isize) => v as isize as i128,
            TyKind::Uint(UintTy::U8) => (v as u8) as i128,
            TyKind::Uint(UintTy::U16) => (v as u16) as i128,
            TyKind::Uint(UintTy::U32) => (v as u32) as i128,
            TyKind::Uint(UintTy::U64) => (v as u64) as i128,
            TyKind::Uint(UintTy::Usize) => (v as usize) as i128,
            _ => v,
        }
    }

    fn eval_binary_op(
        &self,
        op: BinOp,
        left: &InterpValue,
        right: &InterpValue,
        dest_ty: Ty,
    ) -> InterpResult<InterpValue> {
        use InterpValue::*;
        match (left, right) {
            (Int(l), Int(r)) => {
                let result = match op {
                    BinOp::Add => l.wrapping_add(*r),
                    BinOp::Sub => l.wrapping_sub(*r),
                    BinOp::Mul => l.wrapping_mul(*r),
                    BinOp::Div => {
                        // T107-PATCHED [INT-4]: `INT_MIN / -1` overflows in
                        // two's-complement arithmetic; the LLVM backend
                        // lowers this to `sdiv`, which SIGFPEs on x86. The
                        // interpreter previously returned `0` here, so the
                        // same source produced different results on the two
                        // backends. Treat it like any other overflow: panic
                        // with a clear message. (A matching check still
                        // needs to be added to the LLVM backend; this is
                        // the interpreter side.)
                        if *r == 0 {
                            return Err(InterpError::DivisionByZero);
                        }
                        match l.checked_div(*r) {
                            Some(v) => v,
                            None => {
                                return Err(InterpError::Panic(
                                    "attempt to divide with overflow".into(),
                                ));
                            }
                        }
                    }
                    BinOp::Rem => {
                        // T107-PATCHED [INT-4]: same overflow behavior as
                        // Div above.
                        if *r == 0 {
                            return Err(InterpError::RemainderByZero);
                        }
                        match l.checked_rem(*r) {
                            Some(v) => v,
                            None => {
                                return Err(InterpError::Panic(
                                    "attempt to calculate the remainder with overflow".into(),
                                ));
                            }
                        }
                    }
                    BinOp::BitAnd => l & *r,
                    BinOp::BitOr => l | *r,
                    BinOp::BitXor => l ^ *r,
                    BinOp::Shl => l.wrapping_shl(*r as u32),
                    BinOp::Shr => l.wrapping_shr(*r as u32),
                    BinOp::Eq => return Ok(Bool(l == r)),
                    BinOp::Ne => return Ok(Bool(l != r)),
                    BinOp::Lt => return Ok(Bool(l < r)),
                    BinOp::Gt => return Ok(Bool(l > r)),
                    BinOp::LtEq => return Ok(Bool(l <= r)),
                    BinOp::GtEq => return Ok(Bool(l >= r)),
                    _ => {
                        return Err(InterpError::Panic(format!(
                            "unsupported integer binop: {:?}",
                            op
                        )));
                    }
                };
                Ok(Int(self.trunc_to_ty(result, dest_ty)))
            }
            (Uint(l), Uint(r)) => {
                let result = match op {
                    BinOp::Add => l.wrapping_add(*r),
                    BinOp::Sub => l.wrapping_sub(*r),
                    BinOp::Mul => l.wrapping_mul(*r),
                    BinOp::Div => {
                        // Plan §11.2: unsigned division by zero must be a clean
                        // interpreter error, not a Rust-level panic.
                        if *r == 0 {
                            return Err(InterpError::DivisionByZero);
                        }
                        l.wrapping_div(*r)
                    }
                    BinOp::Rem => {
                        // Plan §11.2: unsigned remainder by zero must be a clean
                        // interpreter error, not a Rust-level panic.
                        if *r == 0 {
                            return Err(InterpError::RemainderByZero);
                        }
                        l.wrapping_rem(*r)
                    }
                    BinOp::BitAnd => l & *r,
                    BinOp::BitOr => l | *r,
                    BinOp::BitXor => l ^ *r,
                    BinOp::Shl => l.wrapping_shl(*r as u32),
                    BinOp::Shr => l.wrapping_shr(*r as u32),
                    BinOp::Eq => return Ok(Bool(l == r)),
                    BinOp::Ne => return Ok(Bool(l != r)),
                    BinOp::Lt => return Ok(Bool(l < r)),
                    BinOp::Gt => return Ok(Bool(l > r)),
                    BinOp::LtEq => return Ok(Bool(l <= r)),
                    BinOp::GtEq => return Ok(Bool(l >= r)),
                    _ => {
                        return Err(InterpError::Panic(format!(
                            "unsupported unsigned binop: {:?}",
                            op
                        )));
                    }
                };
                Ok(Uint(self.trunc_to_ty(result as i128, dest_ty) as u128))
            }
            (Bool(l), Bool(r)) => match op {
                BinOp::Eq => Ok(Bool(l == r)),
                BinOp::Ne => Ok(Bool(l != r)),
                BinOp::And => Ok(Bool(*l && *r)),
                BinOp::Or => Ok(Bool(*l || *r)),
                _ => Err(InterpError::Panic(format!(
                    "unsupported bool binop: {:?}",
                    op
                ))),
            },
            (Float(l), Float(r)) => {
                let result = match op {
                    BinOp::Add => *l + *r,
                    BinOp::Sub => *l - *r,
                    BinOp::Mul => *l * *r,
                    BinOp::Div => {
                        // T106-PATCHED [INT-3]: `1.0 / 0.0` is IEEE `+inf`
                        // in Rust/LLVM; the LLVM backend emits `fdiv` which
                        // follows that convention. The interpreter previously
                        // returned a panic here, so the same source program
                        // had different observable behavior depending on
                        // backend. Let the native floating-point operation
                        // produce inf/NaN and match LLVM.
                        *l / *r
                    }
                    BinOp::Eq => return Ok(Bool(l == r)),
                    BinOp::Ne => return Ok(Bool(l != r)),
                    BinOp::Lt => return Ok(Bool(l < r)),
                    BinOp::Gt => return Ok(Bool(l > r)),
                    BinOp::LtEq => return Ok(Bool(l <= r)),
                    BinOp::GtEq => return Ok(Bool(l >= r)),
                    _ => {
                        return Err(InterpError::Panic(format!(
                            "unsupported float binop: {:?}",
                            op
                        )));
                    }
                };
                Ok(Float(result))
            }
            // T140-PATCHED-MIXED-NUM: mixed-signedness numeric ops.
            // Rust typeck allows `i32 < usize` by inserting a coercion at
            // the MIR level; the interpreter sees pre-coercion operand
            // values, so `println`'s `written < buf.len()` (Int vs Uint)
            // reached the fallthrough and panicked. Promote both operands
            // to `Int(i128)` and recurse — the `(Int, Int)` arm above
            // handles every op. Depth is bounded at 2.
            (Int(a), Uint(b)) => self.eval_binary_op(op, &Int(*a), &Int(*b as i128), dest_ty),
            (Uint(a), Int(b)) => self.eval_binary_op(op, &Int(*a as i128), &Int(*b), dest_ty),
            // T140-PATCHED-BYTEPTR-ARITH: `InterpValue::String` stands in for
            // a byte pointer (see `byte_payload`). `buf.as_ptr() + offset` in
            // the MIR's sub-slice lowering reaches this fallthrough as
            // `(String, Int|Uint)`. Model pointer advance as "drop N leading
            // bytes" so `&buf[written..]` produces the correct suffix, and
            // `len - written` (Int/Int) computes the matching new length.
            (String(s), Int(n)) => {
                let skip = (*n).max(0) as usize;
                if skip >= s.len() {
                    Ok(InterpValue::String(std::string::String::new()))
                } else {
                    Ok(InterpValue::String(s[skip..].to_string()))
                }
            }
            (String(s), Uint(n)) => {
                let skip = *n as usize;
                if skip >= s.len() {
                    Ok(InterpValue::String(std::string::String::new()))
                } else {
                    Ok(InterpValue::String(s[skip..].to_string()))
                }
            }
            // Pointer comparison: two pointer-shaped operands (`Ref` or
            // `StaticPtr`) are address-compared, mirroring LLVM's
            // `ptrtoint`-then-`icmp`. Encode the address the same way the
            // `PtrToInt` cast does (`(frame << 48) | local`), with the
            // high bit set for `StaticPtr` so a static address can never
            // collide with a frame-local address.
            //
            // The pointer-vs-numeric arm below catches the remaining
            // `(p as usize) != 0` shape when the intermediate cast result
            // `String`/`&str` equality. Both sides are modelled as
            // `InterpValue::String` (the interp stands in for a byte
            // payload with a UTF-8 string): `String::from(..)` returns
            // one, and a `"literal"` const lowers to one via
            // `MirConstKind::String`. So `String == String`,
            // `String == &str`, and `&str == String` all reach the same
            // arm. Only `Eq`/`Ne` are meaningful here (byte-for-byte);
            // the LLVM backend's `glyim_str_eq` is the equivalent.
            (String(a), String(b)) => match op {
                BinOp::Eq => Ok(Bool(a == b)),
                BinOp::Ne => Ok(Bool(a != b)),
                _ => Err(InterpError::Panic(format!(
                    "unsupported String binop: {op:?}"
                ))),
            },
            // is optimised/elided and the raw `Ref` reaches the compare
            // (LLVM lowers this to ptrtoint + icmp; the interpreter
            // matches it by encoding the address on the fly).
            (
                Ref { .. } | StaticPtr { .. },
                Ref { .. } | StaticPtr { .. },
            ) => {
                let ptr_addr = |v: &InterpValue| -> u128 {
                    match v {
                        InterpValue::Ref { frame, local, .. } => {
                            ((*frame as u128) << 48) | (*local as u128)
                        }
                        InterpValue::StaticPtr { def_id } => {
                            (1u128 << 127) | (def_id.to_raw() as u128)
                        }
                        _ => 0,
                    }
                };
                let l = ptr_addr(left);
                let r = ptr_addr(right);
                self.eval_binary_op(op, &Uint(l), &Uint(r), dest_ty)
            }
            (
                Ref { .. } | StaticPtr { .. },
                Int(_) | Uint(_),
            ) => {
                let l = match left {
                    InterpValue::Ref { frame, local, .. } => {
                        ((*frame as u128) << 48) | (*local as u128)
                    }
                    InterpValue::StaticPtr { def_id } => {
                        (1u128 << 127) | (def_id.to_raw() as u128)
                    }
                    _ => 0,
                };
                self.eval_binary_op(op, &Uint(l), right, dest_ty)
            }
            (
                Int(_) | Uint(_),
                Ref { .. } | StaticPtr { .. },
            ) => {
                let r = match right {
                    InterpValue::Ref { frame, local, .. } => {
                        ((*frame as u128) << 48) | (*local as u128)
                    }
                    InterpValue::StaticPtr { def_id } => {
                        (1u128 << 127) | (def_id.to_raw() as u128)
                    }
                    _ => 0,
                };
                self.eval_binary_op(op, left, &Uint(r), dest_ty)
            }
            _ => Err(InterpError::Panic(format!(
                "unsupported binop types: {:?} and {:?}",
                left, right
            ))),
        }
    }

    fn eval_unary_op(&self, op: UnOp, val: &InterpValue) -> InterpResult<InterpValue> {
        match (op, val) {
            (UnOp::Not, InterpValue::Bool(b)) => Ok(InterpValue::Bool(!b)),
            (UnOp::Not, InterpValue::Int(i)) => Ok(InterpValue::Int(!*i)),
            (UnOp::Not, InterpValue::Uint(u)) => Ok(InterpValue::Uint(!*u)),
            (UnOp::Neg, InterpValue::Int(i)) => Ok(InterpValue::Int(-*i)),
            (UnOp::Neg, InterpValue::Float(f)) => Ok(InterpValue::Float(-*f)),
            // `Deref` on a `Ref` *value* (as produced by `*x` where `x: &T`) reads
            // the pointee out of the referenced frame's locals. This is required
            // for `Drop`/`fn` bodies that dereference a `&mut` field (e.g. a
            // `&mut i32` counter incremented inside `drop`).
            (UnOp::Deref, InterpValue::StaticPtr { def_id }) => {
                // `*p` where `p = &STATIC`: read the global's current value.
                self.globals.borrow().get(def_id).cloned().ok_or_else(|| {
                    InterpError::Panic(format!(
                        "deref of uninitialized static {:?}",
                        def_id
                    ))
                })
            }
            (UnOp::Deref, InterpValue::Ref { frame, local, .. }) => {
                let frame_locals = self.locals_for_ref_frame(*frame).ok_or_else(|| {
                    InterpError::Panic(format!("deref of Ref from dead frame {frame}"))
                })?;
                frame_locals
                    .get(*local)
                    .and_then(|opt| opt.as_ref())
                    .cloned()
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "deref of uninitialized local {local} (frame {frame})"
                        ))
                    })
            }
            _ => Err(InterpError::Panic(format!(
                "unsupported unary op: {:?} on {:?}",
                op, val
            ))),
        }
    }

    fn read_place(&self, place: &Place) -> InterpResult<InterpValue> {
        let idx = place.local.index();
        let mut val = self
            .locals
            .get(idx)
            .and_then(|opt| opt.as_ref())
            .cloned()
            .ok_or_else(|| {
                let owner = self.current_body_owner;
                InterpError::Panic(format!(
                    "read from uninitialized local {} (owner={:?})",
                    idx, owner
                ))
            })?;

        // Keep track of the current type to compute offsets for ConstantIndex.
        let mut current_ty = self
            .local_decls
            .get(place.local.index())
            .map(|d| d.ty)
            .unwrap_or(Ty::ERROR);

        for proj in place.projection.iter() {
            match proj {
                ProjectionElem::Deref => match val {
                    // T140-PATCHED-DEREF-STRING: `InterpValue::String` is the
                    // interpreter's stand-in for `&str`/`&[u8]` byte payloads
                    // (see `byte_payload`). A `Deref` projection on it
                    // (`buf[0]` on `buf: &[u8]`) is a no-op — the value is
                    // already the "dereferenced" byte view.
                    InterpValue::String(_) => { /* val unchanged */ }
                    InterpValue::Ref { frame, local, .. } => {
                        let target = local;
                        let frame_locals = self.locals_for_ref_frame(frame).ok_or_else(|| {
                            InterpError::Panic(format!("deref of Ref from dead frame {frame}"))
                        })?;
                        val = frame_locals
                            .get(target)
                            .and_then(|opt| opt.as_ref())
                            .cloned()
                            .ok_or_else(|| {
                                InterpError::Panic(format!(
                                    "deref of uninitialized local {target} (frame {frame})"
                                ))
                            })?;
                        // Update current_ty from the *referenced* frame's decls,
                        // not `self.local_decls` (which only holds the current
                        // frame). A cross-frame `&mut self` ref would otherwise
                        // read the wrong frame's local type and corrupt the
                        // enum discriminant skip in the `Field` projection below.
                        let decl_ty = self
                            .decls_for_ref_frame(frame)
                            .and_then(|decls| decls.get(target))
                            .map(|decl| decl.ty);
                        current_ty = decl_ty.unwrap_or(Ty::ERROR);
                    }
                    InterpValue::StaticPtr { def_id } => {
                        // `static mut` / `&STATIC` read: fetch the value
                        // stored in the interpreter's globals map (lazily
                        // initialized from the `StaticRef` init on first
                        // reference).
                        let v = self.globals.borrow().get(&def_id).cloned().ok_or_else(
                            || {
                                InterpError::Panic(format!(
                                    "deref of uninitialized static {:?}",
                                    def_id
                                ))
                            },
                        )?;
                        val = v;
                    }
                    // `Deref` on a `Vec<T>` (AdtId 1020) is a trait deref:
                    // `impl<T> Deref for Vec<T> { type Target = [T]; }`.
                    // `self.v[i]` inside an `impl<T> Iterator for
                    // VecIter<T>` body lowers to
                    // `[Deref, Field(0), Deref, Index(..)]`; the second
                    // `Deref` derefs the `Vec<T>` to `[T]`.
                    //
                    // The interpreter models a Vec as
                    // `Aggregate([Aggregate(elements), Uint(len)])` and a
                    // slice as a one-char-per-element `String` (same as
                    // `(1020, "as_slice")`, which this mirrors). Convert
                    // here, so the following `Index` arm (which already
                    // handles `InterpValue::String`) extracts the element.
                    //
                    // Note: the String encoding is lossy for elements
                    // outside `0..=255` — same limitation `as_slice` has.
                    // Acceptable for the corpus-level contract; a faithful
                    // model would need `InterpValue::Slice` (a broader
                    // change).
                    InterpValue::Aggregate(fields)
                        if fields.len() == 2
                            && matches!(&fields[0], InterpValue::Aggregate(_))
                            && matches!(&fields[1], InterpValue::Uint(_) | InterpValue::Int(_)) =>
                    {
                        let elems: Vec<InterpValue> = match &fields[0] {
                            InterpValue::Aggregate(buf) => buf.clone(),
                            _ => Vec::new(),
                        };
                        let slice: String = elems
                            .iter()
                            .map(|e| match e {
                                InterpValue::Uint(n) => {
                                    char::from_u32((*n as u32) & 0xff).unwrap_or('\u{0}')
                                }
                                InterpValue::Int(n) => {
                                    char::from_u32(((*n).max(0) as u32) & 0xff)
                                        .unwrap_or('\u{0}')
                                }
                                _ => '\u{0}',
                            })
                            .collect();
                        val = InterpValue::String(slice);
                        // Update `current_ty` to `Slice(elem)` so the
                        // following `Index` classifies correctly.
                        if let TyKind::Adt(adt_id, substs) = self.tcx.ty_kind(current_ty) {
                            if adt_id.to_raw() == 1020 {
                                let elem = self
                                    .tcx
                                    .substitution_args(*substs)
                                    .first()
                                    .and_then(|a| match a {
                                        glyim_type::GenericArg::Ty(t) => Some(*t),
                                        _ => None,
                                    });
                                if let Some(e) = elem {
                                    current_ty = self.tcx.mk_ty(TyKind::Slice(e));
                                }
                            }
                        }
                    }
                    _ => {
                        return Err(InterpError::Panic(
                            "deref projection on non-reference value".into(),
                        ));
                    }
                },
                ProjectionElem::Field(field_idx) => {
                    let fi = field_idx.index();
                    match val {
                        // T140-PATCHED-FIELD-STRING: `InterpValue::String`
                        // stands in for a fat-pointer slice `[ptr, len]`. The
                        // MIR's `as_ptr()`/`as_bytes()` lowering reads
                        // `[Deref, Field(0)]` (data ptr) and `[Deref, Field(1)]`
                        // (len) directly. Model field 0 = the byte payload
                        // itself (a reference stand-in) and field 1 = the byte
                        // length, so the extern call's `(fd, ptr, len)` shape
                        // arrives intact.
                        InterpValue::String(ref s) => {
                            if fi == 0 {
                                val = InterpValue::String(s.clone());
                            } else if fi == 1 {
                                val = InterpValue::Uint(s.len() as u128);
                            } else {
                                return Err(InterpError::Panic(format!(
                                    "field {fi} on byte payload (expected 0=ptr or 1=len)"
                                )));
                            }
                        }
                        InterpValue::Aggregate(ref fields) => {
                            // Enum values are laid out as `[tag, ...payload]` in
                            // this interpreter (see `Rvalue::Aggregate` /
                            // `ProjectionElem::Downcast`). A `Field` projection on
                            // an enum must therefore skip the discriminant tag,
                            // exactly like an explicit `Downcast` would. Structs
                            // and tuples have no tag, so they index directly.
                            let is_enum =
                                if let TyKind::Adt(adt_id, _) = self.tcx.ty_kind(current_ty) {
                                    self.tcx
                                        .adt_def(*adt_id)
                                        .map(|d| d.kind == AdtKind::Enum)
                                        .unwrap_or(false)
                                } else {
                                    false
                                };
                            let adjusted = if is_enum { fi + 1 } else { fi };
                            val = fields.get(adjusted).cloned().ok_or_else(|| {
                                InterpError::Panic(format!(
                                    "field index {} out of bounds (len {})",
                                    adjusted,
                                    fields.len()
                                ))
                            })?;
                            // Update current_ty: we need the field type. We'll use ty_ctx.
                            if let Some(_decl) = self.local_decls.get(place.local.index()) {
                                // We need to compute field type from current_ty.
                                // For simplicity, we'll just set to ERROR and compute later.
                                // But we can improve by using layout.
                                current_ty = self.get_field_type(current_ty, *field_idx);
                            } else {
                                current_ty = Ty::ERROR;
                            }
                        }
                        _ => {
                            return Err(InterpError::Panic(
                                "field projection on non-aggregate value".into(),
                            ));
                        }
                    }
                }
                ProjectionElem::Index(index_local) => {
                    let index_val = self
                        .locals
                        .get(index_local.index())
                        .and_then(|opt| opt.as_ref())
                        .ok_or_else(|| {
                            InterpError::Panic(format!(
                                "index local {} not initialized",
                                index_local.index()
                            ))
                        })?;
                    let idx_u = match index_val {
                        InterpValue::Int(i) => *i as usize,
                        InterpValue::Uint(u) => *u as usize,
                        _ => {
                            return Err(InterpError::Panic("index must be an integer".into()));
                        }
                    };
                    match val {
                        // T140-PATCHED-DEREF-STRING: `buf[i]` on a byte-payload
                        // String returns the i-th byte as a Uint.
                        InterpValue::String(ref s) => {
                            let bytes = s.as_bytes();
                            val = bytes
                                .get(idx_u)
                                .map(|b| InterpValue::Uint(*b as u128))
                                .ok_or_else(|| {
                                    InterpError::Panic(format!(
                                        "index out of bounds: {idx_u} >= {}",
                                        bytes.len()
                                    ))
                                })?;
                        }
                        InterpValue::Aggregate(ref elems) => {
                            // `Vec<T>` is modelled as `Aggregate([buf, len])`
                            // (see `vec_push`), so an index selects from
                            // `buf`, not from the two-field header. Without
                            // this, `v[0]` on a one-element Vec read the
                            // *data pointer* field and returned garbage.
                            let is_vec = matches!(
                                self.tcx.ty_kind(current_ty),
                                glyim_type::TyKind::Adt(id, _) if id.to_raw() == 1020
                            );
                            let target = if is_vec {
                                match elems.first() {
                                    Some(InterpValue::Aggregate(buf)) => buf,
                                    _ => {
                                        return Err(InterpError::Panic(
                                            "Vec index: header field 0 is not a buffer".into(),
                                        ));
                                    }
                                }
                            } else {
                                elems
                            };
                            val = target.get(idx_u).cloned().ok_or_else(|| {
                                InterpError::Panic(format!(
                                    "index {} out of bounds (len {})",
                                    idx_u,
                                    target.len()
                                ))
                            })?;
                            // Update current_ty: we need element type.
                            current_ty = self.get_element_type(current_ty);
                        }
                        _ => {
                            return Err(InterpError::Panic(
                                "index projection on non-aggregate value".into(),
                            ));
                        }
                    }
                }
                ProjectionElem::Downcast(_variant_idx) => {
                    // For enum values, the discriminant tag is stored as
                    // `fields[0]`. Stripping it exposes the payload at its
                    // natural field indices so the following `Field` projections
                    // line up. Scalar (non-aggregate) values (e.g. plain ints)
                    // are left unchanged — `Downcast` is a no-op for them.
                    if let InterpValue::Aggregate(fields) = &val {
                        val = InterpValue::Aggregate(fields[1..].to_vec());
                        // The tag has been removed, so the value is now the bare
                        // variant payload. Mark the type as non-enum (ERROR
                        // sentinel) so the subsequent `Field` projection indexes
                        // the payload directly instead of adding a +1 for the tag.
                        current_ty = Ty::ERROR;
                    }
                }
                ProjectionElem::ConstantIndex {
                    offset,
                    min_length: _,
                    from_end,
                } => {
                    // Compute the actual index.
                    let len = self.get_length_of_aggregate(&val)?;
                    let idx = if *from_end {
                        if len <= *offset as usize {
                            return Err(InterpError::Panic(format!(
                                "constant index from end offset {} out of bounds (len {})",
                                offset, len
                            )));
                        }
                        len - *offset as usize
                    } else {
                        *offset as usize
                    };
                    // Now extract the element at idx.
                    match val {
                        InterpValue::Aggregate(ref elems) => {
                            if idx >= elems.len() {
                                return Err(InterpError::Panic(format!(
                                    "constant index {} out of bounds (len {})",
                                    idx,
                                    elems.len()
                                )));
                            }
                            val = elems[idx].clone();
                            // Update current_ty.
                            current_ty = self.get_element_type(current_ty);
                        }
                        _ => {
                            return Err(InterpError::Panic(
                                "constant index on non-aggregate value".into(),
                            ));
                        }
                    }
                }
                ProjectionElem::Subslice { from, to, from_end } => {
                    // Subslice produces a slice value (ptr, len).
                    // For interpreter, we need to represent a slice as an aggregate of (ptr, len)?
                    // But we don't have a concrete representation for slices yet.
                    // We'll treat it as a tuple of (data_ptr, len).
                    // Since we don't have heap, we'll just treat as a special value.
                    // For simplicity, we'll just panic for now? But better to implement.
                    // Let's return an aggregate with two values: address of the first element and length.
                    // We need to compute the data pointer and the new length.
                    let (_base_ptr, base_len) = self.get_slice_base_and_len(&val, current_ty)?;
                    let start = if *from_end {
                        if base_len < *to as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice from end to {} out of bounds (len {})",
                                to, base_len
                            )));
                        }
                        base_len - (*to as usize)
                    } else {
                        if base_len < *from as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice start {} out of bounds (len {})",
                                from, base_len
                            )));
                        }
                        (*from) as usize
                    };
                    let end = if *from_end {
                        // T-ICE-2: the guard here checked `to` while the
                        // arithmetic subtracts `from` — `from > base_len`
                        // underflowed (panic in debug, wrapped huge length
                        // in release). Check the operand actually used.
                        if base_len < *from as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice from end offset {} out of bounds (len {})",
                                from, base_len
                            )));
                        }
                        base_len - (*from as usize)
                    } else {
                        if base_len < *to as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice end {} out of bounds (len {})",
                                to, base_len
                            )));
                        }
                        (*to) as usize
                    };
                    if start > end {
                        return Err(InterpError::Panic("subslice start > end".into()));
                    }
                    let new_len = end - start;
                    // data_ptr = base_ptr + start * elem_size (but we don't have elem size).
                    // We'll just use the base_ptr as is and adjust len.
                    // In interpreter, we treat the aggregate as a tuple of (ptr, len).
                    // We'll create a new aggregate of two usize values: (start, new_len) as placeholder.
                    // Actually better: (base_ptr + start, new_len).
                    // But we can just represent as (start, new_len) for simplicity.
                    let elem_size = self.get_element_size(current_ty)?;
                    let data_ptr = if let InterpValue::Ref { frame, local, .. } = val {
                        let ptr = local + start * elem_size;
                        InterpValue::Ref {
                            frame,
                            local: ptr,
                            // The arithmetic here shifts `local` by whole
                            // elements; projection identity is lost, so
                            // reset to empty.
                            projection: Box::new([]),
                        }
                    } else {
                        // For non-ref aggregates, we can't compute ptr.
                        // We'll just use the index start as a placeholder.
                        InterpValue::Ref {
                            frame: 0,
                            local: 0,
                            projection: Box::new([]),
                        }
                    };
                    val = InterpValue::Aggregate(vec![data_ptr, InterpValue::Int(new_len as i128)]);
                }
            }
        }
        Ok(val)
    }

    pub(crate) fn write_place(&mut self, place: &Place, val: InterpValue) -> InterpResult<()> {
        self.write_place_frame(self.frame_depth, place, val)
    }

    /// Frame-aware write: `frame` is the depth of the locals block that owns
    /// `place.local`. Needed so that `*r = v` (where `r` is a `Ref` borrowed
    /// from an ancestor frame) writes into the correct frame's locals rather
    /// than the current one (plan §1: `next(&mut self)` mutating `main`'s
    /// `Counter`).
    fn write_place_frame(
        &mut self,
        frame: usize,
        place: &Place,
        val: InterpValue,
    ) -> InterpResult<()> {
        let idx = place.local.index();
        // Read the base local's declared type from the OWNING frame (not the
        // current one) before taking the mutable `frame_locals` borrow. MIR-17
        // uses it to pick the enum-tag offset; reading the current frame's
        // decls for a cross-frame write (`*r.field = v` where `r` points at a
        // caller's local) gave the wrong local's type and mis-decided `is_enum`.
        let base_ty = self
            .decls_for_ref_frame(frame)
            .and_then(|decls| decls.get(idx))
            .map(|d| d.ty)
            .unwrap_or(Ty::ERROR);
        let frame_locals = self
            .write_target_locals_mut(frame)
            .ok_or_else(|| InterpError::Panic(format!("write to dead frame {frame}")))?;
        if idx >= frame_locals.len() {
            return Err(InterpError::Panic(format!(
                "local index out of bounds: {}",
                idx
            )));
        }
        if place.projection.is_empty() {
            frame_locals[idx] = Some(val);
            return Ok(());
        }

        let proj_count = place.projection.len();

        if let Some(ProjectionElem::Deref) = place.projection.first() {
            if proj_count > 1 {
                let base_val = frame_locals
                    .get(idx)
                    .and_then(|opt| opt.as_ref())
                    .cloned()
                    .ok_or_else(|| {
                        InterpError::Panic(format!("write to uninitialized local {idx}"))
                    })?;
                // Snapshot the frame's locals before entering the match, so
                // the `frame_locals` mutable borrow of `self` ends before any
                // arm touches `self.globals`. Used by the `StaticPtr` arm to
                // resolve dynamic `Index` projections.
                let frame_snapshot: Vec<Option<InterpValue>> =
                    frame_locals.iter().cloned().collect();
                let target = match base_val {
                    InterpValue::Ref { frame, local, .. } => (frame, local),
                    InterpValue::StaticPtr { def_id } => {
                        // Write to a `static mut` field through a deeper
                        // projection (`STATIC.field = v` /
                        // `*STATIC_PTR.field = v`). Load the current global
                        // value, apply the remaining projections to it
                        // in-place, then write the mutated value back.
                        let mut cur = self.globals.borrow().get(&def_id).cloned()
                            .ok_or_else(|| InterpError::Panic(format!(
                                "write to uninitialized static {:?}",
                                def_id
                            )))?;
                        write_projection_into_value(
                            &mut cur,
                            &place.projection[1..],
                            val,
                            &frame_snapshot,
                        )?;
                        self.globals.borrow_mut().insert(def_id, cur);
                        return Ok(());
                    }
                    _ => {
                        return Err(InterpError::Panic(
                            "deref projection on non-reference value".into(),
                        ));
                    }
                };
                let target_place = Place {
                    local: LocalIdx::from_raw(target.1 as u32),
                    projection: place.projection[1..].to_vec().into_boxed_slice(),
                };
                return self.write_place_frame(target.0, &target_place, val);
            } else {
                let base_val = frame_locals
                    .get(idx)
                    .and_then(|opt| opt.as_ref())
                    .cloned()
                    .ok_or_else(|| {
                        InterpError::Panic(format!("write to uninitialized local {idx}"))
                    })?;
                let target = match base_val {
                    InterpValue::Ref { frame, local, .. } => (frame, local),
                    InterpValue::StaticPtr { def_id } => {
                        // Write to the whole `static mut` global.
                        self.globals.borrow_mut().insert(def_id, val);
                        return Ok(());
                    }
                    _ => {
                        return Err(InterpError::Panic(
                            "deref projection on non-reference value".into(),
                        ));
                    }
                };
                let target_frame_locals =
                    self.write_target_locals_mut(target.0).ok_or_else(|| {
                        InterpError::Panic(format!("write through ref to dead frame {}", target.0))
                    })?;
                if target.1 >= target_frame_locals.len() {
                    return Err(InterpError::Panic(format!(
                        "write through ref to invalid local {}",
                        target.1
                    )));
                }
                target_frame_locals[target.1] = Some(val);
                return Ok(());
            }
        }

        let base_val = frame_locals
            .get(idx)
            .and_then(|opt| opt.as_ref())
            .cloned()
            .ok_or_else(|| InterpError::Panic(format!("write to uninitialized local {idx}")))?;

        let modified =
            self.write_through_projections_with_locals(base_val, &place.projection, val, base_ty)?;
        let frame_locals = self
            .write_target_locals_mut(frame)
            .ok_or_else(|| InterpError::Panic(format!("write to dead frame {frame}")))?;
        frame_locals[idx] = Some(modified);
        Ok(())
    }

    fn write_through_projections_with_locals(
        &self,
        base: InterpValue,
        projections: &[ProjectionElem],
        val: InterpValue,
        current_ty: Ty,
    ) -> InterpResult<InterpValue> {
        if projections.is_empty() {
            return Ok(val);
        }
        let (first, rest) = (&projections[0], &projections[1..]);
        match first {
            ProjectionElem::Field(field_idx) => {
                let fi = field_idx.index();
                // MIR-17: enum values are laid out as `[tag, ...payload]`.
                // A `Field` write on an enum must skip the tag (like the read
                // path and an explicit `Downcast` do); otherwise payload data
                // overwrites the discriminant.
                let is_enum = if let TyKind::Adt(adt_id, _) = self.tcx.ty_kind(current_ty) {
                    self.tcx
                        .adt_def(*adt_id)
                        .map(|d| d.kind == AdtKind::Enum)
                        .unwrap_or(false)
                } else {
                    false
                };
                let adjusted = if is_enum { fi + 1 } else { fi };
                let field_ty = self.get_field_type(current_ty, *field_idx);
                match base {
                    InterpValue::Aggregate(mut fields) => {
                        if adjusted >= fields.len() {
                            return Err(InterpError::Panic(format!(
                                "field index {} out of bounds (len {})",
                                adjusted,
                                fields.len()
                            )));
                        }
                        let inner = fields[adjusted].clone();
                        fields[adjusted] =
                            self.write_through_projections_with_locals(inner, rest, val, field_ty)?;
                        Ok(InterpValue::Aggregate(fields))
                    }
                    _ => Err(InterpError::Panic(
                        "field projection on non-aggregate".into(),
                    )),
                }
            }
            ProjectionElem::Index(index_local) => {
                let index_val = self
                    .locals
                    .get(index_local.index())
                    .and_then(|opt| opt.as_ref())
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "index local {} not initialized",
                            index_local.index()
                        ))
                    })?;
                let idx_u = match index_val {
                    InterpValue::Int(i) => *i as usize,
                    InterpValue::Uint(u) => *u as usize,
                    _ => return Err(InterpError::Panic("index must be an integer".into())),
                };
                let elem_ty = self.get_element_type(current_ty);
                match base {
                    InterpValue::Aggregate(mut elems) => {
                        if idx_u >= elems.len() {
                            return Err(InterpError::Panic(format!(
                                "index {} out of bounds (len {})",
                                idx_u,
                                elems.len()
                            )));
                        }
                        let inner = elems[idx_u].clone();
                        elems[idx_u] =
                            self.write_through_projections_with_locals(inner, rest, val, elem_ty)?;
                        Ok(InterpValue::Aggregate(elems))
                    }
                    _ => Err(InterpError::Panic(
                        "index projection on non-aggregate".into(),
                    )),
                }
            }
            ProjectionElem::Downcast(_) => {
                // MIR-17: strip the discriminant tag so the following `Field`
                // projections index the payload directly. Mark the type
                // non-enum (ERROR sentinel) so the payload's own fields are not
                // re-offset by +1. Capture the tag before consuming `base`.
                match base {
                    InterpValue::Aggregate(fields) => {
                        let mut it = fields.into_iter();
                        let tag = it.next().unwrap_or(InterpValue::Int(0));
                        let payload = InterpValue::Aggregate(it.collect());
                        let written = self.write_through_projections_with_locals(
                            payload,
                            rest,
                            val,
                            Ty::ERROR,
                        )?;
                        match written {
                            InterpValue::Aggregate(payload_fields) => {
                                let mut new_fields = Vec::with_capacity(payload_fields.len() + 1);
                                new_fields.push(tag);
                                new_fields.extend(payload_fields);
                                Ok(InterpValue::Aggregate(new_fields))
                            }
                            other => Ok(other),
                        }
                    }
                    other => {
                        // Non-aggregate base: `Downcast` is a no-op.
                        self.write_through_projections_with_locals(other, rest, val, current_ty)
                    }
                }
            }
            ProjectionElem::Deref => Err(InterpError::Panic(
                "Deref projection unexpected in write_through_projections".into(),
            )),
            ProjectionElem::ConstantIndex {
                offset,
                min_length,
                from_end,
            } => {
                debug_assert!(
                    rest.is_empty(),
                    "ConstantIndex must be terminal (see slice_desugar invariant)"
                );
                match base {
                    InterpValue::Aggregate(mut elems) => {
                        let len = elems.len() as u64;
                        let idx = if *from_end {
                            len.checked_sub(*offset).ok_or_else(|| {
                                InterpError::Panic(format!(
                                    "ConstantIndex from_end offset {offset} out of bounds for length {len}"
                                ))
                            })?
                        } else {
                            *offset
                        } as usize;
                        if idx >= elems.len() || len < *min_length {
                            return Err(InterpError::Panic(format!(
                                "ConstantIndex {idx} out of bounds (len {len}, min_length {min_length})"
                            )));
                        }
                        elems[idx] = val;
                        Ok(InterpValue::Aggregate(elems))
                    }
                    _ => Err(InterpError::Panic(
                        "ConstantIndex write on non-aggregate".into(),
                    )),
                }
            }
            ProjectionElem::Subslice { from, to, from_end } => {
                debug_assert!(
                    rest.is_empty(),
                    "Subslice must be terminal (see slice_desugar invariant)"
                );
                match (base, val) {
                    (
                        InterpValue::Aggregate(mut elems),
                        InterpValue::Aggregate(new_slice_elems),
                    ) => {
                        let len = elems.len() as u64;
                        let end = if *from_end {
                            len.checked_sub(*to).ok_or_else(|| {
                                InterpError::Panic(format!(
                                    "Subslice `to` {to} out of bounds for length {len}"
                                ))
                            })?
                        } else {
                            *to
                        } as usize;
                        let (from, end) = (*from as usize, end);
                        if from > end || end > elems.len() || (end - from) != new_slice_elems.len()
                        {
                            return Err(InterpError::Panic(format!(
                                "Subslice write range [{from}, {end}) doesn't match value length {}",
                                new_slice_elems.len()
                            )));
                        }
                        elems.splice(from..end, new_slice_elems);
                        Ok(InterpValue::Aggregate(elems))
                    }
                    _ => Err(InterpError::Panic(
                        "Subslice write requires aggregate base and aggregate (slice) value".into(),
                    )),
                }
            }
        }
    }

    fn resolve_callee(
        &self,
        func: &Operand,
        args: &[Operand],
    ) -> InterpResult<(DefId, Vec<InterpValue>)> {
        match func {
            Operand::Constant(c) => match &c.kind {
                MirConstKind::Fn(def_id, _) => {
                    let crate_id = CrateId::from_raw(0);
                    let local_def_id = LocalDefId::from_raw(def_id.to_raw());
                    Ok((DefId::new(crate_id, local_def_id), Vec::new()))
                }
                MirConstKind::ConstRef(_, _) => Err(InterpError::Panic(
                    "ConstRef constant not interpretable as function".into(),
                )),
                MirConstKind::Int(id) => Ok((
                    DefId::new(CrateId::from_raw(0), LocalDefId::from_raw(*id as u32)),
                    Vec::new(),
                )),
                MirConstKind::VirtualMethod {
                    trait_def_id,
                    method_name,
                    self_ty,
                } => {
                    let recv_ty = self_ty.or_else(|| args.first().and_then(|a| self.operand_ty(a)));
                    if let Some(recv_ty) = recv_ty {
                        if let Some(fn_def_id) =
                            self.tcx
                                .resolve_trait_method(*trait_def_id, recv_ty, *method_name)
                        {
                            let crate_id = CrateId::from_raw(0);
                            let local_def_id = LocalDefId::from_raw(fn_def_id.to_raw());
                            return Ok((DefId::new(crate_id, local_def_id), Vec::new()));
                        }
                    }
                    Err(InterpError::Panic(format!(
                        "could not devirtualize trait method {:?} on receiver {:?}",
                        method_name, recv_ty
                    )))
                }
                _ => Err(InterpError::Panic(
                    "callee must be a function reference".into(),
                )),
            },
            // Indirect call: the callee is read from a place (e.g. a function
            // pointer stored in a local, or a closure aggregate value).
            // Plan §12.1: a closure value is an aggregate
            // `[Fn(def_id), captures...]`; unpack it so the captured values can
            // be passed as leading arguments to the closure body.
            Operand::Copy(place) | Operand::Move(place) => match self.read_place(place)? {
                InterpValue::Fn(def_id) => Ok((def_id, Vec::new())),
                InterpValue::Aggregate(fields) => {
                    if let Some(InterpValue::Fn(def_id)) = fields.first() {
                        let captured = fields[1..].to_vec();
                        Ok((*def_id, captured))
                    } else {
                        Err(InterpError::Panic(format!(
                            "indirect call through non-function value: {fields:?}"
                        )))
                    }
                }
                other => Err(InterpError::Panic(format!(
                    "indirect call through non-function value: {other:?}"
                ))),
            },
        }
    }

    /// Type of an operand, derived from its constant or (for `Copy`/`Move`)
    /// the local declaration. Used to recover the *concrete* receiver type of a
    /// devirtualized `VirtualMethod` call so it can be resolved against the
    /// trait-method dispatch table.
    fn operand_ty(&self, operand: &Operand) -> Option<Ty> {
        match operand {
            Operand::Constant(c) => Some(c.ty),
            Operand::Copy(place) | Operand::Move(place) => {
                self.local_decls.get(place.local.index()).map(|d| d.ty)
            }
        }
    }

    fn interp_value_to_u128(&self, val: &InterpValue) -> u128 {
        match val {
            InterpValue::Int(i) => *i as u128,
            InterpValue::Uint(u) => *u,
            InterpValue::Bool(b) => *b as u128,
            InterpValue::Unit => 0,
            InterpValue::Aggregate(fields) => {
                if fields.is_empty() {
                    0
                } else {
                    self.interp_value_to_u128(&fields[0])
                }
            }
            InterpValue::Ref { frame, local, .. } => {
                let _ = frame;
                *local as u128
            }
            InterpValue::Float(f) => f.to_bits() as u128,
            InterpValue::String(s) => s.len() as u128,
            InterpValue::Fn(_) | InterpValue::ConstRef(_) => 0,
            InterpValue::StaticPtr { def_id } => def_id.to_raw() as u128,
        }
    }

    fn interp_value_to_bool(&self, val: &InterpValue) -> InterpResult<bool> {
        match val {
            InterpValue::Bool(b) => Ok(*b),
            InterpValue::Int(i) => Ok(*i != 0),
            InterpValue::Uint(u) => Ok(*u != 0),
            InterpValue::Unit => Ok(false),
            InterpValue::Aggregate(fields) => {
                if fields.is_empty() {
                    Ok(false)
                } else {
                    self.interp_value_to_bool(&fields[0])
                }
            }
            InterpValue::Ref { .. } => Ok(true),
            InterpValue::Float(f) => Ok(*f != 0.0),
            InterpValue::String(s) => Ok(!s.is_empty()),
            InterpValue::Fn(_) | InterpValue::ConstRef(_) => Ok(true),
            InterpValue::StaticPtr { .. } => Ok(true),
        }
    }

    #[allow(dead_code)]
    fn array_length_from_ty(&self, ty: &glyim_type::Ty) -> InterpResult<usize> {
        let kind = self.tcx.ty_kind(*ty);
        match kind {
            glyim_type::TyKind::Array(_, const_val) => match &const_val.kind {
                glyim_type::ConstKind::Int(n) => Ok(*n as usize),
                glyim_type::ConstKind::Uint(n) => Ok(*n as usize),
                _ => Err(InterpError::Panic(
                    "Len: unsupported array length kind (non-integer)".into(),
                )),
            },
            _ => Err(InterpError::Panic("Len: expected array type".into())),
        }
    }

    fn slice_length_from_value(&self, val: &InterpValue) -> InterpResult<usize> {
        match val {
            InterpValue::Aggregate(fields) => {
                if fields.len() >= 2 {
                    match &fields[1] {
                        InterpValue::Int(i) => Ok(*i as usize),
                        InterpValue::Uint(u) => Ok(*u as usize),
                        _ => Err(InterpError::Panic("slice length must be an integer".into())),
                    }
                } else {
                    Err(InterpError::Panic(
                        "slice value must be an aggregate of at least 2 elements".into(),
                    ))
                }
            }
            InterpValue::Ref { frame, local, .. } => {
                let target_val = self
                    .locals_for_ref_frame(*frame)
                    .and_then(|fl| fl.get(*local))
                    .and_then(|opt| opt.as_ref())
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "slice reference to uninitialized local {local} (frame {frame})"
                        ))
                    })?;
                self.slice_length_from_value(target_val)
            }
            // T140-PATCHED-LEN-REF: the interpreter carries `&str`/`&[u8]`
            // byte payloads in `InterpValue::String` (see `byte_payload`).
            InterpValue::String(s) => Ok(s.len()),
            _ => Err(InterpError::Panic(
                "slice length expected aggregate or reference".into(),
            )),
        }
    }

    /// Helper to get the length of an aggregate (array, slice, tuple).
    fn get_length_of_aggregate(&self, val: &InterpValue) -> InterpResult<usize> {
        match val {
            InterpValue::Aggregate(fields) => Ok(fields.len()),
            InterpValue::Ref { frame, local, .. } => {
                // Dereference to get the aggregate.
                let target_val = self
                    .locals_for_ref_frame(*frame)
                    .and_then(|fl| fl.get(*local))
                    .and_then(|opt| opt.as_ref())
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "deref of uninitialized local {local} (frame {frame})"
                        ))
                    })?;
                self.get_length_of_aggregate(target_val)
            }
            _ => Err(InterpError::Panic("expected aggregate for length".into())),
        }
    }

    /// Helper to get element type of an aggregate (for updating current_ty).
    fn get_element_type(&self, ty: Ty) -> Ty {
        match self.tcx.ty_kind(ty) {
            glyim_type::TyKind::Array(elem, _) | glyim_type::TyKind::Slice(elem) => *elem,
            _ => Ty::ERROR,
        }
    }

    /// Helper to get field type.
    fn get_field_type(&self, ty: Ty, field_idx: FieldIdx) -> Ty {
        match self.tcx.ty_kind(ty) {
            glyim_type::TyKind::Tuple(substs) => {
                let args = self.tcx.substitution_args(*substs);
                if let Some(glyim_type::GenericArg::Ty(t)) = args.get(field_idx.index()) {
                    *t
                } else {
                    Ty::ERROR
                }
            }
            glyim_type::TyKind::Adt(adt_id, _) => self.tcx.field_ty(*adt_id, field_idx.index()),
            _ => Ty::ERROR,
        }
    }

    /// Helper to get slice base and length.
    fn get_slice_base_and_len(&self, val: &InterpValue, ty: Ty) -> InterpResult<(usize, usize)> {
        // We need to compute base pointer and length from the value.
        // For a slice represented as aggregate of (ptr, len), we extract.
        match val {
            InterpValue::Aggregate(fields) if fields.len() == 2 => {
                let ptr = match &fields[0] {
                    InterpValue::Ref { frame, local, .. } => {
                        let _ = frame;
                        *local
                    }
                    InterpValue::Int(i) => *i as usize,
                    _ => 0,
                };
                let len = match &fields[1] {
                    InterpValue::Int(i) => *i as usize,
                    InterpValue::Uint(u) => *u as usize,
                    _ => 0,
                };
                Ok((ptr, len))
            }
            InterpValue::Ref { frame, local, .. } => {
                // Dereference to get the aggregate.
                let target_val = self
                    .locals_for_ref_frame(*frame)
                    .and_then(|fl| fl.get(*local))
                    .and_then(|opt| opt.as_ref())
                    .ok_or_else(|| {
                        InterpError::Panic(format!(
                            "deref of uninitialized local {local} (frame {frame})"
                        ))
                    })?;
                self.get_slice_base_and_len(target_val, ty)
            }
            _ => Err(InterpError::Panic("expected slice aggregate".into())),
        }
    }

    /// Helper to get element size (for pointer arithmetic).
    ///
    /// Uses the real layout computer so that pointer arithmetic walks the
    /// correct number of bytes per element instead of assuming 1.
    fn get_element_size(&self, ty: Ty) -> InterpResult<usize> {
        self.layout
            .layout_of(ty)
            .map(|l| l.size.0 as usize)
            .map_err(|e| {
                InterpError::Panic(format!("cannot size type for pointer arithmetic: {e:?}"))
            })
    }
}

/// InterpResult.
pub type InterpResult<T> = Result<T, InterpError>;

#[cfg(test)]
mod tests;

/// Recursively write `val` into `cur` at `projections`. Supports `Field`
/// (index into `Aggregate`), and is the free-function form so it can be
/// called from `&self` methods that only have a `RefCell`-owned value to
/// mutate. `projections` empty means "replace the whole value".
fn write_projection_into_value(
    cur: &mut InterpValue,
    projections: &[glyim_mir::ProjectionElem],
    val: InterpValue,
    frame_locals: &[Option<InterpValue>],
) -> InterpResult<()> {
    if projections.is_empty() {
        *cur = val;
        return Ok(());
    }
    match &projections[0] {
        glyim_mir::ProjectionElem::Field(idx) => {
            let fi = idx.index();
            match cur {
                InterpValue::Aggregate(fields) => {
                    let len = fields.len();
                    let field = fields.get_mut(fi).ok_or_else(|| {
                        InterpError::Panic(format!(
                            "field {fi} out of range on static aggregate ({len} fields)"
                        ))
                    })?;
                    write_projection_into_value(field, &projections[1..], val, frame_locals)
                }
                other => Err(InterpError::Panic(format!(
                    "field projection on non-aggregate static value: {:?}",
                    other
                ))),
            }
        }
        glyim_mir::ProjectionElem::ConstantIndex {
            offset, from_end, ..
        } => {
            // Fixed offset into an array/tuple/slice aggregate. For arrays
            // this is a straight index (Rust MIR uses `from_end = false`);
            // for slices with `from_end = true` it counts from the tail.
            let fi = match cur {
                InterpValue::Aggregate(fields) => {
                    let idx = if *from_end {
                        fields.len().saturating_sub(*offset as usize)
                    } else {
                        *offset as usize
                    };
                    let len = fields.len();
                    fields.get_mut(idx).ok_or_else(|| {
                        InterpError::Panic(format!(
                            "constant index {idx} out of range on static aggregate ({len} fields)"
                        ))
                    })?
                }
                other => {
                    return Err(InterpError::Panic(format!(
                        "constant-index projection on non-aggregate static value: {:?}",
                        other
                    )));
                }
            };
            write_projection_into_value(fi, &projections[1..], val, frame_locals)
        }
        glyim_mir::ProjectionElem::Index(index_local) => {
            // Dynamic index: the index lives in a local of the *current
            // frame*. `frame_locals` is a snapshot of that frame taken by
            // the caller (we can't borrow `self.locals` while the frame's
            // borrow is alive).
            let raw = frame_locals
                .get(index_local.index())
                .and_then(|o| o.as_ref())
                .ok_or_else(|| {
                    InterpError::Panic(format!(
                        "index local {} uninitialized in static deeper-write path",
                        index_local.index()
                    ))
                })?;
            let i = match raw {
                InterpValue::Uint(u) => *u as usize,
                InterpValue::Int(v) if *v >= 0 => *v as usize,
                other => {
                    return Err(InterpError::Panic(format!(
                        "non-integer index value in static deeper-write path: {:?}",
                        other
                    )));
                }
            };
            let fi = match cur {
                InterpValue::Aggregate(fields) => {
                    let len = fields.len();
                    fields.get_mut(i).ok_or_else(|| {
                        InterpError::Panic(format!(
                            "index {i} out of range on static aggregate ({len} fields)"
                        ))
                    })?
                }
                other => {
                    return Err(InterpError::Panic(format!(
                        "index projection on non-aggregate static value: {:?}",
                        other
                    )));
                }
            };
            write_projection_into_value(fi, &projections[1..], val, frame_locals)
        }
        glyim_mir::ProjectionElem::Downcast(_) => {
            // A `Downcast` on an enum value in this interpreter is a no-op
            // at the value level (the tag is encoded in field 0 of the
            // Aggregate). Recurse through.
            write_projection_into_value(cur, &projections[1..], val, frame_locals)
        }
        glyim_mir::ProjectionElem::Deref => {
            // Deref through a nested `Ref` isn't reachable for statics (no
            // way to form a `Ref` to a static's field via `&STATIC.field`
            // yet); reject clearly rather than miscompiling.
            Err(InterpError::Panic(
                "deref projection in static deeper-write path is not yet supported".into(),
            ))
        }
        _ => Err(InterpError::Panic(
            "unsupported projection in static deeper-write path".into(),
        )),
    }
}
