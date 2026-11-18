//! Run-diff seam: did two runs of the same program execute identically?
//!
//! The module-image diff ([`crate::diff`]) answers the *structural* change
//! question — what changed between two decoded artifacts. It cannot answer
//! the *behavioral* one: an emitter tweak that leaves the artifact bytes
//! almost untouched can still change what a run *does*, and two runs of
//! the same artifact can legitimately end at different points (a fuel
//! budget, a fault). This module is the behavioral counterpart: a pure
//! function over two captured transcripts ([`crate::trace::VmStep`] lists,
//! as captured by [`crate::TraceCollector`]) that renders where the two
//! runs part ways — or the explicit `no divergence` line when they never
//! do, the determinism gate any no-op change must pass.
//!
//! # Contract
//!
//! The diff is **deterministic** (the same pair of transcripts renders
//! byte-identical text — the test suite pins a golden), **total** (any two
//! transcripts diff, including an empty one and a pair where one run is a
//! strict prefix of the other — a shorter run is displayed as an honest
//! end, never padded), and **honest about equality** (two identical
//! transcripts render the explicit `no divergence` line). The payload
//! spellings are module-free (a `ConstStr` step shows the pool index and
//! its resolution, never the content — the two sides may come from
//! different modules), and the line shape mirrors [`crate::render_trace`]
//! (the same `fn`/`pc`/`frame`/`stack`/mnemonic fields), so a diverging
//! step reads identically in a trace listing and the run diff.

use std::fmt::Write as _;

use crate::trace::{StepDetail, VmStep};
use tracing::debug;

/// Render the behavioral differences between two captured runs into a
/// deterministic listing: a header line (the dispatch count on each side),
/// then — when the runs part ways — either the `diverge at dispatch N:`
/// site (both steps spelled in [`crate::render_trace`]'s field shape,
/// module-free payloads) with the honest tail counts, or the shorter
/// side's explicit end line when one transcript is a strict prefix of the
/// other (a fuel budget or a fault ends one run where the other
/// continues). Two identical transcripts render header + `no divergence`.
///
/// Deterministic (the same pair renders byte-identical text) and total
/// (any two transcripts diff, the empty pair included).
#[must_use]
pub fn render_run_diff(baseline: &[VmStep], current: &[VmStep]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "run-diff steps {}->{}", baseline.len(), current.len());
    let common = baseline.len().min(current.len());
    let mut diverge_at: Option<usize> = None;
    for i in 0..common {
        if baseline[i] != current[i] {
            diverge_at = Some(i);
            break;
        }
    }
    match diverge_at {
        None => {
            if baseline.len() == current.len() {
                let _ = writeln!(out, "no divergence");
            } else if baseline.len() < current.len() {
                let _ = writeln!(
                    out,
                    "a ended at dispatch {} (b continues {} more dispatches)",
                    baseline.len(),
                    current.len() - baseline.len()
                );
            } else {
                let _ = writeln!(
                    out,
                    "b ended at dispatch {} (a continues {} more dispatches)",
                    current.len(),
                    baseline.len() - current.len()
                );
            }
        }
        Some(i) => {
            let _ = writeln!(out, "diverge at dispatch {}:", i + 1);
            let _ = writeln!(out, "  a: {}", step_line(&baseline[i]));
            let _ = writeln!(out, "  b: {}", step_line(&current[i]));
            let _ = writeln!(
                out,
                "a tail {} dispatches | b tail {} dispatches",
                baseline.len() - i - 1,
                current.len() - i - 1
            );
        }
    }
    debug!(
        baseline = baseline.len(),
        current = current.len(),
        "vm run diff rendered"
    );
    out
}

/// Spell one step in [`crate::render_trace`]'s field shape (module-free):
/// the site, the depths, the mnemonic (unknown bytes as the disassembler's
/// `unknown(0xNN)`), and the payload suffix.
fn step_line(step: &VmStep) -> String {
    let mnemonic = match step.opcode {
        Some(op) => crate::disasm::op_name(op).to_string(),
        None => format!("unknown(0x{:02x})", step.raw),
    };
    let mut line = format!(
        "fn {} pc {} frame={} stack={} {mnemonic}",
        step.func, step.pc, step.frame_depth, step.stack_depth,
    );
    let _ = write!(line, "{}", detail_suffix(&step.detail));
    line
}

/// The payload suffix (the text after the mnemonic, module-free — a diff
/// may compare runs over two different modules, so a `ConstStr` step shows
/// the pool index and its resolution, never the content).
fn detail_suffix(detail: &StepDetail) -> String {
    match *detail {
        StepDetail::None => String::new(),
        StepDetail::ConstInt(v) => format!(" {v}"),
        StepDetail::ConstFloatBits(bits) => {
            format!(" {} (bits 0x{bits:016x})", f64::from_bits(bits))
        }
        StepDetail::ConstStr {
            pool_index,
            resolved,
        } => {
            if resolved {
                format!(" pool[{pool_index}]")
            } else {
                format!(" pool[{pool_index}] <out of range>")
            }
        }
        StepDetail::Local(idx) => format!(" {idx}"),
        StepDetail::Cast { kind, target } => format!(
            " {}->{}",
            crate::disasm::cast_kind_name(kind),
            crate::disasm::cast_target_name(target)
        ),
        StepDetail::Branch { target } => format!(" t{target}"),
        StepDetail::Switch { n_arms, otherwise } => {
            format!(" n={n_arms} otherwise=t{otherwise}")
        }
        StepDetail::Assert { expected, target } => {
            format!(" expected={expected} t{target}")
        }
        StepDetail::Aggregate(n) => format!(" n={n}"),
        StepDetail::Call { argc, dest, target } => format!(" argc={argc} dest={dest} t{target}"),
        StepDetail::Truncated => " <truncated>".to_string(),
    }
}

#[cfg(test)]
mod rundiff_tests {
    use crate::rundiff::render_run_diff;
    use crate::trace::{StepDetail, TraceCollector, VmStep};
    use crate::{Function, Module, Opcode, Vm};

    /// Hand-built steps: exact control over the two sides, so the golden
    /// pins the divergence spellings without depending on a run.
    fn step(
        func: usize,
        pc: usize,
        opcode: Option<Opcode>,
        stack: usize,
        detail: StepDetail,
    ) -> VmStep {
        let raw = opcode.map(|o| o as u8).unwrap_or(0xFF);
        VmStep {
            func,
            pc,
            raw,
            opcode,
            frame_depth: 1,
            stack_depth: stack,
            detail,
        }
    }

    /// The disasm/trace golden module's shape: main (fn 1) calls helper
    /// (fn 0), helper doubles its argument.
    fn golden_module() -> Module {
        let mut helper = Vec::new();
        helper.push(Opcode::LoadConst as u8);
        helper.extend_from_slice(&2i64.to_le_bytes());
        helper.push(Opcode::LoadLocal as u8);
        helper.extend_from_slice(&1u32.to_le_bytes());
        helper.push(Opcode::Mul as u8);
        helper.push(Opcode::Return as u8);
        let mut main = Vec::new();
        main.push(Opcode::LoadConst as u8);
        main.extend_from_slice(&0i64.to_le_bytes());
        main.push(Opcode::LoadConst as u8);
        main.extend_from_slice(&21i64.to_le_bytes());
        main.push(Opcode::Call as u8);
        main.extend_from_slice(&1u32.to_le_bytes());
        main.extend_from_slice(&0u32.to_le_bytes());
        main.extend_from_slice(&31u32.to_le_bytes());
        main.push(Opcode::LoadLocal as u8);
        main.extend_from_slice(&0u32.to_le_bytes());
        main.push(Opcode::Return as u8);
        Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            1,
        )
    }

    /// The same module with one constant changed (21 -> 20): identical
    /// shape, different behavior — the emitter-tweak scenario.
    fn tweaked_module() -> Module {
        let mut helper = Vec::new();
        helper.push(Opcode::LoadConst as u8);
        helper.extend_from_slice(&2i64.to_le_bytes());
        helper.push(Opcode::LoadLocal as u8);
        helper.extend_from_slice(&1u32.to_le_bytes());
        helper.push(Opcode::Mul as u8);
        helper.push(Opcode::Return as u8);
        let mut main = Vec::new();
        main.push(Opcode::LoadConst as u8);
        main.extend_from_slice(&0i64.to_le_bytes());
        main.push(Opcode::LoadConst as u8);
        main.extend_from_slice(&20i64.to_le_bytes());
        main.push(Opcode::Call as u8);
        main.extend_from_slice(&1u32.to_le_bytes());
        main.extend_from_slice(&0u32.to_le_bytes());
        main.extend_from_slice(&31u32.to_le_bytes());
        main.push(Opcode::LoadLocal as u8);
        main.extend_from_slice(&0u32.to_le_bytes());
        main.push(Opcode::Return as u8);
        Module::new(
            vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
            1,
        )
    }

    /// Capture a module's transcript through the trace hook.
    fn transcript(module: &Module) -> Vec<VmStep> {
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let _ = vm.run_module(module);
        let mut boxed = vm.take_trace().expect("sink still attached");
        boxed
            .as_any()
            .downcast_ref::<TraceCollector>()
            .expect("the collector came back")
            .steps()
            .to_vec()
    }

    /// Two identical runs render the explicit `no divergence` line — the
    /// determinism gate any no-op change must pass.
    #[test]
    fn identical_runs_render_no_divergence() {
        let module = golden_module();
        let a = transcript(&module);
        let b = transcript(&module);
        assert_eq!(a, b, "the machine is deterministic over the same module");
        assert_eq!(
            render_run_diff(&a, &b),
            format!("run-diff steps {}->{}\nno divergence\n", a.len(), b.len())
        );
    }

    /// Two runs differing at one step render the diverge site with both
    /// spellings (the `render_trace` field shape, module-free payloads)
    /// and the honest tail counts.
    #[test]
    fn golden_pinned_divergence() {
        let a = vec![
            step(1, 0, Some(Opcode::LoadConst), 0, StepDetail::ConstInt(1)),
            step(1, 9, Some(Opcode::LoadConst), 1, StepDetail::ConstInt(21)),
        ];
        let b = vec![
            step(1, 0, Some(Opcode::LoadConst), 0, StepDetail::ConstInt(1)),
            step(1, 9, Some(Opcode::LoadConst), 1, StepDetail::ConstInt(20)),
        ];
        assert_eq!(
            render_run_diff(&a, &b),
            "\
run-diff steps 2->2
diverge at dispatch 2:
  a: fn 1 pc 9 frame=1 stack=1 OP_LOAD_CONST 21
  b: fn 1 pc 9 frame=1 stack=1 OP_LOAD_CONST 20
a tail 0 dispatches | b tail 0 dispatches
"
        );
    }

    /// A shorter run that is a strict prefix of the longer one is an
    /// honest end, never a divergence: the render names which side ended
    /// and how many dispatches the other continues.
    #[test]
    fn prefix_run_renders_an_honest_end() {
        let a = vec![step(
            1,
            0,
            Some(Opcode::LoadConst),
            0,
            StepDetail::ConstInt(1),
        )];
        let b = vec![
            step(1, 0, Some(Opcode::LoadConst), 0, StepDetail::ConstInt(1)),
            step(1, 9, Some(Opcode::LoadLocal), 1, StepDetail::Local(0)),
            step(1, 14, Some(Opcode::Return), 1, StepDetail::None),
        ];
        assert_eq!(
            render_run_diff(&a, &b),
            "\
run-diff steps 1->3
a ended at dispatch 1 (b continues 2 more dispatches)
"
        );
        assert_eq!(
            render_run_diff(&b, &a),
            "\
run-diff steps 3->1
b ended at dispatch 1 (a continues 2 more dispatches)
"
        );
    }

    /// An empty transcript against a non-empty one ends at dispatch 0.
    #[test]
    fn empty_side_ends_at_dispatch_zero() {
        let b = vec![step(0, 0, Some(Opcode::Return), 0, StepDetail::None)];
        assert_eq!(
            render_run_diff(&[], &b),
            "\
run-diff steps 0->1
a ended at dispatch 0 (b continues 1 more dispatches)
"
        );
    }

    /// Two identical transcripts of length zero render `no divergence`
    /// (nothing ran on either side — total over the empty pair).
    #[test]
    fn empty_pair_is_no_divergence() {
        assert_eq!(
            render_run_diff(&[], &[]),
            "run-diff steps 0->0\nno divergence\n"
        );
    }

    /// Real runs: the same module twice diverges nowhere; the same module
    /// with one changed constant diverges at that constant's dispatch —
    /// the regression gate an emitter tweak faces.
    #[test]
    fn real_runs_gate_an_emitter_tweak() {
        let base = golden_module();
        let tweaked = tweaked_module();
        assert_eq!(
            render_run_diff(&transcript(&base), &transcript(&base)),
            "run-diff steps 9->9\nno divergence\n"
        );
        let a = transcript(&base);
        let b = transcript(&tweaked);
        let rendered = render_run_diff(&a, &b);
        assert!(rendered.contains("diverge at dispatch 2:"), "{rendered}");
        assert!(rendered.contains("OP_LOAD_CONST 21"), "{rendered}");
        assert!(rendered.contains("OP_LOAD_CONST 20"), "{rendered}");
    }

    /// Fuel composes: two budgets over the same module produce a prefix
    /// pair — the shorter run ended, the longer continues — and neither
    /// side saw a refused dispatch (the fuel gate never reaches the hook).
    #[test]
    fn fuel_budgets_produce_a_prefix_pair() {
        let module = golden_module();
        let collect_fuel = |fuel: u64| -> Vec<VmStep> {
            let collector = TraceCollector::new();
            let mut vm = Vm::new().with_fuel(fuel).with_trace(Box::new(collector));
            let _ = vm.run_module(&module);
            let mut boxed = vm.take_trace().expect("sink still attached");
            boxed
                .as_any()
                .downcast_ref::<TraceCollector>()
                .expect("the collector came back")
                .steps()
                .to_vec()
        };
        let short = collect_fuel(4);
        let long = collect_fuel(9);
        assert_eq!(short.len(), 4);
        assert_eq!(long.len(), 9);
        assert_eq!(
            render_run_diff(&short, &long),
            "run-diff steps 4->9\na ended at dispatch 4 (b continues 5 more dispatches)\n"
        );
    }
}
