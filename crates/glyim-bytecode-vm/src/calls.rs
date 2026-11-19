//! Call-stream observation seam: what did this run call, with what
//! arguments, and what came back?
//!
//! The call-graph seam ([`crate::callgraph`]) folds the execution trace
//! into per-`(caller, callee)` edges, but the trace hook fires *before* a
//! call executes and its one-line step entries fold the call's operands
//! away — the argument values, the destination local, and the returned
//! value are unreadable after the fact. The call-state seam
//! ([`crate::state`]) snapshots a frame's locals at its boundaries — the
//! *frame* face — but its event carries the whole local array, not the
//! value the callee produced, and nothing in it names the call site or
//! the resume block. Nothing answered the *call-ABI* question: which
//! call dispatched at this site, with which argument values, into which
//! destination local, resuming where, and what did the callee hand back?
//! This module is that counterpart: a hook in the `OP_CALL` /
//! `OP_CALL_INDIRECT` arm — the VM fires [`CallSink::call_started`] just
//! after the callee's frame is pushed — and a hook in the `OP_RETURN`
//! arm — [`CallSink::call_returned`] fires just after the returning
//! frame is popped, with the value it returned — handing one event per
//! executed call/return to a sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the arguments land in the callee's locals and the return
//! value reaches the caller's destination exactly as the pre-seam
//! machine produced them), **total** (every executed call and return
//! reports, including the entry frame's return — its value is the module
//! result — while a call the machine *refused* (a bad function index, a
//! depth overflow) never reports: the fault stream owns the refusal),
//! **bounded** (the collector's memory is capped and flags overflow
//! instead of growing without limit over a call-heavy loop), and
//! **deterministic** ([`render_calls`] renders the same collector
//! byte-identically — the test suite pins a golden).
//!
//! The hooks fire only when a sink is attached: one `Option` check per
//! executed call and one per executed return, exactly the pre-seam
//! behavior when the sink is absent. The event's arguments and return
//! value are sink-side copies (the copies-into-the-event shape every
//! value-carrying seam follows) — the machine's own values stay exactly
//! where they were.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// One executed call, handed to the attached [`CallSink`] just after the
/// callee's frame has been pushed (a refused call — a bad function
/// index, a depth overflow — never reaches the hook: the fault stream
/// owns it): the caller's function index, the call site, the callee, the
/// argument values in full, the destination local the return value will
/// land in, the resume target, and whether the dispatch was indirect.
#[derive(Clone, Debug, PartialEq)]
pub struct CallStarted {
    /// The function index containing the call site (the caller).
    pub caller_func: usize,
    /// The byte offset of the `OP_CALL` / `OP_CALL_INDIRECT` opcode (the
    /// call site).
    pub site_pc: usize,
    /// The function index of the callee (the freshly pushed frame).
    pub callee_func: usize,
    /// The call's argument values in callee order (a sink-side copy —
    /// the machine's own values moved into the callee's locals).
    pub args: Vec<crate::Value>,
    /// The caller's local the callee's return value will be written into
    /// (the MIR ABI's destination).
    pub dest_local: usize,
    /// The caller's resume target (where execution continues when the
    /// callee returns — the raw wire target, resolved by the caller's
    /// own block table).
    pub resume_target: u32,
    /// Whether the call dispatched through `OP_CALL_INDIRECT` (the target
    /// came off the operand stack) rather than `OP_CALL`.
    pub indirect: bool,
}

/// One executed return, handed to the attached [`CallSink`] just after
/// the returning frame has been popped: the returning function's index,
/// the frame depth it sat at (the call-state seam's depth spelling: the
/// entry frame is depth 1), and the value it returned in full. The entry
/// frame's return is the run's last event — its value is the module
/// result.
#[derive(Clone, Debug, PartialEq)]
pub struct CallReturned {
    /// The function index of the returning frame.
    pub func: usize,
    /// The call depth the returning frame sat at (1-based; the entry
    /// frame is depth 1, a direct callee depth 2, and so on).
    pub depth: usize,
    /// The value the frame returned (a sink-side copy — the machine's
    /// own value is written into the caller's destination local, or
    /// handed to the caller as the module result).
    pub value: crate::Value,
}

/// The interleaved call transcript: one entry per executed call or
/// return, in execution order — the shape [`CallCollector::events`]
/// returns and [`render_calls`] lists.
#[derive(Clone, Debug, PartialEq)]
pub enum CallEvent {
    /// A call ([`CallStarted`]).
    Started(CallStarted),
    /// A return ([`CallReturned`]).
    Returned(CallReturned),
}

/// A sink that observes every executed call and return: a [`CallSink`]
/// the VM hands one [`CallStarted`] per executed `OP_CALL` /
/// `OP_CALL_INDIRECT` and one [`CallReturned`] per executed `OP_RETURN`.
/// Attach through [`crate::Vm::with_calls`], recover through
/// [`crate::Vm::take_calls`] + [`CallSink::as_any`].
pub trait CallSink: 'static {
    /// One call dispatched: the callee's frame has been pushed (a
    /// refused call never reaches the hook — the fault stream owns it).
    fn call_started(&mut self, event: &CallStarted);

    /// One frame returned: the frame has been popped and the value is
    /// about to reach the caller's destination (or end the run).
    fn call_returned(&mut self, event: &CallReturned);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_calls`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed call and return in memory.
///
/// `CallCollector::new()` is unbounded; `CallCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit over a call-heavy loop — one event per call and
/// return must not OOM the embedding host. The flag is a deliberate,
/// *displayed* fact, never a silent drop.
///
/// The collector also folds as it captures: the open-call count
/// ([`CallCollector::open_calls`], the calls started but not yet
/// returned — a fault or fuel budget ending the run mid-frame leaves
/// them open, and the count is *displayed*, never silently paired away),
/// the call-depth high-water mark ([`CallCollector::max_depth`], the
/// deepest nesting the *calls* reached — the entry frame, which no call
/// started, sits outside the fold), and the indirect-call tally
/// ([`CallCollector::indirect_count`]).
#[derive(Clone, Default)]
pub struct CallCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<CallEvent>,
    /// The fold's current depth (calls started minus calls returned).
    depth: usize,
    /// The fold's maximum depth (the calls' high-water mark).
    max_depth: usize,
    /// The indirect-call tally (the `OP_CALL_INDIRECT` count).
    indirect: usize,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl CallCollector {
    /// Create an unbounded collector (every executed call and return is
    /// captured).
    #[must_use]
    pub fn new() -> CallCollector {
        CallCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> CallCollector {
        CallCollector {
            cap: Some(max),
            events: Vec::new(),
            depth: 0,
            max_depth: 0,
            indirect: 0,
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order (calls and returns
    /// interleaved exactly as the run performed them).
    #[must_use]
    pub fn events(&self) -> &[CallEvent] {
        &self.events
    }

    /// The captured calls, in execution order.
    #[must_use]
    pub fn starts(&self) -> Vec<&CallStarted> {
        self.events
            .iter()
            .filter_map(|e| match e {
                CallEvent::Started(s) => Some(s),
                CallEvent::Returned(_) => None,
            })
            .collect()
    }

    /// The captured returns, in execution order.
    #[must_use]
    pub fn returns(&self) -> Vec<&CallReturned> {
        self.events
            .iter()
            .filter_map(|e| match e {
                CallEvent::Started(_) => None,
                CallEvent::Returned(r) => Some(r),
            })
            .collect()
    }

    /// How many captured calls have not been returned yet — the fold's
    /// current depth. A run ended by a fault or a fuel budget mid-frame
    /// leaves that frame's call open; the count is displayed by
    /// [`render_calls`], never silently paired away.
    #[must_use]
    pub fn open_calls(&self) -> usize {
        self.depth
    }

    /// The maximum call depth the captured run reached (the calls'
    /// high-water mark; the entry frame — which no call started — sits
    /// outside the fold, so a single direct call reports 1).
    #[must_use]
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// How many captured calls dispatched through `OP_CALL_INDIRECT`.
    #[must_use]
    pub fn indirect_count(&self) -> usize {
        self.indirect
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no call or return was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl CallSink for CallCollector {
    fn call_started(&mut self, event: &CallStarted) {
        self.push(CallEvent::Started(event.clone()));
    }

    fn call_returned(&mut self, event: &CallReturned) {
        self.push(CallEvent::Returned(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl CallCollector {
    /// Append one event under the capture cap (an overflow drops the
    /// event and sets the flag — displayed, never a silent drop),
    /// updating the depth/max-depth/indirect folds as it goes.
    fn push(&mut self, event: CallEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        match &event {
            CallEvent::Started(s) => {
                self.depth += 1;
                if self.depth > self.max_depth {
                    self.max_depth = self.depth;
                }
                if s.indirect {
                    self.indirect += 1;
                }
            }
            CallEvent::Returned(_) => {
                self.depth = self.depth.saturating_sub(1);
            }
        }
        self.events.push(event);
    }
}

/// Render a call stream into a deterministic listing: a header line
/// (image format version, call/return/indirect counts, the call-depth
/// high-water mark, and the open-call total), then one line per event in
/// execution order — the event's kind (`call`/`return`), the caller (or
/// returning function) index, the call site with the callee's argument
/// spellings, destination, resume target, and indirect flag (for calls),
/// and the returned value in the machine's value spellings (for returns)
/// — then the open calls as their own displayed total when the capture
/// stopped mid-frame. A run with no calls or returns renders the
/// explicit `no call events` line (honest about emptiness, like the
/// memory stream's `no memory events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_calls(collector: &CallCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "calls glyim.module-image/{} calls={} returns={} indirect={} max-depth={} open={}",
        crate::IMAGE_VERSION,
        collector.starts().len(),
        collector.returns().len(),
        collector.indirect,
        collector.max_depth,
        collector.depth,
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no call events");
        debug!(events = 0, "vm call stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        match event {
            CallEvent::Started(s) => {
                let mut args = String::from("[");
                for (j, a) in s.args.iter().enumerate() {
                    if j > 0 {
                        let _ = args.write_str(", ");
                    }
                    let _ = write!(args, "{}", crate::watch::render_value(a));
                }
                let _ = args.write_str("]");
                let _ = writeln!(
                    out,
                    "{i}: call fn {} pc {} -> fn {} argc {} args={args} dest {} resume {} indirect={}",
                    s.caller_func,
                    s.site_pc,
                    s.callee_func,
                    s.args.len(),
                    s.dest_local,
                    s.resume_target,
                    s.indirect,
                );
            }
            CallEvent::Returned(r) => {
                let _ = writeln!(
                    out,
                    "{i}: return fn {} depth {} value={}",
                    r.func,
                    r.depth,
                    crate::watch::render_value(&r.value),
                );
            }
        }
    }
    if collector.depth > 0 {
        let _ = writeln!(out, "unreturned calls: {}", collector.depth);
    }
    debug!(events = collector.events.len(), "vm call stream rendered");
    out
}

#[cfg(test)]
mod calls_tests {
    use super::{CallCollector, CallEvent, render_calls};
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
    /// doubles its argument into local 0 and pushes it for `OP_RETURN`.
    /// The canonical two-frame module: one call, one return, plus the
    /// entry frame's return with the module result. Emitter order: the
    /// callee index goes on the stack first (`LoadConst 0`), then the
    /// arguments, then `OP_CALL argc dest resume` — the arm pops the
    /// arguments and finally the callee index off the stack.
    fn call_double_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadLocal)); // pc 0 — the arg
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadLocal)); // pc 5 — the arg again
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::Add)); // pc 10 — arg + arg
        helper.push(op(Opcode::StoreLocal)); // pc 11 — return place
        helper.extend_from_slice(&u32le(0));
        helper.push(op(Opcode::LoadLocal)); // pc 16 — push the result
        helper.extend_from_slice(&u32le(0));
        helper.push(op(Opcode::Return)); // pc 21
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the callee (fn 0)
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst)); // pc 9 — the argument
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Call)); // pc 18 — the call
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(2)); // dest_local
        main.extend_from_slice(&u32le(31)); // resume (raw byte offset)
        main.push(op(Opcode::LoadLocal)); // pc 31 — the result
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::Return)); // pc 36
        Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 3, 0)],
            1,
        )
    }

    /// main (fn 1) calls `count_down` (fn 0) with `3`; `count_down` recurses
    /// on `n - 1` until it reaches zero and returns the countdown depth.
    /// A bounded recursion whose nesting is readable in the depth folds.
    fn recursion_module() -> Module {
        // fn 0 count_down(n): n == 0 ? return 0 : return count_down(n-1)+1
        let mut count_down = Vec::new();
        count_down.push(op(Opcode::LoadLocal)); // pc 0 — n
        count_down.extend_from_slice(&u32le(1));
        count_down.push(op(Opcode::LoadConst)); // pc 5 — zero
        count_down.extend_from_slice(&i64le(0));
        count_down.push(op(Opcode::Eq)); // pc 14 — n == 0
        count_down.push(op(Opcode::JumpIf)); // pc 15 — base case (pc 73)
        count_down.extend_from_slice(&u32le(73));
        count_down.push(op(Opcode::LoadConst)); // pc 20 — the callee (self)
        count_down.extend_from_slice(&i64le(0));
        count_down.push(op(Opcode::LoadLocal)); // pc 29 — n
        count_down.extend_from_slice(&u32le(1));
        count_down.push(op(Opcode::LoadConst)); // pc 34 — one
        count_down.extend_from_slice(&i64le(1));
        count_down.push(op(Opcode::Sub)); // pc 43 — n - 1
        count_down.push(op(Opcode::Call)); // pc 44 — count_down(n-1)
        count_down.extend_from_slice(&u32le(1)); // argc
        count_down.extend_from_slice(&u32le(2)); // dest_local
        count_down.extend_from_slice(&u32le(57)); // resume
        count_down.push(op(Opcode::LoadLocal)); // pc 57 — recursion result
        count_down.extend_from_slice(&u32le(2));
        count_down.push(op(Opcode::LoadConst)); // pc 62 — one
        count_down.extend_from_slice(&i64le(1));
        count_down.push(op(Opcode::Add)); // pc 71 — result + 1
        count_down.push(op(Opcode::Return)); // pc 72
        count_down.push(op(Opcode::LoadConst)); // pc 73 — zero (base case)
        count_down.extend_from_slice(&i64le(0));
        count_down.push(op(Opcode::Return)); // pc 82
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the callee (fn 0)
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst)); // pc 9 — the seed (3)
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Call)); // pc 18 — count_down(3)
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(2)); // dest_local
        main.extend_from_slice(&u32le(31)); // resume
        main.push(op(Opcode::LoadLocal)); // pc 31 — the result
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::Return)); // pc 36
        Module::new(
            vec![Function::new(count_down, 3, 1), Function::new(main, 3, 0)],
            1,
        )
    }

    /// main (fn 1) calls helper (fn 0), which traps: the run faults
    /// after the call started, before any return — the open call the
    /// render must display.
    fn faulting_callee_module() -> Module {
        let helper = vec![op(Opcode::Trap)]; // pc 0 — fault immediately
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the callee (fn 0)
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst)); // pc 9 — the argument
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::Call)); // pc 18 — the call
        main.extend_from_slice(&u32le(1));
        main.extend_from_slice(&u32le(2));
        main.extend_from_slice(&u32le(31));
        main.push(op(Opcode::Return)); // pc 31 — never reached
        Module::new(
            vec![Function::new(helper, 1, 1), Function::new(main, 3, 0)],
            1,
        )
    }

    /// main (fn 1) calls function 999: a bad function index — the
    /// machine refuses the call, the run faults, and the seam must
    /// report nothing (the fault stream owns the refusal).
    fn bad_callee_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the callee (999)
        main.extend_from_slice(&i64le(999));
        main.push(op(Opcode::LoadConst)); // pc 9 — an argument
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Call)); // pc 18 — refused
        main.extend_from_slice(&u32le(1));
        main.extend_from_slice(&u32le(2));
        main.extend_from_slice(&u32le(31));
        Module::new(
            vec![Function::new(Vec::new(), 0, 0), Function::new(main, 3, 0)],
            1,
        )
    }

    /// A call-heavy countdown: main calls helper once per loop
    /// iteration, seeding local 0 with `count` and looping while it is
    /// positive. One call and one return per iteration.
    fn call_loop_module(count: i64) -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadLocal)); // pc 0 — the arg
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::Return)); // pc 5 — pops the pushed arg
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): call helper(local 0), discard the result,
        // decrement, loop while still positive.
        main.push(op(Opcode::LoadConst)); // pc 14 — the callee (fn 0)
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadLocal)); // pc 23 — the argument
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Call)); // pc 28 — helper(local 0)
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(2)); // dest_local
        main.extend_from_slice(&u32le(41)); // resume
        main.push(op(Opcode::LoadLocal)); // pc 41 — the result
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::Drop)); // pc 46 — discard it
        main.push(op(Opcode::LoadLocal)); // pc 47 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 52 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 61 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 62
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 67 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 72 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 81 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 82 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 87 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 92
        Module::new(
            vec![Function::new(helper, 1, 1), Function::new(main, 3, 0)],
            1,
        )
    }

    /// Run a module with a call collector attached; the run's outcome
    /// and the typed collector both come back (the collector through
    /// `take_calls` + `as_any`).
    fn traced_run(module: &Module) -> (CallCollector, Result<Value, crate::VmError>) {
        let collector = CallCollector::new();
        let mut vm = Vm::new().with_calls(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_calls().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CallCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One executed call-and-return reports exactly three events: the
    /// call (site, callee, argument spellings, destination, resume
    /// target), the helper's return at the callee's frame depth, and the
    /// entry frame's return with the module result — and the run's value
    /// is the doubled argument.
    #[test]
    fn call_return_stream_reports_all_three_events() {
        let module = call_double_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(42));
        assert_eq!(collector.events().len(), 3);
        assert_eq!(collector.starts().len(), 1);
        assert_eq!(collector.returns().len(), 2);
        let call = collector.starts()[0];
        assert_eq!(call.caller_func, 1);
        assert_eq!(call.site_pc, 18, "the OP_CALL opcode's byte offset");
        assert_eq!(call.callee_func, 0);
        assert_eq!(call.args, vec![Value::Int(21)]);
        assert_eq!(call.dest_local, 2);
        assert_eq!(call.resume_target, 31);
        assert!(!call.indirect);
        let helper_return = collector.returns()[0];
        assert_eq!(helper_return.func, 0);
        assert_eq!(helper_return.depth, 2, "the callee's frame depth");
        assert_eq!(helper_return.value, Value::Int(42));
        let entry_return = collector.returns()[1];
        assert_eq!(entry_return.func, 1);
        assert_eq!(entry_return.depth, 1, "the entry frame's depth");
        assert_eq!(entry_return.value, Value::Int(42), "the module result");
        assert_eq!(collector.open_calls(), 0, "the run ended balanced");
        assert_eq!(collector.max_depth(), 1, "one call, no nesting");
        assert_eq!(collector.indirect_count(), 0);
    }

    /// A bounded recursion drives the depth folds: `count_down(3)` makes
    /// four calls (main→cd(3), then cd(3)→cd(2)→cd(1)→cd(0)), returns
    /// them LIFO, and the call-depth high-water mark is 4.
    #[test]
    fn recursion_drives_the_depth_folds() {
        let module = recursion_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(3));
        assert_eq!(collector.starts().len(), 4, "main + three recursions");
        assert_eq!(collector.returns().len(), 5, "four callees plus the entry");
        assert_eq!(collector.max_depth(), 4, "four nested calls at the deepest");
        assert_eq!(collector.open_calls(), 0);
        // The returns unwind innermost first: the deepest callee's frame
        // sat at machine depth 5, then 4, 3, 2, and the entry frame's
        // return at depth 1.
        let depths: Vec<usize> = collector.returns().iter().map(|r| r.depth).collect();
        assert_eq!(depths, vec![5, 4, 3, 2, 1], "LIFO unwind");
        // The recursion's argument values, callee order: 3 first, then
        // 2, 1, 0.
        let args: Vec<i64> = collector
            .starts()
            .iter()
            .map(|c| match c.args[0] {
                Value::Int(v) => v,
                _ => panic!("the recursion passes Ints"),
            })
            .collect();
        assert_eq!(args, vec![3, 2, 1, 0], "the passed counters, in order");
    }

    /// The render is deterministic and pins the transcript line spellings:
    /// the call line with its argument spellings, the return lines, and
    /// the entry frame's return.
    #[test]
    fn render_is_deterministic_golden() {
        let module = call_double_module();
        let (collector, _) = traced_run(&module);
        let first = render_calls(&collector);
        let second = render_calls(&collector);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
calls glyim.module-image/2 calls=1 returns=2 indirect=0 max-depth=1 open=0
0: call fn 1 pc 18 -> fn 0 argc 1 args=[21] dest 2 resume 31 indirect=false
1: return fn 0 depth 2 value=42
2: return fn 1 depth 1 value=42
"
        );
    }

    /// A faulting callee leaves the call open: the run faults with
    /// exactly the pre-seam error, the capture holds the call but no
    /// return for it, and the render displays the unreturned tail.
    #[test]
    fn a_fault_mid_call_displays_the_open_tail() {
        let module = faulting_callee_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the callee faults"),
            crate::VmError::AbnormalTermination
        );
        assert_eq!(collector.starts().len(), 1, "the call started");
        assert!(collector.returns().is_empty(), "no return followed");
        assert_eq!(collector.open_calls(), 1, "the open call is folded");
        let rendered = render_calls(&collector);
        assert!(rendered.contains("unreturned calls: 1"), "{rendered}");
    }

    /// A call the machine refuses (a bad function index) reports
    /// nothing — the fault stream owns the refusal — and the run faults
    /// with exactly the pre-seam error.
    #[test]
    fn a_refused_call_reports_nothing() {
        let module = bad_callee_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the bad callee faults"),
            crate::VmError::FunctionOutOfBounds(999)
        );
        assert!(collector.is_empty(), "no event for a refused call");
        assert_eq!(collector.open_calls(), 0);
    }

    /// A call-free straight line reports exactly one event: the entry
    /// frame's return (the module result) — the seam is total over the
    /// executed call stream, calls or not.
    #[test]
    fn a_call_free_run_reports_only_the_entry_return() {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(5));
        main.push(op(Opcode::Return)); // pc 9
        let module = Module::new(vec![Function::new(main, 0, 0)], 0);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(5));
        assert_eq!(collector.events().len(), 1);
        let entry_return = collector.returns()[0];
        assert_eq!(entry_return.func, 0);
        assert_eq!(entry_return.value, Value::Int(5));
    }

    /// A call-heavy loop reports one call and one return per iteration,
    /// interleaved in execution order.
    #[test]
    fn call_loop_reports_a_call_and_a_return_per_iteration() {
        let module = call_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(
            collector.events().len(),
            7,
            "three call+return pairs plus the entry return"
        );
        assert_eq!(collector.starts().len(), 3);
        assert_eq!(collector.returns().len(), 4);
        let kinds: Vec<&str> = collector
            .events()
            .iter()
            .map(|e| match e {
                CallEvent::Started(_) => "call",
                CallEvent::Returned(_) => "return",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "call", "return", "call", "return", "call", "return", "return"
            ]
        );
        let args: Vec<i64> = collector
            .starts()
            .iter()
            .map(|c| match c.args[0] {
                Value::Int(v) => v,
                _ => panic!("the loop passes Ints"),
            })
            .collect();
        assert_eq!(args, vec![3, 2, 1], "the passed counters, in order");
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = call_loop_module(3);
        let collector = CallCollector::bounded(4);
        let mut vm = Vm::new().with_calls(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_calls().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CallCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 4, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The seam is read-only: the traced run's value equals the untraced
    /// one exactly, and the faulting run's error equals the untraced one
    /// exactly.
    #[test]
    fn the_seam_is_read_only() {
        let paired = call_double_module();
        let untraced = Vm::new().run_module(&paired).expect("untraced run");
        let (_, traced) = traced_run(&paired);
        assert_eq!(traced.expect("traced run"), untraced);
        let faulting = faulting_callee_module();
        let untraced = Vm::new()
            .run_module(&faulting)
            .expect_err("untraced faulting run");
        let (_, traced) = traced_run(&faulting);
        assert_eq!(traced.expect_err("traced faulting run"), untraced);
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_call_events() {
        assert_eq!(
            render_calls(&CallCollector::new()),
            "\
calls glyim.module-image/2 calls=0 returns=0 indirect=0 max-depth=0 open=0
no call events
"
        );
    }

    /// The event types are comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let s = super::CallStarted {
            caller_func: 1,
            site_pc: 18,
            callee_func: 0,
            args: vec![Value::Int(21)],
            dest_local: 2,
            resume_target: 31,
            indirect: false,
        };
        let mut s2 = s.clone();
        assert_eq!(s, s2);
        s2.indirect = true;
        assert_ne!(s, s2);
        let r = super::CallReturned {
            func: 0,
            depth: 2,
            value: Value::Int(42),
        };
        assert_ne!(
            CallEvent::Started(s.clone()),
            CallEvent::Returned(r.clone()),
            "the transcript's two faces are distinct"
        );
    }
}
