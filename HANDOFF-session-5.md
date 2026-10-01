# Handoff -- glyim-v2, session 5

## TL;DR

Session 5 continued the session-4 next-steps. Item #1 (more `FromStr`
impls) is **done**. Along the way a **new, reproducible compiler ICE** was
discovered and left as a tracked bug (see "New bug" below).

Suite: **4183/4183 pass** (2 skipped), clean tree.

    git log --oneline -4
    b86fc537 feat(stdlib): FromStr impls for all integer primitives
    c0bf7390 docs(handoff): session-4 handoff -- both session-3 open items closed
    58a08fc6 test(hir): pin byte-literal lowering + kitchen-sink arena completeness
    ae0db97e fix(hir): diagnose silent drops of `let` / match-arm statements

## Open item #1 (from session 4) -- DONE: FromStr for all integer primitives

`crates/glyim-lang-core/lib/parse.g` now provides:

    u8, u16, u32, u64, usize, i8, i16, i32, i64, isize

Each `from_str` body is **fully inlined** — no shared helper — for reasons
spelled out under "New bug".

### Design

- Magnitude accumulates in `u64` with the exact pre-multiply overflow check
  `value > (LIMIT - digit) / 10`. Correct in the absence of `i128`/`u128`.
- Signed impls track the sign separately, range-check the positive case
  against `T::MAX`, and use the larger negative ceiling for the `T::MIN`
  magnitude.
- `i64` / `isize` special-case the `-9223372036854775808` magnitude (which
  cannot be written as a positive literal): emit `-9223372036854775807 - 1`.
- Rejects: empty input, lone `-`, non-ASCII-digit bytes, out-of-range
  magnitude. No whitespace, no `+` sign (yet) — matching `net.g`'s
  `parse_u8_dec`.

### Test

`primitive_trait_impl.rs::stdlib_fromstr_available_for_all_integer_primitives`
compiles one program that exercises the boundary value of each type
(`255`, `65535`, `4294967295`, `18446744073709551615`, `-128`, `-32768`,
`-2147483648`, `-9223372036854775808`).

## New bug (tracked, not fixed): ICE when a flat-emitted stdlib free
## function is called from an impl method

### Symptom

    thread 'main' panicked at crates/glyim-core/src/arena.rs:173:9:
    IndexVec::get: index out of bounds
    error: glyim panicked (ICE).

Exit code 1, ICE report written to `$TMPDIR/glyim-ice.txt`.

### Reproduction

Two variants of a *stdlib* `.g` file were tested. With `parse.g` (flat-
emitted, see `flat_modules` in `crates/glyim-lang-std/src/lib.rs`) containing
a *shared helper free function called by the impl methods*:

    // ICEs
    struct ParseIntError;
    fn helper_signed(s: &str, limit: u64) -> Option<(u64, bool)> {
        // real body: as_bytes / len / while / indexing
        ...
    }
    impl FromStr for i32 {
        type Err = ParseIntError;
        fn from_str(s: &str) -> Result<i32, ParseIntError> {
            match helper_signed(s, 2147483648) { ... }
        }
    }

compiling any user program that calls `"42".parse::<i32>()` ICEs.

The **inlined** form (no shared free function; body duplicated into the impl)
compiles cleanly. That is what landed.

### What was bisected

Everything in `/tmp/pe/`, `/tmp/pd/` (transient). Summary of observations:

| Variant                                              | Result |
|------------------------------------------------------|--------|
| session-4 `parse.g` alone (inline i32)               | OK     |
| session-4 + an **unused** top-level free `fn`         | OK     |
| session-4 + a helper returning `Option<u64>` **with a trivial body**, called from an impl | OK |
| session-4 + helper returning `Option<(u64, bool)>` **with a real body**, called | **ICE** |
| session-4 + helper with `while`+indexing, called      | **ICE** |
| all 10 impls, every body inlined (landed)             | OK     |

Minimal reproduction to *attempt next session*: shrink
`/tmp/pe/R2`'s `helper_signed` line by line until the ICE disappears; the
suspect is the `Option<(u64, bool)>` return type crossing an impl-method
call site, or the `while`+indexing body. See "Where to look" below.

### Why it matters

It is a **`u64`-indexed arena bounds panic**, not a diagnostic. It fires
only when the shared function has a non-trivial body. That is exactly the
kind of bug the session-4 "silent drop" work was chartered to surface —
except this one panics before the diagnostic machinery can run.

### Where to look

- `crates/glyim-core/src/arena.rs:173` is the `IndexVec::get` debug
  assertion. The value of `idx` at the panic is the key datum — the ICE
  report does **not** capture it. Running under `RUST_BACKTRACE=full`
  and/or adding a temporary `eprintln!("{idx:?}")` before the
  `debug_assert!` will name the offending id.
- Prime suspects:
  - `crates/glyim-typeck/src/check_expr.rs` — the `Expr::Call` arm when
    the callee is a free function whose return type is `Option<(A, B)>`
    (tuple inside `Option`).
  - `crates/glyim-hir/src/lower/lower_async.rs` — `desugar_async` walks
    bodies and allocates; a mis-sized `IndexVec` there would match the
    panic site.
  - `crates/glyim-mir/src/` — if the panic happens after typeck, the
    MIR builder's local/place arenas are the next candidate.

### Suggested first step

Reproduce with the in-tree `glyim-pipeline/tests/stdlib_full_probe.rs`
harness (add a temporary `.g` file with the shared-helper shape), since
that runs the real pipeline and prints diagnostics.

## Open items (unchanged from session 4, plus the new bug above)

### 1. Isolate and fix the ICE (top priority)

See the section above. Until it is fixed, **do not** refactor `parse.g`
back to shared helpers — the inlined form is the workaround.

### 2. `FromStr for f64`

Not attempted this session. `f64` parsing requires a fractional-part state
machine and an exponent parser; check whether `f64` arithmetic and
`powi`/`powf`-style operations exist in `.g` source. May be a good
isolated exercise once the ICE is fixed.

### 3. Session-3 / session-4 leftovers

- **Never lower `Ty::ERROR` at codegen** — `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Every `thir::Expr::err(span)` should be paired with `diagnostics.push(...)`.
- **Do not `git stash pop` a stale stash without checking `git stash list`.**
  `stash@{0}: WIP on main: 155188ea chore: up logo` is still there.

## Environment notes (carried forward)

- CLI valid emits: `obj`, `exec`, `mir`, `llvm-ir`, `asm`, `cdylib`.
  Use `--emit=obj -o /tmp/x.o path.g`.
- `--with-stdlib` prepends the *minimal* assembled stdlib.
- MIR `ERROR glyim_mir: Place::ty(): Field projection on non-tuple/ADT
  type` lines on every stdlib compile are **pre-existing tracing noise**.
  Filter with `grep -v '^2026-'`.
- Test fixtures:
  - `crates/glyim-hir/src/tests/*.rs` — in-crate lowering unit tests.
  - `crates/glyim-typeck/tests/compile-pass/*.g` — must compile.
  - `crates/glyim-cli/tests/*.rs` — end-to-end CLI regression tests
    (`Command::new(env!("CARGO_BIN_EXE_glyim-cli"))`).

## Working-style constraints (still in force)

- Never lower `Ty::ERROR` at codegen.
- Every `thir::Expr::err(span)` should be paired with `diagnostics.push(...)`.
- Conventional commit prefixes; document remaining blockers in this file.
- Do not `git stash pop` without checking `git stash list`.
- When chasing a diagnostic, reproduce the *exact* failing input against a
  freshly built binary before bisecting. This session found the ICE only
  after several false starts where the shell captured the wrong exit code
  (`$?` inside a pipeline). Prefer `set +e; cmd >out 2>err; rc=$?; set -e`.
