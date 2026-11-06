// KNOWN_GAPS §3.9 (this session): same unsize-coercion gap as
// struct_field_slice_len.g, exercised through a generic ADT — the field's
// declared type `&[T]` must be substituted to `&[i32]` before the
// aggregate-construction coercion decides.
// test-mode: run-pass
// exit-code: 3
struct Wrapper<T> { buf: &[T] }
fn main() -> i32 {
    let arr = [7, 8, 9];
    let w = Wrapper::<i32> { buf: &arr };
    w.buf.len() as i32
}
