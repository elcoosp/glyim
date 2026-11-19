//! Memory-observation seam: what did the run's frame memory actually
//! hold, and which reads never saw a write?
//!
//! The local-watch seam ([`crate::watch`]) records what values a local
//! slot *took* and the data-flow seam ([`crate::dataflow`]) pairs those
//! writes with the reads that consumed them — but both watch the *slot*
//! face. The compiler's aggregate lowering also runs on the *memory*
//! face: `OP_STORE_FIELD` writes a field through an address and `OP_DEREF`
//! reads one back, and the frame's addressable memory region is
//! zero-initialised — a read of an address the run never wrote is a
//! silent zero, not an error. Nothing answered "which addresses did this
//! run write, and which reads came before their write" — the audit that
//! catches a wrong field offset (T042's class) reading a neighbour slot's
//! zero instead of the stored field. This module is that counterpart: a
//! hook in the `OP_STORE_FIELD` arm and the `OP_DEREF` arm — the VM fires
//! [`MemSink::mem_wrote`] just after a store lands and
//! [`MemSink::mem_read`] just after a deref's value is fetched — handing
//! one event per executed memory access to a sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the stored value and the pushed read are exactly the pre-seam
//! values), **total** (every *executed* access reports — a store refused
//! by the bounds check writes nothing and reports nothing, matching the
//! watch seam's failed-store rule; the fault stream owns the refusal),
//! **bounded** (the collector's memory is capped and flags overflow
//! instead of growing without limit over a memory-heavy loop), and
//! **deterministic** ([`render_memory`] renders the same collector
//! byte-identically — the test suite pins a golden).
//!
//! The hooks fire only when a sink is attached: one `Option` check per
//! executed access, exactly the pre-seam behavior when the sink is
//! absent. The write event takes a sink-side copy of the stored value
//! (the machine's own value moved into the slot), and the read event a
//! sink-side copy of the fetched value — the copies-into-the-event shape
//! every value-carrying seam follows.

use std::any::Any;
use std::collections::BTreeSet;
use std::fmt::Write as _;

use tracing::debug;

/// One frame-memory write, handed to the attached [`MemSink`] just after
/// the `OP_STORE_FIELD`'s value has landed in the slot: the site, the
/// address written, and the value in full.
#[derive(Clone, Debug, PartialEq)]
pub struct MemWriteEvent {
    /// The function index containing the store site.
    pub func: usize,
    /// The byte offset of the `OP_STORE_FIELD` opcode (the store site).
    pub pc: usize,
    /// The frame-relative address written (the popped address operand).
    pub addr: usize,
    /// The value the store landed (a sink-side copy — the machine's own
    /// value moved into the slot).
    pub value: crate::Value,
}

/// One frame-memory read, handed to the attached [`MemSink`] just after
/// the `OP_DEREF`'s value has been fetched: the site, the address read,
/// and the value in full.
#[derive(Clone, Debug, PartialEq)]
pub struct MemReadEvent {
    /// The function index containing the deref site.
    pub func: usize,
    /// The byte offset of the `OP_DEREF` opcode (the deref site).
    pub pc: usize,
    /// The frame-relative address read (the popped address operand).
    pub addr: usize,
    /// The value the deref fetched (a sink-side copy — the machine's own
    /// copy is pushed onto the operand stack right after the hook
    /// returns).
    pub value: crate::Value,
}

/// The interleaved memory transcript: one entry per executed access, in
/// execution order — the shape [`MemCollector::events`] returns and
/// [`render_memory`] lists.
#[derive(Clone, Debug, PartialEq)]
pub enum MemEvent {
    /// A write ([`MemWriteEvent`]).
    Write(MemWriteEvent),
    /// A read ([`MemReadEvent`]).
    Read(MemReadEvent),
}

/// A sink that observes every executed frame-memory access: a
/// [`MemSink`] the VM hands one [`MemWriteEvent`] per executed
/// `OP_STORE_FIELD` and one [`MemReadEvent`] per executed `OP_DEREF`.
/// Attach through [`crate::Vm::with_memory`], recover through
/// [`crate::Vm::take_memory`] + [`MemSink::as_any`].
pub trait MemSink: 'static {
    /// One store landed: the value has been written into the slot (a
    /// bounds-refused store never reaches the hook — the fault stream
    /// owns it).
    fn mem_wrote(&mut self, event: &MemWriteEvent);

    /// One deref fetched: the value has been read out of the slot and is
    /// about to be pushed.
    fn mem_read(&mut self, event: &MemReadEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_memory`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed frame-memory access in memory.
///
/// `MemCollector::new()` is unbounded; `MemCollector::bounded(max)` stops
/// capturing at the cap and sets the `overflowed` flag instead of growing
/// without limit over a memory-heavy loop — one event per access must not
/// OOM the embedding host. The flag is a deliberate, *displayed* fact,
/// never a silent drop.
///
/// The collector also folds as it captures: the addresses written so far
/// ([`MemCollector::is_written`]) and the reads that came before their
/// address's first write ([`MemCollector::unwritten_reads`]) — the
/// zero-initialised slot a wrong field offset would surface through.
#[derive(Clone, Default)]
pub struct MemCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<MemEvent>,
    /// The reads whose address had no prior write when the read executed,
    /// in execution order.
    unwritten: Vec<MemReadEvent>,
    /// The addresses written so far (the fold's written set).
    written: BTreeSet<usize>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl MemCollector {
    /// Create an unbounded collector (every executed access is captured).
    #[must_use]
    pub fn new() -> MemCollector {
        MemCollector::default()
    }

    /// Create a collector that captures at most `max` events (and folds
    /// at most `max` unwritten reads) and flags overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> MemCollector {
        MemCollector {
            cap: Some(max),
            events: Vec::new(),
            unwritten: Vec::new(),
            written: BTreeSet::new(),
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order (writes and reads
    /// interleaved exactly as the run performed them).
    #[must_use]
    pub fn events(&self) -> &[MemEvent] {
        &self.events
    }

    /// The captured writes, in execution order.
    #[must_use]
    pub fn writes(&self) -> Vec<&MemWriteEvent> {
        self.events
            .iter()
            .filter_map(|e| match e {
                MemEvent::Write(w) => Some(w),
                MemEvent::Read(_) => None,
            })
            .collect()
    }

    /// The captured reads, in execution order.
    #[must_use]
    pub fn reads(&self) -> Vec<&MemReadEvent> {
        self.events
            .iter()
            .filter_map(|e| match e {
                MemEvent::Write(_) => None,
                MemEvent::Read(r) => Some(r),
            })
            .collect()
    }

    /// The reads whose address had no prior write when the read executed,
    /// in execution order — the zero-initialised slots the run read
    /// before any store landed on them.
    #[must_use]
    pub fn unwritten_reads(&self) -> &[MemReadEvent] {
        &self.unwritten
    }

    /// How many captured reads came before their address's first write.
    #[must_use]
    pub fn unwritten_read_count(&self) -> usize {
        self.unwritten.len()
    }

    /// Whether the address has been written by any captured store so far.
    #[must_use]
    pub fn is_written(&self, addr: usize) -> bool {
        self.written.contains(&addr)
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no access was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl MemSink for MemCollector {
    fn mem_wrote(&mut self, event: &MemWriteEvent) {
        self.push(MemEvent::Write(event.clone()));
    }

    fn mem_read(&mut self, event: &MemReadEvent) {
        self.push(MemEvent::Read(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl MemCollector {
    /// Append one event under the capture cap (an overflow drops the event
    /// and sets the flag — displayed, never a silent drop), updating the
    /// written set and the unwritten-reads fold as it goes.
    fn push(&mut self, event: MemEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        match &event {
            MemEvent::Write(w) => {
                self.written.insert(w.addr);
            }
            MemEvent::Read(r) => {
                if !self.written.contains(&r.addr) {
                    if let Some(cap) = self.cap {
                        if self.unwritten.len() >= cap {
                            self.overflowed = true;
                            self.events.push(event);
                            return;
                        }
                    }
                    self.unwritten.push(r.clone());
                }
            }
        }
        self.events.push(event);
    }
}

/// Render a memory stream into a deterministic listing: a header line
/// (image format version, transcript size, write/read counts, and the
/// unwritten-read total), then one line per access in execution order —
/// the access's kind (`write`/`read`), the site's function index and
/// opcode byte offset, the frame-relative address, and the value in the
/// machine's value spellings — then the unwritten reads again as their
/// own section. A run with no accesses renders the explicit
/// `no memory events` line (honest about emptiness, like the assert
/// stream's `no assertions`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_memory(collector: &MemCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "memory glyim.module-image/{} events={} writes={} reads={} unwritten-reads={}",
        crate::IMAGE_VERSION,
        collector.events.len(),
        collector.writes().len(),
        collector.reads().len(),
        collector.unwritten.len(),
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no memory events");
        debug!(events = 0, "vm memory stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        match event {
            MemEvent::Write(w) => {
                let _ = writeln!(
                    out,
                    "{i}: write fn {} pc {} addr {} value={}",
                    w.func,
                    w.pc,
                    w.addr,
                    crate::watch::render_value(&w.value),
                );
            }
            MemEvent::Read(r) => {
                let _ = writeln!(
                    out,
                    "{i}: read fn {} pc {} addr {} value={}",
                    r.func,
                    r.pc,
                    r.addr,
                    crate::watch::render_value(&r.value),
                );
            }
        }
    }
    if collector.unwritten.is_empty() {
        debug!(events = collector.events.len(), "vm memory stream rendered");
        return out;
    }
    let _ = writeln!(out, "unwritten reads:");
    for (i, r) in collector.unwritten.iter().enumerate() {
        let _ = writeln!(
            out,
            "{i}: read fn {} pc {} addr {} value={}",
            r.func,
            r.pc,
            r.addr,
            crate::watch::render_value(&r.value),
        );
    }
    debug!(events = collector.events.len(), "vm memory stream rendered");
    out
}

#[cfg(test)]
mod memory_tests {
    use super::{MemCollector, MemEvent, MemWriteEvent, render_memory};
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

    /// main (fn 0) stores `42` through address 0 and reads it back: the
    /// canonical write-then-read pair (one event each, no unwritten
    /// reads). The emitter's operand order: the value goes on the stack
    /// first, the place address second — the address is on top and the
    /// arm pops it first.
    fn write_read_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the value
        main.extend_from_slice(&i64le(42));
        main.push(op(Opcode::LoadLocalAddr)); // pc 9 — the address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::StoreField)); // pc 14 — the store
        main.push(op(Opcode::LoadLocalAddr)); // pc 15 — the address again
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Deref)); // pc 20 — the read back
        main.push(op(Opcode::Return)); // pc 21
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) reads address 1 without ever writing it: the
    /// zero-initialised slot an unwritten read surfaces.
    fn read_before_write_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadLocalAddr)); // pc 0 — never written
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Deref)); // pc 5 — the unwritten read
        main.push(op(Opcode::Return)); // pc 6
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) writes address 0 with `7`, reads it back, then reads
    /// address 1 (never written): the render golden's module — a write, a
    /// paired read, and an unwritten read in one entry point.
    fn mixed_memory_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the value
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadLocalAddr)); // pc 9 — the address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::StoreField)); // pc 14 — the store
        main.push(op(Opcode::LoadLocalAddr)); // pc 15 — the address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Deref)); // pc 20 — the paired read
        main.push(op(Opcode::LoadLocalAddr)); // pc 21 — never written
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Deref)); // pc 26 — the unwritten read
        main.push(op(Opcode::Return)); // pc 27
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// A countdown loop whose body stores the counter through address 0
    /// and reads it back every iteration: a memory-heavy shape (one write
    /// and one read per iteration). Seeds local 0 with `count`.
    fn store_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): store the counter through address 0, read it
        // back, drop the read, decrement, loop while still positive.
        main.push(op(Opcode::LoadLocal)); // pc 14 — the value
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocalAddr)); // pc 19 — the address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::StoreField)); // pc 24 — one write per iteration
        main.push(op(Opcode::LoadLocalAddr)); // pc 25 — the address
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Deref)); // pc 30 — one read per iteration
        main.push(op(Opcode::Drop)); // pc 31 — discard the read
        main.push(op(Opcode::LoadLocal)); // pc 32 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 37 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 46 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 47
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 52 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 57 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 66 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 67 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 72
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 77
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0) stores through an address past the frame's memory
    /// region (`n_locals = 1` sizes the region at 8 slots): the bounds
    /// refusal the seam must not report.
    fn faulting_store_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the value
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadLocalAddr)); // pc 9 — wild address
        main.extend_from_slice(&u32le(100));
        main.push(op(Opcode::StoreField)); // pc 14 — refused
        main.push(op(Opcode::Return)); // pc 15 — never reached
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Run a module with a memory collector attached; the run's outcome
    /// and the typed collector both come back (the collector through
    /// `take_memory` + `as_any`).
    fn traced_run(module: &Module) -> (MemCollector, Result<Value, crate::VmError>) {
        let collector = MemCollector::new();
        let mut vm = Vm::new().with_memory(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_memory().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<MemCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One executed store-and-read reports exactly two events: the write
    /// (site, address, value) and the read back — and the run's value is
    /// the stored one.
    #[test]
    fn write_read_stream_reports_both_events() {
        let module = write_read_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(42));
        assert_eq!(collector.events().len(), 2);
        assert_eq!(collector.writes().len(), 1);
        assert_eq!(collector.reads().len(), 1);
        let write = collector.writes()[0];
        assert_eq!(write.func, 0);
        assert_eq!(write.pc, 14, "the OP_STORE_FIELD opcode's byte offset");
        assert_eq!(write.addr, 0);
        assert_eq!(write.value, Value::Int(42));
        let read = collector.reads()[0];
        assert_eq!(read.pc, 20, "the OP_DEREF opcode's byte offset");
        assert_eq!(read.addr, 0);
        assert_eq!(read.value, Value::Int(42));
        assert_eq!(collector.unwritten_read_count(), 0);
        assert!(collector.is_written(0));
    }

    /// A read of an address the run never wrote is displayed as an
    /// unwritten read — the zero-initialised slot the wrong-field-offset
    /// class surfaces through — and the run still returns the machine's
    /// zero (the seam observes, it never faults).
    #[test]
    fn read_before_write_is_displayed() {
        let module = read_before_write_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 1);
        assert_eq!(collector.unwritten_read_count(), 1);
        let unwritten = &collector.unwritten_reads()[0];
        assert_eq!(unwritten.addr, 1);
        assert_eq!(unwritten.pc, 5);
        assert_eq!(unwritten.value, Value::Int(0), "the zero-init read");
        assert!(!collector.is_written(1));
    }

    /// A memory-heavy loop reports one write and one read per iteration,
    /// interleaved in execution order, with no unwritten reads.
    #[test]
    fn memory_loop_reports_a_write_and_a_read_per_iteration() {
        let module = store_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 6, "two events per iteration");
        assert_eq!(collector.writes().len(), 3);
        assert_eq!(collector.reads().len(), 3);
        assert_eq!(collector.unwritten_read_count(), 0);
        // The transcript interleaves write/read per iteration, and every
        // access names the same pair of sites (pc 24, pc 30).
        let kinds: Vec<&str> = collector
            .events()
            .iter()
            .map(|e| match e {
                MemEvent::Write(_) => "write",
                MemEvent::Read(_) => "read",
            })
            .collect();
        assert_eq!(
            kinds,
            vec!["write", "read", "write", "read", "write", "read"]
        );
        let values: Vec<i64> = collector
            .writes()
            .iter()
            .map(|w| match w.value {
                Value::Int(v) => v,
                _ => panic!("the loop stores Ints"),
            })
            .collect();
        assert_eq!(values, vec![3, 2, 1], "the stored counters, in order");
    }

    /// The render is deterministic and pins the transcript line spellings:
    /// the interleaved write/read lines and the unwritten-reads section.
    #[test]
    fn render_is_deterministic_golden() {
        let module = mixed_memory_module();
        let (collector, _) = traced_run(&module);
        let first = render_memory(&collector);
        let second = render_memory(&collector);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
memory glyim.module-image/2 events=3 writes=1 reads=2 unwritten-reads=1
0: write fn 0 pc 14 addr 0 value=7
1: read fn 0 pc 20 addr 0 value=7
2: read fn 0 pc 26 addr 1 value=0
unwritten reads:
0: read fn 0 pc 26 addr 1 value=0
"
        );
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = store_loop_module(3);
        let collector = MemCollector::bounded(4);
        let mut vm = Vm::new().with_memory(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_memory().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<MemCollector>()
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
        let write_read = write_read_module();
        let untraced = Vm::new().run_module(&write_read).expect("untraced run");
        let (_, traced) = traced_run(&write_read);
        assert_eq!(traced.expect("traced run"), untraced);
        let faulting = faulting_store_module();
        let untraced = Vm::new()
            .run_module(&faulting)
            .expect_err("untraced faulting run");
        let (_, traced) = traced_run(&faulting);
        assert_eq!(traced.expect_err("traced faulting run"), untraced);
    }

    /// A store refused by the bounds check reports nothing — the fault
    /// stream owns the refusal — and the run faults with exactly the
    /// pre-seam error.
    #[test]
    fn a_failed_store_reports_nothing() {
        let module = faulting_store_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the wild store faults"),
            crate::VmError::LocalOutOfBounds(100)
        );
        assert!(collector.is_empty(), "no event for a refused store");
        assert!(collector.writes().is_empty());
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_memory_events() {
        assert_eq!(
            render_memory(&MemCollector::new()),
            "\
memory glyim.module-image/2 events=0 writes=0 reads=0 unwritten-reads=0
no memory events
"
        );
    }

    /// The event types are comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let w = MemWriteEvent {
            func: 0,
            pc: 14,
            addr: 0,
            value: Value::Int(7),
        };
        let mut w2 = w.clone();
        assert_eq!(w, w2);
        w2.value = Value::Int(8);
        assert_ne!(w, w2);
        let r = super::MemReadEvent {
            func: 0,
            pc: 20,
            addr: 0,
            value: Value::Int(7),
        };
        assert_ne!(
            MemEvent::Write(w.clone()),
            MemEvent::Read(r.clone()),
            "the transcript's two faces are distinct"
        );
    }
}
