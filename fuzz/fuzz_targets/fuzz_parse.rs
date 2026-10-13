#![no_main]
// Target 1: lexer + parser + CST construction. A crash here is a P0 bug:
// the frontend is the user-input boundary and must never panic (its
// crate-root ratchet enforces no unwrap/expect/panic in non-test code;
// the fuzzer verifies the claim against reality).
//
// Invariants checked per input:
//   1. parse() must not panic (libFuzzer catches any panic).
//   2. The produced CST must round-trip to the exact input text
//      (losslessness is Rowan's core contract).

use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

static PANIC_HOOK: OnceLock<()> = OnceLock::new();

fn install_hook() {
    PANIC_HOOK.get_or_init(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Print the offending input so the reproducer is obvious.
            eprintln!("PANIC — see /tmp/glyim-fuzz-last-input.g");
            default_hook(info);
        }));
    });
}

fuzz_target!(|data: &[u8]| {
    install_hook();
    // Only feed inputs that are at least valid UTF-8 (the lexer is
    // specified over UTF-8 sources); non-UTF-8 bytes are rejected the
    // same way the CLI would reject them.
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    // Keep inputs bounded so the fuzzer spends its budget on structure
    // instead of on gigantic files (the parser also has its own depth
    // limits — the 1 MiB cap here guards the fuzz driver's own memory).
    if src.len() > 1024 * 1024 {
        return;
    }
    let _ = std::fs::write("/tmp/glyim-fuzz-last-input.g", src);
    let parsed = glyim_frontend::parse_to_syntax(src, glyim_span::FileId::from_raw(0));
    // Losslessness: the CST must render back to the exact input.
    let rendered = parsed.root.to_string();
    assert_eq!(
        rendered, src,
        "CST round-trip mismatch: the parser rewrote the program"
    );
});
