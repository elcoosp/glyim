//! Tests for glyim_panic FFI function.
//!
//! `glyim_panic` exits the process with status 101 — the same exit code a
//! Rust panic produces, and the same code `glyim-mir-interp` reports for an
//! `InterpError`. It used to call `std::process::abort()`, which raised
//! SIGABRT (exit status 134 in a shell): the same program then exited with
//! a different code depending on whether it ran through the LLVM backend
//! or the interpreter, which is precisely what the `run-fail` fixtures
//! (`// exit-code: 101`) exist to pin.
//!
//! The test spawns a child process that calls `glyim_panic` and verifies
//! the child exits 101 with the message on stderr.

use crate::glyim_panic;

/// Test that `glyim_panic` exits 101 and writes the message to stderr.
#[test]
fn panic_exits_101_with_message() {
    // If we're the child process, trigger the panic.
    if std::env::var("__GLYIM_PANIC_CHILD").is_ok() {
        let msg = b"boom from the child";
        glyim_panic(msg.as_ptr(), msg.len());
    }

    // Parent process: spawn child and check its exit status.
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        panic!("failed to get current executable: {}", e);
    });

    let output = std::process::Command::new(&exe)
        .env("__GLYIM_PANIC_CHILD", "1")
        .output()
        .unwrap_or_else(|e| {
            panic!("failed to spawn child process: {}", e);
        });

    assert_eq!(
        output.status.code(),
        Some(101),
        "glyim_panic must exit 101; got {:?} (signal {:?})",
        output.status.code(),
        {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                output.status.signal()
            }
            #[cfg(not(unix))]
            {
                None::<i32>
            }
        }
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("boom from the child"),
        "the panic message must reach stderr; got: {stderr:?}"
    );
}
