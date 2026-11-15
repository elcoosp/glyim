//! Consumer walkthrough of the server-log seam (LSP-LOG-FILE): the
//! `--log-file` flag behind an editor session's trace file.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p glyim-lsp --example server_log_walkthrough
//! ```
//!
//! The example drives the public path end-to-end without a client
//! process: a `FileLog` sink is opened on a throwaway file, installed
//! as the global default subscriber exactly as `run_server` does for
//! `--log-file`, a few events of the shape the engines emit (a request
//! `debug!`, a dropped-edit `warn!`) are traced, and the file is read
//! back — one JSON object per line, ready for `jq`. A log you cannot
//! read after a crash is no log, so the writer flushes per event (the
//! `FileLogWriter` drop is the flush point); the walkthrough proves
//! that by reading the file while the process is still alive.

use glyim_lsp::server_log::{FileLog, install_global};

fn main() {
    let path = std::env::temp_dir().join(format!(
        "glyim-lsp-server-log-walkthrough-{}.log",
        std::process::id()
    ));

    // The wire-up `run_server` performs for `--log-file X`: open the
    // sink, install it as the global default subscriber. A failure
    // here is a named error the server degrades on — never a panic.
    let log = FileLog::open(&path).expect("the temp-dir log file opens");
    install_global(log).expect("the walkthrough process has no default subscriber");

    // Events of the shape the engines already emit: a request-completed
    // `debug!` and a dropped-edit `warn!` (the same discipline the
    // didOpen/didChange handlers use on the bounded analysis channel).
    tracing::debug!(
        method = "textDocument/declaration",
        elapsed_us = 47,
        "request answered"
    );
    tracing::warn!(
        path = "/tmp/session/main.g",
        "analysis channel dropped the didChange edit"
    );

    // Read the file back while the process is still alive — the
    // flush-on-drop guarantee at work.
    let written =
        std::fs::read_to_string(&path).expect("events are flushed to the file as they are emitted");
    println!("log file: {}", path.display());
    for line in written.lines() {
        println!("  {line}");
    }
    println!("Each line is one JSON object: try `jq -r .fields.msg` on the real file.");

    // The walkthrough's file is throwaway; leave no temp litter.
    let _ = std::fs::remove_file(&path);
}
