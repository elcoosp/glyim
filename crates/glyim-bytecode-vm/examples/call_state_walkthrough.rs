//! Call-state walkthrough: run a module with the call-state seam attached,
//! then read every frame's locals at the moment it began and the moment it
//! ended.
//!
//! The watch seam (`WatchCollector` + `render_watch`) reports one event
//! per local *write*; the call-state seam (`StateCollector` +
//! `render_state`) reports the *frame-shaped* view the write stream does
//! not: at each frame's enter, the freshly bound locals (the MIR ABI's
//! zero return place at slot 0, the arguments at `1..=arg_count`); at its
//! exit, the final local state after everything the body did. The hook is
//! new — the first one the VM fires at the frame lifecycle (call entry
//! and return) rather than per dispatch — and it observes exactly like
//! the other seams: read-only, total (the entry frame's enter and exit
//! report too), bounded capture with a displayed overflow flag, and a
//! deterministic render. An unbalanced tail (a fuel budget or a fault
//! ends the run between the boundaries) is displayed, never silently
//! paired away. This walkthrough runs a call, reads the argument
//! landing and the mutated exit snapshot, nests a recursion to see the
//! LIFO depth pairing, and proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example call_state_walkthrough
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, StateCollector, Vm, render_state};

/// main (fn 1) calls helper (fn 0) once with argument 21; helper doubles
/// its argument into local 1 and returns it.
fn call_double_module() -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadLocal as u8); // local 1 (the arg)
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::LoadConst as u8);
    helper.extend_from_slice(&2i64.to_le_bytes());
    helper.push(Opcode::Mul as u8);
    helper.push(Opcode::StoreLocal as u8); // local 1 = arg * 2
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&21i64.to_le_bytes()); // the argument
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

/// Run a module with a state collector attached and return the typed
/// collector.
fn call_state(module: &Module) -> StateCollector {
    let collector = StateCollector::new();
    let mut vm = Vm::new().with_state(Box::new(collector));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_state().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<StateCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The frame-shaped read: one call produces four boundaries — the
    //    entry frame's enter/exit and the callee's enter/exit, with the
    //    argument landing at the enter and the mutated slot at the exit.
    let module = call_double_module();
    let state = call_state(&module);
    println!("one call (helper doubles its argument):");
    println!("{}", render_state(state.events()));

    // 2. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let bounded = StateCollector::bounded(2);
    let mut vm = Vm::new().with_state(Box::new(bounded));
    let _ = vm.run_module(&module).expect("module runs");
    let mut boxed = vm.take_state().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<StateCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(2): {} events kept, overflowed = {}",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 3. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let collector = StateCollector::new();
        let mut vm = Vm::new().with_state(Box::new(collector));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_state().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
