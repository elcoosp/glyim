# Glyim Compiler — Master Agent Context

## Project Overview
Glyim is a from‑scratch compiler for a Rust‑like language, written in Rust. The codebase uses **26 crates** organised in layers (edition 2024, resolver = "3").  
You are implementing one stream of work within this project.

## Architecture Rules (NON‑NEGOTIABLE)

1. **No `pub` signature changes.** If a public type or function exists in a crate you don't own, you MAY NOT modify it. If you need a change, add a `pub(crate)` helper instead.
2. **No new `pub` items in existing modules** without explicit approval. You MAY add new private modules and `pub(crate)` items freely.
3. **No `unsafe` in compiler crates.** Only `glyim-runtime` may contain `unsafe`, and each block must have a `// SAFETY:` proof.
4. **No `todo!()` in non‑test code.** Use `tracing::warn!("STUB: {reason}")` for optional paths that need attention but won't crash.
5. **All stubs must be visible.** Silent no‑ops (empty match arms, `let _ = x`) are forbidden in implementation code. Every stub must emit a warning on first execution.
6. **Tracing convention:** `trace` for hot paths, `debug` for inference, `info` for phases. Always `skip(self, ctx)`.
7. **Test‑first:** Write all test cases from your stream's TDD plan **before** implementing. Tests must compile before implementation begins.
8. **Shared files are append‑only.** Files that multiple streams may modify — especially `src/tests/mod.rs` and `src/lib.rs` — MUST be modified using the safe‑append pattern (Python script that reads existing content and only adds new lines if absent). NEVER use `cat >` (overwrite) on these files. Overwriting `tests/mod.rs` silently deletes other streams' test registrations.

## Output Protocol: `glyim-ops` Blocks

Your **entire response** MUST be a single Markdown code block with the language `glyim-ops`.  
Inside the block, use these directives:

- `::WRITE <path>` – Write the full content of a file. Follow with the content, then `::END` on its own line.
- `::REPLACE <path>` – Replace a section of a file. Use `---FIND---`, then the exact text to replace, then `---REPLACE---`, then the new text, then `::END`.
- `::DELETE <path>` – Delete a file.
- `::COMMIT "message"` – MUST be included after every set of file modifications. The commit message should describe the changes.
- `::DONE` – Signal that the task is complete and the code is ready for review.
- `::INCOMPLETE` – Signal that you need another turn (e.g., response cut off).
- `::APPROVED` – Approve a self‑review (used after `::DONE`).

Example:

```glyim-ops
::WRITE src/lib.rs
fn add(a: i32, b: i32) -> i32 { a + b }
::END
::COMMIT "Add add function"
```

Do not output any text outside the code block.

## Code Quality Mandate (NON‑NEGOTIABLE)

Every piece of Rust code produced in any stream is subject to a ruthless review across six dimensions. Before committing, mentally run the following checklist. **If any item fails, fix it — do not ship.**

The target bar is: *a senior principal engineer reads the diff and thinks "this is clean, correct, and maintainable."*

### 1. Correctness & Compilation Safety

**Goal:** The code must compile with `cargo check --all-targets` and `cargo clippy -- -D warnings` with zero errors and zero warnings. Logic must be provably correct, not just "probably fine".

**Rules:**

- **Exhaustive `match`.** Every `match` on an enum must cover all variants explicitly. Do not use a catch‑all `_` arm unless the remaining variants are genuinely identical *and* you leave a comment explaining why.
- **No integer arithmetic without overflow consideration.** Use `checked_add`, `saturating_add`, or `wrapping_add` where overflow is conceivable. Bare `+` on `usize`/`u32` is forbidden in index arithmetic.
- **No silent lossy conversions.** `as usize` and `as u32` casts are banned unless the value is provably within range; use `try_from` or a named conversion function with a documented panic/error contract.
- **No `unwrap()` / `expect()` in non‑test, non‑infallible code.** An `expect()` is allowed only when the caller can *statically prove* the `Option`/`Result` is `Some`/`Ok` — add an `// INVARIANT: <reason>` comment on the same line. Use `?` or propagate `GlyimDiagnostic` otherwise.
- **No logic inversion bugs.** Every `if !cond` or `unless` pattern must have a unit test that exercises *both* branches. If you write a predicate, write a test that catches a sign flip.
- **All error paths return `GlyimDiagnostic`.** Never swallow an error into `()` or a default value. If recovery is intentional, document it with `// RECOVERY: <why this is safe>`.
- **Off‑by‑one discipline.** Spans, slices, and index ranges must use half‑open intervals `[lo, hi)` consistently. Any closed range must be called out with a comment.
- **No dead code.** `#[allow(dead_code)]` is forbidden in production paths. If a function is not yet called, it is not yet needed — don't write it.

**Self‑check before committing:**
```
cargo clippy --all-targets -- -D warnings
cargo test --all
```

### 2. Boundaries & Contracts

**Goal:** Every public and `pub(crate)` item is a *contract*, not an implementation detail. A caller should never need to read the body to use the function correctly.

**Rules:**

- **Document every `pub` and `pub(crate)` item.** At minimum: what it does, what it expects (preconditions), what it returns (postconditions), and what it emits (side effects: diagnostics, tracing spans, mutations). Use `///` doc comments, not `//`.
- **State preconditions as `debug_assert!`.** If a function requires `idx < self.len()`, add `debug_assert!(idx < self.len(), "…")` as the first line.
- **No leaking internals.** A `pub(crate)` function must not return a `&mut` to an internal field that callers can corrupt.
- **Infallible vs. fallible is a type-level choice.** A function that can fail returns `Result<T, GlyimDiagnostic>` or `CompResult<T>`. A function that panics on contract violation is documented as such. Never mix the two silently.
- **No God functions.** Any function longer than ~60 lines or with cyclomatic complexity > 10 must be decomposed.
- **`Default` must be meaningful.** If you `#[derive(Default)]`, the default value must be a valid, usable instance — not a half‑initialised placeholder.

### 3. Modularity & Separation of Concerns

**Goal:** Each module does one thing. Dependencies point inward (toward stable abstractions), never outward (toward volatile implementations).

**Rules:**

- **One concept per module.** A `.rs` file that handles both parsing *and* type inference, or both lowering *and* codegen, is wrong. Split it.
- **Acyclic module graph.** Within a crate, module `A` importing from module `B` which imports from `A` is forbidden.
- **Traits as seams.** Any place where two subsystems communicate, the dependency must flow through a trait, not a concrete type.
- **No ambient state.** No `static mut`, no `thread_local!` in compiler logic, no hidden global singletons. All state flows through explicit parameters.
- **Feature cohesion.** If you add a helper function, it belongs in the module whose *data* it primarily operates on.
- **Test isolation.** Every non‑trivial function must be testable without spinning up the entire compiler pipeline.

### 4. Performance & Resource Efficiency

**Goal:** Compiler performance is a feature. Allocate intentionally; never accidentally.

**Rules:**

- **No O(n²) in disguise.** Nested loops over compiler-managed collections must be justified.
- **Intern aggressively.** Strings, types, and substitutions that escape a single function call must be interned.
- **Avoid cloning across hot paths.** `Clone` on a non‑`Copy` type in a path called per‑expression is a red flag.
- **Pre‑allocate collections.** When the approximate size is known, use `with_capacity`.
- **Bound recursion depth.** Every recursive function that follows user‑provided structure must track depth and return an error diagnostic at a configurable limit (default 128).
- **No redundant traversals.** If you need two properties of the same node, compute them in one pass.

### 5. Debuggability & Observability

**Goal:** When something goes wrong in a 50,000‑line compilation, a developer must be able to trace it in under five minutes.

**Rules:**

- **Every phase boundary gets an `info!` span.** Use `tracing::info_span!("phase_name", crate = %name)`.
- **Every non‑trivial decision gets a `debug!` log.** Type inference decisions, trait resolution results, and MIR lowering choices must be logged at `debug` level.
- **Diagnostics carry full context.** A `GlyimDiagnostic` must include the source span, the message, and at least one note explaining *why* the error occurred.
- **No silent fallbacks.** If a lookup fails and you substitute a sentinel (`Ty::ERROR`), emit a `tracing::debug!` log at that point.
- **Assertions in debug builds.** Use `debug_assert!` liberally on invariants.
- **`Display` on all domain types.** Every type that appears in a diagnostic message must implement `std::fmt::Display`.

### 6. Elegance & Hack‑Free Design

**Goal:** The implementation is the simplest correct solution. No clever tricks. No "temporary" workarounds. No accidental complexity.

**Rules:**

- **No `#[allow(...)]` without a justification comment.**
- **Builder or context struct over long argument lists.** More than 4 parameters is a code smell.
- **No stringly‑typed logic.** Never branch on `&str` or `String` values for compiler-internal decisions. Use enums.
- **Pattern-match, don't interrogate.** Prefer `if let Some(x) = opt { … }` and `match` over `.is_some()` followed by `.unwrap()`.
- **No boolean traps.** Use enums instead of `bool` parameters.
- **Newtype wrappers for distinct indices.** Never use bare `u32` or `usize` as an index into more than one collection.
- **Derive traits, don't implement them manually unless necessary.**
- **No speculative code.** Do not implement functionality that is not required by the current stream's TDD plan.

---

## Applying the Mandate: Workflow

When implementing a feature or fixing a bug, follow this order:

1. **Plan** — write out the types, traits, and function signatures in comments before writing bodies.
2. **Test** — write the tests per the TDD plan. They must compile (but can fail) before implementation.
3. **Implement** — write the minimum correct implementation that makes the tests pass.
4. **Refactor** — clean up: rename, extract helpers, add doc comments, add `debug_assert!`s, add tracing spans.
5. **Verify** — `cargo clippy -- -D warnings` and `cargo test --all` must both pass clean.
6. **Review yourself** — read the diff as if you are a merciless senior reviewer. Would you approve it? If not, fix it before committing.

**If you are uncertain whether a design decision meets this mandate, the answer is: make it simpler, more explicit, and better typed. Complexity is never the right default.**

---

## Crate Dependency Rules

- Frontend crates (`glyim-syntax`, `glyim-frontend`, `glyim-hir`, `glyim-def-map`, `glyim-meta`) **never** depend on `glyim-type`.
- `glyim-lsp` depends **only** on `glyim-db` (no LLVM transitive dependency).
- `glyim-db` does **not** use Salsa (removed for v0.1.0 honesty).
- `TyCtxMut` is `!Send + !Sync`. Post‑typeck, only `TyCtx` (frozen, `Send + Sync`) is used.

## Key Type Contracts

- `Ty` **does not** implement `IdxLike`. Use sentinels: `Ty::ERROR`, `Ty::NEVER`, `Ty::UNIT`, `Ty::BOOL`.
- `TypeLookup` trait bridges `TyCtx` and `TyCtxMut` for display and flag computation.
- `InferVar` has separate index types: `TyVar`, `IntVar`, `FloatVar`.
- `compute_flags` is generic over `TypeLookup`.
- `Substitution` is interned. Access via `ctx.substitution_args(sub)`. Construction via `TyCtxMut::intern_substitution()`.
- `Place::ty()` takes `&impl TypeLookup` and `&IndexVec<LocalIdx, LocalDecl>`.
- `InferenceTable::new_ty_var`, `new_int_var`, `new_float_var` all take `&mut TyCtxMut`.
- `SimpleTraitSolver::new` takes `&TraitContext`.
- `Database::new` takes `CrateConfig { name: String, target_triple: String, opt_level: u8 }`.

## Error Handling

- Use `GlyimDiagnostic` for all errors. Constructors: `lex_error`, `parse_error`, `type_error`, `borrow_error`, `internal_error`.
- `DiagSink` collects diagnostics with error limiting (default 50).
- `CompResult<T> = Result<T, Vec<GlyimDiagnostic>>`.

## Git Convention

- Branch: `stream-{XX}/v0.1.0` (e.g., `stream-S01/v0.1.0`)
- Commit format: `stream-{XX}: description` (e.g., `stream-S01: feat(lex): add float exponent scanning`)
- **Do NOT commit to `main` directly.** Create a PR.

---

**This master context is authoritative.** Always refer to it when uncertain about project rules or conventions.
