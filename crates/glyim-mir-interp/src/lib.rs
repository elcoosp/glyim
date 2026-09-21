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

    /// Configure whether a panicking call is expected to unwind to its
    /// `cleanup` block. Currently a no-op flag (full unwind-table support is
    /// out of scope for this tree-walking interpreter); provided so callers
    /// can opt into the documented-but-unimplemented behavior intentionally.
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

            let terminator_kind = body.basic_blocks[bb_idx].terminator.kind.clone();

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
            let stmt_count = body.basic_blocks[bb_idx].statements.len();
            let mut stmt_idx = 0;
            while stmt_idx < stmt_count {
                let stmt = body.basic_blocks[bb_idx].statements[stmt_idx].clone();
                if let Err(e) = self.execute_statement(&stmt) {
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
                        let ret_val = self.read_place(&Place::new(LocalIdx::from_raw(0)))?;
                        let caller_body = frame.body;
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
                        "reached unreachable terminator".to_string(),
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
                            },
                        span,
                        ..
                    }) = &func
                    {
                        if let Some(recv_ty) = args.first().and_then(|a| self.operand_ty(a)) {
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
                    let callee_body =
                        self.function_table
                            .get(&callee_id)
                            .cloned()
                            .ok_or_else(|| {
                                InterpError::Panic(format!("function not found: {:?}", callee_id))
                            })?;

                    let mut arg_values = captured;
                    for arg_op in &args {
                        arg_values.push(self.eval_operand(arg_op)?);
                    }

                    self.recursion_depth += 1;
                    if self.recursion_depth > self.recursion_limit {
                        self.current_body = Some(body);
                        self.current_bb = bb_idx;
                        return Err(InterpError::StackOverflow);
                    }

                    let mut callee_locals = vec![None; callee_body.locals.len()];
                    for (i, val) in arg_values.into_iter().enumerate() {
                        callee_locals[i + 1] = Some(val);
                    }

                    let next_bb = target
                        .unwrap_or_else(|| BasicBlockIdx::from_raw((bb_idx.index() + 1) as u32));

                    let caller_frame = CallFrame {
                        body,
                        bb: next_bb,
                        locals: std::mem::take(&mut self.locals),
                        return_place: destination,
                        target_bb: next_bb,
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
                    // By the time MIR reaches the interpreter, `glyim-opt`'s
                    // drop-elaboration pass must have rewritten `Drop`
                    // terminators for types with actual destructors into
                    // `Call`s to the generated drop-glue functions. A bare
                    // `Drop` surviving to the interpreter means either the
                    // type needs no drop glue (a legitimate no-op) or a
                    // missing/misordered drop-elaboration pass (a compiler
                    // bug). Enforce the invariant loudly in debug/test
                    // builds instead of silently no-op'ing (plan §14.1): if a
                    // `Drop` reaches here for a type that DOES need drop glue,
                    // drop elaboration failed to run. We cannot inspect the
                    // type cheaply here without the type context, so we assert
                    // unconditionally and rely on the §15.1 validator (which
                    // flags `Drop` on droppable types post-elaboration) to
                    // catch the real bug at the MIR level. Fail open only in
                    // release builds.
                    debug_assert!(
                        false,
                        "Drop terminator reached the interpreter for place {place:?}; \
                         drop elaboration should have lowered this to a drop-glue call. \
                         This indicates a missing/misordered optimization pass — \
                         check that DropElaboration runs before MIR reaches the interpreter/codegen."
                    );
                    tracing::debug!(
                        "interpreter Drop terminator: skipping (drop glue already lowered to a Call)"
                    );
                    bb_idx = target;
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
                let val = self.eval_rvalue(rvalue)?;
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

    pub(crate) fn eval_rvalue(&self, rvalue: &Rvalue) -> InterpResult<InterpValue> {
        match rvalue {
            Rvalue::Use(operand) => self.eval_operand(operand),
            Rvalue::BinaryOp(op, operands) => {
                let (left, right) = operands.as_ref();
                let l = self.eval_operand(left)?;
                let r = self.eval_operand(right)?;
                self.eval_binary_op(*op, &l, &r)
            }
            Rvalue::UnaryOp(op, operand) => {
                let v = self.eval_operand(operand)?;
                self.eval_unary_op(*op, &v)
            }
            Rvalue::Ref(place, _borrow_kind) => {
                let local_idx = place.local.index();
                Ok(InterpValue::Ref {
                    frame: self.frame_depth,
                    local: local_idx,
                })
            }
            Rvalue::Aggregate(kind, operands) => {
                let mut values = Vec::with_capacity(operands.len());
                for op in operands {
                    values.push(self.eval_operand(op)?);
                }
                if values.is_empty() {
                    Ok(InterpValue::Unit)
                } else {
                    // For enum ADTs, prepend the discriminant as `fields[0]` so that
                    // `SwitchInt` and `Rvalue::Discriminant` can read the variant
                    // index from the value. `read_place`'s `Downcast` projection
                    // strips this tag so the payload remains at its natural field
                    // indices. Struct/tuple/closure aggregates are left untagged.
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
                    _ => Err(InterpError::Panic("Len: expected array or slice".into())),
                }
            }
            Rvalue::Cast(kind, operand, _target_ty) => {
                let val = self.eval_operand(operand)?;
                match kind {
                    &glyim_mir::CastKind::FloatToFloat => {
                        if let InterpValue::Float(a) = val {
                            Ok(InterpValue::Float(a))
                        } else {
                            Err(InterpError::Panic("expected float for FloatToFloat".into()))
                        }
                    }
                    CastKind::IntToInt => Ok(val),
                    CastKind::IntToFloat => match val {
                        InterpValue::Int(i) => Ok(InterpValue::Float(i as f64)),
                        _ => Err(InterpError::Panic("expected int for IntToFloat".into())),
                    },
                    CastKind::FloatToInt => match val {
                        InterpValue::Float(f) => Ok(InterpValue::Int(f as i128)),
                        _ => Err(InterpError::Panic("expected float for FloatToInt".into())),
                    },
                    // Deliberately a no-op: InterpValue pointers carry no
                    // type-specific representation to convert between, so a
                    // pointer-to-pointer cast is identity at this value level.
                    CastKind::PtrToPtr | CastKind::FnPtrToPtr => Ok(val),
                    CastKind::PtrToInt => match val {
                        InterpValue::Ref { frame, local } => {
                            Ok(InterpValue::Uint(((frame as u128) << 48) | (local as u128)))
                        }
                        _ => Err(InterpError::Panic("PtrToInt on non-reference".into())),
                    },
                    CastKind::IntToPtr => match val {
                        InterpValue::Uint(addr) => Ok(InterpValue::Ref {
                            frame: (addr >> 48) as usize,
                            local: (addr & ((1u128 << 48) - 1)) as usize,
                        }),
                        InterpValue::Int(addr) if addr >= 0 => Ok(InterpValue::Ref {
                            frame: 0,
                            local: addr as usize,
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
            MirConstKind::Aggregate(elems) => {
                let mut values = Vec::with_capacity(elems.len());
                for e in elems {
                    values.push(self.eval_mir_const(e)?);
                }
                Ok(InterpValue::Aggregate(values))
            }
            MirConstKind::Error => Err(InterpError::Panic("Error const encountered".into())),
            // Devirtualized at the `Call` terminator before being evaluated as a
            // value; should never be reached as an operand constant.
            MirConstKind::VirtualMethod { .. } => Err(InterpError::Panic(
                "VirtualMethod const reached eval_const".into(),
            )),
        }
    }

    fn eval_binary_op(
        &self,
        op: BinOp,
        left: &InterpValue,
        right: &InterpValue,
    ) -> InterpResult<InterpValue> {
        use InterpValue::*;
        match (left, right) {
            (Int(l), Int(r)) => {
                let result = match op {
                    BinOp::Add => l.wrapping_add(*r),
                    BinOp::Sub => l.wrapping_sub(*r),
                    BinOp::Mul => l.wrapping_mul(*r),
                    BinOp::Div => {
                        // Plan §11.2: signed division must not panic on
                        // `MIN / -1`; `checked_div` returns `None` there and the
                        // language semantics require `0`.
                        if *r == 0 {
                            return Err(InterpError::DivisionByZero);
                        }
                        l.checked_div(*r).unwrap_or(0)
                    }
                    BinOp::Rem => {
                        // Plan §11.2: signed remainder must not panic on
                        // `MIN % -1`; `checked_rem` returns `None` there and the
                        // language semantics require `0`.
                        if *r == 0 {
                            return Err(InterpError::DivisionByZero);
                        }
                        l.checked_rem(*r).unwrap_or(0)
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
                Ok(Int(result))
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
                            return Err(InterpError::DivisionByZero);
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
                Ok(Uint(result))
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
                        if *r == 0.0 {
                            return Err(InterpError::Panic("division by zero".into()));
                        }
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
            (UnOp::Deref, InterpValue::Ref { frame, local }) => {
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
                    InterpValue::Ref { frame, local } => {
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
                    _ => {
                        return Err(InterpError::Panic(
                            "deref projection on non-reference value".into(),
                        ));
                    }
                },
                ProjectionElem::Field(field_idx) => {
                    let fi = field_idx.index();
                    match val {
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
                        InterpValue::Aggregate(ref elems) => {
                            val = elems.get(idx_u).cloned().ok_or_else(|| {
                                InterpError::Panic(format!(
                                    "index {} out of bounds (len {})",
                                    idx_u,
                                    elems.len()
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
                        (*from) as usize
                    };
                    let end = if *from_end {
                        if base_len < *to as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice from end to {} out of bounds (len {})",
                                to, base_len
                            )));
                        }
                        base_len - (*from as usize)
                    } else {
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
                    let data_ptr = if let InterpValue::Ref { frame, local } = val {
                        let ptr = local + start * elem_size;
                        InterpValue::Ref { frame, local: ptr }
                    } else {
                        // For non-ref aggregates, we can't compute ptr.
                        // We'll just use the index start as a placeholder.
                        InterpValue::Ref { frame: 0, local: 0 }
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
                let target = match base_val {
                    InterpValue::Ref { frame, local } => (frame, local),
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
                    InterpValue::Ref { frame, local } => (frame, local),
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
            self.write_through_projections_with_locals(base_val, &place.projection, val)?;
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
    ) -> InterpResult<InterpValue> {
        if projections.is_empty() {
            return Ok(val);
        }
        let (first, rest) = (&projections[0], &projections[1..]);
        match first {
            ProjectionElem::Field(field_idx) => {
                let fi = field_idx.index();
                match base {
                    InterpValue::Aggregate(mut fields) => {
                        if fi >= fields.len() {
                            return Err(InterpError::Panic(format!(
                                "field index {} out of bounds (len {})",
                                fi,
                                fields.len()
                            )));
                        }
                        let inner = fields[fi].clone();
                        fields[fi] =
                            self.write_through_projections_with_locals(inner, rest, val)?;
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
                            self.write_through_projections_with_locals(inner, rest, val)?;
                        Ok(InterpValue::Aggregate(elems))
                    }
                    _ => Err(InterpError::Panic(
                        "index projection on non-aggregate".into(),
                    )),
                }
            }
            ProjectionElem::Downcast(_) => {
                Ok(self.write_through_projections_with_locals(base, rest, val)?)
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
                } => {
                    let recv_ty = args.first().and_then(|a| self.operand_ty(a));
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
            InterpValue::Ref { frame, local } => {
                let _ = frame;
                *local as u128
            }
            InterpValue::Float(f) => f.to_bits() as u128,
            InterpValue::String(s) => s.len() as u128,
            InterpValue::Fn(_) | InterpValue::ConstRef(_) => 0,
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
            InterpValue::Ref { frame, local } => {
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
            _ => Err(InterpError::Panic(
                "slice length expected aggregate or reference".into(),
            )),
        }
    }

    /// Helper to get the length of an aggregate (array, slice, tuple).
    fn get_length_of_aggregate(&self, val: &InterpValue) -> InterpResult<usize> {
        match val {
            InterpValue::Aggregate(fields) => Ok(fields.len()),
            InterpValue::Ref { frame, local } => {
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
                    InterpValue::Ref { frame, local } => {
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
            InterpValue::Ref { frame, local } => {
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
