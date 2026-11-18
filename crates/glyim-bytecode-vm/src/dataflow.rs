//! Data-flow seam: which reads consumed which writes?
//!
//! The local-watch seam ([`crate::watch`]) records what values a module's
//! local slots *took*; nothing pairs those writes with the reads that
//! consumed them. A run that reads a slot it never stored in (the frame's
//! zero-initialisation leaking into semantics) is invisible to every other
//! seam: the trace shows a `LoadLocal`, the watch shows the writes, but
//! "this read had no producing write in its frame" is nowhere. This module
//! is the *pairing* counterpart: a read hook on the machine's
//! `OP_LOAD_LOCAL` dispatch and a fold that links each read to the dispatch
//! that last wrote the slot — per `(function, frame depth, slot)`, the same
//! key the watch events carry, so recursion disambiguates by depth and a
//! read whose slot was never written in its frame is reported through
//! `uninitialized_reads` instead of silently consuming the zero-init.
//!
//! # Contract
//!
//! The seam is **read-only** (a sink observes; it never mutates VM state
//! and cannot change execution), **total over the reads** (every successful
//! `OP_LOAD_LOCAL` is reported; a read that faults — a
//! `LocalOutOfBounds` load — reads nothing and reports nothing; the
//! address-form `OP_LOAD_LOCAL_ADDR` and the aggregate-length `OP_LEN` are
//! not value reads and are out of this seam's scope, the same scoping the
//! watch seam applies to its three write paths), and **zero-cost when
//! unused** (one `None` check per local read when no sink is attached).
//!
//! The pairing needs both streams: attach the same [`DataFlowCollector`]
//! through [`Vm::with_watch`](crate::Vm::with_watch) *and*
//! [`Vm::with_reads`](crate::Vm::with_reads) — the write side rides the
//! watch hook (the three write paths the watch seam reports), the read
//! side rides the read hook, and both fire synchronously in dispatch order,
//! so the fold sees a true interleaving. A host that wants the raw
//! interleaved transcript attaches a [`FlowRecorder`] the same way and
//! replays it through [`DataFlowCollector::from_transcript`]. Separate
//! sinks on the two hooks are legal but cannot pair (a read stream without
//! its write stream reports every read as uninitialized — the honest
//! answer given what that sink saw).
//!
//! # Bounded memory
//!
//! The fold is a constant number of counters per *slot touched* (never per
//! dispatch), and the `uninitialized_reads` list is capped: a host can
//! generate unboundedly many uninitialized reads without growing the
//! collector past the cap, and the cap's overflow is a *displayed* fact
//! (`overflowed` plus the exact total), never a silent drop.
//!
//! # Composition
//!
//! The seam composes with every other one: a trace sink rides the dispatch
//! loop, the watch and data-flow sinks ride the write/read paths, fuel
//! refusal means a read never happens (a refused dispatch reports
//! nothing), and a decoded image watches and reads exactly like an
//! in-memory module.

use std::any::Any;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tracing::debug;

use crate::Value;
use crate::watch::{WatchEvent, WatchSink};

/// Lock a shared fold, recovering the guard if a panicking sink poisoned
/// the mutex (the same recovery [`crate::watch`]-era shared state uses:
/// the counters are plain additions, not an invariant a panic could have
/// left half-updated).
fn lock_fold<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One local value read: a successful `OP_LOAD_LOCAL` fetched a slot's
/// value.
///
/// The field shapes mirror [`WatchEvent`] exactly (minus the write-path
/// kind): `pc` is the load's dispatch site, `dispatch` its 1-based lifetime
/// number (the same count [`crate::Vm::executed`] reports and every other
/// seam's events pair by), and `frame_depth` disambiguates two same-function
/// frames (recursion).
#[derive(Debug, Clone, PartialEq)]
pub struct ReadEvent {
    /// The index of the function whose frame owns the slot.
    pub func: usize,
    /// The slot index that was read.
    pub local: usize,
    /// The value the slot held at the read (a clone — the machine keeps
    /// executing).
    pub value: Value,
    /// The read dispatch's pc within the executing function.
    pub pc: usize,
    /// The read dispatch's 1-based lifetime number.
    pub dispatch: u64,
    /// Depth of the frame that owns the slot (1 = the entry frame).
    pub frame_depth: usize,
}

/// The observer the VM hands each successful local read to.
///
/// Implementors must treat [`ReadSink::read`] as an observation only: the
/// VM's state and control flow are invisible to the sink and unaffected by
/// it. The method must not panic (the VM never defends against a panicking
/// sink — a sink that panics aborts the run, which is the embedding host's
/// explicit choice). The [`ReadSink::as_any`] hook lets a caller recover
/// the typed sink after [`crate::Vm::take_reads`].
pub trait ReadSink: 'static {
    /// One local read, reported just after the value is fetched (so a read
    /// that faults — a bounds failure — is never reported).
    fn read(&mut self, event: &ReadEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_reads`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// One slot's data-flow counters, keyed by `(function, frame depth, slot)`.
///
/// `last_write_dispatch` is the producing dispatch of the slot's *current*
/// value — the def every read since it pairs with. A slot with reads and no
/// writes (or reads before the first write) accumulates
/// `uninitialized_reads`: the frame's zero-initialisation leaking into
/// semantics, displayed rather than silently consumed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotFlow {
    writes: u64,
    reads: u64,
    last_write_dispatch: Option<u64>,
    last_read_dispatch: Option<u64>,
    uninitialized_reads: u64,
}

impl SlotFlow {
    /// How many writes the slot took.
    #[must_use]
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// How many reads the slot served.
    #[must_use]
    pub fn reads(&self) -> u64 {
        self.reads
    }

    /// The dispatch that produced the slot's current value, if any write
    /// was observed.
    #[must_use]
    pub fn last_write_dispatch(&self) -> Option<u64> {
        self.last_write_dispatch
    }

    /// The most recent read's dispatch, if any read was observed.
    #[must_use]
    pub fn last_read_dispatch(&self) -> Option<u64> {
        self.last_read_dispatch
    }

    /// How many reads ran before any write to the slot in this frame — the
    /// zero-init consumptions this slot contributed.
    #[must_use]
    pub fn uninitialized_reads(&self) -> u64 {
        self.uninitialized_reads
    }
}

/// One uninitialized read, pinned at the dispatch that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninitializedRead {
    /// The reading function's index.
    pub func: usize,
    /// The slot that was read before any write.
    pub local: usize,
    /// Depth of the reading frame (1 = the entry frame).
    pub frame_depth: usize,
    /// The read dispatch's pc within the reading function.
    pub pc: usize,
    /// The read dispatch's 1-based lifetime number.
    pub dispatch: u64,
}

/// How many uninitialized reads a default [`DataFlowCollector`] keeps in
/// its displayed list (the total stays exact regardless of the cap).
const DEFAULT_UNINITIALIZED_CAP: usize = 64;

/// The data-flow fold state behind the shared [`DataFlowCollector`] handle.
#[derive(Debug, Default, PartialEq, Eq)]
struct FoldState {
    /// Per-slot counters, keyed `(function, frame depth, slot)`.
    slots: HashMap<(usize, usize, usize), SlotFlow>,
    /// The displayed uninitialized list (capped).
    uninitialized: Vec<UninitializedRead>,
    /// The cap on the displayed list.
    uninit_cap: usize,
    /// Whether any uninitialized read was dropped from the list (the total
    /// stays exact).
    overflowed: bool,
}

/// The data-flow fold: pairs reads with writes into per-slot counters.
///
/// The collector implements *both* sink traits — [`WatchSink`] for the
/// write stream (the watch hook's three write paths) and [`ReadSink`] for
/// the read stream (`OP_LOAD_LOCAL`) — so attaching the *same* collector
/// through [`Vm::with_watch`](crate::Vm::with_watch) and
/// [`Vm::with_reads`](crate::Vm::with_reads) gives the fold a true
/// interleaving and every read resolves to its producing dispatch. The
/// two hooks take the sink by value (`Box`), so the collector is a
/// shared-state handle: `clone()` shares one fold, and that is exactly how
/// a host attaches one collector to both hooks. The state is a constant
/// number of counters per slot touched (never per dispatch); the displayed
/// uninitialized list is capped by
/// [`DataFlowCollector::with_uninitialized_cap`] with the exact total kept
/// in [`DataFlowCollector::uninitialized_read_total`].
#[derive(Debug, Clone)]
pub struct DataFlowCollector {
    inner: std::sync::Arc<Mutex<FoldState>>,
}

impl Default for DataFlowCollector {
    fn default() -> DataFlowCollector {
        DataFlowCollector::new()
    }
}

impl PartialEq for DataFlowCollector {
    fn eq(&self, other: &DataFlowCollector) -> bool {
        *lock_fold(&self.inner) == *lock_fold(&other.inner)
    }
}

impl Eq for DataFlowCollector {}

impl DataFlowCollector {
    /// Create a collector with the default displayed uninitialized-list
    /// cap ([`DEFAULT_UNINITIALIZED_CAP`]).
    #[must_use]
    pub fn new() -> DataFlowCollector {
        DataFlowCollector::with_uninitialized_cap(DEFAULT_UNINITIALIZED_CAP)
    }

    /// Create a collector whose displayed uninitialized list holds at most
    /// `cap` entries (the total stays exact; the overflow is displayed).
    #[must_use]
    pub fn with_uninitialized_cap(cap: usize) -> DataFlowCollector {
        DataFlowCollector {
            inner: std::sync::Arc::new(Mutex::new(FoldState {
                slots: HashMap::new(),
                uninitialized: Vec::new(),
                uninit_cap: cap,
                overflowed: false,
            })),
        }
    }

    /// Replay a captured transcript into the same aggregate an incremental
    /// fold would have produced (the transcript-replay contract the other
    /// seams' `from_*` re-folds follow).
    #[must_use]
    pub fn from_transcript(events: impl IntoIterator<Item = FlowEvent>) -> DataFlowCollector {
        let collector = DataFlowCollector::new();
        for event in events {
            match event {
                FlowEvent::Write(write) => collector.record_write(&write),
                FlowEvent::Read(read) => collector.record_read(&read),
            }
        }
        collector
    }

    /// Fold one write event (the [`WatchSink::write`] implementation
    /// delegates here, and a write-only host can call it directly).
    pub fn record_write(&self, event: &WatchEvent) {
        let mut state = lock_fold(&self.inner);
        let slot = state
            .slots
            .entry((event.func, event.frame_depth, event.local))
            .or_default();
        slot.writes += 1;
        slot.last_write_dispatch = Some(event.dispatch);
    }

    /// Fold one read event (the [`ReadSink::read`] implementation
    /// delegates here, and a read-only host can call it directly). A read
    /// whose slot has no observed write in its frame counts as
    /// uninitialized — displayed, never silently consumed.
    pub fn record_read(&self, event: &ReadEvent) {
        let mut state = lock_fold(&self.inner);
        let slot = state
            .slots
            .entry((event.func, event.frame_depth, event.local))
            .or_default();
        slot.reads += 1;
        slot.last_read_dispatch = Some(event.dispatch);
        if slot.last_write_dispatch.is_none() {
            slot.uninitialized_reads += 1;
            if state.uninitialized.len() < state.uninit_cap {
                state.uninitialized.push(UninitializedRead {
                    func: event.func,
                    local: event.local,
                    frame_depth: event.frame_depth,
                    pc: event.pc,
                    dispatch: event.dispatch,
                });
            } else {
                state.overflowed = true;
            }
        }
    }

    /// The per-slot counters, sorted by `(function, frame depth, slot)` —
    /// the deterministic order the render lists them in.
    #[must_use]
    pub fn slot_stats(&self) -> Vec<((usize, usize, usize), SlotFlow)> {
        let state = lock_fold(&self.inner);
        let mut slots: Vec<((usize, usize, usize), SlotFlow)> = state
            .slots
            .iter()
            .map(|(key, flow)| (*key, flow.clone()))
            .collect();
        slots.sort_unstable_by_key(|(key, _)| *key);
        slots
    }

    /// The displayed uninitialized reads, in dispatch order (capped — see
    /// [`DataFlowCollector::uninitialized_read_total`] for the exact
    /// count).
    #[must_use]
    pub fn uninitialized_reads(&self) -> Vec<UninitializedRead> {
        lock_fold(&self.inner).uninitialized.clone()
    }

    /// How many uninitialized reads the run made, exactly (independent of
    /// the displayed list's cap).
    #[must_use]
    pub fn uninitialized_read_total(&self) -> u64 {
        lock_fold(&self.inner)
            .slots
            .values()
            .map(|slot| slot.uninitialized_reads)
            .sum()
    }

    /// Total writes folded (summed over slots).
    #[must_use]
    pub fn total_writes(&self) -> u64 {
        lock_fold(&self.inner)
            .slots
            .values()
            .map(|slot| slot.writes)
            .sum()
    }

    /// Total reads folded (summed over slots).
    #[must_use]
    pub fn total_reads(&self) -> u64 {
        lock_fold(&self.inner)
            .slots
            .values()
            .map(|slot| slot.reads)
            .sum()
    }

    /// Whether any uninitialized read was dropped from the displayed list
    /// because the cap was reached (the total is unaffected).
    #[must_use]
    pub fn overflowed(&self) -> bool {
        lock_fold(&self.inner).overflowed
    }
}

impl WatchSink for DataFlowCollector {
    fn write(&mut self, event: &WatchEvent) {
        self.record_write(event);
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl ReadSink for DataFlowCollector {
    fn read(&mut self, event: &ReadEvent) {
        self.record_read(event);
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// The seam's transcript event: one entry of the interleaved write/read
/// order the two hooks produce on a single run.
#[derive(Debug, Clone, PartialEq)]
pub enum FlowEvent {
    /// A write from the watch hook (the watch seam's event, unchanged).
    Write(WatchEvent),
    /// A read from the data-flow hook.
    Read(ReadEvent),
}

/// A sink pair that records the interleaved transcript in arrival order.
///
/// The two hooks take the sink by value (`Box`), so the recorder is a
/// shared-state handle: `clone()` shares one transcript, and that is how a
/// host attaches one recorder to both hooks ([`Vm::with_watch`] and
/// [`Vm::with_reads`]) to capture the full interleaving. `FlowRecorder::new()`
/// is unbounded; `FlowRecorder::bounded(max)` stops capturing at the cap
/// and sets the `overflowed` flag instead of growing without limit — an
/// infinite loop reading a slot per dispatch must not OOM the embedding
/// host. The flag is a deliberate, *displayed* fact, never a silent drop.
#[derive(Debug, Clone, Default)]
pub struct FlowRecorder {
    inner: std::sync::Arc<Mutex<RecorderState>>,
}

#[derive(Debug, Default)]
struct RecorderState {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in arrival order.
    events: Vec<FlowEvent>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl FlowRecorder {
    /// Create an unbounded recorder (the whole interleaved transcript is
    /// captured).
    #[must_use]
    pub fn new() -> FlowRecorder {
        FlowRecorder::default()
    }

    /// Create a recorder that captures at most `max_events` entries and
    /// marks the overflow instead of growing without limit.
    #[must_use]
    pub fn bounded(max_events: usize) -> FlowRecorder {
        FlowRecorder {
            inner: std::sync::Arc::new(Mutex::new(RecorderState {
                cap: Some(max_events),
                events: Vec::new(),
                overflowed: false,
            })),
        }
    }

    /// The captured transcript snapshot, in arrival order (a bounded
    /// recorder holds at most its cap).
    #[must_use]
    pub fn transcript(&self) -> Vec<FlowEvent> {
        lock_fold(&self.inner).events.clone()
    }

    /// Consume the handle into the captured transcript.
    #[must_use]
    pub fn into_transcript(self) -> Vec<FlowEvent> {
        lock_fold(&self.inner).events.clone()
    }

    /// How many events were captured.
    #[must_use]
    pub fn len(&self) -> usize {
        lock_fold(&self.inner).events.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        lock_fold(&self.inner).events.is_empty()
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        lock_fold(&self.inner).overflowed
    }

    fn push(&self, event: FlowEvent) {
        let mut state = lock_fold(&self.inner);
        if let Some(cap) = state.cap {
            if state.events.len() >= cap {
                state.overflowed = true;
                return;
            }
        }
        state.events.push(event);
    }
}

impl WatchSink for FlowRecorder {
    fn write(&mut self, event: &WatchEvent) {
        self.push(FlowEvent::Write(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl ReadSink for FlowRecorder {
    fn read(&mut self, event: &ReadEvent) {
        self.push(FlowEvent::Read(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// A read sink that emits one `tracing::debug!` event per local read — the
/// seam's counterpart for the workspace-standard observability layer (the
/// same `tracing` dependency the other seams log through). Enable it
/// per-subscriber, never by paying the cost unconditionally.
pub struct TracingReadSink;

impl ReadSink for TracingReadSink {
    fn read(&mut self, event: &ReadEvent) {
        debug!(
            func = event.func,
            local = event.local,
            pc = event.pc,
            dispatch = event.dispatch,
            frame_depth = event.frame_depth,
            "vm local read"
        );
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a [`DataFlowCollector`] fold into the deterministic
/// diagnostic-text style the other observability seams use.
///
/// The empty state spells an explicit `no data-flow events recorded` line
/// (never an empty string); the slot lines sort by `(function, frame
/// depth, slot)`; a slot with reads but no writes spells its
/// `uninitialized` count; and the exact uninitialized totals ride the
/// header — two renders of the same fold are byte-identical, which is the
/// contract the regression gates over rendered listings rely on.
#[must_use]
pub fn render_data_flow(collector: &DataFlowCollector) -> String {
    let slots = collector.slot_stats();
    if slots.is_empty() {
        return "no data-flow events recorded\n".to_string();
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "data-flow report: writes={} reads={} slots={} uninitialized-reads={}",
        collector.total_writes(),
        collector.total_reads(),
        slots.len(),
        collector.uninitialized_read_total(),
    );
    for ((func, depth, local), slot) in slots {
        let _ = write!(
            out,
            "  fn {func} depth {depth} slot {local}: writes {}, reads {}",
            slot.writes(),
            slot.reads()
        );
        if let Some(def) = slot.last_write_dispatch() {
            let _ = write!(out, ", last def d{def}");
        }
        if slot.uninitialized_reads() > 0 {
            let _ = write!(out, ", uninitialized {}", slot.uninitialized_reads());
        }
        let _ = writeln!(out);
    }
    for uninit in collector.uninitialized_reads() {
        let _ = writeln!(
            out,
            "  uninitialized: fn {} depth {} slot {} pc {} dispatch {}",
            uninit.func, uninit.frame_depth, uninit.local, uninit.pc, uninit.dispatch,
        );
    }
    if collector.overflowed() {
        let _ = writeln!(
            out,
            "  uninitialized shown: {} of {} (capped)",
            collector.uninitialized_reads().len(),
            collector.uninitialized_read_total(),
        );
    }
    out
}

#[cfg(test)]
mod dataflow_tests {
    use crate::dataflow::{DataFlowCollector, FlowEvent, FlowRecorder, ReadEvent};
    use crate::trace::TraceCollector;
    use crate::watch::{WatchEvent, WatchVia};
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

    /// helper(n) = n * 2 + main returning helper(21): the watch seam's
    /// golden module. Its data-flow stream is the ABI pair: the call binds
    /// the argument into the callee's local 1 (dispatch 3, depth 2), the
    /// callee reads it (dispatch 5), the return lands the product in the
    /// caller's dest local 0 (dispatch 7, depth 1), and the caller reads it
    /// (dispatch 8).
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

    /// A countdown loop over local 0: the store-and-read-heavy shape — one
    /// seed store, then per iteration a read-compute-store and a
    /// read-test-jump, with a final read for the return value.
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

    /// Run a module with a data-flow collector attached to both hooks; the
    /// value must match the unattached result exactly (the seam is
    /// read-only) and the typed collector must come back through
    /// `take_reads`.
    fn paired_run(module: &Module) -> (DataFlowCollector, Value) {
        let collector = DataFlowCollector::new();
        let mut vm = Vm::new()
            .with_watch(Box::new(collector.clone()))
            .with_reads(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_reads().expect("read sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<DataFlowCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// The countdown run's reads are total and in order: one per executed
    /// `OP_LOAD_LOCAL`, carrying the value the slot held, the read site's
    /// dispatch number, and the frame depth.
    #[test]
    fn load_local_reads_are_recorded_in_order() {
        let module = countdown_module(2);
        let mut vm = Vm::new().with_reads(Box::new(FlowRecorder::new()));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_reads().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let recorder = sink
            .downcast_mut::<FlowRecorder>()
            .expect("the recorder came back");
        let reads: Vec<ReadEvent> = recorder
            .transcript()
            .iter()
            .cloned()
            .filter_map(|event| match event {
                FlowEvent::Read(read) => Some(read),
                FlowEvent::Write(_) => None,
            })
            .collect();
        let descs: Vec<(usize, i64, u64)> = reads
            .iter()
            .map(|e| {
                (
                    e.local,
                    match e.value {
                        Value::Int(n) => n,
                        _ => panic!("int reads only"),
                    },
                    e.dispatch,
                )
            })
            .collect();
        assert_eq!(
            descs,
            vec![
                (0, 2, 3),  // loop head: the seed
                (0, 1, 7),  // the loop test after the first store
                (0, 1, 11), // the second iteration's test (slot still 1)
                (0, 0, 15), // the test after the second store
                (0, 0, 19), // the return read
            ],
            "every OP_LOAD_LOCAL, in dispatch order, with the slot's value"
        );
        assert!(reads.iter().all(|e| e.func == 0 && e.frame_depth == 1));
    }

    /// The fold pairs each read with the dispatch that last wrote the slot:
    /// the countdown's five reads resolve to their three writes, and a
    /// slot's `last def` is the newest write before the newest read.
    #[test]
    fn reads_pair_with_the_last_write() {
        let module = countdown_module(2);
        let (collector, value) = paired_run(&module);
        assert_eq!(value, Value::Int(0));
        let slots = collector.slot_stats();
        assert_eq!(slots.len(), 1, "one slot touched");
        let slot = &slots[0].1;
        assert_eq!(slot.writes(), 3, "seed + two loop stores");
        assert_eq!(slot.reads(), 5, "two per iteration + the return read");
        assert_eq!(slot.last_write_dispatch(), Some(14), "the final store");
        assert_eq!(slot.uninitialized_reads(), 0);
        assert_eq!(collector.total_writes(), 3);
        assert_eq!(collector.total_reads(), 5);
        assert_eq!(collector.uninitialized_read_total(), 0);
        assert!(!collector.overflowed());
    }

    /// A read whose slot was never written in its frame is reported
    /// through `uninitialized_reads` — the zero-init consumption is a
    /// displayed fact, never a silent one.
    #[test]
    fn uninitialized_reads_are_displayed() {
        // Read slot 0 twice without ever storing it, then return one.
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let (collector, value) = paired_run(&module);
        assert_eq!(value, Value::Int(0), "the zero-init value is returned");
        assert_eq!(collector.total_reads(), 2);
        assert_eq!(collector.total_writes(), 0);
        assert_eq!(collector.uninitialized_read_total(), 2);
        let uninit = collector.uninitialized_reads();
        assert_eq!(uninit.len(), 2);
        assert_eq!(uninit[0].dispatch, 1, "the first load");
        assert_eq!(uninit[0].local, 0);
        assert_eq!(uninit[0].frame_depth, 1);
        assert_eq!(uninit[1].dispatch, 2);
        // The slot line says so too.
        let rendered = crate::dataflow::render_data_flow(&collector);
        assert!(
            rendered.contains("uninitialized 2"),
            "slot line spells the uninit count: {rendered}"
        );
    }

    /// The bounded uninitialized list marks overflow instead of growing
    /// without limit, and the total stays exact.
    #[test]
    fn bounded_uninitialized_list_marks_overflow() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let collector = DataFlowCollector::with_uninitialized_cap(1);
        let mut vm = Vm::new()
            .with_watch(Box::new(collector.clone()))
            .with_reads(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_reads().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<DataFlowCollector>()
            .expect("the collector came back");
        assert_eq!(
            collector.uninitialized_read_total(),
            2,
            "the total is exact"
        );
        assert_eq!(
            collector.uninitialized_reads().len(),
            1,
            "the list stops at the cap"
        );
        assert!(collector.overflowed());
    }

    /// The golden module's fold: the two ABI writes pair with the two ABI
    /// reads, keyed by function, frame depth, and slot.
    #[test]
    fn golden_abi_writes_pair_with_the_abi_reads() {
        let module = golden_module();
        let (collector, value) = paired_run(&module);
        assert_eq!(value, Value::Int(42));
        assert_eq!(collector.total_writes(), 2, "arg bind + callee return");
        assert_eq!(collector.total_reads(), 2, "callee read + caller read");
        assert_eq!(collector.uninitialized_read_total(), 0);
        let slots = collector.slot_stats();
        assert_eq!(slots.len(), 2);
        // (func, frame depth, local) sorted: (0, 2, 1) then (1, 1, 0).
        assert_eq!(slots[0].0, (0, 2, 1));
        assert_eq!(slots[0].1.writes(), 1);
        assert_eq!(slots[0].1.reads(), 1);
        assert_eq!(slots[0].1.last_write_dispatch(), Some(3), "the arg bind");
        assert_eq!(slots[1].0, (1, 1, 0));
        assert_eq!(slots[1].1.writes(), 1);
        assert_eq!(slots[1].1.reads(), 1);
        assert_eq!(slots[1].1.last_write_dispatch(), Some(7), "the return");
    }

    /// The transcript replay contract: a `FlowRecorder` attached to both
    /// hooks captures the interleaved write/read order, and
    /// `from_transcript` reproduces the incremental fold exactly.
    #[test]
    fn from_transcript_refold_matches_incremental_fold() {
        let module = countdown_module(2);
        let recorder = FlowRecorder::new();
        let mut vm = Vm::new()
            .with_watch(Box::new(recorder.clone()))
            .with_reads(Box::new(recorder));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_reads().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let recorder = sink
            .downcast_ref::<FlowRecorder>()
            .expect("the recorder came back");
        let transcript = recorder.transcript();
        // The interleaving is the run's real order: the seed write, the
        // loop-head read, the store, the test read, ... the return read.
        let kinds: Vec<&str> = transcript
            .iter()
            .map(|e| match e {
                FlowEvent::Write(_) => "write",
                FlowEvent::Read(_) => "read",
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "write", "read", "write", "read", "read", "write", "read", "read",
            ],
            "seed, head-read, store, test-read, jump-read, store, test-read, return-read"
        );
        let incremental = {
            let collector = DataFlowCollector::new();
            for event in &transcript {
                match event {
                    FlowEvent::Write(w) => collector.record_write(w),
                    FlowEvent::Read(r) => collector.record_read(r),
                }
            }
            collector
        };
        let refolded = DataFlowCollector::from_transcript(transcript);
        assert_eq!(refolded, incremental, "re-fold must reproduce the fold");
    }

    /// The render is deterministic, sorts its slot lines, spells the empty
    /// state explicitly, and pins the countdown listing.
    #[test]
    fn render_is_deterministic_and_pins_the_listing() {
        let empty = crate::dataflow::render_data_flow(&DataFlowCollector::new());
        assert_eq!(empty, "no data-flow events recorded\n");

        let module = countdown_module(2);
        let (collector, _) = paired_run(&module);
        let first = crate::dataflow::render_data_flow(&collector);
        let second = crate::dataflow::render_data_flow(&collector);
        assert_eq!(first, second, "two renders must be byte-identical");
        assert_eq!(
            first,
            "\
data-flow report: writes=3 reads=5 slots=1 uninitialized-reads=0
  fn 0 depth 1 slot 0: writes 3, reads 5, last def d14
"
        );
    }

    /// The data-flow seam is read-only: the paired result equals the
    /// unpaired result, and attaching the sinks does not perturb the run.
    #[test]
    fn paired_result_matches_unpaired() {
        let module = golden_module();
        let unpaired = Vm::new().run_module(&module).expect("unpaired runs");
        let (collector, paired) = paired_run(&module);
        assert_eq!(paired, unpaired);
        assert_eq!(collector.total_reads(), 2);
    }

    /// A VM without a read sink behaves exactly as before: no events, no
    /// panic, and `take_reads` hands back nothing.
    #[test]
    fn no_read_sink_by_default() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_reads().is_none());
        assert!(vm.take_watch().is_none());
    }

    /// A fuel-refused dispatch never executes and never reads: the read
    /// stream ends exactly at the reads the executed dispatches produced.
    /// The countdown's dispatch 3 (the head read) is the budget's last
    /// *allowed* dispatch — it executes and reads — and dispatch 4 is
    /// refused.
    #[test]
    fn fuel_refused_dispatch_reads_nothing() {
        let module = countdown_module(10);
        let mut vm = Vm::new()
            .with_fuel(3)
            .with_reads(Box::new(FlowRecorder::new()));
        let err = vm
            .run_module(&module)
            .expect_err("the loop burns the budget");
        assert_eq!(err, VmError::FuelExhausted { executed: 3 });
        let mut boxed = vm.take_reads().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let recorder = sink
            .downcast_mut::<FlowRecorder>()
            .expect("the recorder came back");
        let reads = recorder
            .transcript()
            .iter()
            .filter(|e| matches!(e, FlowEvent::Read(_)))
            .count();
        assert_eq!(
            reads, 1,
            "the head read (dispatch 3) ran; the refused dispatch read nothing"
        );
    }

    /// The seam composes with the trace seam on one run: separate hooks,
    /// both sinks come back typed, and the step count is unchanged.
    #[test]
    fn dataflow_composes_with_trace() {
        let module = golden_module();
        let trace = TraceCollector::new();
        let mut vm = Vm::new()
            .with_trace(Box::new(trace))
            .with_reads(Box::new(FlowRecorder::new()));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut traced = vm.take_trace().expect("trace attached");
        let steps = traced
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("trace collector")
            .len();
        assert_eq!(steps, 9, "the golden module dispatches 9 steps");
        let mut recorder_box = vm.take_reads().expect("reads attached");
        let events = recorder_box
            .as_any()
            .downcast_ref::<FlowRecorder>()
            .expect("flow recorder")
            .transcript();
        assert_eq!(events.len(), 2, "two reads over the decoded run");
    }

    /// The image round trip composes with the seam: decode the shipped
    /// bytes, then read-watch the loaded module.
    #[test]
    fn dataflow_composes_with_module_image() {
        let module = golden_module();
        let bytes = crate::encode_module(&module);
        let loaded = crate::decode_module(&bytes).expect("decode");
        let (collector, value) = paired_run(&loaded);
        assert_eq!(value, Value::Int(42));
        assert_eq!(collector.total_reads(), 2, "the ABI reads over the image");
    }

    /// A read that faults — a `LocalOutOfBounds` load — reads nothing and
    /// reports nothing: the seam is total over the *successful* reads.
    #[test]
    fn failed_read_reports_nothing() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(99));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let mut vm = Vm::new().with_reads(Box::new(FlowRecorder::new()));
        let err = vm
            .run_module(&module)
            .expect_err("local 99 is out of bounds");
        assert_eq!(err, VmError::LocalOutOfBounds(99));
        let mut boxed = vm.take_reads().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let recorder = sink
            .downcast_mut::<FlowRecorder>()
            .expect("the recorder came back");
        assert!(
            recorder.transcript().is_empty(),
            "no event for the failed read"
        );
    }

    /// The watch event spelled through the fold: `record_write` alone
    /// (no read stream attached) still folds the write side, so a
    /// watch-only host gets the write half of the story.
    #[test]
    fn write_only_fold_still_counts_writes() {
        let collector = DataFlowCollector::new();
        collector.record_write(&WatchEvent {
            func: 0,
            local: 1,
            value: Value::Int(21),
            pc: 18,
            dispatch: 3,
            frame_depth: 2,
            via: WatchVia::ArgBind,
        });
        assert_eq!(collector.total_writes(), 1);
        assert_eq!(collector.total_reads(), 0);
        let slots = collector.slot_stats();
        assert_eq!(slots[0].0, (0, 2, 1));
        assert_eq!(slots[0].1.last_write_dispatch(), Some(3));
    }
}
