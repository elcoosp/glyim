# Handoff -- glyim-v2, session 2 (continued from the hello-world session)

## TL;DR

HEAD is the commit shown by `git log --oneline -1`. This session landed the
commits listed under "Commits landed" below (all conventional-prefixed,
all green at their time of commit).

Suite at the point this handoff was written: **4178/4178 pass** (2 skipped),
plus one **uncommitted** change (see below).

**Uncommitted (WIP)**: a `SyntaxKind::ByteLit` arm added to `lower_literal`
in `crates/glyim-hir/src/lower/lower_expr.rs`. Byte literals (`b'0'`, `b'\n'`)
were being lowered to `Literal::Unit` (i.e. `()`) by the catch-all `_` arm,
which poisons any expression they appear in. The arm is added; the
investigation it was pulled into is not yet closed.

**Active investigation**: a `let` binding inside a `while` loop that appears
after an early `return` still reports `[T0001] unresolved name` for the bound
name -- even though each sub-piece type-checks in isolation. See "Active bug"
below. This is the next thing to fix.

## Commits landed this session

Run `git log --oneline origin/main..HEAD` for the authoritative list. In
chronological order (earliest first) they are:

1. `b63258ec` refactor(pipeline): share one front half across all emit modes
2. `fd247a85` fix(cache): invalidate on compiler rebuild; add per-emit-mode
   regression tests
3. `2f6f6590` docs(handoff): pinpoint param-bound-assoc-call ICE to mono enqueue
4. `253c9794` feat(typeck,hir,lower): thread method turbofish + devirtualize
   param-bound assoc calls
5. `7eeb6c1b` docs(handoff): record turbofish/devirt landing
6. `13c292e5` fix(lower): diagnose unresolved trait-method calls instead of ICEing
7. `df581987` test(cli): pin that an unsatisfied trait bound is a diagnostic
8. `c277a26b` fix(typeck): let/assign in expr position yield unit, not an Err node
9. `0c4f32ae` fix(stdlib): declare the ErrorKind::InvalidInput variant
10. `ace25071` docs(handoff): close out the trait-bound + X0000 warning work
11. `d672aa91` fix(runtime): reactor no longer drops a registration racing
    a shutdown check
12. `6f5b4d44` fix(type): support trait impls on primitive Self types
13. `7d956f78` test(cli): pin that `impl Trait for <primitive>` resolves
14. `6e2a8942` fix(lower): slice indexing `bytes[i]` on a `&[u8]`

(Numbering is approximate; `git log` is authoritative.)

## What works (verified end-to-end)

| Path | Result |
|------|--------|
| `--emit=obj` on hello world | valid Mach-O arm64, zero X0000 |
| `--emit=exec` | runs, prints `hello` |
| `--emit=mir` / `llvm-ir` / `asm` | all produce output |
| user `impl FromStr for i32` + `parse::<i32>()` | compiles |
| `"42".as_bytes()[0]` | compiles |
| `impl Trait for <primitive>` | resolves |
| unsatisfied trait bound | clean `[T0001]` diagnostic, no ICE |
| full workspace suite | 4178/4178 pass |

## The WIP change (uncommitted)

`crates/glyim-hir/src/lower/lower_expr.rs`, `lower_literal`:

    SyntaxKind::ByteLit => {
        let after_b = &text[1..];
        let inner = if after_b.len() >= 2 {
            &after_b[1..after_b.len() - 1]
        } else {
            ""
        };
        match parse_char_literal(inner) {
            Some(c) if (c as u32) <= u8::MAX as u32 => {
                Literal::Uint(c as u128, Some(UintTy::U8))
            }
            _ => {
                tracing::warn!("failed to parse byte literal: {}", text);
                Literal::Unit
            }
        }
    }

Before this arm, `b'0'` fell into the catch-all `_ => Literal::Unit`, so it
became `()` -- a genuine bug independent of the `digit` issue, since byte
literals are used throughout `net.g` / `io.g` parsing code. Keep it.

## Active bug: `unresolved name` for a `let` binding after an early `return`

### Minimal failing case

    fn compute(s: &str) -> i32 {
        let bytes = s.as_bytes();
        let mut i = 0;
        let mut value: i32 = 0;
        while i < bytes.len() {
            let ch = bytes[i];
            if ch < b'0' || ch > b'9' {
                return -1;
            }
            let digit: i32 = (ch as i32) - (b'0' as i32);
            value = value * 10 + digit;
            i += 1;
        }
        value
    }
    fn main() { let x = compute("4"); }

Fails with `[T0001] unresolved name \`digit\`` pointing at the use of `digit`
on the `value = ...` line -- the binding from the `let digit` line is not
visible to the next statement in the same block.

### Bisection so far (each PASSES individually)

- `while` + early `return` (no byte literal)
- `while` + early `return` + byte-literal *condition* (`if ch < b'0'`)
- `while` + early `return` + byte-literal *cast* (`(ch as i32) - (b'0' as i32)`)
- `while` + two-condition `||` `if`
- `while` + accumulator (`value = value * 10 + digit`)
- byte literal alone (`let x: u8 = b'0';`) -- OK after the WIP arm
- `(52 as i32) - (b'0' as i32)` as a `let` -- OK

The **combination** fails, or the failing run was against a stale build
(there was a `git checkout` / `git stash pop` interleaving mid-investigation
that may have left stale artifacts).

### Suggested next steps

1. `cargo clean -p glyim-cli` (or at least `cargo build`), then re-run the
   **exact minimal case** above. Confirm it still fails before bisecting.
2. If it still fails, bisect from the **full** case downward (remove one
   construct at a time), rather than from empty upward. Record exactly which
   construct must be present for the failure.
3. Prime suspects to read:
   - `crates/glyim-typeck/src/check_stmt.rs` -- `check`, `check_stmt_to_thir`
   - `crates/glyim-typeck/src/check_expr.rs` -- the `Expr::Block`,
     `Expr::While`, `Expr::Loop`, `Expr::If` arms (does the loop body go
     through `check_stmt_to_thir` for its statements, or through `check_expr`?)
   - `crates/glyim-typeck/src/check_body.rs` -- `FnCtxt` / `LocalEnv::add_binding`
4. Hypothesis to test first: the `while`-body block is checked through a path
   that pushes a fresh scope per statement (so the `let` binding is discarded
   before the next statement). Compare the top-level body loop in `FnCtxt::check`
   (which correctly keeps one scope) against the `Expr::Block` arm.

### Where to look

| What | Where |
|------|-------|
| Byte-literal lowering (WIP) | `crates/glyim-hir/src/lower/lower_expr.rs::lower_literal` |
| Statement checking | `crates/glyim-typeck/src/check_stmt.rs` |
| Expression checking | `crates/glyim-typeck/src/check_expr.rs` |
| Local env / bindings | `crates/glyim-typeck/src/check_body.rs` |
| Trait-impl dispatch table | `crates/glyim-type/src/ty_ctx.rs`, `ty_ctx_mut.rs` |
| Stdlib sources | `crates/glyim-lang-core/lib/*.g`, `crates/glyim-lang-std/lib/*.g` |
| Assembled stdlib | `crates/glyim-lang-std/src/lib.rs` |
| Runtime | `crates/glyim-runtime/src/` |

## Open item (lower priority): stdlib `impl FromStr for i32`

The stdlib has a `trait FromStr` (`crates/glyim-lang-core/lib/str.g`) but **no
impls**. Now that the compiler supports `impl Trait for <primitive>` and byte
literals, a stdlib `impl FromStr for i32` can be written. Do **not** use
`i32::checked_mul` / `checked_add` -- they are not registered (only
u32/u64/usize have checked arithmetic today); use plain `*` / `+` like
`net.g`'s `parse_u8_dec` helper does. Once the `unresolved name` bug above is
fixed, re-attempt adding the impl, and check `cargo nextest run --workspace`
stays green.

## Working-style constraints (still in force)

- Never lower `Ty::ERROR` at codegen -- `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Every `thir::Expr::err(span)` should be paired with `diagnostics.push(...)`.
- Conventional commit prefixes; document remaining blockers in this file.
- **Do not `git stash pop` a stale stash without checking `git stash list`
  first.** During this session an unreviewed `git stash pop` pulled a stash
  from a much earlier session and produced a conflict that needed
  `git reset --hard`. If in doubt, `git stash show -p stash@{N}` before
  popping.
