//! Drop-observation seam: what did the run discard, and where?
//!
//! The watch seam ([`crate::watch`]) records what values a local slot
//! *took*; the call-state seam ([`crate::state`]) records the state a
//! frame *held* at its boundaries. Neither answers the destruction-side
//! question: every `OP_DROP` discards a value — the scope exits the
//! emitter compiles to — and the discarded value vanished from every
//! view at the pop. This module is that counterpart: a hook in the
//! `OP_DROP` arm — the VM fires [`DropSink::dropped`] with the value the
//! opcode just popped, so the discard stream (the emitter's scope-exit
//! shape, `OP_DROP` per destroyed value) is readable after the run —
//! handing one [`DropEvent`] per discard to a sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state and cannot resurrect a discarded value — the pop the opcode
//! pays is unchanged), **total** (every executed `OP_DROP` reports — the
//! event borrows the popped value, the pop the opcode already paid for,
//! so the seam adds no clone on the fire path), **bounded** (the
//! collector's memory is capped and flags overflow instead of growing
//! without limit over a drop-heavy loop), and **deterministic**
//! ([`render_drops`] renders the same event list byte-identically — the
//! test suite pins a golden).
//!
//! The hook fires only when a sink is attached: one `Option` check per
//! executed `OP_DROP`, exactly the pre-seam behavior when the sink is
//! absent. The event carries the discard site (function index, opcode
//! byte offset) and the discarded value in full.

use std::any::Any;
use std::fmt::Write as _;

use crate::Value;
use tracing::debug;

/// One discard event, handed to the attached [`DropSink`] the moment the
/// `OP_DROP`'s value has been popped: the discard site and the discarded
/// value in full (an address the scope-exit shape discards, an aggregate
/// the program destroyed — whatever the operand stack held).
#[derive(Clone, Debug, PartialEq)]
pub struct DropEvent {
    /// The function index containing the discard site.
    pub func: usize,
    /// The byte offset of the `OP_DROP` opcode (the discard site).
    pub pc: usize,
    /// The discarded value (the pop the opcode already paid for).
    pub value: Value,
}

/// A sink that observes every discard: a [`DropSink`] the VM hands one
/// [`DropEvent`] to at each executed `OP_DROP`. Attach through
/// [`crate::Vm::with_drops`], recover through [`crate::Vm::take_drops`] +
/// [`DropSink::as_any`].
pub trait DropSink: 'static {
    /// One value discarded: the `OP_DROP`'s operand has been popped and
    /// is about to be dropped. The event borrows the popped value — the
    /// discard itself is unchanged.
    fn dropped(&mut self, event: &DropEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_drops`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every discard in memory.
///
/// `DropCollector::new()` is unbounded; `DropCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit over a drop-heavy loop — one event per discard
/// must not OOM the embedding host. The flag is a deliberate, *displayed*
/// fact, never a silent drop.
#[derive(Clone, Default)]
pub struct DropCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured events, in discard order.
    events: Vec<DropEvent>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl DropCollector {
    /// Create an unbounded collector (every discard is captured).
    #[must_use]
    pub fn new() -> DropCollector {
        DropCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> DropCollector {
        DropCollector {
            cap: Some(max),
            events: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured events, in discard order.
    #[must_use]
    pub fn events(&self) -> &[DropEvent] {
        &self.events
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no discard was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The captured events for one function's discard sites, in discard
    /// order.
    #[must_use]
    pub fn events_of(&self, func: usize) -> Vec<&DropEvent> {
        self.events.iter().filter(|e| e.func == func).collect()
    }
}

impl DropSink for DropCollector {
    fn dropped(&mut self, event: &DropEvent) {
        self.push(event.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl DropCollector {
    /// Append one event under the capture cap (an overflow drops the
    /// event and sets the flag — displayed, never a silent drop).
    fn push(&mut self, event: DropEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.events.push(event);
    }
}

/// Render a discard stream into a deterministic listing: a header line
/// (image format version, discard count), then one line per discard in
/// event order — the site's function index and opcode byte offset, and
/// the discarded value in the diagnostic value spellings
/// (`render_watch`'s — integers plain, floats with their raw bits,
/// strings quoted, tuples parenthesized). A run with no discards renders
/// the explicit `no drops` line (honest about emptiness, like the call
/// graph's `no calls`).
///
/// Deterministic and total (any event list renders).
#[must_use]
pub fn render_drops(events: &[DropEvent]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "drops glyim.module-image/{} drops={}",
        crate::IMAGE_VERSION,
        events.len()
    );
    if events.is_empty() {
        let _ = writeln!(out, "no drops");
        debug!(drops = 0, "vm drop stream rendered");
        return out;
    }
    for (i, event) in events.iter().enumerate() {
        let _ = writeln!(
            out,
            "{i}: fn {} pc {} value={}",
            event.func,
            event.pc,
            crate::watch::render_value(&event.value),
        );
    }
    debug!(drops = events.len(), "vm drop stream rendered");
    out
}

#[cfg(test)]
mod drops_tests {
    use super::{DropCollector, DropEvent, render_drops};
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

    /// main (fn 0) loads slot 0's address and drops it, then returns 42.
    /// The canonical one-discard module: the scope-exit shape the emitter
    /// compiles, one event.
    fn one_drop_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadLocalAddr)); // pc 0 — slot 0's address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Drop)); // pc 5 — the discard site
        main.push(op(Opcode::LoadConst)); // pc 6
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::Return)); // pc 15
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) builds a two-element aggregate and drops it, then
    /// drops slot 0's address, then returns 42: two discards with
    /// different shapes (the aggregate the program destroyed, the
    /// address the scope exit discarded).
    fn two_shape_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — element 0
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConst)); // pc 9 — element 1
        main.extend_from_slice(&i64le(2));
        main.push(op(Opcode::Aggregate)); // pc 18 — (1, 2)
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::Drop)); // pc 23 — the aggregate destroyed
        main.push(op(Opcode::LoadLocalAddr)); // pc 24 — slot 0's address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Drop)); // pc 29 — the address discarded
        main.push(op(Opcode::LoadConst)); // pc 30
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::Return)); // pc 39
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// A countdown loop whose body loads slot 0's address and drops it
    /// every iteration: a drop-heavy shape (one event per iteration).
    fn drop_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): discard slot 0's address, decrement the
        // counter, then loop while it is still positive.
        main.push(op(Opcode::LoadLocalAddr)); // pc 14 — slot 0's address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Drop)); // pc 19 — one discard per iteration
        main.push(op(Opcode::LoadLocal)); // pc 20 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 25 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 34 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 35
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 40 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 45 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 54 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 55 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 60
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 65
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Run a module with a drop collector attached; the run's outcome and
    /// the typed collector both come back (the collector through
    /// `take_drops` + `as_any`).
    fn traced_run(module: &Module) -> (DropCollector, Result<Value, crate::VmError>) {
        let collector = DropCollector::new();
        let mut vm = Vm::new().with_drops(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_drops().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<DropCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One discard reports exactly one event: the site and the discarded
    /// address — and the run's value is untouched.
    #[test]
    fn one_discard_reports_one_event() {
        let module = one_drop_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(42));
        assert_eq!(collector.events().len(), 1);
        let event = &collector.events()[0];
        assert_eq!(event.func, 0);
        assert_eq!(event.pc, 5, "the OP_DROP opcode's byte offset");
        assert_eq!(
            event.value,
            Value::Int(0),
            "the discarded address (slot 0, the load-local-addr spelling)"
        );
    }

    /// The discarded value is reported in full: an aggregate the program
    /// destroyed and an address the scope exit discarded are two events
    /// with two shapes, readable through `events_of`.
    #[test]
    fn discarded_values_report_in_full() {
        let module = two_shape_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(42));
        assert_eq!(collector.events().len(), 2);
        assert_eq!(collector.events_of(0).len(), 2);
        assert_eq!(
            collector.events()[0].value,
            Value::Tuple(vec![Value::Int(1), Value::Int(2)]),
            "the destroyed aggregate, element order preserved"
        );
        assert_eq!(collector.events()[0].pc, 23);
        assert_eq!(collector.events()[1].value, Value::Int(0));
        assert_eq!(collector.events()[1].pc, 29);
    }

    /// A drop-heavy loop reports one event per iteration: the loop's
    /// per-site events all carry the same discarded address.
    #[test]
    fn drop_loop_reports_one_event_per_iteration() {
        let module = drop_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 3, "one discard per iteration");
        assert!(collector.events().iter().all(|e| e.pc == 19));
        assert!(
            collector.events().iter().all(|e| e.value == Value::Int(0)),
            "every iteration discarded slot 0's address"
        );
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = drop_loop_module(3);
        let collector = DropCollector::bounded(2);
        let mut vm = Vm::new().with_drops(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_drops().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<DropCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 2, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The render is deterministic and pins the discard line spellings:
    /// the site and the value in the watch render's value spellings.
    #[test]
    fn render_is_deterministic_golden() {
        let module = two_shape_module();
        let (collector, _) = traced_run(&module);
        let first = render_drops(collector.events());
        let second = render_drops(collector.events());
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
drops glyim.module-image/2 drops=2
0: fn 0 pc 23 value=(1, 2)
1: fn 0 pc 29 value=0
"
        );
    }

    /// The seam is read-only: the traced run's value equals the untraced
    /// one exactly (the discard the opcode pays is unchanged).
    #[test]
    fn the_seam_is_read_only() {
        let module = two_shape_module();
        let mut vm = Vm::new();
        let untraced = vm.run_module(&module).expect("untraced run");
        let (_, traced) = traced_run(&module);
        assert_eq!(traced.expect("traced run"), untraced);
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_drops() {
        assert_eq!(
            render_drops(&[]),
            "\
drops glyim.module-image/2 drops=0
no drops
"
        );
    }

    /// The event type is comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let a = DropEvent {
            func: 0,
            pc: 5,
            value: Value::Int(0),
        };
        let mut b = a.clone();
        assert_eq!(a, b);
        b.value = Value::Int(1);
        assert_ne!(a, b);
    }
}
