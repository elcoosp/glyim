//! Module-disassembly timing harness.
//!
//! Dependency-free (`harness = false`), mirroring the `module_image`
//! bench in this crate: only `std::time::Instant` +
//! `std::hint::black_box`. Rendering a decoded module back to its
//! deterministic instruction listing is the observability half of the
//! image seam (`encode_module`/`decode_module`/`render_module`), so its
//! per-call cost must stay small against a real compile. This bench
//! measures the full-module render (header + both function listings +
//! pool section) and the single-function render over the same corpus, so
//! CI logs can track headroom. Run with:
//!
//! ```text
//! cargo bench -p glyim-bytecode-vm --bench module_disasm
//! ```

use glyim_bytecode_vm::{
    Function, Module, Opcode, decode_module, encode_module, render_function, render_module,
};
use std::hint::black_box;
use std::time::Instant;

/// The lib-test assembler, mirrored for the measured corpus: records
/// basic-block offsets so jump targets can reference blocks by index.
struct Asm {
    code: Vec<u8>,
    blocks: Vec<usize>,
}

impl Asm {
    fn new() -> Self {
        Asm {
            code: Vec::new(),
            blocks: Vec::new(),
        }
    }
    fn label(&mut self, idx: usize) {
        while self.blocks.len() <= idx {
            self.blocks.push(0);
        }
        self.blocks[idx] = self.code.len();
    }
    fn op(&mut self, o: Opcode) {
        self.code.push(o as u8);
    }
    fn u32(&mut self, v: u32) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.code.extend_from_slice(&v.to_le_bytes());
    }
    fn load_const(&mut self, v: i64) {
        self.op(Opcode::LoadConst);
        self.i64(v);
    }
    fn load_local(&mut self, idx: u32) {
        self.op(Opcode::LoadLocal);
        self.u32(idx);
    }
    fn finish(self, n_locals: usize, arg_count: usize) -> Function {
        Function::with_blocks(self.code, n_locals, arg_count, self.blocks)
    }
}

/// fib(n) via recursion + main calling fib(6): the cross-function,
/// cross-frame shape the disassembler's most valuable listing covers.
fn fib_main_module() -> Module {
    let mut fib = Asm::new();
    fib.label(0);
    fib.load_local(1); // MIR ABI: the argument lives at local 1
    fib.load_const(2);
    fib.op(Opcode::Lt);
    fib.op(Opcode::JumpIf);
    fib.u32(3);
    fib.label(1);
    fib.load_const(0);
    fib.load_local(1);
    fib.load_const(1);
    fib.op(Opcode::Sub);
    fib.op(Opcode::Call);
    fib.u32(1);
    fib.u32(2);
    fib.u32(2);
    fib.label(2);
    fib.load_const(0);
    fib.load_local(1);
    fib.load_const(2);
    fib.op(Opcode::Sub);
    fib.op(Opcode::Call);
    fib.u32(1);
    fib.u32(3);
    fib.u32(4);
    fib.label(4);
    fib.load_local(2);
    fib.load_local(3);
    fib.op(Opcode::Add);
    fib.op(Opcode::Return);
    fib.label(3);
    fib.load_local(1);
    fib.op(Opcode::Return);

    let mut main = Asm::new();
    main.label(0);
    main.load_const(0);
    main.load_const(6);
    main.op(Opcode::Call);
    main.u32(1);
    main.u32(0);
    main.u32(1);
    main.label(1);
    main.load_local(0);
    main.op(Opcode::Return);

    Module::new(vec![fib.finish(8, 1), main.finish(8, 0)], 1)
}

/// How many times to render each target. A full-module render is a few
/// hundred string allocations, so a large iteration count keeps the
/// wall-clock reading above timer noise.
const ITERS: u32 = 2_000;

fn main() {
    // Contract pins: the decoded artifact renders, deterministically.
    let module = fib_main_module();
    let image = encode_module(&module);
    let decoded = decode_module(&image).expect("measured corpus must decode");
    let first = render_module(&decoded);
    assert_eq!(
        first,
        render_module(&decoded),
        "render must be deterministic"
    );
    assert!(first.contains("OP_CALL"), "call sites must list");
    black_box(&first);

    // Full-module render: the header + every function listing + the pool.
    let started = Instant::now();
    for _ in 0..ITERS {
        let text = render_module(black_box(&decoded));
        black_box(text.len());
    }
    let elapsed = started.elapsed();
    let per_call = elapsed.as_nanos() / u128::from(ITERS);
    println!(
        "render_module: {ITERS} iters, {elapsed:?} total, ~{per_call} ns/call ({} bytes/listing)",
        first.len()
    );

    // Single-function render: the diagnostic slice (one fn, no header/pool).
    let f = &decoded.functions[0];
    let started = Instant::now();
    for _ in 0..ITERS {
        let text = render_function(black_box(f));
        black_box(text.len());
    }
    let elapsed = started.elapsed();
    let per_call = elapsed.as_nanos() / u128::from(ITERS);
    println!("render_function: {ITERS} iters, {elapsed:?} total, ~{per_call} ns/call");
}
