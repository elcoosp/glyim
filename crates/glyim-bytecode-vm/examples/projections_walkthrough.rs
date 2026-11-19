//! Projections walkthrough: run modules with the composite-projection
//! seam attached, then read what every executed `OP_LEN` read and
//! `OP_DISCRIMINANT` read asked — the sites, the projected operands,
//! the answers, and the scalar-convention traffic.
//!
//! The aggregates seam (`AggregateCollector` + `render_aggregates`)
//! watches the moment of *construction*; nothing watched the moment of
//! *interrogation*. The `OP_LEN` length read and the
//! `OP_DISCRIMINANT` discriminant read are the machine's only
//! aggregate-projection opcodes, and both carry a documented *scalar
//! convention* — a projection over a non-aggregate value answers `0` —
//! so a lowering bug that projects a scalar, or a field-offset bug
//! that reads the wrong slot's length, lands in that convention as a
//! silent zero. The projections seam (`ProjectionCollector` +
//! `render_projections`) is that counterpart: the VM hands it one
//! `LenRead` per executed length read (the site, the local index, the
//! operand in full, the answer) and one `DiscrRead` per executed
//! discriminant read (the site, the popped operand, the answer), and
//! its `scalar_reads()` fold keeps the convention path visible instead
//! of silently consumed. The hooks observe exactly like the other
//! seams: read-only (the answer is unchanged), total over *executed*
//! projections (a read the machine refuses — an out-of-bounds local
//! index — faults and reports nothing; the fault stream owns it),
//! bounded capture with a displayed overflow flag, and a deterministic
//! render. This walkthrough reads a triple's length, reads a popped
//! tuple's discriminant, lands in the scalar convention, counts a
//! projection-heavy loop's traffic, reads the bounded capture, and
//! proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example projections_walkthrough
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, ProjectionCollector, Vm, render_projections};

/// main (fn 0) builds the triple `(1, 2, 3)` into local 0 and reads
/// its length: the canonical one-projection module.
fn len_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // pc 0 — the first element
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // pc 9 — the second element
    main.extend_from_slice(&2i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // pc 18 — the third element
    main.extend_from_slice(&3i64.to_le_bytes());
    main.push(Opcode::Aggregate as u8); // pc 27 — build (1, 2, 3)
    main.extend_from_slice(&3u32.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // pc 32 — bind local 0
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Len as u8); // pc 37 — read local 0's length
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8); // pc 42 — pops the answer
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// main (fn 0) builds the triple `(1, 2, 3)` into local 0, pushes a
/// copy, and reads its discriminant: the stack-popped projection.
fn discr_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the first element
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the second element
    main.extend_from_slice(&2i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the third element
    main.extend_from_slice(&3i64.to_le_bytes());
    main.push(Opcode::Aggregate as u8); // build (1, 2, 3)
    main.extend_from_slice(&3u32.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // bind local 0
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // push a copy
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Discriminant as u8); // pop and project
    main.push(Opcode::Return as u8); // pops the answer
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// main (fn 0) stores the scalar `7` into local 0 and reads its
/// length: the *scalar convention* shape — the machine answers `0`.
fn scalar_len_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the scalar
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // bind local 0
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Len as u8); // read local 0's length
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8); // pops the 0
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// main (fn 0) reads the length of a freshly built pair in a countdown
/// loop (`count` iterations): the projection-heavy shape — one length
/// read per iteration.
fn len_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): build (1, 2) into local 1, read its length,
    // drop the answer, decrement, loop while still positive.
    main.push(Opcode::LoadConst as u8); // the first element
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the second element
    main.extend_from_slice(&2i64.to_le_bytes());
    main.push(Opcode::Aggregate as u8); // build (1, 2)
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // bind local 1
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Len as u8); // read local 1's length
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Drop as u8); // discard the answer
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // one
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8);
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // zero
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::Gt as u8); // loop condition
    main.push(Opcode::JumpIf as u8); // back to the loop head
    main.extend_from_slice(&14u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::new(vec![Function::new(main, 2, 0)], 0)
}

/// Run a module with a fresh projection collector attached and return
/// the typed collector.
fn projections_run(module: &Module) -> ProjectionCollector {
    let mut vm = Vm::new().with_projections(Box::new(ProjectionCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_projections().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<ProjectionCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The one-read shape: the length read names its site, its
    //    local, its projected operand, and the machine's answer.
    let module = len_module();
    let projections = projections_run(&module);
    println!("one length read, one event:");
    println!("{}", render_projections(&projections));

    // 2. The popped shape: the discriminant read over a copy the run
    //    pushed — the aggregate seam built the tuple, this read says
    //    what the run then asked about it.
    let discr = discr_module();
    let projections = projections_run(&discr);
    println!("the discriminant read:");
    println!("{}", render_projections(&projections));

    // 3. The scalar-convention shape: a projection over a non-aggregate
    //    answers `0`, and the scalar fold keeps the path visible.
    let scalar = scalar_len_module();
    let projections = projections_run(&scalar);
    println!("the scalar convention (answered 0):");
    println!("{}", render_projections(&projections));

    // 4. The projection-heavy shape: a countdown loop reads one length
    //    per iteration — the tallies count them all.
    let loop_module = len_loop_module(3);
    let projections = projections_run(&loop_module);
    println!(
        "a 3-iteration projection-heavy loop: {} reads, {} scalar reads\n",
        projections.events().len(),
        projections.scalar_reads()
    );

    // 5. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_projections(Box::new(ProjectionCollector::bounded(2)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_projections().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<ProjectionCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(2) over three reads: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 6. The read-only proof: the traced run's result equals the
    //    untraced one exactly (the seam observes — it never changes
    //    execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_projections(Box::new(ProjectionCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_projections().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
