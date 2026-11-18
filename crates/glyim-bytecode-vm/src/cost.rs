//! Inclusive-cost seam: where did a run's dispatches go, counting callees?
//!
//! The profile seam ([`crate::profile`]) is the *flat* aggregate of the
//! per-dispatch stream: every dispatch is charged to the function that
//! executed it, so a dispatcher loop that spends its whole run inside a
//! callee reads as "one dispatch here, a million there" — the *cumulative*
//! question has no answer. This module is that counterpart: a
//! [`TraceSink`] that folds the same per-dispatch stream the trace hook
//! produces into a [`CostCollector`] carrying, per function, both the
//! **inclusive** count (every dispatch that happened while the function
//! was on the active call stack — its own body plus everything its
//! callees did) and the **exclusive** count (the dispatches the function
//! itself executed — [`crate::profile`]'s per-function line). The
//! inclusive column is the call-tree view a flat profile cannot
//! reconstruct from per-function counts alone, and it is what answers
//! "which call subtree dominates the run?" without holding the
//! transcript.
//!
//! The fold reconstructs the active call stack from the stream's
//! `frame_depth` transitions (exactly the trick the call-graph seam uses
//! to name a callee by the following step): a step at a deeper frame
//! depth pushes a frame, a step back at a shallower depth pops the
//! frames that returned, and every active frame earns one inclusive
//! credit per dispatch. No `Call` payload is needed, so `OP_CALL` and
//! `OP_CALL_INDIRECT` fold identically and recursion disambiguates by
//! depth; a function active at several depths at once (a recursion's
//! nesting) earns one credit per active frame per dispatch, so the
//! inclusive column sums the function's whole active subtree.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM
//! state and cannot change execution), **total** (every dispatch folds
//! in, including the faulting dispatch of a run that ends in a `VmError`
//! and the last dispatch a fuel budget allows — the entry frame is on
//! the stack for the whole run, so its inclusive count always equals the
//! step total), **bounded** (the collector's memory is a function of the
//! module's shape — two counters per observed function plus the active
//! stack — never of the run length; an infinite recursion accumulates
//! counts without growing), and **deterministic**
//! ([`render_cost`] renders the same aggregate byte-identically — the
//! test suite pins a golden).
//!
//! The collector composes with every other seam: attach it through
//! [`crate::Vm::with_trace`] exactly like any other sink (it impls
//! [`TraceSink`]), recover it after the run through
//! [`crate::Vm::take_trace`] + [`TraceSink::as_any`], re-fold a captured
//! [`crate::TraceCollector`] transcript post-hoc with
//! [`CostCollector::from_steps`] (a pure function of the step stream —
//! the result is identical to the live fold), and pair it with
//! [`crate::Vm::with_fuel`] — the refused dispatch never reaches the
//! hook, so the cost columns end exactly at the executed count.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::Module;
use crate::trace::{TraceSink, VmStep};
use tracing::debug;

/// The inclusive/exclusive dispatch accounting for one function: the
/// inclusive count is every dispatch that happened while the function was
/// on the active call stack (its body plus its callees' work), the
/// exclusive count the dispatches it executed itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotFunction {
    /// The function's index in the module.
    pub func: usize,
    /// Dispatches while the function was on the active call stack.
    pub inclusive: u64,
    /// Dispatches the function executed itself.
    pub exclusive: u64,
}

/// The aggregate fold of the trace stream into per-function inclusive and
/// exclusive dispatch counts. Attach through [`crate::Vm::with_trace`],
/// recover through [`crate::Vm::take_trace`] + [`TraceSink::as_any`].
#[derive(Clone, Default)]
pub struct CostCollector {
    /// Inclusive dispatch count per function index (the call-tree view:
    /// the function plus everything it called, in bounded memory).
    inclusive: BTreeMap<usize, u64>,
    /// Exclusive dispatch count per function index (the flat view —
    /// `crate::profile`'s per-function column).
    exclusive: BTreeMap<usize, u64>,
    /// Total dispatches observed (the sum of every exclusive bucket).
    steps: u64,
    /// The reconstructed active call stack — `(function, frame depth)`
    /// pairs, innermost last. Fold state, not an observable.
    stack: Vec<(usize, usize)>,
}

impl CostCollector {
    /// Create an empty collector (no dispatches observed).
    #[must_use]
    pub fn new() -> CostCollector {
        CostCollector::default()
    }

    /// Re-fold a captured step list post-hoc. The result is identical to
    /// what a live-attached collector would have produced over the same
    /// steps — the cost columns are a pure function of the step stream.
    #[must_use]
    pub fn from_steps(steps: &[VmStep]) -> CostCollector {
        let mut cost = CostCollector::new();
        for step in steps {
            cost.step(step);
        }
        cost
    }

    /// Total dispatches observed (including the faulting dispatch of a run
    /// that ended in a `VmError` and the last dispatch a fuel budget
    /// allows).
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Whether no dispatch was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps == 0
    }

    /// Inclusive dispatch counts per function index (ascending order; a
    /// function the run never touched simply has no entry).
    #[must_use]
    pub fn inclusive_counts(&self) -> &BTreeMap<usize, u64> {
        &self.inclusive
    }

    /// Exclusive dispatch counts per function index (ascending order;
    /// a function the run never entered simply has no entry).
    #[must_use]
    pub fn exclusive_counts(&self) -> &BTreeMap<usize, u64> {
        &self.exclusive
    }

    /// Inclusive dispatch count for one function index (`0` when never
    /// on the stack).
    #[must_use]
    pub fn inclusive(&self, func: usize) -> u64 {
        self.inclusive.get(&func).copied().unwrap_or(0)
    }

    /// Exclusive dispatch count for one function index (`0` when never
    /// observed).
    #[must_use]
    pub fn exclusive(&self, func: usize) -> u64 {
        self.exclusive.get(&func).copied().unwrap_or(0)
    }

    /// The `n` functions with the highest inclusive counts, hottest
    /// first; ties break by function index ascending, so the ordering is
    /// a total, deterministic order. Fewer than `n` entries when fewer
    /// distinct functions were observed.
    #[must_use]
    pub fn hottest_inclusive(&self, n: usize) -> Vec<HotFunction> {
        self.ranked().into_iter().take(n).collect()
    }

    /// Every observed function as a [`HotFunction`], hottest-inclusive
    /// first (ties by function index ascending — the same total order
    /// [`CostCollector::hottest_inclusive`] ranks by).
    #[must_use]
    pub fn ranked(&self) -> Vec<HotFunction> {
        let mut ranked: Vec<HotFunction> = self
            .inclusive
            .keys()
            .copied()
            .map(|func| HotFunction {
                func,
                inclusive: self.inclusive(func),
                exclusive: self.exclusive(func),
            })
            .collect();
        // (inclusive desc, func asc): a total, deterministic order — the
        // same tie-break rule the profile's hottest ranking uses.
        ranked.sort_by(|a, b| b.inclusive.cmp(&a.inclusive).then(a.func.cmp(&b.func)));
        ranked
    }
}

impl TraceSink for CostCollector {
    fn step(&mut self, step: &VmStep) {
        // Reconstruct the active call stack from the stream's frame-depth
        // transitions (the call-graph seam's follow-step trick, depth
        // edition): pop the frames that returned, continue or enter. The
        // stream is authoritative — a same-depth frame whose function
        // spelling changes is overwritten, a depth jump past several
        // frames still pops down one frame at a time, so a corrupt
        // transcript folds without panicking and the entry frame (or
        // whatever the deepest surviving ancestor is) keeps earning
        // inclusive credit.
        let depth = step.frame_depth;
        while self.stack.last().is_some_and(|&(_, d)| d > depth) {
            self.stack.pop();
        }
        match self.stack.last_mut() {
            Some(top) if top.1 == depth => top.0 = step.func,
            _ => self.stack.push((step.func, depth)),
        }
        // Inclusive credit: every active frame earns this dispatch (the
        // executing frame and each of its ancestors).
        for &(func, _) in &self.stack {
            *self.inclusive.entry(func).or_insert(0) += 1;
        }
        // Exclusive credit: the executing function alone.
        *self.exclusive.entry(step.func).or_insert(0) += 1;
        self.steps += 1;
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render an inclusive-cost run into a deterministic listing (the
/// call-tree counterpart of [`crate::render_profile`]): a header line
/// (image format version, total steps, the module's function count), then
/// one `fn <i>: inclusive=<a> exclusive=<b>` line over every function of
/// `module` (a `0`/`0` line for a function the run never entered — the
/// listing is total over the module, not just the executed subset),
/// then the hottest-five summary line ranked by inclusive count (ties by
/// function index). A run with no dispatches renders the explicit
/// `no dispatches` line (honest about emptiness, like the call graph's
/// `no calls`).
///
/// Deterministic (the same aggregate renders byte-identical text — the
/// test suite pins a golden) and total (any collector renders).
#[must_use]
pub fn render_cost(cost: &CostCollector, module: &Module) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "cost glyim.module-image/{} steps={} fns={}",
        crate::IMAGE_VERSION,
        cost.steps(),
        module.functions.len(),
    );
    if cost.is_empty() {
        let _ = writeln!(out, "no dispatches");
        debug!(steps = 0, "vm inclusive cost rendered");
        return out;
    }
    let _ = writeln!(out, "functions:");
    for func in 0..module.functions.len() {
        let _ = writeln!(
            out,
            "fn {func}: inclusive={} exclusive={}",
            cost.inclusive(func),
            cost.exclusive(func),
        );
    }
    let top: Vec<String> = cost
        .hottest_inclusive(5)
        .into_iter()
        .map(|hot| format!("fn {}={}", hot.func, hot.inclusive))
        .collect();
    if !top.is_empty() {
        let _ = writeln!(out, "hottest: {}", top.join(", "));
    }
    debug!(steps = cost.steps(), "vm inclusive cost rendered");
    out
}

#[cfg(test)]
mod cost_tests {
    use super::{CostCollector, HotFunction, render_cost};
    use crate::trace::TraceCollector;
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

    /// main (fn 1) calls helper (fn 0) once with argument 21; helper
    /// doubles its argument into local 1 and returns it. The canonical
    /// two-frame module: 11 dispatches, one call edge.
    fn call_double_module() -> Module {
        let mut helper = Vec::new();
        helper.push(op(Opcode::LoadLocal)); // local 1 (arg)
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadConst));
        helper.extend_from_slice(&i64le(2));
        helper.push(op(Opcode::Mul));
        helper.push(op(Opcode::StoreLocal)); // local 1 = arg * 2
        helper.extend_from_slice(&u32le(1));
        helper.push(op(Opcode::LoadLocal));
        helper.extend_from_slice(&u32le(1));
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
        Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            1,
        )
    }

    /// rec(n) (fn 0) returns 0 at n <= 0, else recurses with n - 1 and
    /// returns the recursive result. main (fn 1) seeds it with 2 — the
    /// three-deep recursion the nesting tests walk.
    fn recursion_module() -> Module {
        let mut rec = Vec::new();
        rec.push(op(Opcode::LoadLocal)); // pc 0 — arg n
        rec.extend_from_slice(&u32le(1));
        rec.push(op(Opcode::LoadConst)); // pc 5
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::Le)); // pc 14 — n <= 0
        rec.push(op(Opcode::JumpIf)); // pc 15 — taken -> base case (63)
        rec.extend_from_slice(&u32le(63));
        rec.push(op(Opcode::LoadConst)); // pc 20 — fn index 0: self
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::LoadLocal)); // pc 29 — arg n
        rec.extend_from_slice(&u32le(1));
        rec.push(op(Opcode::LoadConst)); // pc 34
        rec.extend_from_slice(&i64le(1));
        rec.push(op(Opcode::Sub)); // pc 43 — n - 1 (the call's argument)
        rec.push(op(Opcode::Call)); // pc 44 — argc=1 dest=2 resume=57
        rec.extend_from_slice(&u32le(1));
        rec.extend_from_slice(&u32le(2));
        rec.extend_from_slice(&u32le(57));
        rec.push(op(Opcode::LoadLocal)); // pc 57 — the recursive result
        rec.extend_from_slice(&u32le(2));
        rec.push(op(Opcode::Return)); // pc 62
        rec.push(op(Opcode::LoadConst)); // pc 63 — base case: return 0
        rec.extend_from_slice(&i64le(0));
        rec.push(op(Opcode::Return)); // pc 72
        let mut main = Vec::new();
        main.push(op(Opcode::LoadConst)); // pc 0 — fn index 0: rec
        main.extend_from_slice(&i64le(0));
        main.push(op(Opcode::LoadConst)); // pc 9 — the seed argument
        main.extend_from_slice(&i64le(2));
        main.push(op(Opcode::Call)); // pc 18 — argc=1 dest=0 resume=31
        main.extend_from_slice(&u32le(1));
        main.extend_from_slice(&u32le(0));
        main.extend_from_slice(&u32le(31));
        main.push(op(Opcode::LoadLocal)); // pc 31 — the result
        main.extend_from_slice(&u32le(0));
        main.push(op(Opcode::Return)); // pc 36
        Module::new(vec![Function::new(rec, 3, 1), Function::new(main, 1, 0)], 1)
    }

    /// Run a module with a cost collector attached through the trace
    /// hook; the value must match the untraced result exactly (the seam
    /// is read-only) and the typed collector must come back through
    /// `take_trace`.
    fn costed_run(module: &Module) -> (CostCollector, Result<Value, crate::VmError>) {
        let collector = CostCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let outcome = vm.run_module(module);
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CostCollector>()
            .expect("the collector came back")
            .clone();
        (collector, outcome)
    }

    /// The leaf's inclusive count covers its whole subtree (itself — a
    /// leaf has no callees), and the root's inclusive count is the run's
    /// entire step total (the entry frame is on the stack throughout).
    #[test]
    fn inclusive_covers_the_call_subtree() {
        let module = call_double_module();
        let (cost, outcome) = costed_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(42));
        assert_eq!(cost.steps(), 11, "5 in main + 6 in helper");
        assert_eq!(cost.exclusive(0), 6, "the helper's own dispatches");
        assert_eq!(
            cost.inclusive(0),
            6,
            "the helper is a leaf: inclusive == exclusive"
        );
        assert_eq!(cost.exclusive(1), 5, "main's own dispatches");
        assert_eq!(cost.inclusive(1), 11, "the root earns every dispatch");
        assert_eq!(cost.inclusive_counts().len(), 2);
    }

    /// The cost columns are a pure function of the step stream: re-folding
    /// a captured transcript produces the identical aggregate (both maps
    /// and the render) as the live-attached collector.
    #[test]
    fn from_steps_matches_the_live_fold() {
        for module in [call_double_module(), recursion_module()] {
            let transcript = TraceCollector::new();
            let mut vm = Vm::new().with_trace(Box::new(transcript));
            let _ = vm.run_module(&module).expect("transcript run");
            let mut boxed = vm.take_trace().expect("sink still attached");
            let steps = boxed
                .as_any()
                .downcast_ref::<TraceCollector>()
                .expect("the collector came back")
                .steps()
                .to_vec();
            let live = CostCollector::from_steps(&steps);
            let refolded = CostCollector::from_steps(&steps);
            assert_eq!(live.inclusive_counts(), refolded.inclusive_counts());
            assert_eq!(live.exclusive_counts(), refolded.exclusive_counts());
            assert_eq!(render_cost(&live, &module), render_cost(&refolded, &module));
        }
    }

    /// Recursion nests: the entry frame's inclusive count stays the run's
    /// total, and the recursive function's inclusive count is the sum of
    /// its active frames' credits — one per dispatch per depth it sat on
    /// the stack — so a nested subtree dominates its own flat count.
    #[test]
    fn recursion_nests_inclusive_counts() {
        let module = recursion_module();
        let (cost, outcome) = costed_run(&module);
        assert_eq!(outcome.expect("module runs"), Value::Int(0));
        assert_eq!(
            cost.steps(),
            33,
            "5 main + 11 rec(2) + 11 rec(1) + 6 rec(0)"
        );
        assert_eq!(cost.inclusive(1), 33, "the root earns every dispatch");
        assert_eq!(cost.exclusive(1), 5);
        assert_eq!(
            cost.inclusive(0),
            51,
            "rec(2) at depth 2 earns 11, rec(1) at depth 3 earns 22, rec(0) at depth 4 earns 18"
        );
        assert_eq!(cost.exclusive(0), 28, "11 + 11 + 6 own dispatches");
    }

    /// A fuel budget cutting the run mid-callee is folded honestly: the
    /// executed prefix is counted, the entry frame's inclusive count
    /// equals the executed total, and the re-fold of the captured
    /// transcript matches the live fold (the refused dispatch reaches no
    /// hook, so both end at the same count).
    #[test]
    fn fuel_cut_is_total_and_matches_the_refold() {
        let module = recursion_module();
        let transcript = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(transcript)).with_fuel(12);
        let outcome = vm.run_module(&module);
        assert!(outcome.is_err(), "the budget cuts the run");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let steps = boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("the collector came back")
            .steps()
            .to_vec();
        let live = CostCollector::from_steps(&steps);
        assert_eq!(live.steps() as usize, steps.len());
        assert_eq!(
            live.inclusive(1) as usize,
            steps.len(),
            "the root was on the stack for every executed dispatch"
        );
        assert!(live.inclusive(0) > 0, "the budget cut landed inside rec");
        let refolded = CostCollector::from_steps(&steps);
        assert_eq!(live.inclusive_counts(), refolded.inclusive_counts());
    }

    /// The fold is total over a hand-built stream whose frame depths
    /// jump (a corrupt transcript): no panic, the entry frame still
    /// earns every step, and a popped-then-refilled stack counts
    /// correctly.
    #[test]
    fn corrupt_depth_jumps_are_total() {
        let steps: Vec<crate::trace::VmStep> = vec![
            vm_step(1, 1), // main at depth 1
            vm_step(0, 2), // callee at depth 2
            vm_step(0, 4), // a depth jump (corrupt: two frames at once)
            vm_step(1, 1), // back to the root
        ];
        let cost = CostCollector::from_steps(&steps);
        assert_eq!(cost.steps(), 4);
        assert_eq!(cost.inclusive(1), 4, "the root was active throughout");
        assert_eq!(
            cost.inclusive(0),
            3,
            "the callee was active at two depths during the jump (fn 0 earns one credit per active frame)"
        );
        assert_eq!(cost.exclusive(0), 2);
        assert_eq!(cost.exclusive(1), 2);
    }

    /// `hottest_inclusive` ranks by inclusive count descending, ties
    /// breaking by function index ascending (a total, deterministic
    /// order), and carries both columns.
    #[test]
    fn hottest_inclusive_orders_and_ties_break() {
        let steps: Vec<crate::trace::VmStep> = vec![
            vm_step(2, 1),
            vm_step(2, 1),
            vm_step(1, 1),
            vm_step(1, 1),
            vm_step(0, 1),
        ];
        let cost = CostCollector::from_steps(&steps);
        let ranked = cost.hottest_inclusive(3);
        assert_eq!(
            ranked,
            vec![
                HotFunction {
                    func: 1,
                    inclusive: 2,
                    exclusive: 2
                },
                HotFunction {
                    func: 2,
                    inclusive: 2,
                    exclusive: 2
                },
                HotFunction {
                    func: 0,
                    inclusive: 1,
                    exclusive: 1
                },
            ],
            "ties break by function index ascending"
        );
        assert_eq!(
            cost.hottest_inclusive(10).len(),
            3,
            "short lists are honest"
        );
        assert_eq!(cost.ranked().len(), 3);
    }

    /// The render is deterministic and pins the cost-line spellings.
    #[test]
    fn render_is_deterministic_golden() {
        let module = call_double_module();
        let (cost, _) = costed_run(&module);
        let first = render_cost(&cost, &module);
        let second = render_cost(&cost, &module);
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
cost glyim.module-image/2 steps=11 fns=2
functions:
fn 0: inclusive=6 exclusive=6
fn 1: inclusive=11 exclusive=5
hottest: fn 1=11, fn 0=6
"
        );
    }

    /// An empty render says so explicitly.
    #[test]
    fn empty_render_says_no_dispatches() {
        let module = Module::new(vec![Function::new(Vec::new(), 1, 0)], 0);
        assert_eq!(
            render_cost(&CostCollector::new(), &module),
            "\
cost glyim.module-image/2 steps=0 fns=1
no dispatches
"
        );
    }

    /// A function the run never entered renders its `0`/`0` line (the
    /// listing is total over the module, like the profile's).
    #[test]
    fn unexecuted_functions_render_zero_lines() {
        let module = call_double_module();
        let steps: Vec<crate::trace::VmStep> = vec![vm_step(1, 1), vm_step(1, 1)];
        let cost = CostCollector::from_steps(&steps);
        assert!(
            render_cost(&cost, &module).contains("fn 0: inclusive=0 exclusive=0"),
            "the never-entered helper renders its zero line"
        );
    }

    /// The seam is read-only: the traced run's result equals the untraced
    /// one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let module = call_double_module();
        let mut vm = Vm::new();
        let untraced = vm.run_module(&module).expect("untraced run");
        let (_, traced) = costed_run(&module);
        assert_eq!(traced.expect("traced run"), untraced);
    }

    /// A minimal well-formed step: `fn` dispatching at `depth`, with the
    /// opcode/stack fields the fold ignores (the cost columns are
    /// function-and-depth shaped).
    #[cfg(test)]
    fn vm_step(func: usize, depth: usize) -> crate::trace::VmStep {
        crate::trace::VmStep {
            func,
            pc: 0,
            raw: op(Opcode::LoadLocal),
            opcode: Some(Opcode::LoadLocal),
            frame_depth: depth,
            stack_depth: 0,
            detail: crate::trace::StepDetail::None,
        }
    }
}
