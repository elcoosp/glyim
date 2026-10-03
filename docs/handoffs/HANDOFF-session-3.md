# Handoff -- glyim-v2, session 3 (continued from session 2)

## TL;DR

HEAD is the commit shown by `git log --oneline -1`.

Suite at the point this handoff was written: **4178/4178 pass** (2 skipped),
no uncommitted changes, no stashes needed.

The session-2 handoff's **"Active bug" (unresolved name after early return in
a while loop) is closed** -- and it was *not* a scope/type-checking bug. The
real cause was much shallower. See below.

## Commits landed this session

Only one, but it is the one the session-2 handoff was looking for:

1. `fix(syntax): include ByteLit in SyntaxKind::is_literal` -- the fix
   for the session-2 "active bug". Also pins the predicate and adds a
   typeck compile-pass regression fixture.

(`git log --oneline -5` is authoritative.)

## The session-2 "active bug", re-diagnosed and fixed

### What the session-2 handoff claimed

> a `let` binding inside a `while` loop that appears after an early `return`
> still reports `[T0001] unresolved name` for the bound name

### What it actually is

A **byte literal that appears as the operand of a cast** (`b'0' as i32`)
silently drops the enclosing *statement* from the HIR. Any subsequent use of
a name that statement was supposed to bind then reports
`[T0001] unresolved name`.

Loops, early returns, assignment, and the two-condition `if` are all
irrelevant. Bisection (fifteen variants, `/tmp/glyim_repro/*.g` and
`/tmp/glyim_repro2/*.g`) settled this:

| Variant                                  | Result before fix |
|------------------------------------------|-------------------|
| `let a: u8 = b'0';` (byte lit, no cast)  | OK                |
| `let a: i32 = b'0' as i32;` (unused)     | OK                |
| `let a: i32 = b'0' as i32;` then **use `a`** | **FAIL** `unresolved name \`a\`` |
| `let digit: i32 = (ch as i32) - (b'0' as i32);` then use `digit` | **FAIL** |
| `48u8 as i32` (int, not byte literal)    | OK                |
| `b'A'` (no cast, used directly)          | OK                |
| `while`/`return`/`if` removed            | irrelevant (still fails without them) |

Minimal reproduction:

    fn digit_value(ch: u8) -> i32 {
        let digit: i32 = (ch as i32) - (b'0' as i32);
        digit
    }
    fn main() { let _ = digit_value(b'7'); }

### Root cause

`SyntaxKind::is_literal()` (`crates/glyim-syntax/src/lib.rs`) omitted
`SyntaxKind::ByteLit`. That predicate gates two HIR lowering sites:

- `lower_lit_expr` (`crates/glyim-hir/src/lower/lower_expr.rs:962`) --
  filters the leaf token of a `LitExpr` node.
- `lower_pat.rs:63,79` -- filters leaf tokens of `PatLit` / `PatRange`.

Consequence chain:

1. `b'0'` is a `SyntaxKind::ByteLit` token (lexer at
   `crates/glyim-frontend/src/lexer.rs:138`).
2. Its enclosing `LitExpr` node hits `lower_lit_expr`, whose `.find(...)`
   uses `is_literal()` -- returns `None` for `ByteLit`.
3. `lower_lit_expr` returns `None`.
4. `lower_cast_expr`'s `lower_expr(&expr_node?, ...)` propagates that `None`
   via `?`.
5. The enclosing `LetStmt` in `lower_block_to_expr` then sees `rhs_expr_id ==
   None` and **silently drops the whole `let`** (the `if let (Some(pat),
   Some(rhs))` guard fails; the fallback `if let Some(rhs) = expr_node` also
   fails because `rhs` is `None`).
6. No HIR node is emitted for the `let`; nothing binds `digit`.
7. The next statement's `Expr::Path("digit")` reaches `check_path`, misses
   every namespace, and emits `[T0001] unresolved name \`digit\``.

The `ByteLit` arm that session 2 left **uncommitted** in `lower_literal`
(`crates/glyim-hir/src/lower/lower_expr.rs:1026`) was *dead code*: the token
never reached `lower_literal`. The session-2 handoff kept that arm; it is
correct and now reachable (it lowers byte literals to
`Literal::Uint(c, Some(UintTy::U8))`). This session's commit kept it and made
it live by fixing the upstream filter.

### What the fix actually changes

- `crates/glyim-syntax/src/lib.rs`: `is_literal()` now includes
  `SyntaxKind::ByteLit`.
- `crates/glyim-syntax/src/tests/kind_tests.rs`: `is_literal_works` now
  includes `ByteLit` in the positive-case list.
- `crates/glyim-typeck/tests/compile-pass/byte_literal_in_cast.g`: new
  regression fixture exercising the exact minimal shape.

### Why the session-2 bisection was confusing

Each "sub-piece type-checks in isolation" because *any* isolated sub-piece
that did not pair a `b'0' as …` cast with a downstream *use of the bound
name* was fine. The bug needs **both halves**: the drop (from the cast) and
a later reference (which surfaces as "unresolved name"). The handoff's
suspicion of `Expr::Block` / `while` scope handling was a false positive;
the typeck code paths for those constructs are correct.

### Diagnostic lesson for future sessions

An "unresolved name `X`" where `X` is clearly introduced earlier in the
same block is a strong signal of a **statement that was silently dropped
during HIR lowering**, not of a scope bug in typeck. Both `.find(...)`
returning `None` and `let (Some(a), Some(b)) = (…) else { fall through }`
are common silent-drop shapes in `crates/glyim-hir/src/lower/`. Grep for
`?`-propagation chains out of `lower_*_expr` when chasing this class.

## Where the project now stands

### Verified end-to-end (carried forward from session 2, still true)

| Path | Result |
|------|--------|
| `--emit=obj` on hello world | valid Mach-O arm64, zero X0000 |
| `--emit=exec` | runs, prints `hello` |
| `--emit=mir` / `llvm-ir` / `asm` | all produce output |
| user `impl FromStr for i32` + `parse::<i32>()` | compiles |
| `"42".as_bytes()[0]` | compiles |
| `impl Trait for <primitive>` | resolves |
| unsatisfied trait bound | clean `[T0001]`, no ICE |
| full workspace suite | **4178/4178 pass** (2 skipped) |

### Newly verified this session

| Path | Result |
|------|--------|
| `b'0' as i32` used via its bound name | compiles |
| byte literal in a pattern (`PatLit`) | compiles |
| every variant in `/tmp/glyim_repro{,_2}/*.g` | compiles |

## Open items, in priority order

### 1. Stdlib `impl FromStr for i32` (unchanged from session 2)

The stdlib has a `trait FromStr` (`crates/glyim-lang-core/lib/str.g`) but no
impls. Now that `impl Trait for <primitive>` and byte literals both work,
write the impl and confirm `cargo nextest run --workspace` stays green.

**Do not** use `i32::checked_mul` / `checked_add` -- only u32/u64/usize have
checked arithmetic registered. Use plain `*` / `+`, as `net.g`'s
`parse_u8_dec` does.

A plausible shape (worth checking against `net.g` / `io.g` conventions
before committing):

    impl FromStr for i32 {
        type Err = Error;
        fn from_str(s: &str) -> Result<i32, Error> {
            let bytes = s.as_bytes();
            let mut i = 0;
            let mut neg = false;
            if i < bytes.len() && bytes[i] == b'-' {
                neg = true;
                i += 1;
            }
            let mut value: i32 = 0;
            while i < bytes.len() {
                let ch = bytes[i];
                if ch < b'0' || ch > b'9' {
                    return Err(Error::InvalidInput);
                }
                let digit: i32 = (ch as i32) - (b'0' as i32);
                value = value * 10 + digit;
                i += 1;
            }
            if neg { Ok(-value) } else { Ok(value) }
        }
    }

(Adjust `Error` / `Err` variant to match `str.g`'s `FromStr` signature.)

### 2. Sweep the silent-drop class in HIR lowering

The bug fixed this session is one instance of a broader pattern. Worth a
short audit pass:

- Every `SyntaxKind::is_literal()` / `is_expr_node` / `is_type_node` /
  `is_pattern` call site: are all variants that can appear here accepted?
- Every `lower_*_expr` that returns `Option<ExprId>`: when it returns
  `None`, does the caller *diagnose*, or silently drop?
- The `let (Some(a), Some(b)) = … else { fall through }` shape in
  `lower_block_to_expr` is especially suspect: it fails the whole statement
  silently.

A single new `glyim-hir` unit test that walks a "kitchen-sink" body and
asserts the HIR arena size matches an expected count would catch a class of
these.

### 3. Session-2's leftover items (still relevant)

- **Never lower `Ty::ERROR` at codegen** -- `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Every `thir::Expr::err(span)` should be paired with
  `diagnostics.push(...)`.
- **Do not `git stash pop` a stale stash without checking
  `git stash list`.** At the start of this session there was one stale
  stash from an earlier branch: `stash@{0}: WIP on main: 155188ea chore: up
  logo`. It was not popped. It may still be there.

## Tooling / environment notes

- The CLI does not accept `--emit=check`; valid emits are `obj`, `exec`,
  `mir`, `llvm-ir`, `asm`, `cdylib`. To reproduce, use `--emit=obj -o
  /tmp/x.o path.g`.
- The CLI does not automatically prepend the stdlib unless `--with-stdlib`
  is passed. For a bare test file, leave it off.
- `crates/glyim-cli/tests/trait_bound_diagnostic.rs` is the canonical
  template for "run the CLI on a temp file, assert on stdout/stderr/exit"
  regression tests. Any new end-to-end CLI regression test should mirror
  its structure (`tempdir`, `CARGO_BIN_EXE_glyim-cli`, `Command::new(...)`).
- Test fixtures live at:
  - `crates/glyim-typeck/tests/compile-pass/*.g` -- must compile.
  - `crates/glyim-typeck/tests/compile-fail/*.g` -- must produce a diagnostic.

## Working-style constraints (still in force)

- Never lower `Ty::ERROR` at codegen.
- Every `thir::Expr::err(span)` should be paired with `diagnostics.push(...)`.
- Conventional commit prefixes; document remaining blockers in this file.
- Do not `git stash pop` without checking `git stash list`.
- When chasing a diagnostic, reproduce the *exact* failing input against a
  freshly built binary **before** bisecting. The session-2 handoff spent
  effort suspecting scope bugs; the actual cause was one missing enum
  variant in a predicate.
