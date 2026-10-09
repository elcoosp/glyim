# HANDOFF — Glyim compiler (current)

**Updated**: end of Session 7
**HEAD**: `946403fe`
**Matrix**: **4598 passing / 0 failing / 2 skipped** — fully green.

> Session 5's handoff (`.fix-log/HANDOFF.md` before this) claimed
> `17aacb46`, 4570/1. Superseded.

---

## 1. Session 7 summary

Started from the `better-testing.md` doc, ended up fixing five real
compiler defects plus building a four-class regression suite.

### Bugs fixed (6 commits)

| Commit | Bug |
|---|---|
| `7df95f81` | typeck returned sentinel `FnDefId(u32::MAX-2)` for **any** `.to_string()`; lowering replaced it with a poison `Error` constant → every `to_string` returned 0 |
| `9950a459` | builtin methods never registered in `fn_sigs` (codegen panicked "no signature registered"); `str::to_string` had no LLVM arm |
| `59ed4b2c` | `String`/`Vec`/`Box` are `TyKind::Adt(1050/1020/1040)`, not `TyKind::String` — the builtin-before-impl guard skipped them, so `s.len()` resolved to an empty `str::len` stub (uninitialized → 0) |
| `414e19a9` | `format!("{}", x)` interpolation end-to-end (emitter + LLVM `as_str`/`new`/`push_str` arms) |
| `cf68f584` | interpreter now hard-fails on sentinel ids instead of returning poison |

`probe_string_push_str.g` — the failure that motivated this — passes.

### Regression suite (4 classes, all committed)

| Class | Where | Guards |
|---|---|---|
| 1. Registry invariants | `glyim-type/src/tests/builtin_registry.rs` | builtin id present in method table but missing `fn_sigs`; sentinel slots; duplicate/out-of-range ids |
| 2. Poison hard-fail | `glyim-mir-interp/src/tests/poison.rs` | a compiler sentinel reaching a live path must panic |
| 3. Behavioral matrix | `glyim-test/tests/run-pass/behavior/*.g` (9 fixtures) | method resolving to the wrong/empty body — value assertions through the real pipeline |
| 4. Backend completeness | `glyim-cli/tests/emit_modes.rs::object_has_no_unresolved_glyim_fn_symbols` | a builtin with no lowering arm → undefined `__glyim_fn_*` symbol |

---

## 2. Original plan status (`docs/roadmaps/glyim-fix-plan.md`)

202 tasks (T001–T202), 199 findings, 4 waves.

- **Waves 1–4**: done in sessions 1–4 (114 commits).
- Status markers in tree: **`-PATCHED` 111 files**, `-REVIEWED` 2, `-REVISED` 1, **`-DEFERRED` 1** (`str.g`).

---

## 3. What remains: T140 [STD-15]

Bodyless "compiler intrinsic" stubs with no lowering. Plan verification:
`"  x ".trim().len()` must print `1`.

**Open items:**

- **`str.g`**: `str::len` (empty stub — masked by the builtin table for
  `String`, but still there), `str::contains`, `str::trim`, `Chars::next`,
  `Lines::next`.
- **`mem.g`**: `size_of`, `align_of`, `needs_drop`.
- **`ptr.g`**: `read`, `write`, `copy`, `copy_nonoverlapping`,
  `drop_in_place` (these four have runtime FFI: `glyim_string_from_bytes`,
  etc., but the *core* ptr intrinsics do not).
- **`MaybeUninit`**: `uninit` / `new` / `assume_init`.

Already done (T140 partials): `ptr::null`/`null_mut`, `NonNull::{new_unchecked,dangling}`,
`mem::forget`.

**Blocker for `size_of`/`align_of`**: needs a `FnDefId`→name registry at
lowering (see session-5 handoff §2.3); the calls carry the generic `T` in
the callee's `Substitution`, so the fold is feasible once the id is
identifiable.

---

## 4. Verification (corrected)

```bash
cargo check --workspace --tests   # --tests is REQUIRED (skips test targets otherwise)
cargo nextest run --workspace --no-fail-fast
```

Expected: **4598 passed / 2 skipped**.
