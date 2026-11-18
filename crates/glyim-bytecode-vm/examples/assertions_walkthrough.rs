//! Assertions walkthrough: run modules with the assertion seam attached,
//! then read what every assert the run evaluated observed — the passing
//! ones and the failing one.
//!
//! The coverage seam (`CoverageCollector` + `render_coverage`) says which
//! instruction sites executed; the branch-history seam
//! (`BranchCollector` + `render_branches`) says which way every decision
//! went. An `OP_ASSERT` is both — a decision the program's own invariant
//! wrote — but its outcome is folded away by both views. The assertion
//! seam (`AssertCollector` + `render_asserts`) answers the question
//! neither can: did this run's assertions hold, and which one failed —
//! for every assert, not only the one that ended the run. The hook is
//! new — the VM fires it the moment an assert's condition has been
//! compared against its asserted polarity, *before* the fault a failing
//! assert raises, so the failing assert is the stream's last entry — and
//! it observes exactly like the other seams: read-only, total (passing
//! and failing asserts report), bounded capture with a displayed
//! overflow flag, and a deterministic render. This walkthrough asserts
//! over passing runs, walks the polarity-0 (assert-not) shape, watches a
//! failing assert report and then fault, reads the pass/fail counts, and
//! proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example assertions_walkthrough
//! ```

use glyim_bytecode_vm::{AssertCollector, Function, Module, Opcode, Value, Vm, render_asserts};

/// main (fn 0) asserts `true` (expected 1, observed 1) and returns 42.
fn passing_assert_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the condition (true)
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Assert as u8); // expected 1, fall-through
    main.push(1u8);
    main.extend_from_slice(&15u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&42i64.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// main (fn 0) asserts polarity 0 over an observed 0 (the assert-not
/// convention) and returns 7.
fn negative_polarity_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the condition (false)
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Assert as u8); // expected 0, observed 0
    main.push(0u8);
    main.extend_from_slice(&15u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// main (fn 0) asserts `true`, then `not` (polarity 0), then asserts
/// truth over an observed zero — the failing one ends the run.
fn failing_third_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Assert as u8); // pass (1 over 1)
    main.push(1u8);
    main.extend_from_slice(&15u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Assert as u8); // pass (0 over 0)
    main.push(0u8);
    main.extend_from_slice(&30u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Assert as u8); // fail (0 over 1)
    main.push(1u8);
    main.extend_from_slice(&45u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&42i64.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// Run a module with a fresh assert collector attached; the run's
/// outcome and the typed collector both come back.
fn assert_run(module: &Module) -> (AssertCollector, Result<Value, glyim_bytecode_vm::VmError>) {
    let mut vm = Vm::new().with_asserts(Box::new(AssertCollector::new()));
    let outcome = vm.run_module(module);
    let mut boxed = vm.take_asserts().expect("sink still attached");
    let collector = boxed
        .as_any()
        .downcast_ref::<AssertCollector>()
        .expect("the collector came back")
        .clone();
    (collector, outcome)
}

fn main() {
    // 1. The assertion-shaped read: one passing assert — the site, the
    //    polarity, the observed condition, and the pass verdict.
    let passing = passing_assert_module();
    let (asserts, outcome) = assert_run(&passing);
    println!("one passing assert (the run returns 42):");
    println!("{}", render_asserts(asserts.events()));
    println!("outcome: {outcome:?}\n");

    // 2. The assert-not shape: polarity 0 over an observed zero passes,
    //    and the event reports both sides of the comparison.
    let (asserts, outcome) = assert_run(&negative_polarity_module());
    println!("one assert-not (expected=0, observed=0, the run returns 7):");
    println!("{}", render_asserts(asserts.events()));
    println!("outcome: {outcome:?}\n");

    // 3. The failing assert: it reports like every other evaluation —
    //    it is the stream's last entry — and the run faults right after.
    let (asserts, outcome) = assert_run(&failing_third_module());
    println!("two passing asserts, then the failing one:");
    println!("{}", render_asserts(asserts.events()));
    println!(
        "outcome: {:?} | passed {} / failed {}\n",
        outcome.err(),
        asserts.passed_count(),
        asserts.failed_count(),
    );

    // 4. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_asserts(Box::new(AssertCollector::bounded(1)));
    let _ = vm
        .run_module(&failing_third_module())
        .expect_err("the third assert faults");
    let mut boxed = vm.take_asserts().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<AssertCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(1) over three asserts: {} event kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 5. The read-only proof: the traced run's result equals the
    //    untraced one exactly (the seam observes — it never changes
    //    execution).
    let untraced = Vm::new().run_module(&passing).expect("untraced run");
    let (_, traced) = assert_run(&passing);
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
