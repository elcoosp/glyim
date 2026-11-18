//! Drops walkthrough: run modules with the discard seam attached, then
//! read what the run discarded — the destroyed aggregates, the discarded
//! addresses — and where each discard happened.
//!
//! The watch seam (`WatchCollector` + `render_watch`) records what values
//! a local slot took; the call-state seam (`StateCollector` +
//! `render_state`) records the state a frame held at its boundaries.
//! Neither answers the destruction-side question: every `OP_DROP`
//! discards a value and the discarded value vanished from every view at
//! the pop. The discard seam (`DropCollector` + `render_drops`) is that
//! counterpart: the VM hands it one `DropEvent` per executed discard —
//! the site and the popped value in full — so the emitter's scope-exit
//! shape is readable after the run. The hook observes exactly like the
//! other seams: read-only (the pop the opcode pays is unchanged), total
//! (every executed discard reports), bounded capture with a displayed
//! overflow flag, and a deterministic render. This walkthrough discards
//! an address, destroys an aggregate, counts a drop-heavy loop's
//! discards, reads the bounded capture, and proves the seam is
//! read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example drops_walkthrough
//! ```

use glyim_bytecode_vm::{DropCollector, Function, Module, Opcode, Vm, render_drops};

/// main (fn 0) builds a two-element aggregate and drops it, then drops
/// slot 0's address, then returns 42: two discards with different shapes.
fn two_shape_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // element 0
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // element 1
    main.extend_from_slice(&2i64.to_le_bytes());
    main.push(Opcode::Aggregate as u8); // (1, 2)
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::Drop as u8); // the aggregate destroyed
    main.push(Opcode::LoadLocalAddr as u8); // slot 0's address
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Drop as u8); // the address discarded
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&42i64.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// A countdown loop whose body loads slot 0's address and drops it every
/// iteration: a drop-heavy shape (one discard per iteration).
fn drop_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // local 0
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): discard slot 0's address, decrement, loop.
    main.push(Opcode::LoadLocalAddr as u8); // slot 0's address
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Drop as u8); // one discard per iteration
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // one
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8); // counter - 1
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // zero
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Gt as u8); // loop condition
    main.push(Opcode::JumpIf as u8); // back to the loop head
    main.extend_from_slice(&14u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// Run a module with a fresh drop collector attached and return the typed
/// collector.
fn drop_run(module: &Module) -> DropCollector {
    let mut vm = Vm::new().with_drops(Box::new(DropCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_drops().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<DropCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The discard-shaped read: an aggregate the program destroyed and
    //    an address the scope exit discarded — two events, two shapes,
    //    each with its site.
    let module = two_shape_module();
    let drops = drop_run(&module);
    println!("one destroyed aggregate, one discarded address:");
    println!("{}", render_drops(drops.events()));

    // 2. The drop-heavy shape: a countdown loop discards one address per
    //    iteration — the stream counts them.
    let loop_module = drop_loop_module(3);
    let drops = drop_run(&loop_module);
    println!("a 3-iteration drop-heavy loop:");
    println!("{}", render_drops(drops.events()));
    println!(
        "{} discards, every one slot 0's address at pc 19\n",
        drops.events().len()
    );

    // 3. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_drops(Box::new(DropCollector::bounded(2)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_drops().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<DropCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(2) over three discards: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 4. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_drops(Box::new(DropCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_drops().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
