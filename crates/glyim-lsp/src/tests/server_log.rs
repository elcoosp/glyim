//! Tests for the server log-file seam (LSP-LOG-FILE).
//!
//! The `--log-file` flag was parsed and documented but `run_server`
//! accepted it as `_log_file` and never used it — every tracing event the
//! engines emit went nowhere. These tests pin the seam that closes that
//! gap: `FileLog` implements `tracing_subscriber`'s `MakeWriter` (the
//! writer choice is pluggable), bytes actually land in the file, a
//! globally-installed log emits JSON lines a consumer can parse, and a
//! file that cannot be opened degrades to a named error — never a panic,
//! never a silent no-op.

use crate::server_log::{FileLog, LogInitError, install_global};
use std::io::Write;
use tracing_subscriber::fmt::MakeWriter;

/// The `MakeWriter` seam hands out writers that land bytes in the log
/// file — proven here without any subscriber, exactly as a future
/// in-process consumer would drive it.
#[test]
fn file_log_writes_bytes_through_the_make_writer_seam() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = dir.path().join("server.log");
    let log = FileLog::open(&path).expect("the log file opens");

    let mut writer = log.make_writer();
    writeln!(writer, "first line").expect("write through the seam");
    writeln!(writer, "second line").expect("write through the seam");
    drop(writer);

    let written = std::fs::read_to_string(&path).expect("the log file is readable");
    assert_eq!(written, "first line\nsecond line\n", "bytes land in order");
}

/// The production wiring (`run_server` → `install_global`) emits one
/// JSON line per tracing event; a consumer can parse each line and find
/// the event's target and fields.
#[test]
fn install_global_emits_parseable_json_lines() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = dir.path().join("server-events.log");
    let log = FileLog::open(&path).expect("the log file opens");

    install_global(log).expect("no default subscriber is set yet in this process");

    // Two events with marker fields: one at the level the engines use
    // for their per-request records (`debug!`) and one at `info!` — the
    // flag exists to capture the former, so the subscriber must not sit
    // behind the default INFO filter.
    tracing::info!(
        marker = "server_log_install_probe",
        "walkthrough event for the log-file seam"
    );
    tracing::debug!(
        marker = "server_log_install_probe_debug",
        "debug-level engine record"
    );

    let written = std::fs::read_to_string(&path).expect("events are flushed to the file");
    for marker in ["server_log_install_probe", "server_log_install_probe_debug"] {
        let probe_line = written
            .lines()
            .find(|line| line.contains(marker))
            .unwrap_or_else(|| panic!("the {marker} event reached the log file"));
        let parsed: serde_json::Value =
            serde_json::from_str(probe_line).expect("each log line is one JSON object");
        assert!(
            parsed["target"]
                .as_str()
                .is_some_and(|t| t.contains("server_log")),
            "the JSON line names the emitting target, got: {parsed}"
        );
        assert_eq!(
            parsed["fields"]["marker"].as_str(),
            Some(marker),
            "the event's fields survive the round trip"
        );
    }
}

/// A log file that cannot be opened is a named error — the caller
/// (`run_server`) degrades to a one-line stderr note and keeps serving,
/// but the failure is never silent and never a panic.
#[test]
fn unopenable_log_file_is_a_named_error() {
    // A directory is not a writable log file: the open must fail with
    // the Open variant, and the error text must name the operation.
    let dir = tempfile::TempDir::new().expect("temp dir");
    let err = FileLog::open(dir.path()).expect_err("a directory cannot be a log");
    match &err {
        LogInitError::Open(io_err) => {
            assert!(
                io_err.raw_os_error().is_some(),
                "the underlying OS error is preserved for diagnostics"
            );
        }
        other => panic!("an open failure is the Open variant, got {other:?}"),
    }
    assert!(
        err.to_string().contains("open the log file"),
        "the display text names the failed operation, got: {err}"
    );
}

/// The log appends across handles: a restarted server keeps its prior
/// session's history instead of truncating it.
#[test]
fn file_log_appends_across_handles() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let path = dir.path().join("append.log");

    {
        let log = FileLog::open(&path).expect("first open");
        writeln!(log.make_writer(), "session one").expect("write");
    }
    {
        let log = FileLog::open(&path).expect("second open");
        writeln!(log.make_writer(), "session two").expect("write");
    }

    let written = std::fs::read_to_string(&path).expect("the log file is readable");
    assert_eq!(
        written, "session one\nsession two\n",
        "a re-opened log appends, never truncates"
    );
}
