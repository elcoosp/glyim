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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    };
    let result = run_with_args(args);
    assert!(
        result.is_ok(),
        "Expected compilation to succeed, got: {result:?}"
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    };
    let result = run_with_args(args);
    assert!(result.is_ok(), "asm emit should succeed, got: {result:?}");

    assert!(asm_path.exists(), "assembly output file should exist");
    let asm = std::fs::read_to_string(&asm_path).unwrap();
    // A trivial `fn main() {}` must lower to at least a `ret` instruction on
    // every supported host backend.
    assert!(
        asm.contains("ret"),
        "assembly output should contain a `ret` instruction; got:\n{asm}"
    );
}

/// T126-PATCHED: `--lto fat` is a tracked gap (`KNOWN_GAPS` Phase 10.2).
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
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

/// `--lto thin` used to be a tracked gap; it now runs `llvm-lto2` (with
/// the `-r=` symbol-resolution directives it requires). When `llvm-lto2`
/// is unavailable the compile must fail with an actionable diagnostic
/// naming the missing tool (degraded-path contract).
#[test]
fn test_lto_thin_is_no_longer_a_tracked_gap() {
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    };
    let result = run_with_args(args);
    // Two acceptable outcomes: success when llvm-lto2 is present, or a
    // diagnostic naming the missing tool. What is NOT acceptable is
    // succeeding silently as a no-op or emitting an unrelated error.
    if let Err(diags) = result {
        let joined = diags
            .iter()
            .map(|d| format!("{d:?}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            joined.contains("llvm-lto2"),
            "LTO=thin failure must name llvm-lto2; got:\n{joined}"
        );
    }
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    };
    let result = run_with_args(args);
    assert!(
        result.is_ok(),
        "cdylib emit should succeed, got: {result:?}"
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
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

// ---------------------------------------------------------------------------
// M2.1: phase instrumentation flags (`--time-passes`, `--self-profile`,
// `--emit=stats`). The parse-level tests pin the clap wiring — especially
// that the optional `--self-profile` value can never swallow the
// positional INPUT — and the `--emit=stats` run-level test pins the
// on-disk report contract.
// ---------------------------------------------------------------------------

/// `--emit=stats` is a recognised emit kind.
#[test]
fn test_emit_stats_flag_parses() {
    let args = CliArgs::try_parse_from(["glyim", "--emit", "stats", "input.g"]).unwrap();
    assert_eq!(args.emit, "stats");
}

/// `--time-passes` is a plain boolean flag: it must never consume the
/// positional INPUT that follows it.
#[test]
fn test_time_passes_flag_does_not_swallow_input() {
    let args = CliArgs::try_parse_from(["glyim", "--time-passes", "input.g"]).unwrap();
    assert!(args.time_passes);
    assert_eq!(args.input, std::path::PathBuf::from("input.g"));
}

/// A bare `--self-profile` defaults to `./trace.json`, and the positional
/// INPUT stays the positional INPUT (the value is `=`-bound).
#[test]
fn test_self_profile_default_value() {
    let args = CliArgs::try_parse_from(["glyim", "--self-profile", "input.g"]).unwrap();
    assert_eq!(
        args.self_profile,
        Some(std::path::PathBuf::from("trace.json"))
    );
    assert_eq!(args.input, std::path::PathBuf::from("input.g"));
}

/// `--self-profile=FILE` binds the explicit path.
#[test]
fn test_self_profile_explicit_value() {
    let args = CliArgs::try_parse_from(["glyim", "--self-profile=out.json", "input.g"]).unwrap();
    assert_eq!(
        args.self_profile,
        Some(std::path::PathBuf::from("out.json"))
    );
}

/// Run-level contract: `--emit=stats` writes a schema-versioned JSON
/// report with the phase list and counters, and no object file is needed.
#[test]
fn test_emit_stats_writes_report() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let src = tmp.into_temp_path();

    let out_dir = tempfile::tempdir().unwrap();
    let stats_path = out_dir.path().join("out.stats.json");

    let args = CliArgs {
        input: src.to_path_buf(),
        output: Some(stats_path.clone()),
        opt_level: 0,
        target: None,
        backend: "llvm".to_string(),
        emit: "stats".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    };
    let result = run_with_args(args);
    assert!(result.is_ok(), "--emit=stats failed: {result:?}");
    assert!(stats_path.exists(), "stats report not written");

    let text = std::fs::read_to_string(&stats_path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).expect("report is valid JSON");
    assert_eq!(value["version"], 1, "schema version pinned at 1");
    let phases = value["phases"].as_array().expect("phases array");
    assert!(!phases.is_empty(), "phase rows present");
    assert!(value["total_micros"].is_u64(), "total_micros present");
    assert!(value["mono_bodies"].is_u64(), "mono_bodies present");
    assert!(value["warnings"].is_u64(), "warnings present");
}

/// Run-level contract: `--time-passes` + `--self-profile` on a normal
/// object compile — the trace file is written and the compile succeeds.
#[test]
fn test_self_profile_writes_trace_file() {
    // The compile cache is content-addressed and persists across runs
    // ($HOME/.glyip/cache/glyim); a cache hit skips the pipeline and
    // records no profile. Bust it with a process-unique comment so this
    // test always exercises a real compile.
    let token = std::process::id();
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(
        tmp,
        "// cache-buster: self-profile-trace-{token}\nfn main() {{}}"
    )
    .unwrap();
    let src = tmp.into_temp_path();

    let out_dir = tempfile::tempdir().unwrap();
    let trace_path = out_dir.path().join("trace.json");
    let obj_path = out_dir.path().join("out.o");

    let args = CliArgs {
        input: src.to_path_buf(),
        output: Some(obj_path),
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
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: true,
        self_profile: Some(trace_path.clone()),
    };
    let result = run_with_args(args);
    assert!(result.is_ok(), "profiled compile failed: {result:?}");
    let text = std::fs::read_to_string(&trace_path).expect("trace file written");
    let value: serde_json::Value = serde_json::from_str(&text).expect("trace is valid JSON");
    let events = value["traceEvents"].as_array().expect("traceEvents array");
    assert!(
        events
            .iter()
            .any(|e| e["ph"] == "X" && e["name"] == "codegen"),
        "object compiles record the codegen phase"
    );
}

// ---- `--emit=run` (M2.4: in-process interpreter execution) ----

/// Helper: build `CliArgs` for `--emit=run` over a temp source file.
fn run_args(src_path: &std::path::Path, backend: &str, with_stdlib: bool) -> CliArgs {
    CliArgs {
        input: src_path.to_path_buf(),
        output: None,
        opt_level: 0,
        target: None,
        backend: backend.to_string(),
        emit: "run".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib,
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    }
}

/// M2.4: `--emit=run` executes the program in-process and surfaces the
/// program's (masked) return value as the process exit code — the same
/// contract a linked `--emit=exec` binary has for the OS.
#[test]
fn test_emit_run_returns_program_exit_code() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() -> i32 {{\n    3\n}}").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "llvm", false));
    assert!(result.is_ok(), "run mode must succeed: {result:?}");
    assert_eq!(result.unwrap(), 3, "program exit code must propagate");
}

/// M2.4: exit-code masking matches a real process (`300 & 0xff == 44`),
/// and `--with-stdlib` injection composes with run mode.
#[test]
fn test_emit_run_masks_exit_code_like_a_native_binary() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() -> i32 {{\n    300\n}}").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "llvm", true));
    assert!(result.is_ok(), "run mode must succeed: {result:?}");
    assert_eq!(result.unwrap(), 44, "exit code must be masked to 0xff");
}

/// M2.4: a unit-returning `main` exits 0 (the println smoke path — the
/// interpreter's FFI stdout buffer flows to the process stdout).
#[test]
fn test_emit_run_unit_main_with_stdlib_exits_zero() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(
        tmp,
        "fn main() {{\n    println!(\"hello from run mode\");\n}}"
    )
    .unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "llvm", true));
    assert!(result.is_ok(), "run mode must succeed: {result:?}");
    assert_eq!(result.unwrap(), 0, "a unit-returning main exits 0");
}

/// M2.4: a program that panics (division by zero — the run-fail corpus
/// shape) exits 101, matching `glyim_panic` and a native `--emit=exec`
/// binary; stderr carries the canonical panic line.
#[test]
fn test_emit_run_panicking_program_exits_101() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() -> i32 {{\n    let z = 0;\n    5 / z\n}}").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "llvm", false));
    assert!(
        result.is_ok(),
        "a panicking program is a successful driver run: {result:?}"
    );
    assert_eq!(result.unwrap(), 101, "panics exit 101 like the native path");
}

/// M2.4: a compile error in run mode is a driver failure (diagnostics
/// path), not a program exit code.
#[test]
fn test_emit_run_compile_error_is_driver_failure() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() -> i32 {{").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "llvm", false));
    assert!(result.is_err(), "compile errors must fail the driver");
}

/// M2.4: `--backend=bytecode` with `--emit=run` is an explicit error —
/// the bytecode backend is a codegen backend; run mode executes through
/// the interpreter. Silent ignore would violate the driver's honesty rule.
#[test]
fn test_emit_run_rejects_bytecode_backend() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "bytecode", false));
    assert!(result.is_err(), "bytecode + run must be rejected");
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("--emit=run") || err_text.contains("--backend"),
        "error must name the conflicting flags; got: {err_text}"
    );
}

/// M2.4: `--backend=interp` is only valid together with `--emit=run`;
/// it does not emit code, so every other emit kind is an explicit error.
#[test]
fn test_backend_interp_rejects_codegen_emit_kinds() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();
    let mut args = run_args(&path, "interp", false);
    args.emit = "obj".to_string();
    let result = run_with_args(args);
    assert!(result.is_err(), "interp backend cannot emit code");
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("--emit=run"),
        "error must point at the run emit kind; got: {err_text}"
    );
}

/// M2.4: `--backend=interp --emit=run` is the explicit spelling of the
/// interpreter run path and behaves like the default spelling.
#[test]
fn test_backend_interp_with_run_mode_executes() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() -> i32 {{\n    7\n}}").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "interp", false));
    assert!(result.is_ok(), "interp run mode must succeed: {result:?}");
    assert_eq!(result.unwrap(), 7);
}

/// M2.4: `--output` writes no artifact in run mode (execution happens
/// in-process), so the flag is rejected instead of silently ignored.
#[test]
fn test_emit_run_rejects_output_flag() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn main() {{}}").unwrap();
    let path = tmp.into_temp_path();
    let mut args = run_args(&path, "llvm", false);
    args.output = Some(std::path::PathBuf::from("unused.o"));
    let result = run_with_args(args);
    assert!(result.is_err(), "--output with --emit=run must be rejected");
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("--output"),
        "error must name the inapplicable flag; got: {err_text}"
    );
}

/// M2.4: a program without `main` is a clean, actionable error in run
/// mode (unlike the fixture fallback the test harness keeps).
#[test]
fn test_emit_run_missing_main_is_actionable_error() {
    let mut tmp = NamedTempFile::new().unwrap();
    writeln!(tmp, "fn helper() -> i32 {{ 1 }}").unwrap();
    let path = tmp.into_temp_path();
    let result = run_with_args(run_args(&path, "llvm", false));
    assert!(result.is_err(), "no-main program must be a driver error");
    let err_text = format!("{:?}", result.unwrap_err());
    assert!(
        err_text.contains("main"),
        "error must say main was not found; got: {err_text}"
    );
}
