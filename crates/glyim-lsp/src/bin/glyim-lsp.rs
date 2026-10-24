//! `glyim-lsp` — the Glyim language server binary.
//!
//! P0-4 (production-readiness plan §5): the LSP crate advertised itself in
//! the README but had no `[[bin]]`, no `main`, and `run_server` was only
//! reachable from tests. This entry point makes the server launchable by
//! any editor that speaks LSP over stdio.
//!
//! Usage:
//!   glyim-lsp                 # LSP over stdio
//!   glyim-lsp --log-file X    # tee tracing to X
//!
//! Editors configure it as the "command" for the Glyim language. Example
//! for VS Code / Zed / helix: `glyim-lsp` (found on `$PATH`).
//!
//! The server exits non-zero on a fatal I/O error (broken pipe, missing
//! stdin); a clean client disconnect returns 0.

use std::path::PathBuf;

fn main() {
    let mut log_file: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--log-file" => log_file = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!(
                    "glyim-lsp — Glyim language server (LSP over stdio)\n\
                     \n\
                     USAGE:\n    glyim-lsp [--log-file PATH]"
                );
                return;
            }
            "-V" | "--version" => {
                println!("glyim-lsp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("glyim-lsp: unknown argument '{other}' (try --help)");
                std::process::exit(2);
            }
        }
    }
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("glyim-lsp: failed to start tokio runtime: {e}");
            std::process::exit(1);
        }
    };
    rt.block_on(glyim_lsp::server::run_server(log_file));
}
