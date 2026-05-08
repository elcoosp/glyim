#![no_main]
use libfuzzer_sys::fuzz_target;
use glyim_parse::parse;
use glyim_hir::lower;

fuzz_target!(|data: &[u8]| {
    if let Ok(source) = std::str::from_utf8(data) {
        if let Ok(parse_output) = parse(source) {
            let mut interner = parse_output.interner;
            let _ = lower(&parse_output.ast, &mut interner);
        }
    }
});
