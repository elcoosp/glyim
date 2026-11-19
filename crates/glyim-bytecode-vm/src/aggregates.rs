//! Composite-construction observation seam: what aggregate values did
//! this run *build*, at which sites, and from which operands?
//!
//! Every other value seam watches a value *after* it exists: the
//! local-watch seam ([`crate::watch`]) records what slots took, the
//! drop seam ([`crate::drops`]) records what the run destroyed, the
//! cast seam ([`crate::casts`]) records what the run transformed. The
//! moment of *construction* — the `OP_AGGREGATE` fold that turns the
//! operand stack's top `n` values into one tuple, and the `OP_REPEAT`
//! replication that turns one value and a count into an `n`-tuple —
//! was invisible: a lowering bug that aggregates the wrong operand
//! span, or repeats into the wrong length, produced a wrong tuple with
//! no observation anywhere on its path. This module is that
//! counterpart: hooks in the `OP_AGGREGATE` and `OP_REPEAT` arms — the
//! VM fires [`AggregateSink::aggregate_built`] just after the fold has
//! popped its operands and before the tuple is pushed, and
//! [`AggregateSink::repeat_built`] just after the count and value have
//! been popped and before the produced tuple is pushed — handing one
//! event per executed construction to a sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the pushed tuple is exactly the pre-seam machine's),
//! **total** (every *executed* construction reports — a construction
//! the machine *refuses* (an operand-stack underflow mid-fold, a
//! non-Int repeat count) never reaches the hook: the fault stream owns
//! the refusal), **bounded** (the collector's memory is capped and
//! flags overflow instead of growing without limit over a
//! construction-heavy loop), and **deterministic**
//! ([`render_aggregates`] renders the same collector byte-identically
//! — the test suite pins a golden).
//!
//! The hooks fire only when a sink is attached: one `Option` check per
//! executed construction, exactly the pre-seam behavior when the sink
//! is absent. The events' operands are sink-side copies (the
//! copies-into-the-event shape every value-carrying seam follows) —
//! the machine's own values stay exactly where they were.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// One executed aggregate fold, handed to the attached
/// [`AggregateSink`] just after the fold has popped its `arity`
/// operands off the operand stack and just before the built tuple is
/// pushed (a fold the machine *refuses* — an operand-stack underflow
/// mid-pop — never reaches the hook: the fault stream owns it): the
/// site, the arity, and the folded elements in source order.
#[derive(Clone, Debug, PartialEq)]
pub struct AggregateBuilt {
    /// The function index containing the fold site.
    pub func: usize,
    /// The byte offset of the `OP_AGGREGATE` opcode (the fold site).
    pub pc: usize,
    /// The fold's arity (the `u32` payload the opcode carries — how
    /// many operand-stack values the fold consumed).
    pub arity: usize,
    /// The folded elements in source order (sink-side copies — the
    /// machine pushes its own `Value::Tuple` over them right after the
    /// hook returns).
    pub elements: Vec<crate::Value>,
}

/// One executed replication, handed to the attached [`AggregateSink`]
/// just after the count and the value have been popped off the operand
/// stack and just before the produced tuple is pushed (a non-Int count
/// is an [`crate::VmError::ExpectedInt`] and never reaches the hook):
/// the site, the raw count, and the replicated value. The count is
/// **raw** — a negative count is not a fault (the machine clamps it to
/// the empty tuple), and the event displays it instead of silently
/// clamping it away.
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatBuilt {
    /// The function index containing the replication site.
    pub func: usize,
    /// The byte offset of the `OP_REPEAT` opcode (the replication site).
    pub pc: usize,
    /// The raw repeat count off the operand stack (a negative count
    /// produces the empty tuple — the raw value is displayed, never
    /// silently clamped away).
    pub count: i64,
    /// The replicated value (a sink-side copy — the machine clones it
    /// `count.max(0)` times into its own tuple right after the hook
    /// returns).
    pub value: crate::Value,
}

/// The interleaved construction transcript: one entry per executed
/// construction, in execution order — the shape
/// [`AggregateCollector::events`] returns and [`render_aggregates`]
/// lists.
#[derive(Clone, Debug, PartialEq)]
pub enum AggregateEvent {
    /// An aggregate fold ([`AggregateBuilt`]).
    Built(AggregateBuilt),
    /// A replication ([`RepeatBuilt`]).
    Repeated(RepeatBuilt),
}

/// A sink that observes every executed composite construction: an
/// [`AggregateSink`] the VM hands one [`AggregateBuilt`] per executed
/// `OP_AGGREGATE` and one [`RepeatBuilt`] per executed `OP_REPEAT`.
/// Attach through [`crate::Vm::with_aggregates`], recover through
/// [`crate::Vm::take_aggregates`] + [`AggregateSink::as_any`].
pub trait AggregateSink: 'static {
    /// One aggregate fold executed: the operands have been popped and
    /// the built tuple is about to be pushed (a refused fold never
    /// reaches the hook — the fault stream owns it).
    fn aggregate_built(&mut self, event: &AggregateBuilt);

    /// One replication executed: the count and the value have been
    /// popped and the produced tuple is about to be pushed (a non-Int
    /// count never reaches the hook — the fault stream owns it).
    fn repeat_built(&mut self, event: &RepeatBuilt);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_aggregates`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed composite construction in
/// memory.
///
/// `AggregateCollector::new()` is unbounded;
/// `AggregateCollector::bounded(max)` stops capturing at the cap and
/// sets the `overflowed` flag instead of growing without limit over a
/// construction-heavy loop — one event per construction must not OOM
/// the embedding host. The flag is a deliberate, *displayed* fact,
/// never a silent drop.
///
/// The collector also folds as it captures: the build tally
/// ([`AggregateCollector::built_count`]), the replication tally
/// ([`AggregateCollector::repeat_count`]), the element total
/// ([`AggregateCollector::total_elements`] — every folded arity plus
/// every produced repeat length, the run's composite-materialization
/// count), and the arity high-water mark
/// ([`AggregateCollector::max_arity`] — the largest tuple one
/// construction produced; a negative repeat's clamp contributes `0`
/// without moving the mark).
#[derive(Clone, Default)]
pub struct AggregateCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<AggregateEvent>,
    /// The build tally (the `OP_AGGREGATE` count).
    built: usize,
    /// The replication tally (the `OP_REPEAT` count).
    repeats: usize,
    /// The element total (folded arities plus produced repeat lengths).
    elements: usize,
    /// The arity high-water mark (the largest one-construction tuple).
    max_arity: usize,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl AggregateCollector {
    /// Create an unbounded collector (every executed construction is
    /// captured).
    #[must_use]
    pub fn new() -> AggregateCollector {
        AggregateCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> AggregateCollector {
        AggregateCollector {
            cap: Some(max),
            events: Vec::new(),
            built: 0,
            repeats: 0,
            elements: 0,
            max_arity: 0,
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order (builds and repeats
    /// interleaved exactly as the run performed them).
    #[must_use]
    pub fn events(&self) -> &[AggregateEvent] {
        &self.events
    }

    /// The captured folds, in execution order.
    #[must_use]
    pub fn builds(&self) -> Vec<&AggregateBuilt> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AggregateEvent::Built(b) => Some(b),
                AggregateEvent::Repeated(_) => None,
            })
            .collect()
    }

    /// The captured replications, in execution order.
    #[must_use]
    pub fn repeats(&self) -> Vec<&RepeatBuilt> {
        self.events
            .iter()
            .filter_map(|e| match e {
                AggregateEvent::Built(_) => None,
                AggregateEvent::Repeated(r) => Some(r),
            })
            .collect()
    }

    /// How many captured folds executed (`OP_AGGREGATE` events).
    #[must_use]
    pub fn built_count(&self) -> usize {
        self.built
    }

    /// How many captured replications executed (`OP_REPEAT` events).
    #[must_use]
    pub fn repeat_count(&self) -> usize {
        self.repeats
    }

    /// How many element slots the captured constructions materialized
    /// in total — every fold's arity plus every replication's produced
    /// length (a negative repeat's clamp contributes `0`).
    #[must_use]
    pub fn total_elements(&self) -> usize {
        self.elements
    }

    /// The largest tuple one captured construction produced (the arity
    /// high-water mark).
    #[must_use]
    pub fn max_arity(&self) -> usize {
        self.max_arity
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no construction was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl AggregateSink for AggregateCollector {
    fn aggregate_built(&mut self, event: &AggregateBuilt) {
        self.push(AggregateEvent::Built(event.clone()));
    }

    fn repeat_built(&mut self, event: &RepeatBuilt) {
        self.push(AggregateEvent::Repeated(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl AggregateCollector {
    /// Append one event under the capture cap (an overflow drops the
    /// event and sets the flag — displayed, never a silent drop),
    /// updating the tallies and the arity high-water mark as it goes.
    fn push(&mut self, event: AggregateEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        let (tally, arity) = match &event {
            AggregateEvent::Built(b) => (&mut self.built, b.arity),
            AggregateEvent::Repeated(r) => (&mut self.repeats, r.count.max(0) as usize),
        };
        *tally += 1;
        self.elements += arity;
        if arity > self.max_arity {
            self.max_arity = arity;
        }
        self.events.push(event);
    }
}

/// Render a construction stream into a deterministic listing: a header
/// line (image format version, the build/replication tallies, the
/// element total, and the arity high-water mark), then one line per
/// event in execution order — the event's kind (`build`/`repeat`), the
/// site, and the fold's element spellings or the replication's raw
/// count, replicated value, and produced arity — with a negative
/// repeat's clamp displayed in its `produced-arity 0` line. A run with
/// no constructions renders the explicit `no aggregate events` line
/// (honest about emptiness, like the call stream's `no call events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_aggregates(collector: &AggregateCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "aggregates glyim.module-image/{} built={} repeats={} elements={} max-arity={}",
        crate::IMAGE_VERSION,
        collector.built,
        collector.repeats,
        collector.elements,
        collector.max_arity,
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no aggregate events");
        debug!(events = 0, "vm aggregate stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        match event {
            AggregateEvent::Built(b) => {
                let mut elements = String::from("[");
                for (j, e) in b.elements.iter().enumerate() {
                    if j > 0 {
                        let _ = elements.write_str(", ");
                    }
                    let _ = write!(elements, "{}", crate::watch::render_value(e));
                }
                let _ = elements.write_str("]");
                let _ = writeln!(
                    out,
                    "{i}: build fn {} pc {} arity {} elements={elements}",
                    b.func, b.pc, b.arity,
                );
            }
            AggregateEvent::Repeated(r) => {
                let _ = writeln!(
                    out,
                    "{i}: repeat fn {} pc {} count {} value={} produced-arity {}",
                    r.func,
                    r.pc,
                    r.count,
                    crate::watch::render_value(&r.value),
                    r.count.max(0) as usize,
                );
            }
        }
    }
    debug!(
        events = collector.events.len(),
        "vm aggregate stream rendered"
    );
    out
}

#[cfg(test)]
mod aggregates_tests {
    use super::render_aggregates;
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

    /// main (fn 0) builds the pair `[21, 42]` from two constants and
    /// returns it: the canonical one-construction module.
    fn build_pair_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the first element
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::LoadConst)); // pc 9 — the second element
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::Aggregate)); // pc 18 — fold the top two
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::Return)); // pc 23 — pops the tuple
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) repeats `7` three times and returns the produced
    /// `[7, 7, 7]`: the canonical one-replication module.
    fn repeat_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the value
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadConst)); // pc 9 — the count
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Repeat)); // pc 18 — value first, count on top
        main.push(op(Opcode::Return)); // pc 19 — pops the tuple
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) builds the pair `[21, 42]`, then repeats `7` three
    /// times, and returns the produced tuple (the first build's tuple
    /// stays on the operand stack underneath): the interleaved shape —
    /// one build and one repeat in a single run.
    fn mixed_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the first element
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::LoadConst)); // pc 9 — the second element
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::Aggregate)); // pc 18 — build [21, 42]
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::LoadConst)); // pc 23 — the value
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadConst)); // pc 32 — the count
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Repeat)); // pc 41 — build [7, 7, 7]
        main.push(op(Opcode::Return)); // pc 42 — pops the repeat tuple
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) aggregates two elements with only one value on the
    /// operand stack: the machine refuses the fold mid-pop (a stack
    /// underflow), the run faults, and the seam must report nothing —
    /// the fault stream owns the refusal.
    fn underflowing_build_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the only operand
        main.extend_from_slice(&i64le(21));
        main.push(op(Opcode::Aggregate)); // pc 9 — needs two, pops one
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::Return)); // pc 14 — never reached
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) repeats `7` a *negative* number of times: the count
    /// is non-Int-rejected territory no more — a negative Int count is
    /// not a fault, the machine clamps it to the empty tuple, and the
    /// event must report the raw negative count (displayed, never
    /// silently clamped away).
    fn negative_repeat_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the value
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadConst)); // pc 9 — the negative count
        main.extend_from_slice(&i64le(-2));
        main.push(op(Opcode::Repeat)); // pc 18 — clamps to []
        main.push(op(Opcode::Return)); // pc 19 — pops the empty tuple
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) repeats `7` once per loop iteration (`count`
    /// iterations): the construction-heavy shape — one repeat event per
    /// iteration, the bounded collector's worst case.
    fn repeat_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): repeat 7 once, drop the tuple, decrement,
        // loop while still positive.
        main.push(op(Opcode::LoadConst)); // pc 14 — the value
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadConst)); // pc 23 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Repeat)); // pc 32 — one repeat per iteration
        main.push(op(Opcode::Drop)); // pc 33 — discard the tuple
        main.push(op(Opcode::LoadLocal)); // pc 34 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 39 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 48 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 49
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 54 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 59 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 68 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 69 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 74 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 79
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Run a module with an aggregate collector attached; the run's
    /// outcome and the typed collector both come back (the collector
    /// through `take_aggregates` + `as_any`).
    fn traced_run(module: &Module) -> (super::AggregateCollector, Result<Value, crate::VmError>) {
        let collector = super::AggregateCollector::new();
        let mut vm = Vm::new().with_aggregates(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_aggregates().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<super::AggregateCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One executed fold reports exactly one event: the site, the
    /// arity, and the folded elements in source order — and the run's
    /// value is the built tuple.
    #[test]
    fn the_build_stream_reports_the_fold() {
        let module = build_pair_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect("module runs"),
            Value::Tuple(vec![Value::Int(21), Value::Int(42)])
        );
        assert_eq!(collector.events().len(), 1);
        let built = collector.builds()[0];
        assert_eq!(built.func, 0);
        assert_eq!(built.pc, 18, "the OP_AGGREGATE opcode's byte offset");
        assert_eq!(built.arity, 2);
        assert_eq!(built.elements, vec![Value::Int(21), Value::Int(42)]);
        assert_eq!(collector.repeats().len(), 0);
        assert_eq!(collector.built_count(), 1);
        assert_eq!(collector.repeat_count(), 0);
        assert_eq!(collector.total_elements(), 2);
        assert_eq!(collector.max_arity(), 2);
    }

    /// One executed replication reports exactly one event: the site,
    /// the raw count, and the replicated value — the produced tuple's
    /// arity feeds the folds.
    #[test]
    fn the_repeat_stream_reports_the_replication() {
        let module = repeat_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect("module runs"),
            Value::Tuple(vec![Value::Int(7), Value::Int(7), Value::Int(7)])
        );
        assert_eq!(collector.events().len(), 1);
        let repeated = collector.repeats()[0];
        assert_eq!(repeated.func, 0);
        assert_eq!(repeated.pc, 18, "the OP_REPEAT opcode's byte offset");
        assert_eq!(repeated.count, 3);
        assert_eq!(repeated.value, Value::Int(7));
        assert_eq!(collector.built_count(), 0);
        assert_eq!(collector.repeat_count(), 1);
        assert_eq!(collector.total_elements(), 3, "the produced arity");
        assert_eq!(collector.max_arity(), 3);
    }

    /// A run that builds and repeats interleaves the two event kinds
    /// in execution order, and the folds accumulate over both.
    #[test]
    fn the_mixed_stream_interleaves_in_execution_order() {
        let module = mixed_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect("module runs"),
            Value::Tuple(vec![Value::Int(7), Value::Int(7), Value::Int(7)])
        );
        assert_eq!(collector.events().len(), 2, "one build plus one repeat");
        let kinds: Vec<&str> = collector
            .events()
            .iter()
            .map(|e| match e {
                super::AggregateEvent::Built(_) => "build",
                super::AggregateEvent::Repeated(_) => "repeat",
            })
            .collect();
        assert_eq!(kinds, vec!["build", "repeat"]);
        assert_eq!(collector.built_count(), 1);
        assert_eq!(collector.repeat_count(), 1);
        assert_eq!(
            collector.total_elements(),
            5,
            "two folded plus three produced"
        );
        assert_eq!(
            collector.max_arity(),
            3,
            "the repeat's produced arity high-water"
        );
    }

    /// The render is deterministic and pins the transcript line spellings:
    /// the header totals, the build line with its element spellings, and
    /// the repeat line with its raw count and produced arity.
    #[test]
    fn render_is_deterministic_golden() {
        let module = mixed_module();
        let (collector, _) = traced_run(&module);
        let first = render_aggregates(&collector);
        let second = render_aggregates(&collector);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
aggregates glyim.module-image/2 built=1 repeats=1 elements=5 max-arity=3
0: build fn 0 pc 18 arity 2 elements=[21, 42]
1: repeat fn 0 pc 41 count 3 value=7 produced-arity 3
"
        );
    }

    /// A construction the machine refuses (an operand-stack underflow
    /// mid-fold) reports nothing — the fault stream owns the refusal —
    /// and the run faults with exactly the pre-seam error.
    #[test]
    fn a_refused_build_reports_nothing() {
        let module = underflowing_build_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the underflowing fold faults"),
            crate::VmError::StackUnderflow
        );
        assert!(collector.is_empty(), "no event for a refused fold");
        assert_eq!(collector.total_elements(), 0);
    }

    /// A negative repeat count is not a fault: the machine clamps it to
    /// the empty tuple, and the event reports the raw negative count —
    /// displayed, never silently clamped away.
    #[test]
    fn a_negative_repeat_reports_the_raw_count() {
        let module = negative_repeat_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Tuple(vec![]));
        assert_eq!(collector.events().len(), 1);
        let repeated = collector.repeats()[0];
        assert_eq!(repeated.count, -2, "the raw count, not the clamp");
        assert_eq!(repeated.value, Value::Int(7));
        assert_eq!(collector.total_elements(), 0, "the clamp's arity");
        assert_eq!(collector.max_arity(), 0);
        let rendered = render_aggregates(&collector);
        assert!(rendered.contains("count -2"), "{rendered}");
        assert!(rendered.contains("produced-arity 0"), "{rendered}");
    }

    /// A construction-free straight line reports nothing — the seam is
    /// total over the *executed* construction stream, constructions or
    /// not — and the render says so explicitly.
    #[test]
    fn a_construction_free_run_reports_nothing() {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(5));
        main.push(op(Opcode::Return)); // pc 9
        let module = Module::new(vec![Function::new(main, 0, 0)], 0);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(5));
        assert!(collector.is_empty());
        assert_eq!(
            render_aggregates(&collector),
            "\
aggregates glyim.module-image/2 built=0 repeats=0 elements=0 max-arity=0
no aggregate events
"
        );
    }

    /// A construction-heavy loop reports one repeat event per
    /// iteration, in execution order.
    #[test]
    fn the_repeat_loop_reports_one_event_per_iteration() {
        let module = repeat_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 3, "one repeat per iteration");
        assert_eq!(collector.built_count(), 0);
        assert_eq!(collector.repeat_count(), 3);
        assert_eq!(collector.total_elements(), 3);
        assert_eq!(collector.max_arity(), 1);
    }

    /// A bounded collector stops at the cap and flags overflow
    /// (displayed, never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = repeat_loop_module(3);
        let collector = super::AggregateCollector::bounded(2);
        let mut vm = Vm::new().with_aggregates(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_aggregates().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<super::AggregateCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 2, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The seam is read-only: the traced run's value equals the
    /// untraced one exactly, and the faulting run's error equals the
    /// untraced one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let paired = mixed_module();
        let untraced = Vm::new().run_module(&paired).expect("untraced run");
        let (_, traced) = traced_run(&paired);
        assert_eq!(traced.expect("traced run"), untraced);
        let faulting = underflowing_build_module();
        let untraced = Vm::new()
            .run_module(&faulting)
            .expect_err("untraced faulting run");
        let (_, traced) = traced_run(&faulting);
        assert_eq!(traced.expect_err("traced faulting run"), untraced);
    }

    /// The event types are comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let b = super::AggregateBuilt {
            func: 0,
            pc: 18,
            arity: 2,
            elements: vec![Value::Int(21), Value::Int(42)],
        };
        let mut b2 = b.clone();
        assert_eq!(b, b2);
        b2.arity = 3;
        assert_ne!(b, b2);
        let r = super::RepeatBuilt {
            func: 0,
            pc: 41,
            count: 3,
            value: Value::Int(7),
        };
        assert_ne!(
            super::AggregateEvent::Built(b.clone()),
            super::AggregateEvent::Repeated(r.clone()),
            "the transcript's two faces are distinct"
        );
    }
}
