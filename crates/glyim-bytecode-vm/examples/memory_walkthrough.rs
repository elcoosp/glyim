//! Memory walkthrough: run modules with the memory seam attached, then
//! read what every executed frame-memory access did — the stores, the
//! reads, and the reads that never saw a write.
//!
//! The local-watch seam (`WatchCollector` + `render_watch`) records what
//! values a local slot took, and the data-flow seam pairs those writes
//! with the reads — but both watch the slot face. The aggregate lowering
//! also runs on the memory face: `OP_STORE_FIELD` writes a field through
//! an address and `OP_DEREf` reads it back, and the frame's addressable
//! memory is zero-initialised — a read of an address the run never wrote
//! is a silent zero, not an error. The memory seam (`MemCollector` +
//! `render_memory`) is that counterpart: the VM hands it one
//! `MemWriteEvent` per executed store and one `MemReadEvent` per executed
//! deref — the site, the address, and the value in full — so the run's
//! field traffic is readable after the fact, including the
//! unwritten-address reads a wrong field offset would surface through.
//! The hooks observe exactly like the other seams: read-only (the stored
//! and fetched values are unchanged), total over *executed* accesses (a
//! bounds-refused store reports nothing — the fault stream owns it),
//! bounded capture with a displayed overflow flag, and a deterministic
//! render. This walkthrough stores and reads back a field, reads an
//! unwritten address, counts a memory-heavy loop's traffic, reads the
//! bounded capture, and proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example memory_walkthrough
//! ```

use glyim_bytecode_vm::{Function, MemCollector, Module, Opcode, Vm, render_memory};

/// main (fn 0) stores `7` through address 0, reads it back, then reads
/// address 1 (never written): a paired access and an unwritten read in
/// one entry point. The emitter's operand order: the value goes on the
/// stack first, the place address second — the address is on top and the
/// arm pops it first.
fn mixed_memory_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // pc 0 — the value
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::LoadLocalAddr as u8); // pc 9 — the address
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::StoreField as u8); // pc 14 — the store
    main.push(Opcode::LoadLocalAddr as u8); // pc 15 — the address
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Deref as u8); // pc 20 — the paired read
    main.push(Opcode::LoadLocalAddr as u8); // pc 21 — never written
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Deref as u8); // pc 26 — the unwritten read
    main.push(Opcode::Return as u8); // pc 27
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// A countdown loop whose body stores the counter through address 0 and
/// reads it back every iteration: a memory-heavy shape (one write and
/// one read per iteration).
fn store_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // local 0
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): store the counter through address 0, read it
    // back, drop the read, decrement, loop while still positive.
    main.push(Opcode::LoadLocal as u8); // the value
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocalAddr as u8); // the address
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::StoreField as u8); // pc 24 — one write per iteration
    main.push(Opcode::LoadLocalAddr as u8); // the address
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Deref as u8); // pc 30 — one read per iteration
    main.push(Opcode::Drop as u8); // discard the read
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

/// Run a module with a fresh memory collector attached and return the
/// typed collector.
fn memory_run(module: &Module) -> MemCollector {
    let mut vm = Vm::new().with_memory(Box::new(MemCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_memory().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<MemCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The memory-shaped read: one store, its paired read back, and one
    //    unwritten-address read — the zero-initialised slot displayed.
    let module = mixed_memory_module();
    let memory = memory_run(&module);
    println!("one store, one paired read, one unwritten read:");
    println!("{}", render_memory(&memory));

    // 2. The memory-heavy shape: a countdown loop stores and reads back
    //    every iteration — the stream counts both faces.
    let loop_module = store_loop_module(3);
    let memory = memory_run(&loop_module);
    println!(
        "a 3-iteration memory-heavy loop: {} writes, {} reads, {} unwritten reads\n",
        memory.writes().len(),
        memory.reads().len(),
        memory.unwritten_read_count()
    );

    // 3. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_memory(Box::new(MemCollector::bounded(4)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_memory().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<MemCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(4) over six accesses: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 4. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_memory(Box::new(MemCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_memory().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
