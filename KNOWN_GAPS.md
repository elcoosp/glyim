# KNOWN_GAPS

A factual inventory of known correctness/feature gaps in the Glyim compiler,
current as of the commit that introduced this file. Each entry is either

  (a) a **hard error** the compiler emits (the program is rejected with a
      diagnostic rather than miscompiled), or
  (b) a **latent bug** that is reachable from user code but not covered by
      the run-pass corpus.

Anything in category (b) is a candidate for a `no-ice` or `should-fail`
fixture. Anything in category (a) is a candidate for a "not yet supported"
UI test.

The compiler's differential test (`crates/glyim-test/tests/differential.rs`)
runs every run-pass fixture on **both** backends — the LLVM backend and the
MIR interpreter — and asserts they agree on exit code and stdout. A gap that
only exists on one backend shows up there; a gap that fails identically on
both does not, which is why some entries below have no corpus coverage.

---

## 1. Builtin methods not lowered inline (category b)

`crates/glyim-type/src/ty_ctx_mut.rs`'s builtin table registers ~57
`(AdtId, method)` pairs. Codegen lowers them inline only if the method name
is in the whitelist in `crates/glyim-codegen-llvm/src/lower.rs`
(`try_lower_builtin_intrinsic`). A registered method **not** in that
whitelist falls through to the ordinary `Call` path and emits a call to a
`__glyim_fn_NNNN` symbol that has no body — an undefined symbol at link time.
The interpreter is a second implementation that must be kept in sync, or the
differential will disagree.

### 1a. Lowered this session (for reference)

`Vec::{clear, remove, insert, truncate, set_len, capacity, reserve,
as_slice, as_ref}`, `String::{clear, from, from_utf8_lossy, into_bytes}`,
`str::clear`, `slice::to_vec`. Fixtures in
`crates/glyim-test/tests/run-pass/behavior/`.

### 1b. Closures — FIXED; combinators still open

Closures now work end-to-end on the LLVM backend. The original breakage had
four independent causes, all now fixed:

1. **Closure signatures were never zonked.** `TyCtxMut::closure_sigs`
   stored each `FnSig` as registered during typeck, when the body's
   result/parameter types could still be raw `Infer` vars. `zonk_bodies`
   folded the THIR body but not this separate table, so `fn_abi_of`
   panicked with `UnknownType(Infer)`. Fixed by
   `glyim-typeck::zonk::zonk_closure_sigs`, called from `typeck_crate`.
2. **Closures were declared with a void type.** `MirConstKind::Fn`'s
   forward-declaration arm looked the callee up in `fn_sigs` only;
   closure signatures live in `closure_sigs`, so every closure got a
   placeholder `void` type that `lower_body` then reused. Fixed by
   falling back to `closure_sigs`.
3. **Closure parameter locals were allocated twice** — once by
   `lower_closure`, once by `MirBuilder::lower_body` — so the prologue
   stored the incoming argument into one slot and the body read another
   (uninitialized stack). Fixed by sharing the local via
   `local_var_map`.
4. **Closure value layout had no fn-pointer field.** Only captures were
   counted, so a non-capturing closure was zero-sized and its aggregate
   store went through a null alloca; a closure in a `fn(..)`-typed
   formal position SIGBUS'd. Fixed by making `TyKind::Closure` layout
   `{ fn_ptr, captures... }`, adding a `Place::ty` arm for closure
   `Field` projections, and reading captures via projected places.

Coverage: `behavior/closure_{bare,capture,inferred,no_args,fnptr}.g`.

**Still open — closure-passing combinators.** `Option::map`,
`Result::{map, map_err, map_or}`, `Option::unwrap_or_else`. Neither backend
implements them. This is a multi-layer feature, not a builtin arm.

Diagnosis (verified by probe at the time of writing):

1. `o.map(f)` resolves to `MethodDispatch::Static`, **not** `Builtin`. The
   builtin fast-path gate in `resolve_method_call`
   (`glyim-typeck/src/check_expr.rs`, the `is_builtin_adt` closure) lists
   only `1020 | 1040 | 1050` (Vec / Box / String). `Option` (1010) and
   `Result` (1011) are deliberately excluded.
2. Simply widening `is_builtin_adt` to include `1010 | 1011` breaks the
   stdlib: `Option`/`Result` also have *user* `impl` blocks in
   `glyim-lang-core/lib/{option,result}.g`, and forcing the builtin table
   first produces
   `mismatched types: T vs <Self as Trait37>::Item` cascades (the generic
   combinator impls in `iter.g` no longer agree with the builtin sigs).
3. Even when dispatch reaches `try_builtin_method`, the closure argument
   needs a `fn(T) -> U` expectation so `U` unifies. The mechanism exists
   (`pending_closure_expectation`, set from the builtin sig's `inst_inputs`
   in the `MethodCall` arg loop), but only fires for `MethodDispatch::Builtin`.

So the fix is a real feature: decide how builtin `Option`/`Result` methods
coexist with the stdlib's own impls (either drop the stdlib impls for the
combinator methods, or make the builtin table the sole authority for them),
then lower the combinators on both backends (codegen: construct the result
enum + invoke the closure via the existing fn-pointer call path; interp: add
the `map`/`map_or`/`map_err` arms). This is out of scope for a tail fix —
it is its own work item.

Remaining symptoms:

| Method | Symptom |
|---|---|
| `Option::map` | `layout error building aggregate: UnknownType(Ty(0))` |
| `Result::map` | same class |
| `Result::map_err` | `fn_abi_of failed: UnknownType(Ty(169))` |
| `Result::map_or` | undefined `__glyim_fn_9122` |

### 1c. Atomics — mostly done

`AtomicBool` / `AtomicUsize` / `AtomicU8`: `new`, `load`, `store`,
`fetch_and`, `fetch_or`, `fetch_add`, `fetch_sub` are lowered inline
(codegen + interp), with runtime helpers. Fixtures in
`behavior/atomic_*.g`.

Three sub-items were fixed along the way (each its own commit):

1. The `Atomic*` ADTs were registered as **zero-field structs** (0 bytes)
   while `sync.g` declares them as `struct AtomicX { v: UnsafeCell<T> }`.
   `AtomicBool::new(true)` stored `const_zero()` into a 0-byte alloca and
   discarded the value. Now registered with the bare integer field
   (`u8`/`usize`/`i32`/`u32`).
2. `cmp.g`'s `Ordering` lacked the four memory-order variants, so every
   atomic call site failed type-check before reaching codegen.
3. `glyim_atomic_u8_fetch_and` / `_fetch_or` runtime helpers added.

**`compare_exchange` — DONE.** Lowered inline on both backends
(commit `2c59c67f`): codegen builds `Result::Ok(prev)` / `Result::Err(prev)`
from the CAS helper's return; the interp mutates in place on success.
`glyim_atomic_usize_cas` added to the runtime. Fixtures:
`behavior/atomic_{usize,bool}_compare_exchange*.g`.

Mechanism note (kept from the earlier investigation, still true): the
builtin method table **shadows** `sync.g`'s atomic impls, so do NOT try
to fix atomics by deleting the builtin entries — the methods then fail
earlier with ``no method `load` found``. The table is load-bearing for
resolution.

### 1d. Iterator protocol — partial

**Working:** `for i in 0usize..n` (a `Range<usize>`). Fixed by two changes
(commit `2171bde9`): the for-loop `SwitchInt` now switches on the
`Option`'s discriminant (it switched on the whole aggregate), and
`Range<usize>` gained an `impl Iterator`.

**Still open — `for i in 0..5` (a `Range<i32>`).** Fails with
`mismatched types: usize vs i32`. `0..5` is `Range<i32>` (the unsuffixed
integer default), but the stdlib only implements `Iterator for
Range<usize>`, so the for-loop's `Iterator::next` lookup binds the loop
variable to `usize` and the range's `i32` bounds mismatch. Adding
`impl Iterator for Range<i32>` does **not** fix it — the solver unifies
`Range<?int>` against the `usize` impl first (`conflicting
implementations`/wrong-variant). The correct fix is the `into_iter()`
desugar: typeck's `For` arm (`check_expr.rs`) currently resolves
`Iterator::next` on the *iterable* directly; it should resolve
`IntoIterator::into_iter(iterable)` → `IntoIter`, then `Iterator::next` on
that. That also unblocks the rest of the protocol.

**Still open — slice iteration.** `slice::iter` / `iter_mut` →
`no method 'iter' found`; `[T]::as_slice` on a bare array likewise. Needs
the same `IntoIterator` wiring plus an `Iter`/`IntoIter` ADT for slices
(the stdlib has `impl<'a, T> Iterator for Iter<'a, T>` in `iter.g`, but
the for-loop path never reaches it without `into_iter`).

### 1e. Simple, unimplemented

| Method | Symptom |
|---|---|
| `slice::copy_from_slice` | undefined `__glyim_fn_9092` |  **← fixed**
| `slice::split_at` | undefined `__glyim_fn_9164` (returns a tuple of two fat pointers) |  **← fixed**
| `String::from_utf8` | `ambiguous method 'len' found in multiple impls` (typeck, on the `Result` it returns) |

### 1f. `Vec::new()` element type never unified — inference ordering (category b)

The root cause is upstream of `pop`. `let mut v = Vec::new(); v.push(5);`
leaves `v` typed `Vec<Infer>` — the element type is **never unified with
`i32`**. Evidence: the MIR for

    fn main() -> i32 { let mut v = Vec::new(); v.push(5); let o = v.pop(); ... }

prints every `Vec` local as `Adt1020<<error>>` and the `pop` result as
`Adt1010<<error>>`. `Vec::new`'s registered output is `Vec<Param(0)>`; the
`Param(0)` becomes a fresh inference var at the call site, but the later
`v.push(5)` does not propagate `i32` back into it.

Consequences:
- `v.pop()` alone: the codegen `pop` arm bails (both the receiver's and the
  destination's element types are non-concrete) and the call falls through to
  `fn_abi_of` on `Option<Infer>` -> `UnknownType(Ty(22677))` ICE.
- `v.pop().unwrap()`: the same ICE one step later.
- `Vec::len()` after `push` works only because the `len` arm reads the
  length word directly and never needs `T`.

Precise blocker (found by probe): in the method-arg loop of `check_expr`,
`v.push(5)` sees

    arg[0] method=push  a_ty=Infer(Int(IntVar(240)))  expected=Infer(Ty(TyVar(_)))

— the argument is an **int** inference var (the unsuffixed `5`), the
expected (receiver element) is a **type** inference var. `unify` cannot
connect an `InferVar::Int` to an `InferVar::Ty`, so `T` stays unresolved.
A correct fix has to give `Vec::new()`'s element var the right *kind* — or
unify the integer literal's default (`i32`) into it — which is a change to
the inference engine's var representation, not a local patch. The
`Vec::len()`-only fixtures pass because that arm reads the length word and
never needs `T`.

This is a typeck inference-ordering bug (the class the `MaybeUninit::new`
fix addressed for associated functions), not an inline-lowering gap. Out of
scope for a codegen-only fix.

---

## 2. Async (category a — hard error)

`.await` inside a loop body is rejected:

  `.await` inside a loop body is not yet supported by the async
   state-machine lowering (tracked: KNOWN_GAPS.md async-v2).

Source: `crates/glyim-hir/src/lower/lower_async.rs`.

What **does** work: single-suspension bodies (single-poll desugar), and
multi-`.await` sequential bodies with no `.await` inside a loop. Multi-await
v1 state-machine scaffolding exists in `lower_async.rs` but the resume
dispatch is deferred.

---

## 3. Const evaluation (category a)

`crates/glyim-const-eval/src/eval.rs` rejects, with an explicit diagnostic:

- path expressions for non-local paths
- `await`
- `?`
- dereference
- `for` over some iterable kinds

---

## 4. Codegen / ABI (category b)

- **Windows SEH landingpads** — deferred. Only the Itanium
  `landingpad`/`resume` pair is emitted. Windows targets with cleanup blocks
  are not supported. (`crates/glyim-codegen-llvm/src/lower.rs`, plan §19.1.)
- **ThinLTO** — `--lto thin` surfaces an explicit error rather than silently
  no-op'ing (correct behavior; the linker-driver integration is the gap).

---

## 5. Macros (category a)

- `include!` (source-code inclusion) is not supported. `include_str!` works.
  Source: `crates/glyim-meta/src/expander/mod.rs`.

---

## 6. Other

- **Enum-variant value paths** — `let f = Some;` (a variant constructor used
  as a value, not called) is rejected:
  `enum-variant value paths are not yet supported`. Source:
  `crates/glyim-typeck/src/unify.rs`.
- **`glyim-bytecode-vm`** — the crate exists but nothing in `glyim-cli` or
  `glyim-pipeline` references it. Either wire it or remove it.

---

## Verification

To re-check the status of any entry:

```bash
# builtin lowering: does the method reach codegen inline?
grep -n '<method>' crates/glyim-codegen-llvm/src/lower.rs

# differential (both backends)
GLYIM_TEST_REAL_LLVM=1 cargo test -p glyim-test --features real-llvm \
  --test differential -- --nocapture
```
