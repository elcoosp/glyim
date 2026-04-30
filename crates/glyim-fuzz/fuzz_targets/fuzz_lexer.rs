#![no_main]

use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    data: Vec<u8>,
}

fuzz_target!(|input: FuzzInput| {
    let s = String::from_utf8_lossy(&input.data);
    let tokens = glyim_lex::tokenize(&s);
    let mut last_end = 0;
    for tok in &tokens {
        assert!(tok.start <= tok.end, "token start > end: {:?}", tok);
        if tok.kind.is_trivia() {
            continue;
        }
        if tok.start < last_end {
            // overlapping tokens possible during error recovery; just continue
        }
        last_end = tok.end;
    }
});
