// §3.10 regression: user-defined trait with `&mut self` method used to
// ICE the compiler (`IndexVec::get: index out of bounds`). Root cause
// was `InferenceTable::rollback_to` replacing the var vector with a
// shorter clone, orphaning `Infer(TyVar(N))` handles stored in
// `fn_sigs`. Fixed by truncating *values* back into the existing slots
// (length preserved), so every allocated `TyVar` index stays valid.
//@ mode: run-pass
//@ expect-exit: 42
//@ feature: LANG-SYN-SELF, LANG-SYN-MUT, LANG-TRAIT-IMPL, TOOL-TEST-RUN-PASS
//@ backends: gam, llvm
trait Read {
    fn get(&mut self) -> i32;
}

struct W { r: i32 }

impl Read for W {
    fn get(&mut self) -> i32 { self.r }
}

fn main() -> i32 {
    let mut w = W { r: 42 };
    w.get()
}
