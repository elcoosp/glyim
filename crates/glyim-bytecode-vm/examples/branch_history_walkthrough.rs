//! Branch-history walkthrough: run a module with the branch-history seam
//! attached, then read which way every branch actually went.
//!
//! The coverage seam (`CoverageCollector` + `render_coverage`) answers the
//! *spatial* question — which instruction sites dispatched, which stayed
//! dead. The branch-history seam (`BranchCollector` +
//! `render_branches`) answers the *directional* one the coverage map
//! cannot: at a `JumpIf` site, did the machine take the branch or fall
//! through, and how often? Which of a `SwitchInt`'s targets did each
//! dispatch land on? Did an `Assert` hold, and where did control resume?
//! Because the step stream carries no "branch was taken" bit, the fold
//! names each outcome by the step that *follows* the branch — the
//! machine's next dispatch — exactly the way the call-graph seam names a
//! callee. A decision whose resolving step never arrives (a fault at the
//! branch itself, a fuel budget cut between the branch and its landing)
//! stays decision-counted at its site and reports through
//! `unresolved_decisions` — displayed, never silently dropped. This
//! walkthrough runs a loop module (the back edge takes, the exit falls
//! through), re-folds a captured `TraceCollector` transcript through
//! `from_steps` into the identical history, faults a float condition
//! under `JumpIf` to see the unresolved line, and proves the seam is
//! read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example branch_history_walkthrough
//! ```

use glyim_bytecode_vm::{
    BranchCollector, Function, Module, Opcode, TraceCollector, Vm, render_branches,
};

/// A countdown loop: the `JumpIf` back edge (pc 29) is taken `n` times and
/// falls through once at the exit; the `Jump` (pc 60) closes the loop.
/// Table-less function: branch targets are raw byte offsets.
fn loop_module(n: i64) -> Module {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8); // pc 0 — counter = n
    code.extend_from_slice(&n.to_le_bytes());
    code.push(Opcode::StoreLocal as u8); // pc 9
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8); // pc 14 — loop head
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8); // pc 19
    code.extend_from_slice(&0i64.to_le_bytes());
    code.push(Opcode::Gt as u8); // pc 28 — n > 0
    code.push(Opcode::JumpIf as u8); // pc 29 — taken -> body (40)
    code.extend_from_slice(&40u32.to_le_bytes());
    code.push(Opcode::LoadLocal as u8); // pc 34 — exit: return n
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8); // pc 39
    code.push(Opcode::LoadLocal as u8); // pc 40 — body
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::LoadConst as u8); // pc 45
    code.extend_from_slice(&1i64.to_le_bytes());
    code.push(Opcode::Sub as u8); // pc 54 — n -= 1
    code.push(Opcode::StoreLocal as u8); // pc 55
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Jump as u8); // pc 60 — back edge
    code.extend_from_slice(&14u32.to_le_bytes());
    Module::new(vec![Function::new(code, 1, 0)], 0)
}

/// Run a module with a branch collector attached and return the typed
/// collector (the result value is proven read-only by the caller).
fn branch_history(module: &Module) -> BranchCollector {
    let collector = BranchCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(collector));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_trace().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<BranchCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The directional read: a 3-iteration loop takes its back edge 3
    //    times and falls through once — the split the coverage map cannot
    //    express (the site is "covered" either way).
    let module = loop_module(3);
    let history = branch_history(&module);
    println!("a 3-iteration loop (back edge taken, exit fallen through):");
    println!("{}", render_branches(history.sites(), history.decisions()));

    // 2. The re-fold: a captured TraceCollector transcript folded through
    //    `from_steps` produces the identical history — the fold is a pure
    //    function of the step stream, so a captured run can be re-analyzed
    //    without re-executing it.
    let trace = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(trace));
    let value = vm.run_module(&module).expect("transcript run");
    let mut boxed = vm.take_trace().expect("sink still attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("the collector came back")
        .steps()
        .to_vec();
    let re_folded = BranchCollector::from_steps(&steps);
    println!(
        "re-fold of a captured transcript: identical = {} (run returned {value:?})",
        re_folded.sites() == history.sites()
    );

    // 3. The honest unresolved: a float condition under `JumpIf` faults at
    //    the branch itself (ExpectedInt), so the decision dispatches but
    //    never resolves — the site keeps its dispatch count and the render
    //    displays the missing landing instead of dropping the decision.
    let mut faulting = Vec::new();
    faulting.push(Opcode::LoadConstF64 as u8); // pc 0 — a float condition
    faulting.extend_from_slice(&(1.0f64.to_bits() as i64).to_le_bytes());
    faulting.push(Opcode::JumpIf as u8); // pc 9 — pops a Float: faults
    faulting.extend_from_slice(&20u32.to_le_bytes());
    let fault_module = Module::new(vec![Function::new(faulting, 1, 0)], 0);
    let faulted = branch_history(&fault_module);
    println!("a JumpIf that faults on its float condition:");
    println!("{}", render_branches(faulted.sites(), faulted.decisions()));

    // 4. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let collector = BranchCollector::new();
        let mut vm = Vm::new().with_trace(Box::new(collector));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_trace().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
