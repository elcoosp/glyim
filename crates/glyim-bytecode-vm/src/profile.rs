//! Execution-profile seam: aggregate what a [`crate::Vm`] executed.
//!
//! The trace seam ([`crate::trace`]) is the *sequential* half of the
//! dynamic observability story: it records every dispatch in order, which
//! answers "what did this module *do*?" at step granularity. A full trace of
//! a long run is a linear transcript — the raw material a consumer then has
//! to aggregate by hand. This module is the *aggregate* counterpart: a
//! [`TraceSink`] that folds the same per-dispatch stream into an
//! [`ProfileCollector`] — per-opcode dispatch counts, per-function dispatch
//! counts, and the peak frame/stack depths — so an embedding host can answer
//! "where did the dispatches go?" over an arbitrarily long run while holding
//! O(#distinct opcodes + #functions) memory instead of O(#steps).
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change execution), **total** (every dispatch is folded in,
//! including the faulting dispatch that produces a `VmError`, and an unknown
//! opcode byte folds into its own `unknown(0xNN)` bucket instead of being
//! skipped), **bounded** (the collector's memory is a function of the
//! module's shape, never of the run length — an infinite loop accumulates
//! counts without growing the allocation), and **deterministic**
//! ([`render_profile`] renders the same aggregate byte-identically — the
//! test suite pins a golden).
//!
//! The collector composes with every other seam: attach it through
//! [`crate::Vm::with_trace`] exactly like any other sink (it impls
//! [`TraceSink`]), recover it after the run through
//! [`crate::Vm::take_trace`] + [`TraceSink::as_any`], re-aggregate a
//! captured [`TraceCollector`] transcript post-hoc with
//! [`ProfileCollector::from_steps`], and pair it with
//! [`crate::Vm::with_fuel`] — the refused dispatch never reaches the hook,
//! so the profile ends exactly at the executed count.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::trace::{TraceSink, VmStep};
use crate::{Module, Opcode};
use tracing::debug;

/// The observer the VM hands each executed step to: the aggregate fold of
/// the trace stream. Attach through [`crate::Vm::with_trace`], recover
/// through [`crate::Vm::take_trace`] + [`TraceSink::as_any`].
#[derive(Clone, Default)]
pub struct ProfileCollector {
    /// Dispatch count per raw opcode byte (unknown bytes keep their own
    /// bucket — the profile is total over corrupt code).
    opcodes: BTreeMap<u8, u64>,
    /// Dispatch count per executing function index.
    fns: BTreeMap<usize, u64>,
    /// Total dispatches observed (the sum of every bucket).
    steps: u64,
    /// The deepest call-frame stack the run reached.
    peak_frame: usize,
    /// The deepest operand stack the run reached.
    peak_stack: usize,
}

/// One entry of [`ProfileCollector::hottest`]: a raw opcode byte, its
/// decoded opcode (`None` for an unknown byte — the byte survives), and how
/// many dispatches it accounted for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotOpcode {
    /// The raw opcode byte (an unknown byte's value survives here).
    pub raw: u8,
    /// The decoded opcode, or `None` for a byte that is not one.
    pub opcode: Option<Opcode>,
    /// How many dispatches carried this byte.
    pub dispatches: u64,
}

impl ProfileCollector {
    /// Create an empty profile (no dispatches observed).
    #[must_use]
    pub fn new() -> ProfileCollector {
        ProfileCollector::default()
    }

    /// Aggregate a captured step list post-hoc. The result is identical to
    /// what a live-attached collector would have produced over the same
    /// steps — the profile is a pure function of the step stream.
    #[must_use]
    pub fn from_steps(steps: &[VmStep]) -> ProfileCollector {
        let mut profile = ProfileCollector::new();
        for step in steps {
            profile.step(step);
        }
        profile
    }

    /// Total dispatches observed (including the faulting dispatch of a run
    /// that ended in a `VmError`).
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Whether no dispatch was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps == 0
    }

    /// Dispatch counts per raw opcode byte (ascending byte order — an
    /// unknown byte keeps its own bucket).
    #[must_use]
    pub fn opcode_counts(&self) -> &BTreeMap<u8, u64> {
        &self.opcodes
    }

    /// Dispatch count for one raw opcode byte (`0` when never observed).
    #[must_use]
    pub fn opcode_count(&self, raw: u8) -> u64 {
        self.opcodes.get(&raw).copied().unwrap_or(0)
    }

    /// Dispatch counts per executing function index (ascending order;
    /// a function the run never entered simply has no entry).
    #[must_use]
    pub fn function_counts(&self) -> &BTreeMap<usize, u64> {
        &self.fns
    }

    /// Dispatch count for one function index (`0` when never observed).
    #[must_use]
    pub fn function_count(&self, func: usize) -> u64 {
        self.fns.get(&func).copied().unwrap_or(0)
    }

    /// The deepest call-frame stack the run reached (`1` = the entry frame).
    #[must_use]
    pub fn peak_frame_depth(&self) -> usize {
        self.peak_frame
    }

    /// The deepest operand stack the run reached at dispatch.
    #[must_use]
    pub fn peak_stack_depth(&self) -> usize {
        self.peak_stack
    }

    /// The `n` most-dispatched opcodes, hottest first; ties break by raw
    /// byte ascending, so the ordering is a total, deterministic order.
    /// Fewer than `n` entries when fewer distinct bytes were observed.
    #[must_use]
    pub fn hottest(&self, n: usize) -> Vec<HotOpcode> {
        let mut ranked: Vec<(u64, u8)> = self
            .opcodes
            .iter()
            .map(|(raw, count)| (*count, *raw))
            .collect();
        // (count desc, byte asc): negate the count for a single-key sort on
        // (byte asc) order.
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        ranked
            .into_iter()
            .take(n)
            .map(|(dispatches, raw)| HotOpcode {
                raw,
                opcode: Opcode::from_u8(raw),
                dispatches,
            })
            .collect()
    }
}

impl TraceSink for ProfileCollector {
    fn step(&mut self, step: &VmStep) {
        *self.opcodes.entry(step.raw).or_insert(0) += 1;
        *self.fns.entry(step.func).or_insert(0) += 1;
        self.steps += 1;
        self.peak_frame = self.peak_frame.max(step.frame_depth);
        self.peak_stack = self.peak_stack.max(step.stack_depth);
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// The label one raw opcode byte carries in the profile listing: the
/// disassembler's mnemonic for a decoded byte, `unknown(0xNN)` otherwise
/// (the byte's value stays visible — the same honesty rule the disassembler
/// and the trace render follow).
fn op_label(raw: u8) -> String {
    match Opcode::from_u8(raw) {
        Some(op) => crate::disasm::op_name(op).to_string(),
        None => format!("unknown(0x{raw:02x})"),
    }
}

/// Render an execution profile into a deterministic listing (the aggregate
/// counterpart of [`crate::render_trace`]): a header line (image format
/// version, total steps, peak depths), the per-opcode dispatch counts
/// hottest-first (ties by raw byte), the per-function dispatch counts over
/// every function of `module` (a `0` line for a function the run never
/// entered — the listing is total over the module, not just the executed
/// subset), and the hottest-five summary line.
///
/// Deterministic (the same profile renders byte-identical text — the test
/// suite pins a golden) and total (unknown opcode bytes render their
/// `unknown(0xNN)` bucket).
#[must_use]
pub fn render_profile(profile: &ProfileCollector, module: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "profile glyim.module-image/{} steps={} peak_frame={} peak_stack={}",
        crate::IMAGE_VERSION,
        profile.steps(),
        profile.peak_frame_depth(),
        profile.peak_stack_depth()
    );
    let _ = writeln!(out, "opcodes:");
    for hot in profile.hottest(usize::MAX) {
        let _ = writeln!(out, "{} {}", hot.dispatches, op_label(hot.raw));
    }
    let _ = writeln!(out, "functions:");
    for func in 0..module.functions.len() {
        let _ = writeln!(out, "fn {func}: {}", profile.function_count(func));
    }
    let top: Vec<String> = profile
        .hottest(5)
        .into_iter()
        .map(|hot| format!("{}={}", op_label(hot.raw), hot.dispatches))
        .collect();
    if !top.is_empty() {
        let _ = writeln!(out, "hottest: {}", top.join(", "));
    }
    debug!(steps = profile.steps(), "vm profile rendered");
    out
}

#[cfg(test)]
mod profile_tests {
    use crate::trace::{TraceCollector, VmStep};
    use crate::{Function, Module, Opcode, Value, Vm, VmError, decode_module, encode_module};
    use std::any::Any;

    use super::{ProfileCollector, render_profile};

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// helper(n) = n * 2 + main returning helper(21): the disasm/trace
    /// golden module's exact shape, so the profile pins a real emitted
    /// program the other seams already cover.
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

    /// A `Vm` runs the golden module with an attached profile collector;
    /// the value must match the unprofiled result exactly (the seam is
    /// read-only).
    fn profiled_golden() -> (ProfileCollector, Value) {
        let module = golden_module();
        let collector = ProfileCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("golden module runs");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<ProfileCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    const GOLDEN: &str = "\
profile glyim.module-image/2 steps=9 peak_frame=2 peak_stack=2
opcodes:
3 OP_LOAD_CONST
2 OP_LOAD_LOCAL
2 OP_RETURN
1 OP_MUL
1 OP_CALL
functions:
fn 0: 4
fn 1: 5
hottest: OP_LOAD_CONST=3, OP_LOAD_LOCAL=2, OP_RETURN=2, OP_MUL=1, OP_CALL=1
";

    #[test]
    fn golden_pinned_profile() {
        let (profile, value) = profiled_golden();
        assert_eq!(value, Value::Int(42));
        let rendered = render_profile(&profile, &golden_module());
        assert_eq!(rendered, GOLDEN);
    }

    #[test]
    fn render_is_deterministic() {
        let (profile, _) = profiled_golden();
        let module = golden_module();
        assert_eq!(
            render_profile(&profile, &module),
            render_profile(&profile, &module)
        );
    }

    /// The profile is read-only: the profiled result equals the unprofiled
    /// result, and the aggregate matches the step stream's own arithmetic.
    #[test]
    fn profiled_result_matches_unprofiled() {
        let module = golden_module();
        let unprofiled = Vm::new().run_module(&module).expect("unprofiled runs");
        let (profile, profiled) = profiled_golden();
        assert_eq!(profiled, unprofiled);
        assert_eq!(profile.steps(), 9);
        assert_eq!(profile.opcode_count(op(Opcode::LoadConst)), 3);
        assert_eq!(profile.opcode_count(op(Opcode::Call)), 1);
        assert_eq!(profile.opcode_count(op(Opcode::Mul)), 1);
        assert_eq!(profile.opcode_count(op(Opcode::Return)), 2);
        assert_eq!(profile.opcode_count(op(Opcode::LoadLocal)), 2);
    }

    /// Per-function dispatch counts: the call shape lands 4 dispatches in
    /// the helper (fn 0) and 5 in the entry (fn 1).
    #[test]
    fn per_function_counts() {
        let (profile, _) = profiled_golden();
        assert_eq!(profile.function_count(0), 4);
        assert_eq!(profile.function_count(1), 5);
        assert_eq!(profile.function_count(2), 0, "no fn 2 in the module");
    }

    /// The call records the depth peaks: frame stack reaches 2, operand
    /// stack reaches 2 at the call/mul dispatches.
    #[test]
    fn peaks_recorded() {
        let (profile, _) = profiled_golden();
        assert_eq!(profile.peak_frame_depth(), 2);
        assert_eq!(profile.peak_stack_depth(), 2);
    }

    /// An unknown opcode byte folds into its own `unknown(0xNN)` bucket and
    /// the run still fails with the explicit error — the profile is total
    /// over faulting runs.
    #[test]
    fn unknown_opcode_counts_honestly() {
        let code = vec![0x42, op(Opcode::Return)];
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let profile = ProfileCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(profile));
        let err = vm.run_module(&module).expect_err("0x42 is not an opcode");
        assert_eq!(err, VmError::UnknownOpcode(0x42));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let profile = sink.downcast_mut::<ProfileCollector>().expect("collector");
        assert_eq!(profile.steps(), 1, "the faulting dispatch is folded");
        assert_eq!(profile.opcode_count(0x42), 1);
        let rendered = render_profile(profile, &module);
        assert!(rendered.contains("1 unknown(0x42)"), "bucket in {rendered}");
        assert!(
            rendered.contains("hottest: unknown(0x42)=1"),
            "hot in {rendered}"
        );
        let hot = profile.hottest(1);
        assert_eq!(hot[0].raw, 0x42);
        assert_eq!(hot[0].opcode, None, "an unknown byte decodes to None");
        assert_eq!(hot[0].dispatches, 1);
    }

    /// The profile composes with the fuel budget: the refused dispatch
    /// never reaches the hook, so the profile's total equals the executed
    /// count exactly.
    #[test]
    fn composes_with_fuel() {
        let mut code = Vec::new();
        code.push(op(Opcode::Jump));
        code.extend_from_slice(&0u32.to_le_bytes()); // self-jump: b0 -> b0
        let module = Module::new(vec![Function::with_blocks(code, 0, 0, vec![0])], 0);
        let profile = ProfileCollector::new();
        let mut vm = Vm::new().with_fuel(500).with_trace(Box::new(profile));
        let err = vm
            .run_module(&module)
            .expect_err("a self-jump loop cannot halt");
        assert_eq!(err, VmError::FuelExhausted { executed: 500 });
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let profile = sink.downcast_mut::<ProfileCollector>().expect("collector");
        assert_eq!(profile.steps(), 500);
        assert_eq!(profile.steps(), vm.executed());
        assert_eq!(profile.opcode_count(op(Opcode::Jump)), 500);
        assert_eq!(profile.function_count(0), 500);
    }

    /// `from_steps` re-aggregates a captured transcript into the identical
    /// profile — the fold is a pure function of the step stream.
    #[test]
    fn from_steps_matches_live_profile() {
        let module = golden_module();
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(&module).expect("runs");
        let mut boxed = vm.take_trace().expect("attached");
        let steps: Vec<VmStep> = boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("collector")
            .steps()
            .to_vec();
        let live = ProfileCollector::from_steps(&steps);
        let profile = ProfileCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(profile));
        let _ = vm.run_module(&module).expect("runs");
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let direct = sink.downcast_mut::<ProfileCollector>().expect("collector");
        assert_eq!(live.steps(), direct.steps());
        assert_eq!(live.opcode_counts(), direct.opcode_counts());
        assert_eq!(live.function_counts(), direct.function_counts());
        assert_eq!(live.peak_frame_depth(), direct.peak_frame_depth());
        assert_eq!(live.peak_stack_depth(), direct.peak_stack_depth());
        assert_eq!(
            render_profile(&live, &module),
            render_profile(direct, &module)
        );
    }

    /// An empty profile renders the header and a zero line per module
    /// function — no opcode lines, no hottest line.
    #[test]
    fn empty_profile_renders_header_only() {
        let profile = ProfileCollector::new();
        assert!(profile.is_empty());
        let rendered = render_profile(&profile, &golden_module());
        assert_eq!(
            rendered,
            "\
profile glyim.module-image/2 steps=0 peak_frame=0 peak_stack=0
opcodes:
functions:
fn 0: 0
fn 1: 0
"
        );
    }

    /// `hottest` orders by dispatch count descending with raw-byte
    /// tie-breaks; `hottest(0)` is empty and an oversized `n` is harmless.
    #[test]
    fn hottest_orders_and_ties() {
        let (profile, _) = profiled_golden();
        let top2 = profile.hottest(2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].opcode, Some(Opcode::LoadConst));
        assert_eq!(top2[0].dispatches, 3);
        // LoadLocal (0x16) and Return (0x18) tie at 2: the lower byte wins.
        assert_eq!(top2[1].opcode, Some(Opcode::LoadLocal));
        assert_eq!(profile.hottest(0), Vec::new());
        assert_eq!(profile.hottest(999).len(), 5, "capped at distinct ops");
    }

    /// The typed sink is recoverable through `as_any` after
    /// `Vm::take_trace` (the same downcast path every sink rides).
    #[test]
    fn as_any_recovers_typed_sink() {
        let module = golden_module();
        let mut vm = Vm::new().with_trace(Box::new(ProfileCollector::new()));
        let _ = vm.run_module(&module).expect("runs");
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let profile = sink.downcast_ref::<ProfileCollector>().expect("collector");
        assert_eq!(profile.steps(), 9);
    }

    /// The image round trip composes with the profile: decode the shipped
    /// bytes, then profile the loaded module.
    #[test]
    fn profile_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let profile = ProfileCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(profile));
        let value = vm.run_module(&loaded).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let profile = sink.downcast_ref::<ProfileCollector>().expect("collector");
        assert_eq!(profile.steps(), 9);
        assert_eq!(render_profile(profile, &loaded), GOLDEN);
    }
}
