//@ mode: compile-pass
// V18-T03: Spawn a thread and join
//@ feature: STD-STD-THREAD, RUNTIME-FFI-THREAD, TOOL-TEST-COMPILE-PASS
//@ backends: gam

fn main() {
    let handle = thread::spawn(|| {
        42
    });
    let result = handle.join();
}
