//! Branch-history timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `vm_trace` /
//! `vm_profile` / `vm_coverage` / `vm_callgraph` benches in this crate:
//! only `std::time::Instant` + `std::hint::black_box`. The branches seam
//! (the `branches` module) folds the same per-dispatch stream the trace
//! hook produces, so its headline numbers are the **hot-path overhead** of
//! folding a live run (the sink rides the identical `with_trace` hook —
//! the extra work per dispatch is one pending-decision check plus, per
//! decision, one map update and one entry registration), the **post-hoc
//! fold** of a captured transcript through `from_steps`, and the
//! deterministic render. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench vm_branches
//! ```

use glyim_bytecode_vm::{BranchCollector, Function, Module, Opcode, TraceCollector, Vm};
use std::hint::black_box;
use std::time::Instant;

/// A branch-heavy loop: fn 0 decrements local 0 and loops back through a
/// `JumpIf` (the conditional back edge) until zero — every iteration
/// dispatches one `JumpIf` and one `Jump`, the aggregate-friendly shape:
/// bounded map, hot decision sites.
fn branch_loop() -> Module {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8); // pc 0 — n = 10_000
    code.extend_from_slice(&10_000i64.to_le_bytes());
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

fn main() {
    const ROUNDS: u32 = 20;
    let module = branch_loop();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let mut vm = Vm::new();
        let _ = black_box(vm.run_module(&module).expect("untraced run"));
    }
    let untraced = start.elapsed();

    let start = Instant::now();
    let mut decisions = 0u64;
    for _ in 0..ROUNDS {
        let mut vm = Vm::new().with_trace(Box::new(BranchCollector::new()));
        let _ = black_box(vm.run_module(&module).expect("branch run"));
        let mut boxed = vm.take_trace().expect("attached");
        decisions += boxed
            .as_any()
            .downcast_ref::<BranchCollector>()
            .map(|c| c.decisions())
            .unwrap_or(0);
    }
    let folded = start.elapsed();

    let transcript = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(transcript));
    let _ = vm.run_module(&module).expect("transcript run");
    let mut boxed = vm.take_trace().expect("attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .map(|t| t.steps().to_vec())
        .unwrap_or_default();

    let start = Instant::now();
    for _ in 0..ROUNDS {
        let history = BranchCollector::from_steps(black_box(&steps));
        black_box(history.decisions());
    }
    let refold = start.elapsed();

    let history = BranchCollector::from_steps(&steps);
    let start = Instant::now();
    let rendered = render_len(&history);
    let render = start.elapsed();

    println!(
        "branch-loop runs (20 x ~90k dispatches): untraced {untraced:?} | branch-fold {folded:?} ({decisions} decisions folded) | from_steps {refold:?} ({} steps) | render {render:?} ({rendered} bytes)",
        steps.len(),
    );
}

fn render_len(history: &BranchCollector) -> usize {
    glyim_bytecode_vm::render_branches(history.sites(), history.decisions()).len()
}
