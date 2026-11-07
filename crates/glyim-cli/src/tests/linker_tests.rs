use crate::linker::{
    DriverKind, invoke_linker, linker_flags_for_driver, linker_flags_for_raw_ld,
    linker_flags_for_target,
};
use std::path::PathBuf;

#[test]
fn test_invoke_linker_basic() {
    let obj = PathBuf::from("dummy.o");
    let out = PathBuf::from("dummy_out");
    let result = invoke_linker(&obj, &out, None, None, None);
    let _ = result;
}

// Plan §18.1: cross-compile target → linker-flag table.
//
// The flag computation is split into a pure, driver-kind-independent core
// (`linker_flags_for_driver` / `linker_flags_for_raw_ld`) plus a runtime
// probe (`driver_is_clang`) — because `cc` is clang on macOS but gcc on most
// Linux distros, and gcc REJECTS `-target` ("unrecognized command-line
// option"). Unit tests cover the pure core deterministically for both driver
// flavors; the probing path is covered by a host-behavior smoke test.

#[test]
fn test_linker_flags_clang_driver_emits_target() {
    // Any clang-style driver takes a single-dash `-target <triple>` — that is
    // what lets a darwin host link a darwin executable via its clang `cc`.
    let flags = linker_flags_for_driver(DriverKind::Clang, "aarch64-unknown-linux-gnu").unwrap();
    assert_eq!(flags, vec!["-target", "aarch64-unknown-linux-gnu"]);

    let flags = linker_flags_for_driver(DriverKind::Clang, "x86_64-apple-darwin").unwrap();
    assert_eq!(flags, vec!["-target", "x86_64-apple-darwin"]);
}

#[test]
fn test_linker_flags_gcc_driver_host_triple_needs_no_flags() {
    // GNU gcc has no `-target`. A triple that matches the compiling host is a
    // plain host link: no extra flags (this is the case the old code got
    // wrong — it always emitted `-target`, so every `--emit=exec` with an
    // explicit `--target <host>` failed on gcc-only Linux hosts).
    let host = crate::host_target_triple();
    let flags = linker_flags_for_driver(DriverKind::Gcc, &host).unwrap();
    assert!(
        flags.is_empty(),
        "gcc host-triple link must not add flags, got {flags:?}"
    );
}

#[test]
fn test_linker_flags_gcc_driver_foreign_triple_is_a_clear_error() {
    // A foreign triple cannot be linked by gcc: the error must name the
    // target and the reason, not surface as a baffling cc failure.
    let foreign = if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    let err = linker_flags_for_driver(DriverKind::Gcc, foreign).unwrap_err();
    assert!(err.contains(foreign), "error must name the target: {err}");
    assert!(
        err.contains("-target") || err.contains("clang"),
        "error must be actionable: {err}"
    );
}

#[test]
fn test_linker_flags_raw_ld_keeps_emulation() {
    // A raw GNU `ld` has no `--target`; it needs only the `-m <emulation>` flag.
    let flags = linker_flags_for_raw_ld("x86_64-unknown-linux-gnu").unwrap();
    assert_eq!(flags, vec!["-m", "elf_x86_64"]);

    let flags = linker_flags_for_raw_ld("aarch64-unknown-linux-gnu").unwrap();
    assert_eq!(flags, vec!["-m", "aarch64linux"]);
}

#[test]
fn test_linker_flags_raw_ld_rejects_macho() {
    // GNU ld cannot link Mach-O; the error must say so instead of emitting a
    // nonsense emulation flag.
    let err = linker_flags_for_raw_ld("x86_64-apple-darwin").unwrap_err();
    assert!(err.contains("Mach-O"), "error: {err}");
}

#[test]
fn test_linker_flags_for_unknown_target_errors() {
    // An unmapped target must be a hard error rather than silently passing
    // host flags to a cross target. Only the raw-ld path validates the triple
    // (clang drivers accept any triple and let the toolchain reject later).
    let err = linker_flags_for_raw_ld("sparc64-unknown-linux-gnu");
    assert!(err.is_err());
    let msg = err.unwrap_err();
    assert!(msg.contains("sparc64-unknown-linux-gnu"));
    assert!(msg.contains("not in the supported linker-flag table"));
}

#[test]
fn test_linker_flags_for_target_smoke() {
    // End-to-end smoke of the probing entry point: whatever driver the host
    // resolves, the call must succeed for the host triple and produce flags
    // the driver will actually accept (gcc: none, clang: `-target`).
    let host = crate::host_target_triple();
    let flags = linker_flags_for_target(&host, "cc").unwrap();
    if cfg!(target_os = "macos") {
        // macOS `cc` is Apple clang.
        assert_eq!(flags, vec!["-target".to_string(), host.clone()]);
    } else if flags.len() == 2 && flags[0] == "-target" {
        // A clang installed as `cc` on Linux/BSD.
        assert_eq!(flags[1], host);
    } else {
        // GNU gcc: host triple needs no flags.
        assert!(flags.is_empty(), "unexpected flags {flags:?}");
    }
}
