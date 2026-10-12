# HANDOFF — session 11 (LLVM backend hardening)

**HEAD**: `ffb357aa`
**Tree**: clean (only `docs/roadmaps/better-testing.md` untracked — not mine)

## ⚠️ Read this first

**Free memory before running anything.** `cargo build` needs ≥ 1 GB. Brave's
renderer helpers alone hold ~1.3 GB. If `Pages free` in `vm_stat` is under
~150 MB, `cargo` gets OOM-killed mid-link and reports a spurious ICE.
Close browser tabs first.

## 0. Resume

```bash
cd /Users/adm/Documents/Repos/glyim-v2
vm_stat | head -3                      # check free pages first
CARGO_BUILD_JOBS=1 cargo check --workspace --tests
CARGO_BUILD_JOBS=1 cargo build -p glyim-runtime -p glyim-cli
cargo nextest run --workspace --no-fail-fast
```

Use `CARGO_BUILD_JOBS=1` when memory is tight. It roughly halves peak
usage at the cost of wall time.

## 1. The differential test (the key new tool)

`crates/glyim-test/tests/differential.rs`. For every `run-pass` fixture,
compiles once and runs BOTH the LLVM-native binary and the interpreter,
asserting they agree on exit code + stdout.

Run it (memory-hungry — close other apps):

```bash
GLYIM_TEST_REAL_LLVM=1 \
  cargo test -p glyim-test --features real-llvm --test differential -- --nocapture
```

Why it exists: the existing 99 run-pass fixtures all go through the
interpreter; the LLVM backend was verified by exactly one fixture
(`hello_world`). A silent miscompile in LLVM passed all 4599 tests. This
test is the class-coverage gate that was missing — every bug fixed this
session was invisible to the interp-only corpus.

**Memory note:** the test compiles the full assembled stdlib once per
fixture and runs them in parallel. Peak usage can hit several GB. If it
OOMs, edit `crates/glyim-test/tests/differential.rs` and change
`.parallel(false)` (currently set) — the test is already sequential, but
the stdlib compile itself is heavy. Running with only a few fixtures
(`TestRunner::filter`) is the fallback.

## 2. Commits landed this session (5)

| Commit | Fix |
|---|---|
| `dc1694cf` | differential test (LLVM vs interp) |
| `33ed3e63` | `Rvalue::Len` handles `TyKind::String` (was panicking on `str::trim`); harness links `libglyim_runtime.a` (was silently `.ok()`-swallowing the link error, showing every stdlib fixture as "interp-only") |
| `60bab294` | `Rvalue::Ref` on an unsized place (`str`/`[T]`) loads the *fat pointer* struct, not its address. Fixed all 8 `str/trim_*` + 4 `str/contains_*` fixtures. |
| `2fee353f` | `try_lower_builtin_intrinsic`'s `len`/`is_empty` arm falls through when the receiver isn't a 2-field fat pointer (`String` is 1-field `{Vec<u8>}`) |
| `ffb357aa` | same guard for the `to_string` arm |

## 3. What's still broken — 8 of 83 compared fixtures

From the last full differential run (before the machine ran out of memory):

### Cluster A — `fn_abi_of failed: UnknownType(Ty(NNN))` × 4 (biggest, one root cause)
- `behavior/vec_len_after_push.g`
- `core/maybe_uninit_new.g`
- `probe_option_unwrap.g`
- `probe_vec_push_len.g`

Generic enum/struct monomorphization ICEs inside
`glyim-codegen-llvm/src/lower.rs::fn_abi_of`. Suggested first step: add a
TMP `eprintln!` at the `fn_abi_of failed` site in `lower.rs` printing the
`Ty` handle, then a matching eprintln at the callsite that produced that
type (likely `build_layout_aggregate` or a `Call` terminator's
signature). The `Ty(NNN)` is likely an unresolved infer var from a
generic that monomorphization didn't substitute.

### Cluster B — `String::as_str()` returns wrong length × 2
- `behavior/as_str_len.g` — `String::new(); s.push_str("abc"); s.as_str().len()`
- `behavior/string_push_then_index.g`

Root cause: `String` (AdtId 1050) lowers to an opaque 3-field struct
`{ ptr, capacity, len }` (it wraps `Vec<u8>`), while `&str` is a 2-field
fat pointer `{ ptr, len }`. The `as_str` arm in
`try_lower_builtin_intrinsic` passes the whole 3-field struct through as
if it were a 2-field `&str`. LLVM IR confirms: the extracted field 1 (used
as the length) is `capacity`, not `len`.

**Fix sketch (was written, reverted before verifying — see §4):**
bit-cast the receiver to `{ i64, i64, i64 }`, extract fields 0 and 2,
rebuild the destination `{ ptr, i64 }`.

### Cluster C — `String::push_str` mutates a copy × 1
- `behavior/push_str_roundtrip.g` — `String::new(); s.push_str("xy"); s.push_str("z"); s.to_string().len()`

Root cause: the `push_str` arm stores the receiver into a local
`alloca %ps_hdr` and passes *that* to the runtime helper, then never
writes the mutated header back. So `s` stays empty after `push_str`.

**Fix sketch (was written, reverted before verifying):** when the
receiver is `Operand::Copy(p) | Operand::Move(p)`, use `place_ptr(p)` as
the header pointer (mutates the caller's place in place); keep the alloca
only for the no-place fallback.

### Cluster D — `unsupported intrinsic as_bytes receiver/destination shape` × 1
- `probe_string_push_str.g`

The `as_bytes` arm at the bottom of `try_lower_builtin_intrinsic` has a
shape guard `recv_val.is_struct_value() && dest_llvm_ty.is_pointer_type()`
that rejects the `String` receiver (3-field struct) or the `[u8]`
destination. Needs the same "unwrap `String` to its `Vec<u8>` inner
`{ptr, cap, len}` and rebuild as `{ptr, len}`" treatment as Cluster B.

### Cluster E — `abort_stops` hangs on the LLVM side × 1
- `core/abort_stops.g` — interp returns 101 (expected), LLVM side times
  out.

The `glyim_process_abort` / `glyim_panic` extern hooks work in the
interpreter (see `crates/glyim-mir-interp/src/lib.rs` — they return
`InterpError::Panic`), but the LLVM backend either doesn't route the
extern call or emits a call that returns. Investigate how
`glyim-codegen-llvm` lowers a `Call` to a `!`-returning extern.

## 4. What was reverted (untested — the machine ran out of memory)

Two edits to `crates/glyim-codegen-llvm/src/lower.rs` were written but
**reverted** because `cargo build` started OOM-killing before they could
be compiled:

1. The `push_str` **write-back fix** (Cluster C).
2. The `as_str` **field-extraction fix** (Cluster B).

The exact edit text is in this session's conversation history. Re-apply
them one at a time, with a single-fixture verification after each, once
memory is free.

## 5. Debugging rules that worked

1. **Dump LLVM IR** with `--emit=llvm-ir -o /tmp/x.ll` and diff the
   `define fastcc ... @__glyim_fn_N` block against a known-good case.
   Two bugs this session (the `Rvalue::Ref` fat-pointer bug and the
   `as_str` capacity-vs-len bug) were found *only* by reading the IR —
   the Rust code path looked correct.
2. **`GLYIM_TMP_*` env-gated eprintln probes** for narrow fields; remove
   them once found.
3. **Fresh `GLYIM_CACHE_DIR`** for every probe — the CLI caches
   aggressively.
4. **`ulimit -t 5`** on any fixture binary you're not sure terminates.

## 6. Matrix status

- `cargo check --workspace --tests`: clean.
- `cargo nextest run --workspace`: 4599 pass / 0 fail / 2 skip (last
  full run before memory pressure).
- Differential: 83 compared, **8 disagreements** (was 22 at session
  start), 0-1 interp-only depending on fixture set.

## 7. Other useful docs

- `docs/roadmaps/HANDOFF-session-10.md` — the previous handoff (enum
  match §4). Kept for context on the interpreter-side invariants.
- `docs/roadmaps/glyim-fix-plan.md` — original 202-task plan.
