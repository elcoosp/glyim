//! Composite-projection observation seam: what did this run *ask*
//! about its aggregate values, at which sites, and what did the
//! machine answer?
//!
//! The aggregate seam ([`crate::aggregates`]) watches the moment of
//! *construction*; nothing watched the moment of *interrogation*. The
//! `OP_LEN` length read and the `OP_DISCRIMINANT` discriminant read are
//! the machine's only aggregate-projection opcodes, and both have a
//! documented *scalar convention* — a projection over a non-aggregate
//! value answers `0` (the pre-float VM's convention) — so a lowering
//! bug that projects a scalar, or a field-offset bug that reads the
//! wrong slot's length, lands in that convention as a silent zero with
//! no observation anywhere on its path. This module is that
//! counterpart: hooks in the `OP_LEN` and `OP_DISCRIMINANT` arms — the
//! VM fires [`ProjectionSink::length_read`] just after the operand has
//! been fetched from its local and before the answer is pushed, and
//! [`ProjectionSink::discriminant_read`] just after the operand has
//! been popped and before the answer is pushed — handing one event per
//! executed projection to a sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the pushed answer is exactly the pre-seam machine's),
//! **total** (every *executed* projection reports — a projection the
//! machine *refuses* (an out-of-bounds local index) never reaches the
//! hook: the fault stream owns the refusal), **bounded** (the
//! collector's memory is capped and flags overflow instead of growing
//! without limit over a projection-heavy loop), and **deterministic**
//! ([`render_projections`] renders the same collector byte-identically
//! — the test suite pins a golden).
//!
//! The hooks fire only when a sink is attached: one `Option` check per
//! executed projection, exactly the pre-seam behavior when the sink is
//! absent. The events' operands are sink-side copies (the
//! copies-into-the-event shape every value-carrying seam follows) —
//! the machine's own values stay exactly where they were.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// One executed length read, handed to the attached
/// [`ProjectionSink`] just after the operand has been fetched from its
/// local slot and just before the answer is pushed (a read the machine
/// *refuses* — an out-of-bounds local index — never reaches the hook:
/// the fault stream owns it): the site, the local index, the projected
/// operand in full, and the machine's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct LenRead {
    /// The function index containing the read site.
    pub func: usize,
    /// The byte offset of the `OP_LEN` opcode (the read site).
    pub pc: usize,
    /// The local slot the read projected.
    pub local: usize,
    /// The projected operand (a sink-side copy — the machine reads its
    /// own local and pushes its own answer right after the hook
    /// returns).
    pub operand: crate::Value,
    /// The machine's answer (the pushed `Value::Int`): the tuple's
    /// element count, or `0` for a non-aggregate operand — the
    /// documented scalar convention.
    pub result: i64,
}

/// One executed discriminant read, handed to the attached
/// [`ProjectionSink`] just after the operand has been popped off the
/// operand stack and just before the answer is pushed (a non-aggregate
/// operand is not a fault — the scalar convention answers `0` — and the
/// empty-stack pop the opcode would fault on never reaches the hook):
/// the site, the projected operand in full, and the machine's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscrRead {
    /// The function index containing the read site.
    pub func: usize,
    /// The byte offset of the `OP_DISCRIMINANT` opcode (the read site).
    pub pc: usize,
    /// The projected operand (a sink-side copy — the machine popped its
    /// own operand and pushes its own answer right after the hook
    /// returns).
    pub operand: crate::Value,
    /// The machine's answer (the pushed `Value::Int`): the tuple's
    /// element count, or `0` for a non-aggregate operand — the
    /// documented scalar convention.
    pub result: i64,
}

/// The interleaved projection transcript: one entry per executed
/// projection, in execution order — the shape
/// [`ProjectionCollector::events`] returns and
/// [`render_projections`] lists.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectionEvent {
    /// A length read ([`LenRead`]).
    Len(LenRead),
    /// A discriminant read ([`DiscrRead`]).
    Discriminant(DiscrRead),
}

/// A sink that observes every executed aggregate projection: a
/// [`ProjectionSink`] the VM hands one [`LenRead`] per executed
/// `OP_LEN` and one [`DiscrRead`] per executed `OP_DISCRIMINANT`.
/// Attach through [`crate::Vm::with_projections`], recover through
/// [`crate::Vm::take_projections`] + [`ProjectionSink::as_any`].
pub trait ProjectionSink: 'static {
    /// One length read executed: the operand has been fetched from its
    /// local and the answer is about to be pushed (a refused read never
    /// reaches the hook — the fault stream owns it).
    fn length_read(&mut self, event: &LenRead);

    /// One discriminant read executed: the operand has been popped and
    /// the answer is about to be pushed.
    fn discriminant_read(&mut self, event: &DiscrRead);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_projections`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed aggregate projection in memory.
///
/// `ProjectionCollector::new()` is unbounded;
/// `ProjectionCollector::bounded(max)` stops capturing at the cap and
/// sets the `overflowed` flag instead of growing without limit over a
/// projection-heavy loop — one event per projection must not OOM the
/// embedding host. The flag is a deliberate, *displayed* fact, never a
/// silent drop.
///
/// The collector also folds as it captures: the length-read tally
/// ([`ProjectionCollector::len_count`]), the discriminant-read tally
/// ([`ProjectionCollector::discr_count`]), and the scalar-convention
/// tally ([`ProjectionCollector::scalar_reads`] — the reads whose
/// operand was a non-aggregate and the documented convention answered
/// `0`; the path a lowering bug projects a scalar or a field-offset bug
/// reads the wrong slot's length lands in, kept visible instead of
/// silently consumed).
#[derive(Clone, Default)]
pub struct ProjectionCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<ProjectionEvent>,
    /// The length-read tally (the `OP_LEN` count).
    lens: usize,
    /// The discriminant-read tally (the `OP_DISCRIMINANT` count).
    discrs: usize,
    /// The scalar-convention tally (the non-aggregate operands).
    scalars: usize,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl ProjectionCollector {
    /// Create an unbounded collector (every executed projection is
    /// captured).
    #[must_use]
    pub fn new() -> ProjectionCollector {
        ProjectionCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> ProjectionCollector {
        ProjectionCollector {
            cap: Some(max),
            events: Vec::new(),
            lens: 0,
            discrs: 0,
            scalars: 0,
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order (length and
    /// discriminant reads interleaved exactly as the run performed
    /// them).
    #[must_use]
    pub fn events(&self) -> &[ProjectionEvent] {
        &self.events
    }

    /// The captured length reads, in execution order.
    #[must_use]
    pub fn len_events(&self) -> Vec<&LenRead> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ProjectionEvent::Len(l) => Some(l),
                ProjectionEvent::Discriminant(_) => None,
            })
            .collect()
    }

    /// The captured discriminant reads, in execution order.
    #[must_use]
    pub fn discr_events(&self) -> Vec<&DiscrRead> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ProjectionEvent::Len(_) => None,
                ProjectionEvent::Discriminant(d) => Some(d),
            })
            .collect()
    }

    /// How many captured length reads executed (`OP_LEN` events).
    #[must_use]
    pub fn len_count(&self) -> usize {
        self.lens
    }

    /// How many captured discriminant reads executed
    /// (`OP_DISCRIMINANT` events).
    #[must_use]
    pub fn discr_count(&self) -> usize {
        self.discrs
    }

    /// How many captured reads projected a non-aggregate operand — the
    /// scalar-convention path, where a lowering bug that projects a
    /// scalar (or a field-offset bug that reads the wrong slot's
    /// length) lands as a silent zero.
    #[must_use]
    pub fn scalar_reads(&self) -> usize {
        self.scalars
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no projection was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl ProjectionSink for ProjectionCollector {
    fn length_read(&mut self, event: &LenRead) {
        self.push(ProjectionEvent::Len(event.clone()));
    }

    fn discriminant_read(&mut self, event: &DiscrRead) {
        self.push(ProjectionEvent::Discriminant(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl ProjectionCollector {
    /// Append one event under the capture cap (an overflow drops the
    /// event and sets the flag — displayed, never a silent drop),
    /// updating the tallies and the scalar-convention fold as it goes.
    fn push(&mut self, event: ProjectionEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        let (tally, scalar) = match &event {
            ProjectionEvent::Len(l) => (&mut self.lens, l.result == 0),
            ProjectionEvent::Discriminant(d) => (&mut self.discrs, d.result == 0),
        };
        *tally += 1;
        if scalar {
            self.scalars += 1;
        }
        self.events.push(event);
    }
}

/// Render a projection stream into a deterministic listing: a header
/// line (image format version, the length/discriminant tallies, and
/// the scalar-convention tally), then one line per event in execution
/// order — the event's kind (`len`/`discr`), the read site, the local
/// slot (for length reads), the projected operand in the machine's
/// value spellings, and the answer after a `->` — with the scalar
/// convention's zeros spelled like any other answer (the tally, not
/// the line, is where the convention is audited). A run with no
/// projections renders the explicit `no projection events` line
/// (honest about emptiness, like the aggregate stream's `no aggregate
/// events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_projections(collector: &ProjectionCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "projections glyim.module-image/{} len={} discr={} scalar={}",
        crate::IMAGE_VERSION,
        collector.lens,
        collector.discrs,
        collector.scalars,
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no projection events");
        debug!(events = 0, "vm projection stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        match event {
            ProjectionEvent::Len(l) => {
                let _ = writeln!(
                    out,
                    "{i}: len fn {} pc {} local {} value={} -> {}",
                    l.func,
                    l.pc,
                    l.local,
                    crate::watch::render_value(&l.operand),
                    l.result,
                );
            }
            ProjectionEvent::Discriminant(d) => {
                let _ = writeln!(
                    out,
                    "{i}: discr fn {} pc {} value={} -> {}",
                    d.func,
                    d.pc,
                    crate::watch::render_value(&d.operand),
                    d.result,
                );
            }
        }
    }
    debug!(
        events = collector.events.len(),
        "vm projection stream rendered"
    );
    out
}

#[cfg(test)]
mod projections_tests {
    use super::render_projections;
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

    /// main (fn 0) builds the triple `[1, 2, 3]` into local 0 and reads
    /// its length: the canonical one-projection module.
    fn len_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the first element
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConst)); // pc 9 — the second element
        main.extend_from_slice(&i64le(2));
        main.push(op(Opcode::LoadConst)); // pc 18 — the third element
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Aggregate)); // pc 27 — build (1, 2, 3)
        main.extend_from_slice(&u32le(3));
        main.push(op(Opcode::StoreLocal)); // pc 32 — bind local 0
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Len)); // pc 37 — read local 0's length
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 42 — pops the answer
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) builds the triple `[1, 2, 3]` into local 0, pushes a
    /// copy, and reads its discriminant: the stack-popped projection.
    fn discr_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the first element
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConst)); // pc 9 — the second element
        main.extend_from_slice(&i64le(2));
        main.push(op(Opcode::LoadConst)); // pc 18 — the third element
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Aggregate)); // pc 27 — build (1, 2, 3)
        main.extend_from_slice(&u32le(3));
        main.push(op(Opcode::StoreLocal)); // pc 32 — bind local 0
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 37 — push a copy
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Discriminant)); // pc 42 — pop and project
        main.push(op(Opcode::Return)); // pc 43 — pops the answer
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) stores the scalar `7` into local 0 and reads its
    /// length: the *scalar convention* shape — the machine answers `0`,
    /// and the event must report the scalar operand so the convention
    /// path stays visible.
    fn scalar_len_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the scalar
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::StoreLocal)); // pc 9 — bind local 0
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Len)); // pc 14 — read local 0's length
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 19 — pops the 0
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) reads the length of local 5 with no locals at all:
    /// the machine refuses the projection (an out-of-bounds local), the
    /// run faults, and the seam must report nothing — the fault stream
    /// owns the refusal.
    fn refused_len_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::Len)); // pc 0 — local 5 out of bounds
        main.extend_from_slice(&u32le(5));
        main.push(op(Opcode::Return)); // pc 5 — never reached
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) reads the length of a freshly built pair in a
    /// countdown loop (`count` iterations): the projection-heavy shape
    /// — one length read per iteration.
    fn len_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): build (1, 2) into local 1, read its
        // length, drop the answer, decrement, loop while still positive.
        main.push(op(Opcode::LoadConst)); // pc 14 — the first element
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConst)); // pc 23 — the second element
        main.extend_from_slice(&i64le(2));
        main.push(op(Opcode::Aggregate)); // pc 32 — build (1, 2)
        main.extend_from_slice(&u32le(2));
        main.push(op(Opcode::StoreLocal)); // pc 37 — bind local 1
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Len)); // pc 42 — read local 1's length
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Drop)); // pc 47 — discard the answer
        main.push(op(Opcode::LoadLocal)); // pc 48 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 53 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 62 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 63
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 68 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 73 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 82 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 83 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 88 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 93
        Module::new(vec![Function::new(main, 2, 0)], 0)
    }

    /// Run a module with a projection collector attached; the run's
    /// outcome and the typed collector both come back (the collector
    /// through `take_projections` + `as_any`).
    fn traced_run(module: &Module) -> (super::ProjectionCollector, Result<Value, crate::VmError>) {
        let collector = super::ProjectionCollector::new();
        let mut vm = Vm::new().with_projections(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_projections().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<super::ProjectionCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One executed length read reports exactly one event: the site,
    /// the local index, the projected operand in full, and the
    /// machine's answer — and the run's value is the answer.
    #[test]
    fn the_len_stream_reports_the_read() {
        let module = len_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(3));
        assert_eq!(collector.events().len(), 1);
        let len = collector.len_events()[0];
        assert_eq!(len.func, 0);
        assert_eq!(len.pc, 37, "the OP_LEN opcode's byte offset");
        assert_eq!(len.local, 0);
        assert_eq!(
            len.operand,
            Value::Tuple(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        assert_eq!(len.result, 3);
        assert_eq!(collector.len_count(), 1);
        assert_eq!(collector.discr_count(), 0);
        assert_eq!(collector.scalar_reads(), 0, "the operand was a tuple");
    }

    /// One executed discriminant read reports exactly one event: the
    /// site, the popped operand in full, and the machine's answer —
    /// the aggregate seam built the tuple, the discriminant read says
    /// what the run then asked about it.
    #[test]
    fn the_discr_stream_reports_the_read() {
        let module = discr_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(3));
        assert_eq!(collector.events().len(), 1);
        let discr = collector.discr_events()[0];
        assert_eq!(discr.func, 0);
        assert_eq!(discr.pc, 42, "the OP_DISCRIMINANT opcode's byte offset");
        assert_eq!(
            discr.operand,
            Value::Tuple(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        assert_eq!(discr.result, 3);
        assert_eq!(collector.len_count(), 0);
        assert_eq!(collector.discr_count(), 1);
    }

    /// A projection over a scalar lands in the documented scalar
    /// convention (`0`), and the event reports the scalar operand —
    /// the convention path stays visible, never silently consumed.
    #[test]
    fn a_scalar_read_reports_the_zero_convention() {
        let module = scalar_len_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        let len = collector.len_events()[0];
        assert_eq!(len.operand, Value::Int(7), "the scalar operand");
        assert_eq!(len.result, 0, "the convention's answer");
        assert_eq!(
            collector.scalar_reads(),
            1,
            "the convention path is counted"
        );
    }

    /// The render is deterministic and pins the transcript line
    /// spellings: the header tallies, the length line with its local
    /// index and operand spelling, and the discriminant line.
    #[test]
    fn render_is_deterministic_golden() {
        let module = len_module();
        let (collector, _) = traced_run(&module);
        let first = render_projections(&collector);
        let second = render_projections(&collector);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
projections glyim.module-image/2 len=1 discr=0 scalar=0
0: len fn 0 pc 37 local 0 value=(1, 2, 3) -> 3
"
        );
    }

    /// A projection the machine refuses (an out-of-bounds local index)
    /// reports nothing — the fault stream owns the refusal — and the
    /// run faults with exactly the pre-seam error.
    #[test]
    fn a_refused_projection_reports_nothing() {
        let module = refused_len_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the refused read faults"),
            crate::VmError::LocalOutOfBounds(5)
        );
        assert!(collector.is_empty(), "no event for a refused read");
    }

    /// A projection-free straight line reports nothing — the seam is
    /// total over the *executed* projection stream, projections or not
    /// — and the render says so explicitly.
    #[test]
    fn a_projection_free_run_reports_nothing() {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(5));
        main.push(op(Opcode::Return)); // pc 9
        let module = Module::new(vec![Function::new(main, 0, 0)], 0);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(5));
        assert!(collector.is_empty());
        assert_eq!(
            render_projections(&collector),
            "\
projections glyim.module-image/2 len=0 discr=0 scalar=0
no projection events
"
        );
    }

    /// A projection-heavy loop reports one length read per iteration,
    /// in execution order.
    #[test]
    fn the_len_loop_reports_one_event_per_iteration() {
        let module = len_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 3, "one length read per iteration");
        assert_eq!(collector.len_count(), 3);
        assert_eq!(collector.discr_count(), 0);
        assert_eq!(collector.scalar_reads(), 0);
    }

    /// A bounded collector stops at the cap and flags overflow
    /// (displayed, never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = len_loop_module(3);
        let collector = super::ProjectionCollector::bounded(2);
        let mut vm = Vm::new().with_projections(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_projections().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<super::ProjectionCollector>()
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
        let paired = len_module();
        let untraced = Vm::new().run_module(&paired).expect("untraced run");
        let (_, traced) = traced_run(&paired);
        assert_eq!(traced.expect("traced run"), untraced);
        let faulting = refused_len_module();
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
        let l = super::LenRead {
            func: 0,
            pc: 37,
            local: 0,
            operand: Value::Tuple(vec![Value::Int(1)]),
            result: 1,
        };
        let mut l2 = l.clone();
        assert_eq!(l, l2);
        l2.result = 2;
        assert_ne!(l, l2);
        let d = super::DiscrRead {
            func: 0,
            pc: 42,
            operand: Value::Int(7),
            result: 0,
        };
        assert_ne!(
            super::ProjectionEvent::Len(l.clone()),
            super::ProjectionEvent::Discriminant(d.clone()),
            "the transcript's two faces are distinct"
        );
    }
}
