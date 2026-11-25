//! CT-6 validated-config tests (plan §6 M3.8 / §5.2 CT-6).
//!
//! The plan line these tests pin:
//!
//! > **CT-6** Configuration redesign (`Backend`, `EmitKind`, validated
//! > `ValidConfig`) — DoD: illegal combos fail to compile; CLI snapshot
//! > tests.
//!
//! Today the driver checks `--backend`/`--emit`/`--lto` compatibility with
//! *scattered runtime string compares* (`args.backend == "bytecode"` in
//! five unrelated places); the seam makes the combination space a type:
//! `ValidConfig::build` is the single function that parses the flag
//! strings and returns either a [`ValidConfig`] (with the derived facts —
//! run-mode flag, cacheability) or a [`ConfigError`] naming the conflicting
//! flags. The end-to-end `run_with_args` behaviour is unchanged (the
//! existing `cli_tests` pin the message texts).

use crate::{Backend, ConfigError, EmitKind, ValidConfig};
use std::path::PathBuf;

fn build(
    backend: &str,
    emit: &str,
    lto: &str,
    output: Option<PathBuf>,
) -> Result<ValidConfig, ConfigError> {
    ValidConfig::build_parts(backend, emit, lto, output.as_deref())
}

#[test]
fn legal_llvm_obj_off_is_cacheable() {
    let cfg = build("llvm", "obj", "off", None).expect("the default shape is legal");
    assert!(matches!(cfg.backend, Backend::Llvm));
    assert!(matches!(cfg.emit, EmitKind::Obj));
    assert!(cfg.cacheable, "obj + llvm + off-thin is the cacheable rung");
}

#[test]
fn interp_run_is_legal_and_not_cacheable() {
    let cfg = build("interp", "run", "off", None).expect("interp + run is legal");
    assert!(matches!(cfg.backend, Backend::Interp));
    assert!(matches!(cfg.emit, EmitKind::Run));
    assert!(!cfg.cacheable, "run mode writes no object");
}

#[test]
fn default_backend_with_run_implies_interp() {
    // The default spelling of run mode executes through the interpreter.
    let cfg = build("llvm", "run", "off", None).expect("run mode is legal on any backend spelling");
    assert!(matches!(cfg.emit, EmitKind::Run));
    assert!(!cfg.cacheable);
}

#[test]
fn bytecode_with_run_is_a_named_error() {
    let err = build("bytecode", "run", "off", None).unwrap_err();
    assert!(
        matches!(err, ConfigError::BytecodeCannotRun),
        "bytecode + run must be the named combo error, got: {err}"
    );
    let text = err.to_string();
    assert!(
        text.contains("--emit=run") && text.contains("--backend"),
        "the refusal must name the conflicting flags: {text}"
    );
}

#[test]
fn interp_with_codegen_emit_is_a_named_error() {
    let err = build("interp", "obj", "off", None).unwrap_err();
    assert!(
        matches!(err, ConfigError::InterpEmitsNoCode),
        "interp + obj must be the named combo error, got: {err}"
    );
}

#[test]
fn bytecode_with_exec_or_cdylib_is_a_named_error() {
    for emit in ["exec", "cdylib"] {
        let err = build("bytecode", emit, "off", None).unwrap_err();
        assert!(
            matches!(err, ConfigError::BytecodeCannotLink),
            "bytecode + {emit} must be the named combo error, got: {err}"
        );
    }
}

#[test]
fn thin_lto_requires_the_llvm_backend() {
    let err = build("bytecode", "obj", "thin", None).unwrap_err();
    assert!(
        matches!(err, ConfigError::ThinLtoRequiresLlvm),
        "bytecode + thin must be the named combo error, got: {err}"
    );
    // Thin on the LLVM backend is the supported spelling.
    let cfg = build("llvm", "obj", "thin", None).expect("llvm + thin is legal");
    assert!(
        !cfg.cacheable,
        "thin bitcode is not the plain-object cache rung"
    );
}

#[test]
fn unknown_flag_values_are_parse_errors() {
    let err = build("gpu", "obj", "off", None).unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidBackend(_)),
        "unknown backend must be a parse error, got: {err}"
    );
    let err = build("llvm", "obj", "turbo", None).unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLto(_)),
        "unknown lto must be a parse error, got: {err}"
    );
    let err = build("llvm", "wasm", "off", None).unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidEmit(_)),
        "unknown emit must be a parse error, got: {err}"
    );
}

#[test]
fn thin_or_fat_lto_disables_the_object_cache() {
    for lto in ["thin", "fat"] {
        let cfg = build("llvm", "obj", lto, None).expect("llvm codegen is legal");
        assert!(
            !cfg.cacheable,
            "lto={lto} objects must bypass the cache rung"
        );
    }
}
