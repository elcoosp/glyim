# Glyim compiler development tasks.
#
# Real dev recipes (build, test, lint, differential, fuzz) at the top;
# per-session watcher recipes (`wr1`..`wr5`) at the bottom.

default:
    @just --list

# ---- build ----

build:
    cargo build --workspace

build-release:
    cargo build --workspace --release

check:
    cargo check --workspace

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# ---- M0 rails: bootstrap + local CI gates (plan M0.1 / M0.5) ----

# Guard: the workspace hard-depends on LLVM 23 (llvm-sys 231 via inkwell).
# Every gate recipe below calls this first so the failure names the remedy
# instead of surfacing as an opaque llvm-sys build-script error.
[private]
require-llvm:
    #!/usr/bin/env bash
    if [ -n "${LLVM_SYS_231_PREFIX:-}" ] && "${LLVM_SYS_231_PREFIX}/bin/llvm-config" --version >/dev/null 2>&1; then
        exit 0
    fi
    if command -v llvm-config-23 >/dev/null 2>&1; then
        exit 0
    fi
    echo "LLVM 23 not found: set LLVM_SYS_231_PREFIX to an llvm-23 prefix," >&2
    echo "or run 'just bootstrap' (installs LLVM 23 via apt.llvm.org/brew)." >&2
    exit 1

# M0.1: reproducible environment from a clean clone — the pinned LLVM 23
# (llvm-sys 231 / inkwell llvm23-1 triple-lock), the Rust stable toolchain
# with the lint components, and the cargo tools the CI gates use.
# Idempotent: every step skips itself when already satisfied.
bootstrap:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "== Rust toolchain =="
    if ! command -v rustup >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
            | sh -s -- -y --default-toolchain stable --profile minimal
    fi
    rustup component add rustfmt clippy
    echo "== LLVM 23 (llvm-sys 231) =="
    if [ -n "${LLVM_SYS_231_PREFIX:-}" ] && \
        "${LLVM_SYS_231_PREFIX}/bin/llvm-config" --version >/dev/null 2>&1; then
        echo "LLVM_SYS_231_PREFIX already points at working LLVM $("$LLVM_SYS_231_PREFIX/bin/llvm-config" --version)"
    elif command -v llvm-config-23 >/dev/null 2>&1; then
        echo "system llvm-config-23 found: $(llvm-config-23 --version)"
    elif command -v apt-get >/dev/null 2>&1 && command -v sudo >/dev/null 2>&1; then
        wget -q https://apt.llvm.org/llvm.sh -O /tmp/llvm.sh
        chmod +x /tmp/llvm.sh
        sudo /tmp/llvm.sh 23
        sudo apt-get install -y llvm-23-dev libpolly-23-dev
        echo 'export LLVM_SYS_231_PREFIX=/usr/lib/llvm-23' >> ~/.bashrc
    elif command -v brew >/dev/null 2>&1; then
        brew install llvm@23
        echo "export LLVM_SYS_231_PREFIX=\"$(brew --prefix llvm@23)\"" >> ~/.bashrc
    else
        echo "no supported LLVM 23 install path found;" >&2
        echo "download llvm-23 + libllvm23 + llvm-23-dev debs and extract:" >&2
        echo "  dpkg-deb -x <deb> <prefix-root>; export LLVM_SYS_231_PREFIX=<prefix-root>/usr/lib/llvm-23" >&2
        exit 1
    fi
    echo "== cargo tools (nextest, cargo-deny, cargo-machete) =="
    for tool in cargo-nextest cargo-deny cargo-machete; do
        if cargo "$tool" --version >/dev/null 2>&1; then
            echo "$tool already installed"
        else
            cargo install --locked "$tool"
        fi
    done
    echo "bootstrap complete"

# M0.5 `just verify`: the universal-DoD mechanical subset (plan §7.4) —
# fmt + clippy(deny) + tests + ledger check + evidence check + STATUS
# byte-check + README generated-block drift check. Heavy LLVM corpora and
# the differential gate stay in CI/nightly (they are not part of the M0.5
# verify DoD).
verify: require-llvm fmt-check clippy
    cargo nextest run --workspace --test-threads=2
    cargo run -q -p glyim-ledger -- check .
    cargo run -q -p glyim-ledger -- evidence check .
    cargo run -q -p glyim-ledger -- status . --check
    cargo run -q -p glyim-ledger -- status . --check --format json
    bash scripts/readme-status-block.sh --check
    echo "verify OK"

# M0.1 `just ci-local`: the PR-tier CI gates, locally, in CI's own order.
# Mirrors .github/workflows/ci.yml: lint (fmt/clippy/warn-fallbacks),
# process templates, tests, metrics ratchet, the ledger gate suite, waiver
# health, dead-code check, and cargo-deny. NOT mirrored locally: the
# multi-OS test jobs, coverage/mutation advisories, MSRV (pinned by the
# dtolnay action), and `ledger diff` (it needs the PR base SHA and runs in
# CI against a worktree).
ci-local: require-llvm
    #!/usr/bin/env bash
    set -euo pipefail
    cd "$(git rev-parse --show-toplevel)"
    echo "== fmt --check =="
    cargo fmt --all -- --check
    echo "== clippy -D warnings =="
    cargo clippy --workspace --all-targets -- -D warnings
    echo "== warn-fallback heuristics =="
    bash .github/scripts/check-warn-fallbacks.sh
    echo "== process templates (M0.5) =="
    bash .github/scripts/check-templates.sh
    echo "== tests =="
    if cargo nextest --version >/dev/null 2>&1; then
        cargo nextest run --workspace --test-threads=2
    else
        echo "NOTICE: nextest not installed (run 'just bootstrap'); running plain cargo test (same tests, different runner)"
        cargo test --workspace
    fi
    echo "== metrics ratchet (M0.6) =="
    cargo run -q -p glyim-metrics -- . --check metrics/baseline.json
    echo "== ledger check (M1.1) =="
    cargo run -q -p glyim-ledger -- check .
    cargo run -q -p glyim-ledger -- evidence check .
    cargo run -q -p glyim-ledger -- evidence audit .
    echo "== STATUS is up to date (M1.10) =="
    cargo run -q -p glyim-ledger -- status . --check
    cargo run -q -p glyim-ledger -- status . --check --format json
    echo "== README status block (M1.9) =="
    bash scripts/readme-status-block.sh --check
    echo "== waiver health (C9) =="
    cargo run -q -p glyim-ledger -- waivers .
    echo "== dead-code burndown =="
    cargo clippy -p glyim-codegen --all-targets -- -W dead_code -D warnings
    cargo clippy -p glyim-frontend --all-targets -- -W dead_code -D warnings
    echo "== cargo-deny =="
    if cargo deny --version >/dev/null 2>&1; then
        cargo deny check
    else
        echo "NOTICE: cargo-deny not installed (run 'just bootstrap'); skipped locally, still enforced in CI's supply-chain job"
    fi
    echo "ci-local OK"

# ---- test ----

test:
    cargo nextest run --workspace --test-threads=2

test-fast:
    cargo nextest run --workspace --test-threads=2 --no-fail-fast

# The bench harnesses are `harness = false` binaries: compiling and running
# them under the plain test profile exercises every bench target without a
# full `cargo bench` release run (the nightly-free bench gate).
test-benches:
    cargo test --workspace --benches

test-corpus:
    cargo nextest run -p glyim-test --test run_pass_corpus --features glyim-test/real-llvm
    cargo nextest run -p glyim-test --test compile_pass_corpus --features glyim-test/real-llvm
    cargo nextest run -p glyim-test --test run_fail_corpus --features glyim-test/real-llvm
    cargo nextest run -p glyim-test --test compile_fail_corpus --features glyim-test/real-llvm

differential:
    GLYIM_TEST_REAL_LLVM=1 cargo nextest run -p glyim-test --test differential \
        --features real-llvm --test-threads=2 --no-fail-fast

# ---- fuzz (short smoke) ----

fuzz-smoke:
    cargo fuzz run fuzz_parse -- -max_total_time=60
    cargo fuzz run fuzz_compile -- -max_total_time=60

# ---- housekeeping ----

clean:
    cargo clean

clean-cache:
    rm -rf "$HOME/.glyip/cache/glyim"

# ---- per-session watcher (drives `wr1.sh`..`wr5.sh`) ----

wr1:
    watchexec -w ./wr1.sh --clear -r "./wr1.sh"
wr2:
    watchexec -w ./wr2.sh --clear -r "./wr2.sh"
wr3:
    watchexec -w ./wr3.sh --clear -r "./wr3.sh"
wr4:
    watchexec -w ./wr4.sh --clear -r "./wr4.sh"
wr5:
    watchexec -w ./wr5.sh --clear -r "./wr5.sh"
