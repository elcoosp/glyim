//! Call-graph seam: which functions called which, and how often?
//!
//! The profile seam ([`crate::profile`]) aggregates the dispatch stream by
//! *opcode* and *function*; it cannot answer the *structural-dynamic*
//! question — of a run's calls, which caller entered which callee, how
//! many times, and which call sites never reached their callee. This
//! module is the call-graph counterpart: a [`crate::trace::TraceSink`]
//! that folds the same per-dispatch stream into per-`(caller, callee)`
//! edge counts, keyed by function index, in bounded memory.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change execution), **total** (every dispatch is folded in,
//! including the faulting dispatch of a failed run), **bounded** (the
//! collector's memory is a function of the module's function count, never
//! of the run length), and **deterministic** ([`render_call_graph`]
//! renders the same edge map byte-identically — the test suite pins a
//! golden).
//!
//! # The callee-naming contract
//!
//! The trace hook fires *before* an instruction executes, and the `Call`
//! wire payload carries the arity, the destination local, and the resume
//! target — not the callee (the callee index sits on the operand stack at
//! dispatch time, where the read-only sink cannot see it). The fold
//! therefore names a callee by the step that *follows* the call: a
//! successful call dispatches into the callee's entry instruction, whose
//! step sits exactly one frame deeper. A call whose callee never
//! dispatches (an out-of-range callee index, a faulting entry instruction,
//! a callee with empty code) leaves no such step; the call is counted in
//! [`CallGraphCollector::calls`] but in no edge, so
//! [`CallGraphCollector::unresolved_calls`] reports it — displayed, never
//! silently dropped.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::trace::{TraceSink, VmStep};
use tracing::debug;

/// The call-graph fold of the trace stream: dispatch counts per
/// `(caller, callee)` edge. Attach through [`crate::Vm::with_trace`],
/// recover through [`crate::Vm::take_trace`] +
/// [`crate::trace::TraceSink::as_any`], re-aggregate a captured
/// [`crate::TraceCollector`] transcript post-hoc with
/// [`CallGraphCollector::from_steps`], and pair it with
/// [`crate::Vm::with_fuel`] — a fuel-refused dispatch never reaches the
/// hook, so the graph ends exactly at the executed calls.
#[derive(Clone, Default)]
pub struct CallGraphCollector {
    /// Dispatch counts per `(caller, callee)` function-index pair, in
    /// ascending key order (a `BTreeMap`, so the render's order is the
    /// map's order, never an iteration accident).
    edges: BTreeMap<(usize, usize), u64>,
    /// Total call dispatches observed (each one resolved into an edge or
    /// reported through [`CallGraphCollector::unresolved_calls`]).
    calls: u64,
    /// Total dispatches observed (the sum of everything folded).
    steps: u64,
    /// The call still awaiting the step that names its callee: the
    /// caller's function index and the caller's frame depth (the callee's
    /// entry step sits exactly one frame deeper). At most one call is
    /// pending at a time — the very next dispatch after a call resolves
    /// or drops it.
    pending: Option<(usize, usize)>,
}

impl CallGraphCollector {
    /// Create an empty call graph (no calls observed).
    #[must_use]
    pub fn new() -> CallGraphCollector {
        CallGraphCollector::default()
    }

    /// Aggregate a captured step list post-hoc. The result is identical to
    /// what a live-attached collector would have produced over the same
    /// steps — the graph is a pure function of the step stream.
    #[must_use]
    pub fn from_steps(steps: &[VmStep]) -> CallGraphCollector {
        let mut graph = CallGraphCollector::new();
        for step in steps {
            graph.step(step);
        }
        graph
    }

    /// Total dispatches observed (including the faulting dispatch of a run
    /// that ended in a `VmError`).
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Total call dispatches observed — every one either resolved into an
    /// edge or reported through [`CallGraphCollector::unresolved_calls`].
    #[must_use]
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Whether no dispatch was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps == 0
    }

    /// The edge map: dispatch counts per `(caller, callee)` function-index
    /// pair (ascending key order).
    #[must_use]
    pub fn edges(&self) -> &BTreeMap<(usize, usize), u64> {
        &self.edges
    }

    /// Dispatch count for one `(caller, callee)` edge (`0` when the call
    /// never happened).
    #[must_use]
    pub fn call_count(&self, caller: usize, callee: usize) -> u64 {
        self.edges.get(&(caller, callee)).copied().unwrap_or(0)
    }

    /// Calls that never produced a callee step: an out-of-range callee
    /// index, a callee whose entry instruction faulted, a callee with
    /// empty code — or a fuel budget that ran out between the call
    /// dispatch and the callee's entry. Derived (`calls` − resolved), so
    /// it stays honest over a still-pending stream end too. Displayed by
    /// [`render_call_graph`], never silently dropped.
    #[must_use]
    pub fn unresolved_calls(&self) -> u64 {
        self.calls
            .saturating_sub(self.edges.values().copied().sum())
    }

    /// The callees one caller entered, ascending index order (each once,
    /// however often the edge dispatched).
    #[must_use]
    pub fn callees_of(&self, caller: usize) -> Vec<usize> {
        self.edges
            .range((caller, 0)..(caller, usize::MAX))
            .map(|(site, _)| site.1)
            .collect()
    }

    /// The callers that entered one callee, ascending index order.
    #[must_use]
    pub fn callers_of(&self, callee: usize) -> Vec<usize> {
        self.edges
            .iter()
            .filter(|(site, _)| site.1 == callee)
            .map(|(site, _)| site.0)
            .collect()
    }
}

impl TraceSink for CallGraphCollector {
    fn step(&mut self, step: &VmStep) {
        self.steps += 1;
        // Resolve a pending call: a successful call dispatches into the
        // callee's entry instruction, whose step sits exactly one frame
        // deeper. Any step at that depth names the callee (recursion
        // disambiguates through the depth, not the function pair).
        if let Some((caller, depth)) = self.pending.take() {
            if step.frame_depth == depth + 1 {
                *self.edges.entry((caller, step.func)).or_insert(0) += 1;
            }
            // A step at any other depth cannot name the pending call's
            // callee (the machine can only descend one frame at a call);
            // the call stays call-counted but edge-less, and
            // `unresolved_calls` reports it — never a silent drop.
        }
        // The call opcodes' payload (`StepDetail::Call`, both `OP_CALL`
        // and `OP_CALL_INDIRECT`) carries the arity, the destination
        // local, and the resume target — not the callee — so the callee
        // is named by the step above, and this dispatch only opens the
        // pending window.
        if matches!(step.detail, crate::trace::StepDetail::Call { .. }) {
            self.calls += 1;
            self.pending = Some((step.func, step.frame_depth));
        }
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a call graph into a deterministic listing: a header line (image
/// format version, total calls, distinct edges), then — when calls were
/// observed — the `edges:` section (one line per edge, ascending
/// `(caller, callee)` order, with the edge's dispatch count) and, when any
/// call never produced a callee step, the `unresolved-calls` count. A run
/// with no calls renders the explicit `no calls` line (honest about
/// equality, like the diff seam's `no differences`).
///
/// Deterministic (the same edge map renders byte-identical text) and total
/// (any map over any call count renders). The function indices are the
/// module's (the disassembler's `fn N` spellings), so an edge reads
/// identically in a [`crate::render_module`] listing and the graph.
#[must_use]
pub fn render_call_graph(edges: &BTreeMap<(usize, usize), u64>, calls: u64) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "callgraph glyim.module-image/{} calls={calls} edges={}",
        crate::IMAGE_VERSION,
        edges.len(),
    );
    if calls == 0 {
        let _ = writeln!(out, "no calls");
        debug!(calls = 0, edges = edges.len(), "vm call graph rendered");
        return out;
    }
    if !edges.is_empty() {
        let _ = writeln!(out, "edges:");
        for (&(caller, callee), count) in edges {
            let _ = writeln!(out, "  fn {caller} -> fn {callee} dispatches={count}");
        }
    }
    let resolved: u64 = edges.values().copied().sum();
    if resolved < calls {
        let _ = writeln!(out, "unresolved-calls {}", calls - resolved);
    }
    debug!(calls = calls, edges = edges.len(), "vm call graph rendered");
    out
}

#[cfg(test)]
mod callgraph_tests {
    use crate::callgraph::{CallGraphCollector, render_call_graph};
    use crate::trace::TraceCollector;
    use crate::{Function, Module, Opcode, Vm};
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

    /// The disasm/trace/coverage golden module's exact shape: main (fn 1)
    /// calls helper (fn 0) once, helper doubles its argument — one edge,
    /// no recursion.
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

    /// `rec(n)` recurses `n` times then returns 1 per frame (0 at the
    /// base), main (fn 1) seeds it with 3: the self-edge shape —
    /// `(0, 0)` accumulates once per recursive dispatch, plus the one
    /// main -> rec edge. Table-less functions (branch targets and call
    /// resume targets are raw byte offsets).
    fn recursion_module() -> Module {
        let mut rec = Vec::new();
        rec.push(op(Opcode::LoadLocal));
        rec.extend_from_slice(&u32le(1)); // arg n at local 1
        rec.push(op(Opcode::LoadConst));
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::Le));
        rec.push(op(Opcode::JumpIf));
        rec.extend_from_slice(&u32le(86)); // base case at pc 86
        rec.push(op(Opcode::LoadConst));
        rec.extend_from_slice(&i64le(0)); // fn index 0: self
        rec.push(op(Opcode::LoadLocal));
        rec.extend_from_slice(&u32le(1));
        rec.push(op(Opcode::LoadConst));
        rec.extend_from_slice(&i64le(1));
        rec.push(op(Opcode::Sub));
        rec.push(op(Opcode::StoreLocal));
        rec.extend_from_slice(&u32le(1)); // n -= 1
        rec.push(op(Opcode::LoadConst));
        rec.extend_from_slice(&i64le(0)); // fn index 0: self
        rec.push(op(Opcode::LoadLocal));
        rec.extend_from_slice(&u32le(1)); // arg n-1
        rec.push(op(Opcode::Call));
        rec.extend_from_slice(&u32le(1)); // argc
        rec.extend_from_slice(&u32le(1)); // dest local (scratch)
        rec.extend_from_slice(&u32le(76)); // resume byte offset
        rec.push(op(Opcode::LoadConst));
        rec.extend_from_slice(&i64le(1));
        rec.push(op(Opcode::Return));
        // Base case (pc 86): return 0.
        rec.push(op(Opcode::LoadConst));
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::Return));
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: rec
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(3)); // seed n = 3
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(rec, 2, 1), Function::new(main, 1, 0)], 1)
    }

    /// main (fn 0) calls function index 5, which the module does not carry
    /// — the call dispatch faults before any callee step exists.
    fn out_of_range_callee_module() -> Module {
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(5)); // fn index 5: out of range
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        Module::new(vec![Function::new(main, 1, 0)], 0)
    }

    /// A straight-line module: no `Call` dispatches, so the graph has no
    /// edges and the render says so explicitly.
    fn no_calls_module() -> Module {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(7));
        code.push(op(Opcode::Return));
        Module::new(vec![Function::new(code, 1, 0)], 0)
    }

    /// main (fn 2) calls helper a (fn 0) then helper b (fn 1): two edges
    /// out of one caller, rendered in ascending key order.
    fn multi_edge_module() -> Module {
        let ret_arg = {
            let mut f = Vec::new();
            f.push(op(Opcode::LoadLocal));
            f.extend_from_slice(&u32le(1));
            f.push(op(Opcode::Return));
            f
        };
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(0)); // fn index 0: a
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(31)); // resume byte offset
        main.push(op(Opcode::LoadConst));
        main.extend_from_slice(&i64le(1)); // fn index 1: b
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Call));
        main.extend_from_slice(&u32le(1)); // argc
        main.extend_from_slice(&u32le(0)); // dest local
        main.extend_from_slice(&u32le(58)); // resume byte offset
        main.push(op(Opcode::LoadLocal));
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return));
        let ret_arg_clone = ret_arg.clone();
        Module::new(
            vec![
                Function::new(ret_arg, 2, 1),
                Function::new(ret_arg_clone, 2, 1),
                Function::new(main, 1, 0),
            ],
            2,
        )
    }

    /// Run a module with a call-graph collector attached; the value (or
    /// error) must match the untraced run exactly (the seam is read-only)
    /// and the typed collector must come back through `take_trace`.
    fn traced_run(module: &Module) -> (CallGraphCollector, Result<crate::Value, crate::VmError>) {
        let collector = CallGraphCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CallGraphCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, outcome)
    }

    /// The golden module makes exactly one call (main -> helper): one
    /// edge, no recursion, no unresolved calls — and the render pins the
    /// listing-text spellings.
    #[test]
    fn golden_pinned_call_edges() {
        let module = golden_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome, Ok(crate::Value::Int(42)));
        assert_eq!(collector.calls(), 1);
        assert_eq!(collector.unresolved_calls(), 0);
        assert_eq!(collector.call_count(1, 0), 1);
        assert_eq!(collector.edges().len(), 1);
        assert_eq!(
            render_call_graph(collector.edges(), collector.calls()),
            "\
callgraph glyim.module-image/2 calls=1 edges=1
edges:
  fn 1 -> fn 0 dispatches=1
"
        );
    }

    /// The recursion module's self-edge: rec calls itself once per
    /// decremented frame (n = 3 -> three recursive dispatches), plus the
    /// one main -> rec edge; recursion disambiguates through the frame
    /// depth, not the function pair.
    #[test]
    fn recursion_accumulates_self_edge() {
        let module = recursion_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome, Ok(crate::Value::Int(1)));
        assert_eq!(collector.calls(), 4, "main -> rec + three recursive calls");
        assert_eq!(collector.call_count(1, 0), 1, "main -> rec");
        assert_eq!(collector.call_count(0, 0), 3, "rec -> rec");
        assert_eq!(collector.unresolved_calls(), 0);
        assert_eq!(
            render_call_graph(collector.edges(), collector.calls()),
            "\
callgraph glyim.module-image/2 calls=4 edges=2
edges:
  fn 0 -> fn 0 dispatches=3
  fn 1 -> fn 0 dispatches=1
"
        );
    }

    /// A call whose callee never dispatches (out-of-range function index)
    /// stays in `calls`, lands in no edge, and reports through
    /// `unresolved_calls` — displayed, never silently dropped. The run
    /// fails exactly as it would untraced (the seam is read-only).
    #[test]
    fn unresolved_call_when_callee_never_dispatches() {
        let module = out_of_range_callee_module();
        let (collector, outcome) = traced_run(&module);
        assert!(matches!(
            outcome,
            Err(crate::VmError::FunctionOutOfBounds { .. })
        ));
        assert_eq!(collector.calls(), 1);
        assert!(collector.edges().is_empty(), "no callee step, no edge");
        assert_eq!(collector.unresolved_calls(), 1);
        assert_eq!(
            render_call_graph(collector.edges(), collector.calls()),
            "\
callgraph glyim.module-image/2 calls=1 edges=0
unresolved-calls 1
"
        );
    }

    /// A straight-line run makes no calls: the render ends with the
    /// explicit `no calls` line (honest about equality, like the diff
    /// seam's `no differences`).
    #[test]
    fn no_calls_renders_explicit_line() {
        let module = no_calls_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome, Ok(crate::Value::Int(7)));
        assert_eq!(collector.calls(), 0);
        assert_eq!(collector.steps(), 2, "LoadConst + Return dispatched");
        assert_eq!(
            render_call_graph(collector.edges(), collector.calls()),
            "\
callgraph glyim.module-image/2 calls=0 edges=0
no calls
"
        );
    }

    /// One caller fanning out to two callees renders two edge lines, in
    /// ascending `(caller, callee)` order — the `BTreeMap`'s order is the
    /// render's order, never an iteration-order accident.
    #[test]
    fn multi_edge_renders_ascending() {
        let module = multi_edge_module();
        let (collector, outcome) = traced_run(&module);
        assert_eq!(outcome, Ok(crate::Value::Int(1)));
        assert_eq!(collector.calls(), 2);
        assert_eq!(collector.call_count(2, 0), 1);
        assert_eq!(collector.call_count(2, 1), 1);
        assert_eq!(
            render_call_graph(collector.edges(), collector.calls()),
            "\
callgraph glyim.module-image/2 calls=2 edges=2
edges:
  fn 2 -> fn 0 dispatches=1
  fn 2 -> fn 1 dispatches=1
"
        );
    }

    /// Re-folding a captured `TraceCollector` transcript through
    /// `from_steps` produces the identical edge map — the graph is a pure
    /// function of the step stream, so a post-hoc fold and a live-attached
    /// collector agree byte for byte (render included).
    #[test]
    fn from_steps_matches_live_fold() {
        let module = recursion_module();
        let transcript = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(transcript));
        let _ = vm.run_module(&module).expect("module runs");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let transcript = sink
            .downcast_mut::<TraceCollector>()
            .expect("the collector came back");
        let steps = transcript.steps().to_vec();

        let live = CallGraphCollector::from_steps(&steps);
        let (attached, _) = traced_run(&module);
        assert_eq!(live.edges(), attached.edges());
        assert_eq!(live.calls(), attached.calls());
        assert_eq!(live.steps(), attached.steps());
        assert_eq!(
            render_call_graph(live.edges(), live.calls()),
            render_call_graph(attached.edges(), attached.calls())
        );
    }

    /// The seam is read-only: a watched run's result equals the untraced
    /// run's, and the fuel gate composes — a budget spent mid-call-chain
    /// ends the stream exactly at the executed dispatches (a refused
    /// dispatch reaches neither the trace hook nor the fold).
    #[test]
    fn seam_is_read_only_and_fuel_composes() {
        let module = recursion_module();
        let untraced = Vm::new().run_module(&module).expect("untraced run");
        let (_, outcome) = traced_run(&module);
        assert_eq!(outcome, Ok(untraced));

        let cut = CallGraphCollector::new();
        let mut vm = Vm::new().with_fuel(40).with_trace(Box::new(cut));
        let _ = vm
            .run_module(&module)
            .expect_err("the budget runs out mid-chain");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let cut = sink
            .downcast_mut::<CallGraphCollector>()
            .expect("the collector came back");
        assert_eq!(cut.steps(), 40, "the fold ends at the executed dispatches");
        assert_eq!(
            cut.calls(),
            cut.edges().values().copied().sum::<u64>() + cut.unresolved_calls(),
            "every observed call is either resolved into an edge or unresolved"
        );
    }
}
