#![no_main]
use libfuzzer_sys::fuzz_target;
use glyim_parse::parse;
use glyim_hir::lower;
use glyim_typeck::TypeChecker;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let parse_out = parse(s);
        let mut interner = parse_out.interner;
        let hir = lower(&parse_out.ast, &mut interner);
        let mut tc = TypeChecker::new(interner);
        let _ = tc.check(&hir); // ensure no panics
    }
});
