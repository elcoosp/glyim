//! Fuel-observation seam: where did the execution budget actually go?
//!
//! The fuel gate ([`crate::Vm::with_fuel`]) is the machine's execution
//! budget: a budgeted run stops with the explicit
//! [`crate::VmError::FuelExhausted`] instead of hanging an embedding host
//! on a loaded artifact's infinite loop. But the budget itself was
//! observable only through its refusal: nothing answered *where* the
//! budget went — how much burned per function, at which site the last
//! allowed dispatch ran, what remained at each step. This module is that
//! budget face: the VM fires [`FuelSink::fuel_ticked`] once per executed
//! dispatch, just after the budget for that dispatch has been spent and
//! before the instruction executes — the same cadence as the trace hook
//! ([`crate::trace`]), with the remaining budget spelled per tick when
//! the run is budgeted (`None` = an unbounded run, still observable as
//! burn).

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use tracing::debug;

/// One executed dispatch's budget checkpoint, handed to the attached
/// [`FuelSink`] just after the budget for that dispatch has been spent and
/// just before the instruction executes — the same cadence as the trace
/// hook ([`crate::trace`]), with the remaining budget spelled per tick
/// when the run is budgeted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FuelTick {
    /// The function index containing the executing instruction.
    pub func: usize,
    /// The byte offset of the executing instruction (the dispatch site).
    pub pc: usize,
    /// The dispatch's 1-based lifetime number (the same count
    /// [`crate::Vm::executed`] reports after the run).
    pub dispatch: u64,
    /// The budget remaining after this dispatch's spend (`None` = the run
    /// is unbounded — there is no budget to descend, and the tick is pure
    /// burn observation).
    pub remaining: Option<u64>,
}

/// The observer the VM hands each budget checkpoint to.
///
/// Implementors must treat [`FuelSink::fuel_ticked`] as an observation
/// only: the VM's state and control flow are invisible to the sink and
/// unaffected by it. The method must not panic (the VM never defends
/// against a panicking sink — a sink that panics aborts the run, which is
/// the embedding host's explicit choice). The [`FuelSink::as_any`] hook
/// lets a caller recover the typed sink after
/// [`crate::Vm::take_fuel_sink`].
pub trait FuelSink: 'static {
    /// One budget checkpoint, reported once per executed dispatch (a
    /// fuel-refused dispatch never executes and never ticks — the fault
    /// stream owns the refusal).
    fn fuel_ticked(&mut self, event: &FuelTick);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_fuel_sink`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every budget checkpoint in memory.
///
/// `FuelCollector::new()` is unbounded; `FuelCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit — a loop ticking every dispatch must not OOM the
/// embedding host. The flag is a deliberate, *displayed* fact, never a
/// silent drop.
#[derive(Clone, Default)]
pub struct FuelCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured ticks, in dispatch order.
    ticks: Vec<FuelTick>,
    /// Whether any tick was dropped because the cap was reached.
    overflowed: bool,
}

impl FuelCollector {
    /// Create an unbounded collector (every checkpoint of the run is
    /// captured).
    #[must_use]
    pub fn new() -> FuelCollector {
        FuelCollector::default()
    }

    /// Create a collector that captures at most `max_ticks` checkpoints
    /// and marks the overflow instead of growing without limit.
    #[must_use]
    pub fn bounded(max_ticks: usize) -> FuelCollector {
        FuelCollector {
            cap: Some(max_ticks),
            ticks: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured ticks, in dispatch order (a bounded collector holds at
    /// most its cap).
    #[must_use]
    pub fn events(&self) -> &[FuelTick] {
        &self.ticks
    }

    /// Consume the collector into its ticks.
    #[must_use]
    pub fn into_events(self) -> Vec<FuelTick> {
        self.ticks
    }

    /// How many ticks were captured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ticks.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ticks.is_empty()
    }

    /// Whether any tick was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// The burn per function: dispatch counts keyed by function index (a
    /// `BTreeMap`, so the fold is deterministic — the same capture folds
    /// identically every time).
    #[must_use]
    pub fn burn_by_function(&self) -> BTreeMap<usize, u64> {
        let mut burn = BTreeMap::new();
        for tick in &self.ticks {
            *burn.entry(tick.func).or_insert(0) += 1;
        }
        burn
    }

    /// The run's last checkpoint — the last *allowed* dispatch: for a
    /// budgeted run that exhausted its budget, this names the site the
    /// refusal happened at (the next dispatch is the refused one), and
    /// its `remaining` spells what was left. `None` for an empty capture.
    #[must_use]
    pub fn last_tick(&self) -> Option<&FuelTick> {
        self.ticks.last()
    }
}

impl FuelSink for FuelCollector {
    fn fuel_ticked(&mut self, event: &FuelTick) {
        if let Some(cap) = self.cap {
            if self.ticks.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.ticks.push(*event);
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A sink that emits one `tracing::debug!` event per budget checkpoint —
/// the seam's counterpart for the workspace-standard observability layer
/// (the same `tracing` dependency every other seam logs through). Enable
/// it per subscriber, never by paying the cost unconditionally.
pub struct TracingFuelSink;

impl FuelSink for TracingFuelSink {
    fn fuel_ticked(&mut self, event: &FuelTick) {
        match event.remaining {
            Some(remaining) => debug!(
                func = event.func,
                pc = event.pc,
                dispatch = event.dispatch,
                remaining,
                "vm fuel tick"
            ),
            None => debug!(
                func = event.func,
                pc = event.pc,
                dispatch = event.dispatch,
                remaining = "-",
                "vm fuel tick"
            ),
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a captured budget diary into a deterministic report: a header
/// line (image format version, tick count, the remaining budget the last
/// checkpoint reported — `-` when the run was unbounded or the capture is
/// empty), the per-function burn lines (function order, from the
/// deterministic fold), then one line per tick — tick index, the
/// executing function, the dispatch site, the dispatch number, and the
/// remaining budget (`-` when unbounded). An empty capture renders the
/// explicit `no fuel ticks` line (honest about emptiness, like the float
/// stream's `no float events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_fuel(collector: &FuelCollector) -> String {
    let mut out = String::new();
    let remaining = match collector.last_tick().map(|t| t.remaining) {
        Some(Some(remaining)) => remaining.to_string(),
        _ => String::from("-"),
    };
    let _ = writeln!(
        out,
        "fuel glyim.module-image/{} ticks={} remaining={remaining}",
        crate::IMAGE_VERSION,
        collector.ticks.len(),
    );
    if collector.ticks.is_empty() {
        let _ = writeln!(out, "no fuel ticks");
        debug!(ticks = 0, "vm fuel rendered");
        return out;
    }
    for (func, burn) in collector.burn_by_function() {
        let _ = writeln!(out, "burn fn {func} = {burn}");
    }
    for (i, tick) in collector.ticks.iter().enumerate() {
        let remaining = match tick.remaining {
            Some(remaining) => remaining.to_string(),
            None => String::from("-"),
        };
        let _ = writeln!(
            out,
            "{i}: fn {} pc {} dispatch {} remaining {remaining}",
            tick.func, tick.pc, tick.dispatch,
        );
    }
    debug!(ticks = collector.ticks.len(), "vm fuel rendered");
    out
}

#[cfg(test)]
mod fuel_tests {
    use crate::fuel::{FuelCollector, FuelSink, FuelTick, TracingFuelSink, render_fuel};
    use crate::trace::TraceCollector;
    use crate::{Function, Module, Opcode, Value, Vm, VmError, decode_module, encode_module};
    use std::any::Any;
    use std::collections::BTreeMap;

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
    /// golden module's exact shape — 9 dispatches, 4 in the callee, 5 in
    /// the caller.
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

    /// A countdown loop over local 0 — the budget-burning shape: an
    /// infinite loop under a fixed budget stops with the explicit error,
    /// and the ticks name exactly how far it got.
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

    /// Run a module with a fuel collector attached; the value must match
    /// the unsinked result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_fuel_sink`.
    fn ticked_run(module: &Module) -> (FuelCollector, Value) {
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel_sink(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_fuel_sink().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<FuelCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// A budgeted run's tick stream is the budget's diary: one tick per
    /// dispatch with the remaining budget descending to exactly zero — an
    /// exact-fit budget runs to completion, and the burn folds name each
    /// function's share (the callee 4, the caller 5). The render pins the
    /// whole report byte-identically.
    #[test]
    fn golden_pinned_budget_diary() {
        let module = golden_module();
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel(9).with_fuel_sink(Box::new(collector));
        let value = vm.run_module(&module).expect("9 dispatches fit in 9 fuel");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_fuel_sink().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<FuelCollector>().expect("collector");
        let ticks = collector.events();
        assert_eq!(ticks.len(), 9, "one tick per executed dispatch");
        assert_eq!(
            ticks.iter().map(|t| t.remaining).collect::<Vec<_>>(),
            vec![
                Some(8),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
                Some(3),
                Some(2),
                Some(1),
                Some(0)
            ],
            "the budget descends to exactly zero"
        );
        assert_eq!(
            ticks
                .iter()
                .map(|t| (t.func, t.dispatch))
                .collect::<Vec<_>>(),
            vec![
                (1, 1),
                (1, 2),
                (1, 3),
                (0, 4),
                (0, 5),
                (0, 6),
                (0, 7),
                (1, 8),
                (1, 9),
            ],
            "the caller's three dispatches, the callee's four, the caller's tail"
        );
        let mut burn = BTreeMap::new();
        burn.insert(0usize, 4u64);
        burn.insert(1usize, 5u64);
        assert_eq!(collector.burn_by_function(), burn);
        let last = collector.last_tick().expect("ticks exist");
        assert_eq!(
            (last.func, last.pc, last.dispatch),
            (1, 36, 9),
            "the last allowed dispatch is the return"
        );
        let rendered = render_fuel(collector);
        assert_eq!(
            rendered,
            "\
fuel glyim.module-image/2 ticks=9 remaining=0
burn fn 0 = 4
burn fn 1 = 5
0: fn 1 pc 0 dispatch 1 remaining 8
1: fn 1 pc 9 dispatch 2 remaining 7
2: fn 1 pc 18 dispatch 3 remaining 6
3: fn 0 pc 0 dispatch 4 remaining 5
4: fn 0 pc 9 dispatch 5 remaining 4
5: fn 0 pc 14 dispatch 6 remaining 3
6: fn 0 pc 15 dispatch 7 remaining 2
7: fn 1 pc 31 dispatch 8 remaining 1
8: fn 1 pc 36 dispatch 9 remaining 0
"
        );
    }

    /// A budget that runs out mid-loop stops with the explicit error, and
    /// the stream names exactly how far the budget got: three ticks, the
    /// last at the third dispatch's site, the tick count equal to the
    /// error's `executed` count.
    #[test]
    fn exhausted_budget_reports_how_far_it_got() {
        let module = countdown_module(10);
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel(3).with_fuel_sink(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the loop burns the budget");
        assert_eq!(err, VmError::FuelExhausted { executed: 3 });
        let mut boxed = vm.take_fuel_sink().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<FuelCollector>().expect("collector");
        let ticks = collector.events();
        assert_eq!(ticks.len(), 3, "one tick per allowed dispatch");
        assert_eq!(
            ticks.iter().map(|t| t.remaining).collect::<Vec<_>>(),
            vec![Some(2), Some(1), Some(0)]
        );
        let last = collector.last_tick().expect("ticks exist");
        assert_eq!(
            (last.func, last.pc, last.dispatch),
            (0, 14, 3),
            "the third dispatch is the countdown's first loop load"
        );
    }

    /// An unbounded run still ticks: `remaining` is `None` per tick (there
    /// is no budget to descend), and the burn folds work unchanged — the
    /// seam is the budget face whether or not a budget is set.
    #[test]
    fn unbounded_run_ticks_without_a_budget() {
        let module = golden_module();
        let (collector, value) = ticked_run(&module);
        assert_eq!(value, Value::Int(42));
        let ticks = collector.events();
        assert_eq!(ticks.len(), 9);
        assert!(
            ticks.iter().all(|t| t.remaining.is_none()),
            "no budget, no remaining"
        );
        let mut burn = BTreeMap::new();
        burn.insert(0usize, 4u64);
        burn.insert(1usize, 5u64);
        assert_eq!(collector.burn_by_function(), burn);
    }

    /// The seam is read-only: a ticked budgeted run produces the same
    /// result and the same fuel refusal as an unticked one.
    #[test]
    fn ticked_result_matches_unticked() {
        let module = countdown_module(5);
        let unticked = Vm::new()
            .with_fuel(7)
            .run_module(&module)
            .expect_err("the budget stops the loop");
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel(7).with_fuel_sink(Box::new(collector));
        let ticked = vm
            .run_module(&module)
            .expect_err("the budget stops the loop, ticked");
        assert_eq!(ticked, unticked);
    }

    /// A VM without a sink behaves exactly as before: no ticks, no panic.
    #[test]
    fn no_sink_by_default() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_fuel_sink().is_none());
    }

    /// A bounded collector marks overflow instead of growing without limit
    /// (a loop ticking every dispatch must not OOM the embedding host).
    #[test]
    fn bounded_collector_marks_overflow() {
        let module = countdown_module(5);
        let collector = FuelCollector::bounded(2);
        let mut vm = Vm::new().with_fuel_sink(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_fuel_sink().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<FuelCollector>().expect("collector");
        assert_eq!(collector.len(), 2, "capture stops at the cap");
        assert!(collector.overflowed());
    }

    /// The fuel seam composes with the trace seam and the budget on one
    /// run: separate hooks, the tick count equals the step count (the same
    /// cadence), and both sinks come back typed.
    #[test]
    fn fuel_composes_with_trace() {
        let module = golden_module();
        let collector = FuelCollector::new();
        let trace = TraceCollector::new();
        let mut vm = Vm::new()
            .with_fuel(20)
            .with_trace(Box::new(trace))
            .with_fuel_sink(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut traced = vm.take_trace().expect("trace attached");
        let steps = traced
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("trace collector")
            .len();
        let mut fueled = vm.take_fuel_sink().expect("fuel attached");
        let ticks = fueled
            .as_any()
            .downcast_ref::<FuelCollector>()
            .expect("fuel collector")
            .len();
        assert_eq!(ticks, steps, "one tick per dispatch, one step per dispatch");
    }

    /// The image round trip composes with the seam: decode the shipped
    /// bytes, then tick the loaded module's budget.
    #[test]
    fn fuel_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel(9).with_fuel_sink(Box::new(collector));
        let value = vm.run_module(&loaded).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut boxed = vm.take_fuel_sink().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<FuelCollector>().expect("collector");
        assert_eq!(collector.len(), 9, "the golden ticks over the image");
    }

    /// The render is deterministic: the same collector renders
    /// byte-identical text.
    #[test]
    fn render_is_deterministic() {
        let module = golden_module();
        let collector = FuelCollector::new();
        let mut vm = Vm::new().with_fuel(9).with_fuel_sink(Box::new(collector));
        let _ = vm.run_module(&module).expect("runs");
        let mut boxed = vm.take_fuel_sink().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<FuelCollector>().expect("collector");
        let collector = collector.clone();
        assert_eq!(render_fuel(&collector), render_fuel(&collector));
    }

    /// An empty capture renders the zero case (displayable, not an error).
    #[test]
    fn empty_capture_renders_zero_ticks() {
        let module = golden_module();
        let rendered = render_fuel(&FuelCollector::new());
        assert_eq!(
            rendered,
            "fuel glyim.module-image/2 ticks=0 remaining=-\nno fuel ticks\n"
        );
        let _ = module;
    }

    /// `TracingFuelSink` is total: attaching it changes nothing about the
    /// run and never panics (with or without a subscriber installed).
    #[test]
    fn tracing_fuel_sink_is_total() {
        let module = golden_module();
        let mut vm = Vm::new().with_fuel_sink(Box::new(TracingFuelSink));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        // An event can also be delivered directly.
        TracingFuelSink.fuel_ticked(&FuelTick {
            func: 0,
            pc: 0,
            dispatch: 1,
            remaining: Some(8),
        });
        TracingFuelSink.fuel_ticked(&FuelTick {
            func: 0,
            pc: 1,
            dispatch: 2,
            remaining: None,
        });
    }
}
