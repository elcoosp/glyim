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

# ---- test ----

test:
    cargo nextest run --workspace --test-threads=2

test-fast:
    cargo nextest run --workspace --test-threads=2 --no-fail-fast

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
