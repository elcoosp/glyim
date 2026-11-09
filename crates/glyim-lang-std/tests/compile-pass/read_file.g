//@ mode: compile-pass
// V18-T02: Read a file from disk
//@ feature: STD-STD-FS, RUNTIME-FFI-FS, STD-STD-IO, RUNTIME-FFI-IO, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let content = fs::read_to_string("test.txt");
    match content {
        Result::Ok(s) => println!("{}", s),
        Result::Err(e) => eprintln!("error: {}", e.message()),
    }
}
