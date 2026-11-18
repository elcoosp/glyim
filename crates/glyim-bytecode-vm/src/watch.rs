//! Local-watch seam: what values did a module's local slots take during a
//! run?
//!
//! The trace seam ([`crate::trace`]) answers *where* the machine went (one
//! step per dispatch), the profile seam ([`crate::profile`]) *how much* it
//! spent where, the coverage seam ([`crate::coverage`]) *which sites* ran,
//! and the backtrace seam ([`crate::backtrace`]) *why a run failed* — none
//! of them answer the data question. A `VmError::AssertionFailed` over a
//! countdown loop says nothing about what the loop's counter actually held
//! iteration by iteration; reconstructing that from a captured trace means
//! replaying the operand stack by hand. This module is the *data-flow*
//! counterpart: a hook that hands one [`WatchEvent`] to a [`WatchSink`] for
//! every value a local slot takes during a run — the program's own
//! `OP_STORE_LOCAL` writes, the ABI's call-argument binds (an `OP_CALL`
//! landing its arguments in the callee's locals `1..=arg_count`), and the
//! callee-return write that lands a return value in the caller's recorded
//! `dest_local`.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change execution), **total over the writes** (every successful
//! local write of the three kinds above is reported; a write that fails —
//! a `LocalOutOfBounds` store — writes nothing and reports nothing, and the
//! frames' zero-initialisation is allocation, not a write, so it is never
//! reported), **bounded** (the collector's capture cap marks overflow
//! instead of growing without limit over an infinite loop), and
//! **deterministic** ([`render_watch`] renders the same event list
//! byte-identically — the test suite pins a golden).
//!
//! Each [`WatchEvent`] carries the writer's function index, the slot, the
//! value (a clone — the machine keeps executing), the producing dispatch's
//! site (`pc`) and lifetime dispatch number (1-based, the same count
//! [`crate::Vm::executed`] reports), the frame depth (which of two
//! same-function frames wrote), and the [`WatchVia`] kind. The hook is one
//! `None` check per local write when no sink is attached (the same
//! cost model as the trace seam's one check per dispatch).
//!
//! The seam composes with every other one: attach a trace sink *and* a
//! watch sink to the same `Vm` (separate hooks — the trace rides the
//! dispatch loop, the watch rides the write paths), pair it with
//! [`crate::Vm::with_fuel`] (a refused dispatch never executes, so it never
//! writes), and round-trip the module through the image first — a
//! decoded artifact watches exactly like an in-memory one.

use std::any::Any;
use std::fmt::Write as _;

use crate::{Module, Value};
use tracing::debug;

/// How a value came to be written into a local slot — the three write
/// paths the seam reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchVia {
    /// An `OP_STORE_LOCAL` executed by the program.
    StoreLocal,
    /// An `OP_CALL` binding its arguments into the callee's locals
    /// `1..=arg_count` (the MIR ABI's argument slots).
    ArgBind,
    /// A callee's `OP_RETURN` landing its return value in the caller's
    /// recorded `dest_local`.
    CalleeReturn,
}

impl WatchVia {
    /// The listing name (the deterministic render's spelling).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            WatchVia::StoreLocal => "store-local",
            WatchVia::ArgBind => "arg-bind",
            WatchVia::CalleeReturn => "callee-return",
        }
    }
}

/// One local write: a value landed in a slot of a call frame.
///
/// `pc` is the producing dispatch's site — the `OP_STORE_LOCAL` itself for
/// [`WatchVia::StoreLocal`], the `OP_CALL` for [`WatchVia::ArgBind`], the
/// callee's `OP_RETURN` for [`WatchVia::CalleeReturn`] — and `dispatch` is
/// that dispatch's 1-based lifetime number (the same count
/// [`crate::Vm::executed`] reports after the run). `frame_depth` is the
/// depth of the frame that owns the slot (1 = the entry frame), which
/// disambiguates two same-function frames (recursion).
#[derive(Debug, Clone, PartialEq)]
pub struct WatchEvent {
    /// The index of the function whose frame owns the slot.
    pub func: usize,
    /// The slot index that was written.
    pub local: usize,
    /// The value the slot now holds.
    pub value: Value,
    /// The producing dispatch's pc within the *executing* function (see the
    /// per-variant note — for an [`WatchVia::ArgBind`] it is the caller's
    /// `OP_CALL` site, for a [`WatchVia::CalleeReturn`] the callee's return
    /// site).
    pub pc: usize,
    /// The producing dispatch's 1-based lifetime number.
    pub dispatch: u64,
    /// Depth of the frame that owns the slot (1 = the entry frame).
    pub frame_depth: usize,
    /// Which write path produced the value.
    pub via: WatchVia,
}

/// The observer the VM hands each local write to.
///
/// Implementors must treat [`WatchSink::write`] as an observation only: the
/// VM's state and control flow are invisible to the sink and unaffected by
/// it. The method must not panic (the VM never defends against a panicking
/// sink — a sink that panics aborts the run, which is the embedding host's
/// explicit choice). The [`WatchSink::as_any`] hook lets a caller recover
/// the typed sink after [`crate::Vm::take_watch`].
pub trait WatchSink: 'static {
    /// One local write, reported just after the slot is updated (so a
    /// write that faults — a bounds failure — is never reported).
    fn write(&mut self, event: &WatchEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_watch`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every local write in memory.
///
/// `WatchCollector::new()` is unbounded; `WatchCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit — an infinite loop writing a slot every dispatch
/// must not OOM the embedding host. The flag is a deliberate, *displayed*
/// fact, never a silent drop.
#[derive(Clone, Default)]
pub struct WatchCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured events, in write order.
    events: Vec<WatchEvent>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl WatchCollector {
    /// Create an unbounded collector (every local write of the run is
    /// captured).
    #[must_use]
    pub fn new() -> WatchCollector {
        WatchCollector::default()
    }

    /// Create a collector that captures at most `max_events` writes and
    /// marks the overflow instead of growing without limit.
    #[must_use]
    pub fn bounded(max_events: usize) -> WatchCollector {
        WatchCollector {
            cap: Some(max_events),
            events: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured events, in write order (a bounded collector holds at
    /// most its cap).
    #[must_use]
    pub fn events(&self) -> &[WatchEvent] {
        &self.events
    }

    /// Consume the collector into its events.
    #[must_use]
    pub fn into_events(self) -> Vec<WatchEvent> {
        self.events
    }

    /// How many events were captured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// The captured values of one watched slot, in write order — the
    /// slot's history. Events of other slots and other frames are skipped;
    /// pair it with [`WatchEvent::frame_depth`] at the caller when
    /// recursion makes the frame identity matter.
    #[must_use]
    pub fn slot_history(&self, func: usize, local: usize) -> Vec<&Value> {
        self.events
            .iter()
            .filter(|e| e.func == func && e.local == local)
            .map(|e| &e.value)
            .collect()
    }
}

impl WatchSink for WatchCollector {
    fn write(&mut self, event: &WatchEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.events.push(event.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A sink that emits one `tracing::debug!` event per local write — the
/// seam's counterpart for the workspace-standard observability layer (the
/// same `tracing` dependency every other seam logs through). Enable it per
/// subscriber, never by paying the cost unconditionally.
pub struct TracingWatchSink;

impl WatchSink for TracingWatchSink {
    fn write(&mut self, event: &WatchEvent) {
        debug!(
            func = event.func,
            local = event.local,
            pc = event.pc,
            dispatch = event.dispatch,
            frame_depth = event.frame_depth,
            via = event.via.name(),
            "vm local write"
        );
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a captured watch into a deterministic listing: a header line
/// (image format version, captured write count), then one line per event —
/// event index, writer function, producing pc, slot, rendered value, the
/// [`WatchVia`] kind, and the producing dispatch number.
///
/// Deterministic (the same event list renders byte-identical text — the
/// test suite pins a golden) and total (any event list renders). `module`
/// is accepted for signature symmetry with the other seams' renderers
/// (the trace's pool-resolving render); the watch listing's values are
/// self-describing and need no pool.
#[must_use]
pub fn render_watch(events: &[WatchEvent], module: &Module) -> String {
    let _ = module;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "watch glyim.module-image/{} writes={}",
        crate::IMAGE_VERSION,
        events.len()
    );
    for (i, event) in events.iter().enumerate() {
        let _ = writeln!(
            out,
            "{i}: fn {} pc {} local {} = {} {} dispatch {}",
            event.func,
            event.pc,
            event.local,
            render_value(&event.value),
            event.via.name(),
            event.dispatch,
        );
    }
    debug!(writes = events.len(), "vm watch rendered");
    out
}

/// Render one [`Value`] in the disassembler's diagnostic spellings:
/// integers plain, floats with their raw IEEE-754 bits (the disasm
/// convention — a corrupted bit pattern stays visible), strings quoted
/// (`Debug`), tuples as a parenthesized, comma-separated, recursively
/// rendered list (`()` for the empty aggregate).
fn render_value(value: &Value) -> String {
    match value {
        Value::Int(n) => format!("{n}"),
        Value::Float(f) => format!("{f} (bits 0x{:016x})", f.to_bits()),
        Value::Str(s) => format!("{s:?}"),
        Value::Tuple(elems) => {
            let mut out = String::from("(");
            for (i, e) in elems.iter().enumerate() {
                if i > 0 {
                    let _ = out.write_str(", ");
                }
                let _ = out.write_str(&render_value(e));
            }
            let _ = out.write_str(")");
            out
        }
    }
}

#[cfg(test)]
mod watch_tests {
    use crate::trace::TraceCollector;
    use crate::watch::{TracingWatchSink, WatchCollector, WatchEvent, WatchSink, render_watch};
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

    /// helper(n) = n * 2 + main returning helper(21): the disasm/trace
    /// golden module's exact shape. Its watch stream is the ABI pair: the
    /// call binds the argument into the callee's local 1, and the return
    /// lands the product in the caller's dest local 0.
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

    /// A countdown loop over local 0: the store-heavy shape — one argument
    /// bind, then `SEED + 1` program stores (the seed plus one per
    /// iteration).
    fn countdown_module(seed: i64) -> Module {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&seed.to_le_bytes());
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        let head = code.len();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Sub));
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Gt));
        code.push(op(Opcode::JumpIf));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return));
        Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0)
    }

    /// Run a module with a watch collector attached; the value must match
    /// the unwatched result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_watch`.
    fn watched_run(module: &Module) -> (WatchCollector, Value) {
        let collector = WatchCollector::new();
        let mut vm = Vm::new().with_watch(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_watch().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<WatchCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// The golden module's watch stream is the two ABI writes: the call
    /// binds 21 into the callee's local 1 (at the call site, frame depth
    /// 2), the return lands 42 in the caller's local 0 (at the callee's
    /// return site, frame depth 1). The render pins both spellings.
    #[test]
    fn golden_pinned_abi_writes() {
        let module = golden_module();
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(42));
        let events = collector.events();
        assert_eq!(events.len(), 2, "arg bind + callee return");
        assert_eq!(
            events[0],
            WatchEvent {
                func: 0,
                local: 1,
                value: Value::Int(21),
                pc: 18,
                dispatch: 3,
                frame_depth: 2,
                via: crate::watch::WatchVia::ArgBind,
            }
        );
        assert_eq!(
            events[1],
            WatchEvent {
                func: 1,
                local: 0,
                value: Value::Int(42),
                pc: 15,
                dispatch: 7,
                frame_depth: 1,
                via: crate::watch::WatchVia::CalleeReturn,
            }
        );
        let rendered = render_watch(events, &module);
        assert_eq!(
            rendered,
            "\
watch glyim.module-image/2 writes=2
0: fn 0 pc 18 local 1 = 21 arg-bind dispatch 3
1: fn 1 pc 15 local 0 = 42 callee-return dispatch 7
"
        );
    }

    /// A store-heavy loop records every program write in dispatch order:
    /// the seed, then one per iteration, descending — and `slot_history`
    /// reads the same values back for the watched slot only.
    #[test]
    fn store_local_writes_are_recorded_in_order() {
        let module = countdown_module(2);
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(0));
        let events = collector.events();
        assert_eq!(events.len(), 3, "seed + two iterations");
        let descs: Vec<(usize, usize, i64, u64)> = events
            .iter()
            .map(|e| {
                (
                    e.pc,
                    e.local,
                    match e.value {
                        Value::Int(n) => n,
                        _ => panic!("int writes only"),
                    },
                    e.dispatch,
                )
            })
            .collect();
        assert_eq!(
            descs,
            vec![(9, 0, 2, 2), (29, 0, 1, 6), (29, 0, 0, 14)],
            "seed at the first store, then the loop-head store per iteration"
        );
        assert!(
            events
                .iter()
                .all(|e| e.via == crate::watch::WatchVia::StoreLocal)
        );
        let history: Vec<i64> = collector
            .slot_history(0, 0)
            .into_iter()
            .map(|v| match v {
                Value::Int(n) => *n,
                _ => panic!("int writes only"),
            })
            .collect();
        assert_eq!(history, vec![2, 1, 0], "the slot's descending history");
    }

    /// The watch is read-only: the watched result equals the unwatched
    /// result, and the write count does not perturb execution.
    #[test]
    fn watched_result_matches_unwatched() {
        let module = golden_module();
        let unwatched = Vm::new().run_module(&module).expect("unwatched runs");
        let (collector, watched) = watched_run(&module);
        assert_eq!(watched, unwatched);
        assert_eq!(collector.len(), 2);
        assert!(!collector.overflowed());
    }

    /// A VM without a sink behaves exactly as before: no events, no panic.
    #[test]
    fn no_sink_by_default() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_watch().is_none());
    }

    /// A bounded collector marks overflow instead of growing without limit
    /// (an infinite loop writing a slot per dispatch must not OOM the
    /// embedding host).
    #[test]
    fn bounded_collector_marks_overflow() {
        let module = countdown_module(5);
        let collector = WatchCollector::bounded(2);
        let mut vm = Vm::new().with_watch(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_watch().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<WatchCollector>().expect("collector");
        assert_eq!(collector.len(), 2, "capture stops at the cap");
        assert!(collector.overflowed());
    }

    /// Every value kind renders: floats with their raw IEEE-754 bits, pool
    /// strings quoted, tuples parenthesized and recursive.
    #[test]
    fn value_spellings_render() {
        let mut code = Vec::new();
        // local 0 = 1.5 (float bits visible in the render)
        code.push(op(Opcode::LoadConstF64));
        code.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        // local 1 = "hello" (pool string)
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(1));
        // local 2 = (1, (2, 3)) (nested aggregate)
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(3));
        code.push(op(Opcode::Aggregate));
        code.extend_from_slice(&u32le(2));
        code.push(op(Opcode::Aggregate));
        code.extend_from_slice(&u32le(2));
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(2));
        code.push(op(Opcode::Return));
        let module = Module::with_strings(
            vec![Function::new(code, 3, 0)],
            vec!["hello".to_string()],
            0,
        );
        let (collector, _) = watched_run(&module);
        let rendered = render_watch(collector.events(), &module);
        assert!(
            rendered.contains("local 0 = 1.5 (bits 0x3ff8000000000000) store-local"),
            "float bits visible in {rendered}"
        );
        assert!(
            rendered.contains("local 1 = \"hello\" store-local"),
            "quoted string in {rendered}"
        );
        assert!(
            rendered.contains("local 2 = (1, (2, 3)) store-local"),
            "nested tuple in {rendered}"
        );
    }

    /// The trace seam and the watch seam compose on one run: separate
    /// hooks, both sinks come back typed, and the step count is unchanged
    /// by the watch.
    #[test]
    fn watch_composes_with_trace() {
        let module = golden_module();
        let collector = WatchCollector::new();
        let trace = TraceCollector::new();
        let mut vm = Vm::new()
            .with_trace(Box::new(trace))
            .with_watch(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut traced = vm.take_trace().expect("trace attached");
        let steps = traced
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("trace collector")
            .len();
        assert_eq!(steps, 9, "the golden module dispatches 9 steps");
        let mut watched = vm.take_watch().expect("watch attached");
        let writes = watched
            .as_any()
            .downcast_ref::<WatchCollector>()
            .expect("watch collector")
            .len();
        assert_eq!(writes, 2, "the ABI pair, as in the golden test");
    }

    /// A fuel-refused dispatch never executes and never writes: the watch
    /// ends exactly at the writes the executed dispatches produced.
    #[test]
    fn fuel_refused_dispatch_writes_nothing() {
        let module = countdown_module(10);
        let collector = WatchCollector::new();
        let mut vm = Vm::new().with_fuel(3).with_watch(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the loop burns the budget");
        assert_eq!(err, VmError::FuelExhausted { executed: 3 });
        let mut boxed = vm.take_watch().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<WatchCollector>().expect("collector");
        assert_eq!(
            collector.len(),
            1,
            "the seed store ran (dispatch 2); the refused dispatch wrote nothing"
        );
    }

    /// The image round trip composes with the seam: decode the shipped
    /// bytes, then watch the loaded module.
    #[test]
    fn watch_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let (collector, value) = watched_run(&loaded);
        assert_eq!(value, Value::Int(42));
        assert_eq!(collector.len(), 2, "the ABI pair over the decoded image");
    }

    /// A write that fails — a `LocalOutOfBounds` store — writes nothing
    /// and reports nothing: the seam is total over the *successful*
    /// writes.
    #[test]
    fn failed_store_reports_nothing() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(99));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = WatchCollector::new();
        let mut vm = Vm::new().with_watch(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("local 99 is out of bounds");
        assert_eq!(err, VmError::LocalOutOfBounds(99));
        let mut boxed = vm.take_watch().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<WatchCollector>().expect("collector");
        assert!(collector.is_empty(), "no event for the failed write");
    }

    /// The render is deterministic: the same event list renders
    /// byte-identical text.
    #[test]
    fn render_is_deterministic() {
        let module = golden_module();
        let (collector, _) = watched_run(&module);
        let events = collector.events();
        assert_eq!(render_watch(events, &module), render_watch(events, &module));
    }

    /// An empty capture renders the zero case (displayable, not an error).
    #[test]
    fn empty_capture_renders_zero_writes() {
        let module = golden_module();
        let rendered = render_watch(&[], &module);
        assert_eq!(rendered, "watch glyim.module-image/2 writes=0\n");
    }

    /// `TracingWatchSink` is total: attaching it changes nothing about the
    /// run and never panics (with or without a subscriber installed).
    #[test]
    fn tracing_watch_sink_is_total() {
        let module = golden_module();
        let mut vm = Vm::new().with_watch(Box::new(TracingWatchSink));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        // An event can also be delivered directly.
        TracingWatchSink.write(&WatchEvent {
            func: 0,
            local: 0,
            value: Value::Int(1),
            pc: 0,
            dispatch: 1,
            frame_depth: 1,
            via: crate::watch::WatchVia::StoreLocal,
        });
    }

    /// Same-function frames are disambiguated by frame depth: a recursive
    /// helper's arg binds land at depth 2 (and 3), the caller's dest write
    /// at depth 1, all keyed by the writer's function index.
    #[test]
    fn same_function_frames_carry_their_depth() {
        // helper(n): local 0 = n + 1; return local 0 — one store per frame.
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(1));
        helper.push(op(Opcode::Add));
        helper.push(op(Opcode::StoreLocal));
        helper.extend_from_slice(&u32le(0));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(0));
        helper.push(op(Opcode::Return));
        // main: helper(helper(9)) — the inner call's return feeds the outer
        // call, so both callee frames live at once (depths 2 and 3).
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(9));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(1)); // dest local 1 of main
        main.extend_from_slice(&u32le(31)); // resume (byte offset mode)
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(1)); // the inner result is the arg
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local 0 of main
        main.extend_from_slice(&u32le(58)); // resume (byte offset mode)
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        let module = Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 2, 0)],
            1,
        );
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(11), "helper(helper(9)) == 11");
        let events = collector.events();
        // The streams: inner arg bind 9 at depth 2, inner store 10 at
        // depth 2, inner return 10 into main's local 1 at depth 1, outer
        // arg bind 10 at depth 2, outer store 11 at depth 2, outer return
        // 11 into main's local 0 at depth 1.
        let descs: Vec<(usize, usize, i64, usize, &str)> = events
            .iter()
            .map(|e| {
                (
                    e.func,
                    e.local,
                    match &e.value {
                        Value::Int(n) => *n,
                        _ => panic!("int writes only"),
                    },
                    e.frame_depth,
                    e.via.name(),
                )
            })
            .collect();
        assert_eq!(
            descs,
            vec![
                (0, 1, 9, 2, "arg-bind"),
                (0, 0, 10, 2, "store-local"),
                (1, 1, 10, 1, "callee-return"),
                (0, 1, 10, 2, "arg-bind"),
                (0, 0, 11, 2, "store-local"),
                (1, 0, 11, 1, "callee-return"),
            ],
            "each write keyed by frame depth and writer function"
        );
    }
}
