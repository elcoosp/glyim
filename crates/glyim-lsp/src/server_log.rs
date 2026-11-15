//! Server log-file support (feature tag LSP-LOG-FILE).
//!
//! The `glyim-lsp` binary parses `--log-file X` and documents it as
//! "tee tracing to X", but `run_server` accepted the flag as `_log_file`
//! and never used it — every tracing event the engines emit (a `debug!`
//! per completed request, a `warn!` per dropped edit or unconvertible
//! URI) went nowhere, because no subscriber was ever installed. This
//! module closes that observability gap.
//!
//! The seam: *where events go* is split from *what gets emitted* by
//! [`FileLog`]'s `MakeWriter` implementation — the writer trait
//! `tracing_subscriber`'s format layer builds on, so tests and embedded
//! consumers can drive the sink without a subscriber, and a future
//! rotating or dual-tee writer slots in behind the same implementation.
//! [`install_global`] wires a [`FileLog`] in as the process-wide default
//! subscriber, emitting one JSON object per line (machine-parsable, one
//! event per line, ready for `jq`). The wire-up in `run_server` is
//! best-effort by contract: a log file that cannot be opened degrades to
//! a one-line stderr note and a server that still serves — losing the
//! log must never cost the session — and the [`LogInitError`] names the
//! failed operation instead of vanishing.

use std::fmt;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use tracing_subscriber::fmt::MakeWriter;

/// Why the log-file sink could not be brought up (LSP-LOG-FILE).
///
/// The display text names the failed operation — the `run_server`
/// wire-up prints it verbatim as its one-line stderr note, so a user
/// chasing a missing log file sees *what* failed, not just that
/// something did.
#[derive(Debug)]
pub enum LogInitError {
    /// The log file could not be opened (bad path, permissions, a
    /// directory). The underlying OS error is preserved.
    Open(io::Error),
    /// The global default subscriber could not be installed because one
    /// is already set (an embedding process installed its own, or the
    /// server ran twice in one process). The boxed error is whatever
    /// `try_init` reported.
    Install(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for LogInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(err) => write!(f, "could not open the log file: {err}"),
            Self::Install(err) => write!(
                f,
                "could not install the global log subscriber: {err} \
                 (a default subscriber is already set)"
            ),
        }
    }
}

impl std::error::Error for LogInitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open(err) => Some(err),
            Self::Install(err) => Some(err.as_ref()),
        }
    }
}

/// The production log sink: appends every event as one line to a file
/// (LSP-LOG-FILE).
///
/// The file is opened in append mode — a restarted server keeps its
/// prior session's history instead of truncating it — and the writes
/// funnel through one `parking_lot`-guarded [`BufWriter`]: no lock
/// poisoning (a panicking engine cannot wedge the log), one
/// serialization point, and the guard-per-event drop is the flush point.
/// Cloning shares the same underlying writer, so an embedding process
/// can hand the sink to several components.
#[derive(Debug, Clone)]
pub struct FileLog {
    writer: Arc<parking_lot::Mutex<io::BufWriter<File>>>,
}

impl FileLog {
    /// Open (or create) `path` as the append-mode log sink.
    ///
    /// # Errors
    ///
    /// [`LogInitError::Open`] when the file cannot be opened for
    /// appending — the OS error is preserved for diagnostics.
    pub fn open(path: &Path) -> Result<Self, LogInitError> {
        let file = File::options().append(true).create(true).open(path);
        match file {
            Ok(file) => Ok(Self {
                writer: Arc::new(parking_lot::Mutex::new(io::BufWriter::new(file))),
            }),
            Err(err) => Err(LogInitError::Open(err)),
        }
    }
}

/// The per-call writer handed to the format layer: a guard over the
/// shared buffered file writer. Dropping the guard releases the lock
/// *and* is the flush point — the buffered bytes land in the file
/// immediately, so the log stays readable after a crash mid-session.
pub struct FileLogWriter<'a> {
    guard: parking_lot::MutexGuard<'a, io::BufWriter<File>>,
}

impl Drop for FileLogWriter<'_> {
    fn drop(&mut self) {
        // Event-per-line durability: the format layer drops this writer
        // after each event, so an explicit flush here is what makes a
        // JSON line hit the disk before the next event is emitted. A
        // log you cannot read after a crash is no log.
        let _ = self.guard.flush();
    }
}

impl Write for FileLogWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.guard.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.guard.flush()
    }
}

impl<'a> MakeWriter<'a> for FileLog {
    type Writer = FileLogWriter<'a>;

    fn make_writer(&'a self) -> Self::Writer {
        FileLogWriter {
            // `parking_lot` guards are infallible: a panicking engine
            // cannot poison the shared writer.
            guard: self.writer.lock(),
        }
    }
}

/// Install `log` as the process-wide default tracing subscriber,
/// emitting one JSON object per event line (LSP-LOG-FILE).
///
/// The filter captures [`DEBUG`](tracing::Level) and above: the engines
/// emit their per-request record as `debug!` events, and the default
/// INFO-level filter would swallow exactly the lines the flag exists to
/// capture. The JSON shape keeps the fields the engines already emit
/// (`target`, `level`, the message, the structured fields) parseable
/// without any server-side state. The subscriber lives as long as the
/// process — the `run_server` wire-up installs it once, before the main
/// loop starts, so every engine event lands in the file.
///
/// # Errors
///
/// [`LogInitError::Install`] when a global default subscriber is
/// already set (the binary runs the server once per process; an
/// embedding host that installs its own subscriber should keep its own
/// and pass `--log-file` off).
pub fn install_global(log: FileLog) -> Result<(), LogInitError> {
    tracing_subscriber::fmt()
        .json()
        // The engines' request records are `debug!` events; the flag
        // exists to capture them, so INFO would silence the file.
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(log)
        .try_init()
        .map_err(LogInitError::Install)
}
