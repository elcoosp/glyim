//! Tail-window seam: the last N dispatches before a run ended, in bounded
//! memory.
//!
//! The trace seam ([`crate::trace`]) records *every* dispatch — the
//! sequential answer to "what did this run do?" — but a full transcript
//! costs O(steps) memory, which is exactly the wrong shape for a
//! long-running host that only ever asks the *failure-shaped* question:
//! what happened immediately before the run ended? The backtrace seam
//! ([`crate::backtrace`]) reports the fault site and the suspended caller
//! chain, but not the dispatches that led up to it. This module is that
//! counterpart: a [`TraceSink`] that keeps a fixed-size ring buffer of
//! the most recent [`VmStep`]s — a flight recorder. Attach it before a
//! run you expect to fail (or budget), and after the run you hold the
//! last `n` dispatches with their absolute dispatch numbers, regardless
//! of how long the run was: the memory bound is the window, never the
//! transcript.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state and cannot change execution), **total** (every dispatch enters
//! the window, including the faulting dispatch of a run that ends in a
//! `VmError` and the last dispatch a fuel budget allows — the window's
//! newest entry is always the run's final dispatch), **bounded** (the
//! buffer holds at most `n` steps; the dispatches that scrolled out are
//! counted in [`WindowCollector::dropped`] — a *displayed* fact, never a
//! silent drop), and **deterministic** ([`render_window`] renders the
//! same window byte-identically — the test suite pins a golden, with
//! each line named by its absolute dispatch number so the tail slots
//! into the run's numbering).
//!
//! The collector composes with every other seam: attach it through
//! [`crate::Vm::with_trace`] exactly like any other sink (it impls
//! [`TraceSink`]), recover it after the run through
//! [`crate::Vm::take_trace`] + [`TraceSink::as_any`], re-fold a captured
//! [`crate::TraceCollector`] transcript post-hoc with
//! [`WindowCollector::from_tail`] (a pure function of the step stream —
//! the result is identical to the live window), and pair it with
//! [`crate::backtrace`]: the backtrace names the fault site, the window
//! shows the dispatches that led there.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt::Write as _;

use crate::Module;
use crate::trace::{TraceSink, VmStep};
use tracing::debug;

/// A bounded flight recorder over the per-dispatch stream: the most
/// recent `n` steps, oldest first, plus the count of dispatches that
/// scrolled out of the window. Attach through [`crate::Vm::with_trace`],
/// recover through [`crate::Vm::take_trace`] + [`TraceSink::as_any`].
#[derive(Clone)]
pub struct WindowCollector {
    /// The window capacity (0 records nothing — an honest empty window).
    cap: usize,
    /// The captured tail, oldest first (at most `cap` steps).
    steps: VecDeque<VmStep>,
    /// How many dispatches scrolled out of the window (displayed, never a
    /// silent drop).
    dropped: u64,
}

impl WindowCollector {
    /// Create a window that keeps the most recent `max` steps. The
    /// memory bound is the window: whatever the run's length, the
    /// collector holds at most `max` steps and counts the rest in
    /// [`WindowCollector::dropped`].
    #[must_use]
    pub fn tail(max: usize) -> WindowCollector {
        WindowCollector {
            cap: max,
            steps: VecDeque::with_capacity(max.min(64)),
            dropped: 0,
        }
    }

    /// Re-fold a captured step list post-hoc, keeping its last `max`
    /// steps. The result is identical to what a live window over the
    /// same run would have held — the tail is a pure function of the
    /// step stream.
    #[must_use]
    pub fn from_tail(max: usize, steps: &[VmStep]) -> WindowCollector {
        let mut window = WindowCollector::tail(max);
        for step in steps {
            window.step(step);
        }
        window
    }

    /// The captured tail, oldest first (at most the window capacity).
    /// The fold keeps the deque contiguous, so the single front slice is
    /// the whole window.
    #[must_use]
    pub fn steps(&self) -> &[VmStep] {
        let (front, back) = self.steps.as_slices();
        debug_assert!(back.is_empty(), "the fold keeps the window contiguous");
        front
    }

    /// The window capacity (the memory bound).
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.cap
    }

    /// How many dispatches scrolled out of the window — the *displayed*
    /// honesty counter: the newest entry is always the run's final
    /// dispatch, and this says how much of the run's history the bound
    /// traded away.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Whether no dispatch was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The absolute 1-based dispatch number of the window's oldest
    /// captured step (the run's step total minus what the window holds);
    /// 0 when the window is empty.
    #[must_use]
    pub fn first_dispatch(&self) -> u64 {
        self.dropped + 1
    }
}

impl TraceSink for WindowCollector {
    fn step(&mut self, step: &VmStep) {
        // The ring: push the newest dispatch, evict the oldest past the
        // cap (the eviction is *counted* in `dropped` — displayed, never
        // a silent drop), and keep the deque contiguous so the `&self`
        // reader can hand back one slice. With no sink attached this is
        // one `None` check — exactly the pre-seam behavior; with a
        // window attached the per-dispatch cost is one clone into the
        // ring (the memory bound is the window, never the run length).
        self.steps.push_back(step.clone());
        if self.steps.len() > self.cap {
            self.steps.pop_front();
            self.dropped += 1;
        }
        self.steps.make_contiguous();
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a captured tail into a deterministic listing: a header line
/// (image format version, window capacity, captured count, dropped
/// count), then one line per captured dispatch in run order — the
/// absolute 1-based dispatch number (so a tail slots into the run's
/// numbering), the executing function and pc, the frame/stack depths, and
/// the mnemonic with the trace render's payload spelling (unknown bytes
/// list as `unknown(0x??)`, exactly the trace's honesty rule). A window
/// with no captured dispatches renders the explicit
/// `no dispatches in the window` line (honest about emptiness, like the
/// call graph's `no calls`).
///
/// Deterministic (the same window renders byte-identical text — the test
/// suite pins a golden) and total (any window renders).
#[must_use]
pub fn render_window(window: &WindowCollector, module: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "window glyim.module-image/{} tail={} captured={} dropped={}",
        crate::IMAGE_VERSION,
        window.capacity(),
        window.steps().len(),
        window.dropped(),
    );
    if window.is_empty() {
        let _ = writeln!(out, "no dispatches in the window");
        debug!(
            captured = 0,
            dropped = window.dropped(),
            "vm tail window rendered"
        );
        return out;
    }
    for (i, step) in window.steps().iter().enumerate() {
        let mnemonic = step
            .opcode
            .map(crate::disasm::op_name)
            .unwrap_or("unknown(0x??)");
        let suffix = crate::trace::render_detail(&step.detail, module);
        let _ = writeln!(
            out,
            "dispatch {}: fn {} pc {} frame={} stack={} {mnemonic}{suffix}",
            window.first_dispatch() + i as u64,
            step.func,
            step.pc,
            step.frame_depth,
            step.stack_depth,
        );
    }
    debug!(
        captured = window.steps().len(),
        dropped = window.dropped(),
        "vm tail window rendered"
    );
    out
}

#[cfg(test)]
mod window_tests {
    use super::{WindowCollector, render_window};
    use crate::trace::TraceCollector;
    use crate::{Function, Module, Opcode, Value, Vm, VmError};
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

    /// main (fn 0) calls fn 5 — out of range: three dispatches, the run
    /// ends in `VmError::FunctionOutOfBounds`, and the faulting `Call`
    /// is the window's newest entry (the trace hook fires *before* the
    /// instruction executes).
    fn out_of_range_call_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(5)); // fn index 5: out of range
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// A countdown loop: seeds local 0 with `n` and counts down to 0 —
    /// the long-run shape the window's scrolling and dropping show.
    fn countdown_module(n: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(n));
        main.push(op(Opcode::StoreLocal));
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): decrement, then branch back while > 0.
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub));
        main.push(op(Opcode::StoreLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt));
        main.push(op(Opcode::JumpIf));
        main.extend_from_slice(&u32le(14)); // loop head
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Run a module with a window attached through the trace hook; the
    /// typed collector comes back through `take_trace`.
    fn windowed_run(module: &Module, max: usize) -> (WindowCollector, Result<Value, VmError>) {
        let mut vm = Vm::new().with_trace(Box::new(WindowCollector::tail(max)));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let window = sink
            .downcast_mut::<WindowCollector>()
            .expect("the collector came back")
            .clone();
        (window, outcome)
    }

    /// The faulting dispatch is the window's newest entry: the trace hook
    /// fires before the instruction executes, so a run that ends in a
    /// `VmError` carries the fault site in the tail — the dispatches that
    /// led up to it, with the run's honest outcome preserved.
    #[test]
    fn fault_run_captures_the_tail() {
        let module = out_of_range_call_module();
        let (window, outcome) = windowed_run(&module, 3);
        assert!(matches!(outcome, Err(VmError::FunctionOutOfBounds(5))));
        assert_eq!(window.steps().len(), 3, "the whole run fits the window");
        assert_eq!(window.dropped(), 0);
        assert_eq!(window.capacity(), 3);
        let last = &window.steps()[2];
        assert_eq!(last.pc, 18, "the faulting Call dispatches at pc 18");
        assert_eq!(last.opcode, Some(Opcode::Call));
        assert_eq!(window.first_dispatch(), 1);
    }

    /// A long run scrolls: the window holds exactly the newest `n`
    /// dispatches and counts every scrolled-out dispatch in `dropped` —
    /// displayed, never a silent drop. The newest entry is the run's
    /// final dispatch.
    #[test]
    fn window_scrolls_and_counts_drops() {
        let module = countdown_module(50);
        let (window, outcome) = windowed_run(&module, 5);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        let total = 2 + 50 * 8 + 2; // preamble + 8-dispatch loop body x 50 + tail
        assert_eq!(window.steps().len(), 5, "the bound holds");
        assert_eq!(
            window.dropped() as usize,
            total - 5,
            "the scroll is counted"
        );
        assert_eq!(window.first_dispatch() as usize, total - 4);
        assert_eq!(
            window.steps().last().expect("the newest entry").opcode,
            Some(Opcode::Return),
            "the newest entry is the run's final dispatch"
        );
    }

    /// The tail is a pure function of the step stream: re-folding a
    /// captured transcript produces the identical window (steps, dropped
    /// count, render).
    #[test]
    fn from_tail_matches_the_live_window() {
        let module = countdown_module(20);
        let transcript = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(transcript));
        let _ = vm.run_module(&module).expect("transcript run");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let steps = boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("the collector came back")
            .steps()
            .to_vec();
        let (live, _) = windowed_run(&module, 6);
        let refolded = WindowCollector::from_tail(6, &steps);
        assert_eq!(live.steps(), refolded.steps());
        assert_eq!(live.dropped(), refolded.dropped());
        assert_eq!(
            render_window(&live, &module),
            render_window(&refolded, &module)
        );
    }

    /// A fuel budget's last allowed dispatch is the window's newest
    /// entry, and the refused dispatch reaches no hook: the window ends
    /// exactly at the executed count.
    #[test]
    fn fuel_cut_window_ends_at_the_budget() {
        let module = countdown_module(50);
        let mut vm = Vm::new()
            .with_trace(Box::new(WindowCollector::tail(3)))
            .with_fuel(10);
        let outcome = vm.run_module(&module);
        assert!(outcome.is_err(), "the budget cuts the run");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let window = boxed
            .as_any()
            .downcast_mut::<WindowCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(window.steps().len(), 3);
        assert_eq!(window.dropped(), 7, "10 executed - 3 captured");
        assert_eq!(window.first_dispatch(), 8);
    }

    /// A zero-capacity window records nothing and says so honestly: the
    /// dropped count still carries the run's total.
    #[test]
    fn zero_window_records_nothing() {
        let module = out_of_range_call_module();
        let (window, _) = windowed_run(&module, 0);
        assert!(window.is_empty());
        assert_eq!(window.dropped(), 3, "every dispatch scrolled out");
        assert_eq!(window.first_dispatch(), 4);
    }

    /// The render is deterministic and pins the window line spellings
    /// (absolute dispatch numbers, the trace's field shape).
    #[test]
    fn render_is_deterministic_golden() {
        let module = out_of_range_call_module();
        let (window, _) = windowed_run(&module, 3);
        let first = render_window(&window, &module);
        let second = render_window(&window, &module);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
window glyim.module-image/2 tail=3 captured=3 dropped=0
dispatch 1: fn 0 pc 0 frame=1 stack=0 OP_LOAD_CONST 5
dispatch 2: fn 0 pc 9 frame=1 stack=1 OP_LOAD_CONST 1
dispatch 3: fn 0 pc 18 frame=1 stack=2 OP_CALL argc=1 dest=0 t31
"
        );
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_dispatches() {
        let module = out_of_range_call_module();
        let (window, _) = windowed_run(&module, 0);
        assert_eq!(
            render_window(&window, &module),
            "\
window glyim.module-image/2 tail=0 captured=0 dropped=3
no dispatches in the window
"
        );
    }

    /// The seam is read-only: the windowed run's result equals the
    /// untraced one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let module = countdown_module(10);
        let mut vm = Vm::new();
        let untraced = vm.run_module(&module).expect("untraced run");
        let (_, traced) = windowed_run(&module, 4);
        assert_eq!(traced.expect("traced run"), untraced);
    }
}
