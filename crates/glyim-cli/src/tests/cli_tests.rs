use crate::{CliArgs, run_with_args};
use clap::Parser;
use std::io::Write;
use tempfile::NamedTempFile;

/// S20-T01: Compile valid file → exit 0
#[test]
fn test_compile_valid_file() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();
    let args = CliArgs {
        input: path.to_path_buf(),
        output: None,
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "obj".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(
        result.is_ok(),
        "Expected compilation to succeed, got: {:?}",
        result
    );
}

/// S20-T02: Compile invalid file → exit 1
#[test]
fn test_compile_invalid_file() {
    let mut tmp = NamedTempFile::new().unwrap();
    write!(tmp, "fn main() {{").unwrap();
    let path = tmp.into_temp_path();
    let args = CliArgs {
        input: path.to_path_buf(),
        output: None,
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "obj".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(result.is_err(), "Expected compilation to fail");
}

/// S20-T03: --help
#[test]
fn test_help_flag() {
    let result = CliArgs::try_parse_from(["glyim", "--help"]);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("Usage:") || msg.contains("glyim"));
}

/// S20-T04: Missing input → error
#[test]
fn test_missing_input() {
    let result = CliArgs::try_parse_from(["glyim"]);
    assert!(
        result.is_err(),
        "Expected error for missing required argument"
    );
}

/// S20-T05: --backend bytecode
#[test]
fn test_backend_bytecode_flag() {
    let args = CliArgs::try_parse_from(["glyim", "--backend", "bytecode", "input.g"]).unwrap();
    assert_eq!(args.backend, "bytecode");
}

/// S20-T06: --emit mir flag
#[test]
fn test_emit_mir_flag() {
    let args = CliArgs::try_parse_from(["glyim", "--emit", "mir", "input.g"]).unwrap();
    assert_eq!(args.emit, "mir");
}

/// S20-T07: --emit llvm-ir flag
#[test]
fn test_emit_llvm_ir_flag() {
    let args = CliArgs::try_parse_from(["glyim", "--emit", "llvm-ir", "input.g"]).unwrap();
    assert_eq!(args.emit, "llvm-ir");
}

/// §18.3: --emit=asm produces a `.s` assembly file containing recognizable
/// host assembly (a function epilogue `ret` instruction at minimum).
#[test]
fn test_emit_asm_produces_assembly_file() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let src = tmp.into_temp_path();

    let out_dir = tempfile::tempdir().unwrap();
    let asm_path = out_dir.path().join("out.s");

    let args = CliArgs {
        input: src.to_path_buf(),
        output: Some(asm_path.clone()),
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "asm".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(result.is_ok(), "asm emit should succeed, got: {:?}", result);

    assert!(asm_path.exists(), "assembly output file should exist");
    let asm = std::fs::read_to_string(&asm_path).unwrap();
    // A trivial `fn main() {}` must lower to at least a `ret` instruction on
    // every supported host backend.
    assert!(
        asm.contains("ret"),
        "assembly output should contain a `ret` instruction; got:\n{}",
        asm
    );
}

/// T126-PATCHED: `--lto fat` is a tracked gap (KNOWN_GAPS Phase 10.2).
/// The driver must emit a clear error rather than silently downgrading
/// to thin/off. This test pins the error contract; the neighbouring
/// `test_lto_thin_surfaces_tracked_gap` covers the same for `thin`.
#[test]
fn test_lto_fat_compiles_to_object() {
    // T126-PATCHED: `--lto fat` is intentionally a tracked gap (see
    // KNOWN_GAPS Phase 10.2). The driver emits a clear, actionable error
    // rather than silently downgrading to thin/off. This test previously
    // asserted the old silent-success behavior and went stale when T126
    // landed; it now pins the *error* contract instead.
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();
    let out_dir = tempfile::tempdir().unwrap();
    let obj_path = out_dir.path().join("out.o");

    let args = CliArgs {
        input: path.to_path_buf(),
        output: Some(obj_path.clone()),
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "obj".to_string(),
        linker: None,
        link_flags: None,
        lto: "fat".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(
        result.is_err(),
        "`--lto fat` must surface the tracked-gap error, got success"
    );
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("--lto fat") || err_text.contains("lto fat"),
        "error should name the unsupported flag; got: {err_text}"
    );
    assert!(
        err_text.contains("not yet wired") || err_text.contains("KNOWN_GAPS"),
        "error should point at the tracked gap; got: {err_text}"
    );
    assert!(
        !obj_path.exists(),
        "no object output should be produced when --lto fat errors"
    );
}

/// Phase 10.2: `--lto thin` is a tracked gap (linker-driver integration) and
/// must surface an explicit error rather than silently degrading to a no-op.
#[test]
fn test_lto_thin_surfaces_tracked_gap() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();

    let args = CliArgs {
        input: path.to_path_buf(),
        output: None,
        opt_level: 2,
        target: None,
        backend: "llvm".to_string(),
        emit: "obj".to_string(),
        linker: None,
        link_flags: None,
        lto: "thin".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(
        result.is_err(),
        "LTO=thin must surface its tracked-gap error, not silently no-op"
    );
}

/// Phase 9.2: `--emit=cdylib` compiles the program to an object then links it
/// into a position-independent shared library (`-shared`). This is the host
/// artifact a proc-macro crate compiles to so `load_cdylib` can dlopen it
/// during macro expansion. We assert the shared object is produced.
///
/// Gated to Linux: glyim's default target triple is `x86_64-unknown-linux-gnu`
/// (it emits ELF objects), and the host `cc`/`ld` on Linux links ELF cleanly.
/// On macOS the host linker expects Mach-O, so it cannot link the default ELF
/// object — that host/target mismatch is a separate cross-compilation concern,
/// not a cdylib-emit defect.
#[cfg(target_os = "linux")]
#[test]
fn test_emit_cdylib_produces_shared_library() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let src = tmp.into_temp_path();

    let out_dir = tempfile::tempdir().unwrap();
    let cdylib_path = out_dir.path().join("out.so");

    let args = CliArgs {
        input: src.to_path_buf(),
        output: Some(cdylib_path.clone()),
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "cdylib".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(
        result.is_ok(),
        "cdylib emit should succeed, got: {:?}",
        result
    );
    assert!(cdylib_path.exists(), "cdylib output should exist");
    let meta = std::fs::metadata(&cdylib_path).unwrap();
    assert!(meta.len() > 0, "cdylib output should be non-empty");
}
#[test]
fn test_lto_invalid_value_rejected() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();

    let args = CliArgs {
        input: path.to_path_buf(),
        output: None,
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "obj".to_string(),
        linker: None,
        link_flags: None,
        lto: "bogus".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(result.is_err(), "invalid --lto value must be rejected");
}

/// Phase: native executable (`--emit=exec`). On a darwin host the produced
/// binary must link and *run* (exit 0), which proves the codegen emitted a
/// C-ABI `main` entry symbol that forwards into the glyim `main` body. We
/// target `x86_64-apple-darwin` so the test is reproducible on the macOS host
/// (the default ELF triple needs a Linux linker, covered by the Linux CI
/// matrix).
///
/// Note: a richer body such as `let _x = 2 + 3;` cannot be used here because
/// `i32` integer-literal arithmetic type-checking is a *separate, pre-existing*
/// gap (it lowers to `TyKind::Error` and fails codegen) — that is unrelated to
/// native-exec output and is tracked outside this phase.
#[cfg(target_os = "macos")]
#[test]
fn exec_emit_links_and_runs() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("run.g");
    std::fs::write(&src, "fn main() {}\n").unwrap();
    let out = tmp.path().join("run_bin");

    let args = CliArgs {
        input: src.clone(),
        output: Some(out.clone()),
        opt_level: 0,
        target: Some("x86_64-apple-darwin".to_string()),
        backend: "llvm".to_string(),
        emit: "exec".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
    };
    let result = run_with_args(args);
    assert!(result.is_ok(), "glyim-cli --emit=exec failed: {:?}", result);
    assert!(out.exists(), "executable not produced");

    // The binary must actually run to completion with a zero exit code.
    let status = std::process::Command::new(&out)
        .status()
        .expect("execute produced binary");
    assert!(status.success(), "produced binary exited non-zero");
}
