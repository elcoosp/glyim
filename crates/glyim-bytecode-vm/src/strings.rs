//! String-pool observation seam: which pooled string constants did this
//! run actually materialize, and from which sites?
//!
//! The module-image diff seam ([`crate::diff`]) names string-pool changes
//! *structurally* — the `-`/`+` pool lines of a rebuilt artifact — but the
//! pool's *runtime* face is invisible: which slots did this run resolve,
//! from which dispatch sites, how many times was one shared slot
//! re-resolved, and how many bytes did the run materialize? Nothing
//! answered that — and the pool is exactly where a regression class
//! lives: an opaque-index constant (the historical interner-index bug the
//! pooled-string work replaced) or a content-equality break shows up as
//! the wrong slot, or the same slot behaving differently across sites.
//! This module is that counterpart: a hook in the `OP_LOAD_CONST_STR`
//! arm — the VM fires [`StringSink::string_loaded`] just after the pool
//! slot's byte string has been resolved and just before the value is
//! pushed — handing one event per executed pool resolution to a sink the
//! caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the pushed value is exactly the pre-seam resolution), **total**
//! (every *executed* resolution reports — a bad pool index faults and
//! resolves nothing, so it reports nothing: the fault stream owns the
//! refusal), **bounded** (the collector's memory is capped and flags
//! overflow instead of growing without limit over a string-heavy loop),
//! and **deterministic** ([`render_strings`] renders the same collector
//! byte-identically — the test suite pins a golden).
//!
//! The hook fires only when a sink is attached: one `Option` check per
//! executed resolution, exactly the pre-seam behavior when the sink is
//! absent. The event's value is a sink-side copy (the copies-into-the-
//! event shape every value-carrying seam follows) — the machine's own
//! value stays exactly where it was.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use tracing::debug;

/// One executed string-pool resolution, handed to the attached
/// [`StringSink`] just after the pool slot's byte string has been
/// resolved and just before the value is pushed (a bad pool index faults
/// and never reaches the hook — the fault stream owns it): the site, the
/// pool slot, and the resolved value in full.
#[derive(Clone, Debug, PartialEq)]
pub struct StringLoadEvent {
    /// The function index containing the resolution site.
    pub func: usize,
    /// The byte offset of the `OP_LOAD_CONST_STR` opcode (the
    /// resolution site).
    pub pc: usize,
    /// The pool slot the opcode resolved.
    pub pool_idx: usize,
    /// The resolved byte string (a sink-side copy — the machine's own
    /// copy is pushed onto the operand stack right after the hook
    /// returns).
    pub value: crate::Value,
}

/// A sink that observes every executed string-pool resolution: a
/// [`StringSink`] the VM hands one [`StringLoadEvent`] per executed
/// `OP_LOAD_CONST_STR`. Attach through [`crate::Vm::with_strings`],
/// recover through [`crate::Vm::take_strings`] + [`StringSink::as_any`].
pub trait StringSink: 'static {
    /// One pool slot resolved: the byte string has been fetched and is
    /// about to be pushed (a bad index never reaches the hook — the
    /// fault stream owns it).
    fn string_loaded(&mut self, event: &StringLoadEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_strings`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed string-pool resolution in memory.
///
/// `StringCollector::new()` is unbounded; `StringCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit over a string-heavy loop — one event per
/// resolution must not OOM the embedding host. The flag is a deliberate,
/// *displayed* fact, never a silent drop.
///
/// The collector also folds as it captures: how many times each pool
/// slot was resolved ([`StringCollector::resolutions`]), how many
/// distinct slots the run touched ([`StringCollector::slot_count`]), and
/// how many bytes the captured resolutions materialized
/// ([`StringCollector::total_bytes`]) — the shared-slot audit the
/// content-equality regression class surfaces through.
#[derive(Clone, Default)]
pub struct StringCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<StringLoadEvent>,
    /// Resolutions per pool slot (the fold's per-slot counts).
    slots: BTreeMap<usize, usize>,
    /// Total byte length the captured resolutions materialized.
    bytes: usize,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl StringCollector {
    /// Create an unbounded collector (every executed resolution is
    /// captured).
    #[must_use]
    pub fn new() -> StringCollector {
        StringCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> StringCollector {
        StringCollector {
            cap: Some(max),
            events: Vec::new(),
            slots: BTreeMap::new(),
            bytes: 0,
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order.
    #[must_use]
    pub fn events(&self) -> &[StringLoadEvent] {
        &self.events
    }

    /// How many times the slot was resolved by the captured loads (0 for
    /// a slot the run never touched).
    #[must_use]
    pub fn resolutions(&self, pool_idx: usize) -> usize {
        self.slots.get(&pool_idx).copied().unwrap_or(0)
    }

    /// How many distinct pool slots the captured loads resolved — the
    /// shared-slot audit's denominator.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// The byte length the captured resolutions materialized in total
    /// (every event's value, summed — a loop re-resolving one slot pays
    /// its bytes each iteration).
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.bytes
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no resolution was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl StringSink for StringCollector {
    fn string_loaded(&mut self, event: &StringLoadEvent) {
        self.push(event.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl StringCollector {
    /// Append one event under the capture cap (an overflow drops the
    /// event and sets the flag — displayed, never a silent drop),
    /// updating the per-slot and byte folds as it goes.
    fn push(&mut self, event: StringLoadEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        let len = match &event.value {
            crate::Value::Str(s) => s.len(),
            _ => 0,
        };
        *self.slots.entry(event.pool_idx).or_insert(0) += 1;
        self.bytes += len;
        self.events.push(event);
    }
}

/// Render a string stream into a deterministic listing: a header line
/// (image format version, the load count, the distinct-slot count, and
/// the materialized byte total), then one line per load in execution
/// order — the site's function index and opcode byte offset, the pool
/// slot, and the resolved value in the machine's value spellings — then
/// the per-slot summary (one `resolved N` line per touched slot in slot
/// order). A run with no resolutions renders the explicit
/// `no string loads` line (honest about emptiness, like the memory
/// stream's `no memory events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_strings(collector: &StringCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "strings glyim.module-image/{} loads={} slots={} bytes={}",
        crate::IMAGE_VERSION,
        collector.events.len(),
        collector.slot_count(),
        collector.bytes,
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no string loads");
        debug!(events = 0, "vm string stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        let _ = writeln!(
            out,
            "{i}: load fn {} pc {} slot {} value={}",
            event.func,
            event.pc,
            event.pool_idx,
            crate::watch::render_value(&event.value),
        );
    }
    let _ = writeln!(out, "slots:");
    for (slot, count) in &collector.slots {
        let _ = writeln!(out, "{slot}: resolved {count}x");
    }
    debug!(events = collector.events.len(), "vm string stream rendered");
    out
}

#[cfg(test)]
mod strings_tests {
    use super::{StringCollector, render_strings};
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

    /// main (fn 0) loads slot 0 (`"hello"`) and slot 1 (`"world"`) and
    /// returns the second: the canonical two-slot module (two events,
    /// two distinct slots).
    fn two_slot_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstStr)); // pc 0 — "hello"
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConstStr)); // pc 5 — "world"
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Return)); // pc 10 — pops "world"
        Module::with_strings(
            vec![Function::new(main, 0, 0)],
            vec!["hello".into(), "world".into()],
            0,
        )
    }

    /// main (fn 0) loads slot 0 in a countdown loop (`count` times):
    /// the shared-slot shape — one slot, many resolutions.
    fn shared_slot_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): load slot 0, drop it, decrement, loop while
        // still positive.
        main.push(op(Opcode::LoadConstStr)); // pc 14 — one load per iteration
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Drop)); // pc 19 — discard the string
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
        main.push(op(Opcode::LoadConstStr)); // pc 60 — the module result
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 65
        Module::with_strings(vec![Function::new(main, 1, 0)], vec!["payload".into()], 0)
    }

    /// main (fn 0) loads pool slot 9 of a one-slot pool: a bad index —
    /// the resolution faults, the run faults, and the seam must report
    /// nothing (the fault stream owns the refusal).
    fn bad_index_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstStr)); // pc 0 — bad index
        main.extend_from_slice(&u32le(9));
        main.push(op(Opcode::Return)); // never reached
        Module::with_strings(vec![Function::new(main, 0, 0)], vec!["only".into()], 0)
    }

    /// main (fn 0) loads slot 0 (`"hello"`), drops it, loads slot 1
    /// (`"hello"` again — same content, different slot), and returns it:
    /// the render golden's module — content equality is by bytes, and
    /// the two slots' events name their own sites.
    fn mixed_slots_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstStr)); // pc 0 — slot 0
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Drop)); // pc 5
        main.push(op(Opcode::LoadConstStr)); // pc 6 — slot 1
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Return)); // pc 11
        Module::with_strings(
            vec![Function::new(main, 0, 0)],
            vec!["hello".into(), "hello".into()],
            0,
        )
    }

    /// Run a module with a string collector attached; the run's outcome
    /// and the typed collector both come back (the collector through
    /// `take_strings` + `as_any`).
    fn traced_run(module: &Module) -> (StringCollector, Result<Value, crate::VmError>) {
        let collector = StringCollector::new();
        let mut vm = Vm::new().with_strings(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_strings().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<StringCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// Two executed resolutions report exactly two events — the site,
    /// the pool slot, and the resolved value in full — and the folds
    /// count two distinct slots.
    #[test]
    fn two_loads_report_both_events() {
        let module = two_slot_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Str("world".into()));
        assert_eq!(collector.events().len(), 2);
        assert_eq!(collector.slot_count(), 2);
        let first = &collector.events()[0];
        assert_eq!(first.func, 0);
        assert_eq!(first.pc, 0, "the OP_LOAD_CONST_STR opcode's byte offset");
        assert_eq!(first.pool_idx, 0);
        assert_eq!(first.value, Value::Str("hello".into()));
        let second = &collector.events()[1];
        assert_eq!(second.pc, 5, "the second resolution's byte offset");
        assert_eq!(second.pool_idx, 1);
        assert_eq!(second.value, Value::Str("world".into()));
        assert_eq!(collector.resolutions(0), 1);
        assert_eq!(collector.resolutions(1), 1);
        assert_eq!(collector.resolutions(2), 0, "a slot the run never touched");
        assert_eq!(collector.total_bytes(), 10, "5 + 5 bytes");
    }

    /// A loop re-resolving one shared slot reports one event per
    /// iteration — the per-slot fold counts them all, and the byte total
    /// counts each iteration's copy.
    #[test]
    fn shared_slot_loop_counts_every_resolution() {
        let module = shared_slot_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Str("payload".into()));
        assert_eq!(collector.events().len(), 4, "three loop loads + the result");
        assert_eq!(collector.slot_count(), 1, "one shared slot");
        assert_eq!(collector.resolutions(0), 4);
        assert_eq!(collector.total_bytes(), 4 * 7, "7 bytes per resolution");
        // Every event names the same pair of sites (pc 14, pc 60).
        let sites: Vec<usize> = collector.events().iter().map(|e| e.pc).collect();
        assert_eq!(sites, vec![14, 14, 14, 60]);
    }

    /// The render is deterministic and pins the transcript line spellings:
    /// the load lines with the machine's value spellings and the per-slot
    /// summary in slot order.
    #[test]
    fn render_is_deterministic_golden() {
        let module = mixed_slots_module();
        let (collector, _) = traced_run(&module);
        let first = render_strings(&collector);
        let second = render_strings(&collector);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
strings glyim.module-image/2 loads=2 slots=2 bytes=10
0: load fn 0 pc 0 slot 0 value=\"hello\"
1: load fn 0 pc 6 slot 1 value=\"hello\"
slots:
0: resolved 1x
1: resolved 1x
"
        );
    }

    /// A resolution refused by the pool bounds check reports nothing —
    /// the fault stream owns the refusal — and the run faults with
    /// exactly the pre-seam error.
    #[test]
    fn a_bad_index_reports_nothing() {
        let module = bad_index_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(
            outcome.expect_err("the bad index faults"),
            crate::VmError::StringIndexOutOfBounds(9)
        );
        assert!(collector.is_empty(), "no event for a refused resolution");
        assert_eq!(collector.slot_count(), 0);
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = shared_slot_loop_module(3);
        let collector = StringCollector::bounded(2);
        let mut vm = Vm::new().with_strings(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_strings().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<StringCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 2, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
        assert_eq!(
            collector.slot_count(),
            1,
            "the folds cover the captured only"
        );
        assert_eq!(collector.total_bytes(), 2 * 7);
    }

    /// The seam is read-only: the traced run's value equals the untraced
    /// one exactly, and the faulting run's error equals the untraced one
    /// exactly.
    #[test]
    fn the_seam_is_read_only() {
        let paired = two_slot_module();
        let untraced = Vm::new().run_module(&paired).expect("untraced run");
        let (_, traced) = traced_run(&paired);
        assert_eq!(traced.expect("traced run"), untraced);
        let faulting = bad_index_module();
        let untraced = Vm::new()
            .run_module(&faulting)
            .expect_err("untraced faulting run");
        let (_, traced) = traced_run(&faulting);
        assert_eq!(traced.expect_err("traced faulting run"), untraced);
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_string_loads() {
        assert_eq!(
            render_strings(&StringCollector::new()),
            "\
strings glyim.module-image/2 loads=0 slots=0 bytes=0
no string loads
"
        );
    }

    /// The event type is comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let e = super::StringLoadEvent {
            func: 0,
            pc: 0,
            pool_idx: 0,
            value: Value::Str("hello".into()),
        };
        let mut e2 = e.clone();
        assert_eq!(e, e2);
        e2.pool_idx = 1;
        assert_ne!(e, e2);
        assert_ne!(
            e.value,
            Value::Str("world".into()),
            "the sink's copy keeps its own content"
        );
    }

    /// A run with no `OP_LOAD_CONST_STR` at all reports nothing: the
    /// seam is total over the executed resolution stream, loads or not.
    #[test]
    fn a_string_free_run_reports_nothing() {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(5));
        main.push(op(Opcode::Return)); // pc 9
        let module = Module::new(vec![Function::new(main, 0, 0)], 0);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(5));
        assert!(collector.is_empty());
    }
}
