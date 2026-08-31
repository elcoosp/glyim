fn main() {
    // `run()` prints any diagnostics itself (in the format selected by
    // `--error-format`, plan §3.4) and returns `Err` only as a signal to exit
    // non-zero.
    if glyim_cli::run().is_err() {
        std::process::exit(1);
    }
}
