//! Cast-observation seam: what did every cast the run executed actually do
//! to the value?
//!
//! The execution-trace seam ([`crate::trace`]) says *which* instructions
//! dispatched, and its step detail names an `OP_CAST`'s kind and target
//! bytes — but the trace's one-line entries fold the values away. Nothing
//! answered "which casts in this run changed the value, and from what to
//! what" — the classic silent-lossy-cast audit: `(-1i32) as u8 == 255`,
//! `(3.99) as i32 == 3`. The pre-seam VM made that audit impossible in
//! exactly one direction: `OP_CAST` was a no-op (every float and narrowing
//! cast silently kept its operand), and the fix made the truncation real —
//! but the run's *result* still only shows the last cast's output, not the
//! per-cast traffic. This module is that counterpart: a hook in the
//! `OP_CAST` arm — the VM fires [`CastSink::cast_evaluated`] the moment a
//! cast has produced its result, just before the result is pushed —
//! handing one [`CastEvent`] per executed cast to a sink the caller
//! attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change a cast's outcome — the pushed result is exactly the
//! pre-seam value), **total** (every *executed* cast reports — a cast that
//! faults with [`crate::VmError::UnknownCastKind`] or
//! [`crate::VmError::CastOperandMismatch`] never executes, and the fault
//! stream owns it), **bounded** (the collector's memory is capped and flags
//! overflow instead of growing without limit over a cast-heavy loop), and
//! **deterministic** ([`render_casts`] renders the same event list
//! byte-identically — the test suite pins a golden).
//!
//! The hook fires only when a sink is attached: one `Option` check per
//! executed `OP_CAST`, exactly the pre-seam behavior when the sink is
//! absent. The event carries the cast's site (function index and the
//! opcode's byte offset), the decoded kind and target, and sink-side
//! copies of the operand and the result — the machine's own values stay
//! exactly where they were (the read hook's copies-into-the-event shape).

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// The seam's spelling of the emitter's cast-kind byte (the same numbering
/// `cast_value` matches on, which mirrors `glyim_mir::CastKind`). The VM
/// crate carries no dependency on the type crates, so the wire byte is
/// decoded here into a named kind instead of travelling raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CastKind {
    /// `IntToInt`: a real truncation to the target width/signedness
    /// (`(-1i32) as u8 == 255`).
    IntToInt = 0,
    /// `FloatToInt`: truncate the float toward zero into the target integer
    /// (`(3.99) as i32 == 3`).
    FloatToInt = 1,
    /// `IntToFloat`: widen the integer into an `f64`.
    IntToFloat = 2,
    /// `PtrToPtr`: identity at the VM's value level (addresses are `Int`s).
    PtrToPtr = 3,
    /// `FnPtrToPtr`: identity at the VM's value level.
    FnPtrToPtr = 4,
    /// `PtrToInt`: identity at the VM's value level.
    PtrToInt = 5,
    /// `IntToPtr`: identity at the VM's value level.
    IntToPtr = 6,
    /// `FloatToFloat`: the `f64`-level identity (the VM's floats are
    /// `f64`, so an `f32` target does not narrow at the value level).
    FloatToFloat = 7,
}

impl CastKind {
    /// Decode a cast-kind byte, or `None` if the byte is not a
    /// [`CastKind`] (the arm's [`VmError::UnknownCastKind`] rejection).
    #[must_use]
    pub fn from_u8(b: u8) -> Option<CastKind> {
        Some(match b {
            0 => CastKind::IntToInt,
            1 => CastKind::FloatToInt,
            2 => CastKind::IntToFloat,
            3 => CastKind::PtrToPtr,
            4 => CastKind::FnPtrToPtr,
            5 => CastKind::PtrToInt,
            6 => CastKind::IntToPtr,
            7 => CastKind::FloatToFloat,
            _ => return None,
        })
    }

    /// The kind's name, spelled the way the typed cast-mismatch errors
    /// spell it (`FloatToInt`, not a debug dump).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            CastKind::IntToInt => "IntToInt",
            CastKind::FloatToInt => "FloatToInt",
            CastKind::IntToFloat => "IntToFloat",
            CastKind::PtrToPtr => "PtrToPtr",
            CastKind::FnPtrToPtr => "FnPtrToPtr",
            CastKind::PtrToInt => "PtrToInt",
            CastKind::IntToPtr => "IntToPtr",
            CastKind::FloatToFloat => "FloatToFloat",
        }
    }
}

/// One cast-execution event, handed to the attached [`CastSink`] the
/// moment the cast has produced its result: the site, the decoded kind
/// and target, and the operand/result pair in full.
#[derive(Clone, Debug, PartialEq)]
pub struct CastEvent {
    /// The function index containing the cast site.
    pub func: usize,
    /// The byte offset of the `OP_CAST` opcode (the cast site).
    pub pc: usize,
    /// The decoded cast kind (the wire byte's spelling).
    pub kind: CastKind,
    /// The decoded target primitive the cast converts to.
    pub target: crate::CastTarget,
    /// The operand the cast consumed (a sink-side copy — the machine's
    /// own value was moved into the cast).
    pub operand: crate::Value,
    /// The value the cast produced (a sink-side copy — the machine's own
    /// copy is pushed onto the operand stack right after the hook
    /// returns).
    pub result: crate::Value,
}

/// A sink that observes every executed cast: a [`CastSink`] the VM hands
/// one [`CastEvent`] to at each executed `OP_CAST`. Attach through
/// [`crate::Vm::with_casts`], recover through [`crate::Vm::take_casts`] +
/// [`CastSink::as_any`].
pub trait CastSink: 'static {
    /// One cast executed: the operand has been consumed and the result
    /// computed, but not yet pushed. Fires only for casts that produced a
    /// value — a cast that faults (`UnknownCastKind` /
    /// `CastOperandMismatch`) never reaches the hook, and the run's fault
    /// carries the site instead.
    fn cast_evaluated(&mut self, event: &CastEvent);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_casts`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed cast in memory.
///
/// `CastCollector::new()` is unbounded; `CastCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit over a cast-heavy loop — one event per cast must
/// not OOM the embedding host. The flag is a deliberate, *displayed*
/// fact, never a silent drop.
#[derive(Clone, Default)]
pub struct CastCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured events, in execution order.
    events: Vec<CastEvent>,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl CastCollector {
    /// Create an unbounded collector (every executed cast is captured).
    #[must_use]
    pub fn new() -> CastCollector {
        CastCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> CastCollector {
        CastCollector {
            cap: Some(max),
            events: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured events, in execution order.
    #[must_use]
    pub fn events(&self) -> &[CastEvent] {
        &self.events
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no cast was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The captured events for one function's cast sites, in execution
    /// order.
    #[must_use]
    pub fn events_of(&self, func: usize) -> Vec<&CastEvent> {
        self.events.iter().filter(|e| e.func == func).collect()
    }

    /// The captured events of one cast kind, in execution order — the
    /// float-cast audit's first query (`FloatToInt` traffic in one list).
    #[must_use]
    pub fn events_of_kind(&self, kind: CastKind) -> Vec<&CastEvent> {
        self.events.iter().filter(|e| e.kind == kind).collect()
    }

    /// How many captured casts changed the value: the result `Value`
    /// differs from the operand `Value` under the VM's structural
    /// equality. Every cross-kind cast counts (an `Int` is never equal to
    /// a `Float`, so each `IntToFloat`/`FloatToInt` is a change), as does
    /// every truncation or wrap that altered the bits; an identity cast
    /// (`PtrToPtr` traffic, an `IntToInt` over the `Unknown` target) does
    /// not.
    #[must_use]
    pub fn changed_count(&self) -> usize {
        self.events.iter().filter(|e| e.operand != e.result).count()
    }
}

impl CastSink for CastCollector {
    fn cast_evaluated(&mut self, event: &CastEvent) {
        self.push(event.clone());
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl CastCollector {
    /// Append one event under the capture cap (an overflow drops the event
    /// and sets the flag — displayed, never a silent drop).
    fn push(&mut self, event: CastEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.events.push(event);
    }
}

/// The target primitive's name, spelled the way the emitter's target byte
/// names it (the same spellings [`crate::CastTarget`]'s variants carry).
fn target_name(target: crate::CastTarget) -> &'static str {
    match target {
        crate::CastTarget::Unknown => "Unknown",
        crate::CastTarget::I8 => "I8",
        crate::CastTarget::I16 => "I16",
        crate::CastTarget::I32 => "I32",
        crate::CastTarget::I64 => "I64",
        crate::CastTarget::Isize => "Isize",
        crate::CastTarget::U8 => "U8",
        crate::CastTarget::U16 => "U16",
        crate::CastTarget::U32 => "U32",
        crate::CastTarget::U64 => "U64",
        crate::CastTarget::Usize => "Usize",
        crate::CastTarget::F32 => "F32",
        crate::CastTarget::F64 => "F64",
        crate::CastTarget::Bool => "Bool",
        crate::CastTarget::Char => "Char",
    }
}

/// Render a cast stream into a deterministic listing: a header line
/// (image format version, cast count), then one line per executed cast in
/// event order — the decoded kind/target pair, the site's function index
/// and opcode byte offset, and the operand/result values rendered by the
/// machine's value renderer (floats carry their bit pattern, so the
/// golden is platform-exact). A run with no casts renders the explicit
/// `no casts` line (honest about emptiness, like the assert stream's
/// `no assertions`).
///
/// Deterministic and total (any event list renders).
#[must_use]
pub fn render_casts(events: &[CastEvent]) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "casts glyim.module-image/{} casts={}",
        crate::IMAGE_VERSION,
        events.len()
    );
    if events.is_empty() {
        let _ = writeln!(out, "no casts");
        debug!(casts = 0, "vm cast stream rendered");
        return out;
    }
    for (i, event) in events.iter().enumerate() {
        let _ = writeln!(
            out,
            "{i}: {}/{} fn {} pc {} {} -> {}",
            event.kind.name(),
            target_name(event.target),
            event.func,
            event.pc,
            crate::watch::render_value(&event.operand),
            crate::watch::render_value(&event.result),
        );
    }
    debug!(casts = events.len(), "vm cast stream rendered");
    out
}

#[cfg(test)]
mod casts_tests {
    use super::{CastCollector, CastEvent, CastKind, render_casts};
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

    /// main (fn 0): `(-1i64) as u8` — the narrowing-wrap shape. One cast,
    /// the run returns the wrapped `255`.
    fn narrowing_cast_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the operand (-1)
        main.extend_from_slice(&i64le(-1));
        main.push(op(Opcode::Cast)); // pc 9 — site; IntToInt -> U8
        main.push(0u8);
        main.push(6u8);
        main.push(op(Opcode::Return)); // pc 12
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0): `(3.99f64) as i64` — the float-truncation shape. One
    /// cast, the run returns the truncated `3`.
    fn float_truncation_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0 — the operand (3.99)
        main.extend_from_slice(&3.99f64.to_le_bytes());
        main.push(op(Opcode::Cast)); // pc 9 — site; FloatToInt -> I64
        main.push(1u8);
        main.push(4u8);
        main.push(op(Opcode::Return)); // pc 12
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0): `7i64 as <unknown>` — the identity shape (the
    /// pre-seam VM's behavior for *every* cast).
    fn identity_cast_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the operand (7)
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::Cast)); // pc 9 — site; IntToInt -> Unknown
        main.push(0u8);
        main.push(0u8);
        main.push(op(Opcode::Return)); // pc 12
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Three casts with distinct kind/target pairs in one entry point:
    /// the narrowing wrap, the float truncation, and the identity. The
    /// render golden's module (the stack carries every result out; the
    /// final `Return` pops the last one).
    fn mixed_casts_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — operand (-1)
        main.extend_from_slice(&i64le(-1));
        main.push(op(Opcode::Cast)); // pc 9 — event 0: IntToInt/U8
        main.push(0u8);
        main.push(6u8);
        main.push(op(Opcode::LoadConstF64)); // pc 12 — operand (3.99)
        main.extend_from_slice(&3.99f64.to_le_bytes());
        main.push(op(Opcode::Cast)); // pc 21 — event 1: FloatToInt/I64
        main.push(1u8);
        main.push(4u8);
        main.push(op(Opcode::LoadConst)); // pc 24 — operand (7)
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::Cast)); // pc 33 — event 2: IntToInt/Unknown
        main.push(0u8);
        main.push(0u8);
        main.push(op(Opcode::Return)); // pc 36
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// A countdown loop whose body casts the counter through `u8` every
    /// iteration and discards the result: a cast-heavy shape (one event
    /// per iteration). Seeds local 0 with `count`, decrements it, casts,
    /// loops while it is still positive.
    fn cast_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the seed
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        // Loop head (pc 14): decrement the counter, cast it through u8,
        // drop the cast result, loop while the counter is positive.
        main.push(op(Opcode::LoadLocal)); // pc 14 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 19 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 28 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 29
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 34 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Cast)); // pc 39 — the per-iteration cast site
        main.push(0u8);
        main.push(6u8);
        main.push(op(Opcode::Drop)); // pc 42 — discard the cast result
        main.push(op(Opcode::LoadLocal)); // pc 43 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 48 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 57 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 58 — back to the loop head
        main.extend_from_slice(&u32le(14));
        main.push(op(Opcode::LoadLocal)); // pc 63
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 68
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0): `3i64 as i64` via the `FloatToInt` kind — the
    /// operand/kind mismatch that faults before producing a result.
    fn mismatching_cast_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the Int operand
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Cast)); // pc 9 — FloatToInt over an Int
        main.push(1u8);
        main.push(4u8);
        main.push(op(Opcode::Return)); // pc 12 — never reached
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// main (fn 0): a cast-kind byte the VM does not decode (`9`).
    fn unknown_cast_kind_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(3));
        main.push(op(Opcode::Cast)); // pc 9 — kind byte 9
        main.push(9u8);
        main.push(4u8);
        main.push(op(Opcode::Return)); // pc 12 — never reached
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// Run a module with a cast collector attached; the run's outcome and
    /// the typed collector both come back (the collector through
    /// `take_casts` + `as_any`).
    fn traced_run(module: &Module) -> (CastCollector, Result<Value, crate::VmError>) {
        let collector = CastCollector::new();
        let mut vm = Vm::new().with_casts(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_casts().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CastCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// One executed cast reports exactly one event: the site, the decoded
    /// kind/target, and the operand/result pair — the run's value is the
    /// wrapped result the pre-seam VM computes.
    #[test]
    fn narrowing_wrap_reports_the_full_round_trip() {
        let module = narrowing_cast_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(255));
        assert_eq!(collector.events().len(), 1);
        let event = &collector.events()[0];
        assert_eq!(event.func, 0);
        assert_eq!(event.pc, 9, "the OP_CAST opcode's byte offset");
        assert_eq!(event.kind, CastKind::IntToInt);
        assert_eq!(event.target, crate::CastTarget::U8);
        assert_eq!(event.operand, Value::Int(-1));
        assert_eq!(event.result, Value::Int(255), "the wrap is in the event");
        assert_eq!(collector.changed_count(), 1);
    }

    /// A float truncation reports the float operand and the truncated
    /// integer result — the seam's core audit shape.
    #[test]
    fn float_truncation_reports_the_truncated_result() {
        let module = float_truncation_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(3));
        let event = &collector.events()[0];
        assert_eq!(event.kind, CastKind::FloatToInt);
        assert_eq!(event.target, crate::CastTarget::I64);
        assert_eq!(event.operand, Value::Float(3.99));
        assert_eq!(event.result, Value::Int(3));
        assert_eq!(collector.changed_count(), 1);
        assert_eq!(collector.events_of_kind(CastKind::FloatToInt).len(), 1);
        assert!(collector.events_of_kind(CastKind::IntToFloat).is_empty());
    }

    /// An identity cast (the pre-seam VM's behavior for every cast) still
    /// reports — and its operand and result are equal, so the changed
    /// fold does not count it.
    #[test]
    fn identity_cast_reports_the_unchanged_value() {
        let module = identity_cast_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(7));
        let event = &collector.events()[0];
        assert_eq!(event.kind, CastKind::IntToInt);
        assert_eq!(event.target, crate::CastTarget::Unknown);
        assert_eq!(event.operand, Value::Int(7));
        assert_eq!(event.result, Value::Int(7));
        assert_eq!(collector.changed_count(), 0);
    }

    /// A cast-heavy loop reports one event per iteration: the loop's
    /// per-site events are readable through `events_of`, and the
    /// unchanged small-counter casts do not count as changed.
    #[test]
    fn cast_loop_reports_one_event_per_iteration() {
        let module = cast_loop_module(3);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(collector.events().len(), 3, "one cast per iteration");
        assert_eq!(collector.events_of(0).len(), 3);
        assert_eq!(collector.changed_count(), 0, "2, 1, 0 all survive u8");
        // Every event names the same site: the loop body's cast at pc 39.
        assert!(collector.events().iter().all(|e| e.pc == 39));
        let counters: Vec<i64> = collector
            .events()
            .iter()
            .map(|e| match e.operand {
                Value::Int(v) => v,
                _ => panic!("the loop casts Ints"),
            })
            .collect();
        assert_eq!(counters, vec![2, 1, 0], "in execution order");
    }

    /// `events_of_kind` groups the stream by cast kind: a mixed run's
    /// `IntToInt` and `FloatToInt` traffic come back as disjoint lists.
    #[test]
    fn events_of_kind_groups_by_cast_kind() {
        let module = mixed_casts_module();
        let (collector, _) = traced_run(&module);
        assert_eq!(collector.events().len(), 3);
        let int_traffic = collector.events_of_kind(CastKind::IntToInt);
        assert_eq!(int_traffic.len(), 2, "the wrap and the identity");
        let float_traffic = collector.events_of_kind(CastKind::FloatToInt);
        assert_eq!(float_traffic.len(), 1);
        assert_eq!(float_traffic[0].operand, Value::Float(3.99));
    }

    /// The render is deterministic and pins the event line spellings: the
    /// decoded kind/target pair, the site, and the operand/result values
    /// in the machine's value renderer (floats carry their bit pattern).
    #[test]
    fn render_is_deterministic_golden() {
        let module = mixed_casts_module();
        let (collector, _) = traced_run(&module);
        let first = render_casts(collector.events());
        let second = render_casts(collector.events());
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
casts glyim.module-image/2 casts=3
0: IntToInt/U8 fn 0 pc 9 -1 -> 255
1: FloatToInt/I64 fn 0 pc 21 3.99 (bits 0x400feb851eb851ec) -> 3
2: IntToInt/Unknown fn 0 pc 33 7 -> 7
"
        );
    }

    /// A bounded collector stops at the cap and flags overflow (displayed,
    /// never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = cast_loop_module(3);
        let collector = CastCollector::bounded(2);
        let mut vm = Vm::new().with_casts(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_casts().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CastCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 2, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The seam is read-only: the traced run's value equals the untraced
    /// one exactly, and the faulting runs' errors equal the untraced ones
    /// exactly.
    #[test]
    fn the_seam_is_read_only() {
        let narrowing = narrowing_cast_module();
        let untraced = Vm::new().run_module(&narrowing).expect("untraced run");
        let (_, traced) = traced_run(&narrowing);
        assert_eq!(traced.expect("traced run"), untraced);
        let truncation = float_truncation_module();
        let untraced = Vm::new().run_module(&truncation).expect("untraced run");
        let (_, traced) = traced_run(&truncation);
        assert_eq!(traced.expect("traced run"), untraced);
    }

    /// A cast that faults (kind/kind-operand mismatch) never reaches the
    /// hook — the stream stays empty and the run faults with exactly the
    /// pre-seam errors.
    #[test]
    fn a_faulting_cast_reports_nothing() {
        let mismatching = mismatching_cast_module();
        let (collector, outcome) = traced_run(&mismatching);
        assert_eq!(
            outcome.expect_err("the mismatch faults"),
            crate::VmError::CastOperandMismatch {
                kind: "FloatToInt",
                operand: "Int",
            }
        );
        assert!(collector.is_empty(), "no event for a faulting cast");
        let unknown = unknown_cast_kind_module();
        let (_, outcome) = traced_run(&unknown);
        assert_eq!(
            outcome.expect_err("the unknown kind faults"),
            crate::VmError::UnknownCastKind(9)
        );
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_casts() {
        assert_eq!(
            render_casts(&[]),
            "\
casts glyim.module-image/2 casts=0
no casts
"
        );
    }

    /// The event type is comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable.
    #[test]
    fn events_are_structurally_comparable() {
        let a = CastEvent {
            func: 0,
            pc: 9,
            kind: CastKind::IntToInt,
            target: crate::CastTarget::U8,
            operand: Value::Int(-1),
            result: Value::Int(255),
        };
        let mut b = a.clone();
        assert_eq!(a, b);
        b.result = Value::Int(-1);
        assert_ne!(a, b);
        assert_eq!(CastKind::from_u8(0), Some(CastKind::IntToInt));
        assert_eq!(CastKind::from_u8(8), None);
        assert_eq!(CastKind::FloatToFloat.name(), "FloatToFloat");
    }
}
