//! Run-diff walkthrough: compare two captured runs and read exactly where
//! they part ways — or the explicit line proving they never do.
//!
//! The module-image diff (`render_module_diff`) answers the *structural*
//! change question — what changed between two decoded artifacts. The
//! run-diff seam (`render_run_diff`) answers the *behavioral* one: did
//! two runs execute identically, and if not, at which dispatch did they
//! diverge (both steps spelled in the trace's field shape)? The classic
//! gates it drives: a no-op change over the same module must produce
//! `no divergence` (the determinism gate), an emitter tweak that changes
//! behavior names the dispatch where the old and new builds part ways,
//! and two fuel budgets over the same module produce a prefix pair — the
//! shorter run *ended*, it did not diverge. This walkthrough runs both
//! gates over real captured transcripts and shows the honest-end shape.
//! Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example run_diff_walkthrough
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, TraceCollector, Vm, render_run_diff};

/// `double(n)`: main (fn 1) calls helper (fn 0), helper doubles its
/// argument. `delta` is the constant main passes — the emitter-tweak
/// lever (a one-constant change that alters behavior, not shape).
fn call_double(delta: i64) -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadConst as u8);
    helper.extend_from_slice(&2i64.to_le_bytes());
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes()); // MIR ABI: arg at local 1
    helper.push(Opcode::Mul as u8);
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&delta.to_le_bytes()); // the tweaked constant
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&0u32.to_le_bytes()); // dest local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(
        vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
        1,
    )
}

/// Capture a module's transcript through the trace hook.
fn transcript(module: &Module) -> Vec<glyim_bytecode_vm::VmStep> {
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

fn main() {
    // 1. The no-op gate: two runs of the same module are step-for-step
    //    identical — the diff renders the explicit line, never silence.
    let base = call_double(21);
    let a = transcript(&base);
    let b = transcript(&base);
    println!("two runs of the same module:");
    println!("{}", render_run_diff(&a, &b));

    // 2. The emitter-tweak gate: one changed constant (21 -> 20) — the
    //    shape is identical, the behavior is not, and the diff names the
    //    dispatch where the two builds part ways.
    let tweaked = call_double(20);
    let c = transcript(&tweaked);
    println!("same program, one constant changed (21 -> 20):");
    println!("{}", render_run_diff(&a, &c));

    // 3. The honest end: two fuel budgets over the same module produce a
    //    prefix pair — the shorter run ended (a refused dispatch never
    //    reaches the trace hook), the longer continues.
    let collect_fuel = |fuel: u64| -> Vec<glyim_bytecode_vm::VmStep> {
        let collector = TraceCollector::new();
        let mut vm = Vm::new().with_fuel(fuel).with_trace(Box::new(collector));
        let _ = vm.run_module(&base);
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
    println!("same module under two fuel budgets (4 vs 9):");
    println!("{}", render_run_diff(&short, &long));
}
