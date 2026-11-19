//! Float-operation observation seam: what did this run *compute* in
//! IEEE-754 arithmetic, at which sites, and what did the machine
//! answer — including the answers nobody checks?
//!
//! Every other opcode class carries an observation hook; the float
//! stream was the last one without one. The machine's float faces are
//! the `OP_LOAD_CONST_F64` constant push, the `(Float, Float)` binop
//! arms, and `OP_NEG` over a float, and they are the only arithmetic
//! the VM runs that *never faults*: integer division by zero traps,
//! but `1.0 / 0.0` is the documented convention (`+inf`, matching
//! LLVM's `fdiv` and the MIR interpreter), and `inf - inf` or
//! `0.0 * inf` land as NaN — a silently-poisoned value that flows
//! into comparisons (every NaN compare is false), feeds a branch that
//! then never fires, and looks to the caller like a lowering bug
//! rather than the arithmetic it came from. This module is that
//! stream's counterpart: hooks in the `OP_LOAD_CONST_F64` arm — the
//! VM fires [`FloatSink::float_const`] just after the constant has
//! been decoded and before the value is pushed — in the binop arm —
//! [`FloatSink::float_binop`] just after the `(Float, Float)` arm has
//! computed its answer and before it is pushed — and in the unary arm
//! — [`FloatSink::float_neg`] just after `OP_NEG` has negated a float
//! — handing one event per executed float operation to a sink the
//! caller attaches.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state — the pushed value is exactly the pre-seam machine's),
//! **total** (every *executed* float operation reports — an operation
//! the machine *refuses*, a mixed `(Int, Float)` binop or a bitwise op
//! over floats, never reaches the hook: the fault stream owns the
//! refusal), **bounded** (the collector's memory is capped and flags
//! overflow instead of growing without limit over a float-heavy
//! loop), and **deterministic** ([`render_floats`] renders the same
//! collector byte-identically — the test suite pins a golden).
//!
//! The hooks fire only when a sink is attached: one `Option` check per
//! executed float operation, exactly the pre-seam behavior when the
//! sink is absent (the operand copies are paid only on the fire path).
//! The events' operands and results are sink-side copies (the
//! copies-into-the-event shape every value-carrying seam follows) —
//! the machine's own values stay exactly where they were.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

/// One executed float-constant push, handed to the attached
/// [`FloatSink`] just after the `OP_LOAD_CONST_F64` payload has been
/// decoded and just before the value is pushed: the site and the
/// constant in full.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatConst {
    /// The function index containing the push site.
    pub func: usize,
    /// The byte offset of the `OP_LOAD_CONST_F64` opcode (the push
    /// site).
    pub pc: usize,
    /// The pushed constant (a sink-side copy — the machine pushes its
    /// own value right after the hook returns).
    pub value: f64,
}

/// One executed `(Float, Float)` binop, handed to the attached
/// [`FloatSink`] just after the binop arm has computed its answer and
/// just before it is pushed (a *refused* pairing — mixed `Int`/`Float`
/// operands, or a bitwise/shift/`Rem` op over floats — faults with
/// [`crate::VmError::BinOpTypeMismatch`] and never reaches the hook:
/// the fault stream owns it): the site, the opcode, the operands in
/// full, and the machine's answer. Arithmetic pushes a `Float`
/// (IEEE-754, `1.0 / 0.0` as `+inf`); comparisons push the machine's
/// `Int` bool encoding (`JumpIf` is jump-if-nonzero).
#[derive(Clone, Debug, PartialEq)]
pub struct FloatBinop {
    /// The function index containing the binop site.
    pub func: usize,
    /// The byte offset of the binop opcode (the binop site).
    pub pc: usize,
    /// The dispatched opcode (`Add` … `Ge` — the `(Float, Float)` arm's
    /// face).
    pub op: crate::Opcode,
    /// The left operand (a sink-side copy).
    pub lhs: crate::Value,
    /// The right operand (a sink-side copy).
    pub rhs: crate::Value,
    /// The machine's answer (the pushed value): a `Float` for
    /// arithmetic, an `Int` for comparisons.
    pub result: crate::Value,
}

/// One executed `OP_NEG` over a float, handed to the attached
/// [`FloatSink`] just after the unary arm has negated the operand and
/// just before the answer is pushed (an `OP_NOT` over a float is a
/// refusal — [`crate::VmError::UnOpTypeMismatch`] — and never reaches
/// the hook: the fault stream owns it): the site, the operand in full,
/// and the IEEE negation.
#[derive(Clone, Debug, PartialEq)]
pub struct FloatNeg {
    /// The function index containing the negation site.
    pub func: usize,
    /// The byte offset of the `OP_NEG` opcode (the negation site).
    pub pc: usize,
    /// The negated operand (a sink-side copy).
    pub operand: crate::Value,
    /// The machine's answer (the pushed `Float`): the IEEE negation
    /// (`-NaN` keeps the payload bits; `-0.0` is distinct from `0.0`).
    pub result: crate::Value,
}

/// The interleaved float transcript: one entry per executed float
/// operation, in execution order — the shape
/// [`FloatCollector::events`] returns and [`render_floats`] lists.
#[derive(Clone, Debug, PartialEq)]
pub enum FloatEvent {
    /// A constant push ([`FloatConst`]).
    Const(FloatConst),
    /// A `(Float, Float)` binop ([`FloatBinop`]).
    Binop(FloatBinop),
    /// An `OP_NEG` over a float ([`FloatNeg`]).
    Neg(FloatNeg),
}

/// A sink that observes every executed float operation: a
/// [`FloatSink`] the VM hands one [`FloatConst`] per executed
/// `OP_LOAD_CONST_F64`, one [`FloatBinop`] per executed
/// `(Float, Float)` binop arm, and one [`FloatNeg`] per executed
/// `OP_NEG` over a float. Attach through [`crate::Vm::with_floats`],
/// recover through [`crate::Vm::take_floats`] +
/// [`FloatSink::as_any`].
pub trait FloatSink: 'static {
    /// One float constant pushed: the payload has been decoded and the
    /// value is about to be pushed.
    fn float_const(&mut self, event: &FloatConst);

    /// One `(Float, Float)` binop executed: the answer has been
    /// computed and is about to be pushed (a refused pairing never
    /// reaches the hook — the fault stream owns it).
    fn float_binop(&mut self, event: &FloatBinop);

    /// One `OP_NEG` over a float executed: the answer has been
    /// computed and is about to be pushed.
    fn float_neg(&mut self, event: &FloatNeg);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_floats`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every executed float operation in memory.
///
/// `FloatCollector::new()` is unbounded; `FloatCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead
/// of growing without limit over a float-heavy loop — one event per
/// float operation must not OOM the embedding host. The flag is a
/// deliberate, *displayed* fact, never a silent drop.
///
/// The collector also folds as it captures: the constant-push tally
/// ([`FloatCollector::const_count`]), the binop tally
/// ([`FloatCollector::binop_count`]), the negation tally
/// ([`FloatCollector::neg_count`]), and the IEEE-754 specials folds —
/// the arithmetic results that came back NaN
/// ([`FloatCollector::nan_results`]) or infinite
/// ([`FloatCollector::inf_results`]). The specials are the convention
/// path's *observable*: `1.0 / 0.0` is `+inf` by contract (never a
/// fault), so a lowering bug that divides by a silently-zero float or
/// poisons an operand lands here as a tally instead of flowing through
/// the run unexamined.
#[derive(Clone, Default)]
pub struct FloatCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured transcript, in execution order.
    events: Vec<FloatEvent>,
    /// The constant-push tally (the `OP_LOAD_CONST_F64` count).
    consts: usize,
    /// The binop tally (the executed `(Float, Float)` arms).
    binops: usize,
    /// The negation tally (the `OP_NEG`-over-float count).
    negs: usize,
    /// The NaN-result tally (the arithmetic answers that came back
    /// NaN).
    nans: usize,
    /// The infinite-result tally (the arithmetic answers that came
    /// back `±inf`).
    infs: usize,
    /// Whether any event was dropped because the cap was reached.
    overflowed: bool,
}

impl FloatCollector {
    /// Create an unbounded collector (every executed float operation
    /// is captured).
    #[must_use]
    pub fn new() -> FloatCollector {
        FloatCollector::default()
    }

    /// Create a collector that captures at most `max` events and flags
    /// overflow past that.
    #[must_use]
    pub fn bounded(max: usize) -> FloatCollector {
        FloatCollector {
            cap: Some(max),
            events: Vec::new(),
            consts: 0,
            binops: 0,
            negs: 0,
            nans: 0,
            infs: 0,
            overflowed: false,
        }
    }

    /// The captured transcript, in execution order (constant pushes,
    /// binops, and negations interleaved exactly as the run performed
    /// them).
    #[must_use]
    pub fn events(&self) -> &[FloatEvent] {
        &self.events
    }

    /// The captured constant pushes, in execution order.
    #[must_use]
    pub fn const_events(&self) -> Vec<&FloatConst> {
        self.events
            .iter()
            .filter_map(|e| match e {
                FloatEvent::Const(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    /// The captured binops, in execution order.
    #[must_use]
    pub fn binop_events(&self) -> Vec<&FloatBinop> {
        self.events
            .iter()
            .filter_map(|e| match e {
                FloatEvent::Binop(b) => Some(b),
                _ => None,
            })
            .collect()
    }

    /// The captured negations, in execution order.
    #[must_use]
    pub fn neg_events(&self) -> Vec<&FloatNeg> {
        self.events
            .iter()
            .filter_map(|e| match e {
                FloatEvent::Neg(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    /// How many captured constant pushes executed
    /// (`OP_LOAD_CONST_F64` events).
    #[must_use]
    pub fn const_count(&self) -> usize {
        self.consts
    }

    /// How many captured binops executed (the `(Float, Float)` arms —
    /// comparisons included; the refused pairings live on the fault
    /// stream).
    #[must_use]
    pub fn binop_count(&self) -> usize {
        self.binops
    }

    /// How many captured negations executed (`OP_NEG`-over-float
    /// events).
    #[must_use]
    pub fn neg_count(&self) -> usize {
        self.negs
    }

    /// How many captured arithmetic results came back NaN — the
    /// silently-poisoned answers (`inf - inf`, `0.0 * inf`, `0.0 /
    /// 0.0`) that flow into comparisons as consistently-false
    /// conditions.
    #[must_use]
    pub fn nan_results(&self) -> usize {
        self.nans
    }

    /// How many captured arithmetic results came back infinite — the
    /// `1.0 / 0.0` convention path (never a fault, matching LLVM's
    /// `fdiv`).
    #[must_use]
    pub fn inf_results(&self) -> usize {
        self.infs
    }

    /// Whether any event was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// Whether no float operation was captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl FloatSink for FloatCollector {
    fn float_const(&mut self, event: &FloatConst) {
        self.push(FloatEvent::Const(event.clone()));
    }

    fn float_binop(&mut self, event: &FloatBinop) {
        self.push(FloatEvent::Binop(event.clone()));
    }

    fn float_neg(&mut self, event: &FloatNeg) {
        self.push(FloatEvent::Neg(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl FloatCollector {
    /// Append one event under the capture cap (an overflow drops the
    /// event and sets the flag — displayed, never a silent drop),
    /// updating the tallies and the specials folds as it goes (the
    /// specials folds read the arithmetic results — binop and negation
    /// answers that are floats; a comparison's `Int` bool is not a
    /// special).
    fn push(&mut self, event: FloatEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        match &event {
            FloatEvent::Const(_) => self.consts += 1,
            FloatEvent::Binop(_) => self.binops += 1,
            FloatEvent::Neg(_) => self.negs += 1,
        }
        let result = match &event {
            FloatEvent::Binop(b) => Some(&b.result),
            FloatEvent::Neg(n) => Some(&n.result),
            FloatEvent::Const(_) => None,
        };
        if let Some(crate::Value::Float(f)) = result {
            if f.is_nan() {
                self.nans += 1;
            }
            if f.is_infinite() {
                self.infs += 1;
            }
        }
        self.events.push(event);
    }
}

/// Render a float stream into a deterministic listing: a header line
/// (image format version, the constant/binop/negation tallies, and the
/// NaN/infinite specials tallies), then one line per event in
/// execution order — the event's kind (`const`/`binop`/`neg`), the
/// site, the opcode (for binops), the operands and the answer in the
/// machine's value spellings (the float spelling carries the bit
/// pattern, so `-0.0` and NaN payloads stay auditable) after a `->`
/// where an answer exists. A run with no float operations renders the
/// explicit `no float events` line (honest about emptiness, like the
/// projection stream's `no projection events`).
///
/// Deterministic and total (any collector renders).
#[must_use]
pub fn render_floats(collector: &FloatCollector) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "floats glyim.module-image/{} consts={} binops={} negs={} nan={} inf={}",
        crate::IMAGE_VERSION,
        collector.consts,
        collector.binops,
        collector.negs,
        collector.nans,
        collector.infs,
    );
    if collector.events.is_empty() {
        let _ = writeln!(out, "no float events");
        debug!(events = 0, "vm float stream rendered");
        return out;
    }
    for (i, event) in collector.events.iter().enumerate() {
        match event {
            FloatEvent::Const(c) => {
                let _ = writeln!(
                    out,
                    "{i}: const fn {} pc {} value={}",
                    c.func,
                    c.pc,
                    crate::watch::render_value(&crate::Value::Float(c.value)),
                );
            }
            FloatEvent::Binop(b) => {
                let _ = writeln!(
                    out,
                    "{i}: binop fn {} pc {} op {} lhs={} rhs={} -> {}",
                    b.func,
                    b.pc,
                    b.op.name(),
                    crate::watch::render_value(&b.lhs),
                    crate::watch::render_value(&b.rhs),
                    crate::watch::render_value(&b.result),
                );
            }
            FloatEvent::Neg(n) => {
                let _ = writeln!(
                    out,
                    "{i}: neg fn {} pc {} operand={} -> {}",
                    n.func,
                    n.pc,
                    crate::watch::render_value(&n.operand),
                    crate::watch::render_value(&n.result),
                );
            }
        }
    }
    debug!(events = collector.events.len(), "vm float stream rendered");
    out
}

#[cfg(test)]
mod floats_tests {
    use super::render_floats;
    use crate::{Function, Module, Opcode, Value, Vm, VmError};
    use std::any::Any;

    fn i64le(v: i64) -> [u8; 8] {
        v.to_le_bytes()
    }
    fn u32le(v: u32) -> [u8; 4] {
        v.to_le_bytes()
    }
    fn f64le(v: f64) -> [u8; 8] {
        v.to_bits().to_le_bytes()
    }
    fn op(o: Opcode) -> u8 {
        o as u8
    }

    /// main (fn 0) pushes 1.5 and 2.25 and multiplies them: the
    /// canonical one-binop kernel (two constant pushes, one binop).
    fn kernel_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0 — the left operand
        main.extend_from_slice(&f64le(1.5));
        main.push(op(Opcode::LoadConstF64)); // pc 9 — the right operand
        main.extend_from_slice(&f64le(2.25));
        main.push(op(Opcode::Mul)); // pc 18 — the binop
        main.push(op(Opcode::Return)); // pc 19 — pops the answer
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) divides 1.0 by 0.0 (the `+inf` convention) and then
    /// multiplies the inf by 0.0 (NaN): the specials shape — both
    /// answers are convention results, never faults.
    fn specials_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0 — the dividend
        main.extend_from_slice(&f64le(1.0));
        main.push(op(Opcode::LoadConstF64)); // pc 9 — the zero divisor
        main.extend_from_slice(&f64le(0.0));
        main.push(op(Opcode::Div)); // pc 18 — +inf by convention
        main.push(op(Opcode::LoadConstF64)); // pc 19 — the multiplier
        main.extend_from_slice(&f64le(0.0));
        main.push(op(Opcode::Mul)); // pc 28 — NaN by convention
        main.push(op(Opcode::Return)); // pc 29 — pops the NaN
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) compares 1.5 < 2.5: the comparison face — the
    /// `(Float, Float)` arm answers the machine's `Int` bool.
    fn comparison_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0 — the left operand
        main.extend_from_slice(&f64le(1.5));
        main.push(op(Opcode::LoadConstF64)); // pc 9 — the right operand
        main.extend_from_slice(&f64le(2.5));
        main.push(op(Opcode::Lt)); // pc 18 — the comparison
        main.push(op(Opcode::Return)); // pc 19 — pops the bool
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) negates 1.5: the unary face — the IEEE negation.
    fn neg_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0 — the operand
        main.extend_from_slice(&f64le(1.5));
        main.push(op(Opcode::Neg)); // pc 9 — the negation
        main.push(op(Opcode::Return)); // pc 10 — pops the answer
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) adds an `Int` to a `Float`: the *refused* shape —
    /// the machine faults with `BinOpTypeMismatch`, and the seam must
    /// report nothing (the fault stream owns the refusal).
    fn mixed_kind_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the Int
        main.extend_from_slice(&i64le(7));
        main.push(op(Opcode::LoadConstF64)); // pc 9 — the Float
        main.extend_from_slice(&f64le(2.5));
        main.push(op(Opcode::Add)); // pc 18 — refused
        main.push(op(Opcode::Return)); // pc 19 — never reached
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) bitwise-ands two floats: the *refused* shape — the
    /// `(Float, Float)` arm has no bitwise face, the machine faults.
    fn float_bitwise_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0
        main.extend_from_slice(&f64le(1.0));
        main.push(op(Opcode::LoadConstF64)); // pc 9
        main.extend_from_slice(&f64le(1.0));
        main.push(op(Opcode::BitAnd)); // pc 18 — refused
        main.push(op(Opcode::Return)); // pc 19 — never reached
        Module::new(vec![Function::new(main, 0, 0)], 0)
    }

    /// main (fn 0) runs a float kernel (two constant pushes, a
    /// multiply, an accumulate) in a countdown loop (`count`
    /// iterations): the float-heavy shape — four float events per
    /// iteration.
    fn float_loop_module(count: i64) -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — the counter
        main.extend_from_slice(&i64le(count));
        main.push(op(Opcode::StoreLocal)); // pc 9 — local 0
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConstF64)); // pc 14 — the accumulator
        main.extend_from_slice(&f64le(0.0));
        main.push(op(Opcode::StoreLocal)); // pc 23 — local 1
        main.extend_from_slice(&u32le(1));
        // Loop head (pc 28): the float kernel into local 1, decrement,
        // loop while the counter is still positive.
        main.push(op(Opcode::LoadConstF64)); // pc 28 — the left factor
        main.extend_from_slice(&f64le(1.5));
        main.push(op(Opcode::LoadConstF64)); // pc 37 — the right factor
        main.extend_from_slice(&f64le(2.25));
        main.push(op(Opcode::Mul)); // pc 46 — 3.375
        main.push(op(Opcode::LoadLocal)); // pc 47 — the accumulator
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Add)); // pc 52 — acc + 3.375
        main.push(op(Opcode::StoreLocal)); // pc 53
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::LoadLocal)); // pc 58 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 63 — one
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Sub)); // pc 72 — counter - 1
        main.push(op(Opcode::StoreLocal)); // pc 73
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadLocal)); // pc 78 — the counter
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::LoadConst)); // pc 83 — zero
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::Gt)); // pc 92 — loop condition
        main.push(op(Opcode::JumpIf)); // pc 93 — back to the loop head
        main.extend_from_slice(&u32le(28));
        main.push(op(Opcode::LoadLocal)); // pc 98 — the accumulator
        main.extend_from_slice(&u32le(1));
        main.push(op(Opcode::Return)); // pc 103
        Module::new(vec![Function::new(main, 2, 0)], 0)
    }

    /// Run a module with a float collector attached; the run's outcome
    /// and the typed collector both come back (the collector through
    /// `take_floats` + `as_any`).
    fn traced_run(module: &Module) -> (super::FloatCollector, Result<Value, VmError>) {
        let collector = super::FloatCollector::new();
        let mut vm = Vm::new().with_floats(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_floats().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<super::FloatCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// The executed float kernel reports one event per float
    /// operation: two constant pushes and one binop with the operands
    /// in full and the machine's answer — and the run's value is the
    /// answer.
    #[test]
    fn the_float_stream_reports_the_kernel() {
        let module = kernel_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Float(3.375));
        assert_eq!(collector.events().len(), 3);
        let consts = collector.const_events();
        assert_eq!(consts[0].func, 0);
        assert_eq!(consts[0].pc, 0, "the first OP_LOAD_CONST_F64's offset");
        assert_eq!(consts[0].value, 1.5);
        assert_eq!(consts[1].pc, 9, "the second OP_LOAD_CONST_F64's offset");
        assert_eq!(consts[1].value, 2.25);
        let binop = collector.binop_events()[0];
        assert_eq!(binop.func, 0);
        assert_eq!(binop.pc, 18, "the OP_MUL opcode's byte offset");
        assert_eq!(binop.op, Opcode::Mul);
        assert_eq!(binop.lhs, Value::Float(1.5));
        assert_eq!(binop.rhs, Value::Float(2.25));
        assert_eq!(binop.result, Value::Float(3.375));
        assert_eq!(collector.const_count(), 2);
        assert_eq!(collector.binop_count(), 1);
        assert_eq!(collector.neg_count(), 0);
        assert_eq!(collector.nan_results(), 0);
        assert_eq!(collector.inf_results(), 0);
    }

    /// The specials shape reports both convention results in the
    /// dedicated folds: `1.0 / 0.0` is `+inf` (never a fault — the
    /// LLVM-`fdiv` convention) and `inf * 0.0` is NaN — and the run's
    /// value is that NaN (asserted by the `is_nan` predicate, not by
    /// bit pattern: the hardware's invalid-operation quiet NaN is
    /// platform-defined, which is exactly why the machine's `Value`
    /// equality is bit-exact and this test is not).
    #[test]
    fn the_specials_folds_report_the_convention_path() {
        let module = specials_module();
        let (collector, outcome) = traced_run(&module);
        assert!(
            matches!(outcome, Ok(Value::Float(f)) if f.is_nan()),
            "inf * 0.0 is NaN, not a fault: {outcome:?}"
        );
        assert_eq!(collector.events().len(), 5);
        let binops = collector.binop_events();
        assert_eq!(binops.len(), 2, "the Div and the Mul");
        assert_eq!(binops[0].result, Value::Float(f64::INFINITY));
        assert!(
            matches!(binops[1].result, Value::Float(f) if f.is_nan()),
            "the second binop's answer is NaN: {:?}",
            binops[1].result
        );
        assert_eq!(collector.inf_results(), 1, "the 1.0 / 0.0 answer");
        assert_eq!(collector.nan_results(), 1, "the inf * 0.0 answer");
        assert_eq!(collector.binop_count(), 2);
    }

    /// The comparison face reports the machine's `Int` bool as the
    /// answer — and the specials folds do not count it (an `Int` is
    /// not a float result).
    #[test]
    fn the_comparison_face_reports_the_int_answer() {
        let module = comparison_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(1));
        assert_eq!(collector.events().len(), 3);
        let binop = collector.binop_events()[0];
        assert_eq!(binop.op, Opcode::Lt);
        assert_eq!(binop.result, Value::Int(1), "the machine's bool encoding");
        assert_eq!(collector.nan_results(), 0);
        assert_eq!(collector.inf_results(), 0);
    }

    /// The unary face reports the IEEE negation: `-1.5` for `1.5`, and
    /// the specials folds stay at zero (a finite negation is neither
    /// NaN nor inf).
    #[test]
    fn the_neg_face_reports_the_negation() {
        let module = neg_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Float(-1.5));
        assert_eq!(collector.events().len(), 2);
        let neg = collector.neg_events()[0];
        assert_eq!(neg.func, 0);
        assert_eq!(neg.pc, 9, "the OP_NEG opcode's byte offset");
        assert_eq!(neg.operand, Value::Float(1.5));
        assert_eq!(neg.result, Value::Float(-1.5));
        assert_eq!(collector.neg_count(), 1);
        assert_eq!(collector.nan_results(), 0);
        assert_eq!(collector.inf_results(), 0);
    }

    /// A float operation the machine refuses (a mixed `(Int, Float)`
    /// binop, a bitwise op over floats) reports nothing — the fault
    /// stream owns the refusal — and the run faults with exactly the
    /// pre-seam error. The *executed* constant pushes before the
    /// refusal still report (the seam is total over the executed
    /// stream, and those pushes executed) — what never appears is the
    /// refused binop itself.
    #[test]
    fn a_refused_float_op_reports_nothing() {
        let mixed = mixed_kind_module();
        let (collector, outcome) = traced_run(&mixed);
        assert_eq!(
            outcome.expect_err("the mixed-kind binop faults"),
            VmError::BinOpTypeMismatch {
                op: "Add",
                left: "Int",
                right: "Float",
            }
        );
        assert_eq!(collector.binop_count(), 0, "no event for the refused binop");
        assert_eq!(
            collector.const_count(),
            1,
            "the float constant push executed and reported"
        );
        let bitwise = float_bitwise_module();
        let (collector, outcome) = traced_run(&bitwise);
        assert_eq!(
            outcome.expect_err("the float bitwise op faults"),
            VmError::BinOpTypeMismatch {
                op: "BitAnd",
                left: "Float",
                right: "Float",
            }
        );
        assert_eq!(collector.binop_count(), 0, "no event for the refused binop");
        assert_eq!(
            collector.const_count(),
            2,
            "both constant pushes executed and reported"
        );
    }

    /// The render is deterministic and pins the transcript line
    /// spellings: the header tallies, the const lines, and the binop
    /// line with its opcode, operands, and bit-exact answer spelling.
    #[test]
    fn render_is_deterministic_golden() {
        let module = kernel_module();
        let (collector, _) = traced_run(&module);
        let first = render_floats(&collector);
        let second = render_floats(&collector);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
floats glyim.module-image/2 consts=2 binops=1 negs=0 nan=0 inf=0
0: const fn 0 pc 0 value=1.5 (bits 0x3ff8000000000000)
1: const fn 0 pc 9 value=2.25 (bits 0x4002000000000000)
2: binop fn 0 pc 18 op Mul lhs=1.5 (bits 0x3ff8000000000000) rhs=2.25 (bits 0x4002000000000000) -> 3.375 (bits 0x400b000000000000)
"
        );
        let neg = neg_module();
        let (collector, _) = traced_run(&neg);
        assert_eq!(
            render_floats(&collector),
            "\
floats glyim.module-image/2 consts=1 binops=0 negs=1 nan=0 inf=0
0: const fn 0 pc 0 value=1.5 (bits 0x3ff8000000000000)
1: neg fn 0 pc 9 operand=1.5 (bits 0x3ff8000000000000) -> -1.5 (bits 0xbff8000000000000)
"
        );
    }

    /// The specials show in the render's header tallies (the
    /// specials fold is where the convention path is audited). The
    /// infinite answer's bit pattern is pinned (exact by the
    /// convention); the NaN line is matched by its spelling prefix —
    /// the invalid-operation quiet NaN's payload bits are
    /// platform-defined.
    #[test]
    fn the_specials_show_in_the_render_header() {
        let module = specials_module();
        let (collector, _) = traced_run(&module);
        let rendered = render_floats(&collector);
        assert!(
            rendered
                .starts_with("floats glyim.module-image/2 consts=3 binops=2 negs=0 nan=1 inf=1\n")
        );
        assert!(rendered.contains("-> inf (bits 0x7ff0000000000000)"));
        assert!(
            rendered.contains("-> NaN (bits 0x"),
            "the NaN line spells its bits: {rendered}"
        );
    }

    /// A float-free straight line reports nothing — the seam is total
    /// over the *executed* float stream, floats or not — and the
    /// render says so explicitly.
    #[test]
    fn a_float_free_run_reports_nothing() {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0
        main.extend_from_slice(&i64le(5));
        main.push(op(Opcode::Return)); // pc 9
        let module = Module::new(vec![Function::new(main, 0, 0)], 0);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(5));
        assert!(collector.is_empty());
        assert_eq!(
            render_floats(&collector),
            "\
floats glyim.module-image/2 consts=0 binops=0 negs=0 nan=0 inf=0
no float events
"
        );
    }

    /// A float-heavy loop reports one event per executed float
    /// operation, in execution order — the accumulator's initial
    /// constant push plus four per iteration here — and the
    /// accumulator's answer is exact.
    #[test]
    fn the_float_loop_reports_one_event_per_op() {
        let module = float_loop_module(2);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Float(6.75));
        assert_eq!(
            collector.events().len(),
            9,
            "the accumulator's initial push plus four per iteration"
        );
        assert_eq!(collector.const_count(), 5);
        assert_eq!(collector.binop_count(), 4);
        assert_eq!(collector.neg_count(), 0);
        assert_eq!(collector.nan_results(), 0);
        assert_eq!(collector.inf_results(), 0);
        // The binops interleave: per iteration a Mul then an Add, the
        // accumulator growing by 3.375 each round.
        let binops = collector.binop_events();
        assert_eq!(binops[0].op, Opcode::Mul);
        assert_eq!(binops[1].result, Value::Float(3.375));
        assert_eq!(binops[2].op, Opcode::Mul);
        assert_eq!(binops[3].result, Value::Float(6.75));
    }

    /// A bounded collector stops at the cap and flags overflow
    /// (displayed, never a silent drop).
    #[test]
    fn bounded_collector_flags_overflow() {
        let module = float_loop_module(2);
        let collector = super::FloatCollector::bounded(5);
        let mut vm = Vm::new().with_floats(Box::new(collector));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_floats().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<super::FloatCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.events().len(), 5, "the cap holds");
        assert!(collector.overflowed(), "the overflow is displayed");
    }

    /// The seam is read-only: the traced run's value equals the
    /// untraced one exactly, and the faulting run's error equals the
    /// untraced one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let kernel = kernel_module();
        let untraced = Vm::new().run_module(&kernel).expect("untraced run");
        let (_, traced) = traced_run(&kernel);
        assert_eq!(traced.expect("traced run"), untraced);
        let looped = float_loop_module(3);
        let untraced = Vm::new().run_module(&looped).expect("untraced run");
        let (_, traced) = traced_run(&looped);
        assert_eq!(traced.expect("traced run"), untraced);
        let faulting = mixed_kind_module();
        let untraced = Vm::new()
            .run_module(&faulting)
            .expect_err("untraced faulting run");
        let (_, traced) = traced_run(&faulting);
        assert_eq!(traced.expect_err("traced faulting run"), untraced);
    }

    /// The event types are comparable (the golden test's `PartialEq`
    /// requirement) and every field is inspectable — the transcript's
    /// three faces are distinct.
    #[test]
    fn events_are_structurally_comparable() {
        let c = super::FloatConst {
            func: 0,
            pc: 0,
            value: 1.5,
        };
        let mut c2 = c.clone();
        assert_eq!(c, c2);
        c2.value = 2.5;
        assert_ne!(c, c2);
        let b = super::FloatBinop {
            func: 0,
            pc: 18,
            op: Opcode::Mul,
            lhs: Value::Float(1.5),
            rhs: Value::Float(2.25),
            result: Value::Float(3.375),
        };
        let mut b2 = b.clone();
        assert_eq!(b, b2);
        b2.result = Value::Float(4.0);
        assert_ne!(b, b2);
        let n = super::FloatNeg {
            func: 0,
            pc: 9,
            operand: Value::Float(1.5),
            result: Value::Float(-1.5),
        };
        assert_ne!(
            super::FloatEvent::Const(c.clone()),
            super::FloatEvent::Binop(b.clone()),
            "the transcript's faces are distinct"
        );
        assert_ne!(
            super::FloatEvent::Binop(b.clone()),
            super::FloatEvent::Neg(n.clone()),
            "the transcript's faces are distinct"
        );
    }

    /// The zero-sign face: negating `0.0` yields `-0.0`, a distinct
    /// bit pattern the render spells bit-exactly (the specials folds
    /// stay at zero — `-0.0` is finite).
    #[test]
    fn the_neg_zero_face_stays_bit_exact() {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConstF64)); // pc 0
        main.extend_from_slice(&f64le(0.0));
        main.push(op(Opcode::Neg)); // pc 9
        main.push(op(Opcode::Return)); // pc 10
        let module = Module::new(vec![Function::new(main, 0, 0)], 0);
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Float(-0.0));
        let neg = collector.neg_events()[0];
        assert_eq!(neg.result, Value::Float(-0.0));
        assert_eq!(collector.nan_results(), 0);
        assert_eq!(collector.inf_results(), 0);
    }
}
