//! Integer-arithmetic observation seam: what did this run *compute* in
//! the machine's wrapping integer arithmetic, at which sites, and
//! where did the wrap semantics actually engage?
//!
//! Every other opcode class carries an observation hook; the integer
//! stream was the last one without one. The machine's integer faces
//! are the `(Int, Int)` binop arm — arithmetic (`Add` `Sub` `Mul`
//! `Div` `Rem`), the comparisons (`Eq` … `Ge`), the logic ops (`And`
//! `Or`), and the bitwise/shift ops (`BitAnd` `BitOr` `BitXor` `Shl`
//! `Shr`) — plus the unary `OP_NEG` and `OP_NOT` over an integer, and
//! they are the arithmetic whose *silence* is the hazard: `Add`
//! through `Mul` and `Neg` run in the documented wrapping semantics
//! (`i64::MAX + 1` answers `i64::MIN`, never a fault), shifts mask
//! their shift amount (`wrapping_shl`/`wrapping_shr`), and a wrapped
//! answer flows into comparisons and branches exactly like an exact
//! one — a silently-wrapped value looks to the caller like a lowering
//! bug rather than the arithmetic it came from. This module is that
//! stream's counterpart: hooks in the binop arm — the VM fires
//! [`MathSink::math_binop`] just after the `(Int, Int)` arm has
//! computed its answer and before it is pushed, with the wrap verdict
//! spelled per event — and in the unary arm — [`MathSink::math_neg`]
//! per executed `OP_NEG` over an integer (the `i64::MIN` negation is
//! the one wrapped case) and [`MathSink::math_not`] per executed
//! `OP_NOT` — handing one event per executed integer operation to a
//! sink the caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the pushed value is exactly the pre-seam machine's),
//! **total** (every *executed* integer operation reports — an
//! operation the machine *refuses*, a mixed `(Int, Float)` binop or a
//! bitwise op over floats, never reaches the hook: the fault stream
//! owns the refusal, and so does a division that traps —
//! `checked_div`/`checked_rem` refusals are [`crate::VmError::AbnormalTermination`]
//! faults, not events), **bounded** (the collector's memory is capped
//! and flags overflow instead of growing without limit over an
//! arithmetic-heavy loop), and **deterministic**
//! ([`render_math`] renders the same collector byte-identically — the
//! test suite pins a golden).
//!
//! The hooks fire only when a sink is attached: one `Option` check per
//! executed integer operation, exactly the pre-seam behavior when the
//! sink is absent (the operand copies are paid only on the fire path).
//! The events' operands and results are sink-side copies (the
//! copies-into-the-event shape every value-carrying seam follows) —
//! the machine's own values stay exactly where they were.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use tracing::debug;

/// One executed `(Int, Int)` binop, handed to the attached
/// [`MathSink`] just after the binop arm has computed its answer and
/// just before it is pushed (a *refused* pairing — mixed `Int`/`Float`
/// operands, or a bitwise op over floats — faults with
/// [`crate::VmError::BinOpTypeMismatch`] and never reaches the hook:
/// the fault stream owns it): the site, the opcode, the operands in
/// full, the machine's answer, and the wrap verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct MathBinop {
    /// The function index containing the binop site.
    pub func: usize,
    /// The byte offset of the binop opcode (the binop site).
    pub pc: usize,
    /// The dispatched opcode (`Add` … `Shr` — the `(Int, Int)` arm's
    /// face).
    pub op: crate::Opcode,
    /// The left operand (a sink-side copy).
    pub lhs: crate::Value,
    /// The right operand (a sink-side copy).
    pub rhs: crate::Value,
    /// The machine's answer (the pushed `Int`): arithmetic answers
    /// carry the documented wrapping semantics when `wrapped` holds;
    /// comparisons push the machine's `Int` bool encoding (`JumpIf` is
    /// jump-if-nonzero).
    pub result: crate::Value,
    /// Whether the answer was produced *by the wrapping semantics*
    /// rather than exactly: `Add`/`Sub`/`Mul` overflows, a negation
    /// past the signed range, or a shift whose shift amount was
    /// masked (`wrapping_shl`/`wrapping_shr` shift by
    /// `(amount as u32) & 63`). Comparisons, logic ops, bitwise ops,
    /// and the trapping division/refusal cases never wrap. The fold
    /// the embedding host actually asks for — *where did the wrap
    /// semantics engage* — is [`MathCollector::wraps`] /
    /// [`MathCollector::wrapped_by_function`].
    pub wrapped: bool,
}

/// One executed `OP_NEG` over an integer, handed to the attached
/// [`MathSink`] just after the unary arm has negated the operand and
/// just before the answer is pushed (an `OP_NOT` over a float is a
/// refusal — [`crate::VmError::UnOpTypeMismatch`] — and never reaches
/// the hook: the fault stream owns it): the site, the operand in
/// full, the answer, and the wrap verdict (the negation of `i64::MIN`
/// answers `i64::MIN` — the one wrapped negation).
#[derive(Clone, Debug, PartialEq)]
pub struct MathNeg {
    /// The function index containing the negation site.
    pub func: usize,
    /// The byte offset of the `OP_NEG` opcode (the negation site).
    pub pc: usize,
    /// The negated operand (a sink-side copy).
    pub operand: crate::Value,
    /// The machine's answer (the pushed `Int`): the wrapping negation
    /// (`-i64::MIN` answers `i64::MIN`, flagged in `wrapped`).
    pub result: crate::Value,
    /// Whether the negation ran past the signed range (the operand
    /// was `i64::MIN` and the answer wrapped to itself).
    pub wrapped: bool,
}

/// One executed `OP_NOT` over an integer, handed to the attached
/// [`MathSink`] just after the unary arm has computed the machine's
/// bool encoding and just before the answer is pushed (`OP_NOT` over
/// a float is a refusal — the fault stream owns it): the site, the
/// operand in full, and the answer.
#[derive(Clone, Debug, PartialEq)]
pub struct MathNot {
    /// The function index containing the not site.
    pub func: usize,
    /// The byte offset of the `OP_NOT` opcode (the not site).
    pub pc: usize,
    /// The tested operand (a sink-side copy).
    pub operand: crate::Value,
    /// The machine's answer (the pushed `Int` bool encoding: `1` when
    /// the operand was `0`, `0` otherwise).
    pub result: crate::Value,
}

/// The interleaved integer transcript: one entry per executed integer
/// operation, in execution order — the shape
/// [`MathCollector::events`] returns and [`render_math`] lists.
#[derive(Clone, Debug, PartialEq)]
pub enum MathEvent {
    /// A `(Int, Int)` binop ([`MathBinop`]).
    Binop(MathBinop),
    /// An `OP_NEG` over an integer ([`MathNeg`]).
    Neg(MathNeg),
    /// An `OP_NOT` over an integer ([`MathNot`]).
    Not(MathNot),
}

/// A sink that observes every executed integer operation: a
/// [`MathSink`] the VM hands one [`MathBinop`] per executed
/// `(Int, Int)` binop arm, one [`MathNeg`] per executed `OP_NEG` over
/// an integer, and one [`MathNot`] per executed `OP_NOT` over an
/// integer. Attach through [`crate::Vm::with_math`], recover through
/// [`crate::Vm::take_math`] + [`MathSink::as_any`].
pub trait MathSink: 'static {
    /// One `(Int, Int)` binop executed: the answer has been computed
    /// and is about to be pushed (a refused pairing never reaches the
    /// hook — the fault stream owns it).
    fn math_binop(&mut self, event: &MathBinop);

    /// One `OP_NEG` over an integer executed: the answer has been
    /// computed and is about to be pushed.
    fn math_neg(&mut self, event: &MathNeg);

    /// One `OP_NOT` over an integer executed: the answer has been
    /// computed and is about to be pushed.
    fn math_not(&mut self, event: &MathNot);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_math`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed integer operation in memory.
///
/// `MathCollector::new()` is unbounded; `MathCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead
/// of growing without limit over an arithmetic-heavy loop — one event
/// per integer operation must not OOM the embedding host. The flag is
/// a deliberate, *displayed* fact, never a silent drop.
///
/// The collector also folds as it captures: the operation tallies
/// ([`MathCollector::binop_count`], [`MathCollector::neg_count`],
/// [`MathCollector::not_count`]), the wrap tally
/// ([`MathCollector::wraps`]), the per-opcode binop counts
/// ([`MathCollector::count_by_opcode`]), and the per-function wrap
/// counts ([`MathCollector::wrapped_by_function`]) — the *where did
/// the wrap semantics engage* fold, keyed by function index in a
/// `BTreeMap` so it is deterministic by construction.
#[derive(Clone, Default)]
pub struct MathCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<MathEvent>,
    /// The binop tally (the executed `(Int, Int)` arms).
    binops: usize,
    /// The negation tally (the `OP_NEG`-over-int count).
    negs: usize,
    /// The not tally (the `OP_NOT`-over-int count).
    nots: usize,
    /// The wrap tally (the answers produced by the wrapping semantics).
    wraps: usize,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl MathCollector {
    /// Create an unbounded collector (every executed integer operation
    /// is captured).
    #[must_use]
    pub fn new() -> MathCollector {
        MathCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> MathCollector {
        MathCollector {
            cap: Some(max),
            events: Vec::new(),
            binops: 0,
            negs: 0,
            nots: 0,
            wraps: 0,
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order (binops, negations,
    /// and nots interleaved exactly as the run performed them).
    #[must_use]
    pub fn events(&self) -> &[MathEvent] {
        &self.events
    }

    /// The captured binops, in execution order.
    #[must_use]
    pub fn binop_events(&self) -> Vec<&MathBinop> {
        self.events
            .iter()
            .filter_map(|e| match e {
                MathEvent::Binop(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    /// The captured negations, in execution order.
    #[must_use]
    pub fn neg_events(&self) -> Vec<&MathNeg> {
        self.events
            .iter()
            .filter_map(|e| match e {
                MathEvent::Neg(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    /// The captured nots, in execution order.
    #[must_use]
    pub fn not_events(&self) -> Vec<&MathNot> {
        self.events
            .iter()
            .filter_map(|e| match e {
                MathEvent::Not(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    /// Consume the collector into its transcript.
    #[must_use]
    pub fn into_events(self) -> Vec<MathEvent> {
        self.events
    }

    /// How many events were captured (all kinds together).
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// How many executed `(Int, Int)` binops were captured.
    #[must_use]
    pub fn binop_count(&self) -> usize {
        self.binops
    }

    /// How many executed `OP_NEG`-over-int operations were captured.
    #[must_use]
    pub fn neg_count(&self) -> usize {
        self.negs
    }

    /// How many executed `OP_NOT`-over-int operations were captured.
    #[must_use]
    pub fn not_count(&self) -> usize {
        self.nots
    }

    /// How many captured answers were produced *by the wrapping
    /// semantics* rather than exactly (binop overflows, the
    /// `i64::MIN` negation, masked shift amounts).
    #[must_use]
    pub fn wraps(&self) -> usize {
        self.wraps
    }

    /// The per-opcode binop counts (a `BTreeMap` keyed by the opcode's
    /// name, so the fold is deterministic — the same capture folds
    /// identically every time).
    #[must_use]
    pub fn count_by_opcode(&self) -> BTreeMap<&'static str, u64> {
        let mut counts = BTreeMap::new();
        for event in &self.events {
            if let MathEvent::Binop(b) = event {
                *counts.entry(b.op.name()).or_insert(0) += 1;
            }
        }
        counts
    }

    /// The wrap counts per function: how many captured answers each
    /// function produced *by the wrapping semantics* (a `BTreeMap`,
    /// deterministic by construction) — the *where did the wrap
    /// semantics engage* fold.
    #[must_use]
    pub fn wrapped_by_function(&self) -> BTreeMap<usize, u64> {
        let mut wraps = BTreeMap::new();
        for event in &self.events {
            let (func, wrapped) = match event {
                MathEvent::Binop(b) => (b.func, b.wrapped),
                MathEvent::Neg(n) => (n.func, n.wrapped),
                MathEvent::Not(_) => continue,
            };
            if wrapped {
                *wraps.entry(func).or_insert(0) += 1;
            }
        }
        wraps
    }
}

impl MathSink for MathCollector {
    fn math_binop(&mut self, event: &MathBinop) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.binops += 1;
        if event.wrapped {
            self.wraps += 1;
        }
        self.events.push(MathEvent::Binop(event.clone()));
    }

    fn math_neg(&mut self, event: &MathNeg) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.negs += 1;
        if event.wrapped {
            self.wraps += 1;
        }
        self.events.push(MathEvent::Neg(event.clone()));
    }

    fn math_not(&mut self, event: &MathNot) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.nots += 1;
        self.events.push(MathEvent::Not(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// The wrap verdict for one executed `(Int, Int)` binop: whether the
/// machine's answer came from the wrapping semantics rather than
/// exactly. `Add`/`Sub`/`Mul` overflow past the signed range, shifts
/// mask their shift amount (`wrapping_shl`/`wrapping_shr` shift by
/// `(amount as u32) & 63`), and every other integer binop (comparisons,
/// logic, bitwise, and the trapping division cases) answers exactly or
/// faults. Public for the dispatch-loop hook site; the events carry
/// the verdict already.
#[must_use]
pub fn binop_wrapped(op: crate::Opcode, lhs: &crate::Value, rhs: &crate::Value) -> bool {
    let (crate::Value::Int(a), crate::Value::Int(b)) = (lhs, rhs) else {
        return false;
    };
    match op {
        crate::Opcode::Add => a.checked_add(*b).is_none(),
        crate::Opcode::Sub => a.checked_sub(*b).is_none(),
        crate::Opcode::Mul => a.checked_mul(*b).is_none(),
        // `wrapping_shl`/`wrapping_shr` shift by `(amount as u32) & 63`;
        // the shift was masked exactly when that effective amount
        // differs from the operand the machine was handed.
        crate::Opcode::Shl => i64::from((*b as u32) & 63) != *b,
        crate::Opcode::Shr => i64::from((*b as u32) & 63) != *b,
        _ => false,
    }
}

/// A sink that emits one `tracing::debug!` event per executed integer
/// operation — the seam's counterpart for the workspace-standard
/// observability layer (the same `tracing` dependency every other seam
/// logs through). Enable it per subscriber, never by paying the cost
/// unconditionally.
pub struct TracingMathSink;

impl MathSink for TracingMathSink {
    fn math_binop(&mut self, event: &MathBinop) {
        debug!(
            func = event.func,
            pc = event.pc,
            op = event.op.name(),
            wrapped = event.wrapped,
            "vm math binop"
        );
    }

    fn math_neg(&mut self, event: &MathNeg) {
        debug!(
            func = event.func,
            pc = event.pc,
            wrapped = event.wrapped,
            "vm math neg"
        );
    }

    fn math_not(&mut self, event: &MathNot) {
        debug!(func = event.func, pc = event.pc, "vm math not");
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render an integer transcript into a deterministic listing: a header
/// line (image format version, the binop/negation/not tallies, and the
/// wrap tally), then one line per event in execution order — the
/// event's kind (`binop`/`neg`/`not`), the site, the opcode (for
/// binops), the operands and the answer in the machine's value
/// spellings after a `->` where an answer exists, and the `wrapped`
/// verdict appended when the wrapping semantics produced the answer. A
/// run with no integer operations renders the explicit
/// `no math events` line (honest about emptiness, like the float
/// stream's `no float events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_math(collector: &MathCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "math glyim.module-image/{} binops={} negs={} nots={} wraps={}",
        crate::IMAGE_VERSION,
        collector.binops,
        collector.negs,
        collector.nots,
        collector.wraps,
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no math events");
        debug!(events = 0, "vm math stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        match event {
            MathEvent::Binop(b) => {
                let wrapped = if b.wrapped { " wrapped" } else { "" };
                let _ = writeln!(
                    out,
                    "{i}: binop fn {} pc {} op {} lhs={} rhs={} -> {}{}",
                    b.func,
                    b.pc,
                    b.op.name(),
                    crate::watch::render_value(&b.lhs),
                    crate::watch::render_value(&b.rhs),
                    crate::watch::render_value(&b.result),
                    wrapped,
                );
            }
            MathEvent::Neg(n) => {
                let wrapped = if n.wrapped { " wrapped" } else { "" };
                let _ = writeln!(
                    out,
                    "{i}: neg fn {} pc {} operand={} -> {}{}",
                    n.func,
                    n.pc,
                    crate::watch::render_value(&n.operand),
                    crate::watch::render_value(&n.result),
                    wrapped,
                );
            }
            MathEvent::Not(n) => {
                let _ = writeln!(
                    out,
                    "{i}: not fn {} pc {} operand={} -> {}",
                    n.func,
                    n.pc,
                    crate::watch::render_value(&n.operand),
                    crate::watch::render_value(&n.result),
                );
            }
        }
    }
    debug!(events = collector.events.len(), "vm math stream rendered");
    out
}

#[cfg(test)]
mod math_tests {
    use super::{MathCollector, MathEvent, MathSink, TracingMathSink, render_math};
    use crate::{Function, Module, Opcode, Value, Vm, VmError, decode_module, encode_module};
    use std::collections::BTreeMap;

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// main (fn 0) pushes 7 and 6 and multiplies them: the canonical
    /// one-binop kernel (two constant pushes, one binop, one return —
    /// 4 dispatches, the binop at pc 18).
    fn kernel_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the left operand
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadConst)); // pc 9 — the right operand
        main.extend_from_slice(&i64le(6));
        main.push(op(Opcode::Mul)); // pc 18 — the binop
        main.push(op(Opcode::Return)); // pc 19 — pops the answer
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) computes `i64::MAX - (-1)` — the canonical wrapped
    /// subtraction (answers `i64::MIN` by the wrapping semantics) —
    /// then negates `i64::MIN` (the one wrapped negation) and tests it
    /// with `OP_NOT`: the wrap-kernel shape, three math events, two of
    /// them wrapped.
    fn wrap_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — i64::MAX
        main.extend_from_slice(&i64le(i64::MAX));
        main.push(op(Opcode::LoadConst)); // pc 9 — -1
        main.extend_from_slice(&i64le(-1));
        main.push(op(Opcode::Sub)); // pc 18 — wrapped to i64::MIN
        main.push(op(Opcode::Neg)); // pc 19 — the wrapped negation
        main.push(op(Opcode::Not)); // pc 20 — bools the wrapped answer
        main.push(op(Opcode::Return)); // pc 21 — pops the answer
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) shifts 1 by 70 — the shift amount is masked to 6 by
    /// `wrapping_shl` — the masked-shift shape.
    fn shift_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — 1
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConst)); // pc 9 — 70 (masked to 6)
        main.extend_from_slice(&i64le(70));
        main.push(op(Opcode::Shl)); // pc 18 — wrapped (masked) shift
        main.push(op(Opcode::Return)); // pc 19 — pops the answer
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) pushes 1.5 and 2.25 and multiplies them: the float
    /// kernel — a `(Float, Float)` binop, which is the float seam's
    /// face and never the math seam's.
    fn float_kernel_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64));
        main.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        main.push(op(Opcode::LoadConstF64));
        main.extend_from_slice(&2.25f64.to_bits().to_le_bytes());
        main.push(op(Opcode::Mul));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) adds 1 and 1.5 — the mixed pairing the machine
    /// refuses — under an attached sink: the refusal faults and the
    /// stream stays empty (the fault stream owns it).
    fn mixed_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConstF64));
        main.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        main.push(op(Opcode::Add));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) divides 1 by 0 — the trapping division the machine
    /// refuses — under an attached sink: the fault reports nothing to
    /// the math stream.
    fn trap_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Div));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// Run a module with a math collector attached; the value must match
    /// the unsinked result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_math`.
    fn mathed_run(module: &Module) -> (MathCollector, Value) {
        let collector = MathCollector::new();
        let mut vm = Vm::new().with_math(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_math().expect("sink still attached");
        let collector = boxed
            .as_any()
            .downcast_mut::<MathCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// The golden binop transcript: one `(Int, Int)` binop event with
    /// the operands, the answer, and the exact (unwrapped) verdict, the
    /// per-opcode fold naming `Mul`, and the whole report pinned
    /// byte-identically.
    #[test]
    fn golden_pinned_binop_transcript() {
        let module = kernel_module();
        let (collector, value) = mathed_run(&module);
        assert_eq!(value, Value::Int(42));
        let events = collector.events();
        assert_eq!(events.len(), 1, "one math event: the binop");
        match &events[0] {
            MathEvent::Binop(b) => {
                assert_eq!((b.func, b.pc, b.op), (0, 18, Opcode::Mul));
                assert_eq!(
                    (&b.lhs, &b.rhs, &b.result),
                    (&Value::Int(7), &Value::Int(6), &Value::Int(42))
                );
                assert!(!b.wrapped, "7 * 6 is exact");
            }
            other => panic!("expected a binop event, got {other:?}"),
        }
        assert_eq!(collector.binop_count(), 1);
        assert_eq!(collector.neg_count(), 0);
        assert_eq!(collector.not_count(), 0);
        assert_eq!(collector.wraps(), 0);
        let mut counts = BTreeMap::new();
        counts.insert("Mul", 1u64);
        assert_eq!(collector.count_by_opcode(), counts);
        assert_eq!(collector.wrapped_by_function(), BTreeMap::new());
        assert_eq!(
            render_math(&collector),
            "\
math glyim.module-image/2 binops=1 negs=0 nots=0 wraps=0
0: binop fn 0 pc 18 op Mul lhs=7 rhs=6 -> 42
"
        );
    }

    /// The wrap verdict is exact: the wrapped subtraction, the wrapped
    /// negation of `i64::MIN`, and the masked shift all land as
    /// `wrapped` events; the bool-testing `OP_NOT` between them lands
    /// unwrapped; the per-function wrap fold names fn 0 twice.
    #[test]
    fn wrap_verdicts_are_exact() {
        let module = wrap_module();
        let (collector, value) = mathed_run(&module);
        assert_eq!(value, Value::Int(0), "not(i64::MIN) is 0");
        let events = collector.events();
        assert_eq!(events.len(), 3, "sub, neg, not");
        match &events[0] {
            MathEvent::Binop(b) => {
                assert_eq!((b.op, b.wrapped), (Opcode::Sub, true));
                assert_eq!(
                    (&b.lhs, &b.rhs, &b.result),
                    (
                        &Value::Int(i64::MAX),
                        &Value::Int(-1),
                        &Value::Int(i64::MIN)
                    ),
                    "i64::MAX - (-1) wraps to i64::MIN"
                );
            }
            other => panic!("expected a binop event, got {other:?}"),
        }
        match &events[1] {
            MathEvent::Neg(n) => {
                assert!(n.wrapped, "neg(i64::MIN) wraps");
                assert_eq!(
                    (&n.operand, &n.result),
                    (&Value::Int(i64::MIN), &Value::Int(i64::MIN))
                );
            }
            other => panic!("expected a neg event, got {other:?}"),
        }
        match &events[2] {
            MathEvent::Not(n) => {
                assert_eq!(
                    (&n.operand, &n.result),
                    (&Value::Int(i64::MIN), &Value::Int(0))
                );
            }
            other => panic!("expected a not event, got {other:?}"),
        }
        assert_eq!(collector.binop_count(), 1);
        assert_eq!(collector.neg_count(), 1);
        assert_eq!(collector.not_count(), 1);
        assert_eq!(collector.wraps(), 2);
        let mut wraps = BTreeMap::new();
        wraps.insert(0usize, 2u64);
        assert_eq!(collector.wrapped_by_function(), wraps);
    }

    /// A shift amount past the width is masked — the masked-shift wrap
    /// verdict is exact and the answer is the masked shift's.
    #[test]
    fn masked_shift_is_wrapped() {
        let module = shift_module();
        let (collector, value) = mathed_run(&module);
        assert_eq!(value, Value::Int(64), "1 << (70 & 63) == 1 << 6");
        let events = collector.events();
        match &events[0] {
            MathEvent::Binop(b) => {
                assert_eq!((b.op, b.wrapped), (Opcode::Shl, true));
                assert_eq!(
                    (&b.lhs, &b.rhs, &b.result),
                    (&Value::Int(1), &Value::Int(70), &Value::Int(64)),
                    "the effective shift was 6, not 70"
                );
            }
            other => panic!("expected a binop event, got {other:?}"),
        }
        assert_eq!(collector.wraps(), 1);
        assert_eq!(
            render_math(&collector),
            "\
math glyim.module-image/2 binops=1 negs=0 nots=0 wraps=1
0: binop fn 0 pc 18 op Shl lhs=1 rhs=70 -> 64 wrapped
"
        );
    }

    /// A `(Float, Float)` binop is the float seam's face and never the
    /// math seam's: the float stream owns it, the math stream stays
    /// empty, and the answers match the unsinked run.
    #[test]
    fn float_binops_never_reach_the_math_stream() {
        let module = float_kernel_module();
        let (collector, value) = mathed_run(&module);
        assert_eq!(value, Value::Float(3.375));
        assert!(collector.is_empty(), "no math events for float arithmetic");
        assert_eq!(
            render_math(&collector),
            "math glyim.module-image/2 binops=0 negs=0 nots=0 wraps=0\nno math events\n"
        );
    }

    /// A refused pairing (mixed kinds) faults, and the fault stream
    /// owns it: the math stream stays empty and the error is the typed
    /// mismatch.
    #[test]
    fn refused_pairing_reports_nothing() {
        let module = mixed_module();
        let collector = MathCollector::new();
        let mut vm = Vm::new().with_math(Box::new(collector));
        let err = vm.run_module(&module).expect_err("mixed kinds fault");
        assert!(matches!(err, VmError::BinOpTypeMismatch { .. }));
        let mut boxed = vm.take_math().expect("attached");
        let collector = boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("collector");
        assert!(collector.is_empty());
    }

    /// A trapping division faults, and the fault stream owns it: the
    /// math stream never sees the refused division.
    #[test]
    fn trapping_division_reports_nothing() {
        let module = trap_module();
        let collector = MathCollector::new();
        let mut vm = Vm::new().with_math(Box::new(collector));
        let err = vm.run_module(&module).expect_err("1 / 0 traps");
        assert_eq!(err, VmError::AbnormalTermination);
        let mut boxed = vm.take_math().expect("attached");
        let collector = boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("collector");
        assert!(collector.is_empty());
    }

    /// A VM without a sink behaves exactly as before: no events, no
    /// panic, and `take_math` comes back empty.
    #[test]
    fn no_sink_by_default() {
        let module = kernel_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_math().is_none());
    }

    /// A bounded collector marks overflow instead of growing without
    /// limit (a loop reporting every arithmetic step must not OOM the
    /// embedding host).
    #[test]
    fn bounded_collector_marks_overflow() {
        let module = wrap_module();
        let collector = MathCollector::bounded(2);
        let mut vm = Vm::new().with_math(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_math().expect("attached");
        let collector = boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("collector");
        assert_eq!(collector.len(), 2, "capture stops at the cap");
        assert!(collector.overflowed());
    }

    /// The seam is read-only: a mathed run produces the same result as
    /// an unsinked one.
    #[test]
    fn mathed_result_matches_unmathed() {
        let module = wrap_module();
        let unmathed = Vm::new().run_module(&module).expect("unsinked run");
        let (collector, mathed) = mathed_run(&module);
        assert_eq!(mathed, unmathed);
        assert_eq!(collector.len(), 3);
    }

    /// The math seam composes with the trace seam and the module image:
    /// decode the shipped bytes, then capture the loaded module's
    /// arithmetic, and the typed sinks come back separately.
    #[test]
    fn math_composes_with_trace_and_image() {
        let module = kernel_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let collector = MathCollector::new();
        let trace = crate::trace::TraceCollector::new();
        let mut vm = Vm::new()
            .with_trace(Box::new(trace))
            .with_math(Box::new(collector));
        let value = vm.run_module(&loaded).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut traced = vm.take_trace().expect("trace attached");
        assert_eq!(
            traced
                .as_any()
                .downcast_ref::<crate::trace::TraceCollector>()
                .expect("trace collector")
                .len(),
            4,
            "one step per dispatch over the image"
        );
        let mut boxed = vm.take_math().expect("math attached");
        let collector = boxed
            .as_any()
            .downcast_ref::<MathCollector>()
            .expect("math collector");
        assert_eq!(
            collector.binop_count(),
            1,
            "the golden binop over the image"
        );
    }

    /// The render is deterministic: the same collector renders
    /// byte-identical text.
    #[test]
    fn render_is_deterministic() {
        let module = wrap_module();
        let (collector, _) = mathed_run(&module);
        assert_eq!(render_math(&collector), render_math(&collector));
    }

    /// `TracingMathSink` is total: attaching it changes nothing about
    /// the run and never panics (with or without a subscriber
    /// installed).
    #[test]
    fn tracing_math_sink_is_total() {
        let module = kernel_module();
        let mut vm = Vm::new().with_math(Box::new(TracingMathSink));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        // An event can also be delivered directly.
        TracingMathSink.math_binop(&super::MathBinop {
            func: 0,
            pc: 18,
            op: Opcode::Mul,
            lhs: Value::Int(7),
            rhs: Value::Int(6),
            result: Value::Int(42),
            wrapped: false,
        });
        TracingMathSink.math_neg(&super::MathNeg {
            func: 0,
            pc: 19,
            operand: Value::Int(i64::MIN),
            result: Value::Int(i64::MIN),
            wrapped: true,
        });
        TracingMathSink.math_not(&super::MathNot {
            func: 0,
            pc: 20,
            operand: Value::Int(0),
            result: Value::Int(1),
        });
    }

    /// The wrap-verdict helper is exact on the boundary cases the
    /// dispatch loop feeds it (the events carry the same verdict).
    #[test]
    fn binop_wrapped_helper_is_exact() {
        use super::binop_wrapped;
        let int = Value::Int;
        assert!(binop_wrapped(Opcode::Add, &int(i64::MAX), &int(1)));
        assert!(!binop_wrapped(Opcode::Add, &int(7), &int(6)));
        assert!(binop_wrapped(Opcode::Sub, &int(i64::MIN), &int(1)));
        assert!(binop_wrapped(Opcode::Mul, &int(i64::MAX), &int(2)));
        assert!(binop_wrapped(Opcode::Shl, &int(1), &int(70)));
        assert!(!binop_wrapped(Opcode::Shl, &int(1), &int(6)));
        assert!(binop_wrapped(Opcode::Shr, &int(-1), &int(-1)), "masked");
        assert!(!binop_wrapped(Opcode::Shr, &int(-1), &int(1)));
        assert!(!binop_wrapped(Opcode::Lt, &int(1), &int(2)));
        assert!(!binop_wrapped(Opcode::BitXor, &int(1), &int(3)));
        assert!(
            !binop_wrapped(Opcode::Div, &int(1), &int(0)),
            "traps, not wraps"
        );
    }
}
