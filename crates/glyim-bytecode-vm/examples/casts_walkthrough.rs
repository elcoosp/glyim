//! Casts walkthrough: run modules with the cast seam attached, then read
//! what every executed cast did to its value — the narrowing wraps, the
//! float truncations, the identities — and where each cast happened.
//!
//! The execution-trace seam (`TraceCollector` + `render_trace`) records
//! that an `OP_CAST` dispatched, and its step detail names the cast's
//! kind and target bytes. Neither keeps the values: the operand the cast
//! consumed and the result it produced are folded away by the one-line
//! step entries, and the run's own output shows only the last cast's
//! result. The cast seam (`CastCollector` + `render_casts`) is that
//! counterpart: the VM hands it one `CastEvent` per executed cast — the
//! site, the decoded kind/target pair, and the operand/result pair in
//! full — so the emitter's cast surface (`(-1i32) as u8 == 255`,
//! `(3.99) as i32 == 3`) is readable after the run. The hook observes
//! exactly like the other seams: read-only (the pushed result is
//! unchanged), total over *executed* casts (a faulting cast reports
//! nothing — the fault stream owns it), bounded capture with a displayed
//! overflow flag, and a deterministic render. This walkthrough narrows a
//! negative int through `u8`, truncates a float through `i64`, counts a
//! cast-heavy loop's casts, reads the bounded capture, and proves the
//! seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example casts_walkthrough
//! ```

use glyim_bytecode_vm::{CastCollector, CastKind, Function, Module, Opcode, Vm, render_casts};

/// main (fn 0) narrows `-1` through `u8` (the wrap shape), truncates
/// `3.99` through `i64` (the float shape), and casts `7` over the
/// `Unknown` target (the identity shape — the pre-seam VM's behavior for
/// every cast), then returns the last result.
fn three_shape_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // pc 0 — the operand (-1)
    main.extend_from_slice(&(-1i64).to_le_bytes());
    main.push(Opcode::Cast as u8); // pc 9 — IntToInt -> U8: wrap
    main.push(0u8);
    main.push(6u8);
    main.push(Opcode::LoadConstF64 as u8); // pc 12 — the operand (3.99)
    main.extend_from_slice(&3.99f64.to_le_bytes());
    main.push(Opcode::Cast as u8); // pc 21 — FloatToInt -> I64: truncate
    main.push(1u8);
    main.push(4u8);
    main.push(Opcode::LoadConst as u8); // pc 24 — the operand (7)
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::Cast as u8); // pc 33 — IntToInt -> Unknown: identity
    main.push(0u8);
    main.push(0u8);
    main.push(Opcode::Return as u8); // pc 36
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// A countdown loop whose body casts the counter through `u8` and drops
/// the result every iteration: a cast-heavy shape (one cast per
/// iteration).
fn cast_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8); // local 0
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): decrement, cast through u8, drop, loop.
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // one
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Sub as u8); // counter - 1
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8); // the counter
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Cast as u8); // pc 34 — one cast per iteration
    main.push(0u8); // IntToInt
    main.push(6u8); // U8
    main.push(Opcode::Drop as u8); // discard the cast result
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

/// Run a module with a fresh cast collector attached and return the typed
/// collector.
fn cast_run(module: &Module) -> CastCollector {
    let mut vm = Vm::new().with_casts(Box::new(CastCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_casts().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<CastCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The cast-shaped read: a narrowing wrap, a float truncation, and
    //    an identity — three events, three shapes, each with its site.
    let module = three_shape_module();
    let casts = cast_run(&module);
    println!("one wrap, one truncation, one identity:");
    println!("{}", render_casts(casts.events()));
    println!(
        "{} casts changed the value (the identity did not)\n",
        casts.changed_count()
    );

    // 2. The float-cast audit: the stream groups by kind, so the run's
    //    `FloatToInt` traffic is one query.
    let float_traffic = casts.events_of_kind(CastKind::FloatToInt);
    let first = float_traffic.first();
    println!(
        "FloatToInt traffic: {} cast(s), {:?} -> {:?}\n",
        float_traffic.len(),
        first.map(|e| &e.operand),
        first.map(|e| &e.result),
    );

    // 3. The cast-heavy shape: a countdown loop casts its counter every
    //    iteration — the stream counts them — and a bounded collector
    //    keeps the first events and flags the overflow (displayed, never
    //    a silent drop).
    let loop_module = cast_loop_module(3);
    let casts = cast_run(&loop_module);
    println!(
        "a 3-iteration cast-heavy loop: {} casts, every one at the loop body's cast site",
        casts.events().len()
    );
    let mut vm = Vm::new().with_casts(Box::new(CastCollector::bounded(2)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_casts().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<CastCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(2) over three casts: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 4. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_casts(Box::new(CastCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_casts().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
