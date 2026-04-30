#![no_main]
use libfuzzer_sys::fuzz_target;
use arbitrary::Arbitrary;

#[derive(Arbitrary, Debug)]
struct FuzzInput { data: Vec<u8> }

fuzz_target!(|input: FuzzInput| {
    let s = String::from_utf8_lossy(&input.data);
    let result = glyim_parse::parse(&s);
    let _ = result.ast.items.len();
});
