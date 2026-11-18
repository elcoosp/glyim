//! Branch-history seam: which way did each branch actually go?
//!
//! The coverage seam ([`crate::coverage`]) is the *spatial* half of the
//! control-flow question: of the artifact's instruction sites, which ones
//! dispatched and which ones stayed dead. It cannot answer the
//! *directional* half — at a `JumpIf` site, did the machine take the branch
//! or fall through? Which of a `SwitchInt`'s targets did each dispatch land
//! on? Did an `Assert` hold, and where did control resume? This module is
//! that counterpart: a [`crate::trace::TraceSink`] that folds the same
//! per-dispatch stream into a per-decision-site history keyed
//! `(function, pc)`, with every decision's landing sites and counts.
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change execution), **total** (every decision dispatch is
//! registered at its site, including the faulting one — a branch whose
//! condition never resolves still shows its dispatch count), **bounded**
//! (the collector's memory is a function of the module's decision-site
//! count, never of the run length), and **deterministic**
//! ([`render_branches`] renders the same history byte-identically — the
//! test suite pins a golden).
//!
//! The stream carries no "branch was taken" bit, so — exactly like the
//! call-graph seam names a callee by the step that *follows* the call —
//! the fold names a decision's outcome by the step that *follows* the
//! branch: the machine's next dispatch. A `JumpIf` whose next dispatch
//! sits at its fall-through pc (`pc + 5`, the decoded payload width) fell
//! through; one whose next dispatch sits anywhere else took the branch.
//! `Jump`, `SwitchInt`, and `Assert` transfer unconditionally on success
//! (an `Assert` that does not hold ends the run in
//! [`crate::VmError::AssertionFailed`]), so for them the landing map is
//! the whole outcome. A decision whose resolving step never arrives — the
//! run faults at the branch itself, a fuel budget ends the run between
//! the branch and its landing, the branch is the last dispatch of the
//! stream — stays decision-counted at its site with no landing, and
//! [`BranchCollector::unresolved_decisions`] reports it — displayed,
//! never silently dropped.
//!
//! The collector composes with every other seam: attach it through
//! [`crate::Vm::with_trace`] exactly like any other sink, recover it after
//! the run through [`crate::Vm::take_trace`] +
//! [`crate::trace::TraceSink::as_any`], re-aggregate a captured
//! [`crate::TraceCollector`] transcript post-hoc with
//! [`BranchCollector::from_steps`], and pair it with
//! [`crate::Vm::with_fuel`] — a fuel-refused dispatch never reaches the
//! hook, so the history ends exactly at the executed decisions.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::Opcode;
use crate::trace::{TraceSink, VmStep};
use tracing::debug;

/// The decision opcode a branch site folded (`Jump`, `JumpIf`, `SwitchInt`,
/// or `Assert` — the four control-transfer opcodes the machine dispatches).
/// The spelling matches the disassembler's mnemonic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    /// `OP_JUMP`: the unconditional transfer. Every dispatch lands on the
    /// resolved target (never falls through).
    Jump,
    /// `OP_JUMP_IF`: the conditional branch — jumps when the popped
    /// condition is non-zero, falls through (`pc + 5`) when it is zero.
    JumpIf,
    /// `OP_SWITCH_INT`: the multi-way branch. The arm values travel in the
    /// instruction payload, not in the step stream, so the fold records the
    /// honest observable — which target pc each dispatch landed on.
    Switch,
    /// `OP_ASSERT`: the guard. A held assert transfers to its target block;
    /// a failed one ends the run in [`crate::VmError::AssertionFailed`]
    /// (the decision stays unresolved — the fault seam reports the failure).
    Assert,
}

impl BranchKind {
    /// The disassembler's mnemonic for the decision opcode (the spelling
    /// [`render_branches`] uses).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BranchKind::Jump => "Jump",
            BranchKind::JumpIf => "JumpIf",
            BranchKind::Switch => "SwitchInt",
            BranchKind::Assert => "Assert",
        }
    }
}

/// One decision site's folded history: how often the site dispatched, how
/// each dispatch resolved, and where the machine dispatched next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchSite {
    /// The decision opcode this site folds.
    pub kind: BranchKind,
    /// Decision dispatches at this site (each one either resolved into a
    /// landing or reported through
    /// [`BranchCollector::unresolved_decisions`]).
    pub dispatches: u64,
    /// `JumpIf`-only: dispatches that jumped — the resolving dispatch sat
    /// anywhere other than the fall-through pc.
    pub taken: u64,
    /// `JumpIf`-only: dispatches that fell through — the resolving
    /// dispatch sat exactly at the fall-through pc. A degenerate `JumpIf`
    /// whose target resolves to its own fall-through reports here too
    /// (the observable is the machine's next dispatch; taken-to-next
    /// and fallen-through are the same machine behavior).
    pub fell_through: u64,
    /// Landing sites per outcome: the `(function, pc)` the machine
    /// dispatched next, in ascending key order with per-outcome counts.
    /// Includes a `JumpIf`'s fall-through landings (the destination of a
    /// taken branch is a landing like any other).
    pub landings: BTreeMap<(usize, usize), u64>,
}

/// The pending decision awaiting the step that names its outcome: the
/// branch's function, its pc, its kind, and — for `JumpIf` — the
/// fall-through pc the resolving dispatch is compared against. At most one
/// decision is pending at a time: the very next dispatch resolves or drops
/// it (branches transfer control within the function they dispatch in, so
/// the next dispatch is always a same-function step unless the run ended).
#[derive(Clone, Copy, Debug)]
struct PendingDecision {
    func: usize,
    pc: usize,
    kind: BranchKind,
    /// `JumpIf`-only: the fall-through pc (`pc + 5` — one opcode byte plus
    /// the 4-byte `u32` target payload). Other kinds never fall through.
    fallthrough: usize,
}

/// The branch-history fold of the trace stream: per-decision-site
/// dispatch counts, taken/fall-through splits, and landing maps. Attach
/// through [`crate::Vm::with_trace`], recover through
/// [`crate::Vm::take_trace`] + [`crate::trace::TraceSink::as_any`],
/// re-aggregate a captured [`crate::TraceCollector`] transcript post-hoc
/// with [`BranchCollector::from_steps`], and pair it with
/// [`crate::Vm::with_fuel`] — a fuel-refused dispatch never reaches the
/// hook, so the history ends exactly at the executed decisions.
#[derive(Clone, Default)]
pub struct BranchCollector {
    /// Per-decision-site history, in ascending key order (a `BTreeMap`, so
    /// the render's order is the map's order, never an iteration accident).
    sites: BTreeMap<(usize, usize), BranchSite>,
    /// Total decision dispatches observed (each one registered at its site;
    /// every one either resolved into a landing or reported through
    /// [`BranchCollector::unresolved_decisions`]).
    decisions: u64,
    /// Total dispatches observed (the sum of everything folded).
    steps: u64,
    /// The decision still awaiting the step that names its outcome. At most
    /// one is pending at a time — the very next dispatch resolves it.
    pending: Option<PendingDecision>,
}

impl BranchCollector {
    /// Create an empty branch history (no decisions observed).
    #[must_use]
    pub fn new() -> BranchCollector {
        BranchCollector::default()
    }

    /// Aggregate a captured step list post-hoc. The result is identical to
    /// what a live-attached collector would have produced over the same
    /// steps — the history is a pure function of the step stream.
    #[must_use]
    pub fn from_steps(steps: &[VmStep]) -> BranchCollector {
        let mut history = BranchCollector::new();
        for step in steps {
            history.step(step);
        }
        history
    }

    /// Total dispatches observed (including the faulting dispatch of a run
    /// that ended in a `VmError`).
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Total decision dispatches observed — every one either resolved into
    /// a landing or reported through
    /// [`BranchCollector::unresolved_decisions`].
    #[must_use]
    pub fn decisions(&self) -> u64 {
        self.decisions
    }

    /// Whether no decision was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.decisions == 0
    }

    /// The per-decision-site history (ascending key order).
    #[must_use]
    pub fn sites(&self) -> &BTreeMap<(usize, usize), BranchSite> {
        &self.sites
    }

    /// One decision site's folded history (`None` when the site never
    /// dispatched a decision).
    #[must_use]
    pub fn site(&self, func: usize, pc: usize) -> Option<&BranchSite> {
        self.sites.get(&(func, pc))
    }

    /// Decision dispatch count for one site (`0` when the site never
    /// dispatched a decision).
    #[must_use]
    pub fn decision_count(&self, func: usize, pc: usize) -> u64 {
        self.sites
            .get(&(func, pc))
            .map_or(0, |site| site.dispatches)
    }

    /// `JumpIf` taken count for one site (`0` when the site is not a
    /// `JumpIf` or never took the branch).
    #[must_use]
    pub fn taken_count(&self, func: usize, pc: usize) -> u64 {
        self.sites.get(&(func, pc)).map_or(0, |site| site.taken)
    }

    /// `JumpIf` fall-through count for one site (`0` when the site is not
    /// a `JumpIf` or never fell through).
    #[must_use]
    pub fn fell_through_count(&self, func: usize, pc: usize) -> u64 {
        self.sites
            .get(&(func, pc))
            .map_or(0, |site| site.fell_through)
    }

    /// Decisions that never produced a resolving step: a fault at the
    /// branch itself (`ExpectedInt` on a float condition), a fuel budget
    /// that ran out between the branch dispatch and its landing, a branch
    /// as the stream's last dispatch. Derived (`decisions` − resolved),
    /// so it stays honest over a still-pending stream end too. Displayed
    /// by [`render_branches`], never silently dropped.
    #[must_use]
    pub fn unresolved_decisions(&self) -> u64 {
        let resolved: u64 = self
            .sites
            .values()
            .map(|site| site.landings.values().copied().sum::<u64>())
            .sum();
        self.decisions.saturating_sub(resolved)
    }

    /// The landings one decision site produced, ascending key order (each
    /// `(function, pc)` once, however often it landed).
    #[must_use]
    pub fn landings_of(&self, func: usize, pc: usize) -> Vec<(usize, usize)> {
        self.sites
            .get(&(func, pc))
            .map_or_else(Vec::new, |site| site.landings.keys().copied().collect())
    }
}

impl TraceSink for BranchCollector {
    fn step(&mut self, step: &VmStep) {
        self.steps += 1;
        // Resolve the pending decision with this step — the machine's next
        // dispatch after a branch. Branches transfer control within the
        // function they dispatch in, so the resolving step is always a
        // same-function step unless the run ended; a cross-function step
        // (which the machine cannot produce here) leaves the decision
        // decision-counted but landing-less, and
        // `unresolved_decisions` reports it — never a silent drop.
        if let Some(pending) = self.pending.take() {
            if step.func == pending.func {
                if let Some(site) = self.sites.get_mut(&(pending.func, pending.pc)) {
                    *site.landings.entry((step.func, step.pc)).or_insert(0) += 1;
                    if pending.kind == BranchKind::JumpIf {
                        if step.pc == pending.fallthrough {
                            site.fell_through += 1;
                        } else {
                            site.taken += 1;
                        }
                    }
                }
            }
        }
        // Register this dispatch if it is a decision opcode, then open the
        // one-step pending window for its outcome. The site is registered
        // at dispatch time (not resolution time), so a decision whose
        // resolving step never arrives still shows its dispatch count.
        let kind = match step.opcode {
            Some(Opcode::Jump) => BranchKind::Jump,
            Some(Opcode::JumpIf) => BranchKind::JumpIf,
            Some(Opcode::SwitchInt) => BranchKind::Switch,
            Some(Opcode::Assert) => BranchKind::Assert,
            _ => return,
        };
        self.decisions += 1;
        self.sites
            .entry((step.func, step.pc))
            .or_insert_with(|| BranchSite {
                kind,
                dispatches: 0,
                taken: 0,
                fell_through: 0,
                landings: BTreeMap::new(),
            })
            .dispatches += 1;
        self.pending = Some(PendingDecision {
            func: step.func,
            pc: step.pc,
            kind,
            // The fall-through pc: one opcode byte plus the 4-byte `u32`
            // target payload (`OP_JUMP_IF`'s decoded width). Other kinds
            // never fall through, so their fall-through slot is unused.
            fallthrough: step.pc + 5,
        });
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a branch history into a deterministic listing (the directional
/// counterpart of [`crate::render_coverage`]): a header line (image format
/// version, total decisions, distinct sites), then one block per decision
/// site in ascending `(function, pc)` order — the site's mnemonic spelling,
/// its dispatch count, and, for a `JumpIf`, its taken/fall-through split —
/// followed by the site's landing lines (ascending key order, one per
/// distinct outcome with its count). A run with no decisions renders the
/// explicit `no decision dispatches` line (honest about emptiness, like
/// the call graph's `no calls`). When any decision never resolved, the
/// `unresolved-decisions` count closes the listing.
///
/// Deterministic (the same history renders byte-identical text) and total
/// (any site map over any decision count renders). The function indices
/// and pcs are the machine's (the disassembler's `fn N` spellings), so a
/// site reads identically in a [`crate::render_module`] listing and the
/// history.
#[must_use]
pub fn render_branches(sites: &BTreeMap<(usize, usize), BranchSite>, decisions: u64) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "branch history glyim.module-image/{} decisions={decisions} sites={}",
        crate::IMAGE_VERSION,
        sites.len(),
    );
    if decisions == 0 {
        let _ = writeln!(out, "no decision dispatches");
        debug!(
            decisions = 0,
            sites = sites.len(),
            "vm branch history rendered"
        );
        return out;
    }
    for (&(func, pc), site) in sites {
        if site.kind == BranchKind::JumpIf {
            let _ = writeln!(
                out,
                "  fn {func} pc {pc} {} dispatches={} taken={} fell-through={}",
                site.kind.name(),
                site.dispatches,
                site.taken,
                site.fell_through,
            );
        } else {
            let _ = writeln!(
                out,
                "  fn {func} pc {pc} {} dispatches={}",
                site.kind.name(),
                site.dispatches,
            );
        }
        for (&(landing_func, landing_pc), count) in &site.landings {
            let _ = writeln!(out, "    -> fn {landing_func} pc {landing_pc} x{count}");
        }
    }
    let resolved: u64 = sites
        .values()
        .map(|site| site.landings.values().copied().sum::<u64>())
        .sum();
    if resolved < decisions {
        let _ = writeln!(out, "unresolved-decisions {}", decisions - resolved);
    }
    debug!(
        decisions = decisions,
        sites = sites.len(),
        "vm branch history rendered"
    );
    out
}

#[cfg(test)]
mod branches_tests {
    use super::{BranchCollector, BranchKind, render_branches};
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

    /// A countdown loop, table-less: the `JumpIf` back edge (pc 29) is
    /// taken 3 times and falls through once; the `Jump` (pc 60) closes the
    /// loop 3 times. Both outcomes of a conditional branch, plus an
    /// unconditional site, in one run. Returns `Value::Int(0)`.
    fn loop_module() -> Module {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst)); // pc 0
        code.extend_from_slice(&i64le(3)); // n = 3
        code.push(op(Opcode::StoreLocal)); // pc 9
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal)); // pc 14 — loop head
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst)); // pc 19
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Gt)); // pc 28 — n > 0
        code.push(op(Opcode::JumpIf)); // pc 29
        code.extend_from_slice(&u32le(40)); // body at pc 40; fall-through pc 34
        code.push(op(Opcode::LoadLocal)); // pc 34 — exit: return n
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return)); // pc 39
        code.push(op(Opcode::LoadLocal)); // pc 40 — body
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst)); // pc 45
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Sub)); // pc 54 — n -= 1
        code.push(op(Opcode::StoreLocal)); // pc 55
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Jump)); // pc 60 — back to the loop head
        code.extend_from_slice(&u32le(14));
        Module::new(vec![Function::new(code, 1, 0)], 0)
    }

    /// Run a module with a branch collector attached; the value must match
    /// the untraced result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_trace`.
    fn traced_run(module: &Module) -> (BranchCollector, Value) {
        let collector = BranchCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BranchCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// The countdown loop takes the back edge 3 times and falls through
    /// once; the unconditional `Jump` closes the loop 3 times.
    #[test]
    fn jump_if_taken_and_fall_through_split() {
        let module = loop_module();
        let (collector, value) = traced_run(&module);
        assert_eq!(value, Value::Int(0));
        assert_eq!(
            collector.steps(),
            35,
            "2 preamble + 9 dispatches per pass x 3 + 6 exit"
        );
        assert_eq!(collector.decisions(), 7, "4 JumpIf + 3 Jump dispatches");
        let site = collector.site(0, 29).expect("the JumpIf site folded");
        assert_eq!(site.kind, BranchKind::JumpIf);
        assert_eq!(site.dispatches, 4);
        assert_eq!(site.taken, 3);
        assert_eq!(site.fell_through, 1);
        assert_eq!(
            site.landings.get(&(0, 40)).copied().unwrap_or(0),
            3,
            "taken landings hit the body"
        );
        assert_eq!(
            site.landings.get(&(0, 34)).copied().unwrap_or(0),
            1,
            "the fall-through landing hit the exit"
        );
        let jump = collector.site(0, 60).expect("the Jump site folded");
        assert_eq!(jump.kind, BranchKind::Jump);
        assert_eq!(jump.dispatches, 3);
        assert_eq!(jump.landings.get(&(0, 14)).copied(), Some(3));
        assert_eq!(
            collector.unresolved_decisions(),
            0,
            "every decision resolved"
        );
    }

    /// The taken/fall-through accessors read the same fold the sites do.
    #[test]
    fn accessors_read_the_fold() {
        let module = loop_module();
        let (collector, _) = traced_run(&module);
        assert_eq!(collector.decision_count(0, 29), 4);
        assert_eq!(collector.taken_count(0, 29), 3);
        assert_eq!(collector.fell_through_count(0, 29), 1);
        assert_eq!(collector.decision_count(0, 60), 3);
        assert_eq!(collector.taken_count(0, 60), 0, "a Jump never takes");
        assert_eq!(collector.site(0, 999), None, "unknown site");
        assert_eq!(collector.landings_of(0, 60), vec![(0, 14)]);
    }

    /// A `SwitchInt` in a loop lands on each arm once and on the
    /// otherwise target once; the arm values travel in the payload, so the
    /// landing map (not the arm) is the honest observable.
    #[test]
    fn switch_landings_split_by_outcome() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst)); // pc 0 — n = 2
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::StoreLocal)); // pc 9
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal)); // pc 14 — switch head
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::SwitchInt)); // pc 19 — 2 arms, 33 bytes
        code.extend_from_slice(&u32le(2)); // arm count
        code.extend_from_slice(&i64le(1)); // arm 1: value 1 -> pc 52
        code.extend_from_slice(&u32le(52));
        code.extend_from_slice(&i64le(2)); // arm 2: value 2 -> pc 66
        code.extend_from_slice(&u32le(66));
        code.extend_from_slice(&u32le(105)); // otherwise -> pc 105 (exit)
        code.push(op(Opcode::LoadConst)); // pc 52 — arm 1
        code.extend_from_slice(&i64le(100));
        code.push(op(Opcode::Jump)); // pc 61
        code.extend_from_slice(&u32le(80));
        code.push(op(Opcode::LoadConst)); // pc 66 — arm 2
        code.extend_from_slice(&i64le(200));
        code.push(op(Opcode::Jump)); // pc 75
        code.extend_from_slice(&u32le(80));
        code.push(op(Opcode::LoadLocal)); // pc 80 — decrement block
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst)); // pc 85
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Sub)); // pc 94 — n -= 1
        code.push(op(Opcode::StoreLocal)); // pc 95
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Jump)); // pc 100 — back to the switch head
        code.extend_from_slice(&u32le(14));
        code.push(op(Opcode::LoadLocal)); // pc 105 — otherwise: exit
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return)); // pc 110
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let (collector, value) = traced_run(&module);
        assert_eq!(value, Value::Int(0));
        let switch = collector.site(0, 19).expect("the SwitchInt site folded");
        assert_eq!(switch.kind, BranchKind::Switch);
        assert_eq!(switch.dispatches, 3, "n = 2, then 1, then 0");
        assert_eq!(
            switch.landings.get(&(0, 66)).copied(),
            Some(1),
            "arm 2 (n=2)"
        );
        assert_eq!(
            switch.landings.get(&(0, 52)).copied(),
            Some(1),
            "arm 1 (n=1)"
        );
        assert_eq!(
            switch.landings.get(&(0, 105)).copied(),
            Some(1),
            "otherwise (n=0)"
        );
        assert_eq!(switch.taken, 0, "taken/fall-through is JumpIf-only");
        // The unconditional Jumps funnel both arms into the decrement block.
        assert_eq!(
            collector
                .site(0, 61)
                .expect("arm-1 Jump")
                .landings
                .get(&(0, 80))
                .copied(),
            Some(1)
        );
        assert_eq!(
            collector
                .site(0, 75)
                .expect("arm-2 Jump")
                .landings
                .get(&(0, 80))
                .copied(),
            Some(1)
        );
        assert_eq!(
            collector
                .site(0, 100)
                .expect("loop-back Jump")
                .landings
                .get(&(0, 14))
                .copied(),
            Some(2)
        );
        assert_eq!(collector.unresolved_decisions(), 0);
    }

    /// A held `Assert` transfers to its target: the landing is the guard
    /// passing, and the run continues past it.
    #[test]
    fn held_assert_lands_on_target() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal)); // pc 0 — n
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal)); // pc 5 — n again
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Eq)); // pc 10 — n == n (always 1)
        code.push(op(Opcode::Assert)); // pc 11 — expect truthy, target 17
        code.extend_from_slice(&[1u8]);
        code.extend_from_slice(&u32le(17));
        code.push(op(Opcode::LoadConst)); // pc 17 — the guarded block
        code.extend_from_slice(&i64le(7));
        code.push(op(Opcode::Return)); // pc 26
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let (collector, value) = traced_run(&module);
        assert_eq!(value, Value::Int(7), "the assert held and control resumed");
        let assert_site = collector.site(0, 11).expect("the Assert site folded");
        assert_eq!(assert_site.kind, BranchKind::Assert);
        assert_eq!(assert_site.dispatches, 1);
        assert_eq!(
            assert_site.landings.get(&(0, 17)).copied(),
            Some(1),
            "a held assert transfers to its target"
        );
    }

    /// A failed `Assert` ends the run: the decision stays unresolved (the
    /// fault seam reports the failure; this seam displays the missing
    /// landing).
    #[test]
    fn failed_assert_stays_unresolved() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst)); // pc 0 — cond 0
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Assert)); // pc 9 — expect truthy: fails
        code.extend_from_slice(&[1u8]);
        code.extend_from_slice(&u32le(20));
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let collector = BranchCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let err = vm.run_module(&module).expect_err("the assert fails");
        assert!(matches!(err, crate::VmError::AssertionFailed));
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BranchCollector>()
            .expect("the collector came back")
            .clone();
        let assert_site = collector.site(0, 9).expect("the faulting Assert folded");
        assert_eq!(assert_site.dispatches, 1, "the dispatch is registered");
        assert!(
            assert_site.landings.is_empty(),
            "no landing — the run ended"
        );
        assert_eq!(collector.unresolved_decisions(), 1);
    }

    /// A branch that faults at itself (a float condition under `JumpIf`)
    /// dispatches, never resolves, and reports through
    /// `unresolved_decisions`.
    #[test]
    fn fault_at_branch_is_unresolved() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConstF64)); // pc 0 — a float condition
        code.extend_from_slice(&i64le(1.0f64.to_bits() as i64));
        code.push(op(Opcode::JumpIf)); // pc 9 — pops a Float: ExpectedInt
        code.extend_from_slice(&u32le(20));
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let collector = BranchCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the condition is a Float");
        assert!(matches!(err, crate::VmError::ExpectedInt { .. }));
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BranchCollector>()
            .expect("the collector came back")
            .clone();
        let site = collector.site(0, 9).expect("the faulting JumpIf folded");
        assert_eq!(site.dispatches, 1);
        assert!(site.landings.is_empty());
        assert_eq!(collector.unresolved_decisions(), 1);
    }

    /// A fuel budget that ends the run between the branch dispatch and its
    /// landing leaves the decision unresolved (a fuel-refused dispatch
    /// never reaches the hook, so no phantom landing appears).
    #[test]
    fn fuel_composes_with_the_fold() {
        let module = loop_module();
        // 6 dispatches: LoadConst, StoreLocal, LoadLocal, LoadConst, Gt,
        // JumpIf — then the budget refuses the body's first dispatch.
        let collector = BranchCollector::new();
        let mut vm = Vm::new().with_fuel(6).with_trace(Box::new(collector));
        let err = vm.run_module(&module).expect_err("the budget runs out");
        assert!(matches!(err, crate::VmError::FuelExhausted { .. }));
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<BranchCollector>()
            .expect("the collector came back")
            .clone();
        assert_eq!(collector.decisions(), 1);
        let site = collector.site(0, 29).expect("the JumpIf dispatched");
        assert_eq!(site.dispatches, 1);
        assert!(site.landings.is_empty(), "the landing dispatch was refused");
        assert_eq!(collector.unresolved_decisions(), 1);
    }

    /// The live fold and the post-hoc `from_steps` re-fold of a captured
    /// transcript agree exactly — the history is a pure function of the
    /// step stream.
    #[test]
    fn from_steps_matches_the_live_fold() {
        let module = loop_module();
        let (live, _) = traced_run(&module);
        let mut vm = Vm::new();
        let _ = vm.run_module(&module).expect("untraced run");
        // Re-run with a TraceCollector to capture the transcript.
        let trace = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(trace));
        let _ = vm.run_module(&module).expect("traced run");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let trace = sink
            .downcast_mut::<TraceCollector>()
            .expect("the collector came back");
        let steps = trace.steps().to_vec();
        let re_folded = BranchCollector::from_steps(&steps);
        assert_eq!(re_folded.sites(), live.sites());
        assert_eq!(re_folded.decisions(), live.decisions());
        assert_eq!(re_folded.steps(), live.steps());
        assert_eq!(
            re_folded.unresolved_decisions(),
            live.unresolved_decisions()
        );
    }

    /// A block-table function folds identically to a table-less one: the
    /// fold reads only landing pcs, never resolves targets itself.
    #[test]
    fn block_table_targets_fold_the_same() {
        // Fall-through side: 0 < 0 is false, so the JumpIf falls through.
        let mut fall = Vec::new();
        fall.push(op(Opcode::LoadLocal));
        fall.extend_from_slice(&u32le(0));
        fall.push(op(Opcode::LoadConst));
        fall.extend_from_slice(&i64le(0));
        fall.push(op(Opcode::Lt));
        fall.push(op(Opcode::JumpIf));
        fall.extend_from_slice(&u32le(1)); // block 1
        fall.push(op(Opcode::LoadConst)); // fall-through: return 1
        fall.extend_from_slice(&i64le(1));
        fall.push(op(Opcode::Return));
        fall.push(op(Opcode::LoadConst)); // block 1: return 2
        fall.extend_from_slice(&i64le(2));
        fall.push(op(Opcode::Return));
        let fall_module = Module::new(vec![Function::with_blocks(fall, 1, 1, vec![0, 30])], 0);
        let (collector, value) = traced_run(&fall_module);
        assert_eq!(value, Value::Int(1));
        let site = collector.site(0, 15).expect("the JumpIf folded");
        assert_eq!(
            site.fell_through, 1,
            "the landing sat at the fall-through pc"
        );
        assert_eq!(site.landings.get(&(0, 20)).copied(), Some(1));

        // Taken side: 0 < 1 is true, so the JumpIf enters block 1.
        let mut taken = Vec::new();
        taken.push(op(Opcode::LoadLocal));
        taken.extend_from_slice(&u32le(0));
        taken.push(op(Opcode::LoadConst));
        taken.extend_from_slice(&i64le(1));
        taken.push(op(Opcode::Lt));
        taken.push(op(Opcode::JumpIf));
        taken.extend_from_slice(&u32le(1)); // block 1
        taken.push(op(Opcode::LoadConst)); // fall-through (dead here)
        taken.extend_from_slice(&i64le(1));
        taken.push(op(Opcode::Return));
        taken.push(op(Opcode::LoadConst)); // block 1: return 2
        taken.extend_from_slice(&i64le(2));
        taken.push(op(Opcode::Return));
        let taken_module = Module::new(vec![Function::with_blocks(taken, 1, 1, vec![0, 30])], 0);
        let (collector, value) = traced_run(&taken_module);
        assert_eq!(value, Value::Int(2));
        let site = collector.site(0, 15).expect("the JumpIf folded");
        assert_eq!(site.taken, 1, "the landing sat past the fall-through pc");
        assert_eq!(site.landings.get(&(0, 30)).copied(), Some(1));
    }

    /// A degenerate `JumpIf` whose target resolves to its own fall-through
    /// pc reports as fell-through: the observable is the machine's next
    /// dispatch, and taken-to-next is the same machine behavior.
    #[test]
    fn jump_if_targeting_its_fall_through_reports_fell_through() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst)); // pc 0 — cond 1 (truthy)
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::JumpIf)); // pc 9 — target 14 == fall-through
        code.extend_from_slice(&u32le(14));
        code.push(op(Opcode::LoadConst)); // pc 14
        code.extend_from_slice(&i64le(7));
        code.push(op(Opcode::Return)); // pc 23
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let (collector, value) = traced_run(&module);
        assert_eq!(value, Value::Int(7));
        let site = collector.site(0, 9).expect("the JumpIf folded");
        assert_eq!(site.taken, 0);
        assert_eq!(
            site.fell_through, 1,
            "the landing sat at the fall-through pc"
        );
        assert_eq!(site.landings.get(&(0, 14)).copied(), Some(1));
    }

    /// A straight-line run folds zero decisions and renders the explicit
    /// empty line.
    #[test]
    fn straight_line_run_has_no_decisions() {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::Add));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::new(code, 1, 0)], 0);
        let (collector, _) = traced_run(&module);
        assert!(collector.is_empty());
        assert_eq!(
            render_branches(collector.sites(), collector.decisions()),
            "\
branch history glyim.module-image/2 decisions=0 sites=0
no decision dispatches
"
        );
    }

    /// The render is deterministic: two folds of the same run render
    /// byte-identical text, and the golden pins the site and landing
    /// spellings.
    #[test]
    fn render_is_deterministic_golden() {
        let module = loop_module();
        let (collector, _) = traced_run(&module);
        let first = render_branches(collector.sites(), collector.decisions());
        let second = render_branches(collector.sites(), collector.decisions());
        assert_eq!(first, second);
        assert_eq!(
            first,
            "\
branch history glyim.module-image/2 decisions=7 sites=2
  fn 0 pc 29 JumpIf dispatches=4 taken=3 fell-through=1
    -> fn 0 pc 34 x1
    -> fn 0 pc 40 x3
  fn 0 pc 60 Jump dispatches=3
    -> fn 0 pc 14 x3
"
        );
    }

    /// The seam is read-only: the traced run's result equals the untraced
    /// one exactly.
    #[test]
    fn the_seam_is_read_only() {
        let module = loop_module();
        let mut vm = Vm::new();
        let untraced = vm.run_module(&module).expect("untraced run");
        let (_, traced) = traced_run(&module);
        assert_eq!(traced, untraced);
    }
}
