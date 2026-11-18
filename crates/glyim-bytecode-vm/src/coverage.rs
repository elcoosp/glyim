//! Instruction-coverage seam: which sites of a [`crate::Module`] did a
//! [`crate::Vm`] run actually execute?
//!
//! The profile seam ([`crate::profile`]) aggregates the dispatch stream by
//! *opcode* and *function*; it cannot answer the code-coverage question —
//! of the artifact's instruction sites, which ones ran and which ones are
//! dead (an untaken branch, an unreachable block, a defensive arm the
//! emitter keeps but the inputs never select). This module is the
//! *spatial* counterpart: a [`crate::trace::TraceSink`] that folds the
//! same per-dispatch stream into a per-`pc` hit map keyed
//! `(function, pc)`, aligned against the module's instruction-site
//! inventory (the walk `crate::disasm` uses, so the static listing and the
//! dynamic hit map read the same wire format once).
//!
//! # Contract
//!
//! The seam is **read-only** (the sink observes; it never mutates VM state
//! and cannot change execution), **total** (every dispatch is folded in,
//! including the faulting dispatch that produces a `VmError` — a run that
//! stops at a trap still covers the sites it executed; a dispatch that
//! lands mid-instruction over a corrupt block table is kept and displayed
//! as a *misaligned* hit rather than silently dropped), **bounded** (the
//! collector's memory is a function of the module's site count, never of
//! the run length), and **deterministic** ([`render_coverage`] renders the
//! same hit map byte-identically — the test suite pins a golden).
//!
//! The collector composes with every other seam: attach it through
//! [`crate::Vm::with_trace`] exactly like any other sink, recover it after
//! the run through [`crate::Vm::take_trace`] +
//! [`crate::trace::TraceSink::as_any`], re-aggregate a captured
//! [`crate::TraceCollector`] transcript post-hoc with
//! [`CoverageCollector::from_steps`], and pair it with
//! [`crate::Vm::with_fuel`] — a fuel-refused dispatch never reaches the
//! hook, so the coverage ends exactly at the executed sites.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::Module;
use crate::trace::{TraceSink, VmStep};
use tracing::debug;

/// The code-coverage fold of the trace stream: dispatch counts per
/// instruction site. Attach through [`crate::Vm::with_trace`], recover
/// through [`crate::Vm::take_trace`] + [`crate::trace::TraceSink::as_any`].
#[derive(Clone, Default)]
pub struct CoverageCollector {
    /// Dispatch count per `(function, pc)` site (a dispatch that lands
    /// mid-instruction keeps its exact pc — never rounded, never dropped).
    hits: BTreeMap<(usize, usize), u64>,
    /// Total dispatches observed (the sum of every site's counts).
    steps: u64,
}

impl CoverageCollector {
    /// Create an empty coverage map (no dispatches observed).
    #[must_use]
    pub fn new() -> CoverageCollector {
        CoverageCollector::default()
    }

    /// Aggregate a captured step list post-hoc. The result is identical to
    /// what a live-attached collector would have produced over the same
    /// steps — the coverage map is a pure function of the step stream.
    #[must_use]
    pub fn from_steps(steps: &[VmStep]) -> CoverageCollector {
        let mut coverage = CoverageCollector::new();
        for step in steps {
            coverage.step(step);
        }
        coverage
    }

    /// Total dispatches observed (including the faulting dispatch of a run
    /// that ended in a `VmError`).
    #[must_use]
    pub fn steps(&self) -> u64 {
        self.steps
    }

    /// Whether no dispatch was observed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps == 0
    }

    /// The hit map: dispatch counts per `(function, pc)` site (ascending
    /// key order).
    #[must_use]
    pub fn hits(&self) -> &BTreeMap<(usize, usize), u64> {
        &self.hits
    }

    /// Dispatch count for one site (`0` when never executed).
    #[must_use]
    pub fn hit_count(&self, func: usize, pc: usize) -> u64 {
        self.hits.get(&(func, pc)).copied().unwrap_or(0)
    }

    /// The executed sites, ascending key order (a subset of
    /// [`CoverageCollector::total_sites`] when every hit is aligned).
    #[must_use]
    pub fn covered_sites(&self) -> Vec<(usize, usize)> {
        self.hits.keys().copied().collect()
    }

    /// The module's instruction-site inventory: every `(function, pc)`
    /// instruction start, walked exactly like [`crate::render_module`]'s
    /// listing (unknown opcode bytes are sites that consume nothing; a
    /// truncated payload ends the walk). The coverage denominator.
    #[must_use]
    pub fn total_sites(module: &Module) -> usize {
        module
            .functions
            .iter()
            .map(|f| {
                let code = &f.code;
                let mut count = 0usize;
                let mut pos = 0usize;
                while pos < code.len() {
                    let raw = code[pos];
                    pos += 1;
                    let (_, advance) = crate::disasm::decode_instruction(code, None, raw, pos);
                    count += 1;
                    pos += advance;
                }
                count
            })
            .sum()
    }

    /// The module's instruction sites that the run never executed, in
    /// module walk order — the dead-code list. `hits` is the collector's
    /// hit map ([`CoverageCollector::hits`]); misaligned hits (never the
    /// module's fault) are not part of the aligned denominator and are
    /// rendered separately.
    #[must_use]
    pub fn uncovered_sites(
        module: &Module,
        hits: &BTreeMap<(usize, usize), u64>,
    ) -> Vec<(usize, usize)> {
        Self::site_walk(module)
            .filter(|site| !hits.contains_key(site))
            .collect()
    }

    /// How many of the module's instruction sites the run executed.
    #[must_use]
    pub fn covered_count(module: &Module, hits: &BTreeMap<(usize, usize), u64>) -> usize {
        Self::site_walk(module)
            .filter(|site| hits.contains_key(site))
            .count()
    }

    /// The instruction-site walk ([`CoverageCollector::total_sites`]'s
    /// iterator form): every `(function, pc)` instruction start, in module
    /// order.
    fn site_walk(module: &Module) -> impl Iterator<Item = (usize, usize)> {
        let mut sites = Vec::new();
        for (func, f) in module.functions.iter().enumerate() {
            let code = &f.code;
            let mut pos = 0usize;
            while pos < code.len() {
                let raw = code[pos];
                sites.push((func, pos));
                pos += 1;
                let (_, advance) = crate::disasm::decode_instruction(code, None, raw, pos);
                pos += advance;
            }
        }
        sites.into_iter()
    }
}

impl TraceSink for CoverageCollector {
    fn step(&mut self, step: &VmStep) {
        *self.hits.entry((step.func, step.pc)).or_insert(0) += 1;
        self.steps += 1;
    }

    fn as_any(&mut self) -> &mut dyn Any {
        self
    }
}

/// Render a coverage run into a deterministic listing (the spatial
/// counterpart of [`crate::render_profile`]): a header line (image format
/// version, covered/total site counts), one line per covered site in module
/// walk order (the instruction's listing text, then its hit count), then —
/// when some sites never executed — the `uncovered` section (the dead-code
/// list, in module order), then — when a dispatch landed mid-instruction
/// over a corrupt block table — the misaligned hits (displayed, never
/// silently dropped, never counted against the aligned denominator).
///
/// Deterministic and total (any hit map over any module renders). The
/// instruction spellings are [`crate::render_function`]'s (the shared
/// [`crate::disasm`] walker), so a covered site reads identically in the
/// static listing and the coverage map.
#[must_use]
pub fn render_coverage(hits: &BTreeMap<(usize, usize), u64>, module: &Module) -> String {
    // One site walk drives the whole render: the covered listing, the
    // aligned denominator, and the misaligned/uncovered splits.
    let sites: Vec<(usize, usize)> = CoverageCollector::site_walk(module).collect();
    let covered = sites.iter().filter(|s| hits.contains_key(s)).count();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "coverage glyim.module-image/{} sites={}/{}",
        crate::IMAGE_VERSION,
        covered,
        sites.len()
    );
    // The covered sites, in module walk order; the listing text is the
    // disassembler's (same widths, same spellings), the count is the fold.
    for (func, f) in module.functions.iter().enumerate() {
        let code = &f.code;
        let mut pos = 0usize;
        while pos < code.len() {
            let start = pos;
            let raw = code[pos];
            pos += 1;
            let (text, advance) = crate::disasm::decode_instruction(code, None, raw, pos);
            if let Some(count) = hits.get(&(func, start)) {
                let _ = writeln!(out, "  fn {func} pc {start} {text} hits={count}");
            }
            pos += advance;
        }
    }
    // Misaligned hits (they are not module sites, so the walk above never
    // displayed them): a corrupt block table can dispatch into the middle
    // of an instruction, and the map keeps the exact pc.
    let misaligned: Vec<(&(usize, usize), &u64)> = hits
        .iter()
        .filter(|(site, _)| !sites.iter().any(|s| s == *site))
        .collect();
    if !misaligned.is_empty() {
        let _ = writeln!(out, "misaligned hits:");
        for (&(func, pc), count) in misaligned {
            let _ = writeln!(
                out,
                "  fn {func} pc {pc} hits={count} (mid-instruction site)"
            );
        }
    }
    // The dead-code list: aligned sites the run never executed.
    if covered < sites.len() {
        let _ = writeln!(out, "uncovered:");
        for (func, pc) in &sites {
            if hits.contains_key(&(*func, *pc)) {
                continue;
            }
            let f = &module.functions[*func];
            let raw = f.code[*pc];
            let (text, _) = crate::disasm::decode_instruction(&f.code, None, raw, pc + 1);
            let _ = writeln!(out, "  fn {func} pc {pc} {text}");
        }
    }
    debug!(
        sites = sites.len(),
        dispatches = hits.values().copied().sum::<u64>(),
        "vm coverage rendered"
    );
    out
}

#[cfg(test)]
mod coverage_tests {
    use crate::coverage::{CoverageCollector, render_coverage};
    use crate::trace::TraceCollector;
    use crate::{Function, Module, Opcode, Value, Vm, decode_module, encode_module};
    use std::any::Any;
    use std::collections::BTreeMap;

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
    /// golden module's exact shape — a fully covered run (every site
    /// executes).
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

    /// A branchy function with a dead block: with arg `0`, `0 < 0` is false,
    /// so `JumpIf` falls through and block 1 (`load_const 2; return`) never
    /// executes — the classic partial-coverage shape the trace test uses.
    fn branchy_module() -> Module {
        let mut code = Vec::new();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(0));
        code.push(op(Opcode::Lt));
        code.push(op(Opcode::JumpIf));
        code.extend_from_slice(&u32le(1)); // block index 1
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(1));
        code.push(op(Opcode::Return));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(2));
        code.push(op(Opcode::Return));
        Module::new(vec![Function::with_blocks(code, 1, 1, vec![0, 30])], 0)
    }

    /// Run a module with a coverage collector attached; the value must match
    /// the untraced result exactly (the seam is read-only) and the typed
    /// collector must come back through `take_trace`.
    fn covered_run(module: &Module) -> (CoverageCollector, Value) {
        let collector = CoverageCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(module).expect("module runs");
        let mut boxed = vm.take_trace().expect("sink still attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink
            .downcast_mut::<CoverageCollector>()
            .expect("the collector came back");
        let collector = collector.clone();
        (collector, value)
    }

    /// The golden module is fully covered: 9 sites, 9 dispatches, no
    /// uncovered sites — and the render pins the listing-text spellings.
    #[test]
    fn golden_pinned_full_coverage() {
        let module = golden_module();
        let (collector, value) = covered_run(&module);
        assert_eq!(value, Value::Int(42));
        assert_eq!(collector.steps(), 9);
        let total = CoverageCollector::total_sites(&module);
        assert_eq!(total, 9, "4 helper sites + 5 main sites");
        let hits = collector.hits().clone();
        assert_eq!(
            CoverageCollector::covered_count(&module, &hits),
            9,
            "every site executed"
        );
        assert!(CoverageCollector::uncovered_sites(&module, &hits).is_empty());
        let rendered = render_coverage(&hits, &module);
        assert_eq!(
            rendered,
            "\
coverage glyim.module-image/2 sites=9/9
  fn 0 pc 0 OP_LOAD_CONST 2 hits=1
  fn 0 pc 9 OP_LOAD_LOCAL 1 hits=1
  fn 0 pc 14 OP_MUL hits=1
  fn 0 pc 15 OP_RETURN hits=1
  fn 1 pc 0 OP_LOAD_CONST 0 hits=1
  fn 1 pc 9 OP_LOAD_CONST 21 hits=1
  fn 1 pc 18 OP_CALL argc=1 dest=0 t31 hits=1
  fn 1 pc 31 OP_LOAD_LOCAL 0 hits=1
  fn 1 pc 36 OP_RETURN hits=1
"
        );
    }

    /// The branchy module covers 6 of 8 sites; the two dead-block sites
    /// render in the `uncovered` section with their listing text.
    #[test]
    fn dead_block_renders_uncovered() {
        let module = branchy_module();
        let (collector, value) = covered_run(&module);
        assert_eq!(value, Value::Int(1), "arg 0 falls through to load_const 1");
        assert_eq!(collector.steps(), 6);
        let hits = collector.hits().clone();
        let rendered = render_coverage(&hits, &module);
        assert_eq!(
            rendered,
            "\
coverage glyim.module-image/2 sites=6/8
  fn 0 pc 0 OP_LOAD_LOCAL 0 hits=1
  fn 0 pc 5 OP_LOAD_CONST 0 hits=1
  fn 0 pc 14 OP_LT hits=1
  fn 0 pc 15 OP_JUMP_IF t1 hits=1
  fn 0 pc 20 OP_LOAD_CONST 1 hits=1
  fn 0 pc 29 OP_RETURN hits=1
uncovered:
  fn 0 pc 30 OP_LOAD_CONST 2
  fn 0 pc 39 OP_RETURN
"
        );
    }

    /// The taken branch is the covered one: the same module covered from
    /// the branch-taken side swaps which constant site is live — the hit
    /// map decides, the walk only orders.
    #[test]
    fn taken_branch_is_the_covered_one() {
        let module = branchy_module();
        // The branch-taken path: block 1's constant + return instead of the
        // fall-through's (pcs 30/39 covered, pc 20/29 dead).
        let taken: BTreeMap<(usize, usize), u64> = [
            ((0usize, 0usize), 1u64),
            ((0, 5), 1),
            ((0, 14), 1),
            ((0, 15), 1),
            ((0, 30), 1),
            ((0, 39), 1),
        ]
        .into_iter()
        .collect();
        let rendered = render_coverage(&taken, &module);
        assert!(
            rendered.contains("  fn 0 pc 30 OP_LOAD_CONST 2 hits=1"),
            "the branch block's constant is covered in {rendered}"
        );
        assert!(
            rendered.contains("  fn 0 pc 20 OP_LOAD_CONST 1") && rendered.contains("uncovered:"),
            "the fall-through constant is now the dead one in {rendered}"
        );
        assert_eq!(CoverageCollector::covered_count(&module, &taken), 6);
    }

    /// Repeated execution accumulates counts: a loop body's sites carry
    /// hit counts above one, and the total equals the dispatch count.
    #[test]
    fn loop_hits_accumulate() {
        // Countdown loop: the loop-head sites run ~10k times each.
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&10i64.to_le_bytes());
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        let head = code.len();
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&1i64.to_le_bytes());
        code.push(op(Opcode::Sub));
        code.push(op(Opcode::StoreLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&0i64.to_le_bytes());
        code.push(op(Opcode::Gt));
        code.push(op(Opcode::JumpIf));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::LoadLocal));
        code.extend_from_slice(&u32le(0));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::with_blocks(code, 1, 1, vec![head])], 0);
        let (collector, value) = covered_run(&module);
        assert_eq!(value, Value::Int(0));
        let hits = collector.hits();
        let head_hits = hits.get(&(0, head)).copied().unwrap_or(0);
        assert_eq!(head_hits, 10, "the loop head dispatches once per iteration");
        assert_eq!(
            collector.steps(),
            84,
            "2 preamble + 10 iterations of 8 dispatches + 2 tail"
        );
    }

    /// `from_steps` re-folds a captured transcript into the identical
    /// coverage map post-hoc (a pure function of the step stream).
    #[test]
    fn from_steps_matches_live_fold() {
        let module = branchy_module();
        let trace = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(trace));
        let value = vm.run_module(&module).expect("runs");
        let mut boxed = vm.take_trace().expect("attached");
        let steps: Vec<_> = boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("collector")
            .steps()
            .to_vec();
        let live = CoverageCollector::from_steps(&steps);
        let (collector, _) = covered_run(&module);
        assert_eq!(value, Value::Int(1));
        assert_eq!(live.hits(), collector.hits(), "same steps, same map");
        assert_eq!(live.steps(), collector.steps());
    }

    /// The image round trip composes with the seam: decode the shipped
    /// bytes, then cover the loaded module.
    #[test]
    fn coverage_composes_with_module_image() {
        let module = golden_module();
        let bytes = encode_module(&module);
        let loaded = decode_module(&bytes).expect("decode");
        let (collector, value) = covered_run(&loaded);
        assert_eq!(value, Value::Int(42));
        assert_eq!(
            CoverageCollector::covered_count(&loaded, collector.hits()),
            9
        );
    }

    /// A fuel-refused dispatch never reaches the hook: the coverage ends
    /// exactly at the executed sites, and the totals agree.
    #[test]
    fn fuel_refused_dispatch_is_not_covered() {
        let mut code = Vec::new();
        code.push(op(Opcode::Jump));
        code.extend_from_slice(&u32le(0));
        let module = Module::new(vec![Function::with_blocks(code, 0, 0, vec![0])], 0);
        let collector = CoverageCollector::new();
        let mut vm = Vm::new().with_fuel(5).with_trace(Box::new(collector));
        let err = vm
            .run_module(&module)
            .expect_err("the loop burns the budget");
        assert_eq!(
            err,
            crate::VmError::FuelExhausted { executed: 5 },
            "the loop burns exactly the budget"
        );
        let mut boxed = vm.take_trace().expect("attached");
        let sink: &mut dyn Any = boxed.as_any();
        let collector = sink.downcast_mut::<CoverageCollector>().expect("collector");
        assert_eq!(collector.steps(), 5, "the refused dispatch is not folded");
        assert_eq!(collector.hit_count(0, 0), 5, "the loop head ran five times");
    }

    /// A dispatch that lands mid-instruction (a corrupt block table's
    /// target) is a *misaligned* hit: kept in the map, rendered in its own
    /// section, never silently dropped and never counted against the
    /// aligned denominator.
    #[test]
    fn mid_instruction_dispatch_is_a_misaligned_hit() {
        // pc 5 is mid-instruction (inside LoadConst's payload at pc 0);
        // block 0's offset targets it.
        let mut code = Vec::new();
        code.push(op(Opcode::LoadConst));
        code.extend_from_slice(&i64le(3));
        code.push(op(Opcode::Return));
        let module = Module::new(vec![Function::with_blocks(code, 0, 0, vec![5])], 0);
        let hits: BTreeMap<(usize, usize), u64> = [((0usize, 5usize), 1u64)].into_iter().collect();
        let rendered = render_coverage(&hits, &module);
        assert!(
            rendered.contains("misaligned hits:"),
            "the mid-instruction dispatch is displayed in {rendered}"
        );
        assert!(
            rendered.contains("fn 0 pc 5 hits=1"),
            "with its exact pc in {rendered}"
        );
        assert_eq!(CoverageCollector::total_sites(&module), 2);
    }

    /// The render is deterministic: the same hit map renders byte-identical
    /// text.
    #[test]
    fn render_is_deterministic() {
        let module = golden_module();
        let (collector, _) = covered_run(&module);
        let hits = collector.hits().clone();
        assert_eq!(
            render_coverage(&hits, &module),
            render_coverage(&hits, &module)
        );
    }

    /// An empty collector renders an all-uncovered module (the zero case is
    /// displayable, not an error).
    #[test]
    fn empty_collector_renders_all_uncovered() {
        let module = branchy_module();
        let hits = BTreeMap::new();
        let rendered = render_coverage(&hits, &module);
        assert!(rendered.contains("sites=0/8"), "in {rendered}");
        assert!(rendered.contains("uncovered:"), "in {rendered}");
        assert!(
            rendered.contains("fn 0 pc 0 OP_LOAD_LOCAL 0"),
            "in {rendered}"
        );
    }

    /// An unknown opcode byte is a site (the disasm walk consumes nothing
    /// for it): covering it renders the `unknown(0xNN)` spelling.
    #[test]
    fn unknown_opcode_site_is_displayable() {
        let code = vec![0x42, op(Opcode::Return)];
        let module = Module::new(vec![Function::new(code, 0, 0)], 0);
        assert_eq!(CoverageCollector::total_sites(&module), 2, "0x42 + Return");
        let hits: BTreeMap<(usize, usize), u64> = [((0usize, 0usize), 1u64)].into_iter().collect();
        let rendered = render_coverage(&hits, &module);
        assert!(
            rendered.contains("fn 0 pc 0 unknown(0x42) hits=1"),
            "in {rendered}"
        );
    }
}
