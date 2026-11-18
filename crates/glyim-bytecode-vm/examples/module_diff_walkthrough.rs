//! Module-image diff walkthrough: decode two versions of an artifact and
//! render what changed between them.
//!
//! The disassembler lists what one artifact *carries*; this walkthrough
//! shows the *change* question — a rebuild after an emitter tweak, decoded
//! on both sides, diffed structurally (functions, code bytes with their
//! first-diff pc, block tables, the string pool, the entry point). The
//! identical pair renders the explicit `no differences` line (the
//! no-op-rebuild regression gate), and the divergent pair names every
//! difference in index order. Run with:
//!
//! ```text
//! cargo run -p glyim-bytecode-vm --example module_diff_walkthrough
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, decode_module, encode_module, render_module_diff,
};

/// helper(n): returns `n * 2`.
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

/// `main()`: returns `helper(21)`.
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

fn golden() -> Module {
    Module::with_strings(vec![helper(), main_fn()], vec!["abc".to_string()], 1)
}

fn main() {
    // The no-op rebuild: encode → decode → diff is the regression gate any
    // emitter change must pass when it should not alter shipped bytes.
    let old = decode_module(&encode_module(&golden())).expect("the baseline decodes");
    let same = decode_module(&encode_module(&golden())).expect("the rebuild decodes");
    println!("no-op rebuild:");
    print!("{}", render_module_diff(&old, &same));

    // A real change: helper doubles -> triples (its constant byte moves),
    // the pool entry is re-spelled, a function is added, and the entry
    // point moves — every difference lands in one deterministic listing.
    let mut new = golden();
    new.functions[0].code[1] = 3;
    new.strings[0] = "abd".to_string();
    new.functions
        .push(Function::new(vec![Opcode::Return as u8], 0, 0));
    new.entry = 0; // the entry point moves to helper — an explicit line below
    let loaded = decode_module(&encode_module(&new)).expect("the rebuilt artifact decodes");
    println!();
    println!("divergent rebuild:");
    print!("{}", render_module_diff(&old, &loaded));
}
