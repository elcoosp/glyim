//! Fault-backtrace seam: capture where a [`crate::Vm`] run failed and the
//! call chain that led there.
//!
//! The trace seam ([`crate::trace`]) is the *sequential* half of the
//! dynamic observability story and the profile seam ([`crate::profile`])
//! its aggregate half — but both answer questions about a run that is
//! described by its steps. When a loaded artifact *faults*, the embedding
//! host wants the failure itself diagnosed: which instruction faulted, and
//! which call chain led to it. The bare [`crate::VmError`] carries none of
//! that (an `AbnormalTermination` says nothing about where or how deep),
//! and reconstructing it from a trace means attaching a collector and
//! scanning the transcript after the fact. This module is the
//! *failure-time* counterpart: a hook fired exactly once when a run ends in
//! a [`crate::VmError`], carrying the fault site, the caller chain
//! (innermost first, a Rust-style backtrace), the lifetime dispatch count,
//! and the fuel state.
//!
//! # Contract
//!
//! The seam is **failure-only** (the hook fires once per run that ends in a
//! [`crate::VmError`], and never on a successful run), **read-only** (the
//! sink observes a snapshot; it cannot change execution, and the error the
//! caller receives is untouched), **total** (every error path fires — the
//! pre-execution faults of [`crate::Vm::run_module`], an unknown opcode
//! byte, the fuel gate's refused dispatch, every `VmError` variant — and
//! the report says what is known: a pre-execution fault reports the entry
//! request with zero dispatches instead of an invented instruction), and
//! **zero-cost when unused** (the machine's current dispatch site is two
//! integer stores per dispatch; the hook itself is one `None` check per
//! *failed* run — a clean run pays nothing beyond the stores).
//!
//! [`BacktraceCollector`] captures the report in memory (a run ends at its
//! first fault, so one captured report is the whole story), and
//! [`render_backtrace`] renders it into the same deterministic
//! diagnostic-text style the disassembler and the trace renderer use. The
//! seam composes with every other one: attach it alongside a trace sink or
//! the fuel budget, and run it over a decoded [`crate::decode_module`]
//! image like any other.

use std::any::Any;
use std::fmt::Write as _;

use crate::{Module, Opcode, VmError};
use tracing::debug;

/// One caller frame of a fault backtrace: the suspended function's index
/// and its resume pc — the offset of the *next* instruction the frame would
/// have executed (for a caller suspended in `OP_CALL`, the instruction
/// after the call, which is exactly where the callee's `Return` resumes
/// it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultFrame {
    /// The suspended caller's index in the module.
    pub func: usize,
    /// The offset the caller resumes at when the callee returns.
    pub pc: usize,
}

/// Where a run faulted: the dispatch site (the faulting instruction's start
/// pc) and its decoded opcode. `opcode` is `None` for an unknown opcode
/// byte (the error's `Display` carries the byte) and for a pre-execution
/// fault (nothing has dispatched yet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultSite {
    /// The faulting function's index in the module.
    pub func: usize,
    /// The faulting instruction's start offset in the function's code (for
    /// a pre-execution fault, the entry request: `pc == 0`).
    pub pc: usize,
    /// The decoded opcode, or `None` for an unknown byte / pre-execution
    /// fault.
    pub opcode: Option<Opcode>,
}

/// The report the VM hands to a [`FaultSink`] when a run ends in a
/// [`VmError`]: the fault site, the caller chain (innermost caller first —
/// the order a Rust panic backtrace reads in), the lifetime dispatch count,
/// and the remaining fuel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaultReport {
    /// The error the run failed with (the caller receives the same value).
    pub error: VmError,
    /// Where the run faulted.
    pub site: FaultSite,
    /// The suspended caller chain, innermost caller first; empty when the
    /// fault is in the entry frame itself (or before any frame existed).
    pub callers: Vec<FaultFrame>,
    /// How many instruction dispatches the VM had performed when the run
    /// failed (a pre-execution fault reports `0`).
    pub executed: u64,
    /// The remaining fuel budget, when one was set ([`crate::Vm::with_fuel`]);
    /// `None` when the run was unlimited.
    pub fuel_left: Option<u64>,
}

/// The observer the VM hands the fault report to when a run fails.
///
/// Implementors must treat [`FaultSink::fault`] as an observation only: the
/// run is already over, the report is a snapshot, and the caller receives
/// the same [`VmError`] regardless of what the sink does. The method must
/// not panic (the VM never defends against a panicking sink — a sink that
/// panics aborts the host, which is the embedding host's explicit choice).
/// The [`FaultSink::as_any`] hook lets a caller recover the typed sink
/// after [`crate::Vm::take_backtrace`].
pub trait FaultSink: 'static {
    /// The run ended in `report.error`; `report` describes where and how
    /// deep.
    fn fault(&mut self, report: &FaultReport);

    /// Downcast hook: hand the sink back to its typed owner after
    /// [`crate::Vm::take_backtrace`].
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures the fault report of the last failed run.
///
/// A run ends at its first fault — the machine stops at the error — so one
/// captured report is the whole story; attaching the collector across
/// several runs keeps the *latest* failure (the previous report is
/// overwritten).
#[derive(Clone, Default)]
pub struct BacktraceCollector {
    /// The captured report (`None` until a run has failed with this sink
    /// attached).
    report: Option<FaultReport>,
}

impl BacktraceCollector {
    /// Create an empty collector (nothing captured yet).
    #[must_use]
    pub fn new() -> BacktraceCollector {
        BacktraceCollector { report: None }
    }

    /// The captured fault report, or `None` while no run has failed with
    /// this collector attached.
    #[must_use]
    pub fn report(&self) -> Option<&FaultReport> {
        self.report.as_ref()
    }

    /// Consume the collector into its captured report.
    #[must_use]
    pub fn into_report(self) -> Option<FaultReport> {
        self.report
    }

    /// Whether nothing has been captured yet (no run has failed with this
    /// collector attached).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.report.is_none()
    }
}

impl FaultSink for BacktraceCollector {
    fn fault(&mut self, report: &FaultReport) {
        self.report = Some(report.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a fault report into a deterministic backtrace listing (the
/// failure-time counterpart of [`crate::render_trace`]): a header line
/// (image format version, executed count, and the fuel budget when one was
/// set), then the fault line (site, opcode mnemonic, the error), then one
/// line per suspended caller, innermost first, each with the opcode at its
/// resume pc.
///
/// Deterministic (the same report renders byte-identical text — the test
/// suite pins a golden) and total (any report renders: a pre-execution
/// fault's site renders `before first dispatch`, a caller whose resume pc
/// sits at or past the end of its code renders `<end>`, an undecodable
/// byte renders `?`). `module` supplies the code the caller lines decode
/// their resume opcodes against.
#[must_use]
pub fn render_backtrace(report: &FaultReport, module: &Module) -> String {
    let mut out = String::new();
    // Header: the format version the image seam ships, the dispatch count,
    // and the fuel budget when one was set (a spent budget reads
    // `fuel_left=0`).
    match report.fuel_left {
        Some(fuel) => {
            let _ = writeln!(
                out,
                "backtrace glyim.module-image/{} executed={} fuel_left={fuel}",
                crate::IMAGE_VERSION,
                report.executed
            );
        }
        None => {
            let _ = writeln!(
                out,
                "backtrace glyim.module-image/{} executed={}",
                crate::IMAGE_VERSION,
                report.executed
            );
        }
    }
    // The fault line: the site's opcode mnemonic when the byte decodes
    // (`before first dispatch` for a pre-execution fault, `?` for a byte
    // that is not an opcode — the error's Display carries the byte), then
    // the error itself.
    let mnemonic = match report.site.opcode {
        Some(op) => crate::disasm::op_name(op).to_string(),
        None if report.executed == 0 => "before first dispatch".to_string(),
        None => "?".to_string(),
    };
    let _ = writeln!(
        out,
        "  0: fn {} pc {} {mnemonic}: {}",
        report.site.func, report.site.pc, report.error
    );
    // The suspended caller chain, innermost first; each line decodes the
    // opcode at the frame's resume pc (where the callee's `Return` would
    // resume it).
    for (i, frame) in report.callers.iter().enumerate() {
        let mnemonic = module
            .functions
            .get(frame.func)
            .and_then(|f| f.code.get(frame.pc))
            .map(|&raw| match Opcode::from_u8(raw) {
                Some(op) => crate::disasm::op_name(op).to_string(),
                None => "?".to_string(),
            })
            .unwrap_or_else(|| "<end>".to_string());
        let _ = writeln!(
            out,
            "  {}: fn {} pc {} {mnemonic} (resume)",
            i + 1,
            frame.func,
            frame.pc
        );
    }
    debug!(
        executed = report.executed,
        callers = report.callers.len(),
        "vm fault backtrace rendered"
    );
    out
}

#[cfg(test)]
mod backtrace_tests {
    use crate::backtrace::{
        BacktraceCollector, FaultFrame, FaultReport, FaultSite, render_backtrace,
    };
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

    /// helper(n) = n * 2, but the body traps: the disasm/trace golden
    /// module's call shape with the callee faulting at its first dispatch.
    /// `main` (fn 1) calls helper (fn 0) at byte 18; the call's operands
    /// end at byte 31, so main's resume pc is 31 — the same `t31` the
    /// trace golden pins.
    fn trapping_golden() -> Module {
        let helper = vec![op(Opcode::Trap)];
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

    /// Run the trapping golden module with an attached collector; the error
    /// must match the untrapped machine's, and the collector must hold the
    /// report.
    fn faulted_golden() -> (BacktraceCollector, VmError) {
        let module = trapping_golden();
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the callee traps at its first dispatch");
        let mut boxed = vm.take_backtrace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, err)
    }

    const GOLDEN: &str = "\
backtrace glyim.module-image/2 executed=4
  0: fn 0 pc 0 OP_TRAP: OP_TRAP executed
  1: fn 1 pc 31 OP_LOAD_LOCAL (resume)
";

    /// The golden fault renders byte-identically: fault line first, then
    /// the suspended caller with the opcode at its resume pc.
    #[test]
    fn golden_pinned_backtrace() {
        let (collector, err) = faulted_golden();
        assert_eq!(err, VmError::AbnormalTermination);
        let report = collector.report().expect("the fault was captured");
        assert_eq!(
            report.site,
            FaultSite {
                func: 0,
                pc: 0,
                opcode: Some(Opcode::Trap),
            }
        );
        assert_eq!(
            report.callers,
            vec![FaultFrame { func: 1, pc: 31 }],
            "main is suspended at its resume pc 31"
        );
        assert_eq!(report.executed, 4, "two consts, the call, the trap");
        assert_eq!(report.fuel_left, None);
        let rendered = render_backtrace(report, &trapping_golden());
        assert_eq!(rendered, GOLDEN);
    }

    /// A successful run never fires the hook: the collector stays empty and
    /// `take_backtrace` hands the sink back untouched.
    #[test]
    fn success_never_fires() {
        let module = Module::new(
            vec![Function::new(
                {
                    let mut code = Vec::new();
                    code.push(op(Opcode::LoadConst));
                    code.extend_from_slice(&i64le(42));
                    code.push(op(Opcode::Return));
                    code
                },
                1,
                0,
            )],
            0,
        );
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let value = vm.run_module(&module).expect("clean run");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        assert!(collector.is_empty(), "no fault, no report");
        assert!(collector.report().is_none());
    }

    /// The seam is read-only: the faulted run returns exactly the error the
    /// untraced machine returns, and attaching the sink changes nothing.
    #[test]
    fn faulted_error_matches_untraced() {
        let module = trapping_golden();
        let untraced = Vm::new().run_module(&module).unwrap_err();
        let (_, err) = faulted_golden();
        assert_eq!(err, untraced);
    }

    /// A three-deep call chain (main → mid → leaf) faults with both
    /// callers suspended at their resume pcs, innermost first.
    #[test]
    fn nested_call_chain_is_innermost_first() {
        // leaf (fn 0): trap
        let leaf = vec![op(Opcode::Trap)];
        // mid (fn 1): call leaf (argc=0), then return — resume pc 22
        let mut mid = Vec::new();
        mid.push(op(Opcode::LoadConst));
        mid.extend_from_slice(&i64le(0)); // fn index 0: leaf
        mid.push(op(Opcode::Call));
        mid.extend_from_slice(&u32le(0)); // argc
        mid.extend_from_slice(&u32le(0)); // dest local
        mid.extend_from_slice(&u32le(22)); // resume byte offset
        mid.push(op(Opcode::Return));
        // main (fn 2): call mid (argc=0, resume pc 22), then return
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1)); // fn index 1: mid
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(0));
        main.extend_from_slice(&u32le(0));
        main.extend_from_slice(&u32le(22));
        main.push(op(Opcode::Return));
        let module = Module::new(
            vec![
                Function::new(leaf, 1, 0),
                Function::new(mid, 1, 0),
                Function::new(main, 1, 0),
            ],
            2,
        );
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = vm.run_module(&module).unwrap_err();
        assert_eq!(err, VmError::AbnormalTermination);
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(
            report.callers,
            vec![
                FaultFrame { func: 1, pc: 22 },
                FaultFrame { func: 2, pc: 22 },
            ],
            "mid suspended at 22, main at 22, innermost first"
        );
        let rendered = render_backtrace(report, &module);
        assert!(
            rendered.contains("  1: fn 1 pc 22 OP_RETURN (resume)"),
            "mid's resume pc decodes to OP_RETURN in {rendered}"
        );
        assert!(
            rendered.contains("  2: fn 2 pc 22 OP_RETURN (resume)"),
            "main's resume pc decodes to OP_RETURN in {rendered}"
        );
    }

    /// An unknown opcode byte faults with `opcode: None` and the byte in
    /// the error's Display — the report is total over corrupt code.
    #[test]
    fn unknown_opcode_fault_is_total() {
        let code = vec![0x42, op(Opcode::Return)];
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = vm.run_module(&module).unwrap_err();
        assert_eq!(err, VmError::UnknownOpcode(0x42));
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(report.site.opcode, None, "0x42 decodes to nothing");
        assert_eq!(report.site.func, 0);
        assert_eq!(report.site.pc, 0);
        let rendered = render_backtrace(report, &module);
        assert!(
            rendered.contains("unknown opcode byte 0x42"),
            "in {rendered}"
        );
    }

    /// A pre-execution fault (a module whose entry index is out of range)
    /// reports the entry request with zero dispatches and renders
    /// `before first dispatch` instead of an invented instruction.
    #[test]
    fn pre_execution_fault_reports_entry_request() {
        let module = Module::new(vec![], 0);
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = vm.run_module(&module).unwrap_err();
        assert_eq!(err, VmError::FunctionOutOfBounds(0));
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(report.executed, 0, "nothing dispatched yet");
        assert_eq!(
            report.site,
            FaultSite {
                func: 0,
                pc: 0,
                opcode: None
            }
        );
        assert!(report.callers.is_empty());
        let rendered = render_backtrace(report, &module);
        assert!(rendered.contains("before first dispatch"), "in {rendered}");
        assert!(rendered.contains("executed=0"), "in {rendered}");
    }

    /// The fuel gate's refused dispatch is the reported site, and the
    /// header shows the (spent) budget.
    #[test]
    fn fuel_fault_reports_the_refused_dispatch() {
        // Self-jump loop: block 0 jumps to itself forever.
        let mut code = Vec::new();
        code.push(op(Opcode::Jump));
        code.extend_from_slice(&u32le(0));
        let module = Module::new(vec![Function::with_blocks(code, 0, 0, vec![0])], 0);
        const FUEL: u64 = 5;
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new()
            .with_fuel(FUEL)
            .with_backtrace(Box::new(collector));
        let err = vm.run_module(&module).unwrap_err();
        assert_eq!(
            err,
            VmError::FuelExhausted { executed: FUEL },
            "the loop burns exactly the budget"
        );
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(report.executed, FUEL);
        assert_eq!(report.fuel_left, Some(0), "the budget is spent");
        assert_eq!(
            report.site,
            FaultSite {
                func: 0,
                pc: 0,
                opcode: Some(Opcode::Jump),
            },
            "the refused dispatch is the loop head"
        );
        let rendered = render_backtrace(report, &module);
        assert!(rendered.contains("fuel_left=0"), "in {rendered}");
    }

    /// Other faults under an active budget report the remaining fuel.
    #[test]
    fn fault_under_budget_reports_remaining_fuel() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(10));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Div)); // 10 / 0 traps
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_fuel(100).with_backtrace(Box::new(collector));
        let err = vm.run_module(&module).unwrap_err();
        assert_eq!(err, VmError::AbnormalTermination);
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(report.executed, 3, "two consts and the div");
        assert_eq!(report.fuel_left, Some(97), "100 - 3");
    }

    /// The render is deterministic: the same report renders
    /// byte-identical text.
    #[test]
    fn render_is_deterministic() {
        let (collector, _) = faulted_golden();
        let report = collector.into_report().expect("captured");
        let module = trapping_golden();
        assert_eq!(
            render_backtrace(&report, &module),
            render_backtrace(&report, &module)
        );
    }

    /// The image round trip composes with the seam: decode the shipped
    /// bytes, then fault the loaded module.
    #[test]
    fn backtrace_composes_with_module_image() {
        let module = trapping_golden();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = vm.run_module(&loaded).unwrap_err();
        assert_eq!(err, VmError::AbnormalTermination);
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(report.site.func, 0, "the callee's index survives the trip");
    }

    /// Cross-seam consistency: with a trace collector and a backtrace
    /// collector both attached, the backtrace's fault site is the trace's
    /// last recorded step (the faulting dispatch).
    #[test]
    fn backtrace_site_is_the_trace_last_step() {
        let module = trapping_golden();
        let trace = crate::TraceCollector::new();
        let backtrace = BacktraceCollector::new();
        let mut vm = Vm::new()
            .with_trace(Box::new(trace))
            .with_backtrace(Box::new(backtrace));
        let _ = vm.run_module(&module).unwrap_err();
        let mut boxed = vm.take_trace().expect("trace attached");
        let sink: &mut dyn Any = boxed.as_any();
        let trace = sink
            .downcast_mut::<crate::TraceCollector>()
            .expect("collector");
        let last = trace
            .steps()
            .last()
            .expect("the faulting dispatch is traced")
            .clone();
        drop(boxed);
        let mut boxed = vm.take_backtrace().expect("backtrace attached");
        let sink: &mut dyn Any = boxed.as_any();
        let backtrace = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = backtrace.report().expect("captured");
        assert_eq!(report.site.func, last.func);
        assert_eq!(report.site.pc, last.pc);
    }

    /// Call-frame overflow reports the overflowing `OP_CALL` as the fault
    /// site, with the full suspended chain beneath it.
    #[test]
    fn call_frame_overflow_reports_the_call_site() {
        // A function that calls itself forever: pc 0 loads fn idx 0 (self),
        // pc 9 loads the arg, pc 18 calls (operands end at 31, resume 31),
        // pc 31 returns.
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0)); // fn index 0: self
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Call));
        code.extend_from_slice(&u32le(1)); // argc
        code.extend_from_slice(&u32le(0)); // dest local
        code.extend_from_slice(&u32le(31)); // resume byte offset
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 2, 1)], 0);
        let collector = BacktraceCollector::new();
        let mut vm = Vm::new().with_backtrace(Box::new(collector));
        let err = vm.run_module(&module).unwrap_err();
        assert_eq!(err, VmError::CallFrameOverflow);
        let mut boxed = vm.take_backtrace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BacktraceCollector>()
            .expect("collector");
        let report = collector.report().expect("captured");
        assert_eq!(
            report.site,
            FaultSite {
                func: 0,
                pc: 18,
                opcode: Some(Opcode::Call)
            },
            "the overflowing call is the fault site"
        );
        assert_eq!(
            report.callers.len(),
            1023,
            "every frame but the faulting one is suspended (MAX_CALL_DEPTH 1024)"
        );
        assert_eq!(report.callers[0], FaultFrame { func: 0, pc: 31 });
    }

    /// `render_backtrace` is total over a caller whose resume pc sits at
    /// the end of its code (the `<end>` spelling, never a panic).
    #[test]
    fn caller_at_end_of_code_renders_end() {
        let report = FaultReport {
            error: VmError::AbnormalTermination,
            site: FaultSite {
                func: 0,
                pc: 0,
                opcode: Some(Opcode::Trap),
            },
            callers: vec![FaultFrame { func: 1, pc: 100 }],
            executed: 2,
            fuel_left: None,
        };
        let module = trapping_golden();
        let rendered = render_backtrace(&report, &module);
        assert!(rendered.contains("pc 100 <end>"), "in {rendered}");
    }
}
