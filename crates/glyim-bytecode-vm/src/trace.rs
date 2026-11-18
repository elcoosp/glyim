//! Execution trace seam: record what a [`Vm`] actually executed.
//!
//! The disassembler ([`crate::disasm`]) is the *static* half of the image
//! seam's observability story: it lists what a decoded artifact *carries*.
//! It cannot answer "what did this module *do* when it ran?" — which path a
//! branch took, how deep the call stack grew, what the operand stack held
//! when the machine dispatched each instruction. This module is the
//! *dynamic* counterpart: a hook in [`Vm::drive`]'s dispatch loop that hands
//! one [`VmStep`] per executed instruction to a [`TraceSink`] the caller
//! attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (a sink observes; it never mutates VM state and
//! cannot change execution), **total** (every dispatch is recorded, including
//! the faulting instruction that produces a `VmError`, and an unknown opcode
//! byte records as `opcode: None` rather than being skipped), and **zero-cost
//! when unused** (a `Vm` without an attached sink pays one `None` check per
//! dispatch, exactly the pre-seam behavior).
//!
//! Two sinks ship with the crate: [`TraceCollector`] captures the steps in
//! memory (bounded or unbounded — a bounded collector marks overflow instead
//! of growing without limit over an infinite loop), and [`TracingSink`] emits
//! one `tracing::debug!` event per step for the workspace-standard
//! observability layer. [`render_trace`] renders a captured run into the same
//! deterministic diagnostic-text style the disassembler uses.
//!
//! The payload decode mirrors `Vm::drive`'s operand widths exactly (the same
//! rule `crate::disasm` follows): `i64` constants, `u32` indices, and the
//! two-byte `OP_CAST` form. A payload that runs past the code's end records
//! as [`StepDetail::Truncated`] — never a panic, never a silent read.

use std::any::Any;
use std::fmt::Write as _;

use crate::{Module, Opcode};
use tracing::debug;

/// One executed VM step, recorded by the [`Vm`]'s dispatch hook and handed to
/// the attached [`TraceSink`] just before the instruction executes.
///
/// `pc` is the instruction's start offset within the executing function,
/// `frame_depth` / `stack_depth` are the machine's depths at dispatch (the
/// operand stack still holds everything the instruction is about to pop), and
/// `detail` is the instruction's decoded payload ([`StepDetail::None`] for
/// the no-payload opcodes, [`StepDetail::Truncated`] when a payload runs
/// past the code's end).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmStep {
    /// The executing function's index in the module.
    pub func: usize,
    /// The instruction's start offset in the function's code.
    pub pc: usize,
    /// The raw opcode byte (an unknown byte's value survives in the step).
    pub raw: u8,
    /// The decoded opcode, or `None` for a byte that is not one.
    pub opcode: Option<Opcode>,
    /// Call-frame depth at dispatch (1 = the entry frame).
    pub frame_depth: usize,
    /// Operand-stack depth at dispatch, before the instruction's effects.
    pub stack_depth: usize,
    /// The decoded payload (what the disassembler's listing would show for
    /// this instruction, as structured data instead of text).
    pub detail: StepDetail,
}

/// The decoded payload of one executed instruction — the structured
/// counterpart of the disassembler's per-opcode listing text. The widths and
/// numbering mirror `Vm::drive`'s operand reads exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepDetail {
    /// No payload (arithmetic, unary, control flow, projections, `Return`,
    /// `Trap`).
    None,
    /// `OP_LOAD_CONST`: the pushed `i64` constant.
    ConstInt(i64),
    /// `OP_LOAD_CONST_F64`: the raw IEEE-754 bit pattern (the disasm
    /// convention — a corrupted bit pattern stays visible).
    ConstFloatBits(u64),
    /// `OP_LOAD_CONST_STR`: the pool index and whether it resolved into the
    /// module's string pool.
    ConstStr {
        /// The pool index the instruction references.
        pool_index: usize,
        /// Whether `module.strings` has the index (an out-of-range index is
        /// an explicit [`crate::VmError::StringIndexOutOfBounds`]).
        resolved: bool,
    },
    /// A local-index payload (`OP_LOAD_LOCAL` / `OP_STORE_LOCAL` /
    /// `OP_LOAD_LOCAL_ADDR` / `OP_LEN`).
    Local(usize),
    /// `OP_CAST`: the raw cast-kind and target-type bytes (the renderer
    /// spells their names, unknown bytes included).
    Cast {
        /// The cast-kind byte (`0` = `IntToInt`, …, `7` = `FloatToFloat`).
        kind: u8,
        /// The target-primitive byte ([`crate::CastTarget`] numbering).
        target: u8,
    },
    /// A branch target (`OP_JUMP` / `OP_JUMP_IF`): the block index (or raw
    /// byte offset in table-less functions) the instruction resolves
    /// against, in the disassembler's `tN` spelling.
    Branch {
        /// The block index / byte offset.
        target: u32,
    },
    /// `OP_SWITCH_INT`: the arm count and the otherwise-target.
    Switch {
        /// How many `value -> target` arms the instruction carries.
        n_arms: usize,
        /// The otherwise-target block index.
        otherwise: u32,
    },
    /// `OP_ASSERT`: the expected condition byte and the failure target.
    Assert {
        /// The condition the assert expects (`1` = truthy, `0` = falsy).
        expected: u8,
        /// The block index to branch to on failure.
        target: u32,
    },
    /// `OP_AGGREGATE`: the element count.
    Aggregate(usize),
    /// `OP_CALL` / `OP_CALL_INDIRECT`: the arity, destination local, and
    /// resume block target (the disassembler's `argc=… dest=… tN` fields).
    Call {
        /// How many stack operands become call arguments.
        argc: usize,
        /// The caller's local that receives the return value.
        dest: usize,
        /// The block index in the caller the callee's `Return` resumes to.
        target: u32,
    },
    /// A payload that runs past the code's end — never a panic, never a
    /// silent read.
    Truncated,
}

/// The observer the VM hands each executed step to.
///
/// Implementors must treat [`TraceSink::step`] as an observation only: the
/// VM's state and control flow are invisible to the sink and unaffected by
/// it. The method must not panic (the VM never defends against a panicking
/// sink — a sink that panics aborts the run, which is the embedding host's
/// explicit choice). The [`TraceSink::as_any`] hook lets a caller recover
/// the typed sink after `Vm::take_trace`.
pub trait TraceSink: 'static {
    /// One instruction dispatch, recorded just before the instruction
    /// executes (so a faulting step is the trace's last entry).
    fn step(&mut self, step: &VmStep);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_trace`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every step in memory.
///
/// `TraceCollector::new()` is unbounded; `TraceCollector::bounded(max_steps)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit — an infinite loop must not OOM the embedding host.
/// The flag is a deliberate, *displayed* fact, never a silent drop.
#[derive(Clone, Default)]
pub struct TraceCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured steps, in dispatch order.
    steps: Vec<VmStep>,
    /// Whether any step was dropped because the cap was reached.
    overflowed: bool,
}

impl TraceCollector {
    /// Create an unbounded collector (every step of the run is captured).
    #[must_use]
    pub fn new() -> TraceCollector {
        TraceCollector {
            cap: None,
            steps: Vec::new(),
            overflowed: false,
        }
    }

    /// Create a collector that captures at most `max_steps` steps and marks
    /// the overflow instead of growing without limit.
    #[must_use]
    pub fn bounded(max_steps: usize) -> TraceCollector {
        TraceCollector {
            cap: Some(max_steps),
            steps: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured steps, in dispatch order (a bounded collector holds at
    /// most its cap).
    #[must_use]
    pub fn steps(&self) -> &[VmStep] {
        &self.steps
    }

    /// Consume the collector into its steps.
    #[must_use]
    pub fn into_steps(self) -> Vec<VmStep> {
        self.steps
    }

    /// How many steps were captured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Whether any step was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }
}

impl TraceSink for TraceCollector {
    fn step(&mut self, step: &VmStep) {
        if let Some(cap) = self.cap {
            if self.steps.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.steps.push(step.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A sink that emits one `tracing::debug!` event per step — the seam's
/// counterpart for the workspace-standard observability layer (the same
/// `tracing` dependency the image seam logs through). Every dispatch becomes
/// a structured event; enable it per-subscriber, never by paying the cost
/// unconditionally.
pub struct TracingSink;

impl TraceSink for TracingSink {
    fn step(&mut self, step: &VmStep) {
        debug!(
            func = step.func,
            pc = step.pc,
            raw = step.raw,
            opcode = step.opcode.map(|op| op.name()),
            frame_depth = step.frame_depth,
            stack_depth = step.stack_depth,
            "vm step"
        );
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a captured run into a deterministic listing (the dynamic
/// counterpart of [`crate::render_module`]): a header line (image format
/// version, step count), then one line per step — step index, executing
/// function, pc, depths, opcode mnemonic, decoded payload.
///
/// Deterministic (the same steps render byte-identical text — the test suite
/// pins a golden) and total (any step list renders, unknown opcode bytes
/// included). `module` supplies the string pool the `OP_LOAD_CONST_STR`
/// details resolve their content against.
#[must_use]
pub fn render_trace(steps: &[VmStep], module: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "trace glyim.module-image/{} steps={}",
        crate::IMAGE_VERSION,
        steps.len()
    );
    for (i, step) in steps.iter().enumerate() {
        let mnemonic = step
            .opcode
            .map(crate::disasm::op_name)
            .unwrap_or("unknown(0x??)");
        let suffix = render_detail(&step.detail, module);
        let _ = writeln!(
            out,
            "{i}: fn {} pc {} frame={} stack={} {mnemonic}{suffix}",
            step.func, step.pc, step.frame_depth, step.stack_depth,
        );
    }
    debug!(steps = steps.len(), "vm trace rendered");
    out
}

/// Render one step's payload suffix (the text after the mnemonic, in the
/// disassembler's spelling; [`StepDetail::None`] renders nothing). Shared
/// with the tail-window render, which spells the captured tail in the
/// trace's exact field shape.
pub(crate) fn render_detail(detail: &StepDetail, module: &Module) -> String {
    match *detail {
        StepDetail::None => String::new(),
        StepDetail::ConstInt(v) => format!(" {v}"),
        StepDetail::ConstFloatBits(bits) => {
            format!(" {} (bits 0x{bits:016x})", f64::from_bits(bits))
        }
        StepDetail::ConstStr {
            pool_index,
            resolved: _,
        } => match module.strings.get(pool_index) {
            Some(s) => format!(" pool[{pool_index}] {s:?}"),
            None => format!(" pool[{pool_index}] <out of range>"),
        },
        StepDetail::Local(idx) => format!(" {idx}"),
        StepDetail::Cast { kind, target } => format!(
            " {}->{}",
            crate::disasm::cast_kind_name(kind),
            crate::disasm::cast_target_name(target)
        ),
        StepDetail::Branch { target } => format!(" t{target}"),
        StepDetail::Switch { n_arms, otherwise } => {
            format!(" n={n_arms} otherwise=t{otherwise}")
        }
        StepDetail::Assert { expected, target } => {
            format!(" expected={expected} t{target}")
        }
        StepDetail::Aggregate(n) => format!(" n={n}"),
        StepDetail::Call { argc, dest, target } => format!(" argc={argc} dest={dest} t{target}"),
        StepDetail::Truncated => " <truncated>".to_string(),
    }
}

/// Decode one instruction's payload at `pos` (the operand start, just past
/// the opcode byte) into its [`StepDetail`]. The reads mirror `Vm::drive`'s
/// operand widths exactly; every read is total (`StepDetail::Truncated` when
/// the payload runs past the code's end), so the hook never panics over
/// corrupt code.
pub(crate) fn step_detail(code: &[u8], pool: &[String], pos: usize, op: Opcode) -> StepDetail {
    let read_i64 = |pos: usize| -> Option<i64> {
        if pos + 8 > code.len() {
            return None;
        }
        let mut b = [0u8; 8];
        b.copy_from_slice(&code[pos..pos + 8]);
        Some(i64::from_le_bytes(b))
    };
    let read_u32 = |pos: usize| -> Option<u32> {
        if pos + 4 > code.len() {
            return None;
        }
        let mut b = [0u8; 4];
        b.copy_from_slice(&code[pos..pos + 4]);
        Some(u32::from_le_bytes(b))
    };
    match op {
        Opcode::LoadConst => read_i64(pos).map(StepDetail::ConstInt),
        Opcode::LoadConstF64 => read_i64(pos).map(|bits| StepDetail::ConstFloatBits(bits as u64)),
        Opcode::LoadConstStr => read_u32(pos).map(|idx| StepDetail::ConstStr {
            pool_index: idx as usize,
            resolved: pool.get(idx as usize).is_some(),
        }),
        Opcode::LoadLocal | Opcode::StoreLocal | Opcode::LoadLocalAddr | Opcode::Len => {
            read_u32(pos).map(|idx| StepDetail::Local(idx as usize))
        }
        Opcode::Cast => {
            let kind = code.get(pos).copied();
            let target = code.get(pos + 1).copied();
            match (kind, target) {
                (Some(kind), Some(target)) => Some(StepDetail::Cast { kind, target }),
                _ => None,
            }
        }
        Opcode::Jump | Opcode::JumpIf => read_u32(pos).map(|t| StepDetail::Branch { target: t }),
        Opcode::SwitchInt => read_u32(pos).and_then(|count| {
            read_u32(pos + 4 + count as usize * 12).map(|otherwise| StepDetail::Switch {
                n_arms: count as usize,
                otherwise,
            })
        }),
        Opcode::Assert => code.get(pos).copied().and_then(|expected| {
            read_u32(pos + 1).map(|target| StepDetail::Assert { expected, target })
        }),
        Opcode::Aggregate => read_u32(pos).map(|n| StepDetail::Aggregate(n as usize)),
        Opcode::Call | Opcode::CallIndirect => read_u32(pos).and_then(|argc| {
            read_u32(pos + 4).and_then(|dest| {
                read_u32(pos + 8).map(|target| StepDetail::Call {
                    argc: argc as usize,
                    dest: dest as usize,
                    target,
                })
            })
        }),
        // No-payload opcodes.
        Opcode::Add
        | Opcode::Sub
        | Opcode::Mul
        | Opcode::Div
        | Opcode::Rem
        | Opcode::Eq
        | Opcode::Ne
        | Opcode::Lt
        | Opcode::Gt
        | Opcode::Le
        | Opcode::Ge
        | Opcode::And
        | Opcode::Or
        | Opcode::Not
        | Opcode::Neg
        | Opcode::BitAnd
        | Opcode::BitOr
        | Opcode::BitXor
        | Opcode::Shl
        | Opcode::Shr
        | Opcode::StoreField
        | Opcode::Deref
        | Opcode::Discriminant
        | Opcode::Drop
        | Opcode::Repeat
        | Opcode::Return
        | Opcode::Trap => Some(StepDetail::None),
    }
    .unwrap_or(StepDetail::Truncated)
}

#[cfg(test)]
mod trace_tests {
    use crate::trace::{StepDetail, TraceCollector, TraceSink, TracingSink, VmStep, render_trace};
    use crate::{Function, Module, Opcode, Value, Vm, VmError, decode_module, encode_module};
    use std::any::Any;

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// helper(n) = n * 2 + main returning helper(21): the disasm golden
    /// module's exact shape, so the trace pins a real emitted program.
    fn golden_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1)); // MIR ABI: arg at local 1
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::Return));
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::with_strings(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            vec!["abc".to_string()],
            1,
        )
    }

    /// A `Vm` runs the golden module with an attached collector; the value
    /// must match the untraced result exactly (the seam is read-only).
    fn traced_golden() -> (TraceCollector, Value) {
        let module = golden_module();
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("golden module runs");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<TraceCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    const GOLDEN: &str = "\
trace glyim.module-image/2 steps=9
0: fn 1 pc 0 frame=1 stack=0 OP_LOAD_CONST 0
1: fn 1 pc 9 frame=1 stack=1 OP_LOAD_CONST 21
2: fn 1 pc 18 frame=1 stack=2 OP_CALL argc=1 dest=0 t31
3: fn 0 pc 0 frame=2 stack=0 OP_LOAD_CONST 2
4: fn 0 pc 9 frame=2 stack=1 OP_LOAD_LOCAL 1
5: fn 0 pc 14 frame=2 stack=2 OP_MUL
6: fn 0 pc 15 frame=2 stack=1 OP_RETURN
7: fn 1 pc 31 frame=1 stack=0 OP_LOAD_LOCAL 0
8: fn 1 pc 36 frame=1 stack=1 OP_RETURN
";

    #[test]
    fn golden_pinned_trace() {
        let (collector, value) = traced_golden();
        assert_eq!(value, Value::Int(42));
        let rendered = render_trace(collector.steps(), &golden_module());
        assert_eq!(rendered, GOLDEN);
    }

    #[test]
    fn render_is_deterministic() {
        let module = golden_module();
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(&module).expect("runs");
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        let steps = collector.steps().to_vec();
        assert_eq!(render_trace(&steps, &module), render_trace(&steps, &module));
    }

    /// The trace is read-only: the traced result equals the untraced result.
    #[test]
    fn traced_result_matches_untraced() {
        let module = golden_module();
        let untraced = Vm::new().run_module(&module).expect("untraced runs");
        let (collector, traced) = traced_golden();
        assert_eq!(traced, untraced);
        assert_eq!(collector.steps().len(), 9);
        assert!(!collector.overflowed());
    }

    /// A VM without a sink behaves exactly as before: no steps, no panic.
    #[test]
    fn no_sink_by_default() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_trace().is_none());
    }

    /// A cross-function call records the frame-depth transition (1 → 2 → 1)
    /// and both functions' indices on the executed steps.
    #[test]
    fn call_records_frame_depth_transition() {
        let (collector, _) = traced_golden();
        let depths: Vec<(usize, usize)> = collector
            .steps()
            .iter()
            .map(|s| (s.func, s.frame_depth))
            .collect();
        assert_eq!(
            depths,
            vec![
                (1, 1),
                (1, 1),
                (1, 1),
                (0, 2),
                (0, 2),
                (0, 2),
                (0, 2),
                (1, 1),
                (1, 1),
            ],
            "fn/frame-depth pairs across the call"
        );
    }

    /// Branchy code records only the executed path; the `JumpIf` detail
    /// carries the block target the instruction resolves against.
    #[test]
    fn trace_follows_taken_path_only() {
        // b0: load_local 0; load_const 0; lt; jump_if t1; load_const 1;
        //     return
        // b1: load_const 2; return
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Lt));
        code.push(op(Opcode::JumpIf));
        code.extend_from_slice(&u32le(1)); // block index 1
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Return));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::Return));
        let f = Function::with_blocks(code, 1, 1, vec![0, 30]);
        let module = Module::new(vec![f], 0);

        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(1), "fall-through returns load_const 1");
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");

        // arg 0 → local 0: 0 < 0 is false, so the JumpIf does NOT take the
        // branch; execution falls through to `load_const 1; return`. The
        // `load_const 2` block never executes.
        let ops: Vec<(Opcode, Option<StepDetail>)> = collector
            .steps()
            .iter()
            .map(|s| (s.opcode.expect("known op"), Some(s.detail)))
            .collect();
        assert_eq!(
            ops,
            vec![
                (Opcode::LoadLocal, Some(StepDetail::Local(0))),
                (Opcode::LoadConst, Some(StepDetail::ConstInt(0))),
                (Opcode::Lt, Some(StepDetail::None)),
                (Opcode::JumpIf, Some(StepDetail::Branch { target: 1 })),
                (Opcode::LoadConst, Some(StepDetail::ConstInt(1))),
                (Opcode::Return, Some(StepDetail::None)),
            ],
            "the executed path, not the listed path"
        );
    }

    /// An unknown opcode byte is recorded (`opcode: None`) and the run still
    /// fails with the explicit error — the trace is total over faulting runs.
    #[test]
    fn unknown_opcode_recorded_then_errors() {
        let code = vec![0x42, op(Opcode::Return)];
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let err = vm.run_module(&module).expect_err("0x42 is not an opcode");
        assert_eq!(err, VmError::UnknownOpcode(0x42));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        let steps = collector.steps();
        assert_eq!(steps.len(), 1, "the faulting dispatch is recorded");
        assert_eq!(steps[0].opcode, None);
        assert_eq!(steps[0].raw, 0x42);
    }

    /// A payload that runs past the code's end records `Truncated` and the
    /// run fails with the VM's explicit end-of-code error.
    #[test]
    fn truncated_payload_records_truncated() {
        let code = vec![op(Opcode::LoadConst), 0xAA];
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let err = vm.run_module(&module).expect_err("payload truncated");
        assert_eq!(err, VmError::UnexpectedEndOfCode);
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        let steps = collector.steps();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].detail, StepDetail::Truncated);
    }

    /// Float constants record the raw IEEE-754 bit pattern (the disasm
    /// convention: a corrupted bit pattern stays visible).
    #[test]
    fn float_const_records_bits() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstF64));
        code.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Float(1.5));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        assert_eq!(
            collector.steps()[0].detail,
            StepDetail::ConstFloatBits(0x3ff8000000000000)
        );
        let rendered = render_trace(collector.steps(), &module);
        assert!(
            rendered.contains("OP_LOAD_CONST_F64 1.5 (bits 0x3ff8000000000000)"),
            "float step in {rendered}"
        );
    }

    /// String constants record the pool index and whether it resolved;
    /// the render shows the pool content like the disassembler does.
    #[test]
    fn string_const_records_pool_state() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(9));
        code.push(op(Opcode::Return));
        let module = Module::with_strings(
            vec![Function::new(code, 0, 0)],
            vec!["hello".to_string()],
            0,
        );
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let err = vm.run_module(&module).expect_err("pool[9] is out of range");
        assert_eq!(err, VmError::StringIndexOutOfBounds(9));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        let steps = collector.steps();
        assert_eq!(
            steps[0].detail,
            StepDetail::ConstStr {
                pool_index: 0,
                resolved: true
            }
        );
        assert_eq!(
            steps[1].detail,
            StepDetail::ConstStr {
                pool_index: 9,
                resolved: false
            }
        );
        let rendered = render_trace(collector.steps(), &module);
        assert!(rendered.contains("pool[0] \"hello\""), "pool in {rendered}");
        assert!(
            rendered.contains("pool[9] <out of range>"),
            "out-of-range in {rendered}"
        );
    }

    /// `OP_CAST` records the cast kind and target bytes; the render names
    /// them exactly like the disassembler does.
    #[test]
    fn cast_records_kind_and_target() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(300));
        code.push(op(Opcode::Cast));
        code.push(0); // IntToInt
        code.push(6); // U8
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("IntToInt runs");
        assert_eq!(value, Value::Int(44), "300 as u8 wraps to 44 (MIR-14)");
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        assert_eq!(
            collector.steps()[1].detail,
            StepDetail::Cast { kind: 0, target: 6 }
        );
        let rendered = render_trace(collector.steps(), &module);
        assert!(
            rendered.contains("OP_CAST IntToInt->U8"),
            "cast in {rendered}"
        );
    }

    /// A bounded collector marks overflow instead of growing without limit
    /// (an infinite loop must not OOM the embedding host).
    #[test]
    fn bounded_collector_marks_overflow() {
        let module = golden_module();
        let collector = TraceCollector::bounded(3);
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        assert_eq!(collector.steps().len(), 3, "capture stops at the cap");
        assert!(collector.overflowed());
    }

    /// The image round trip composes with the trace: decode the shipped
    /// bytes, then trace the loaded module.
    #[test]
    fn trace_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&loaded).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<TraceCollector>().expect("collector");
        assert_eq!(collector.steps().len(), 9);
    }

    /// `TracingSink` is total: attaching it changes nothing about the run
    /// and never panics (with or without a subscriber installed).
    #[test]
    fn tracing_sink_is_total() {
        let module = golden_module();
        let mut vm = Vm::new().with_trace(Box::new(TracingSink));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        // A step can also be delivered directly.
        TracingSink.step(&VmStep {
            func: 0,
            pc: 0,
            raw: Opcode::Return as u8,
            opcode: Some(Opcode::Return),
            frame_depth: 1,
            stack_depth: 0,
            detail: StepDetail::None,
        });
    }
}
