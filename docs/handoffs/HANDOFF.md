# HANDOFF — What remains (Session 6 → 7)

**HEAD**: `946403fe`
**Matrix**: 4598 passing / 0 failing / 2 skipped
**Failing test**: none — the last probe (`probe_string_push_str.g`) now passes.

---

## The one remaining item

### `probe_string_push_str.g`

**Fixture:**
```glyim
fn main() {
    let mut s = String::new();
    s.push_str("abc");
    println!("{}", s.as_str());
}
```
**Expected stdout:** `abc` · **Actual:** `` (empty)

**Root cause (proven, not hypothesized):** `println!("{}", x)` expands to
`format!(concat!("{}","\n"), x)`. Emitting real interpolation for that
requires `format!` to produce code calling `String::new`, `String::push_str`,
and `String::as_str`. Those calls lower to **builtin `FnDefId`s**
(9113, 9155, 9095) via typeck's `builtin_fn_index`, and
`glyim-codegen-llvm`'s `try_lower_builtin_intrinsic` handles only
`as_bytes` / `as_ptr` / `as_mut_ptr` / `len` / `is_empty`. So LLVM emits
calls to symbols that were never defined, and the link fails:

```
Undefined symbols for architecture arm64:
  "___glyim_fn_9095", referenced from ___glyim_fn_508   (as_str)
  "___glyim_fn_9113", referenced from ___glyim_fn_508   (String::new)
  "___glyim_fn_9155", referenced from ___glyim_fn_508   (push_str)
```

This was attempted and **reverted** this session: the interpolation emission
regressed `glyim-cli::emit_modes exec_binary_prints_hello` with exactly those
symbols. The revert is the current committed state — `format!("{}", x)` still
falls through to the empty-string stub, and `probe_string_push_str.g` fails
as a documented gap.

---

## Two paths to fix it

### Path A — expand `format!` to source-fn calls, not builtin ids *(preferred, ~2–3 h)*

The stdlib's `String::new` / `push_str` / `as_str` have real MIR bodies
(`crates/glyim-lang-alloc/lib/string.g`) and therefore real LLVM symbols.
If the expansion referenced them **by path** (`String::new()`,
`s.push_str(..)`, `s.as_str()`) rather than resolving to the typeck builtin
`FnDefId`, LLVM would link.

**Recon first — do not patch blind:**
1. Where does typeck pick the builtin `FnDefId` for `String::new` /
   `push_str` / `as_str` over the source impl? (`check_expr.rs`'s
   `builtin_fn_index` lookup vs `inherent_method`.)
2. Does the expander's output preserve enough path information, or does it
   need a marker the typeck will resolve to the source method?

**Change surface:** the `BuiltinMacro::Format` arm in
`crates/glyim-meta/src/expander/mod.rs` (`try_format_one_interp`), plus
whatever typeck needs to prefer the source impl for these three names.

### Path B — extend `try_lower_builtin_intrinsic` *(~1 day)*

Lower the three builtins inline in `crates/glyim-codegen-llvm/src/lower.rs`:

- `String::new` → `dest_llvm_ty.const_zero()` (an empty `String` is
  all-zero: `{ inner: { buf: {ptr, cap}, len: 0 } }`).
- `String::as_str` → extract the `{ptr, len}` sub-struct.
- `String::push_str` → needs real growth (allocation + memcpy). **Not
  inlineable** in a few lines; delegate to a `glyim-runtime` FFI helper
  (`glyim_string_push_str(dest, ptr, len)`) or route to the source fn.

`push_str` is why Path A is preferred — it avoids writing allocator glue
into the LLVM backend.

---

## Verification (unchanged, but re-state because the previous handoff got it wrong)

```bash
cargo check --workspace --tests        # --tests is REQUIRED; plain --workspace skips test targets
cargo nextest run --workspace --no-fail-fast
cargo nextest run -p glyim-test run_pass_corpus_passes --no-fail-fast
```

**Expected after the fix:** 4,571 passing / 0 failing / 2 skipped.

---

## Do not touch

- `crates/glyim-mir-interp/src/lib.rs`'s `InterpValue::String` byte-payload
  model. It is an approximation (a `String` standing in for `&[u8]`), and
  §3.2 of `.fix-log/HANDOFF.md` documents the correct replacement
  (a byte arena + `InterpValue::ByteRef`). The approximation currently
  passes four of five probes; replacing it is separate work, not part of
  closing `probe_string_push_str`.
- The reverted interpolation emission. It is reverted for a reason
  (linker regression). Re-land it only together with Path A or B.

---

## Context: what this session already fixed (so you don't redo it)

Async test · leaky interpreter thread · `probe_option_unwrap` ·
`probe_vec_push_len` · `probe_println_function` · `probe_println_macro` ·
all 4 `emit_modes` tests · the pre-existing `MethodDef` test breakage ·
the stale `--lto fat` test · T121 two-phase refinement · sentinel `FnDefId`
ICE · interpreter builtin dispatch · byte-pointer arithmetic · tolerant
`Return` · extern FFI dispatch · `format!` literal + group handling.

---

## How to resume

1. `cd /Users/adm/Documents/Repos/glyim-v2`
2. `git log --oneline -1` → `c66adbd5`
3. `cargo nextest run -p glyim-test run_pass_corpus_passes --no-fail-fast` → 1 fixture
4. Do the Path A recon (step 1 in "Recon first"). Do not patch until you
   know where the builtin-vs-source choice is made.
5. Re-verify with `cargo nextest run --workspace --no-fail-fast`.
