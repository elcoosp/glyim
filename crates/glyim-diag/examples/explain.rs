//! `--explain <code>` capability demo (plan §6 M3.3, CT-5).
//!
//! The library counterpart of the CLI's `--explain` flag: look up a
//! registered error code's explanation and print it. With no argument, lists
//! every registered code with its title — the `--explain` with no code
//! surface rustc also exposes.
//!
//! Run with:
//!
//! ```text
//! cargo run -p glyim-diag --example explain             # list every code
//! cargo run -p glyim-diag --example explain -- T0060    # explain one code
//! ```
//!
//! The CLI flag itself (`glyim --explain T0060`) is a one-line wrapper around
//! [`explain_code`] in `glyim-cli`; it is not built here because `glyim-cli`
//! hard-depends on `glyim-codegen-llvm` (inkwell/llvm-sys 231) and so builds
//! only on hosts with system LLVM 23 (CI's `test-linux-runtime` job). The
//! capability, registry, and this example live in the LLVM-free leaf.

use std::env;

use glyim_diag::error_registry::{explain_code, registry_entries};

fn main() {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some(code) => match explain_code(code) {
            Some(explanation) => {
                println!("{code}: {explanation}");
            }
            None => {
                eprintln!("error: unknown error code `{code}`");
                eprintln!(
                    "note: run `cargo run -p glyim-diag --example explain` with no arg to list every code"
                );
                std::process::exit(1);
            }
        },
        None => {
            let entries = registry_entries();
            println!("Glyim error codes ({})", entries.len());
            for entry in entries {
                println!("  {} — {}", entry.code, entry.title);
            }
            println!("\nPass a code to see its explanation: `--explain T0060`.");
        }
    }
}
