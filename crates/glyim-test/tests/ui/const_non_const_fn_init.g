// A `const` initializer must be const-evaluable (Rust E0015). This used to
// ICE with `[X0000] ConstRef(..) reached LLVM codegen` for ADT-typed
// consts because the eval error was suppressed; it must be a clean
// `[C0001]`.
struct S { v: i32 }
fn make() -> S { S { v: 7 } }
const K: S = make();
fn main() -> i32 { K.v }
