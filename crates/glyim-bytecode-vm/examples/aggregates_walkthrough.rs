//! Aggregates walkthrough: run modules with the composite-construction
//! seam attached, then read what every executed `OP_AGGREGATE` fold and
//! `OP_REPEAT` replication built — the sites, the folded elements, the
//! raw repeat counts, and the materialization totals.
//!
//! Every other value seam watches a value *after* it exists: the watch
//! seam records what slots took, the drop seam records what the run
//! destroyed, the cast seam records what the run transformed. The
//! moment of *construction* was invisible — a lowering bug that
//! aggregates the wrong operand span, or repeats into the wrong
//! length, produced a wrong tuple with no observation anywhere on its
//! path. The aggregates seam (`AggregateCollector` +
//! `render_aggregates`) is that counterpart: the VM hands it one
//! `AggregateBuilt` per executed fold (the site, the arity, the folded
//! elements in full) and one `RepeatBuilt` per executed replication
//! (the site, the raw count, the replicated value), so a run's
//! construction traffic is readable after the fact — including the
//! negative-count clamp, which the event displays instead of silently
//! pairing away. The hooks observe exactly like the other seams:
//! read-only (the built tuple is unchanged), total over *executed*
//! constructions (a fold the machine refuses — an operand-stack
//! underflow mid-pop — faults and reports nothing; the fault stream
//! owns it), bounded capture with a displayed overflow flag, and a
//! deterministic render. This walkthrough builds a pair, counts a
//! repeat-heavy loop's traffic, reads the negative-count clamp, reads
//! the bounded capture, and proves the seam is read-only. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example aggregates_walkthrough
//! ```

use glyim_bytecode_vm::{AggregateCollector, Function, Module, Opcode, Vm, render_aggregates};

/// main (fn 0) builds the pair `[21, 42]` from two constants and
/// returns it: the canonical one-construction module.
fn build_pair_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // pc 0 — the first element
    main.extend_from_slice(&21i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // pc 9 — the second element
    main.extend_from_slice(&42i64.to_le_bytes());
    main.push(Opcode::Aggregate as u8); // pc 18 — fold the top two
    main.extend_from_slice(&2u32.to_le_bytes());
    main.push(Opcode::Return as u8); // pc 23 — pops the tuple
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// main (fn 0) repeats `7` once per countdown iteration: the
/// construction-heavy shape — one replication per iteration.
fn repeat_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): repeat 7 once, drop the tuple, decrement, loop
    // while still positive.
    main.push(Opcode::LoadConst as u8); // the value
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // one
    main.extend_from_slice(&1i64.to_le_bytes());
    main.push(Opcode::Repeat as u8); // one repeat per iteration
    main.push(Opcode::Drop as u8); // discard the tuple
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
    Module::new(vec![Function::new(main, 1, 0)], 0)
}

/// main (fn 0) repeats `7` a negative number of times: the clamp shape
/// — the machine produces the empty tuple, and the event keeps the raw
/// count.
fn negative_repeat_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the value
    main.extend_from_slice(&7i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8); // the negative count
    main.extend_from_slice(&(-2i64).to_le_bytes());
    main.push(Opcode::Repeat as u8); // clamps to []
    main.push(Opcode::Return as u8); // pops the empty tuple
    Module::new(vec![Function::new(main, 0, 0)], 0)
}

/// Run a module with a fresh aggregate collector attached and return
/// the typed collector.
fn aggregates_run(module: &Module) -> AggregateCollector {
    let mut vm = Vm::new().with_aggregates(Box::new(AggregateCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_aggregates().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<AggregateCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The one-fold shape: the pair build names its site, its arity,
    //    and its folded elements in source order.
    let module = build_pair_module();
    let aggregates = aggregates_run(&module);
    println!("one fold, one event:");
    println!("{}", render_aggregates(&aggregates));

    // 2. The construction-heavy shape: a countdown loop repeats one
    //    value every iteration — the tallies count them all.
    let loop_module = repeat_loop_module(3);
    let aggregates = aggregates_run(&loop_module);
    println!(
        "a 3-iteration repeat-heavy loop: {} constructions, {} elements materialized, max arity {}\n",
        aggregates.events().len(),
        aggregates.total_elements(),
        aggregates.max_arity()
    );

    // 3. The clamp shape: a negative repeat is not a fault — the
    //    machine produces the empty tuple and the event keeps the raw
    //    count (displayed, never silently clamped away).
    let negative = negative_repeat_module();
    let aggregates = aggregates_run(&negative);
    println!("the negative-count clamp, displayed:");
    println!("{}", render_aggregates(&aggregates));

    // 4. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_aggregates(Box::new(AggregateCollector::bounded(2)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_aggregates().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<AggregateCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(2) over three constructions: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 5. The read-only proof: the traced run's result equals the
    //    untraced one exactly (the seam observes — it never changes
    //    execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_aggregates(Box::new(AggregateCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_aggregates().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
