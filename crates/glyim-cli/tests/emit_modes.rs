//! Regression tests for **every** `--emit` mode.
//!
//! Background: the pipeline used to have *four* independent copies of the
//! `parse → expand → defmap → hir → typeck → lower` chain. `--emit=obj` and
//! `--emit=exec` went through the one that ran monomorphization; `--emit=llvm-ir`
//! and `--emit=asm` used partial copies that stopped before it, so generic
//! stdlib definitions reached codegen with `TyKind::Param` still in their
//! locals and `fn_abi_of` ICEd with `UnknownType`. The mode a user picked
//! determined whether the program compiled at all.
//!
//! These tests pin the contract that **every** emit mode succeeds on the same
//! trivial program. If a future change breaks one mode while leaving the
//! others working, this file fails — which is exactly what was missing when
//! `--emit=llvm-ir` / `--emit=asm` silently rotted.
//!
//! They drive the real `glyim-cli` binary and use `--with-stdlib` so
//! `println` resolves. `--emit=exec` additionally runs the produced binary
//! and asserts it prints `hello`, covering the full fat-pointer lowering chain
//! (`&str`/`&[u8]` ABI, slice `len`, `as_bytes`).

use std::io::Write;
use std::process::Command;

fn tempdir() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "glyim_emit_modes_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cli_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_glyim-cli"))
}

fn host_triple() -> String {
    let out = Command::new("rustc").args(["-vV"]).output().unwrap();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(rest) = line.strip_prefix("host: ") {
            return rest.trim().to_string();
        }
    }
    "unknown".to_string()
}

/// Write `fn main() { println("hello"); }` to `dir` and return its path.
fn hello_source(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("hello.g");
    let mut f = std::fs::File::create(&src).unwrap();
    writeln!(f, "fn main() {{ println(\"hello\"); }}").unwrap();
    drop(f);
    src
}

/// Run `glyim-cli --with-stdlib --emit=<mode> -o <out> <src>` and return the
/// output (for diagnostics on failure).
fn run_emit(src: &std::path::Path, mode: &str, out: &std::path::Path) -> std::process::Output {
    Command::new(cli_bin())
        .args([
            src.to_str().unwrap(),
            "--with-stdlib",
            &format!("--emit={mode}"),
            "--target",
            &host_triple(),
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("spawn glyim-cli")
}

/// Every emit mode that produces an artifact must succeed on the same program.
///
/// This is the core regression: if one mode's pipeline diverges from the
/// others (e.g. skips a pass), it fails here while the rest pass.
#[test]
fn all_emit_modes_succeed_on_hello_world() {
    let dir = tempdir();
    let src = hello_source(&dir);

    for mode in ["obj", "mir", "llvm-ir", "asm"] {
        let out = dir.join(format!("hello.{mode}"));
        let output = run_emit(&src, mode, &out);
        assert!(
            output.status.success(),
            "--emit={mode} must succeed on hello world; exit={:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            out.exists(),
            "--emit={mode} must write {out:?}",
        );
        assert!(
            std::fs::metadata(&out).unwrap().len() > 0,
            "--emit={mode} produced an empty file at {out:?}",
        );
    }

    // `--emit=exec` needs a native linker; on a host without one the mode
    // still must *compile* (the link failure is a distinct, expected error).
    // We assert only that it gets far enough to produce an object, by way of
    // `--emit=obj` above. A full exec end-to-end check lives in a separate
    // test below so a linker-less CI host can still run this file.
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--emit=llvm-ir` output must be real LLVM IR, not a stub.
///
/// Guards against a mode "succeeding" by writing an empty or error document.
#[test]
fn llvm_ir_output_looks_like_llvm_ir() {
    let dir = tempdir();
    let src = hello_source(&dir);
    let out = dir.join("hello.ll");
    let output = run_emit(&src, "llvm-ir", &out);
    assert!(output.status.success(), "--emit=llvm-ir must succeed");

    let ir = std::fs::read_to_string(&out).unwrap();
    assert!(
        ir.contains("target triple"),
        "expected an LLVM module header; got:\n{}",
        &ir[..ir.len().min(400)],
    );
    // The hello string constant must appear — proof the body was lowered.
    assert!(
        ir.contains("hello"),
        "expected the `hello` string constant in the IR",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--emit=asm` output must be real assembly.
#[test]
fn asm_output_looks_like_assembly() {
    let dir = tempdir();
    let src = hello_source(&dir);
    let out = dir.join("hello.s");
    let output = run_emit(&src, "asm", &out);
    assert!(output.status.success(), "--emit=asm must succeed");

    let asm = std::fs::read_to_string(&out).unwrap();
    assert!(
        !asm.trim().is_empty(),
        "--emit=asm wrote an empty assembly file",
    );
    // Any target's assembler text has a directive or an instruction mnemonic.
    let has_directive = asm.lines().any(|l| l.trim_start().starts_with('.'));
    assert!(
        has_directive,
        "expected assembly directives (lines starting with `.`); got:\n{}",
        &asm[..asm.len().min(400)],
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// End-to-end: `--emit=exec` on hello world produces a binary that prints
/// exactly `hello\n`.
///
/// This is the full-fat check — it exercises `&str`/`&[u8]` fat-pointer ABI,
/// the slice `len` intrinsic, `as_bytes`, and the runtime `glyim_stdout_write`
/// hook. Skipped only if the host has no C linker (the mode then reports a
/// link error, which is not what this test is about).
#[test]
fn exec_binary_prints_hello() {
    let dir = tempdir();
    let src = hello_source(&dir);
    let out = dir.join("hello");
    let output = run_emit(&src, "exec", &out);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Skip ONLY when the host genuinely has no working C linker — i.e.
        // the *linker driver itself* could not be spawned. A link that runs
        // and fails (`Undefined symbols`, a missing runtime) is a REAL
        // compiler bug and must fail this test, not be silently skipped: an
        // earlier version of this test skipped on `Undefined symbols` too,
        // which masked a stale-cache bug that shipped a pre-fix object.
        let no_linker = stderr.contains("Failed to invoke linker")
            || stderr.contains("No such file or directory")
            || stderr.contains("linker not found");
        if no_linker {
            eprintln!("skipping exec test: no working native linker on host:\n{stderr}");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        panic!(
            "--emit=exec failed (NOT a missing-linker skip); exit={:?}\nstderr: {stderr}",
            output.status.code(),
        );
    }

    let run = Command::new(&out).output().expect("run produced binary");
    assert!(
        run.status.success(),
        "the produced binary must exit 0; got {:?}\nstderr: {}",
        run.status.code(),
        String::from_utf8_lossy(&run.stderr),
    );
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout, "hello\n",
        "the produced binary must print `hello`; got {stdout:?}",
    );
    let _ = std::fs::remove_dir_all(&dir);
}
