//! Inclusive-cost walkthrough: run a module with the cost seam attached,
//! then read where the run's dispatches went — per function, the inclusive
//! count (the function plus everything its callees did while it was on the
//! stack) next to the exclusive count (its own dispatches).
//!
//! The profile seam (`ProfileCollector` + `render_profile`) is the *flat*
//! aggregate: every dispatch is charged to the function that executed it,
//! so a run spent inside a callee reads as "one dispatch here, a million
//! there" and no flat column answers "which call subtree dominates?" —
//! that question needs the *call-tree* view. The cost seam
//! (`CostCollector` + `render_cost`) folds the same per-dispatch stream
//! the trace hook produces into both columns: inclusive credits go to
//! every active frame per dispatch (a recursion's nesting earns one
//! credit per active depth), exclusive credits to the executing function
//! alone. It observes exactly like the other stream folds: read-only,
//! total over faults and fuel cuts (the entry frame is on the stack for
//! the whole run, so the root's inclusive count always equals the step
//! total), bounded by the module's shape, and a deterministic render —
//! plus the `from_steps` re-fold of a captured transcript and the
//! `hottest_inclusive` ranking. This walkthrough runs a recursion to see
//! the nesting sum, ranks the hottest functions, re-folds a captured
//! transcript into the identical aggregate, cuts a run with a fuel budget
//! to see the honest executed-prefix accounting, and proves the seam is
//! read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example inclusive_cost_walkthrough
//! ```

use glyim_bytecode_vm::{CostCollector, Function, Module, Opcode, TraceCollector, Vm, render_cost};

/// rec(n) (fn 0) returns 0 at n <= 0, else recurses with n - 1 and
/// returns the recursive result. main (fn 1) seeds it with 2 — a
/// three-deep recursion, so the inclusive column sums the nesting.
/// Table-less functions: branch and call-resume targets are raw byte
/// offsets.
fn recursion_module() -> Module {
    let mut rec = Vec::new();
    rec.push(Opcode::LoadLocal as u8); // arg n
    rec.extend_from_slice(&1u32.to_le_bytes());
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&0i64.to_le_bytes());
    rec.push(Opcode::Le as u8); // n <= 0
    rec.push(Opcode::JumpIf as u8);
    rec.extend_from_slice(&63u32.to_le_bytes()); // base case at pc 63
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: self
    rec.push(Opcode::LoadLocal as u8);
    rec.extend_from_slice(&1u32.to_le_bytes()); // arg n
    rec.push(Opcode::LoadConst as u8);
    rec.extend_from_slice(&1i64.to_le_bytes());
    rec.push(Opcode::Sub as u8); // n - 1 (the call's argument)
    rec.push(Opcode::Call as u8);
    rec.extend_from_slice(&1u32.to_le_bytes()); // argc
    rec.extend_from_slice(&2u32.to_le_bytes()); // dest local (the result)
    rec.extend_from_slice(&57u32.to_le_bytes()); // resume byte offset
    rec.push(Opcode::LoadLocal as u8);
    rec.extend_from_slice(&2u32.to_le_bytes()); // the recursive result
    rec.push(Opcode::Return as u8);
    rec.push(Opcode::LoadConst as u8); // base case: return 0
    rec.extend_from_slice(&0i64.to_le_bytes());
    rec.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: rec
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&2i64.to_le_bytes()); // the seed argument
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes()); // argc
    main.extend_from_slice(&0u32.to_le_bytes()); // dest local
    main.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes()); // the result
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(rec, 3, 1), Function::new(main, 1, 0)], 1)
}

fn main() {
    // 1. The recursive run: attach the collector through the trace hook
    //    exactly like any other sink, recover it typed afterwards.
    let module = recursion_module();
    let mut vm = Vm::new().with_trace(Box::new(CostCollector::new()));
    let value = vm
        .run_module(&module)
        .expect("the module runs (the seam is read-only)");
    let mut boxed = vm.take_trace().expect("sink still attached");
    let cost = boxed
        .as_any()
        .downcast_ref::<CostCollector>()
        .expect("the collector came back")
        .clone();
    println!("rec(2) returned {value:?} (unwatched result, seam is read-only)");
    println!(
        "steps={} rec: inclusive={} exclusive={} main: inclusive={} exclusive={}",
        cost.steps(),
        cost.inclusive(0),
        cost.exclusive(0),
        cost.inclusive(1),
        cost.exclusive(1),
    );
    println!("{}", render_cost(&cost, &module));

    // 2. The ranking: hottest by inclusive count, ties by function index.
    for hot in cost.hottest_inclusive(5) {
        println!(
            "hottest: fn {} inclusive={} exclusive={}",
            hot.func, hot.inclusive, hot.exclusive,
        );
    }

    // 3. The aggregate is a pure function of the step stream: re-fold a
    //    captured TraceCollector transcript post-hoc into the identical
    //    columns (a shared transcript feeds every aggregate seam).
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
    let refolded = CostCollector::from_steps(&steps);
    println!(
        "from_steps over {} captured steps: inclusive equal = {} (render equal = {})",
        steps.len(),
        refolded.inclusive_counts() == cost.inclusive_counts(),
        render_cost(&refolded, &module) == render_cost(&cost, &module),
    );

    // 4. A fuel budget cut lands mid-recursion: the executed prefix is
    //    folded honestly — the root's inclusive count still equals the
    //    executed total (the entry frame was on the stack for every one
    //    of those dispatches), and the refused dispatch reaches no hook.
    let mut vm = Vm::new()
        .with_trace(Box::new(CostCollector::new()))
        .with_fuel(12);
    let outcome = vm.run_module(&module);
    let mut boxed = vm.take_trace().expect("sink still attached");
    let cut = boxed
        .as_any()
        .downcast_ref::<CostCollector>()
        .expect("the collector came back")
        .clone();
    println!("\nfuel-budget cut after 12 dispatches: {outcome:?}");
    println!(
        "executed steps={} root inclusive={} (the entry frame's honest prefix) rec exclusive={}",
        cut.steps(),
        cut.inclusive(1),
        cut.exclusive(0),
    );
}
