# Contributing to Glyim

Thanks for your interest. This file covers the minimum you need to
build, test, and submit changes.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust toolchain | ≥ 1.85 (see `rust-version` in `Cargo.toml`) | `rustup toolchain install stable` |
| LLVM | 23 | Required by `glyim-codegen-llvm` (`inkwell`) |
| `cargo-nextest` | any recent | `cargo install cargo-nextest --locked` |

### LLVM 23

The LLVM backend is a **hard dependency** of the pipeline — there is no
feature flag to build without it. Install LLVM 23 and point the build at
it via `LLVM_SYS_231_PREFIX`:

```bash
# macOS (Homebrew)
export LLVM_SYS_231_PREFIX="$(brew --prefix llvm@23)"

# Debian / Ubuntu
export LLVM_SYS_231_PREFIX=/usr/lib/llvm-23
```

Add this to your shell profile if you build often.

### Building

```bash
cargo build --workspace
```

Or via the project `justfile`:

```bash
just build         # debug
just build-release # release
just check         # typecheck only
just clippy        # lints with -D warnings
just fmt           # rustfmt
```

## Testing

The project uses `cargo-nextest` for the full suite and a set of
per-corpus gates. **Run every gate before submitting a PR**, because
each one catches a distinct bug class:

```bash
just test          # full workspace (≈5 minutes on a laptop)

# The four corpora — each asserts a different property:
just test-corpus   # run_pass / compile_pass / run_fail / compile_fail
```

### The differential gate

`glyim` runs the LLVM backend and the MIR interpreter on the same
fixtures and asserts they agree. This catches backend divergences
that would otherwise ship silently.

```bash
just differential
```

You need a working `llvm-lto2` for the ThinLTO portion of the diff
(the release install of LLVM 23 ships one).

### The `wr1..wr5` recipes

Those are for interactive sessions driven by watchers; ignore them
unless you are contributing to the dev-loop tooling itself.

## Coding style

- `rustfmt` is enforced (`just fmt-check`). No per-crate `rustfmt.toml`.
- `clippy` at `-D warnings` (`just clippy`). If a lint is genuinely
  wrong for this codebase, add an `#[allow(..., reason = "...")]` at
  the narrowest scope with a comment.
- **No `#![allow(missing_docs)]`.** The workspace sets
  `missing_docs = "warn"` and every public item is documented. Add a
  doc comment (even a short one) to each public struct, enum, fn,
  field, variant, and module.

## Testing discipline

- **Every bug fix gets a fixture.** Put it under
  `crates/glyim-test/tests/run-pass/` (or `run-fail/`,
  `compile-fail/`) with the appropriate `// test-mode:`,
  `// exit-code:`, and `// compile-flags:` header. The fixture is the
  regression guard — a fix that is not covered by one will regress.
- **Surgical fixes only.** If a change touches more than ~4 files,
  split it into a series of small PRs. Each PR should pass the full
  suite on its own.
- **No `unwrap`/`expect`/`panic!` in non-test code.** Every site
  reachable from user input returns a `GlyimDiagnostic::internal_error`
  instead. There are ~300 legacy sites; don't add more.

## Commit messages

Conventional Commits, one logical change per commit:

```
feat(codegen-llvm): lower Vec::clone to slice_to_vec
fix(typeck): bind struct-variant pattern fields by name, not position
refactor(pipeline): compile_file_to_mir delegates to prepare_compilation
docs(gaps): §3.6 — P2-B1 investigated, fallback is defensive-only
chore(glyip): drop the dead `llvm` cargo feature
```

The body should explain **what was broken**, not what the code does.
Cite the KNOWN_GAPS section number where relevant.

## Where to send changes

- Bugs and feature requests: the GitHub issue tracker.
- Security issues: see [`SECURITY.md`](SECURITY.md) — **do not open a
  public issue**.
- Pull requests: `main` is the integration branch. Keep PRs squashed
  to one commit unless review requested otherwise.

## Known gaps

`KNOWN_GAPS.md` is the authoritative list of "things we know are
incomplete". Read the relevant section before starting a
compiler-internals change; several gaps are documented with a
"don't repeat this attempt" note after a previous session tried it
and reverted.

