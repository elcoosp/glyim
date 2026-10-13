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

### 1b. Blocked — closure-passing builtins

| Method | Symptom |
|---|---|
| `Option::map` | `layout error building aggregate: UnknownType(Ty(0))` |
| `Result::map` | same class |
| `Result::map_err` | ICE |
| `Result::map_or` | undefined `__glyim_fn_9122` |
| `Option::unwrap_or_else` | untested, same class |

These take a `fn(T) -> U` argument. The failure is in resolving/​lowering the
closure argument's monomorphized body, not in the builtin dispatch itself.
Needs closure-call lowering through the builtin arm before the arm can be
written.

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

**Still open:** `compare_exchange` (all three atomic types). Needs
`Result` aggregate construction in codegen, plus a
`glyim_atomic_usize_cas` runtime helper (only the `u8` CAS exists).

Mechanism note (kept from the earlier investigation, still true): the
builtin method table **shadows** `sync.g`'s atomic impls, so do NOT try
to fix atomics by deleting the builtin entries — the methods then fail
earlier with ``no method `load` found``. The table is load-bearing for
resolution.

### 1d. Blocked — iterator protocol

`slice::iter` / `slice::iter_mut` → `no method 'iter' found`. Needs the
`Iterator`/`IntoIterator` machinery wired for slices.

### 1e. Simple, unimplemented

| Method | Symptom |
|---|---|
| `slice::copy_from_slice` | undefined `__glyim_fn_9092` |  **← fixed**
| `slice::split_at` | undefined `__glyim_fn_9164` (returns a tuple of two fat pointers) |  **← fixed**
| `String::from_utf8` | `ambiguous method 'len' found in multiple impls` (typeck, on the `Result` it returns) |

### 1f. `Vec::pop` — inference ordering (category b)

`v.pop()` alone compiles and runs. `v.pop().unwrap()` / `v.pop().is_some()`
ICE with `fn_abi_of failed: UnknownType`, because the `Option<T>` payload
stays an unresolved `Infer` when the result feeds another builtin. This is a
typeck/monomorphization ordering issue, not codegen — the same class as the
`MaybeUninit::<T>::new` bug fixed earlier (assoc-fn substs built empty).

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
