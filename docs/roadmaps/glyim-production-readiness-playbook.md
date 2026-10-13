# Glyim Production-Readiness Playbook

**An executable correctness-hardening guide for an autonomous coding agent.**

Target repository: `https://github.com/elcoosp/glyim` (a from-scratch Rust compiler for a Rust-like language, 33 crates, ~4,600 tests)
Focus: **compiler correctness** — no internal compiler errors (ICEs) on user input, no silent miscompilations, CI that actually proves things, and a release a human can install.
Assumed environment: Rust stable toolchain, LLVM 23 dev packages, `LLVM_SYS_231_PREFIX` exported. You do **not** need to set up the environment.

Version: 1.0 — compiled from a full parallel audit of the codebase (panic audit, pipeline-gap audit, test/CI audit, packaging/unsafe audit). Every file path, line number, and code snippet below was verified against the repository at commit `60452ca6` ("fix(runtime,typeck): differential reaches 0 disagreements").

---

## Table of contents

- [Part I — Rules of engagement](#part-i--rules-of-engagement-read-before-doing-anything)
- [Part II — Ground truth](#part-ii--ground-truth-what-this-codebase-actually-is)
- [Part III — Findings register](#part-iii--findings-register-the-gap-analysis)
- [Part IV — Execution phases](#part-iv--execution-phases)
  - [Phase 1 — Baseline and hygiene](#phase-1--baseline-and-hygiene)
  - [Phase 2 — CI repair](#phase-2--ci-repair)
  - [Phase 3 — ICE hardening (correctness core)](#phase-3--ice-hardening-eliminate-the-panic-surface-correctness-core)
  - [Phase 4 — Differential testing and fuzzing](#phase-4--differential-testing-and-fuzzing)
  - [Phase 5 — Packaging and release engineering](#phase-5--packaging-and-release-engineering)
  - [Phase 6 — Final production-readiness gate](#phase-6--final-production-readiness-gate)
- [Appendix A — Deferred correctness backlog](#appendix-a--deferred-correctness-backlog-s3-explicitly-out-of-this-run)
- [Appendix B — Environment variables and features reference](#appendix-b--environment-variables-and-features-reference)
- [Appendix C — Sources](#appendix-c--sources)

---

## Part I — Rules of engagement (read before doing anything)

You are an execution agent. This document is your complete specification. Follow these rules exactly:

- **R1 — Sequential phases.** Execute Phase 1 → 6 in order. Never start a phase before the previous phase's verification gate passes.
- **R2 — Verify every step.** Every step ends with a **Verify** block containing shell commands and their expected output. Run the commands. If actual output does not match expected output, STOP. Do not improvise. Re-read the step, re-apply it, and if it still fails, record the failure and stop the run.
- **R3 — Copy code verbatim.** Code blocks labeled **WRITE FILE** or **PATCH** must be applied exactly as printed. Do not "improve" them, reformat them, or rename identifiers.
- **R4 — One commit per step.** After each step's verification passes, run:
  ```bash
  git add -A && git commit -m "<step id>: <one-line description>"
  ```
  Example: `git add -A && git commit -m "3.1: add recursion depth guard to parse_type"`. If a later step breaks something, `git revert` that step's commit — do not hand-rollback files.
- **R5 — Never touch files not named in a step.** If you believe another file needs changing, add it to the "Deferred backlog" appendix note in your final report instead of changing it.
- **R6 — Tests are the contract.** The workspace test suite is the single source of truth. Any step that makes a previously-passing test fail is a broken step, even if the step's own Verify block passes.
- **R7 — Idempotency.** If a step was already applied (e.g., the file already contains the change), verify and move on. Do not apply a change twice.
- **R8 — Report.** At the end, produce a short report: which phases completed, which verification commands were run, and any deviations.

**How to understand the Verify blocks.** `exit=0` means the command must exit with status code 0. "contains X" means the string X must appear somewhere in stdout/stderr. "not contains X" means it must not.

---

## Part II — Ground truth: what this codebase actually is

Before changing anything, internalize these facts. They were established by reading the code, not by trusting the project's own documents (several of which have drifted).

### 2.1 The machine

- Cargo workspace, Rust 2024 edition, resolver 3, 33 crates under `crates/`, one tool crate `tools/glyim-pilot` (an agent orchestrator — **out of scope, do not modify it**).
- Pipeline: `glyim-frontend` (lexer+parser → Rowan CST) → `glyim-def-map` (name resolution) → `glyim-meta` (macro expansion) → `glyim-hir` → `glyim-type` + `glyim-solve` + `glyim-typeck` (inference + trait solving) → `glyim-lower` (THIR→MIR, monomorphization) → `glyim-borrowck` (NLL) → `glyim-opt` (MIR passes) → `glyim-codegen` (bytecode backend) / `glyim-codegen-llvm` (LLVM 23 via inkwell). `glyim-pipeline` drives it; `glyim-cli` is the `glyim` driver binary; `glyip` is the Cargo-like build tool; `glyim-mir-interp` is the interpreter; `glyim-runtime` builds the `libglyim_runtime.a` staticlib that `--emit=exec` programs link against.
- LLVM coupling: `inkwell` pinned to git rev `c9d9734…` with feature `llvm23-1`, plus a direct `llvm-sys = "231.0.1"` dependency. Discovery via `LLVM_SYS_231_PREFIX`. The LLVM backend is **not** feature-gated — it is a hard dependency of the pipeline.
- `panic = "unwind"` in `[profile.release]` is **mandatory** (the runtime and proc-macro host code catch panics across FFI). Never switch to `panic = "abort"`.
- Test suite: ~4,641 `#[test]` functions; `cargo nextest run --workspace` is the canonical run. The harness crate `glyim-test` discovers `.g` fixtures in five modes (compile-pass / compile-fail / ui / run-pass / run-fail), supports inline `//~ ERROR` annotations, and has two gates that only run with the real LLVM backend: `--test runtime` (M5 async proof) and `--test differential` (LLVM-native vs MIR-interpreter agreement over 99 run-pass fixtures).
- Environment variables the harness reads: `GLYIM_BLESS` (regenerate UI snapshots), `GLYIM_TEST_SHOW_OUTPUT=1` (verbose), `GLYIM_TEST_JSON` (JSON reporter), `GLYIM_TEST_REAL_LLVM` (swap mock codegen for the real LLVM backend), plus Cargo feature `glyim-test/real-llvm`.

### 2.2 Trust corrections (documents lie, code is truth)

| Claim found in the repo | Reality |
|---|---|
| README says clone `github.com/elcoosp/glyim-v2` | Actual repo: `elcoosp/glyim` |
| README says binary is `target/release/glyim` | Actual binary: `target/release/glyim-cli` |
| README says "build with `--no-default-features` to skip LLVM" | False: LLVM is a hard dependency of pipeline/cli; no feature gate exists |
| ICE message points to `elcoosp/glyim-v2/issues` | Same wrong repo URL (`crates/glyim-cli/src/lib.rs:162`) |
| `docs/roadmaps/HANDOFF-session-11.md` lists 8 differential disagreements | Latest commit `60452ca6` states "differential reaches 0 disagreements" — the suite is currently green; this playbook locks that in with CI |
| `docs/TECHSTACK.md` lists salsa/chumsky/egg/… | None of those crates are in the tree; the doc is rotted |
| Some comments reference `docs/plans/v0.1.0/unstub-5/KNOWN_GAPS.md` | That file does not exist anywhere |
| CI file "corrupted" | False (a scanner artifact): `.github/workflows/ci.yml` is valid YAML. Its real defects are in §2.4 |

### 2.3 Panic surface (audit summary)

Non-test production code contains: **0** `todo!`, **0** `unimplemented!`, 21 `unreachable!` (invariant guards), 17 `panic!`, 136 `.unwrap()`, 109 `.expect(`. The `glyim-frontend` and `glyim-solve` crates are already panic-free in production code and carry crate-root ratchet lints; 4 crates have the ratchet (`frontend`, `hir`, `meta`, `typeck`), the rest do not. The CLI driver already wraps compilation in `catch_unwind` and produces an ICE report. The four biggest holes are all fixed in Phase 3 of this playbook.

### 2.4 CI reality (audit summary)

`.github/workflows/ci.yml` is valid YAML with 6 jobs (test-ubuntu, test-linux-runtime, test-macos, test-windows, miri-check, dead-code-check). Real defects:

1. **Ubuntu LLVM install is internally inconsistent** — `sudo ./llvm.sh 22` installs the LLVM 22 toolchain and repo, then the next line installs `llvm-23-dev libpolly-23-dev`. Stale leftover of the LLVM-22 era. Will break when the 23 dev packages can't resolve against the 22 repo.
2. **The strict runtime path is disabled** — `test-linux-runtime` runs without `GLYIM_TEST_REAL_LLVM=1` and without `--features glyim-test/real-llvm`, so the M5 async proof runs in mock-codegen tolerance mode and proves nothing about native codegen. The comment says "enable once the fn_abi_of gap is fixed" — the runtime.rs test header (2026-09-21) says that gap is closed.
3. **The differential suite never runs in CI** — it is feature+env gated and no job enables it. This is the project's single strongest silent-miscompile detector.
4. **No lint job** — `cargo fmt --check` and workspace `clippy -D warnings` (the project's own quality bar, encoded in `tools/glyim-pilot/src/gates/clippy.rs`) are not enforced anywhere.
5. **No supply-chain check** — no `deny.toml`, no `cargo audit`/`cargo deny` job.
6. **No concurrency group, no caching, no timeouts, no permissions block.**
7. `dead-code-check` cannot fail meaningfully (`-W dead_code` warnings don't fail builds).

### 2.5 Packaging reality (audit summary)

- **No LICENSE file** (despite `license = "MIT"` everywhere) — legal blocker.
- **No CHANGELOG.md, zero git tags, no release workflow.**
- No `rust-version` (MSRV) declared in any of the 34 Cargo.toml files.
- `crates/glyip/Cargo.toml` hardcodes `version = "0.1.0"` and `edition = "2024"` instead of inheriting from the workspace.
- Dead `[profile.dev]` blocks in 3 non-root manifests (`crates/glyim-cli/Cargo.toml:6`, `crates/glyim-runtime/Cargo.toml:10`, `crates/glyim-meta/Cargo.toml:6`) — Cargo ignores non-root profiles.
- A zero-byte file named `::` is tracked at the repo root (shell redirect typo).
- `.cargo/config.toml` hardcodes the macOS Homebrew LLVM path as an `[env]` default — a footgun on Linux.
- `--emit=exec` needs `libglyim_runtime.a`, found via `GLYIM_RUNTIME_LIB` env → `target/<profile>/` in CWD → executable-ancestor walk (`crates/glyim-cli/src/linker.rs:165-195`). A released binary needs the install script to place this and export `GLYIM_RUNTIME_LIB`.

---

## Part III — Findings register (the gap analysis)

Severity: **S1** = crash / wrong code reachable today, fix in this run. **S2** = proven-process gap (CI/packaging), fix in this run. **S3** = important but deferred (appendix A).

| ID | Sev | Finding | Evidence |
|---|---|---|---|
| P-01 | S1 | `parse_type` recursion is unguarded → deeply nested types (`&&&&…`, `A<A<…>>`) overflow the stack → SIGSEGV, uncatchable by the CLI's `catch_unwind`. All other parsers are guarded at `MAX_EXPR_DEPTH = 256`. | `crates/glyim-frontend/src/parser/ty.rs:6` (guard pattern exists at `expr.rs:30-45`, constant at `parser/mod.rs:45`) |
| P-02 | S1 | MIR-interpreter subslice: the `end` computation's bounds guard checks `to` but subtracts `from` → `usize` underflow (debug panic / release wrap → bogus slice length). | `crates/glyim-mir-interp/src/lib.rs:1954-1963` |
| P-03 | S1 | MIR-interpreter `byte_payload`: `"\u{0}".repeat(n)` with user-controlled `n` → unbounded allocation → OOM abort of the host process. | `crates/glyim-mir-interp/src/lib.rs:561` |
| P-04 | S1 | const-eval array/tuple index: `u128` index cast `as usize` truncates on 64-bit → index `2^64+3` aliases to `3` and reads an in-bounds element (wrong result, not an error). | `crates/glyim-const-eval/src/eval.rs:440-460` |
| P-05 | S2 | MIR-interpreter direct indexing `body.basic_blocks[bb_idx]` and `callee_locals[i + 1]` panic (instead of `InterpError`) if a compiler bug feeds an out-of-range index. | `crates/glyim-mir-interp/src/lib.rs:767,792,796,1001` |
| P-06 | S2 | `glyip test` runs the interpreter in-process with **no** `catch_unwind` — one fixture panicking kills the whole test run with a raw stack trace. | `crates/glyip/src/commands.rs:338` |
| P-07 | S2 | CLI ICE handler: no panic hook, so the default Rust panic message prints *in addition to* the ICE report; the ICE report omits the panic message payload; bug-report URL points to a nonexistent repo. | `crates/glyim-cli/src/lib.rs:140-168` |
| P-08 | S2 | codegen-llvm converts some recoverable LLVM build errors into `panic!` instead of `CompResult` diagnostics (`"fn type lowering failed"`, `"Codegen error"`, `"Variant index … out of bounds"`). | `crates/glyim-codegen-llvm/src/lower.rs:119,185,880` (correct pattern in use at `lower.rs:3441-3444`) |
| P-09 | S3 | Ratchet lints (`clippy::unwrap_used/expect_used/panic`) exist in only 4 of 33 crates although 3 more crates already satisfy them. | crate roots of `glyim-solve`, `glyim-borrowck`, `glyim-mir` |
| D-01 | S2 | The differential suite (99 fixtures, LLVM-native vs interpreter) and strict M5 runtime proof never run in CI. | `.github/workflows/ci.yml:47-69`; gates: `crates/glyim-test/tests/differential.rs`, `tests/runtime.rs` |
| D-02 | S2 | `String::as_str` native lowering identity claim ("both `{ptr, i64}`") is asserted in code comments but contradicted by session docs — needs a verification probe, not a blind patch. | `crates/glyim-codegen-llvm/src/lower.rs:3434-3453` |
| CI-01..07 | S2 | All defects listed in §2.4. | `.github/workflows/ci.yml` |
| PKG-01 | S2 | No LICENSE file. | repo root |
| PKG-02 | S2 | No MSRV, no CHANGELOG, no tags, no release workflow; glyip hardcodes version/edition. | 34 × Cargo.toml, `git tag` = 0 |
| PKG-03 | S2 | Dead `[profile.dev]` blocks in 3 non-root manifests; junk `::` file tracked. | §2.5 |
| PKG-04 | S2 | `.cargo/config.toml` hardcodes `/opt/homebrew/opt/llvm@23`. | `.cargo/config.toml` |
| PKG-05 | S3 | README doc bugs (URLs, binary name, feature-gate claim). | §2.2 |
| PKG-06 | S3 | No supply-chain policy (`deny.toml` absent), 414 packages, inkwell pinned by git rev (documented triple-lock: inkwell rev ↔ llvm-sys 231 ↔ system LLVM 23.1). | Cargo.lock |
| U-01 | S3 | 290 unsafe tokens in `glyim-runtime` (FFI) have mostly-correct SAFETY comments but no `unsafe_op_in_unsafe_fn` enforcement; `TypeArena` has `unsafe impl Send/Sync` resting on a doc-comment invariant. | `crates/glyim-runtime/src/lib.rs`, `crates/glyim-type/src/type_arena.rs:70-71` |
| X-01 | S3 | Deferred correctness backlog (real compiler bugs, out of scope for one run): sentinel builtins lowering to zero values on error arms; `ConstRef` constants materialized as zero globals; glyip cannot link executables (object-only, no runtime); ThinLTO drops all but the first CGU object; loop-`.await` (T0060); `include!` source inclusion; enum-variant value paths; parser trivia hazards FE-9/FE-11. See Appendix A. | referenced files in Appendix A |

---

## Part IV — Execution phases

### Phase 1 — Baseline and hygiene

**Goal:** prove the environment works, record the green baseline, remove junk, add the legal/metadata minimum. No behavior changes except hygiene.

#### Step 1.1 — Verify toolchain

```bash
cd /path/to/glyim
rustc --version && cargo --version
echo "PREFIX=$LLVM_SYS_231_PREFIX"
llvm-config --version 2>/dev/null || ls "$LLVM_SYS_231_PREFIX/lib/libLLVM*" | head -3
```

**Expected:** rustc prints a stable version ≥ 1.85 (2024 edition), the prefix is non-empty, and an LLVM 23 library is found. If `LLVM_SYS_231_PREFIX` is empty, STOP and ask the operator to export it (this playbook does not set up environments).

#### Step 1.2 — Build the compiler and the runtime

```bash
cargo build -q -p glyim-runtime -p glyim-cli -p glyip 2>&1 | tail -5
ls target/debug/libglyim_runtime.a target/debug/glyim-cli target/debug/glyip
```

**Expected:** exit=0; all three paths exist. (The runtime staticlib is required by `--emit=exec` and by the test harness.)

#### Step 1.3 — Record the test baseline

```bash
cargo install cargo-nextest --locked 2>/dev/null || true
cargo nextest run --workspace 2>&1 | tail -5
```

**Expected:** exit=0 and a summary line with **0 failed** (roughly 4,6xx passed; a few skipped is normal — e.g. Linux-gated M5 fixtures on macOS). Record the exact numbers; you will compare against them after later phases. This takes minutes on a warm cache. Do not proceed with any failing test.

#### Step 1.4 — Record the lint baseline

```bash
cargo fmt --all -- --check 2>&1 | head -20; echo "fmt_exit=$?"
cargo clippy --workspace --all-targets 2>&1 | grep -c "^warning" || true
```

**Expected:** ideally `fmt_exit=0`. If formatting differs, run `cargo fmt --all`, re-run Step 1.3, and commit as `1.4: apply rustfmt`. Note the clippy warning count; Phase 2 enforces `-D warnings`, so if the count is > 0 fix the mechanical warnings now (unused imports, needless clones, etc.). Do not silence warnings with `#[allow]` unless the warning is a false positive; prefer fixing the code.

#### Step 1.5 — Repo hygiene: remove the junk file and dead profile blocks

The repo root tracks a zero-byte file named `::` (an accidental shell redirect), and three non-root manifests contain `[profile.dev]` blocks that Cargo ignores (it emits a warning).

```bash
git rm '::'
rg -n "^\[profile" crates/glyim-cli/Cargo.toml crates/glyim-runtime/Cargo.toml crates/glyim-meta/Cargo.toml
```

**PATCH** — in each of the three files listed by the `rg` command, delete the entire `[profile.dev]` block (the section header plus its following `opt-level`/`debug` lines, up to the next `[` section header or EOF). Leave everything else untouched.

**Verify:**
```bash
test ! -e '::' && echo "junk removed"
cargo build -q -p glyim-cli 2>&1 | grep -c "profiles for the non-root package" || echo "0 profile warnings"
```
**Expected:** `junk removed` and `0 profile warnings`.

#### Step 1.6 — Add the LICENSE and MSRV

`license = "MIT"` is declared everywhere but no LICENSE file exists.

**WRITE FILE** `LICENSE` (MIT, full text, copyright holder = the repository owner):

```text
MIT License

Copyright (c) 2026 elcoosp

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

**PATCH** — in the root `Cargo.toml`, inside the `[workspace.package]` section, add this line directly under `license = "MIT"`:

```toml
rust-version = "1.85"
```

(2024 edition requires ≥ 1.85; the README recommends newer. This declares a build floor without constraining CI, which pins `stable`.)

**Verify:**
```bash
test -f LICENSE && grep -q 'rust-version = "1.85"' Cargo.toml && cargo metadata -q --no-deps >/dev/null && echo "metadata ok"
```
**Expected:** `metadata ok`.

#### Step 1.7 — Neutralize the hardcoded macOS LLVM prefix

**PATCH** — replace the entire `[env]` section of `.cargo/config.toml` with:

```toml
# LLVM discovery for llvm-sys/inkwell. Do NOT hardcode a machine-specific
# prefix here. Export LLVM_SYS_231_PREFIX in your shell (e.g. Homebrew:
#   export LLVM_SYS_231_PREFIX="$(brew --prefix llvm@23)"
# Debian/Ubuntu: export LLVM_SYS_231_PREFIX=/usr/lib/llvm-23
# CI sets it explicitly per runner.
[env]
LLVM_SYS_231_STRICT_VERSIONING = "0"
```

(Removing the `LLVM_SYS_231_PREFIX` line: `[env]` values do not override variables already set in the environment, so CI and macOS shells keep working; Linux shells now get a correct error message instead of a silent Homebrew path.)

**Verify:** `cargo build -q -p glyim-runtime && echo ok` — **Expected:** `ok` (your environment's prefix still applies).

#### Step 1.8 — Phase gate

```bash
cargo nextest run --workspace 2>&1 | tail -3
```
**Expected:** same pass/fail counts as Step 1.3. Commit: `1.x: baseline and hygiene (LICENSE, MSRV, junk removal, profile cleanup, cargo config)`.

---

### Phase 2 — CI repair

**Goal:** a CI file whose jobs prove real things: full test matrix, strict native-runtime proof, differential suite, formatting, clippy at the project's own quality bar, supply-chain, Miri. Replace the whole workflow file.

#### Step 2.1 — Replace `.github/workflows/ci.yml`

**WRITE FILE** `.github/workflows/ci.yml` with the following complete content (this preserves all six existing job concepts, fixes the LLVM install step, enables the strict runtime path, and adds four new jobs):

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

# Cancel superseded runs on the same ref to save runner minutes.
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

permissions:
  contents: read

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

# Reusable LLVM 23 install for Ubuntu runners.
# NOTE: history had `./llvm.sh 22` next to `llvm-23-dev` packages; the
# mismatch broke resolution. The script version and the package versions
# must both be 23.
x-llvm23-ubuntu: &llvm23-ubuntu
  run: |
    wget -q https://apt.llvm.org/llvm.sh
    chmod +x llvm.sh
    sudo ./llvm.sh 23
    sudo apt-get install -y llvm-23-dev libpolly-23-dev
    echo "LLVM_SYS_231_PREFIX=/usr/lib/llvm-23" >> "$GITHUB_ENV"

jobs:
  test-ubuntu:
    name: Test (ubuntu-latest)
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - name: Install LLVM 23
        <<: *llvm23-ubuntu
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Run tests
        run: cargo nextest run --workspace

  test-linux-runtime:
    name: M5 runtime proof — strict (ubuntu-latest)
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - name: Install LLVM 23
        <<: *llvm23-ubuntu
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Run M5 runtime fixtures (strict: real LLVM codegen + link + run)
        # STRICT PATH: build the fixtures with the real LlvmBackend, link
        # them natively, execute them, and REQUIRE Passed. The harness
        # tolerates mock-codegen `CompilationFailed` only when
        # GLYIM_TEST_REAL_LLVM is unset; setting it (plus the `real-llvm`
        # feature) makes CompilationFailed a hard failure. The runtime.rs
        # header (2026-09-21) records that the fn_abi_of codegen gap is
        # closed, so this must stay green.
        env:
          GLYIM_TEST_REAL_LLVM: "1"
        run: cargo nextest run -p glyim-test --test runtime --features glyim-test/real-llvm

  test-differential:
    name: Differential — LLVM native vs MIR interpreter (ubuntu-latest)
    runs-on: ubuntu-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - name: Install LLVM 23
        <<: *llvm23-ubuntu
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Compile every run-pass fixture with both backends and compare
        # The project's strongest silent-miscompile detector: each run-pass
        # fixture is compiled once, executed natively (LLVM) and in the MIR
        # interpreter, and exit code + stdout must agree. Status at
        # commit 60452ca6: 0 disagreements. Any disagreement is a release
        # blocker, not a flake.
        env:
          GLYIM_TEST_REAL_LLVM: "1"
        run: cargo nextest run -p glyim-test --test differential --features glyim-test/real-llvm

  test-macos:
    name: Test (macos-latest)
    runs-on: macos-latest
    timeout-minutes: 75
    steps:
      - uses: actions/checkout@v4
      - name: Install LLVM 23
        run: |
          brew install llvm@23
          echo "LLVM_SYS_231_PREFIX=$(brew --prefix llvm@23)" >> "$GITHUB_ENV"
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Run tests
        run: cargo nextest run --workspace

  test-windows:
    name: Test (windows-latest, runtime)
    runs-on: windows-latest
    timeout-minutes: 60
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Install nextest
        uses: taiki-e/install-action@nextest
      - name: Build runtime
        run: cargo build -p glyim-runtime
      - name: Run runtime tests (verifies Windows process signaling, Phase 6.4)
        run: cargo nextest run -p glyim-runtime

  miri-check:
    name: Miri (nightly, type/check crates)
    runs-on: ubuntu-latest
    timeout-minutes: 90
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: Swatinem/rust-cache@v2
      - name: Install Miri
        uses: taiki-e/install-action@miri
      - name: Run Miri on type-checking crates
        # Miri can't run LLVM-heavy crates; exercise the pure-Rust
        # analysis crates (glyim-type, glyim-borrowck, glyim-solve).
        run: |
          cargo miri test -p glyim-type
          cargo miri test -p glyim-borrowck
          cargo miri test -p glyim-solve

  lint:
    name: Format + Clippy (-D warnings)
    runs-on: ubuntu-latest
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: cargo fmt --check
        run: cargo fmt --all -- --check
      - name: cargo clippy
        # This is the project's own quality bar (tools/glyim-pilot
        # ClippyGate: every warning fatal, workspace-wide).
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Warn-fallback heuristics
        # Previously orphaned script: fails if a `tracing::warn!` in the
        # hot crates is followed by a silent numeric/None fallback.
        run: bash .github/scripts/check-warn-fallbacks.sh

  supply-chain:
    name: cargo-deny (advisories + licenses)
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Install cargo-deny
        uses: taiki-e/install-action@cargo-deny
      - name: cargo deny check
        run: cargo deny check

  dead-code-check:
    name: Dead-code burndown
    runs-on: ubuntu-latest
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - name: Check for dead code in codegen crates
        # Warnings are printed AND made fatal for these two crates only,
        # so the job can actually fail (the old version could not).
        run: |
          cargo clippy -p glyim-codegen --all-targets -- -W dead_code -D warnings
          cargo clippy -p glyim-frontend --all-targets -- -W dead_code -D warnings
```

> YAML note: the top-level `x-llvm23-ubuntu` key with the `&llvm23-ubuntu` anchor plus `<<: *llvm23-ubuntu` merge keys is valid YAML 1.2 merge syntax and is used by many large workflows. If your validator rejects merge keys, inline the three install lines into each of the two Ubuntu steps instead — the content is identical.

#### Step 2.2 — Add `deny.toml`

**WRITE FILE** `deny.toml` (complete content):

```toml
# cargo-deny policy. Run: cargo deny check
# CI job `supply-chain` enforces this on every push/PR.

[advisories]
version = 2
yanked = "warn"

[licenses]
version = 2
# The workspace is MIT. Allow the common permissive licenses its 400+
# transitive dependencies use. If a future dependency adds a license not
# listed here, cargo-deny fails the build — that is the intended tripwire:
# review the dependency, then extend this list deliberately.
allow = [
  "MIT",
  "MIT-0",
  "Apache-2.0",
  "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause",
  "BSD-3-Clause",
  "ISC",
  "Unicode-3.0",
  "Unicode-DFS-2016",
  "Zlib",
  "CC0-1.0",
  "CDLA-Permissive-2.0",
]
confidence-threshold = 0.93

[bans]
multiple-versions = "warn"
wildcards = "warn"

[sources]
unknown-registry = "warn"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
# inkwell is intentionally pinned to a git rev (see the triple-lock note in
# docs: inkwell rev <-> llvm-sys 231 <-> system LLVM 23.1).
allow-git = ["https://github.com/TheDan64/inkwell"]
```

#### Step 2.3 — Validate the workflow locally before pushing

```bash
python3 -c "import yaml,sys; d=yaml.safe_load(open('.github/workflows/ci.yml')); print(sorted(d['jobs'].keys()))"
python3 -c "import yaml; yaml.safe_load(open('deny.toml'))" 2>/dev/null || echo "deny.toml is TOML — checking with cargo instead"
cargo deny check 2>&1 | tail -3
```

**Expected:** the job list prints and includes `test-differential`, `lint`, `supply-chain`. `cargo deny check` must end with `error: 0` style summary or exit=0. If cargo-deny is not installed: `cargo install cargo-deny --locked`.

> Note: `deny.toml` is TOML, not YAML — the second python command is expected to fail; that is fine. Only the `cargo deny check` result matters.

If `cargo deny check` fails on licenses: do NOT delete the job. Read the listed license, and if it is a standard permissive license (OSI-approved), add its SPDX id to the `allow` list in Step 2.2 with a one-line comment naming the dependency. Commit that separately.

#### Step 2.4 — Prove the two newly-enforced suites pass locally

```bash
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test runtime --features glyim-test/real-llvm 2>&1 | tail -3
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential --features glyim-test/real-llvm 2>&1 | tail -3
```

**Expected:** both exit=0. The runtime test is 3 tests; differential is the 99-fixture corpus driver. If the differential run reports disagreements, STOP — you have found a real miscompile. Record the failing fixture names, the native vs interpreter outputs, and stop the run (this playbook does not fix codegen bugs; that is Appendix A territory).

#### Step 2.5 — Phase gate

```bash
python3 -c "import yaml; d=yaml.safe_load(open('.github/workflows/ci.yml')); print(len(d['jobs']), 'jobs')"
git add -A && git commit -m "2.x: repair CI (LLVM 23 install, strict runtime+differential proof, lint/supply-chain jobs, deny.toml, caching, timeouts)"
```
**Expected:** `9 jobs`.

---

### Phase 3 — ICE hardening: eliminate the panic surface (correctness core)

**Goal:** a hostile or unlucky `.g` input can no longer crash the compiler or the in-process interpreter; panics that remain are converted to diagnostics or covered by regression fixtures. Every fix ships with a test.

#### Step 3.1 — Guard `parse_type` recursion depth (P-01, S1: uncatchable SIGSEGV)

`crates/glyim-frontend/src/parser/ty.rs` — the `parse_type` function recurses at ~20 call sites (`RefType` arms, generic arguments, tuple/array element types) with **no** depth guard. A source file containing 100,000 `&` characters overflows the stack; a stack overflow is `SIGSEGV` and cannot be caught by the CLI's `catch_unwind`, so this defeats the entire ICE handler. The codebase already solved this for expressions (`expr.rs:30-45`) with a `recursion_depth` counter and a `MAX_EXPR_DEPTH = 256` constant (`parser/mod.rs:45`). Extend the same guard to types.

**PATCH 1** — `crates/glyim-frontend/src/parser/ty.rs`. At the very top of `pub(crate) fn parse_type(&mut self) {` (line 6), insert the guard so the function begins:

```rust
impl<'a> Parser<'a> {
    pub(crate) fn parse_type(&mut self) {
        // FE-6 sibling: types recurse just as hard as expressions
        // (`&&&&…T`, `A<A<A<…>>>`). Without this guard a deeply nested
        // type overflows the stack — a SIGSEGV the CLI's `catch_unwind`
        // cannot convert into an ICE report. Same policy as parse_expr:
        // emit a diagnostic, consume one token for forward progress,
        // and unwind to the caller.
        if self.recursion_depth > super::MAX_EXPR_DEPTH {
            self.error("type nested too deeply");
            if self.current().is_some() {
                self.bump();
            }
            return;
        }
        self.recursion_depth += 1;
        self.parse_type_inner();
        self.recursion_depth -= 1;
    }

    fn parse_type_inner(&mut self) {
```

The original body of `parse_type` (the `match self.current_kind() { … }` with all its arms, ending with the closing brace of the old function) now becomes the body of `parse_type_inner`. Do not modify anything inside the old body — every recursive `self.parse_type()` call inside it automatically goes through the guarded entry.

**PATCH 2** — add a regression fixture. **WRITE FILE** `crates/glyim-test/tests/no-ice/deep_type_nesting.g`:

```gly
// no-ice corpus: a 100,000-deep reference type must produce a parse
// diagnostic, not a stack overflow. Prior to the parse_type depth guard
// this crashed the compiler with SIGSEGV (uncatchable).
fn main() {
    let p: &&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&&i32 = 0;
    let _ = p;
}
```

**Verify:**
```bash
cargo build -q -p glyim-cli
python3 -c "print('fn main() { let p: ' + '&'*100000 + 'i32 = 0; }')" > /tmp/deep_type.g
./target/debug/glyim-cli /tmp/deep_type.g --emit mir 2>&1 | head -5; echo "exit=$?"
cargo nextest run -p glyim-frontend -p glyim-test 2>&1 | tail -3
```
**Expected:** the CLI prints a diagnostic containing `type nested too deeply` (or a parse-error summary mentioning it) and **exits normally** (`exit=1` is fine — a compile error — the process must NOT crash with `signal: 11` / `SIGSEGV` / `stack overflow`). All frontend and glyim-test tests still pass.

#### Step 3.2 — Fix the interpreter subslice underflow (P-02, S1: memory-corrupting arithmetic)

`crates/glyim-mir-interp/src/lib.rs:1954-1963` — in the `ProjectionElem::Subslice` handler, the `end` computation's bounds guard checks `to` but then subtracts `from`:

```rust
let end = if *from_end {
    if base_len < *to as usize {          // ← checks `to` …
        return Err(InterpError::Panic(format!(
            "subslice from end to {} out of bounds (len {})",
            to, base_len
        )));
    }
    base_len - (*from as usize)           // ← … subtracts `from`
} else {
    (*to) as usize                        // ← no bounds check at all
};
```

With `from_end = true` and `from > base_len` this underflows: in debug builds a panic, in release a wrapped huge length silently propagating into a slice value (wrong code). With `from_end = false` and `to > base_len` there is no check at all.

**PATCH** — replace the entire block from `let start = if *from_end {` through the `};` closing the `end` computation (lines 1943-1964) with:

```rust
                    let start = if *from_end {
                        if base_len < *to as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice from end to {} out of bounds (len {})",
                                to, base_len
                            )));
                        }
                        base_len - (*to as usize)
                    } else {
                        if base_len < *from as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice start {} out of bounds (len {})",
                                from, base_len
                            )));
                        }
                        (*from) as usize
                    };
                    let end = if *from_end {
                        // T-ICE-2: the guard here checked `to` while the
                        // arithmetic subtracts `from` — `from > base_len`
                        // underflowed (panic in debug, wrapped huge length
                        // in release). Check the operand actually used.
                        if base_len < *from as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice from end offset {} out of bounds (len {})",
                                from, base_len
                            )));
                        }
                        base_len - (*from as usize)
                    } else {
                        if base_len < *to as usize {
                            return Err(InterpError::Panic(format!(
                                "subslice end {} out of bounds (len {})",
                                to, base_len
                            )));
                        }
                        (*to) as usize
                    };
```

(The `if start > end` check that follows the block is kept as-is; it now operates on fully validated values.)

**Verify:**
```bash
cargo nextest run -p glyim-mir-interp 2>&1 | tail -3
cargo nextest run -p glyim-lower -p glyim-test --test run_pass_corpus 2>&1 | tail -3
```
**Expected:** all interpreter tests and all 99 run-pass fixtures pass (slice-pattern fixtures in `crates/glyim-lower/tests/mir/slice_pattern.g` and the interp `boundaries.rs`/`stress.rs` suites exercise this path).

#### Step 3.3 — Cap the interpreter's unbounded allocation (P-03, S1: OOM abort of the host)

`crates/glyim-mir-interp/src/lib.rs:561` — `InterpValue::Uint(n) => "\u{0}".repeat(*n as usize)` builds the byte payload for an extern `stdout`/`stderr` write. A program passing a huge unsigned integer allocates unbounded memory inside the host process.

**PATCH** — replace that single match arm with:

```rust
            // T-ICE-3: `n` is program-controlled; a hostile program could
            // request gigabytes of NUL bytes and OOM the host. Cap the
            // payload at 16 MiB — larger writes are truncated. This is a
            // debugging interpreter, and truncation beats a killed host.
            InterpValue::Uint(n) => "\u{0}".repeat((*n).min(16 * 1024 * 1024) as usize),
```

**Verify:**
```bash
cargo build -q -p glyim-mir-interp && cargo nextest run -p glyim-mir-interp 2>&1 | tail -3
```
**Expected:** exit=0, all interpreter tests pass.

#### Step 3.4 — Turn interpreter internal-index panics into `InterpError`s (P-05, S2)

Three direct indexings in the interpreter loop panic if a (hypothetical) lowering bug feeds out-of-range indices. A compiler bug must surface as an `InterpError` (which the CLI and glyip render), not as a host panic.

**PATCH** — in `crates/glyim-mir-interp/src/lib.rs`:

1. Locate all three occurrences of `body.basic_blocks[bb_idx]` (near lines 767, 792, 796) with:
   ```bash
   rg -n "basic_blocks\[bb_idx\]" crates/glyim-mir-interp/src/lib.rs
   ```
   Replace each occurrence of the form `&body.basic_blocks[bb_idx]` (adjust `&`/borrow shape to the local context) with:
   ```rust
   body.basic_blocks.get(bb_idx).ok_or_else(|| {
       InterpError::Panic(format!(
           "internal error: basic block index {} out of range (len {}); \
            this is a compiler bug in lowering, please report it",
           bb_idx,
           body.basic_blocks.len()
       ))
   })?
   ```
   If the surrounding expression cannot carry a `?`, bind it first:
   ```rust
   let bb = body.basic_blocks.get(bb_idx).ok_or_else(|| { ... })?;
   ```
   and use `bb` where the indexed value was used.

2. Locate `callee_locals[i + 1] = Some(val);` (line ~1001). Replace with a bounds-checked version:
   ```rust
   // T-ICE-4: arg/local count mismatch is a lowering bug; fail loudly
   // but as an InterpError instead of an index panic.
   if i + 1 >= callee_locals.len() {
       return Err(InterpError::Panic(format!(
           "internal error: argument index {} exceeds callee local count {}; \
            this is a compiler bug in lowering, please report it",
           i + 1,
           callee_locals.len()
       )));
   }
   callee_locals[i + 1] = Some(val);
   ```

**Verify:**
```bash
cargo build -q -p glyim-mir-interp && cargo nextest run -p glyim-mir-interp -p glyim-pipeline 2>&1 | tail -3
```
**Expected:** exit=0 (the interp and pipeline suites cover the call path — `calls.rs`, `nested_calls.rs`, `tier0.rs`).

#### Step 3.5 — Fix the const-eval u128 index truncation (P-04, S1: silently wrong const value)

`crates/glyim-const-eval/src/eval.rs:440-460` — `Expr::Index` evaluates the index as `u128`, then casts `idx as usize`. On a 64-bit host, an index of `2^64 + 3` truncates to `3`, passes the bounds check, and **returns a wrong value** instead of an out-of-bounds error. This is silent wrong evaluation inside `const` items.

**PATCH** — in `crates/glyim-const-eval/src/eval.rs`, `Expr::Index` arm, replace the line

```rust
                if let Some(idx) = idx_val.as_u128() {
```

with

```rust
                // T-ICE-5: `idx as usize` truncated u128 indices on
                // 64-bit hosts — `2^64 + 3` aliased to element 3 and
                // silently read an in-bounds value. Reject first.
                if let Some(idx) = idx_val.as_u128().filter(|i| {
                    *i <= usize::MAX as u128
                }) {
```

The two `if (idx as usize) < …` bounds checks below it remain valid because `idx` is now guaranteed representable.

**PATCH 2** — regression fixture. **WRITE FILE** `crates/glyim-test/tests/compile-fail/const_index_u128_truncation.g`:

```gly
// T-ICE-5 regression: a const index of 2^64 + 3 must be reported as
// out of bounds, not silently aliased to element 3 by a usize truncation.
const BAD: i32 = [10, 20, 30][18446744073709551619];
//~^ ERROR index out of bounds

fn main() {}
```

**Verify:**
```bash
cargo nextest run -p glyim-const-eval 2>&1 | tail -3
cargo nextest run -p glyim-test --test compile_fail_corpus 2>&1 | tail -3
```
**Expected:** const-eval tests pass; the compile-fail corpus (now 11 fixtures) passes with the new fixture producing `index out of bounds in const eval`. If the fixture reports the error on a different line, adjust the `//~^` arrows so the annotation points at the `const BAD` line — do not change the compiler code.

#### Step 3.6 — Upgrade the CLI ICE handler (P-07, S2)

`crates/glyim-cli/src/lib.rs:130-168` — improvements: capture the panic *message* via a hook (the default hook currently prints a redundant message during `catch_unwind`), include the payload in the ICE report, and point the bug URL at the real repository.

**PATCH** — replace the body of `pub fn run() -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {` up to the closing `}` of the function with:

```rust
pub fn run() -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {
    // Capture args (incl. `error-format`) before entering the panic catcher,
    // so we can print any returned diagnostics in the requested format.
    let cli_args = CliArgs::parse();
    let error_format = cli_args.error_format.clone();

    // T-ICE-6: install a hook that *silences* the default panic printer and
    // records the payload. During catch_unwind the default hook would print
    // a raw `thread 'main' panicked at …` line on top of our ICE report; the
    // ICE report itself is the correct user-facing output, and it should
    // contain the panic message.
    let panic_message: std::sync::Arc<std::sync::Mutex<Option<String>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    {
        let sink = panic_message.clone();
        std::panic::set_hook(Box::new(move |info| {
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            if let Ok(mut slot) = sink.lock() {
                *slot = Some(msg);
            }
        }));
    }

    // Plan §3.1.2b: catch unexpected panics in the driver and turn them into a
    // structured ICE report instead of a raw Rust backtrace. A language
    // toolchain must never crash on user input with an unreadable stack trace —
    // an ICE with the compiler version + input + backtrace path is the
    // production-grade behaviour (rustc's `RUST_BACKTRACE` model).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_with_args(cli_args)));
    let _ = std::panic::take_hook();
    match result {
        Ok(inner) => {
            if let Err(diags) = &inner {
                print_diagnostics(diags, &error_format);
            }
            inner
        }
        Err(_payload) => {
            let backtrace = std::backtrace::Backtrace::force_capture();
            let panic_msg = panic_message
                .lock()
                .ok()
                .and_then(|slot| slot.clone())
                .unwrap_or_else(|| "<unknown>".to_string());
            let report = format!(
                "glyim internal compiler error (ICE): the compiler panicked.\n\
                 version: {}\n\
                 panic message: {}\n\
                 Please file a bug report. Backtrace saved below.\n\n{:?}",
                env!("CARGO_PKG_VERSION"),
                panic_msg,
                backtrace,
            );
            let path = std::env::temp_dir().join("glyim-ice.txt");
            let _ = std::fs::write(&path, &report);
            eprintln!(
                "error: glyim panicked (ICE). Backtrace saved to {}:

please file a bug report at <https://github.com/elcoosp/glyim/issues> \
with the input file and the backtrace.",
                path.display()
            );
            Err(vec![glyim_diag::GlyimDiagnostic::internal_error(report)])
        }
    }
}
```

**Verify:**
```bash
cargo build -q -p glyim-cli && ./target/debug/glyim-cli --version 2>/dev/null; ./target/debug/glyim-cli /tmp/deep_type.g --emit mir 2>&1 | head -3; echo "exit=$?"
cargo nextest run -p glyim-cli 2>&1 | tail -3
```
**Expected:** exit codes are clean (no `SIGSEGV`), CLI tests pass. (The panic path itself is exercised by the no-ice corpus and Phase 3's new fixtures.)

#### Step 3.7 — Make `glyip test` panic-proof (P-06, S2)

`crates/glyip/src/commands.rs` (~line 338, the test runner's interpreter loop) executes `interpreter.run_body(body)` in-process with no panic isolation. One fixture panicking kills the entire test session with a raw stack trace.

**PATCH** — locate in `crates/glyip/src/commands.rs`:

```rust
        match interpreter.run_body(body) {
            Ok(()) => passed += 1,
            Err(_) => failed += 1,
        }
```

Replace with:

```rust
        // T-ICE-7: the interpreter runs in-process; a lowering bug that
        // panics must not abort the whole test session. Convert the panic
        // into a counted failure with the payload printed for triage.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            interpreter.run_body(body)
        })) {
            Ok(Ok(())) => passed += 1,
            Ok(Err(e)) => {
                failed += 1;
                eprintln!("test '{}' failed: {}", discovered_test.name, e);
            }
            Err(payload) => {
                failed += 1;
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "<non-string panic payload>".to_string());
                eprintln!(
                    "test '{}' PANICKED the interpreter (compiler ICE): {}",
                    discovered_test.name,
                    msg
                );
            }
        }
```

**Verify:**
```bash
cargo build -q -p glyip && cargo nextest run -p glyip 2>&1 | tail -3
```
**Expected:** exit=0; glyip's 224 tests pass.

#### Step 3.8 — Convert the three highest-value codegen panics to diagnostics (P-08, S2)

`crates/glyim-codegen-llvm/src/lower.rs` mixes two styles for the same class of recoverable LLVM build errors: the correct `map_err(|e| vec![GlyimDiagnostic::internal_error(format!(…))])?` pattern (see `lower.rs:3441-3444`) and raw `panic!`s. Convert the three panics where a *recoverable* type/ABI failure becomes an ICE:

1. **Line ~119** — `.unwrap_or_else(|e| panic!("fn type lowering failed: {:?}", e))`: the enclosing expression must instead propagate `Err(vec![GlyimDiagnostic::internal_error(format!("fn type lowering failed: {e:?}"))])`. If the enclosing function cannot return `Result`, propagate through the nearest `Result`-returning caller — trace the call chain upward (`rg -n "<fn_name>" crates/glyim-codegen-llvm/src/lower.rs` finds it) and convert at the first caller whose return type is a `Result<_, Vec<GlyimDiagnostic>>`.
2. **Line ~185** — `Err(e) => panic!("Codegen error: {:?}", e)` inside `llvm_type_for_ty`: same treatment — the match arm becomes `Err(e) => return Err(vec![GlyimDiagnostic::internal_error(format!("Codegen error: {e:?}"))])` if the function returns `Result`, else convert at the nearest `Result`-returning caller.
3. **Line ~880** — `panic!("Variant index {} out of bounds for {:?}", vi, current_ty)`: replace with `return Err(vec![GlyimDiagnostic::internal_error(format!("Variant index {vi} out of bounds for {current_ty:?}"))])` in whatever function hosts it (same propagation rule).

Rules for this step (R5 relaxed only here): you may edit exactly `crates/glyim-codegen-llvm/src/lower.rs` and, if signature propagation requires it, add `-> Result<…, Vec<GlyimDiagnostic>>` return types within that one file. Never `unwrap()` to silence a type error. The eleven `unreachable!()` invariant guards in the same file are **kept as-is** (they document real compiler-bug tripwires, and the ICE handler now reports them properly).

**Verify:**
```bash
cargo build -q -p glyim-codegen-llvm
cargo nextest run -p glyim-codegen-llvm 2>&1 | tail -3
rg -c "panic!\(" crates/glyim-codegen-llvm/src/lower.rs || echo "0 panic! sites remain in lower.rs"
```
**Expected:** build clean, all 302 LLVM backend tests pass, and the remaining `panic!` count in `lower.rs` is 0 (the version-check panic in `passes.rs` stays — it is a deliberate environment check).

#### Step 3.9 — Extend the ratchet lints to the crates that already qualify (P-09, S3)

The workspace policy is a rolling ratchet: crates declare `#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]` once their non-test code is clean. Four crates carry it (`frontend`, `hir`, `meta`, `typeck`). The audit confirms three more crates have **zero** unwrap/expect/panic in non-test code and qualify immediately: `glyim-solve`, `glyim-borrowck`, `glyim-mir`.

**PATCH** — for each of `crates/glyim-solve/src/lib.rs`, `crates/glyim-borrowck/src/lib.rs`, `crates/glyim-mir/src/lib.rs`, insert these two lines at the very top of the file (before all other attributes, exactly matching the existing pattern in `crates/glyim-frontend/src/lib.rs:31`):

```rust
#![warn(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
```

**Verify:**
```bash
cargo clippy -p glyim-solve -p glyim-borrowck -p glyim-mir --all-targets -- -D warnings 2>&1 | tail -3
```
**Expected:** exit=0. If a warning appears (meaning the audit's zero-count was wrong for a `#[cfg(not(test))]` path), fix the single occurrence using the same patterns as Steps 3.4-3.8, or remove the ratchet from that one crate and note it in the report — never `#[allow]` it away.

#### Step 3.10 — Phase gate: full suite + no-ICE corpus

```bash
cargo nextest run --workspace 2>&1 | tail -3
cargo nextest run -p glyim-test --test no_ice_corpus 2>&1 | tail -3
```
**Expected:** workspace counts equal the Phase 1 baseline **plus** the new fixtures (all passing); no-ICE corpus passes with 31 fixtures. Commit each step, then: `3.x: ICE hardening (type-parser depth guard, interp bounds/alloc fixes, const-eval index fix, ICE report upgrade, glyip isolation, ratchet lints)`.

---

### Phase 4 — Differential testing and fuzzing

**Goal:** continuous, automated discovery of *semantic* wrongness (differential backends) and *crash* wrongness (fuzzing). The project already has the foundations — a 99-fixture differential harness, a 30-fixture no-ICE hostile corpus, and a property-based type generator — but no fuzz targets exist anywhere. You will add them.

#### Step 4.1 — Verify the differential baseline is zero

```bash
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential --features glyim-test/real-llvm 2>&1 | tail -3
```
**Expected:** exit=0 (commit `60452ca6` claims 0 disagreements). If any fixture disagrees: capture the fixture name, native output, interpreter output, then STOP and file it in your report as a blocking finding. Do not attempt codegen fixes.

#### Step 4.2 — Add a semantic probe fixture for the `String::as_str` layout claim (D-02)

The native `as_str` lowering asserts (in comments) that `String` and `&str` share the `{ptr, i64}` layout, so the whole value can be copied through. History (session-11 docs) recorded a bug where capacity was read as length. Lock the current behavior in with a run-pass fixture.

**WRITE FILE** `crates/glyim-test/tests/run-pass/str/as_str_identity.g`:

```gly
// Differential guard: `String::as_str()` must return a view of the string's
// *length* bytes — not its capacity. If the native lowering copies the
// wrong header fields, the native run prints garbage/extra bytes and the
// differential suite (native vs interpreter) flags a disagreement.
fn check(s: &str) -> i32 {
    let mut n = 0;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        n = n + 1;
        i = i + 1;
    }
    n
}

fn main() {
    let owned = "abc";
    let s = owned.to_string();
    let view = s.as_str();
    print_i32(check(view));
    // Expected stdout: 3
}

// Helper shim: keep the fixture dependency-free; `print_i32` maps to the
// runtime's stdout writer. If the surface lacks `print_i32`, replace the
// call with the stdlib print primitive used by other run-pass fixtures
// (inspect crates/glyim-test/tests/run-pass/println.g for the idiom).
fn print_i32(v: i32) {}
```

> Agent note (R3 exception): before finalizing this fixture, open `crates/glyim-test/tests/run-pass/println.g` and copy **its exact printing idiom** into this fixture, replacing the `print_i32` shim, and set the expected output via the fixture's `check-stdout:` header directive (see other run-pass fixtures for the format). The fixture must assert stdout `3`. Then run:

```bash
cargo nextest run -p glyim-test --test run_pass_corpus -- filter as_str_identity 2>&1 | tail -3
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential --features glyim-test/real-llvm 2>&1 | tail -3
```
**Expected:** the fixture passes in both modes. If the native path disagrees with the interpreter on this fixture, you have just caught the capacity-as-length bug live: record it and STOP (Appendix A, item A-2).

#### Step 4.3 — Create the fuzz workspace

cargo-fuzz builds targets with libFuzzer on nightly. `fuzz_parse` only needs `glyim-frontend` (which has no LLVM dependency — the frontend crate is LLVM-free), so it fuzzes fast without any LLVM requirement. `fuzz_compile` drives the whole pipeline and needs `LLVM_SYS_231_PREFIX`.

```bash
cargo install cargo-fuzz --locked
cargo fuzz init --devrv 2>/dev/null || cargo fuzz init
```

The `cargo fuzz init` scaffolding creates `fuzz/Cargo.toml` and `fuzz/fuzz_targets/fuzz_target_1.rs`. Replace both.

**WRITE FILE** `fuzz/Cargo.toml` (complete content):

```toml
[package]
name = "glyim-fuzz"
version = "0.0.0"
publish = false
edition = "2021"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"

# The fuzz targets are a separate workspace; path-depend into the crates
# under test. glyim-frontend is LLVM-free, so fuzz_parse builds anywhere.
[dependencies.glyim-frontend]
path = "../crates/glyim-frontend"

# fuzz_compile needs the pipeline (and thus llvm-sys + LLVM_SYS_231_PREFIX).
[dependencies.glyim-pipeline]
path = "../crates/glyim-pipeline"

# Prevent this workspace from participating in the parent workspace.
[workspace]

[[bin]]
name = "fuzz_parse"
path = "fuzz_targets/fuzz_parse.rs"
test = false
doc = false
bench = false

[[bin]]
name = "fuzz_compile"
path = "fuzz_targets/fuzz_compile.rs"
test = false
doc = false
bench = false
```

**WRITE FILE** `fuzz/fuzz_targets/fuzz_parse.rs` (complete content):

```rust
#![no_main]
// Target 1: lexer + parser + CST construction. A crash here is a P0 bug:
// the frontend is the user-input boundary and must never panic (its
// crate-root ratchet enforces no unwrap/expect/panic in non-test code;
// the fuzzer verifies the claim against reality).
//
// Invariants checked per input:
//   1. parse() must not panic (libFuzzer catches any panic).
//   2. The produced CST must round-trip to the exact input text
//      (losslessness is Rowan's core contract).

use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;

static PANIC_HOOK: OnceLock<()> = OnceLock::new();

fn install_hook() {
    PANIC_HOOK.get_or_init(|| {
        let default_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Print the offending input so the reproducer is obvious.
            eprintln!("PANIC — see /tmp/glyim-fuzz-last-input.g");
            default_hook(info);
        }));
    });
}

fuzz_target!(|data: &[u8]| {
    install_hook();
    // Only feed inputs that are at least valid UTF-8 (the lexer is
    // specified over UTF-8 sources); non-UTF-8 bytes are rejected the
    // same way the CLI would reject them.
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    // Keep inputs bounded so the fuzzer spends its budget on structure
    // instead of on gigantic files (the parser also has its own depth
    // limits — the 1 MiB cap here guards the fuzz driver's own memory).
    if src.len() > 1024 * 1024 {
        return;
    }
    let _ = std::fs::write("/tmp/glyim-fuzz-last-input.g", src);
    let parsed = glyim_frontend::parse(src);
    // Losslessness: the CST must render back to the exact input.
    let rendered = parsed.to_string();
    assert_eq!(
        rendered, src,
        "CST round-trip mismatch: the parser rewrote the program"
    );
});
```

> **API check before compiling (R2).** The exact public entry points of `glyim-frontend` and `glyim-pipeline` are what matter here. Run:
> ```bash
> rg -n "pub fn parse" crates/glyim-frontend/src/lib.rs
> rg -n "pub fn compile_file|pub fn emit_mir" crates/glyim-pipeline/src/lib.rs | head -5
> ```
> Then adapt the two `fuzz_target!` bodies to the real signatures:
> - If the frontend exposes a different name (e.g. `parse_source`, `parse_to_syntax`), substitute it; the contract being fuzzed is "parse any string without panicking + CST round-trips". If there is no public parse API returning a renderable tree, drop the `assert_eq!` and keep only the no-panic invariant — a no-panic fuzz target is still valuable.
> - If parse returns `(tree, diagnostics)` or a wrapper struct, extract accordingly.

**WRITE FILE** `fuzz/fuzz_targets/fuzz_compile.rs` (complete content):

```rust
#![no_main]
// Target 2: full-pipeline ICE detector. Every input must compile to
// either (a) success, or (b) a Vec<GlyimDiagnostic> — never a panic.
// This is the fuzz-side twin of the no-ice corpus (30 hand-written
// hostile fixtures) with unbounded creativity.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    if src.len() > 256 * 1024 {
        return;
    }
    let _ = std::fs::write("/tmp/glyim-fuzz-last-input.g", src);
    // compile_file takes a path; write the input to a scratch file so the
    // pipeline sees a normal source file. Adapt the call to the exact
    // signature found via:
    //   rg -n "pub fn compile_file" crates/glyim-pipeline/src/lib.rs
    let scratch = std::env::temp_dir().join("glyim-fuzz-compile-input.g");
    let _ = std::fs::write(&scratch, src);
    // The contract: Result<_, Vec<GlyimDiagnostic>>. A panic inside the
    // pipeline is the bug libFuzzer reports. Do not unwrap the result.
    let result = std::panic::catch_unwind(|| {
        glyim_pipeline::compile_file(&scratch)
    });
    if result.is_err() {
        // A panic escaped the pipeline: this is exactly the class of bug
        // the no-ice corpus hunts. Let libFuzzer record the crash.
        panic!(
            "pipeline panicked on input saved at {}",
            scratch.display()
        );
    }
});
```

#### Step 4.4 — Seed the corpus and add a token dictionary

```bash
mkdir -p fuzz/corpus/fuzz_parse fuzz/corpus/fuzz_compile
cp crates/glyim-test/tests/no-ice/*.g fuzz/corpus/fuzz_parse/
cp crates/glyim-test/tests/compile-fail/*.g crates/glyim-test/tests/compile-pass/*.g fuzz/corpus/fuzz_parse/
cp crates/glyim-test/tests/compile-pass/*.g crates/glyim-test/tests/compile-fail/*.g fuzz/corpus/fuzz_compile/
```

**WRITE FILE** `fuzz/glyim.dict` (complete content):

```text
# Glyim token dictionary — helps libFuzzer cross the lexer boundary.
"fn"
"let"
"mut"
"struct"
"enum"
"impl"
"trait"
"match"
"if"
"else"
"while"
"loop"
"for"
"in"
"return"
"break"
"continue"
"async"
"await"
"true"
"false"
"self"
"pub"
"use"
"mod"
"macro_rules"
"i32"
"u32"
"u64"
"f64"
"bool"
"str"
"String"
"Vec"
"Option"
"Result"
"Some"
"None"
"Ok"
"Err"
"&&"
"||"
"->"
"=>"
"::"
".."
"!="
"=="
"<="
">="
"+="
"-="
"<<"
">>"
"@"
"_"
"'a"
"&"
"*"
"#"
"!"
"?"
```

#### Step 4.5 — Smoke-run the fuzz targets

```bash
cargo fuzz build -O --sanitizer none fuzz_parse 2>&1 | tail -2
cargo fuzz run fuzz_parse fuzz/corpus/fuzz_parse -- -max_total_time=120 -rss_limit_mb=4096 2>&1 | tail -5
```

**Expected (both outcomes are meaningful):**
- **Exit=0** after 120s — the parser survived two minutes of structured mutation. 
- **A crash** — libFuzzer prints `artifact_prefix='crash-…'` and the reproducer file. A crash here is a *real P0 finding*: minimize it (`cargo fuzz tmin`), add it to `crates/glyim-test/tests/no-ice/` as a regression fixture, fix the root cause only if it matches one of the patterns in Phase 3, otherwise record it and stop. Never delete the artifact.

Then, if `LLVM_SYS_231_PREFIX` is available:

```bash
cargo fuzz run fuzz_compile fuzz/corpus/fuzz_compile -- -max_total_time=180 -rss_limit_mb=8192 2>&1 | tail -5
```

**Expected:** same policy. `fuzz_compile` exercises lex→parse→resolve→HIR→typeck→lower→borrowck→opt (interpreter/codegen dispatch is downstream of `compile_file`'s emit kind — adapt the call in Step 4.3's API check).

> Practical notes:
> - `cargo fuzz` requires the `nightly` toolchain (it installs it automatically with `rustup`).
> - Run `fuzz_parse` (LLVM-free) on any machine; run `fuzz_compile` only where LLVM 23 is set up.
> - Timeboxing: 2–3 minutes per target is the smoke level; a release-prep session should run `fuzz_parse` for ≥ 1 hour with `-max_len=4096`.

#### Step 4.6 — Phase gate

```bash
cargo nextest run --workspace 2>&1 | tail -3
git add -A && git commit -m "4.x: fuzzing infrastructure (parse + compile targets, corpus seeds, dictionary) + as_str differential guard"
```
**Expected:** workspace suite unchanged-green; the fuzz workspace is self-contained under `fuzz/` and does not affect `cargo build` of the parent workspace.

---

### Phase 5 — Packaging and release engineering

**Goal:** a human can download a v0.2.0 archive, run an install script, and get a working `glyim-cli` + `glyip` with the runtime staticlib wired up; the repo carries a CHANGELOG, tags, and a reproducible release workflow.

#### Step 5.1 — Fix the README doc bugs (PKG-05)

Four surgical corrections in `README.md`:

1. Replace `git clone https://github.com/elcoosp/glyim-v2` with `git clone https://github.com/elcoosp/glyim`.
2. Replace "The compiler driver is then available at `target/release/glyim`" with "The compiler driver is then available at `target/release/glyim-cli` (invoke it as `glyim-cli`; rename or symlink to `glyim` if you prefer)."
3. Replace the tip paragraph claiming "If you don't have LLVM 23 installed, build with `--no-default-features` … and use the bytecode backend" with:

   ```markdown
   > [!IMPORTANT]
   > The LLVM backend is a hard dependency of the pipeline — there is no
   > feature flag to build without it. Install LLVM 23 and set
   > `LLVM_SYS_231_PREFIX` (Homebrew: `$(brew --prefix llvm@23)`,
   > Debian/Ubuntu: `/usr/lib/llvm-23`).
   ```
4. In `crates/glyim-cli/src/lib.rs`, if any `glyim-v2` URL survived Step 3.6 (it should not have), fix it to `elcoosp/glyim`.

**Verify:** `rg -c "glyim-v2" README.md crates/glyim-cli/src/lib.rs || echo "0 glyim-v2 references"` — **Expected:** `0 glyim-v2 references`.

#### Step 5.2 — Workspace metadata hygiene (PKG-02)

**PATCH** — `crates/glyip/Cargo.toml`: replace the hardcoded `version = "0.1.0"` with `version.workspace = true` and the hardcoded `edition = "2024"` with `edition.workspace = true` (the workspace already provides both). Keep the crate's own fields (`description`, `[[bin]]`, features) untouched. If a `license` key exists locally, replace it with `license.workspace = true`.

**PATCH** — root `Cargo.toml`, extend `[workspace.package]` (it currently has edition/version/license plus your new `rust-version`) with:

```toml
repository = "https://github.com/elcoosp/glyim"
readme = "README.md"
```

**Verify:** `cargo metadata -q --no-deps >/dev/null && echo ok` — **Expected:** `ok`. Run `cargo nextest run -p glyip 2>&1 | tail -3` — **Expected:** glyip's tests still pass (version inheritance changes the `--version` string source only).

#### Step 5.3 — Harden the release profile for correctness (overflow checks)

**PATCH** — root `Cargo.toml`, `[profile.release]` section — add one line so it reads:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
panic = "unwind"
strip = "debuginfo"
# Correctness-first: an accidental overflow in compiler-internal arithmetic
# becomes a loud ICE (caught by the driver's handler) instead of a silently
# wrapped value. The interpreter/const-eval do their own checked math, so
# compiled-program semantics are unaffected.
overflow-checks = true
```

**Rationale:** this is a *compiler*; the release binary's own arithmetic integrity matters more than the last few percent of compile speed. If a future benchmark proves the cost unacceptable, reverting this line must require a commit message citing the benchmark — that is the decision procedure.

**Verify:**
```bash
cargo build -q --release -p glyim-cli 2>&1 | tail -2
cargo nextest run -p glyim-cli -p glyim-pipeline 2>&1 | tail -3
```
**Expected:** exit=0 for both (debug profile is unaffected; this validates the release profile parses and builds).

#### Step 5.4 — Create the CHANGELOG

**WRITE FILE** `CHANGELOG.md` (complete content):

```markdown
# Changelog

All notable changes to Glyim are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Fuzzing infrastructure: `fuzz_parse` (lexer/parser/CST round-trip) and
  `fuzz_compile` (full-pipeline ICE detector) cargo-fuzz targets with
  corpus seeds and a token dictionary.
- CI: strict M5 runtime proof (real LLVM codegen), differential suite
  (LLVM native vs MIR interpreter), lint job (`fmt` + `clippy -D
  warnings`), cargo-deny supply-chain job, runner caching, timeouts,
  concurrency cancellation.
- MIT LICENSE file, declared MSRV (`rust-version = "1.85"`).

### Changed
- CI installs LLVM 23 consistently (`./llvm.sh 23` — previously mixed
  v22 script with v23 dev packages).
- Release profile enables `overflow-checks` (compiler-internal arithmetic
  overflow is an ICE, never a silent wrap).

### Fixed
- `parse_type` recursion depth unbounded → deeply nested types crashed the
  compiler with an uncatchable stack overflow (SIGSEGV). Now a parse
  diagnostic (`MAX_EXPR_DEPTH` guard, same policy as expressions).
- MIR interpreter subslice projection: bounds guard checked `to` while
  subtracting `from` → `usize` underflow (panic in debug, bogus slice
  length in release). Both operands are now validated.
- MIR interpreter `byte_payload`: program-controlled `repeat(n)` could
  OOM the host; payload capped at 16 MiB.
- const-eval array/tuple index: `u128 → usize` truncation silently aliased
  indices ≥ 2^64 to in-bounds elements. Oversized indices now error.
- Interpreter internal index accesses (`basic_blocks`, argument locals)
  return `InterpError` with a "please report" message instead of panicking.
- `glyim-cli` ICE report now includes the panic message, suppresses the
  default panic printer during `catch_unwind`, and points to the correct
  issue tracker.
- `glyip test` isolates interpreter panics per fixture instead of aborting
  the whole test session.
- LLVM codegen: recoverable type/ABI failures return diagnostics instead
  of panicking (`fn type lowering failed`, `Codegen error`, variant index).
- README: correct repo URL, binary name, and LLVM-install guidance.

## [0.1.0] — initial public state

Research-grade end-to-end compiler: lexing, parsing, name resolution,
macros, HIR, type inference + trait solving, NLL borrow checking, MIR
optimizations, LLVM 23 backend, bytecode backend + VM, MIR interpreter,
LSP, `glyip` build tool, and a five-mode test harness.
```

#### Step 5.5 — Release workflow

**WRITE FILE** `.github/workflows/release.yml` (complete content):

```yaml
name: Release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

env:
  CARGO_TERM_COLOR: always

jobs:
  build:
    name: Build (${{ matrix.os }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            archive: glyim-${{ github.ref_name }}-x86_64-linux.tar.gz
          - os: macos-latest
            target: aarch64-apple-darwin
            archive: glyim-${{ github.ref_name }}-aarch64-macos.tar.gz
    runs-on: ${{ matrix.os }}
    timeout-minutes: 75
    steps:
      - uses: actions/checkout@v4

      - name: Install LLVM 23 (ubuntu)
        if: runner.os == 'Linux'
        run: |
          wget -q https://apt.llvm.org/llvm.sh
          chmod +x llvm.sh
          sudo ./llvm.sh 23
          sudo apt-get install -y llvm-23-dev libpolly-23-dev
          echo "LLVM_SYS_231_PREFIX=/usr/lib/llvm-23" >> "$GITHUB_ENV"

      - name: Install LLVM 23 (macos)
        if: runner.os == 'macOS'
        run: |
          brew install llvm@23
          echo "LLVM_SYS_231_PREFIX=$(brew --prefix llvm@23)" >> "$GITHUB_ENV"

      - uses: dtolnay/rust-toolchain@stable

      # Gate: the release tag must be green.
      - name: Run workspace tests
        run: cargo nextest run --workspace

      - name: Build release binaries + runtime staticlib
        run: cargo build --release -p glyim-cli -p glyip -p glyim-runtime

      - name: Stage archive layout
        shell: bash
        run: |
          stage=stage/glyim
          mkdir -p "$stage/lib"
          cp target/release/glyim-cli "$stage/"
          cp target/release/glyip "$stage/"
          cp target/release/libglyim_runtime.a "$stage/lib/"
          cp README.md LICENSE CHANGELOG.md "$stage/" 2>/dev/null || true
          cp -r crates/glyim-lang-core/lib "$stage/stdlib-core"
          tar -czf "${{ matrix.archive }}" -C stage glyim
          sha256sum "${{ matrix.archive }}" > "${{ matrix.archive }}.sha256"

      - name: Upload artifacts
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.archive }}
          path: |
            ${{ matrix.archive }}
            ${{ matrix.archive }}.sha256

  publish:
    name: Attach archives to the GitHub release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          merge-multiple: true
          path: dist
      - name: Create release
        uses: softprops/action-gh-release@v2
        with:
          files: dist/*
          generate_release_notes: true
```

#### Step 5.6 — Install script

**WRITE FILE** `scripts/install.sh` (complete content; make it executable with `chmod +x scripts/install.sh`):

```bash
#!/usr/bin/env bash
# Glyim installer — installs the release archive layout produced by
# .github/workflows/release.yml into ~/.local, and wires up the runtime
# staticlib via GLYIM_RUNTIME_LIB (the compiler resolves it from this
# variable first; see crates/glyim-cli/src/linker.rs).
#
# Usage: ./install.sh [archive.tar.gz]
#   with an archive:  installs exactly that archive
#   without:          installs from ../target/release (developer install)
set -euo pipefail

PREFIX="${GLYIM_INSTALL_PREFIX:-$HOME/.local}"
BIN_DIR="$PREFIX/bin"
LIB_DIR="$PREFIX/lib/glyim"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

mkdir -p "$BIN_DIR" "$LIB_DIR"

if [[ $# -ge 1 ]]; then
  ARCHIVE="$1"
  [[ -f "$ARCHIVE" ]] || die "archive not found: $ARCHIVE"
  say "Installing from archive $ARCHIVE"
  TMP="$(mktemp -d)"
  tar -xzf "$ARCHIVE" -C "$TMP"
  SRC="$TMP/glyim"
  [[ -f "$SRC/glyim-cli" ]] || die "archive layout unexpected: no glyim-cli"
  install -m 0755 "$SRC/glyim-cli" "$BIN_DIR/glyim-cli"
  install -m 0755 "$SRC/glyip" "$BIN_DIR/glyip"
  [[ -f "$SRC/lib/libglyim_runtime.a" ]] && \
    install -m 0644 "$SRC/lib/libglyim_runtime.a" "$LIB_DIR/libglyim_runtime.a"
  rm -rf "$TMP"
else
  say "Developer install from target/release"
  ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  [[ -f "$ROOT/target/release/glyim-cli" ]] || die "build first: cargo build --release -p glyim-cli -p glyip -p glyim-runtime"
  install -m 0755 "$ROOT/target/release/glyim-cli" "$BIN_DIR/glyim-cli"
  install -m 0755 "$ROOT/target/release/glyip" "$BIN_DIR/glyip"
  [[ -f "$ROOT/target/release/libglyim_runtime.a" ]] && \
    install -m 0644 "$ROOT/target/release/libglyim_runtime.a" "$LIB_DIR/libglyim_runtime.a"
fi

if [[ -f "$LIB_DIR/libglyim_runtime.a" ]]; then
  ENV_SNIPPET="export GLYIM_RUNTIME_LIB=\"$LIB_DIR/libglyim_runtime.a\""
  say "Runtime staticlib installed at $LIB_DIR/libglyim_runtime.a"
  say "Make sure your shell exports: $ENV_SNIPPET"
  if ! grep -q "GLYIM_RUNTIME_LIB" "$HOME/.profile" 2>/dev/null; then
    printf '\n# Glyim runtime staticlib (added by glyim install script)\n%s\n' "$ENV_SNIPPET" >> "$HOME/.profile"
    say "Appended the export to ~/.profile"
  fi
fi

say "Installed:"
say "  $BIN_DIR/glyim-cli"
say "  $BIN_DIR/glyip"
"$BIN_DIR/glyim-cli" --help >/dev/null 2>&1 && say "glyim-cli runs OK" \
  || die "glyim-cli failed to execute"
say "Done. Ensure $BIN_DIR is on your PATH."
```

#### Step 5.7 — Tag the release

```bash
git add -A && git commit -m "5.x: packaging (metadata, overflow-checks, CHANGELOG, release workflow, install script)"
git tag -a v0.2.0 -m "v0.2.0: correctness-hardened release (see CHANGELOG.md)"
git tag | head -3
```
**Expected:** `v0.2.0` listed. (Do not push unless the operator asks; pushing the tag triggers the release workflow.)

#### Step 5.8 — Phase gate: end-to-end install rehearsal

```bash
cargo build --release -p glyim-cli -p glyip -p glyim-runtime
bash scripts/install.sh
printf 'fn main() -> i32 {\n    0\n}\n' > /tmp/hello.g
GLYIM_RUNTIME_LIB="$HOME/.local/lib/glyim/libglyim_runtime.a" \
  "$HOME/.local/bin/glyim-cli" /tmp/hello.g --emit exec -o /tmp/hello 2>&1 | head -3
echo "compile_exit=$?"
```

**Expected:** `compile_exit=0` and `/tmp/hello` exists (the compile+link succeeded against the installed staticlib). If the linker cannot find `libglyim_runtime.a`, the `GLYIM_RUNTIME_LIB` export in the command above is the contract being tested — debug the install script, not the linker.

---

### Phase 6 — Final production-readiness gate

**Goal:** one command chain that re-proves everything, and a checklist a reviewer can audit.

#### Step 6.1 — Run the full gate

```bash
set -e
echo "── build ──"
cargo build --workspace --all-targets
echo "── test ──"
cargo nextest run --workspace
echo "── fmt ──"
cargo fmt --all -- --check
echo "── clippy (project bar) ──"
cargo clippy --workspace --all-targets -- -D warnings
echo "── supply chain ──"
cargo deny check
echo "── differential (backend agreement) ──"
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential --features glyim-test/real-llvm
echo "── strict runtime proof ──"
GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test runtime --features glyim-test/real-llvm
echo "── no-ice corpus ──"
cargo nextest run -p glyim-test --test no_ice_corpus
echo "── fuzz smoke (2 min, parse) ──"
cargo fuzz run fuzz_parse fuzz/corpus/fuzz_parse -- -max_total_time=120 -rss_limit_mb=4096
echo "ALL GATES PASSED"
```

**Expected:** every section prints, and the final line is `ALL GATES PASSED`.

#### Step 6.2 — Audit checklist

Mark each row by pointing at concrete evidence (a command output, a file, a CI run).

| # | Gate | Evidence required | Phase |
|---|---|---|---|
| 1 | Full suite green, count ≥ baseline + new fixtures | `cargo nextest run --workspace` output | 1 |
| 2 | Workspace builds with no Cargo profile warnings | Step 1.5 verify output | 1 |
| 3 | LICENSE exists, MSRV declared, `cargo metadata` clean | Step 1.6 | 1 |
| 4 | CI has 9 jobs incl. strict runtime + differential + lint + supply-chain | Step 2.5 | 2 |
| 5 | `cargo deny check` exit=0 | Step 2.3 | 2 |
| 6 | Differential = 0 disagreements | Step 2.4 / 4.1 | 2, 4 |
| 7 | Deep type nesting produces a diagnostic, not SIGSEGV | Step 3.1 verify | 3 |
| 8 | Subslice/index/alloc interpreter fixes in place, suites green | Steps 3.2-3.5 | 3 |
| 9 | ICE report includes panic message + correct repo URL | Step 3.6 | 3 |
| 10 | `rg -c "panic!\(" crates/glyim-codegen-llvm/src/lower.rs` → 0 | Step 3.8 | 3 |
| 11 | Ratchet lints in solve/borrowck/mir crate roots, clippy clean | Step 3.9 | 3 |
| 12 | Fuzz smoke ran; artifacts (if any) preserved as fixtures | Step 4.5 | 4 |
| 13 | README has no `glyim-v2` references | Step 5.1 | 5 |
| 14 | CHANGELOG, tag v0.2.0, release workflow, install rehearsal | Steps 5.4-5.8 | 5 |
| 15 | `ALL GATES PASSED` | Step 6.1 | 6 |

#### Step 6.3 — Final report format

Produce a report containing: (a) phases completed; (b) the checklist table filled in; (c) every Verify command run and its one-line outcome; (d) any STOP events with the recorded evidence; (e) the deferred items you noticed but did not touch (Appendix A pointers).

---

## Appendix A — Deferred correctness backlog (S3, explicitly out of this run)

These are real, evidence-backed items a future session should take in roughly this order. Each has a concrete anchor in the code:

1. **Sentinel builtins on error arms** — `into`/`to_string`/`clone`/`drop` sentinels lower to zero values on the LLVM side (`crates/glyim-lower/src/lower_rvalue.rs:411-463`, `crates/glyim-codegen-llvm/src/lower.rs:424-437`). The interpreter hard-fails them; the native path silently emits `0`. Replace with monomorphization-resolved virtual references (the code's own follow-up comment names the plan).
2. **`String`/`&str` layout identity** — if the Step 4.2 probe fixture ever fails, fix `crates/glyim-codegen-llvm/src/lower.rs:3434-3453` to read the actual header fields of the String layout instead of copying the whole value.
3. **`ConstRef` constants are zero-initialized globals** — every `const`/`static` reads `0` in native binaries (`crates/glyim-lower/src/mono.rs:418-423`). Materialize real initializers.
4. **`glyip` cannot build/run executables** — `glyip run` compiles to a bare object without `entry_main` or the runtime staticlib and then tries to exec it (`crates/glyip/src/commands.rs:557-564, 648-694`); `glyip test --compiled` links without the runtime. Wire the same `finalize_after_object`/`find_runtime_staticlib` chain the CLI uses.
5. **ThinLTO drops all but the first CGU object** — `crates/glyim-cli/src/lib.rs:542-553` copies only `thin_objects.into_iter().next()`; multi-CGU thin builds lose code. Copy/link the full set.
6. **Cross-frame unwinding is implemented but never enabled in production** — `panics_unwind` defaults to `false` (`crates/glyim-mir-interp/src/lib.rs:116`), so drop glue never runs during interpreter unwinding outside unit tests; the stale "no-op flag" comment at `:159-162` should be corrected too.
7. **Language-surface honesty** — loop-`.await` (T0060), non-nameable multi-await futures (T0061), `include!` source inclusion (`crates/glyim-meta/src/expander/mod.rs:849-871`), enum-variant value paths (`crates/glyim-typeck/src/unify.rs:364-382`), `x @ subpat` and leading-`|` parser hazards (FE-9/FE-11): all are explicit diagnostics already — add compile-fail fixtures pinning each, and fix the KNOWN_GAPS.md dangling references in code comments.
8. **LSP panic isolation** — `Uri::from_str(...).unwrap()` on editor-supplied URIs (`crates/glyim-lsp/src/navigation.rs:79,165,283`, `goto_definition.rs:40`) and the unguarded mainloop (`server.rs:19-21`); convert to JSON-RPC errors.
9. **Lock-poison policy** — `.lock().unwrap()` cascades in `glyim-runtime` (38 sites), `glyim-db` (2), `glyim-type/type_arena.rs` (6); adopt `.unwrap_or_else(PoisonError::into_inner)` (recovery) or a shared `lock()` helper that reports a diagnostic.
10. **Coverage + mutation gates** — `tools/glyim-pilot/src/gates/coverage.rs` (80% floor via `cargo llvm-cov`) and `mutation.rs` (75% kill rate via `cargo mutants`) encode the project's own bar; add them to CI once the current green state stabilizes. Start with `continue-on-error: true`, then harden.
11. **Thinnest-coverage crates** — `glyim-pipeline` (31 tests for the whole end-to-end driver), `glyim-bytecode-vm` (14), `glyim-db` (8), `glyim-vfs` (6): priority targets for the next testing wave.
12. **Supply-chain dedup** — `rand` (3 majors), `bitflags` (2), `toml` (2) in Cargo.lock; tune `renovate.json` with a Cargo grouping strategy.

## Appendix B — Environment variables and features reference

| Name | Meaning | Set by |
|---|---|---|
| `LLVM_SYS_231_PREFIX` | LLVM 23.1 install prefix for llvm-sys/inkwell | operator / CI (`/usr/lib/llvm-23` ubuntu, `$(brew --prefix llvm@23)` macOS) |
| `LLVM_SYS_231_STRICT_VERSIONING` | `0` = relax llvm-sys version probing | `.cargo/config.toml` |
| `GLYIM_TEST_REAL_LLVM` | test harness swaps mock codegen for the real `LlvmBackend` | CI (strict jobs), operator |
| `glyim-test/real-llvm` | Cargo feature that links `glyim-codegen-llvm` into the harness | CI (`--features glyim-test/real-llvm`) |
| `GLYIM_BLESS=1` | regenerate UI `.expected` snapshots | operator after intentional diagnostic changes |
| `GLYIM_TEST_SHOW_OUTPUT=1` | verbose fixture output on failure | debugging |
| `GLYIM_TEST_JSON` | JSON test reporter (feature `json-output`) | tooling |
| `GLYIM_RUNTIME_LIB` | explicit path to `libglyim_runtime.a` for `--emit=exec` | install script / operator |
| `GLYIM_LLVM` | required by `needs-llvm` fixtures (currently unused by any fixture) | — |

## Appendix C — Sources

This playbook was compiled from a parallel audit of the repository (commit `60452ca6`): (1) a panic/ICE audit of all 33 crates (scanner + manual classification of every macro site); (2) a pipeline-gap audit reading all `docs/roadmaps/*`, `docs/status/*` and the async/proc-macro/ThinLTO/runtime code paths; (3) a test/CI infrastructure audit including the forensic verification of `ci.yml` and the extraction of the project's own quality gates from `tools/glyim-pilot/src/gates/`; (4) a packaging/unsafe audit of all manifests, the linker chain, and every `unsafe` site. Where the project's own documents contradicted the code (session-11 disagreement list vs commit `60452ca6`; rotted TECHSTACK.md; nonexistent KNOWN_GAPS.md), the code was treated as truth.

