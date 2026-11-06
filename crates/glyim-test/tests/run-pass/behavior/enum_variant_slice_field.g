// KNOWN_GAPS §3.9 (this session): enum-variant aggregates are the second
// aggregate-construction site that needs the array→slice unsize coercion
// (variant fields live in `AdtDef.variants[..].fields`, not the struct-only
// flat `fields` list).
// test-mode: run-pass
// exit-code: 3
enum Holder<'a> { Has(&'a [i32]), Empty }
fn main() -> i32 {
    let arr = [4, 5, 6];
    let h = Holder::Has(&arr);
    match h {
        Holder::Has(b) => b.len() as i32,
        Holder::Empty => 0,
    }
}
