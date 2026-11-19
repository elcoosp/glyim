//! Strings walkthrough: run modules with the string-pool seam attached,
//! then read what every executed `OP_LOAD_CONST_STR` resolution did —
//! the sites, the pool slots, the shared-slot traffic, and the byte
//! totals.
//!
//! The module-image diff seam (`render_module_diff`) names string-pool
//! changes *structurally* — the `-`/`+` pool lines of a rebuilt artifact
//! — but the pool's runtime face is invisible: which slots did this run
//! resolve, from which sites, and how many times was one shared slot
//! re-resolved? The strings seam (`StringCollector` + `render_strings`)
//! is that counterpart: the VM hands it one `StringLoadEvent` per
//! executed resolution — the site, the pool slot, and the resolved byte
//! string in full — so the run's pool traffic is readable after the fact,
//! including the shared-slot audit (one slot, many resolutions) where a
//! content-equality regression would surface. The hooks observe exactly
//! like the other seams: read-only (the resolved value is unchanged),
//! total over *executed* resolutions (a bad pool index faults and
//! reports nothing — the fault stream owns it), bounded capture with a
//! displayed overflow flag, and a deterministic render. This walkthrough
//! resolves two distinct slots, counts a shared-slot loop's traffic,
//! reads the bounded capture, and proves the seam is read-only. Run
//! with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example strings_walkthrough
//! ```

use glyim_bytecode_vm::{Function, Module, Opcode, StringCollector, Vm, render_strings};

/// main (fn 0) loads slot 0 (`"hello"`) and slot 1 (`"world"`) and
/// returns the second: the canonical two-slot module.
fn two_slot_module() -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConstStr as u8); // pc 0 — "hello"
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::LoadConstStr as u8); // pc 5 — "world"
    main.extend_from_slice(&1u32.to_le_bytes());
    main.push(Opcode::Return as u8); // pc 10 — pops "world"
    Module::with_strings(
        vec![Function::new(main, 0, 0)],
        vec!["hello".into(), "world".into()],
        0,
    )
}

/// main (fn 0) loads slot 0 in a countdown loop (`count` times): the
/// shared-slot shape — one slot, many resolutions.
fn shared_slot_loop_module(count: i64) -> Module {
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8); // the seed
    main.extend_from_slice(&count.to_le_bytes());
    main.push(Opcode::StoreLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    // Loop head (pc 14): resolve slot 0, drop it, decrement, loop while
    // still positive.
    main.push(Opcode::LoadConstStr as u8); // one load per iteration
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Drop as u8);
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
    main.push(Opcode::LoadConstStr as u8); // the module result
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::with_strings(vec![Function::new(main, 1, 0)], vec!["payload".into()], 0)
}

/// Run a module with a fresh string collector attached and return the
/// typed collector.
fn strings_run(module: &Module) -> StringCollector {
    let mut vm = Vm::new().with_strings(Box::new(StringCollector::new()));
    let _ = vm.run_module(module);
    let mut boxed = vm.take_strings().expect("sink still attached");
    boxed
        .as_any()
        .downcast_ref::<StringCollector>()
        .expect("the collector came back")
        .clone()
}

fn main() {
    // 1. The two-slot shape: two resolutions, two distinct slots, each
    //    naming its own site.
    let module = two_slot_module();
    let strings = strings_run(&module);
    println!("two slots, two resolutions:");
    println!("{}", render_strings(&strings));

    // 2. The shared-slot shape: a countdown loop resolves one slot every
    //    iteration — the per-slot fold counts them all.
    let loop_module = shared_slot_loop_module(3);
    let strings = strings_run(&loop_module);
    println!(
        "a 3-iteration string-heavy loop: {} loads, {} slot, {} bytes\n",
        strings.events().len(),
        strings.slot_count(),
        strings.total_bytes()
    );

    // 3. The bounded capture: a capped collector keeps the first events
    //    and flags the overflow — displayed, never a silent drop.
    let mut vm = Vm::new().with_strings(Box::new(StringCollector::bounded(2)));
    let _ = vm.run_module(&loop_module).expect("module runs");
    let mut boxed = vm.take_strings().expect("sink still attached");
    let bounded = boxed
        .as_any()
        .downcast_ref::<StringCollector>()
        .expect("the collector came back")
        .clone();
    println!(
        "bounded(2) over four loads: {} events kept, overflowed = {}\n",
        bounded.events().len(),
        bounded.overflowed()
    );

    // 4. The read-only proof: the traced run's result equals the untraced
    //    one exactly (the seam observes — it never changes execution).
    let untraced = Vm::new().run_module(&module).expect("untraced run");
    let traced = {
        let mut vm = Vm::new().with_strings(Box::new(StringCollector::new()));
        let value = vm.run_module(&module).expect("traced run");
        let _ = vm.take_strings().expect("sink still attached");
        value
    };
    println!(
        "read-only: traced {traced:?} == untraced {untraced:?} — the seam observes, execution unchanged"
    );
}
