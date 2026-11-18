//! Module-image diff timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `module_image` /
//! `module_disasm` benches in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. The diff seam (the `diff` module) is a pure
//! function over two decoded modules, so its headline numbers are the
//! **identical fast path** (the shape the no-op-rebuild regression gate
//! walks — the union walk still visits every index) and the **divergent
//! render** (a changed function plus changed pool entries). Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench module_diff
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, decode_module, encode_module, render_module_diff,
};
use std::hint::black_box;
use std::time::Instant;

/// helper(n) = n * 2 + main returning helper(21): the multi-function
/// golden shape the disasm/trace benches use.
fn golden_module() -> Module {
    let mut helper = Vec::new();
    helper.push(Opcode::LoadConst as u8);
    helper.extend_from_slice(&2i64.to_le_bytes());
    helper.push(Opcode::LoadLocal as u8);
    helper.extend_from_slice(&1u32.to_le_bytes());
    helper.push(Opcode::Mul as u8);
    helper.push(Opcode::Return as u8);
    let mut main = Vec::new();
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&0i64.to_le_bytes());
    main.push(Opcode::LoadConst as u8);
    main.extend_from_slice(&21i64.to_le_bytes());
    main.push(Opcode::Call as u8);
    main.extend_from_slice(&1u32.to_le_bytes());
    main.extend_from_slice(&0u32.to_le_bytes());
    main.extend_from_slice(&31u32.to_le_bytes());
    main.push(Opcode::LoadLocal as u8);
    main.extend_from_slice(&0u32.to_le_bytes());
    main.push(Opcode::Return as u8);
    Module::with_strings(
        vec![Function::new(helper, 2, 1), Function::new(main, 1, 0)],
        vec!["abc".to_string()],
        1,
    )
}

fn main() {
    // Both sides ride the image round trip (the seam's real consumers diff
    // decoded artifacts, not in-memory values).
    let old = decode_module(&encode_module(&golden_module())).expect("old decodes");
    let mut new_module = golden_module();
    new_module.functions[0].code[1] = 3;
    new_module.strings[0] = "abd".to_string();
    let new = decode_module(&encode_module(&new_module)).expect("new decodes");

    let start = Instant::now();
    let mut identical_bytes = 0usize;
    for _ in 0..10_000 {
        identical_bytes += black_box(render_module_diff(black_box(&old), black_box(&old))).len();
    }
    let identical = start.elapsed();
    println!("identical fast path (10000 renders, {identical_bytes}-byte total): {identical:?}");

    let start = Instant::now();
    let mut divergent_bytes = 0usize;
    for _ in 0..10_000 {
        divergent_bytes += black_box(render_module_diff(black_box(&old), black_box(&new))).len();
    }
    let divergent = start.elapsed();
    println!("divergent render (10000 renders, {divergent_bytes}-byte total): {divergent:?}");
}
