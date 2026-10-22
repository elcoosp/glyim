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

**Not affected — `IntoIterator for Vec<T>` (§3.4, updated).** The
by-value `for x in v` still needs a real `IntoIterator for Vec<T>`
yielding `T` by move, plus a `VecIter<T>` adapter, plus typeck
resolving `IntoIterator::into_iter` before `Iterator::next` (the for-arm
currently looks up `next` directly on `iter_ty`). Until that lands,
`for x in v` rejects with an `as_slice()` hint — an honest diagnostic
replacing what was (before commit `f4f97c2f`) a **silent miscompile**.

**Blocker for the stdlib implementation — one real bug.**

While prototyping `VecIter<T>` (the obvious stdlib implementation of
`IntoIterator for Vec<T>`), found two independent bugs that would each
break the adapter on its own:

**Monomorphization leaves a type parameter in a generic struct field.**

    struct Wrap<T> { v: Vec<T>, i: usize }
    fn main() -> i32 {
        let w = Wrap { v: [1, 2, 3].to_vec(), i: 1 };
        w.v[w.i]
    }
    // -> [X0000] TyKind::Param(ParamTy { index: 0, name: Name(359) })
    //    reached LLVM codegen -- monomorphization should have resolved this

So the IC E reported earlier in this entry is a symptom, not the root
cause: `place_ptr` (or a caller) sees `Vec<T>` with `T` still a
`Param(0)`, and either handles it incorrectly or fails at the
`build_load` boundary. `Wrap<i32>` used directly from `main` should
have been monomorphized to `Wrap` at `T = i32`, but the codegen backend
received the *generic* field type. Same class as the
`Place::ty(): Field projection on non-tuple/ADT type` stderr noise
(cosmetic but same root cause). Fix is likely in `mono.rs`'s
`MonoItem` collection or the caller's `Substitution` construction for
generic struct literals.

## 8. Interp: `w.v.len()` on a `Vec<T>` field (Ref drops Field projection)

**Status:** verified. LLVM is correct; the interpreter returns the wrong
value.

    struct W { v: Vec<i32> }
    fn main() -> i32 {
        let w = W { v: [1, 2, 3].to_vec() };
        w.v.len() as i32         // LLVM: 3. Interp: 1.
    }

**Root cause.** `Rvalue::Ref` in `crates/glyim-mir-interp/src/lib.rs`
(line ~1851) builds `InterpValue::Ref { frame, local }` and **discards
`place.projection`**. For `w.v.len()`, the MIR is:

    _6 = &mut _5.field0;          // Rvalue::Ref(place{5, [Field(0)]})
    _7 = <Vec<i32>>::len(move _6)

The Ref reaches `builtin_deref`, which clones `frame_locals[5]` — the
whole `W { v: ... }` struct (a 1-field `Aggregate`). `vec_len` falls
back to `fields.len()` = **1**, hence the wrong answer.

LLVM was similarly wrong before this session's `e50d03df` (it passed
`&mut Vec<T>` to an arm that only accepted by-value `{[3 x i64]}` and
fell through to an undefined symbol). That is fixed. The interp's
`Ref`-drops-projection is separate and **pre-dates** this session — the
new fixture just exposes it for the first time.

**Scope of the fix.** 24 uses of `InterpValue::Ref` in
`crates/glyim-mir-interp/src/lib.rs`. The proper fix extends the enum
variant to carry `projection: Box<[ProjectionElem]>`, wires it through
`Rvalue::Ref`, and applies it in `builtin_deref` /
`read_place` / `write_place_frame` / `mutate_receiver` / pointer
comparison (each currently peels only the first projection step).
Mechanical but wide; deserves its own session.

**Not in this session's scope.** The e50d03df codegen fix and the two
fixtures it was written against (`vec_field_len.g`, `vec_field_is_empty.g`)
are *reverted* — the fixtures would fail the interp gate until this
gap is closed, and there is no way to mark "LLVM-only" in the run-pass
corpus today.

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
