# Contributing to Glyim

Thanks for your interest. This file covers the minimum you need to
build, test, and submit changes.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust toolchain | ≥ 1.95 (see `rust-version` in `Cargo.toml`; the `msrv` CI job enforces it) | `rustup toolchain install stable` |
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

### Toolchain note: inference strictness on rustc ≥ 1.99

The `msrv` floor is 1.95, but local `stable` tracks current rustc, which has
gotten stricter about closure-parameter inference: a `map_err(|e| …)` that
used to infer `e` from the *annotated binding* on the other side of the `?`
(`let mode: TestMode = value.parse().map_err(|e| …)?;`) now fails with E0282
on rustc 1.99. When you hit that, annotate the closure parameter explicitly
(`|e: TestModeError|`) instead of restructuring the call — see
`crates/glyim-test/src/annotations/directives.rs` for the one site that
needed it. Both spellings compile under the MSRV.

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

Those are for interactive sessions driven by `watchexec`; the
`./wr1.sh`..`./wr5.sh` scripts they invoke are **uncommitted local
tooling**, so the recipes fail on a fresh clone. Ignore them unless you
are contributing to the dev-loop tooling itself.

## Coding style

- `rustfmt` is enforced (`just fmt-check`). No per-crate `rustfmt.toml`.
- `clippy` at `-D warnings` (`just clippy`). The workspace also runs
  `clippy::pedantic` as the review baseline (see `[workspace.lints.clippy]` in
  the root `Cargo.toml`). **Do not add inline `#[allow]`/`#[expect]`** — the
  metrics ratchet (`metrics/baseline.json`, `inline_allows: 0`) fails any PR
  that introduces one (`cargo run -p glyim-metrics -- . --check
  metrics/baseline.json` in CI's `metrics-ratchet` job). If a lint is genuinely wrong
  for this codebase, add a rationale-annotated relaxation to
  `[workspace.lints.clippy]` in the root `Cargo.toml` (one reviewable place;
  gates never weaken, plan C9).
- **The one inline-allow exception:** inline test modules that assert on
  panic paths may carry the crate-root
  `#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used,
  clippy::panic))]` pattern (used in `glyim-typeck`, `glyim-hir`,
  `glyim-mir`, `glyim-frontend`, `glyim-solve`, `glyim-meta`, and
  `glyim-borrowck`). It is `cfg_attr(test, …)`-gated (never compiled into
  shipped code) and deliberately invisible to the `inline_allows` ratchet,
  which counts only shipped-code `#[allow(` lines; use it sparingly and
  never in non-test code.
- **No `#![allow(missing_docs)]`.** The workspace sets
  `missing_docs = "warn"` and every public item is documented. Add a
  doc comment (even a short one) to each public struct, enum, fn,
  field, variant, and module.

## Testing discipline

- **Every bug fix gets a fixture.** Put it under
  `crates/glyim-test/tests/run-pass/` (or `run-fail/`,
  `compile-fail/`) with the canonical M1.4 `//@` header — `//@ mode:`,
  `//@ expect-exit:` / `//@ expect-stdout:` / `//@ expect-stderr:` for
  run fixtures, `//@ expect-error:` or an inline `//~` annotation for
  `compile-fail`. Every fixture also carries `//@ feature:` and
  `//@ backends:` registry tags, and the collector rejects fixtures
  without a `//@` header. The legacy `// key: value` spelling parses
  but must not be mixed with it; `cargo run -p glyim-test --bin
  glyim-tag-fixtures -- --write` converts old headers mechanically.
  The fixture is the regression guard — a fix that is not covered by
  one will regress.
- **Surgical fixes only.** If a change touches more than ~4 files,
  split it into a series of small PRs. Each PR should pass the full
  suite on its own.
- **No `unwrap`/`expect`/`panic!` in non-test code.** Every site
  reachable from user input returns a `GlyimDiagnostic::internal_error`
  instead. There are ~300 legacy sites; don't add more.

## Development gotchas

Three caching layers sit between you and the source. Each has cost a
debugging session; none is obvious from a failing build.

### `kache` replays stale artifacts — disable it when bisecting

`RUSTC_WRAPPER=kache` is set in the *maintainer's* default dev
environment (it is not part of this repo's `.cargo/config.toml` — if
your machine does not have `kache`, this section does not apply).
`kache` caches compilation artifacts keyed on inputs it computes itself —
**`cargo clean` does not invalidate it**. If you apply a patch, run a
test, revert the patch, and run again, the second run may replay the
*patched* artifact.

This produces results that cannot be reconciled with the source on
disk: an "isolated test passed, full test failed" split where both
runs used the same code, a `[T0001]` diagnostic naming a line that
cannot produce it, a compile error from a function you just deleted.

**When a bisect produces contradictory results, do one build with
`RUSTC_WRAPPER=""`:**

```bash
RUSTC_WRAPPER="" cargo build -p glyim-cli
RUSTC_WRAPPER="" cargo nextest run -p glyim-test --test run_pass_corpus
```

For the full picture, combine with `cargo clean -p <crate>` on the
chain — but see the next entry first.

### `cargo clean -p` on the type chain removes the runtime staticlib

`cargo clean -p glyim-type -p glyim-typeck` (or any clean that reaches
`glyim-runtime`) removes `target/<profile>/libglyim_runtime.a`. That
file is not rebuilt by `cargo build -p glyim-cli` — it is the
`glyim-runtime` crate's staticlib output, built separately.

Five `--emit=exec` tests fail with `undefined reference to
glyim_stdout_write` after any such clean, and nothing in the failure
says "rebuild the runtime crate". (`glyim-cli` now annotates this at
link time — KNOWN_GAPS §3.5.26 — but the fix is:

```bash
cargo build -p glyim-runtime
```

### Long corpus runs time out under load

`run_pass_corpus` runs the ~410 tagged fixtures end-to-end (375 in
the `glyim-test` corpora plus the per-crate corpora of
`glyim-lower`/`glyim-typeck`/`glyim-pipeline`/`glyim-lang-*`), each
spawning a compile and execute. A full `cargo nextest run --workspace`
takes
5-6 minutes on an idle machine and 8-10 under load. The per-test
timeout is 60 seconds, and `run_pass_corpus_passes` is a single test
that fans out internally — so a `TimeoutExceeded` on a *fixture* means
the machine is loaded, not that the fixture is broken.

Check `uptime` before treating a random corpus failure as a
regression. Load average above ~20 on an 8-core box is enough to
produce spurious timeouts.


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

