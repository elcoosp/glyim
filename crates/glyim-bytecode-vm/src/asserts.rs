//! Assertion-observation seam: what did every assertion the run evaluated
//! actually observe?
//!
//! The coverage seam ([`crate::coverage`]) says *which instruction sites
//! executed*; the branch-history seam ([`crate::branches`]) says *which
//! way every decision went*. An `OP_ASSERT` is both — a decision the
//! program's own invariant wrote — but its outcome is folded away by both
//! views: coverage counts the site as hit, and the branch history records
//! a landing the assert's polarity forced. Nothing answered "did this
//! run's assertions actually hold, and which one failed" — the one
//! question a failing run's `VmError::AssertionFailed` answers only for
//! the *last* assert, not for the ones that passed before it. This module
//! is that counterpart: a hook in the `OP_ASSERT` arm — the VM fires
//! [`AssertSink::assert_evaluated`] the moment an assertion's condition
//! has been evaluated, *before* the fault a failing assertion raises —
//! handing one [`AssertEvent`] per evaluated assert to a sink the caller
//! attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state and cannot flip an assert's outcome — a failing assert still
//! faults with exactly the pre-seam error), **total** (every evaluated
//! assert reports, passing or failing — the failing one is the event
//! list's last entry, the run's fault raised right after), **bounded**
//! (the collector's memory is capped and flags overflow instead of
//! growing without limit over an assert-heavy loop), and **deterministic**
//! ([`render_asserts`] renders the same event list byte-identically — the
//! test suite pins a golden).
//!
//! The hook fires only when a sink is attached: one `Option` check per
//! executed `OP_ASSERT`, exactly the pre-seam behavior when the sink is
//! absent. The event carries the assert's site (function index and the
//! opcode's byte offset), the polarity the instruction asserted, the
//! condition the machine observed, and the pass/fail verdict.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// One assertion-evaluation event, handed to the attached [`AssertSink`]
/// the moment the assert's condition has been evaluated: the site, the
/// asserted polarity, the observed condition, and the verdict. A `passed`
/// event is followed by normal execution; a failing one is the list's
/// last entry — the run faults with [`crate::VmError::AssertionFailed`]
/// immediately after the hook fires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertEvent {
    /// The function index containing the assert site.
    pub func: usize,
    /// The byte offset of the `OP_ASSERT` opcode (the assert site).
    pub pc: usize,
    /// The polarity the instruction asserted (`true` = the condition must
    /// hold, `false` = it must not).
    pub expected: bool,
    /// The condition the machine actually observed (the popped operand,
    /// zero = `false`, non-zero = `true`).
    pub observed: bool,
    /// Whether the observed condition matched the asserted polarity
    /// (`false` = the run is about to fault).
    pub passed: bool,
}

/// A sink that observes every assertion evaluation: an [`AssertSink`] the
/// VM hands one [`AssertEvent`] to at each executed `OP_ASSERT`, passing
/// or failing. Attach through [`crate::Vm::with_asserts`], recover
/// through [`crate::Vm::take_asserts`] + [`AssertSink::as_any`].
pub trait AssertSink: 'static {
    /// One assertion evaluated: its condition has been read and compared
    /// against the asserted polarity. Fires whether the verdict is `true`
    /// (execution continues) or `false` (the run faults right after the
    /// hook returns).
    fn assert_evaluated(&mut self, event: &AssertEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_asserts`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every assertion evaluation in memory.
///
/// `AssertCollector::new()` is unbounded; `AssertCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit over an assert-heavy loop — one event per assert
/// must not OOM the embedding host. The flag is a deliberate, *displayed*
/// fact, never a silent drop.
#[derive(Clone, Default)]
pub struct AssertCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured events, in evaluation order.
    events: Vec<AssertEvent>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl AssertCollector {
    /// Create an unbounded collector (every assertion evaluation is
    /// captured).
    #[must_use]
    pub fn new() -> AssertCollector {
        AssertCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> AssertCollector {
        AssertCollector {
            cap: Some(max),
            events: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured events, in evaluation order.
    #[must_use]
    pub fn events(&self) -> &[AssertEvent] {
        &self.events
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no assertion was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The captured events for one function's assert sites, in evaluation
    /// order.
    #[must_use]
    pub fn events_of(&self, func: usize) -> Vec<&AssertEvent> {
        self.events.iter().filter(|e| e.func == func).collect()
    }

    /// How many captured assertions passed (the observed condition matched
    /// the asserted polarity).
    #[must_use]
    pub fn passed_count(&self) -> usize {
        self.events.iter().filter(|e| e.passed).count()
    }

    /// How many captured assertions failed (each is followed by the fault
    /// that ended the run, so an unbounded single-run capture holds at
    /// most one).
    #[must_use]
    pub fn failed_count(&self) -> usize {
        self.events.iter().filter(|e| !e.passed).count()
    }
}

impl AssertSink for AssertCollector {
    fn assert_evaluated(&mut self, event: &AssertEvent) {
        self.push(event.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl AssertCollector {
    /// Append one event under the capture cap (an overflow drops the event
    /// and sets the flag — displayed, never a silent drop).
    fn push(&mut self, event: AssertEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.events.push(event);
    }
}

/// Render an assertion stream into a deterministic listing: a header line
/// (image format version, assertion count), then one line per evaluation
/// in event order — the verdict (`pass`/`fail`), the site's function index
/// and opcode byte offset, and the asserted polarity against the observed
/// condition in the machine's Int-bool spelling (`1` = non-zero, `0` =
/// zero). A run with no assertions renders the explicit `no assertions`
/// line (honest about emptiness, like the call graph's `no calls`).
///
/// Deterministic and total (any event list renders).
#[must_use]
pub fn render_asserts(events: &[AssertEvent]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "assert glyim.module-image/{} asserts={}",
        crate::IMAGE_VERSION,
        events.len()
    );
    if events.is_empty() {
        let _ = writeln!(out, "no assertions");
        debug!(asserts = 0, "vm assert stream rendered");
        return out;
    }
    for (i, event) in events.iter().enumerate() {
        let _ = writeln!(
            out,
            "{i}: {} fn {} pc {} expected={} observed={}",
            if event.passed { "pass" } else { "fail" },
            event.func,
            event.pc,
            i64::from(event.expected),
            i64::from(event.observed),
        );
    }
    debug!(asserts = events.len(), "vm assert stream rendered");
    out
}

#[cfg(test)]
mod asserts_tests {
    use super::{AssertCollector, AssertEvent, render_asserts};
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

    /// main (fn 0) asserts `true` (expected 1, observed 1) and returns 42.
    /// The canonical one-assert module: one event, the run succeeds.
    fn passing_assert_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the condition (true)
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Assert)); // pc 9 — site; expected 1, fall-through 15
        main.push(1u8);
        main.extend_from_slice(&u32le(15));
        main.push(op(Opcode::LoadConst)); // pc 15
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::Return)); // pc 24
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) asserts polarity 0 over an observed 0 (the
    /// assert-not convention) and returns 7.
    fn negative_polarity_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the condition (false)
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Assert)); // pc 9 — expected 0, observed 0
        main.push(0u8);
        main.extend_from_slice(&u32le(15));
        main.push(op(Opcode::LoadConst)); // pc 15
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::Return)); // pc 24
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Three asserts in one function: pass, pass (polarity 0), then a
    /// failing assert (expected 1 over an observed 0). The run faults at
    /// the third; the stream keeps all three evaluations.
    fn three_asserts_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Assert)); // pc 9 — pass (1 over 1)
        main.push(1u8);
        main.extend_from_slice(&u32le(15));
        main.push(op(Opcode::LoadConst)); // pc 15
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Assert)); // pc 24 — pass (0 over 0)
        main.push(0u8);
        main.extend_from_slice(&u32le(30));
        main.push(op(Opcode::LoadConst)); // pc 30
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Assert)); // pc 39 — fail (0 over 1)
        main.push(1u8);
        main.extend_from_slice(&u32le(45));
        main.push(op(Opcode::LoadConst)); // pc 45 — never reached
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::Return)); // pc 54
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// A countdown loop whose body asserts the counter is non-negative
    /// every iteration: an assert-heavy shape (one event per iteration).
    /// Seeds local 0 with `count` and asserts `counter >= 0` each pass.
    fn assert_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): assert the counter is non-negative, then
        // decrement it, then loop while it is still positive.
        main.push(op(Opcode::LoadLocal)); // pc 14 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 19 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Ge)); // pc 28 — observed condition
        main.push(op(Opcode::Assert)); // pc 29 — site; expected 1, fall-through 35
        main.push(1u8);
        main.extend_from_slice(&u32le(35));
        main.push(op(Opcode::LoadLocal)); // pc 35 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 40 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 49 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 50
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 55 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 60 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 69 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 70 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 75
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 80
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Run a module with an assert collector attached; the run's outcome
    /// and the typed collector both come back (the collector through
    /// `take_asserts` + `as_any`).
    fn traced_run(module: &Module) -> (AssertCollector, Result<Value, crate::VmError>) {
        let collector = AssertCollector::new();
        let mut vm = Vm::new().with_asserts(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_asserts().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<AssertCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One passing assert reports exactly one event: the site, the
    /// polarity, the observed condition, and the pass verdict — and the
    /// run's value is untouched.
    #[test]
    fn passing_assert_reports_one_event() {
        let module = passing_assert_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(42));
        assert_eq!(collector.events().len(), 1);
        let event = &collector.events()[0];
        assert_eq!(event.func, 0);
        assert_eq!(event.pc, 9, "the OP_ASSERT opcode's byte offset");
        assert!(event.expected, "the instruction asserted truth");
        assert!(event.observed, "the machine observed a non-zero condition");
        assert!(event.passed);
        assert_eq!(collector.passed_count(), 1);
        assert_eq!(collector.failed_count(), 0);
    }

    /// Polarity 0 (the assert-not convention) passes over an observed
    /// zero: expected and observed are reported independently of the
    /// verdict.
    #[test]
    fn polarity_zero_passes_over_an_observed_zero() {
        let module = negative_polarity_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(7));
        let event = &collector.events()[0];
        assert!(!event.expected);
        assert!(!event.observed);
        assert!(event.passed);
    }

    /// A failing assert still reports — it is the stream's last entry —
    /// and the run faults with `AssertionFailed` exactly like the
    /// untraced run (the seam is read-only).
    #[test]
    fn failing_assert_reports_then_the_run_faults() {
        let module = three_asserts_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the failing assert faults"),
            crate::VmError::AssertionFailed
        );
        assert_eq!(collector.events().len(), 3);
        assert_eq!(collector.passed_count(), 2);
        assert_eq!(collector.failed_count(), 1, "the failing assert displays");
        let last = &collector.events()[2];
        assert!(!last.passed, "the failing verdict is displayed");
        assert!(!last.observed, "the machine observed the zero condition");
        assert!(last.expected, "the instruction asserted truth");
        assert_eq!(
            crate::Vm::new()
                .run_module(&module)
                .expect_err("untraced run"),
            crate::VmError::AssertionFailed,
            "the untraced run faults identically"
        );
    }

    /// An assert-heavy loop reports one event per iteration: the loop's
    /// per-site events are readable through `events_of`.
    #[test]
    fn assert_loop_reports_one_event_per_iteration() {
        let module = assert_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 3, "one assert per iteration");
        assert_eq!(collector.events_of(0).len(), 3);
        assert!(collector.events().iter().all(|e| e.passed));
        // Every event names the same site: the loop body's assert at pc 29.
        assert!(collector.events().iter().all(|e| e.pc == 29));
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = assert_loop_module(3);
        let collector = AssertCollector::bounded(2);
        let mut vm = Vm::new().with_asserts(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_asserts().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<AssertCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 2, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The render is deterministic and pins the evaluation line spellings:
    /// the verdict, the site, and the polarity/observation pair in the
    /// machine's Int-bool spelling.
    #[test]
    fn render_is_deterministic_golden() {
        let module = three_asserts_module();
        let (collector, _) = traced_run(&module);
        let first = render_asserts(collector.events());
        let second = render_asserts(collector.events());
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
assert glyim.module-image/2 asserts=3
0: pass fn 0 pc 9 expected=1 observed=1
1: pass fn 0 pc 24 expected=0 observed=0
2: fail fn 0 pc 39 expected=1 observed=0
"
        );
    }

    /// The seam is read-only: the traced run's value equals the untraced
    /// one exactly, and the traced faulting run's error equals the
    /// untraced one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let passing = passing_assert_module();
        let mut vm = Vm::new();
        let untraced = vm.run_module(&passing).expect("untraced run");
        let (_, traced) = traced_run(&passing);
        assert_eq!(traced.expect("traced run"), untraced);
        let failing = three_asserts_module();
        let untraced = Vm::new()
            .run_module(&failing)
            .expect_err("untraced faulting run");
        let (_, traced) = traced_run(&failing);
        assert_eq!(traced.expect_err("traced faulting run"), untraced);
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_assertions() {
        assert_eq!(
            render_asserts(&[]),
            "\
assert glyim.module-image/2 asserts=0
no assertions
"
        );
    }

    /// The event type is comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let a = AssertEvent {
            func: 0,
            pc: 9,
            expected: true,
            observed: true,
            passed: true,
        };
        let mut b = a.clone();
        assert_eq!(a, b);
        b.observed = false;
        b.passed = false;
        assert_ne!(a, b);
    }
}
