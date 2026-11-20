//! Operand-stack seam: what values moved through the machine's operand
//! stack, movement by movement?
//!
//! The trace seam ([`crate::trace`]) answers *where* the machine went and
//! the watch seam ([`crate::watch`]) what its local slots *took* — but the
//! operand stack, the channel every opcode's operands and answers actually
//! travel through, has been observable only as a depth number
//! ([`crate::trace::VmStep::stack_depth`]). A `VmError::BinOpTypeMismatch`
//! names the kinds that collided; a *silent* wrong answer — a binop that
//! consumed a neighbouring kind and produced a plausible Int — leaves
//! nothing behind. This module is the machine's value-movement
//! counterpart: the VM fires [`StackSink::operand_pushed`] just before a
//! value lands on the operand stack and [`StackSink::operand_popped`] just
//! after a pop resolves, handing one event per executed operand-stack
//! movement to a sink the caller attaches.

use std::any::Any;
use std::fmt::Write as _;

use tracing::debug;

use crate::{Module, Value};

/// One operand-stack push, handed to the attached [`StackSink`] just
/// before the value lands on the stack (the machine's own push happens
/// right after the hook returns): the site, the movement's dispatch
/// number, the machine's depths, and the value in full.
///
/// `depth` is the operand-stack depth the movement *produces* — the
/// value's landing depth (`pre + 1`), the same convention a [`Popped`]'s
/// `depth` follows in reverse (what remains after the pop). Pair the two
/// and every stack shape reads directly off the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Pushed {
    /// The function index containing the push site.
    pub func: usize,
    /// The byte offset of the executing instruction (the push site — the
    /// opcode whose arm produces the value).
    pub pc: usize,
    /// The producing dispatch's 1-based lifetime number (the same count
    /// [`crate::Vm::executed`] reports after the run).
    pub dispatch: u64,
    /// Depth of the frame that executes the producing dispatch (1 = the
    /// entry frame), which disambiguates two same-function frames
    /// (recursion).
    pub frame_depth: usize,
    /// The operand-stack depth the push produces (the value's landing
    /// depth).
    pub depth: usize,
    /// The value about to land (a sink-side copy — the machine pushes its
    /// own value right after the hook returns; the copies-into-the-event
    /// shape every value-carrying seam follows).
    pub value: Value,
}

/// One operand-stack pop, handed to the attached [`StackSink`] just after
/// the pop resolves (a pop that *fails* — [`crate::VmError::StackUnderflow`]
/// — pops nothing and reports nothing: the fault stream owns the refusal):
/// the site, the movement's dispatch number, the machine's depths, and the
/// value in full.
///
/// `depth` is what remains on the stack after the pop (`pre - 1`), the
/// mirror of a [`Pushed`]'s landing depth. The unit-return's empty-stack
/// pop is a failed pop: the machine substitutes its `Unit` encoding
/// without a movement, so the stream stays silent there (the call seams'
/// events cover the dest write).
#[derive(Debug, Clone, PartialEq)]
pub struct Popped {
    /// The function index containing the pop site.
    pub func: usize,
    /// The byte offset of the executing instruction (the pop site — the
    /// opcode whose arm consumes the value; for an argument pop, the
    /// `OP_CALL` itself).
    pub pc: usize,
    /// The producing dispatch's 1-based lifetime number (the same count
    /// [`crate::Vm::executed`] reports after the run).
    pub dispatch: u64,
    /// Depth of the frame that executes the producing dispatch (1 = the
    /// entry frame). A `Return`'s retval pop reports while the returning
    /// frame is still current (the callee's depth).
    pub frame_depth: usize,
    /// The operand-stack depth that remains after the pop.
    pub depth: usize,
    /// The popped value (a sink-side copy — the machine's own value moves
    /// into the consuming arm).
    pub value: Value,
}

/// The interleaved movement transcript: one entry per executed
/// operand-stack movement, in execution order — the shape
/// [`StackCollector::events`] returns and [`render_stack`] lists.
#[derive(Debug, Clone, PartialEq)]
pub enum StackEvent {
    /// A push ([`Pushed`]).
    Pushed(Pushed),
    /// A pop ([`Popped`]).
    Popped(Popped),
}

impl StackEvent {
    /// The executing function's index (either movement face).
    #[must_use]
    pub fn func(&self) -> usize {
        match self {
            StackEvent::Pushed(e) => e.func,
            StackEvent::Popped(e) => e.func,
        }
    }

    /// The executing instruction's byte offset (either movement face).
    #[must_use]
    pub fn pc(&self) -> usize {
        match self {
            StackEvent::Pushed(e) => e.pc,
            StackEvent::Popped(e) => e.pc,
        }
    }

    /// The producing dispatch's 1-based lifetime number (either movement
    /// face).
    #[must_use]
    pub fn dispatch(&self) -> u64 {
        match self {
            StackEvent::Pushed(e) => e.dispatch,
            StackEvent::Popped(e) => e.dispatch,
        }
    }

    /// The frame depth the movement reported (either movement face).
    #[must_use]
    pub fn frame_depth(&self) -> usize {
        match self {
            StackEvent::Pushed(e) => e.frame_depth,
            StackEvent::Popped(e) => e.frame_depth,
        }
    }

    /// The operand-stack depth the movement produced (either movement
    /// face).
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            StackEvent::Pushed(e) => e.depth,
            StackEvent::Popped(e) => e.depth,
        }
    }

    /// The moved value (either movement face).
    #[must_use]
    pub fn value(&self) -> &Value {
        match self {
            StackEvent::Pushed(e) => &e.value,
            StackEvent::Popped(e) => &e.value,
        }
    }
}

/// The observer the VM hands each operand-stack movement to.
///
/// Implementors must treat [`StackSink::operand_pushed`] /
/// [`StackSink::operand_popped`] as observations only: the VM's state and
/// control flow are invisible to the sink and unaffected by it. The
/// methods must not panic (the VM never defends against a panicking sink —
/// a sink that panics aborts the run, which is the embedding host's
/// explicit choice). The [`StackSink::as_any`] hook lets a caller recover
/// the typed sink after [`crate::Vm::take_stack`].
pub trait StackSink: 'static {
    /// One push, reported just before the value lands on the stack (a
    /// value the machine refuses — a faulting arm — never reaches the
    /// hook).
    fn operand_pushed(&mut self, event: &Pushed);

    /// One pop, reported just after the pop resolves (a pop that faults —
    /// a [`crate::VmError::StackUnderflow`] — reports nothing).
    fn operand_popped(&mut self, event: &Popped);

    /// Downcast hook: hand the sink back to its typed owner after
    /// `Vm::take_stack`.
    fn as_any(&mut self) -> &mut dyn Any;
}

/// A sink that captures every operand-stack movement in memory.
///
/// `StackCollector::new()` is unbounded; `StackCollector::bounded(max)`
/// stops capturing at the cap and sets the `overflowed` flag instead of
/// growing without limit — a loop moving operands every dispatch must not
/// OOM the embedding host. The flag is a deliberate, *displayed* fact,
/// never a silent drop.
#[derive(Clone, Default)]
pub struct StackCollector {
    /// The capture cap (`None` = unbounded).
    cap: Option<usize>,
    /// The captured movements, in execution order.
    events: Vec<StackEvent>,
    /// Whether any movement was dropped because the cap was reached.
    overflowed: bool,
}

impl StackCollector {
    /// Create an unbounded collector (every movement of the run is
    /// captured).
    #[must_use]
    pub fn new() -> StackCollector {
        StackCollector::default()
    }

    /// Create a collector that captures at most `max_events` movements and
    /// marks the overflow instead of growing without limit.
    #[must_use]
    pub fn bounded(max_events: usize) -> StackCollector {
        StackCollector {
            cap: Some(max_events),
            events: Vec::new(),
            overflowed: false,
        }
    }

    /// The captured movements, in execution order (a bounded collector
    /// holds at most its cap).
    #[must_use]
    pub fn events(&self) -> &[StackEvent] {
        &self.events
    }

    /// Consume the collector into its movements.
    #[must_use]
    pub fn into_events(self) -> Vec<StackEvent> {
        self.events
    }

    /// How many movements were captured.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Whether any movement was dropped because the cap was reached.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// The captured pushes, in execution order (the push face of the
    /// interleaved transcript).
    #[must_use]
    pub fn pushed_events(&self) -> Vec<&Pushed> {
        self.events
            .iter()
            .filter_map(|e| match e {
                StackEvent::Pushed(p) => Some(p),
                StackEvent::Popped(_) => None,
            })
            .collect()
    }

    /// The captured pops, in execution order (the pop face of the
    /// interleaved transcript).
    #[must_use]
    pub fn popped_events(&self) -> Vec<&Popped> {
        self.events
            .iter()
            .filter_map(|e| match e {
                StackEvent::Popped(p) => Some(p),
                StackEvent::Pushed(_) => None,
            })
            .collect()
    }

    /// How many pushes were captured.
    #[must_use]
    pub fn pushed_count(&self) -> usize {
        self.pushed_events().len()
    }

    /// How many pops were captured.
    #[must_use]
    pub fn popped_count(&self) -> usize {
        self.popped_events().len()
    }

    /// Pushes minus pops over the captured stream: `0` for a run that
    /// ends as it started (every balanced program), positive for a run
    /// truncated mid-shape (a fault or a bounded capture's overflow).
    #[must_use]
    pub fn net_growth(&self) -> i64 {
        self.pushed_count() as i64 - self.popped_count() as i64
    }

    /// The highest landing depth any captured movement reported — the
    /// run's operand-stack high-water mark as the machine itself shaped
    /// it (`0` for an empty capture).
    #[must_use]
    pub fn max_depth_seen(&self) -> usize {
        self.events.iter().map(|e| e.depth()).max().unwrap_or(0)
    }
}

impl StackSink for StackCollector {
    fn operand_pushed(&mut self, event: &Pushed) {
        self.capture(StackEvent::Pushed(event.clone()));
    }

    fn operand_popped(&mut self, event: &Popped) {
        self.capture(StackEvent::Popped(event.clone()));
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

impl StackCollector {
    /// Capture one movement (the shared cap discipline of both faces).
    fn capture(&mut self, event: StackEvent) {
        if let Some(cap) = self.cap {
            if self.events.len() >= cap {
                self.overflowed = true;
                return;
            }
        }
        self.events.push(event);
    }
}

/// A sink that emits one `tracing::debug!` event per operand-stack
/// movement — the seam's counterpart for the workspace-standard
/// observability layer (the same `tracing` dependency every other seam
/// logs through). Enable it per subscriber, never by paying the cost
/// unconditionally.
pub struct TracingStackSink;

impl StackSink for TracingStackSink {
    fn operand_pushed(&mut self, event: &Pushed) {
        debug!(
            func = event.func,
            pc = event.pc,
            dispatch = event.dispatch,
            frame_depth = event.frame_depth,
            depth = event.depth,
            kind = event.value.kind_name(),
            "vm operand pushed"
        );
    }

    fn operand_popped(&mut self, event: &Popped) {
        debug!(
            func = event.func,
            pc = event.pc,
            dispatch = event.dispatch,
            frame_depth = event.frame_depth,
            depth = event.depth,
            kind = event.value.kind_name(),
            "vm operand popped"
        );
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a captured operand-stack stream into a deterministic listing: a
/// header line (image format version, captured movement count), then one
/// line per movement — movement index, the face, the executing function
/// and pc, the rendered value, the depth the movement produced, and the
/// producing dispatch number. An empty capture renders the zero case (the
/// header alone).
///
/// Deterministic (the same event list renders byte-identical text — the
/// test suite pins a golden) and total (any event list renders). `module`
/// is accepted for signature symmetry with the other seams' renderers (the
/// trace's pool-resolving render); the stack listing's values are
/// self-describing and need no pool.
#[must_use]
pub fn render_stack(events: &[StackEvent], module: &Module) -> String {
    let _ = module;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "stack glyim.module-image/{} movements={}",
        crate::IMAGE_VERSION,
        events.len()
    );
    for (i, event) in events.iter().enumerate() {
        let face = match event {
            StackEvent::Pushed(_) => "push",
            StackEvent::Popped(_) => "pop",
        };
        let _ = writeln!(
            out,
            "{i}: {face} fn {} pc {} = {} depth {} dispatch {}",
            event.func(),
            event.pc(),
            crate::watch::render_value(event.value()),
            event.depth(),
            event.dispatch(),
        );
    }
    debug!(movements = events.len(), "vm stack rendered");
    out
}

#[cfg(test)]
mod stack_tests {
    use crate::stack::{StackCollector, StackSink, TracingStackSink, render_stack};
    use crate::trace::TraceCollector;
    use crate::{Function, Module, Opcode, Value, Vm, VmError, decode_module, encode_module};
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

    /// helper(n) = n * 2 + main returning helper(21): the disasm/trace
    /// golden module's exact shape. Its operand-stack stream is the
    /// call-return round trip: the call's two pops (the argument and the
    /// call target), the callee's arithmetic pops/push, the return's
    /// retval pop, and the caller's reload.
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

    /// A countdown loop over local 0 — the movement-heavy shape: every
    /// iteration pushes and pops the counter twice through the binop, the
    /// store, the compare, and the branch.
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

    /// Run a module with a stack collector attached; the value must match
    /// the unattached result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_stack`.
    fn watched_run(module: &Module) -> (StackCollector, Value) {
        let collector = StackCollector::new();
        let mut vm = Vm::new().with_stack(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_stack().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<StackCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// The golden module's movement stream, pinned end to end: 9 dispatches
    /// move 12 operands (6 pushes, 6 pops, net zero). The pops name their
    /// operand's role by position — the binop pops its right operand
    /// first — and every event carries the machine's depths at the
    /// movement (frame depth 2 while the callee is current, 1 after).
    #[test]
    fn golden_pinned_value_stream() {
        let module = golden_module();
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(42));
        let events = collector.events();
        assert_eq!(events.len(), 12, "9 dispatches move 12 operands");
        assert_eq!(collector.pushed_count(), 6);
        assert_eq!(collector.popped_count(), 6);
        assert_eq!(collector.net_growth(), 0, "the stream ends balanced");
        assert_eq!(collector.max_depth_seen(), 2);

        // (kind, func, pc, int value, depth after, dispatch, frame depth)
        let descs: Vec<(&str, usize, usize, i64, usize, u64, usize)> = events
            .iter()
            .map(|e| match e {
                crate::stack::StackEvent::Pushed(p) => (
                    "push",
                    p.func,
                    p.pc,
                    match p.value {
                        Value::Int(n) => n,
                        _ => panic!("int movements only in the golden module"),
                    },
                    p.depth,
                    p.dispatch,
                    p.frame_depth,
                ),
                crate::stack::StackEvent::Popped(p) => (
                    "pop",
                    p.func,
                    p.pc,
                    match p.value {
                        Value::Int(n) => n,
                        _ => panic!("int movements only in the golden module"),
                    },
                    p.depth,
                    p.dispatch,
                    p.frame_depth,
                ),
            })
            .collect();
        assert_eq!(
            descs,
            vec![
                ("push", 1, 0, 0, 1, 1, 1),   // main: fn-index constant
                ("push", 1, 9, 21, 2, 2, 1),  // main: the argument 21
                ("pop", 1, 18, 21, 1, 3, 1),  // call: the argument (rhs first)
                ("pop", 1, 18, 0, 0, 3, 1),   // call: the target
                ("push", 0, 0, 2, 1, 4, 2),   // helper: the multiplier 2
                ("push", 0, 9, 21, 2, 5, 2),  // helper: reload the arg
                ("pop", 0, 14, 21, 1, 6, 2),  // mul: the right operand (top)
                ("pop", 0, 14, 2, 0, 6, 2),   // mul: the left operand
                ("push", 0, 14, 42, 1, 6, 2), // mul: the answer
                ("pop", 0, 15, 42, 0, 7, 2),  // return: the retval
                ("push", 1, 31, 42, 1, 8, 1), // main: reload the dest
                ("pop", 1, 36, 42, 0, 9, 1),  // return: the module result
            ],
            "every movement keyed by site, depths, and dispatch number"
        );

        // The filtered views read the two faces back separately.
        assert_eq!(collector.pushed_events().len(), 6);
        assert_eq!(collector.popped_events().len(), 6);

        // The render pins the whole listing byte-identically.
        let rendered = render_stack(events, &module);
        assert_eq!(
            rendered,
            "\
stack glyim.module-image/2 movements=12
0: push fn 1 pc 0 = 0 depth 1 dispatch 1
1: push fn 1 pc 9 = 21 depth 2 dispatch 2
2: pop fn 1 pc 18 = 21 depth 1 dispatch 3
3: pop fn 1 pc 18 = 0 depth 0 dispatch 3
4: push fn 0 pc 0 = 2 depth 1 dispatch 4
5: push fn 0 pc 9 = 21 depth 2 dispatch 5
6: pop fn 0 pc 14 = 21 depth 1 dispatch 6
7: pop fn 0 pc 14 = 2 depth 0 dispatch 6
8: push fn 0 pc 14 = 42 depth 1 dispatch 6
9: pop fn 0 pc 15 = 42 depth 0 dispatch 7
10: push fn 1 pc 31 = 42 depth 1 dispatch 8
11: pop fn 1 pc 36 = 42 depth 0 dispatch 9
"
        );
    }

    /// A movement-heavy loop records every iteration's stream in dispatch
    /// order — the preamble's seed movement, then the same ten-movement
    /// pattern per iteration (load, load, binop, store, load, load,
    /// compare, branch), then the exit tail — and the counts fold back:
    /// the stream ends balanced at the returned value.
    #[test]
    fn countdown_movements_are_recorded_in_order() {
        let module = countdown_module(2);
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(0));
        let events = collector.events();
        // Preamble: push the seed, pop it into the store. Each iteration
        // is twelve movements (two loads, the binop's two pops and push,
        // the store's pop, the compare's two pops and push, the branch's
        // pop); two iterations, then the exit tail's load and return.
        assert_eq!(
            events.len(),
            28,
            "2 preamble + 12 per iteration x2 + 2 tail"
        );
        assert_eq!(collector.pushed_count(), 14);
        assert_eq!(collector.popped_count(), 14);
        assert_eq!(collector.net_growth(), 0);
        assert_eq!(collector.max_depth_seen(), 2);
        // The first four movements: seed push, seed pop, first load, the
        // loop's constant.
        let kinds: Vec<&str> = events
            .iter()
            .take(4)
            .map(|e| match e {
                crate::stack::StackEvent::Pushed(_) => "push",
                crate::stack::StackEvent::Popped(_) => "pop",
            })
            .collect();
        assert_eq!(kinds, vec!["push", "pop", "push", "push"]);
        // The stream ends with the exit tail: push local 0 (0), the
        // return's pop of it.
        let last = &events[events.len() - 1];
        match last {
            crate::stack::StackEvent::Popped(p) => {
                assert_eq!(p.value, Value::Int(0));
                assert_eq!(p.depth, 0, "the run ends on an empty stack");
                assert_eq!(p.dispatch, 20, "the 20th dispatch is the return");
            }
            _ => panic!("the last movement is the return's pop"),
        }
    }

    /// The stack is read-only: the watched result equals the unwatched
    /// result, and the event copies do not perturb execution.
    #[test]
    fn watched_result_matches_unwatched() {
        let module = golden_module();
        let unwatched = Vm::new().run_module(&module).expect("unwatched runs");
        let (collector, watched) = watched_run(&module);
        assert_eq!(watched, unwatched);
        assert_eq!(collector.len(), 12);
        assert!(!collector.overflowed());
    }

    /// A VM without a sink behaves exactly as before: no events, no panic.
    #[test]
    fn no_sink_by_default() {
        let module = golden_module();
        let mut vm = Vm::new();
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        assert!(vm.take_stack().is_none());
    }

    /// A bounded collector marks overflow instead of growing without limit
    /// (a loop moving operands every dispatch must not OOM the embedding
    /// host).
    #[test]
    fn bounded_collector_marks_overflow() {
        let module = countdown_module(5);
        let collector = StackCollector::bounded(2);
        let mut vm = Vm::new().with_stack(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(0));
        let mut boxed = vm.take_stack().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<StackCollector>().expect("collector");
        assert_eq!(collector.len(), 2, "capture stops at the cap");
        assert!(collector.overflowed());
    }

    /// Every value kind renders: floats with their raw IEEE-754 bits, pool
    /// strings quoted, tuples parenthesized and recursive — the same
    /// machine spellings the watch and state renders use.
    #[test]
    fn value_spellings_render() {
        let mut code = Vec::new();
        // push 1.5 (float bits visible in the render)
        code.push(op(Opcode::LoadConstF64));
        code.extend_from_slice(&1.5f64.to_bits().to_le_bytes());
        // push "hello" (pool string)
        code.push(op(Opcode::LoadConstStr));
        code.extend_from_slice(&u32le(0));
        // push (1, (2, 3)) (nested aggregate)
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(3));
        code.push(op(Opcode::Aggregate));
        code.extend_from_slice(&u32le(2));
        code.push(op(Opcode::Aggregate));
        code.extend_from_slice(&u32le(2));
        code.push(op(Opcode::Return));
        let module = Module::with_strings(
            vec![Function::new(code, 0, 0)],
            vec!["hello".to_string()],
            0,
        );
        let (collector, _) = watched_run(&module);
        let rendered = render_stack(collector.events(), &module);
        assert!(
            rendered.contains("push fn 0 pc 0 = 1.5 (bits 0x3ff8000000000000)"),
            "float bits visible in {rendered}"
        );
        assert!(
            rendered.contains("push fn 0 pc 9 = \"hello\""),
            "quoted string in {rendered}"
        );
        assert!(
            rendered.contains("push fn 0 pc 41 = (2, 3)"),
            "the inner tuple renders recursively in {rendered}"
        );
        assert!(
            rendered.contains("push fn 0 pc 46 = (1, (2, 3))"),
            "the outer tuple renders recursively in {rendered}"
        );
    }

    /// The trace seam and the stack seam compose on one run: separate
    /// hooks, both sinks come back typed, and the step count is unchanged
    /// by the movement stream.
    #[test]
    fn stack_composes_with_trace() {
        let module = golden_module();
        let collector = StackCollector::new();
        let trace = TraceCollector::new();
        let mut vm = Vm::new()
            .with_trace(Box::new(trace))
            .with_stack(Box::new(collector));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        let mut traced = vm.take_trace().expect("trace attached");
        let steps = traced
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("trace collector")
            .len();
        assert_eq!(steps, 9, "the golden module dispatches 9 steps");
        let mut stacked = vm.take_stack().expect("stack attached");
        let movements = stacked
            .as_any()
            .downcast_ref::<StackCollector>()
            .expect("stack collector")
            .len();
        assert_eq!(movements, 12, "the golden stream, as in the golden test");
    }

    /// A fuel-refused dispatch never executes and never moves an operand:
    /// the stream ends exactly at the last executed dispatch's movements.
    #[test]
    fn fuel_refused_dispatch_moves_nothing() {
        let module = countdown_module(10);
        let collector = StackCollector::new();
        let mut vm = Vm::new().with_fuel(3).with_stack(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the loop burns the budget");
        assert_eq!(err, VmError::FuelExhausted { executed: 3 });
        let mut boxed = vm.take_stack().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<StackCollector>().expect("collector");
        // Dispatches 1-3 executed (the seed push, its pop, the first loop
        // load's push); dispatch 4 was refused and moved nothing.
        assert_eq!(collector.len(), 3, "the three executed movements only");
    }

    /// The image round trip composes with the seam: decode the shipped
    /// bytes, then watch the loaded module's operand stream.
    #[test]
    fn stack_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let (collector, value) = watched_run(&loaded);
        assert_eq!(value, Value::Int(42));
        assert_eq!(collector.len(), 12, "the golden stream over the image");
    }

    /// A pop that fails — a binop reaching for a second operand that is
    /// not there — pops nothing and reports nothing: the seam is total
    /// over the *executed* movements, and the fault stream owns the
    /// underflow.
    #[test]
    fn failed_pop_reports_nothing() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Add)); // one operand on the stack, two needed
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let collector = StackCollector::new();
        let mut vm = Vm::new().with_stack(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the second pop underflows");
        assert_eq!(err, VmError::StackUnderflow);
        let mut boxed = vm.take_stack().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<StackCollector>().expect("collector");
        assert_eq!(
            collector.len(),
            2,
            "the constant's push and the first operand's pop; the failed pop is silent"
        );
    }

    /// A unit-returning function's `Return` finds an empty stack and
    /// substitutes the machine's `Unit` encoding without a movement: the
    /// substituted value never rides the operand stack, so the stream
    /// stays silent (the call seams' events cover the dest write).
    #[test]
    fn unit_return_moves_nothing() {
        let code = vec![op(Opcode::Return)];
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(0));
        assert!(
            collector.is_empty(),
            "a unit return performs no operand movement"
        );
        assert_eq!(collector.max_depth_seen(), 0);
    }

    /// The render is deterministic: the same event list renders
    /// byte-identical text.
    #[test]
    fn render_is_deterministic() {
        let module = golden_module();
        let (collector, _) = watched_run(&module);
        let events = collector.events();
        assert_eq!(render_stack(events, &module), render_stack(events, &module));
    }

    /// An empty capture renders the zero case (displayable, not an error).
    #[test]
    fn empty_capture_renders_zero_movements() {
        let module = golden_module();
        let rendered = render_stack(&[], &module);
        assert_eq!(rendered, "stack glyim.module-image/2 movements=0\n");
    }

    /// `TracingStackSink` is total: attaching it changes nothing about the
    /// run and never panics (with or without a subscriber installed).
    #[test]
    fn tracing_stack_sink_is_total() {
        let module = golden_module();
        let mut vm = Vm::new().with_stack(Box::new(TracingStackSink));
        let value = vm.run_module(&module).expect("runs");
        assert_eq!(value, Value::Int(42));
        // An event can also be delivered directly.
        TracingStackSink.operand_pushed(&crate::stack::Pushed {
            func: 0,
            pc: 0,
            dispatch: 1,
            frame_depth: 1,
            depth: 1,
            value: Value::Int(1),
        });
        TracingStackSink.operand_popped(&crate::stack::Popped {
            func: 0,
            pc: 0,
            dispatch: 2,
            frame_depth: 1,
            depth: 0,
            value: Value::Int(1),
        });
    }

    /// Same-function frames are disambiguated by frame depth: a recursive
    /// helper's movements report the callee's depth (2) while its frame is
    /// current, the caller's at 1 — the same convention the watch events
    /// follow.
    #[test]
    fn same_function_frames_carry_their_depth() {
        // helper(n): local 0 = n + 1; return local 0.
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(1));
        helper.push(op(Opcode::Add));
        helper.push(op(Opcode::StoreLocal));
        helper.extend_from_slice(&u32le(0));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(0));
        helper.push(op(Opcode::Return));
        // main: helper(helper(9)) — the inner call's return feeds the
        // outer call, so both callee frames live at once (depths 2 and 3).
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(9));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(1)); // dest local 1 of main
        main.extend_from_slice(&u32le(31)); // resume (byte offset mode)
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: helper
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(1)); // the inner result is the arg
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local 0 of main
        main.extend_from_slice(&u32le(58)); // resume (byte offset mode)
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        let module = Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 2, 0)],
            1,
        );
        let (collector, value) = watched_run(&module);
        assert_eq!(value, Value::Int(11), "helper(helper(9)) == 11");
        let events = collector.events();
        // The two helper frames are *sequential* (the inner call returns
        // before the outer one starts), so both run at callee depth 2 —
        // the frame_depth field is what tells them apart from the
        // caller's depth-1 movements, and the dispatch numbers order
        // them.
        let max_frame = events
            .iter()
            .map(|e| e.frame_depth())
            .max()
            .expect("events");
        assert_eq!(max_frame, 2, "the callee frames run at depth 2");
        assert_eq!(collector.max_depth_seen(), 2, "two operands, never three");
    }
}
