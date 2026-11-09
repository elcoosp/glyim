//@ mode: compile-pass
// V18-T05: Environment variables
//@ feature: STD-STD-ENV, RUNTIME-FFI-ENV, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let val = env::var("HOME");
    match val {
        Result::Ok(home) => println!("HOME={}", home),
        Result::Err(e) => eprintln!("error: {}", e),
    }
    env::set_var("GLYIM_TEST", "hello");
    let args = env::args();
}
