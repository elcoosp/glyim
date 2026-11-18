//! Execution-profile walkthrough: run a decoded module with the profile
//! seam attached, then read where its dispatches went.
//!
//! The trace seam (`TraceCollector` + `render_trace`) answers "what did
//! this module *do*, step by step?"; the profile seam (`ProfileCollector` +
//! `render_profile`) answers the aggregate question — "where did the
//! dispatches *go*?" — in O(#distinct opcodes + #functions) memory instead
//! of O(#steps). This walkthrough attaches a `ProfileCollector` to a `Vm`
//! over the two-function module the other VM walkthroughs use, renders the
//! aggregate, and then shows the seam's honesty rules: an unknown opcode
//! byte folds into its own `unknown(0xNN)` bucket, the fuel budget's
//! refused dispatches never reach the hook (profile total == executed
//! count), and a captured `TraceCollector` transcript re-folds into the
//! identical profile through `from_steps`. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example execution_profile_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, ProfileCollector, TraceCollector, Vm, VmError, decode_module,
    encode_module, render_profile, render_trace,
};

/// helper(n): returns `n * 2`. No jumps, so it carries no block-offset
/// table (the VM's raw byte-offset target mode).
fn helper() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&2i64.to_le_bytes());
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // MIR ABI: arg at local 1
    code.push(Opcode::Mul as u8);
    code.push(Opcode::Return as u8);
    Function::new(code, 2, 1)
}

/// `main()`: returns `helper(21)` — the cross-function `OP_CALL` shape the
/// profile splits across two `fn` lines.
fn main_fn() -> Function {
    let mut code = Vec::new();
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&0i64.to_le_bytes()); // fn index 0: helper
    code.push(Opcode::LoadConst as u8);
    code.extend_from_slice(&21i64.to_le_bytes());
    code.push(Opcode::Call as u8);
    code.extend_from_slice(&1u32.to_le_bytes()); // argc
    code.extend_from_slice(&0u32.to_le_bytes()); // dest local
    code.extend_from_slice(&31u32.to_le_bytes()); // resume byte offset
    code.push(Opcode::LoadLocal as u8);
    code.extend_from_slice(&0u32.to_le_bytes());
    code.push(Opcode::Return as u8);
    Function::new(code, 1, 0)
}

fn main() {
    // The image round trip composes with the profile: ship the bytes,
    // decode them back, then profile the loaded module.
    let module = Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1);
    let bytes = encode_module(&module);
    let loaded = decode_module(&bytes).expect("the artifact we just encoded decodes");

    let collector = ProfileCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(collector));
    let value = vm.run_module(&loaded).expect("the profiled run succeeds");
    let mut boxed = vm.take_trace().expect("the sink is still attached");
    let profile = boxed
        .as_any()
        .downcast_ref::<ProfileCollector>()
        .expect("the collector came back through as_any")
        .clone();

    println!("result: {value:?} (helper(21) — the profile is read-only)");
    println!();
    print!("{}", render_profile(&profile, &loaded));
    println!();
    println!(
        "hottest(2): {:?}",
        profile
            .hottest(2)
            .iter()
            .map(|hot| (hot.opcode, hot.dispatches))
            .collect::<Vec<_>>()
    );

    // The same run through a capturing collector: the transcript and the
    // aggregate are two views of one stream, and `from_steps` moves between
    // them after the fact.
    let capture = TraceCollector::new();
    let mut vm = Vm::new().with_trace(Box::new(capture));
    let _ = vm.run_module(&loaded).expect("the captured run succeeds");
    let mut boxed = vm.take_trace().expect("attached");
    let steps = boxed
        .as_any()
        .downcast_ref::<TraceCollector>()
        .expect("collector")
        .steps()
        .to_vec();
    let refolded = ProfileCollector::from_steps(&steps);
    assert_eq!(
        render_profile(&refolded, &loaded),
        render_profile(&profile, &loaded),
        "the fold is a pure function of the step stream"
    );
    println!();
    println!(
        "from_steps over a {}-step capture re-folds to the identical profile",
        steps.len()
    );
    println!("(the step stream itself: {} bytes of listing)", {
        let _ = &steps;
        render_trace(&steps, &loaded).len()
    });

    // An unknown opcode byte folds into its own bucket and stays visible —
    // the profile is total over corrupt code, never a silent skip.
    let corrupt = Module::new(
        vec![Function::new(vec![0x42, Opcode::Return as u8], 0, 0)],
        0,
    );
    let mut vm = Vm::new().with_trace(Box::new(ProfileCollector::new()));
    let err = vm.run_module(&corrupt).expect_err("0x42 is not an opcode");
    assert_eq!(err, VmError::UnknownOpcode(0x42));
    let mut boxed = vm.take_trace().expect("attached");
    let p = boxed
        .as_any()
        .downcast_ref::<ProfileCollector>()
        .expect("collector");
    println!();
    print!("{}", render_profile(p, &corrupt));

    // The fuel budget composes: the refused dispatch never reaches the
    // hook, so the profile's total equals `Vm::executed` exactly.
    let mut code = Vec::new();
    code.push(Opcode::Jump as u8);
    code.extend_from_slice(&0u32.to_le_bytes()); // self-jump: b0 -> b0
    let spinner = Module::new(vec![Function::with_blocks(code, 0, 0, vec![0])], 0);
    let mut vm = Vm::new()
        .with_fuel(500)
        .with_trace(Box::new(ProfileCollector::new()));
    match vm.run_module(&spinner) {
        Err(VmError::FuelExhausted { executed }) => {
            let mut boxed = vm.take_trace().expect("attached");
            let p = boxed
                .as_any()
                .downcast_ref::<ProfileCollector>()
                .expect("collector");
            println!();
            println!(
                "infinite loop stopped by fuel: executed={executed}, profiled={} (the refused dispatch never reaches the hook)",
                p.steps()
            );
            assert_eq!(p.steps(), executed);
        }
        _ => unreachable!("a self-jump loop cannot halt"),
    }
}
