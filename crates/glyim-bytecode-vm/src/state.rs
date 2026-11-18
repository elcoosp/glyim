//! Call-state seam: what did every frame's locals look like when it began
//! and when it ended?
//!
//! The watch seam ([`crate::watch`]) answers the *per-write* data-flow
//! question — one event per value a slot takes — but it cannot hand back
//! the *frame-shaped* answer: at the moment a call began, what did the
//! callee's locals hold (which arguments landed where, was the return
//! place still zero), and at the moment it returned, what did the frame
//! hold after everything the body did to it? This module is that
//! counterpart: a hook in the frame lifecycle — [`Vm::push_frame`] fires
//! [`StateSink::frame_enter`] the moment a frame's locals are bound, and
//! the `OP_RETURN` arm fires [`StateSink::frame_exit`] with the frame's
//! final local state before the frame is discarded — handing one
//! [`StateEvent`] per boundary to a sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change execution), **total** (every frame boundary reports,
//! including the entry frame's enter and exit — a straight-line run
//! reports exactly two events), **bounded** (the collector's memory is
//! capped and flags overflow instead of growing without limit over an
//! unbounded recursion), and **deterministic** ([`render_state`] renders
//! the same event list byte-identically — the test suite pins a golden).
//! Pairwise enter/exit events share the frame's depth, so a recursion's
//! nesting reads LIFO; an unbalanced tail (a fuel budget or a fault ends
//! the run between the boundaries) is *displayed*, never silently paired
//! away — the enter happened, the exit never did.
//!
//! The hook fires only when a sink is attached: one `Option` check per
//! frame boundary, exactly the pre-seam behavior when the sink is absent.
//! The locals snapshot is paid only when a sink is attached (the clones
//! are the event's, not the frame's).

use std::any::Any;
use std::fmt::Write as _;

use crate::Value;
use tracing::debug;

/// The boundary phase one [`StateEvent`] reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatePhase {
    /// The frame entered: its locals are freshly bound.
    Enter,
    /// The frame exited: its locals hold the final state.
    Exit,
}

impl StatePhase {
    /// The listing name (the deterministic render's spelling).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            StatePhase::Enter => "enter",
            StatePhase::Exit => "exit",
        }
    }
}

/// One frame boundary event, handed to the attached [`StateSink`] the
/// moment the boundary completes: the frame's phase, its locals snapshot,
/// its function index and 1-based depth (the enter event reports the
/// depth the frame now sits at; the exit event reports the depth the
/// frame sat at — the two pair up per frame).
#[derive(Clone, Debug, PartialEq)]
pub struct StateEvent {
    /// Whether this boundary is the frame's enter or its exit.
    pub phase: StatePhase,
    /// The frame's function index in the module.
    pub func: usize,
    /// The frame's 1-based depth (1 = the entry frame).
    pub depth: usize,
    /// The frame's local slots at the boundary (a full copy, paid only
    /// when a sink is attached): the enter event's slots are the freshly
    /// bound state (the MIR ABI's zero return place at local 0, the
    /// arguments at `1..=arg_count`); the exit event's slots are the
    /// frame's final state, mutations included.
    pub locals: Vec<Value>,
}

/// A sink that observes every frame boundary: a [`StateSink`] the VM hands
/// one [`StateEvent`] to at each frame's enter and exit. Attach through
/// [`crate::Vm::with_state`], recover through [`crate::Vm::take_state`] +
/// [`StateSink::as_any`].
pub trait StateSink: 'static {
    /// One frame entered: its locals are bound (the MIR ABI's zero return
    /// place at local 0, the arguments at `1..=arg_count`) and it is about
    /// to dispatch its first instruction.
    fn frame_enter(&mut self, event: &StateEvent);

    /// One frame exited: its locals hold the frame's final state (every
    /// mutation the body made) and it is about to be discarded. The entry
    /// frame's exit is the run's last event.
    fn frame_exit(&mut self, event: &StateEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_state`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every frame boundary in memory.
///
/// `StateCollector::new()` is unbounded; `StateCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit over an unbounded recursion — a frame boundary
/// every dispatch must not OOM the embedding host. The flag is a
/// deliberate, *displayed* fact, never a silent drop.
#[derive(Clone, Default)]
pub struct StateCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured events, in boundary order.
    events: Vec<StateEvent>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl StateCollector {
    /// Create an unbounded collector (every frame boundary is captured).
    #[must_use]
    pub fn new() -> StateCollector {
        StateCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> StateCollector {
        StateCollector {
            cap: Some(max),
            events: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured events, in boundary order.
    #[must_use]
    pub fn events(&self) -> &[StateEvent] {
        &self.events
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no boundary was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The captured events for one function's frames (both phases), in
    /// boundary order.
    #[must_use]
    pub fn events_of(&self, func: usize) -> Vec<&StateEvent> {
        self.events.iter().filter(|e| e.func == func).collect()
    }
}

impl StateSink for StateCollector {
    fn frame_enter(&mut self, event: &StateEvent) {
        self.push(event.clone());
    }

    fn frame_exit(&mut self, event: &StateEvent) {
        self.push(event.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl StateCollector {
    /// Append one event under the capture cap (an overflow drops the event
    /// and sets the flag — displayed, never a silent drop).
    fn push(&mut self, event: StateEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.events.push(event);
    }
}

/// Render a call-state run into a deterministic listing: a header line
/// (image format version, boundary count), then one line per boundary in
/// event order — the phase (`enter`/`exit`), the frame's function index
/// and depth, and the frame's local slots in the diagnostic value
/// spellings (`render_watch`'s — integers plain, floats with their raw
/// bits, strings quoted, tuples parenthesized). A run with no frame
/// boundaries renders the explicit `no frame boundaries` line (honest
/// about emptiness, like the call graph's `no calls`).
///
/// Deterministic and total (any event list renders).
#[must_use]
pub fn render_state(events: &[StateEvent]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "call-state glyim.module-image/{} boundaries={}",
        crate::IMAGE_VERSION,
        events.len()
    );
    if events.is_empty() {
        let _ = writeln!(out, "no frame boundaries");
        debug!(boundaries = 0, "vm call state rendered");
        return out;
    }
    for (i, event) in events.iter().enumerate() {
        let mut locals = String::from("[");
        for (j, value) in event.locals.iter().enumerate() {
            if j > 0 {
                let _ = locals.write_str(", ");
            }
            let _ = write!(locals, "{}", crate::watch::render_value(value));
        }
        let _ = locals.write_str("]");
        let _ = writeln!(
            out,
            "{i}: {} fn {} depth {} locals={locals}",
            event.phase.name(),
            event.func,
            event.depth,
        );
    }
    debug!(boundaries = events.len(), "vm call state rendered");
    out
}

#[cfg(test)]
mod state_tests {
    use super::{StateCollector, StateEvent, StatePhase, render_state};
    use crate::{Function, Module, Opcode, Value, Vm};
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

    /// main (fn 1) calls helper (fn 0) once with argument 21; helper
    /// doubles its argument into local 1 and returns it. The canonical
    /// two-frame module: four boundaries, all events visible.
    fn call_double_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadLocal)); // local 1 (arg)
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::StoreLocal)); // local 1 = arg * 2
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1));
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
        Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            1,
        )
    }

    /// Run a module with a state collector attached; the value must match
    /// the untraced result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_state`.
    fn traced_run(module: &Module) -> (StateCollector, Value) {
        let collector = StateCollector::new();
        let mut vm = Vm::new().with_state(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_state().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<StateCollector>()
            .expect("the collector came back")
            .clone();
        (collector, value)
    }

    /// A two-frame run reports four boundaries: the entry frame's enter,
    /// the callee's enter (depth 2), the callee's exit (its final state),
    /// and the entry frame's exit — with matching pairwise depths.
    #[test]
    fn call_and_return_report_boundaries() {
        let module = call_double_module();
        let (collector, value) = traced_run(&module);
        assert_eq!(value, Value::Int(42));
        assert_eq!(collector.events().len(), 4);
        let events = collector.events();
        assert_eq!(events[0].func, 1);
        assert_eq!(events[0].depth, 1, "the entry frame enters at depth 1");
        assert_eq!(events[1].func, 0);
        assert_eq!(events[1].depth, 2, "the callee enters one frame deeper");
        assert_eq!(events[2].func, 0);
        assert_eq!(events[2].depth, 2, "the callee exits at its own depth");
        assert_eq!(events[3].func, 1);
        assert_eq!(events[3].depth, 1, "the entry frame exits last");
        assert_eq!(
            collector.events_of(0).len(),
            2,
            "the callee's enter and exit"
        );
    }

    /// The enter event's snapshot is the freshly bound state: the MIR
    /// ABI's zero return place at local 0 and the argument at local 1.
    #[test]
    fn args_are_visible_at_entry() {
        let module = call_double_module();
        let (collector, _) = traced_run(&module);
        let enter = &collector.events()[1];
        assert_eq!(enter.locals[0], Value::Int(0), "zero return place");
        assert_eq!(enter.locals[1], Value::Int(21), "the bound argument");
    }

    /// The exit event's snapshot is the frame's final state: the body's
    /// mutation of local 1 is visible.
    #[test]
    fn exit_snapshot_reflects_mutations() {
        let module = call_double_module();
        let (collector, _) = traced_run(&module);
        let exit = &collector.events()[2];
        assert_eq!(exit.locals[1], Value::Int(42), "arg * 2 after the body");
    }

    /// A straight-line run (no calls) reports exactly two boundaries: the
    /// entry frame's enter and exit.
    #[test]
    fn straight_line_run_reports_two_boundaries() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let (collector, _) = traced_run(&module);
        assert_eq!(collector.events().len(), 2);
        assert_eq!(collector.events()[0].func, 0);
        assert_eq!(collector.events()[0].depth, 1);
        assert_eq!(collector.events()[1].func, 0);
    }

    /// Recursion nests: a 3-deep recursion enters at depths 1, 2, 3, 4
    /// and exits LIFO at depths 4, 3, 2, 1 — the pair-matched nesting.
    /// Table-less functions: branch and call-resume targets are raw byte
    /// offsets.
    #[test]
    fn recursion_nests_depths() {
        // rec(n) (fn 0) returns 0 at n <= 0 (base case at pc 67), else
        // recurses with n - 1 and returns the recursive result. main
        // (fn 1) seeds it with 2.
        let mut rec = Vec::new();
        rec.push(op(Opcode::LoadLocal)); // pc 0 — arg n
        rec.extend_from_slice(&u32le(1));
        rec.push(op(Opcode::LoadConst)); // pc 5
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::Le)); // pc 14 — n <= 0
        rec.push(op(Opcode::JumpIf)); // pc 15 — taken -> base case (63)
        rec.extend_from_slice(&u32le(63));
        rec.push(op(Opcode::LoadConst)); // pc 20 — fn index 0: self
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::LoadLocal)); // pc 29 — arg n
        rec.extend_from_slice(&u32le(1));
        rec.push(op(Opcode::LoadConst)); // pc 34
        rec.extend_from_slice(&i64le(1));
        rec.push(op(Opcode::Sub)); // pc 43 — n - 1 (the call's argument)
        rec.push(op(Opcode::Call)); // pc 44 — argc=1 dest=2 resume=57
        rec.extend_from_slice(&u32le(1));
        rec.extend_from_slice(&u32le(2));
        rec.extend_from_slice(&u32le(57));
        rec.push(op(Opcode::LoadLocal)); // pc 57 — the recursive result
        rec.extend_from_slice(&u32le(2));
        rec.push(op(Opcode::Return)); // pc 62
        rec.push(op(Opcode::LoadConst)); // pc 63 — base case: return 0
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::Return)); // pc 72
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — fn index 0: rec
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst)); // pc 9 — the seed argument
        main.extend_from_slice(&i64le(2));
        main.push(op(Opcode::Call)); // pc 18 — argc=1 dest=0 resume=31
        main.extend_from_slice(&u32le(1));
        main.extend_from_slice(&u32le(0));
        main.extend_from_slice(&u32le(31));
        main.push(op(Opcode::LoadLocal)); // pc 31 — the result
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 36
        let module = Module::new(vec![Function::new(rec, 3, 1), Function::new(main, 1, 0)], 1);
        let (collector, value) = traced_run(&module);
        assert_eq!(value, Value::Int(0), "rec(2) bottoms out at 0");
        let events = collector.events();
        let enters: Vec<(usize, usize)> = events
            .iter()
            .filter(|e| e.phase == StatePhase::Enter)
            .map(|e| (e.func, e.depth))
            .collect();
        let exits: Vec<(usize, usize)> = events
            .iter()
            .filter(|e| e.phase == StatePhase::Exit)
            .map(|e| (e.func, e.depth))
            .collect();
        assert_eq!(enters, vec![(1, 1), (0, 2), (0, 3), (0, 4)]);
        assert_eq!(
            exits,
            vec![(0, 4), (0, 3), (0, 2), (1, 1)],
            "the exits unwind LIFO"
        );
        // The deepest enter saw n = 0 — the base-case argument.
        let deepest = events
            .iter()
            .filter(|e| e.phase == StatePhase::Enter)
            .max_by_key(|e| e.depth)
            .expect("at least one enter");
        assert_eq!(deepest.locals[1], Value::Int(0));
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = call_double_module();
        let collector = StateCollector::bounded(2);
        let mut vm = Vm::new().with_state(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_state().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<StateCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 2, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The render is deterministic and pins the boundary line spellings.
    #[test]
    fn render_is_deterministic_golden() {
        let module = call_double_module();
        let (collector, _) = traced_run(&module);
        let first = render_state(collector.events());
        let second = render_state(collector.events());
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
call-state glyim.module-image/2 boundaries=4
0: enter fn 1 depth 1 locals=[0]
1: enter fn 0 depth 2 locals=[0, 21]
2: exit fn 0 depth 2 locals=[0, 42]
3: exit fn 1 depth 1 locals=[42]
"
        );
    }

    /// The seam is read-only: the traced run's result equals the untraced
    /// one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let module = call_double_module();
        let mut vm = Vm::new();
        let untraced = vm.run_module(&module).expect("untraced run");
        let (_, traced) = traced_run(&module);
        assert_eq!(traced, untraced);
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_boundaries() {
        assert_eq!(
            render_state(&[]),
            "\
call-state glyim.module-image/2 boundaries=0
no frame boundaries
"
        );
    }

    /// The event type is comparable (the golden test's `PartialEq`
    /// requirement) and its locals list is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        use super::StatePhase;
        let a = StateEvent {
            phase: StatePhase::Enter,
            func: 1,
            depth: 2,
            locals: vec![Value::Int(0), Value::Int(21)],
        };
        let mut b = a.clone();
        assert_eq!(a, b);
        b.locals.push(Value::Int(42));
        assert_ne!(a, b);
    }
}
