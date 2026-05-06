use crate::handler::build_router;
use crate::AnalysisDatabase;
use crate::driver::AnalysisDriver;
use std::sync::Arc;
use tokio::sync::mpsc;
use async_lsp::MainLoop;

pub async fn run_server(_log_file: Option<std::path::PathBuf>) {
    let db = Arc::new(AnalysisDatabase::new());
    let (tx, rx) = mpsc::unbounded_channel();

    let (mainloop, _client_socket) = MainLoop::new_server(|peer_socket| {
        build_router(db.clone(), tx.clone(), peer_socket)
    });

    // Spawn analysis driver (client_socket may be useful for other things,
    // but publish_diagnostics works through the peer_socket already passed into the router)
    let driver = AnalysisDriver::new(db, rx);
    tokio::spawn(driver.run());

    // I/O setup using tokio feature
    let stdin = async_lsp::stdio::PipeStdin::lock_tokio().unwrap();
    let stdout = async_lsp::stdio::PipeStdout::lock_tokio().unwrap();
    mainloop.run_buffered(stdin, stdout).await.unwrap();
}
