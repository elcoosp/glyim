//! `glyim` compiler command-line entry point.
//!
//! Thin binary wrapper: delegates to `glyim_cli::run`, which prints its
//! own diagnostics in the `--error-format`-selected style. `Ok(code)`
//! carries the process exit code to terminate with — 0 for code-producing
//! modes, the interpreted program's own exit code for `--emit=run` — and
//! `Err` signals a driver failure (exit 1).

fn main() {
    match glyim_cli::run() {
        Ok(code) => std::process::exit(code),
        Err(_) => std::process::exit(1),
    }
}
