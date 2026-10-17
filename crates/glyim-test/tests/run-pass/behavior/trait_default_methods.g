// ANTI-REGRESSION / ANTI-MISCOMPILATION: trait default methods must run
// when the impl overrides nothing (`impl Greet for P { }`).
//
// `impl T for P { }` puts every method on the *trait*, so the
// impl-method loop in typeck iterated nothing and never checked the
// default bodies. `resolve_method_call` then fell back to
// `hir.body_owners[body_id]` — a different id space that pointed at an
// unrelated function whose signature was `Adt1010<char>` /
// `&mut Adt2` and whose body was empty. `p.m()` returned 0 (pure
// default) or a garbage composite, both silent wrong values.
//
// The fix walks the trait's un-overridden methods, checks each default
// body, records the mapping in `body_owner_map`, and registers the impl
// method — rolling all of that back if the body can't be checked for
// this impl (missing associated-type bindings in the stdlib's generic
// trait defaults).
// test-mode: run-pass
// exit-code: 105
// compile-flags: --with-stdlib
// needs-llvm
trait Greet {
    fn name(&self) -> i32;
    fn greet(&self) -> i32 { self.name() + 100 }
}
struct P { n: i32 }
impl Greet for P { fn name(&self) -> i32 { self.n } }
fn main() -> i32 {
    let p = P { n: 5 };
    // 5 + 100 = 105
    p.greet()
}
