# HANDOFF — session 15 (follow-ups to session 14)

## 0. TL;DR

All four "orthogonal pre-existing bugs" documented in
`HANDOFF_SESSION_14_FINAL.md` §2 have been fixed.

- **Last green commit**: `b9bbff44` — *"fix(codegen,interp):
  pointer-value comparison in binops"*. All **4599/4599** tests pass;
  corpus green; 2 skipped.
- **Working tree clean.**
- **The session is 54 commits ahead of `origin/main`.**

Closed this session (oldest -> newest):

| Commit | Handoff § | What |
|--------|-----------|------|
| `533122fc` | §2.1 + §2.4 | Structural LLVM type for non-enum `Adt` (fixes struct-typed const/static init) + interpreter `StaticPtr` deeper-projection writes |
| `9f410cfa` | §2.2 | `ConstValue::Fn` const-evaluator variant folds `fn`-item path initializers (`const F: fn(..) = some_fn;`) |
| `b9bbff44` | §2.3 | Pointer-value comparison (`Eq`/`Ne`/`Lt`/`Gt`/`Le`/`Ge`) in LLVM binop + interpreter binop |

## 1. What each fix did

### §2.1 — structural LLVM `Adt` type (`glyim-codegen-llvm/src/types.rs`)

`llvm_type_for_ty`'s `TyKind::Adt` arm returned the *opaque-sized* form
(`{[N x align_ty], [r x i8]}` — exactly 1 LLVM field). `lower_const`'s
`build_insert_value(cur, val, i)` for field >= 1 then hit
`ExtractOutOfRange`. Mirrors the earlier tuple fix: for a non-enum ADT
whose field types can all be lowered, produce a *structural* LLVM
struct. Enum ADTs keep the opaque form (tag-plus-variant-payload).
Generic structs substitute their field types via `TyCtxMut::subst_ty`.

### §2.4 — interpreter static field writes (`glyim-mir-interp/src/lib.rs`)

`write_place` panicked on `StaticPtr` with `proj_count > 1`. The write
now loads the current global value, applies the remaining projections
via the free helper `write_projection_into_value` (supports `Field` on
`Aggregate`; `Deref` and others reject clearly), and stores it back.

### §2.2 — `ConstValue::Fn` (`glyim-const-eval`, `glyim-typeck`,
`glyim-pipeline`, `glyim-lower`)

- `ConstValue::Fn(FnDefId)` added to `value.rs`.
- `ConstEvaluator` gains `fn_defs: HashMap<Name, FnDefId>` +
  `with_fn_def` builder; `Expr::Path` tries `fn_defs` before the
  local-scope `lookup`.
- Typeck threads every top-level `fn`'s name → `FnDefId` (via the
  def-map value namespace) into the evaluator at both const-eval call
  sites.
- `cv_const` maps `ConstValue::Fn` to `MirConstKind::Fn(id, [])`.
- `mono::scan_const`'s `StaticRef` arm recurses into `init` so the
  inner `Fn`'s body is enqueued (LLVM was otherwise emitting a global
  that referenced an undefined `__glyim_fn_<id>`).

### §2.3 — pointer comparison (`glyim-codegen-llvm`, `glyim-mir-interp`)

LLVM: `lower_binop`'s `Eq`/`Ne`/`Lt`/`Gt`/`Le`/`Ge` arms gain a
pointer-pointer branch that ptr-to-int casts both operands and emits
the int compare. Interpreter: `eval_binary_op` gains a
`(Ref|StaticPtr, Ref|StaticPtr)` arm encoding each operand as an
integer address (same scheme as `PtrToInt`).

## 2. Session-14 handoff §2 addendum: `&T -> *const T` (CLOSED)

**Status: fixed in `cdc07f43`.**

The original text of this section (below) described a gap discovered
while diagnosing §2.3. The typeck coercion arm was added in
`cdc07f43`; the interpreter also needed `(Ref|StaticPtr, Int|Uint)`
comparison arms for `(p as usize) != 0` when the intermediate cast
result is elided.

### 2.1 `&T -> *const T` was not an implicit coercion (ORIGINAL TEXT)

```g
let p: *const i32 = &x;
// -> [T0001] mismatched types: &?int349 vs *const i32
```

Found while diagnosing §2.3. Rust coerces `&T` to `*const T`
implicitly; glyim requires an explicit `as *const T` today. The
`as`-form works (see `ptr_eq_same.g` fixture). Fix scope: add
`Ref -> RawPtr` to the coercion predicates in
`glyim-solve/src/solver.rs::can_coerce` and `fulfill.rs` (the same
predicates that already handle `FnDef -> FnPtr`), and ensure the
unify arm for `(Ref, RawPtr)` consults them. Not exercised by any
fixture yet.

## 3. Verification workflow

Run `cargo build -p glyim-cli` first. Then the full suite (slow
tests include `glyim-test::run_pass_corpus`):

```
cargo nextest run --workspace
```

Corpus only:

```
cargo nextest run -p glyim-test --test run_pass_corpus
```

One fixture, bypass the CLI cache:

```
GLYIM_CACHE_DIR=$(mktemp -d) ./target/debug/glyim-cli --with-stdlib \
  --emit=exec crates/glyim-test/tests/run-pass/behavior/<name>.g \
  -o /tmp/f.out && /tmp/f.out; echo $?
```

## 4. Env / process notes (new from this session)

- **`git log` opens `less` by default.** Always `export
  GIT_PAGER=cat PAGER=cat` in scripts. Session 15 lost ~15 minutes to
  a terminal-full of `less` help text.
- **Inline backticks in `-m` commit messages get shell-expanded.** A
  message with `p1 == p2` turned into an empty string and shell
  warnings. Write the message to a temp file and use
  `git commit -F <file>`.
- **Fork exhaustion is real.** `cargo nextest run --workspace` at
  default parallelism occasionally failed with
  `Resource temporarily unavailable (os error 35)` after back-to-back
  suite runs. Killing stray processes and retrying (or
  `--test-threads=4` once) cleared it. It is *not* a test failure.
- **Heredoc truncation** persists: the first write of this handoff
  stopped at line 70 (right at a nested code fence). Chunk with
  unique delimiters + `wc -l` verification after each chunk.

## 5. Fixtures added this session (8 total)

- Struct-typed const/static (3): `struct_const_read.g`,
  `struct_static_read.g`, `struct_static_mut_write_field.g`
- Fn-pointer-valued const/static (3): `fn_const_read.g`,
  `fn_static_read.g`, `fn_const_as_arg.g`
- Pointer comparison (2): `ptr_eq_same.g`, `ptr_ne_different.g`

Plus the session-14 fixtures still green: `fn_ptr_assignment.g`,
`fn_ptr_apply.g`, `fn_ptr_generic.g`, `static_read.g`,
`static_used_in_expr.g`, `static_alongside_const.g`,
`static_ref_deref.g`, `static_mut_write.g`, `static_mut_read.g`,
`static_ref_stable_address.g`.

## 6. Post-handoff work (session 15 continuation)

Two more items closed after the §2 addendum:

### 6.1 `StaticPtr` projections beyond `Field` — `578ed645`

`write_projection_into_value` handled only `Field`; indexing into a
static array (`A[1] = v` / `A[i] = v`) panicked on the interpreter
with "unsupported projection in static deeper-write path" even
though the same program ran correctly through the LLVM backend
(which lowers the store as a byte-GEP).

Added:

- `ConstantIndex { offset, from_end }` — fixed array/tuple offset;
  `from_end = true` counts from the tail.
- `Index(local)` — dynamic index. The index value lives in a local
  of the *current* frame, so the helper takes a
  `&[Option<InterpValue>]` snapshot of that frame. The snapshot is
  taken in `write_place_frame` *before* the `frame_locals` mutable
  borrow ends (touching `self.globals` while `frame_locals` was live
  was a borrow conflict).
- `Downcast(_)` — no-op at the value level (the tag lives in field 0
  of the aggregate); recurse through.

Fixtures (5): tuple-field write, nested-struct field write,
array-of-struct field write (ConstantIndex + Field),
`&mut STATIC.field` deref-then-field write, dynamic-index array
write. LLVM + interpreter both green.

### 6.2 Non-const-evaluable static initializers — `ae449060`

Handoff §2 said "MonoItem::Static with a real MIR body". On
investigation that was based on a false premise:

- `static X: i32 = compute();` is **invalid Rust** (E0015, "cannot
  call non-const fn in statics"). Runtime-initialized statics are a
  C++ feature, not Rust.
- What was actually broken in glyim was a *silent miscompile*:
  typeck's const-eval error was swallowed
  (`if let Ok(value) = ...`), `lower_rvalue` warned and emitted
  `MirConstKind::Error`, and codegen turned that into a zero
  constant. The program compiled cleanly and returned `0` instead
  of failing.
- The `const` arm already propagated evaluator errors as `[C0001]`
  diagnostics; the static arm didn't.

Closed by making the static arm mirror the const arm:

- `glyim-typeck`'s `ItemKind::Static` arm matches on the evaluator
  result and pushes `e.into_diagnostic()` on failure.
- `glyim-lower`'s `StaticRef` arm emits a loud internal error if it
  reaches lowering without a folded initializer (belt and braces:
  typeck should now reject the program first).
- UI fixture `static_non_const_init.g` / `.expected` asserting
  `[C0001]`.

Full C++-style runtime init (MonoItem::Static body,
`__glyim_static_init_<id>` prologue in `main`, interpreter-side
init before `run_body`) remains a possible future language feature,
deliberately out of scope here.

### 6.3 Where things stand

- **4599/4599** tests green; corpus green; 2 skipped.
- `ae449060` is the current tip.
