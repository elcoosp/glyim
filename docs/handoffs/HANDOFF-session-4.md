# Handoff -- glyim-v2, session 4 (continued from session 3)

## TL;DR

Both open items from the session-3 handoff are closed. Suite: **4182/4182 pass**
(2 skipped), no uncommitted changes.

Session 4 landed three commits on top of session 3's two:

1. `feat(stdlib): add impl FromStr for i32` (open item #1 from session 3)
2. `fix(hir): diagnose silent drops of \`let\` / match-arm statements`
   (open item #2, first half)
3. `test(hir): pin byte-literal lowering + kitchen-sink arena completeness`
   (open item #2, second half)

`git log --oneline -6` is authoritative.

## Commits landed this session (chronological)

1. `1a8523d8` feat(stdlib): add impl FromStr for i32
2. `ae0db97e` fix(hir): diagnose silent drops of \`let\` / match-arm statements
3. `58a08fc6` test(hir): pin byte-literal lowering + kitchen-sink arena
   completeness

## Open item #1 (from session 3) -- DONE: stdlib \`impl FromStr for i32\`

### What landed

A new flat source file `crates/glyim-lang-core/lib/parse.g` provides:

    struct ParseIntError;
    impl FromStr for i32 { type Err = ParseIntError; fn from_str(..) { .. } }

It is registered in `glyim-lang-std`'s `std_source` map (key `"parse"`) and
added to `flat_modules` in **both** the full and minimal assemblers
(`std_source_assembled` and `std_source_assembled_minimal`). Emitting it
FLAT means `impl FromStr for i32` binds to the primitive `i32`, not to a
module named `parse`.

### Notable constraints honoured

- **Plain `*` / `+` only** -- only u32/u64/usize have checked arithmetic
  registered (`crates/glyim-type/src/ty_ctx_mut.rs`). Matching `net.g`'s
  `parse_u8_dec` convention.
- **Accumulate in `i64`** so overflow is detected before range-checking
  against i32's bounds (`-2147483648..=2147483647`).
- **`ParseIntError` is a dedicated zero-sized struct**, not `()` -- keeps
  the public surface forward-compatible with a Rust-like `ParseIntError`.

### Test changes

`crates/glyim-cli/tests/primitive_trait_impl.rs` was refactored because the
old single test (a user-supplied `impl FromStr for i32`) now conflicts with
the stdlib one (`[T0001] conflicting implementations of trait`). It now has
**two** tests:

- `user_impl_on_primitive_resolves` -- the original regression (impl on a
  primitive `Self` type resolves), retargeted at a *user* trait on `u16` so
  it does not collide with stdlib. If `impl_method_fns` regresses to
  `AdtId` keying, this fails.
- `stdlib_fromstr_for_i32_is_available` -- pins that the *stdlib* impl alone
  satisfies `"42".parse::<i32>()`, without a user impl in the program.

## Open item #2 (from session 3) -- DONE: sweep the silent-drop class

### What was found

`crates/glyim-hir/src/lower/lower_expr.rs` had exactly the shapes the
session-3 handoff predicted:

- **`lower_block_to_expr`'s `SyntaxKind::LetStmt` arm** -- when
  `lower_expr(&rhs)` returned `None`, the `let` was dropped silently (the
  session-3 bug). The old fallback then *re-lowered* the same failing RHS
  and discarded the result entirely.
- **`lower_match_expr`** -- an arm whose pattern or body failed to lower was
  skipped silently, producing a downstream "non-exhaustive match" report far
  from the real failure.

Every other `return None` in the file is either an already-diagnosed
structural malformed-node guard, or is caught by the enclosing arm.

### What landed (fix)

Both sites now emit a `GlyimDiagnostic::internal_error` naming the span and
stating explicitly that a construct is missing from the HIR. No behavioural
change to programs that lower cleanly.

### What landed (tests, on top of the fix)

Three new tests in `crates/glyim-hir/src/tests/lower_expr_tests.rs`:

1. `byte_literal_lowers_as_u8_literal` -- `b'0'` lowers to
   `Literal::Uint(48, Some(UintTy::U8))`. Direct pin of the session-3 fix.
2. `byte_literal_inside_cast_lowers_the_let_statement` -- the exact session-3
   repro shape; asserts the `Expr::Let` survived.
3. `kitchen_sink_body_lowers_all_statements` -- a body mixing `let`, `while`,
   `if`, `return`, `assign`, `cast`, `binary`, `call`, `index`, method call
   and byte literals; asserts all five expected `let` bindings survive plus
   every other statement kind. Catches the "leaf lowering silently returns
   None" class at the lowering boundary, rather than as a phantom
   `unresolved name` in the type-checker.

## Where the project now stands

| Path | Result |
|------|--------|
| `--emit=obj` on hello world | valid Mach-O arm64 |
| `--emit=exec` | runs, prints `hello` |
| `--emit=mir` / `llvm-ir` / `asm` | all produce output |
| `b'0' as i32` used via its bound name | compiles (session-3 fix) |
| byte literal in a pattern (`PatLit`) | compiles (session-3 fix) |
| `"42".parse::<i32>()` with `--with-stdlib` | compiles (session-4) |
| user `impl Trait for <primitive>` | resolves (session-4 test refactor) |
| unsatisfied trait bound | clean `[T0001]`, no ICE |
| full workspace suite | **4182/4182 pass** (2 skipped) |

## Open items, in priority order

### 1. Stdlib FromStr for other primitives (natural follow-up)

`parse.g` currently has only `impl FromStr for i32`. Worth adding:

- `impl FromStr for u32`, `u64`, `usize`, `i64`, `i8`, `u8`, `i16`, `u16`
- Optionally `impl FromStr for f64` (needs `f64` parsing; check whether
  `split_float_literal` / `f64` arithmetic is usable in `.g` source)

The same i64-accumulator + range-check pattern generalises. Each impl is a
few lines. Also consider a `+`-sign prefix if user code will want it (Rust
accepts it).

### 2. Sweep remaining `Option<ExprId>` returns for silent drops

The session-4 fix made the two biggest silent-drop sites loud. The
remaining `return None` sites in `lower_expr.rs`:

- `line 800` -- `lower_binary_expr` `expr_children.len() < 2`
- `line 888` -- `lower_if_expr` `children.len() < 2`
- `line 984` -- `lower_path_expr` `segments.is_empty()`
- `line 1677` -- `lower_while_expr` `children.len() < 2`
- `line 1751` -- `lower_for_expr` `children.len() < 2`
- `line 1882` -- `lower_index_expr` `children.len() < 2`
- `lines 1921, 1948` -- array-repeat malformed; **already pushes a diag**

The 800/888/1677/1751/1882 sites are *structural* guards for malformed
parser output (which the parser does not currently produce). They are
probably safe to leave as-is, but a defensive `debug_assert!` or an
`internal_error` diagnostic at each would close the loop. `line 984` in
`lower_path_expr` is worth a look: an empty path can come from a `UsePath`
node the parser emitted with no `Ident` child (see the pattern of
`path_as_name`).

### 3. Session-3 leftovers (still relevant)

- **Never lower `Ty::ERROR` at codegen** -- `v15_t25_drop_error_type` is a
  `#[should_panic]` contract.
- Every `thir::Expr::err(span)` should be paired with
  `diagnostics.push(...)`.
- **Do not `git stash pop` a stale stash without checking `git stash
  list`.** One stale stash from before this branch is still there:
  `stash@{0}: WIP on main: 155188ea chore: up logo`. It has *not* been
  popped in sessions 3 or 4.

## Tooling / environment notes (carried forward)

- The CLI does not accept `--emit=check`; valid emits are `obj`, `exec`,
  `mir`, `llvm-ir`, `asm`, `cdylib`. Use `--emit=obj -o /tmp/x.o path.g`.
- `--with-stdlib` prepends the *minimal* assembled stdlib. Full stdlib is
  not currently a CLI flag; the full variant is tested directly by
  `glyim-pipeline/tests/stdlib_full_probe.rs`.
- MIR logs `ERROR glyim_mir: Place::ty(): Field projection on non-tuple/ADT
  type` etc. on **every** stdlib compile -- **pre-existing tracing noise**,
  not a failure. `grep -v '^2026-'` to filter. Do not chase these; they are
  a separate problem.
- Test fixtures:
  - `crates/glyim-hir/src/tests/*.rs` -- in-crate lowering unit tests
    (the `kitchen_sink` test lives here, alongside `lower_expr_tests.rs`).
  - `crates/glyim-typeck/tests/compile-pass/*.g` -- must compile.
  - `crates/glyim-cli/tests/*.rs` -- end-to-end CLI regression tests
    (`Command::new(env!("CARGO_BIN_EXE_glyim-cli"))`).

## Working-style constraints (still in force)

- Never lower `Ty::ERROR` at codegen.
- Every `thir::Expr::err(span)` should be paired with `diagnostics.push(...)`.
- Conventional commit prefixes; document remaining blockers in this file.
- Do not `git stash pop` without checking `git stash list`.
- When chasing a diagnostic, reproduce the *exact* failing input against a
  freshly built binary before bisecting.

## Lesson worth carrying forward

Silent drops in HIR lowering are the highest-leverage class of bug to
eliminate. Every `Option<ExprId>` return that a caller treats as "just skip
this" is a potential phantom-diagnostic generator hundreds of lines away.
The session-3 byte-literal bug is one instance; the session-4 fix makes two
sites loud and adds a kitchen-sink test that would catch the next one at the
boundary. When in doubt, prefer a loud `internal_error` at the lowering site
over a silent `None` propagation.
