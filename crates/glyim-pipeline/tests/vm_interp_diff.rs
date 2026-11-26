//! Bytecode VM ≡ MIR interpreter differential test.
//!
//! Compiles a source program to MIR (the shared front half), then runs it
//! two ways:
//!   1. through the MIR interpreter (ground truth), and
//!   2. through the bytecode backend: `generate_function` → raw bytes →
//!      `Module::run` (single-function programs), or
//!      `generate_module_image` → `decode_module` → `Module::run`
//!      (multi-function programs, below).
//!
//! Both must produce the same observable result. This is the only test that
//! exercises the *emitter's* output through the VM, and it is the class of
//! test that would have caught the inverted-branch (RT-11) and
//! index-stride (RT-13) bugs, which per-opcode unit tests miss.
//!
//! SCOPE: single-function programs run the `main` body through the
//! hand-assembled one-function `Module`; multi-function programs run every
//! body through `generate_module_image`, which resolves the emitter's
//! fn-table index space into a loadable image (cross-function `OP_CALL`,
//! cross-frame resume, and mutual recursion are covered below). The
//! image-assembly path refuses programs whose fn table holds several
//! substitution slots for one emitted body — see the ambiguity notes on
//! `generate_module_image`.
//!
//! HISTORY: the first draft of this file failed with `VmError::EmptyReturn`
//! on every fixture (BC-RETURN): the emitter's `TerminatorKind::Return` arm
//! emitted a bare `OP_RETURN`, but MIR's `Return` means "the value is in
//! local 0" while the VM's `OP_RETURN` pops the operand stack. The emitter
//! now pushes local 0 before `OP_RETURN` for value-returning functions
//! (unit-returning ones push nothing), and the VM's `OP_RETURN` treats an
//! empty stack as Unit rather than `EmptyReturn`. The byte-length goldens
//! were unaffected because their synthetic bodies return unit; only `t99`
//! needed updating (it had asserted the buggy post-run state).

use glyim_core::TargetInfo;
use glyim_core::def_id::{CrateId, DefId, LocalDefId};
use glyim_db::{CrateConfig, Database};
use glyim_mir::Body;
use glyim_mir_interp::{InterpValue, Interpreter};
use glyim_pipeline::{Pipeline, compile_file_to_mir};

fn run_dual(src: &str) -> (i64, i64) {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "glyim_vm_diff_{}_{}.g",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, src).expect("write fixture");

    let config = CrateConfig {
        name: "vm_diff".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path, 2).expect("fixture must compile to MIR");
    let main_local = Pipeline::entry_main_local_id(&mut db, &path).expect("resolve main");
    let main_id = DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(main_local as usize),
    );
    let main_body = mir.bodies.get(&main_id).expect("main body").clone();

    // --- interpreter (ground truth) ---
    let mut interp = Interpreter::new(mir.ty_ctx.as_ref());
    for b in mir.bodies.values() {
        interp.add_function(b.owner, (**b).clone());
    }
    interp
        .run_body(&main_body)
        .expect("interpreter must run clean");
    let interp_val = match interp.get_return_value() {
        Some(InterpValue::Int(n)) => n as i64,
        Some(InterpValue::Uint(n)) => n as i64,
        Some(InterpValue::Bool(b)) => i64::from(b),
        _ => 0,
    };

    // --- bytecode VM ---
    let backend =
        glyim_codegen::BytecodeBackend::with_ty_ctx(mir.ty_ctx.clone(), TargetInfo::default());
    let (bytes, block_offsets) = backend
        .generate_function_with_blocks(&main_body)
        .expect("bytecode generation must succeed");
    let f = glyim_bytecode_vm::Function::with_blocks(
        bytes,
        main_body.locals.len(),
        main_body.arg_count,
        block_offsets,
    );
    let module = glyim_bytecode_vm::Module::new(vec![f], 0);
    let vm_val = match module.run() {
        Ok(glyim_bytecode_vm::Value::Int(n)) => n,
        Ok(other) => panic!("VM returned an unexpected value: {other:?}"),
        Err(e) => panic!("VM failed to run: {e:?}"),
    };

    (interp_val, vm_val)
}

fn assert_agree(src: &str, expected: i64) {
    let (interp, vm) = run_dual(src);
    assert_eq!(
        interp, expected,
        "interpreter disagrees with expected value"
    );
    assert_eq!(vm, expected, "bytecode VM disagrees with expected value");
    assert_eq!(vm, interp, "bytecode VM and interpreter disagree");
}

#[test]
fn constant_return() {
    assert_agree("fn main() -> i32 {\n    42\n}", 42);
}

#[test]
fn arithmetic() {
    assert_agree("fn main() -> i32 {\n    3 + 4 * 2 - 1\n}", 10);
}

#[test]
fn if_else_taken_branch() {
    // RT-11: inverted if-guards would take the wrong branch.
    assert_agree("fn main() -> i32 {\n    if 1 < 2 { 10 } else { 99 }\n}", 10);
}

#[test]
fn if_else_not_taken_branch() {
    assert_agree("fn main() -> i32 {\n    if 1 > 2 { 99 } else { 10 }\n}", 10);
}

#[test]
fn while_loop_sum() {
    assert_agree(
        "fn main() -> i32 { let mut i = 0; let mut s = 0; while i < 5 { s = s + i; i = i + 1; } s }",
        10,
    );
}

// --- multi-function differential (the module-assembly seam) ---
//
// The scope note above said it: this file covered self-contained `main`
// bodies only because *no public API assembled a multi-function `Module`
// from MIR bodies*. `BytecodeBackend::generate_module_image` is that
// assembly path: it emits every body, resolves the emitter's fn-table
// index space, and produces a `glyim.module-image/1` artifact the VM's
// decoder loads and runs. The differential now covers cross-function
// `OP_CALL`, cross-frame resume, and (mutual) recursion end to end.

use glyim_bytecode_vm::decode_module;

/// Differential over a whole program: every body goes through both the MIR
/// interpreter (ground truth) and the bytecode VM via the assembled module
/// image; the two observable results must agree.
fn run_dual_multi(src: &str) -> (i64, i64) {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "glyim_vm_diff_multi_{}_{}.g",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, src).expect("write fixture");

    let config = CrateConfig {
        name: "vm_diff_multi".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path, 2).expect("fixture must compile to MIR");
    let main_local = Pipeline::entry_main_local_id(&mut db, &path).expect("resolve main");
    let main_id = DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(main_local as usize),
    );

    // --- interpreter (ground truth, all bodies) ---
    let mut interp = Interpreter::new(mir.ty_ctx.as_ref());
    for b in mir.bodies.values() {
        interp.add_function(b.owner, (**b).clone());
    }
    let entry_body = mir.bodies.get(&main_id).expect("main body").clone();
    interp
        .run_body(&entry_body)
        .expect("interpreter must run clean");
    let interp_val = match interp.get_return_value() {
        Some(InterpValue::Int(n)) => n as i64,
        Some(InterpValue::Uint(n)) => n as i64,
        Some(InterpValue::Bool(b)) => i64::from(b),
        _ => 0,
    };

    // --- bytecode VM (assembled module image, all bodies) ---
    let bodies: Vec<(String, std::sync::Arc<Body>)> = mir
        .bodies
        .iter()
        .map(|(id, body)| {
            let name = if *id == main_id {
                "main".to_string()
            } else {
                // Names only matter for entry resolution; every other body
                // gets a unique-but-arbitrary label.
                format!("fn_{}", id.local_id.to_raw())
            };
            (name, body.clone())
        })
        .collect();
    let backend =
        glyim_codegen::BytecodeBackend::with_ty_ctx(mir.ty_ctx.clone(), TargetInfo::default());
    let image = backend
        .generate_module_image(&bodies)
        .expect("module image assembly must succeed");
    let module = decode_module(&image).expect("assembled image must decode");
    let vm_val = match module.run() {
        Ok(glyim_bytecode_vm::Value::Int(n)) => n,
        Ok(other) => panic!("VM returned an unexpected value: {other:?}"),
        Err(e) => panic!("VM failed to run: {e:?}"),
    };

    (interp_val, vm_val)
}

fn assert_agree_multi(src: &str, expected: i64) {
    let (interp, vm) = run_dual_multi(src);
    assert_eq!(
        interp, expected,
        "interpreter disagrees with expected value"
    );
    assert_eq!(vm, expected, "bytecode VM disagrees with expected value");
    assert_eq!(vm, interp, "bytecode VM and interpreter disagree");
}

#[test]
fn cross_function_call_agrees() {
    assert_agree_multi(
        "fn helper(n: i32) -> i32 {\n    n * 2\n}\n\nfn main() -> i32 {\n    helper(21)\n}",
        42,
    );
}

#[test]
fn two_level_call_chain_agrees() {
    assert_agree_multi(
        "fn b(x: i32) -> i32 {\n    x + 1\n}\n\nfn a(x: i32) -> i32 {\n    b(x) * 2\n}\n\nfn main() -> i32 {\n    a(10)\n}",
        22,
    );
}

#[test]
fn mutual_recursion_agrees() {
    assert_agree_multi(
        "fn is_odd(n: i32) -> bool {\n    if n == 0 { false } else { is_even(n - 1) }\n}\n\nfn is_even(n: i32) -> bool {\n    if n == 0 { true } else { is_odd(n - 1) }\n}\n\nfn main() -> i32 {\n    if is_even(10) { 1 } else { 0 }\n}",
        1,
    );
}

// --- float differential (the float-seam) ---
//
// Floats are the one value class the emitter shipped that the VM could not
// execute: float constants travelled under the integer `OP_LOAD_CONST` (the
// VM ran float arithmetic as *integer* arithmetic over the raw bit patterns)
// and `OP_CAST` was a documented no-op, so `f as i32` silently kept the
// float. The float seam gives the VM `Value::Float`, the typed
// `OP_LOAD_CONST_F64` constant opcode, IEEE-754 float semantics for the
// binary/unary ops (`1.0 / 0.0` is `+inf`, the T106 convention LLVM and the
// interpreter already share), and the interpreter's MIR-14 cast semantics
// over the emitter's kind+target operand bytes. These cases pin the two
// backends to bit-identical float results.

use glyim_bytecode_vm::Value;

/// Differential over a float-returning `main`: both backends must produce
/// the *same bit pattern* (NaN/`-0.0` edges included), and both must equal
/// the expected value's bits.
fn run_dual_float(src: &str) -> (u64, u64) {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "glyim_vm_diff_f64_{}_{}.g",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, src).expect("write fixture");

    let config = CrateConfig {
        name: "vm_diff_f64".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path, 2).expect("fixture must compile to MIR");
    let main_local = Pipeline::entry_main_local_id(&mut db, &path).expect("resolve main");
    let main_id = DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(main_local as usize),
    );
    let main_body = mir.bodies.get(&main_id).expect("main body").clone();

    // --- interpreter (ground truth) ---
    let mut interp = Interpreter::new(mir.ty_ctx.as_ref());
    for b in mir.bodies.values() {
        interp.add_function(b.owner, (**b).clone());
    }
    interp
        .run_body(&main_body)
        .expect("interpreter must run clean");
    let interp_bits = match interp.get_return_value() {
        Some(InterpValue::Float(f)) => f.to_bits(),
        other => panic!("interpreter returned a non-float: {other:?}"),
    };

    // --- bytecode VM ---
    let backend =
        glyim_codegen::BytecodeBackend::with_ty_ctx(mir.ty_ctx.clone(), TargetInfo::default());
    let (bytes, block_offsets) = backend
        .generate_function_with_blocks(&main_body)
        .expect("bytecode generation must succeed");
    let f = glyim_bytecode_vm::Function::with_blocks(
        bytes,
        main_body.locals.len(),
        main_body.arg_count,
        block_offsets,
    );
    let module = glyim_bytecode_vm::Module::new(vec![f], 0);
    let vm_bits = match module.run() {
        Ok(Value::Float(f)) => f.to_bits(),
        Ok(other) => panic!("VM returned a non-float: {other:?}"),
        Err(e) => panic!("VM failed to run: {e:?}"),
    };

    (interp_bits, vm_bits)
}

fn assert_agree_float(src: &str, expected: f64) {
    let (interp, vm) = run_dual_float(src);
    assert_eq!(
        interp, vm,
        "bytecode VM and interpreter disagree on float bits"
    );
    assert_eq!(
        interp,
        expected.to_bits(),
        "interpreter disagrees with expected float bits"
    );
}

#[test]
fn float_arithmetic_agrees() {
    assert_agree_float("fn main() -> f64 {\n    3.5 + 1.25 * 2.0\n}", 6.0);
}

#[test]
fn float_div_by_zero_is_inf_agrees() {
    // T106: IEEE `+inf` in Rust/LLVM; the interpreter's native `f64` division
    // and the VM's must agree bit-for-bit.
    assert_agree_float("fn main() -> f64 {\n    1.0 / 0.0\n}", f64::INFINITY);
}

#[test]
fn float_negation_agrees() {
    assert_agree_float("fn main() -> f64 {\n    -(2.5)\n}", -2.5);
}

#[test]
fn float_comparison_feeds_a_branch_agrees() {
    // The comparison result crosses back into the VM's Int bool encoding
    // (JumpIf is jump-if-nonzero), so this also pins the float→bool shape.
    assert_agree(
        "fn main() -> i32 {\n    if 2.5 < 3.5 { 1 } else { 0 }\n}",
        1,
    );
    assert_agree(
        "fn main() -> i32 {\n    if 2.5 > 3.5 { 1 } else { 0 }\n}",
        0,
    );
}

#[test]
fn float_cast_then_int_op_agrees() {
    // Mirrors the run-pass corpus fixture `float_cast_then_int_op.g`: the
    // MIR-14 cast semantics must survive the bytecode backend (the no-op
    // `OP_CAST` made `f as i32` keep the float).
    assert_agree(
        "fn main() -> i32 {\n    let f = 3.99;\n    (f as i32) + 20\n}",
        23,
    );
}

#[test]
fn int_truncation_cast_agrees() {
    // MIR-14: `(-1i32) as u8` must be 255 through the VM's width-aware
    // truncation, matching the interpreter.
    assert_agree(
        "fn main() -> i32 {\n    let x: i32 = -1;\n    (x as u8) as i32\n}",
        255,
    );
}

/// Differential over a float-returning multi-function program: the helper's
/// float result must survive the assembled module image bit-for-bit.
#[test]
fn multi_function_float_call_agrees() {
    let src = "fn twice(x: f64) -> f64 {\n    x * 2.0\n}\n\nfn main() -> f64 {\n    twice(1.25)\n}";
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "glyim_vm_diff_multi_f64_{}_{}.g",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&path, src).expect("write fixture");

    let config = CrateConfig {
        name: "vm_diff_multi_f64".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    let mut db = Database::new(config);

    let mir = compile_file_to_mir(&mut db, &path, 2).expect("fixture must compile to MIR");
    let main_local = Pipeline::entry_main_local_id(&mut db, &path).expect("resolve main");
    let main_id = DefId::new(
        CrateId::from_usize(0),
        LocalDefId::from_usize(main_local as usize),
    );

    let mut interp = Interpreter::new(mir.ty_ctx.as_ref());
    for b in mir.bodies.values() {
        interp.add_function(b.owner, (**b).clone());
    }
    let entry_body = mir.bodies.get(&main_id).expect("main body").clone();
    interp
        .run_body(&entry_body)
        .expect("interpreter must run clean");
    let interp_bits = match interp.get_return_value() {
        Some(InterpValue::Float(f)) => f.to_bits(),
        other => panic!("interpreter returned a non-float: {other:?}"),
    };

    let bodies: Vec<(String, std::sync::Arc<Body>)> = mir
        .bodies
        .iter()
        .map(|(id, body)| {
            let name = if *id == main_id {
                "main".to_string()
            } else {
                format!("fn_{}", id.local_id.to_raw())
            };
            (name, body.clone())
        })
        .collect();
    let backend =
        glyim_codegen::BytecodeBackend::with_ty_ctx(mir.ty_ctx.clone(), TargetInfo::default());
    let image = backend
        .generate_module_image(&bodies)
        .expect("module image assembly must succeed");
    let module = decode_module(&image).expect("assembled image must decode");
    let vm_bits = match module.run() {
        Ok(Value::Float(f)) => f.to_bits(),
        Ok(other) => panic!("VM returned a non-float: {other:?}"),
        Err(e) => panic!("VM failed to run: {e:?}"),
    };

    assert_eq!(vm_bits, interp_bits, "VM and interpreter disagree");
    assert_eq!(vm_bits, 2.5f64.to_bits(), "twice(1.25) must be 2.5");
}

// --- string differential (the string-pool seam) ---
//
// String constants used to be opaque interner *indices* under the integer
// `OP_LOAD_CONST`: equality-by-index only worked while the pool lived in the
// same process, and the payload was unrecoverable from the artifact. The
// string-pool seam gives string constants their own opcode
// (`OP_LOAD_CONST_STR`, a pool reference), ships the pool inside the module
// image (`Module::strings`), and makes `OP_EQ`/`OP_NE` over two strings
// compare *content*. Both cases here must go through the assembled-image
// path — the pool is an image feature, so the single-function harness (no
// pool) cannot carry them.

#[test]
fn string_equality_agrees_through_the_image() {
    // Unequal contents -> 0: the decisive case, because index-based equality
    // would agree here only by interning accident.
    assert_agree_multi(
        "fn main() -> i32 {\n    let a = \"abc\";\n    let b = \"abd\";\n    if a == b { 1 } else { 0 }\n}",
        0,
    );
    // Equal contents over distinct pool slots -> 1.
    assert_agree_multi(
        "fn main() -> i32 {\n    let a = \"same\";\n    let b = \"same\";\n    if a == b { 1 } else { 0 }\n}",
        1,
    );
    // The str_equality fixture's mix: == and != across two literals.
    assert_agree_multi(
        "fn main() -> i32 {\n    let a: i32 = if \"abc\" == \"abc\" { 11 } else { 0 };\n    let b: i32 = if \"abc\" != \"xyz\" { 17 } else { 0 };\n    a + b\n}",
        28,
    );
}
