# FIXES — this tree vs. `origin/main`

Every entry names the code that changed and the test that proves it. The
full workspace suite (`cargo test --workspace`) is green on this tree, and
the LLVM-vs-interpreter differential gate
(`GLYIM_TEST_REAL_LLVM=1 cargo test -p glyim-test --features
glyim-test/real-llvm --test differential`) agrees on exit code and stdout
for every comparable run-pass/run-fail fixture.

## 1. Type arena leaked per compilation (`glyim-type`) — OOM kills

`TyCtxMut::new` created the canonical `TypeArena` with `Box::leak` and
`TyCtx.arena` held it as `&'static`. Every compilation in a process
therefore retained its entire interned type/substitution/flag tables —
measured at ~44 MB per stdlib compile, never freed. The run-pass corpus
(283 fixtures compiled in one process) was killed by the OOM killer part
way through; the interpreter grew ~210 MB per fixture.

Fix (`ty_ctx.rs`, `ty_ctx_mut.rs`, `type_arena.rs`): the arena is now
`Arc<TypeArena>`; `freeze` / `freeze_owned` / `from_ty_ctx` / `to_mut`
clone the handle, and a `Drop` impl frees the three `Box::into_raw`
storages exactly once. Handle validity across views is unchanged (Arc
keeps the arena alive while any view exists). Measured after the fix:
growth plateaus (allocator caching), corpus completes.

## 2. §3.9 interpreter half: slice-iterator refs into dead frames (`glyim-mir-interp`)

`behavior/slice_iter_next.g` panicked under the interpreter
("deref of Ref from dead frame 1", exit 101) while LLVM returned 10 —
five sessions of KNOWN_GAPS §3.9. Live-traced root causes, all fixed:

- **Returned refs pointed at the dying callee frame.** The stdlib
  `impl Iterator for &[T]` binds `let cur: &[T] = *self;` and returns
  `&cur[0]` — under the value model that is a `Ref` anchored to the
  callee frame. `Interpreter` now *archives* a popped frame when its
  return value still references it (`dead_frames`, bounded at 256);
  `locals_for_ref_frame` / `decls_for_ref_frame` read the archive.
  Writes through dead-frame refs still fail (no live alias).
- **Derefs dropped the `Ref`'s own projection.** `read_place`'s `Deref`
  arm and `eval_unary_op`'s `UnOp::Deref` returned the whole pointee, so
  `*x` yielded the entire slice instead of element 0. `read_place` now
  splices the ref's projection in front of the reader's remaining steps
  (with per-step frame tracking for `Index` operands, which live in the
  ref's origin frame — possibly an archived dead frame);
  `eval_unary_op` routes through a new `deref_ref_value` helper that
  replays `Deref`/`Field`/`Index`.
- Kept the `slice_length_from_value` array-vs-fat-pointer shape
  discriminator and removed its unconditional `[DBG-LEN]` stderr spam;
  collapsed the 2–4× duplicated `GLYIM_INTERP_TRACE` blocks the WIP
  commit left behind (also in `differential.rs` and
  `interpreter_runner.rs`).

## 3. Linker driver emitted gcc-hostile flags (`glyim-cli`)

- `linker_flags_for_target` passed `-target <triple>` to `cc` on every
  host. GNU gcc rejects `-target`, so every `--emit=exec --target=<host>`
  failed on gcc-only Linux boxes ("unrecognized command-line option
  '-target'"). The flag computation is now split into a pure
  `linker_flags_for_driver` (clang: `-target`; gcc: no flags for a
  host-matching triple, a clear actionable error for foreign triples) plus
  `linker_flags_for_raw_ld`, with a `--version`-probed clang/gcc
  classifier at runtime. Raw GNU `ld` now refuses Mach-O targets with a
  clear message instead of a nonsense emulation flag.
- **PIE**: the backend emits non-PIC code, but modern distro toolchains
  default to PIE — every `--emit=exec` link died with
  "relocation R_X86_64_32 against '.rodata' can not be used when making a
  PIE object". `link_with_args` now passes `-no-pie` for ELF targets with
  cc-style drivers (skipped for `-shared` cdylib links).
- `cache_integration::cache_hit_for_exec_produces_a_runnable_binary`
  hardcoded `--target aarch64-apple-darwin`, which can only ever link on
  a macOS host; it now exercises the same contract against the host
  triple and actually runs the cached binary on every host.

## 4. Invariant guards vanished in release (`glyim-span`, `glyim-runtime`, `glyim-typeck`)

Four `#[should_panic]` tests assert guards written as `debug_assert!`,
which are compiled out under `cargo test --release`:

- `Span::new` lo > hi (`glyim-span`)
- `slice_from_raw_parts` / `_mut` length > `isize::MAX` — the one gate
  between a codegen length bug and a wrapped `from_raw_parts` (UB)
  (`glyim-runtime::fs`)
- `LocalEnv::leave_scope` without a matching `enter_scope`
  (`glyim-typeck::env`)

All four are now unconditional `assert!`s — they are invariant/safety
guards on non-hot paths, and the loud failure matches the workspace's
correctness-first philosophy.

## 5. Incremental-build cache skipped same-size edits (`glyip`)

`FingerprintStore::has_changed` trusted a `(size, mtime)` metadata fast
path. Two same-size writes landing inside one filesystem timestamp tick
(the normal edit-and-rebuild cadence, milliseconds apart) produce
identical metadata with different content, so a real edit was silently
treated as a cache hit — `fingerprint_store_has_changed_detects_
modification` fails on exactly this shape. `has_changed` now always
performs the authoritative content hash; the dead
`metadata_matches_path` helper is removed. (No-op-build cost: hashing a
few MB of source — milliseconds.)

## 6. M5 async fixtures contradicted the harness contract (`glyim-test`)

`runtime/m5/one_step.g` / `two_step.g` declared `check-stdout: 42/3`
while their `main` *returns* the awaited value and never prints — no
backend could ever satisfy them (`RunFailed { exit_code: Some(42),
expected_exit_code: Some(0) }`). They now declare the contract they
actually have (`exit-code: 42` / `exit-code: 3`, the same convention as
`run-pass/baseline_exit_42.g`), with the doc comments updated
(`tests/runtime.rs` prose adjusted accordingly).

## 7. Docs

- `KNOWN_GAPS.md` §3.9: added the session-34 resolution note (interp
  half CLOSED, root causes named), and collapsed the triplicated
  session-33 wrap blocks.

## 8. Array→slice unsizing coercion was missing at every backend boundary (`glyim-codegen-llvm`, `glyim-mir`, `glyim-lower`, `glyim-mir-interp`)

The type checker accepts `&[T; N]` where `&[T]` is expected (Script 359
unification), but nothing materialized the unsize coercion. A thin
array pointer routed through an intermediate local reached every
fat-slice destination unconverted — a silent miscompile on LLVM and a
wrong-value bug on the interpreter. Live-traced root causes, all fixed:

- **LLVM call ABI** (`lower.rs::lower_call`): the argument operand
  keeps its own `&[T; N]` type while the ABI slot is the callee's fat
  `&[T]` formal; the thin pointer misaligned every following argument
  and `fn take(x: &[i32]) -> i32 { x.len() }` on `take(&arr)` returned
  register garbage (72). New `coerce_unsized_slice_ref` materializes
  `{ ptr, len }` at the call boundary (mut refs included).
- **LLVM aggregate fields** (`lower.rs` Aggregate/Tuple arms): a field
  declared `&[T]` receiving `&[T; N]` stored the thin pointer into the
  16-byte slot and `s.buf.len()` read adjacent struct bytes (0 / 64 /
  16). New `coerce_aggregate_field_unsizing` keys on the *declared*
  field types — flat `AdtDef.fields` for structs (the same source
  `FullLayoutComputer` uses; enums keep `variants[..].fields`),
  substitution args for tuples — with the ADT's own generic params
  substituted so `Wrapper<i32> { buf: &arr }` works.
- **MIR `Place::ty` / `Place::ty_mut`** (`glyim-mir`): `Index`,
  `ConstantIndex` and `Subslice` over a `Ref`-typed place (indexing the
  pointee of a fat slot) logged "Index/… on non-array/slice type" and
  returned `Error`; they now peel one reference before classifying.
- **MIR builder Index auto-deref** (`glyim-lower`): the decision used
  the base *local's* declared type, so `b.data[i]` — a slice field read
  through a struct — emitted `[Field, Index]` with no `Deref` and
  codegen ICEd with "Index projection on non-array/slice type". It now
  decides on the type *at the place* (`Place::ty` walk), matching what
  `s[i]` on a `&[T]` local already did.
- **Interpreter `operand_ty`** (`glyim-mir-interp`): ignored place
  projections — a receiver operand `Copy(Place{s, [Field(0)]})`
  reported the STRUCT as the operand type, so the slice `len`
  builtin's `is_fat_ptr` classification missed and read the fat
  aggregate's field count (2) as the length. It now walks the place
  through `Place::ty`.
- **Interpreter value boundaries** (`glyim-mir-interp`): call-argument
  binding coerces thin `Ref` → `Aggregate([data_ref, len])` against the
  callee formal; aggregate field construction coerces against declared
  field types; the `len` builtin discriminates on the *pre-deref*
  receiver (thin `Ref` → raw array → element count; `Aggregate` →
  field 1); `read_place`'s `Deref` arm unwraps fat `[data_ref, len]`
  aggregates (replaying the deref on the data half); `write_place_frame`
  resolves a `[Field…, Deref, rest…]` projection prefix against live
  storage and delegates the write to the referenced frame, so
  `b.data[i] = v` on a `&'a mut [i32]` field mutates the original
  array instead of panicking.
- Deliberately not coerced: assignment-level `let s: &[i32] = &arr;`
  keeps the thin `Ref` in the interpreter's value model — its builtin
  `iter`/`split_first` paths are built around that shape (wrapping at
  every `let` regressed `slice_iter_next`); the `len` builtin's
  receiver discriminator covers the leaked-thin case.

New regression fixtures (all agree across LLVM, interpreter, and the
differential gate): `call_arg_slice_unsize`, `struct_field_slice_len`,
`slice_field_index`, `mut_slice_field_index`,
`generic_struct_slice_field`, `enum_variant_slice_field`,
`tuple_field_slice_unsize`. KNOWN_GAPS §3.9 carries the full
session-35 resolution note.

## Verification

```
cargo test --release --workspace        # 106 test binaries, 0 failures
GLYIM_TEST_REAL_LLVM=1 cargo test --release -p glyim-test \
    --features glyim-test/real-llvm --test differential   # 2/2, backends agree
GLYIM_TEST_REAL_LLVM=1 GLYIM_LLVM=1 cargo test --release -p glyim-test \
    --features glyim-test/real-llvm --test run_pass_corpus   # incl. needs-llvm slice fixtures
glyip new / build / run                 # end-to-end smoke, green
```

Note on toolchain: the LLVM backend pins `llvm-sys 231` (LLVM 23.1).
On hosts without LLVM 23 dev packages, the prebuilt
`LLVM-23.1.2-Linux-X64` release tarball works: extract it and set
`LLVM_SYS_231_PREFIX=<prefix>`.
