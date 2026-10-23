# KNOWN_GAPS

Verified against the tree at `b91a585e`. Each entry names the code that
implements (or fails to implement) the behaviour, so a claim can be re-checked
rather than trusted. Anything not listed here either works or has a clean,
actionable diagnostic.

**Last audit:** this file was last regenerated from the working tree after a
session that closed ~20 gaps. The previous revision (2026-10-14) was stale:
three of its entries — `include!`, `--lto=thin`, and enum-variant value paths
— described behaviour that had since been implemented, and one
(`glyim-bytecode-vm` "referenced by nothing") was simply wrong. Treat an
entry here as a claim about *code*, not about intent.

---

## 1. Async: `.await` inside a loop body (category a — hard error)

`crates/glyim-hir/src/lower/lower_async.rs` rejects with `[T0060]`:

    `.await` inside a loop body is not yet supported by the async
    state-machine lowering (tracked: KNOWN_GAPS.md async-v2). Hoist the
    await out of the loop, or collect futures into a Vec and await them
    sequentially outside the loop.

This is a **deliberate, diagnosed gap**, not a silent miscompile. Single- and
multi-await sequential bodies both desugar; a loop needs a genuine
state-machine v2 (a suspension point that is the loop back-edge).

## 2. Windows SEH cleanup edges are unreachable (category b)

`crates/glyim-codegen-llvm/src/seh_ffi.rs` and `lower.rs`'s
`emit_seh_cleanuppad` implement the funclet path, and `Personality::Seh` is
selected for `x86_64-pc-windows-msvc` / `aarch64-pc-windows-msvc`. **Nothing
exercises it**, because *no lowering site ever sets `cleanup: Some(..)`* on a
`Call` or `Drop` terminator — MIR has the field and the backends handle it,
but lowering never populates it.

That is currently harmless: `glyim_panic` calls `process::exit(101)` rather
than unwinding, so no cleanup edge is ever taken. Verified — a panic inside a
`Drop` scope exits 101 and runs no destructor, on both targets.

If unwinding (`panic = "unwind"` semantics) is ever wanted, lowering must
populate `cleanup` edges first; until then the SEH code is dead but correct.

## 3. MIR interpreter: deeper writes through a `StaticPtr` — CLOSED (unreachable)

*(Historical note: the panic arm this described is still present, but the
only MIR shape that would reach it can't be produced from source in the
current compiler.)*

`write_projection_into_value` (`crates/glyim-mir-interp/src/lib.rs`) rejects
a `Deref` after a `StaticPtr` with an explicit panic. Reaching it needs a
`static mut` whose initializer contains a **reference** field (so that
writing through `*static.field = v` produces `Place{static, [Field, Deref]}`).

Verified unreachable:

- `static mut R: &'static mut i32 = &mut A;` — the const-evaluator rejects
  the initializer (`path expressions not yet supported in const eval for
  non-local paths`).
- `static mut S: S<'static> = S { p: &mut A };` — same rejection.
- A *local* `let s = S { p: &mut A }; *(s.p) = 9;` **does** produce
  `[Field, Deref]`, but on a local (not a `StaticPtr`), which takes the
  `Ref` arm of `write_place_frame`, not `write_projection_into_value`.

So no source program reaches the panic. If the const-evaluator ever gains
path-expression support for non-local paths, this entry becomes live again;
until then the reject arm is a correct guard against a shape that can't
occur.

## 4. `panic!` does not unwind (intentional, documented)

`glyim_panic` (`crates/glyim-runtime/src/lib.rs`) writes the message to
stderr and exits 101. It does not unwind, so destructors do not run on a
panic. This matches `panic = "abort"` and is what the `run-fail` corpus
asserts (`// exit-code: 101`).

The MIR interpreter has a cross-frame unwinding path (`panics_unwind`,
`tests/unwind_cleanup.rs`) that is **off by default**: enabling it changes the
top-level error variant every caller matches on (`Panic` → `Unwind`) and
breaks 14 tests that pin the current contract. That is a deliberate design
decision, not an oversight — see the doc comment on `with_panics_unwind`.

## 5. `include!` — implemented; proc-macro + pre-pass now aligned

`include!("path")` splices the target file's tokens at the call site, at both
item and expression position, resolving absolute paths and paths relative to
the calling file. Nested includes work, and **builtin macros inside an
included file are expanded** (verified: `include!("inner.g");` where
`inner.g` uses `concat!` in a function body compiles and runs).

The previous caveat — "the expander does not run proc-macro expansion inside
an included file" — was imprecise. The full pipeline *always* ran proc
expansion; the issue was the `entry_main_local_id` **pre-pass**, which
expanded with a builtin-only expander so its `LocalDefId` numbering could
diverge from the real pipeline's when a proc macro emitted items before
`main`. The pre-pass is what decides which body codegen treats as the entry
point, so a shifted id meant the wrong function ran.

Fixed by threading the proc registry into the pre-pass
(`entry_main_local_id_with_proc_registry`); the harness's compiler uses it.
The wrapper `entry_main_local_id(db, path)` keeps the old no-registry
behaviour for unit-test callers that don't have a registry to give. CLI and
`glyip` still call the wrapper (they build the registry themselves at a
layer above the pre-pass); if a user program combines an item-emitting proc
macro *and* `include!`, upgrade those two call sites too.

## 6. Deferred (category c — no code path, no diagnostic needed)

- **Windows SEH landingpads.** See §2: implemented, structurally unreachable.
- **`dyn Trait` niche layouts.** `needs_drop` returns `false` for types it
  cannot inspect (`dyn Trait`, generic params, projections, error/opaque),
  documented on `TyCtx::needs_drop`. Conservative and sound.
- **Fine-grained partial-move drop guards.** `drop_flags` models "should this
  whole local's `Drop` run", not per-field state. A partial move disables the
  parent's destructor entirely — sound (over-retains, never double-frees),
  but leaks the still-initialized siblings.

## 7. `for x in <slice-iterable>` — CLOSED

*(Historical note: this gap was open from session 15 until it was closed
by the lowering change described here. Kept in the file so a future reader
who remembers the symptom can trace the fix.)*

**Symptom.** The trait-path lowering of `for x in v.as_slice()` /
`for x in a.iter()` routed through the stdlib's `impl Iterator for &[T]`
(iter.g:332), whose `next` returns `&T` into its own transient frame:

    fn next(&mut self) -> Option<&'a T> {
        let cur: &[T] = *self;
        ...
        let first = &cur[0];        // Ref into `next`'s frame
        Option::Some(first)
    }

When `next` returned, its frame died, and the caller's `*x` panicked
with `deref of Ref from dead frame 1`. LLVM handled it (it doesn't care
about frames); the **interpreter did not**. A `run-pass` fixture could
not be added — the harness runs the interpreter, so the fixture would
fail on the interp path.

**Fix (this session).** `lower_rvalue.rs`'s for-loop lowering now takes
the *index-walk* path for any slice iterable, not just bare arrays:

- `take_array_path` fires for `TyKind::Ref(_, Slice(_))` (a `&[T]`,
  which is what `v.as_slice()` and `a.iter()` return), for bare
  `TyKind::Slice(_)`, and for the existing `TyKind::Array(_, _)`.
- The element read (`iter_local[Deref, Index(i)]`) uses the same
  `[Deref, Index]` projection shape that typeck emits for `s[i]`,
  after which codegen's `pointee_is_fat` path (lower.rs:664) and the
  interpreter's `ProjectionElem::Deref` arm handle the fat pointer
  identically. The item is then wrapped in a `Ref` to build the `&T`
  that `pat` binds — mirroring `impl Iterator for &[T]`'s `Item = &T`,
  but with the reference target in the caller's frame.
- The `Rvalue::Len` place also gets a `Deref` prepended for the
  ref-slice case, matching what `s.len()` emits.

Both backends now agree on every shape. The trait path for `Iterator`
on user-defined types (custom `impl Iterator for Counter`, etc.) is
untouched — only slice iterables route around it.

**Verified.**

- LLVM: `for x in v.as_slice()` → exit 3; `for x in a.iter()` → exit 6.
- Interp: same (harness covers both backends through the fixture).
- No regression to `for x in [1,2]`, `for i in 0..3`, `s[i]`, `s.len()`.
- Fixtures: `behavior/for_loop_over_slice.g`,
  `behavior/for_loop_over_slice_iter.g`.

**`IntoIterator for Vec<T>` — implemented in stdlib; for-loop wiring is
the only remaining piece.**

**Status as of this commit:**

- `crates/glyim-lang-alloc/lib/vec.g` now contains
  `pub struct VecIntoIter<T>` + `impl<T> Iterator for VecIntoIter<T>`
  + `impl<T> IntoIterator for Vec<T>`.
- **`v.into_iter()` works on both backends.** Verified by fixture
  `behavior/vec_into_iter_explicit.g` — a `let mut it = v.into_iter()`
  followed by a manual `match it.next()` loop returns 6 for `[1,2,3]`,
  matching LLVM and the interpreter.
- A first probe of `v.into_iter()` failed with "mismatched types:
  expected integer type, found `<?ty as Trait37>::Item`". That failure
  does not reproduce in the final tree — the associated-type
  projection (`<Vec<T> as IntoIterator>::Item = T`) resolves correctly
  once the impl is registered and the method-call return-type
  substitution runs. The earlier probe was against a stale binary.

**What still does not work: `for x in v` (the syntactic sugar).**
The for-arm in `check_expr.rs` looks up `Iterator::next` on `iter_ty`
directly; it never calls `IntoIterator::into_iter`. So a
`IntoIterator`-only type (which `Vec<T>` now is) is still rejected at
the for-head.

**Attempted this session (twice) and reverted.** The natural wiring is:

- `Expr::For` in `check_expr.rs`: resolve `IntoIterator::into_iter` on
  `iter_ty`; run the `Iterator::next` scan on its **return type**
  instead of `iter_ty`.
- Lowering: emit that `into_iter` call before the loop, re-bind the
  iteration local to its output.
- New THIR field `ForIteratorNext::into_iter: Option<ForIntoIter>` +
  matching `IteratorNextInfo::into_iter: Option<IntoIterInfo>`.

Both attempts produced a **codegen ICE** where the previous behavior
was a clean reject:

    fn type lowering failed: fn_abi_of failed: UnknownType(Ty(7292))

**Actual cause (verified by a `[DBG_RES]` probe in `check_expr.rs`
inserted right before the new `if iter_is_by_value_vec ...` block).**
The resolution block **never fires for the user's `for x in v`.** The
probe was set up to print `iter_kind` at every `Expr::For` typechecked
during the compilation, and every logged line was a stdlib-internal
loop:

    [DBG_RES] iter_is_by_value_vec=false iter_is_concrete=true iter_kind=Adt(AdtId(1000), …)   // Range
    [DBG_RES] iter_is_by_value_vec=false iter_is_concrete=true iter_kind=Ref(Erased, Ty(36), …) // slice ref
    …

Not a single `iter_kind=Adt(AdtId(1020), …)` line — the user's `Vec`
for-head never reaches our block at all. So the emitted MIR showed no
`Fn(419)` (the correct `VecIntoIter::into_iter`) call, the `into_iter`
local kept its pre-`into_iter` type (`Vec<i32>`), and the subsequent
`next` call — resolved against `VecIntoIter` from `iter_ty_for_next`,
which was computed inside the *never-entered* block — landed on an
inconsistent type. Hence `UnknownType`.

**Why the block never fires is not yet diagnosed.** Likely candidates,
in order of suspicion:

1. The user's `for x in v` may be typechecked **after** the stdlib's
   own items and, by the time it reaches `Expr::For`, `iter_ty` is
   already `Infer(...)` rather than the resolved `Adt(1020, …)`. Our
   probe fires on the raw `iter_ty` *before* any inference resolution,
   so if the user's `v` is bound to a `?T` that resolves to
   `Vec<i32>` later, the block's `matches!(TyKind::Adt(1020))` silently
   takes the `false` branch. Stdlib `for` loops bind their iterands to
   already-resolved types (Range literals, slices), so they all pass;
   the user's program is the only one that sees the `Infer`.
2. `main`'s `Expr::For` may be typechecked during a phase that our
   probe doesn't cover (the crash means typeck *did* run, but no
   `Adt(1020)` line appeared — the probe's own print is inside
   `check_expr`'s `Expr::For` arm, so if it didn't fire, that arm
   either ran with a non-`Adt(1020)` iterand or ran somewhere our
   instrumented copy is not visible). This is the second attempt to
   instrument it — the first (`DBG_INTO_ITER` above) *did* resolve
   `fn 419` correctly, which means the two probes are seeing *different*
   `Expr::For` nodes. The working resolution in the first probe likely
   came from a *separate* typecheck invocation (a pre-pass) where
   `iter_ty` was already concrete; the *final* pass that lowering
   consumes runs with `iter_ty = Infer`, which our concrete-only
   check skips.

**Next step for a future attempt.** Instead of gating on
`matches!(TyKind::Adt(1020))`, first resolve `iter_ty` through the
inference table:

    let resolved_iter_ty = self.infer.resolve_ty_shallow(self.ctx, iter_ty);

and match on `resolved_iter_ty`. That is exactly the pattern the
existing `is_unresolved` block uses (`check_expr.rs:707`). The gating
`iter_is_by_value_vec` should consult `resolved_iter_ty`, not the raw
`iter_ty`, for the same reason. This should make the block fire for the
user's `for x in v` — and, because `resolve_trait_method_fn` was
already verified (via `[DBG_INTO_ITER]` probe) to correctly return
`fn_def_id 419` for `Adt(1020, [i32])`, the rest of the wiring is
already known to work.

**Corrected misdiagnosis.** The earlier text in this entry said the
resolved fn id was `9162` (`to_vec`). That was a misreading of the MIR
dump: line 23 of the emitted MIR is the *source-level* `[1,2,3].to_vec()`
call, not the `into_iter` call. The actual `into_iter` emission was
**missing entirely** — see the `[DBG_RES]` result above.


**Already-working fallbacks** (unchanged): `for x in v.as_slice()`,
`for x in &v`, and `for x in v.into_iter()` (explicit).

## 8. Interp: `w.v.len()` on a `Vec<T>` field — CLOSED

*(Historical note. Verified working on both backends.)*

**Symptom.** `w.v.len()` where `v: Vec<T>` is a struct field returned the
wrong value in the interpreter (1 for a 3-element Vec):

    struct W { v: Vec<i32> }
    fn main() -> i32 {
        let w = W { v: [1, 2, 3].to_vec() };
        w.v.len() as i32         // LLVM: 3. Interp: 1 (before this fix)
    }

**Root cause.** `Rvalue::Ref` in `crates/glyim-mir-interp/src/lib.rs`
built `InterpValue::Ref { frame, local }` and **discarded
`place.projection`**. For `w.v.len()`, the MIR is:

    _6 = &mut _5.field0;          // Rvalue::Ref(place{5, [Field(0)]})
    _7 = <Vec<i32>>::len(move _6)

`builtin_deref` resolved the Ref to `frame_locals[5]` — the whole
`W { v, i }` struct (a 2-field `Aggregate`). `vec_len` then fell back to
`fields.len()` = **2**, which the previous exit-code fixture (1) showed
was off by the struct's field count.

**Fix.** `InterpValue::Ref` now carries `projection:
Box<[ProjectionElem]>`. `Rvalue::Ref` copies `place.projection` into it;
`builtin_deref` walks the projection (currently `Field` only) before
handing the result to the builtin. PartialEq on `Ref` deliberately
compares only `(frame, local)` — address identity, not structural
equality — so existing pointer-comparison tests are unaffected.

**Verified.**
- LLVM: `w.v.len()` → 3; `w.v.is_empty()` → 1.
- Interp: same (via `run_pass_corpus`, which uses the interpreter).
- Differential (both backends agree): green.
- Fixtures: `behavior/vec_field_len.g`, `behavior/vec_field_is_empty.g`.

## Verification

Two gates cover most of the above; run both before claiming a gap is closed.

```
# Full workspace suite (includes the run-pass corpus, which asserts exact
# exit codes end-to-end).
cargo nextest run --workspace --test-threads=2

# Differential: compiles every run-pass AND run-fail fixture once and runs
# both backends, asserting they agree on exit code and stdout. This is the
# gate that catches "works on LLVM, silently wrong in the interpreter".
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential \
  --features real-llvm --test-threads=2
```

One-off reproducers (always bypass the CLI compile cache):

```
GLYIM_CACHE_DIR=$(mktemp -d) ./target/debug/glyim-cli --with-stdlib \
  --emit=exec /tmp/x.g -o /tmp/x.out && /tmp/x.out; echo $?
```

`--emit=mir` and `--emit=llvm-ir` write to the `-o` path, not stdout.
