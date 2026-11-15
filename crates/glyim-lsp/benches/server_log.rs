//! Server-log bench (LSP-LOG-FILE).
//!
//! Dependency-free (`harness = false`), mirroring the request benches:
//! measures the per-event cost of emitting through the installed
//! log-file subscriber — the `FileLog` `MakeWriter` seam with its
//! flush-on-drop durability — so CI logs can track the observability
//! overhead the `--log-file` flag adds per engine event. The dominant
//! cost is the per-event file write; batching events would trade
//! crash-tail durability for throughput. Run with:
//!
//! ```text
//! cargo bench -p glyim-lsp --bench server_log
//! ```

use std::hint::black_box;
use std::time::Instant;

use glyim_lsp::server_log::{FileLog, install_global};

const EVENTS: usize = 1_000;

fn main() {
    // The bench runs as its own process: the global install is
    // uncontended, and the log file is a throwaway in the temp dir.
    let path = std::env::temp_dir().join(format!(
        "glyim-lsp-server-log-bench-{}.log",
        std::process::id()
    ));
    let log = FileLog::open(&path).expect("the temp-dir log file opens");
    install_global(log).expect("the bench process has no default subscriber");

    // Warmup: the first event pays the subscriber's lazy setup.
    for i in 0..8 {
        tracing::debug!(seq = i, "warmup event for the server-log bench");
    }

    let t0 = Instant::now();
    for i in 0..EVENTS {
        tracing::info!(
            seq = i,
            cost_hint = i as u64,
            "bench event through the log-file seam"
        );
        black_box(i);
    }
    let elapsed = t0.elapsed();

    println!(
        "event emit            : {:>9.1?}/event ({EVENTS} events through the installed file log)",
        elapsed / EVENTS as u32
    );

    // The bench's own footprint is throwaway; leave no temp litter.
    let _ = std::fs::remove_file(&path);
    black_box(path);
}
