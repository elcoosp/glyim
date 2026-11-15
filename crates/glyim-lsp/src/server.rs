use crate::AnalysisDatabase;
use crate::driver::AnalysisDriver;
use crate::handler::build_router;
use async_lsp::MainLoop;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Runs the language server's main loop over stdio until the client
/// disconnects (a clean EOF is `Ok(())`).
///
/// When `log_file` is `Some`, the tracing events the engines emit are
/// installed as the global default subscriber writing one JSON line per
/// event to that file (LSP-LOG-FILE). The wire-up is best-effort: a file
/// that cannot be opened, or a subscriber that cannot be installed (an
/// embedding host already set one), degrades to a one-line stderr note
/// and a server that still serves — losing the log must never cost the
/// session.
///
/// # Errors
///
/// Any fatal stdio failure — the stdin/stdout tokio locks cannot be
/// taken, or the buffered main loop itself errors (a broken pipe
/// mid-session, an undecodable message). The binary entry point maps
/// this to a non-zero exit and a one-line stderr report instead of a
/// panic (the contract the `glyim-lsp` bin doc already promised).
pub async fn run_server(
    log_file: Option<std::path::PathBuf>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(path) = log_file {
        let installed =
            crate::server_log::FileLog::open(&path).and_then(crate::server_log::install_global);
        if let Err(err) = installed {
            eprintln!(
                "glyim-lsp: --log-file {}: {err}; continuing without a log",
                path.display()
            );
        }
    }
    let db = Arc::new(AnalysisDatabase::new());
    let (tx, rx) = mpsc::channel(16);
    let (mainloop, client_socket) =
        MainLoop::new_server(|_peer_socket| build_router(db.clone(), tx.clone()));
    // T130-PATCHED [LSP-6]: also give the driver the sender so it can
    // enqueue re-analysis of dependent files after a change.
    //
    // P0-4 (production-readiness plan §5): pass the `ClientSocket` through
    // to the driver so it can publish `textDocument/publishDiagnostics`
    // after each analysis. Before this the socket was discarded as
    // `_client_socket` and no notification was ever sent — the README's
    // "LSP with diagnostics" claim was a lie.
    let driver = AnalysisDriver::new(db, rx, tx, Some(client_socket));
    tokio::spawn(driver.run());
    let stdin = async_lsp::stdio::PipeStdin::lock_tokio()?;
    let stdout = async_lsp::stdio::PipeStdout::lock_tokio()?;
    mainloop.run_buffered(stdin, stdout).await?;
    Ok(())
}
