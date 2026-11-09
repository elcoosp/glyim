//@ mode: run-pass
//@ compile-flags: --backend=bytecode
//@ expect-stdout: drop 2\ndrop 1
//@ feature: COMPILER-OPT-DROP-ELAB, STD-CORE-OPS, TOOL-TEST-RUN-PASS
//@ backends: gam, bytecode

struct WithDrop {
    id: i32
}

impl Drop for WithDrop {
    fn drop(&mut self) {
        println!("drop {}", self.id);
    }
}

fn main() {
    let _a = WithDrop { id: 1 };
    let _b = WithDrop { id: 2 };
}
