#!/usr/bin/env bash
# corpus-compile-driver.sh — the M2.8 PGO training/benchmark driver.
#
# Compiles every committed bench-corpus program (`tools/glyim-bench/corpus/`,
# plan M2.3 slice 2) once with the given `glyim` binary, `--emit=obj`, so the
# measured work is the full compiler pipeline (lex → parse → expand →
# def-map → HIR → typeck → lower → mono → borrowck → opt → LLVM codegen →
# object write) with no link-step and no interpreter-run variance. The same
# command serves both faces of the PGO pipeline:
#
# - *training*: invoked under `-Cprofile-generate` with a `--train-dir`, it
#   exercises exactly the code paths the optimized build should specialize;
# - *benchmarking*: invoked through `glyim-bench run --` (plan M2.2/C7:
#   counts are only evidence when a harness measures them), once for the
#   baseline binary and once for the profile-use binary.
#
# Determinism / comparability rules:
#
# - Every compile gets a **fresh `GLYIM_CACHE_DIR`**: the whole-crate
#   SHA-256 object cache (`glyim-db/src/cache.rs`) would otherwise turn
#   every iteration after the first into a cache hit, measuring nothing.
#   Cold compile is the measured unit, for training and for both bench
#   sides alike.
# - Programs run in `ls`-sorted (byte) order, no clock, no PRNG.
# - The two `macro` programs compile with `--with-stdlib`: their source
#   uses `println!`/`format!` and the manifest records empty compile
#   flags. `perf/BASELINE.md` excludes them from the *interpreter-run*
#   budgets because the CLI's `--emit=run --with-stdlib` path fails at
#   HEAD; the compile-only path here is unaffected (the fixtures pass
#   under the `glyim-test` harness, which drives the same pipeline), so
#   they stay in the training/bench set with their one documented flag.
#
# Exit status is non-zero if any corpus program fails to compile: a corpus
# program that cannot be compiled by the compiler-under-test is a bug, not
# a gap (the corpus is byte-verified by `glyim-bench corpus check`).
#
# Usage: corpus-compile-driver.sh <glyim-bin> [train-dir]
#   <glyim-bin>  the compiler binary to drive
#   [train-dir]  when set, each invocation writes its `.profraw` into this
#                directory via LLVM_PROFILE_FILE (training mode)

set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "usage: $0 <glyim-bin> [train-dir]" >&2
    exit 2
fi

GLYIM_BIN=$1
TRAIN_DIR=${2-}

if [ ! -x "$GLYIM_BIN" ]; then
    echo "error: '$GLYIM_BIN' is not an executable" >&2
    exit 2
fi

repo_root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
corpus_dir="$repo_root/tools/glyim-bench/corpus"

if [ ! -d "$corpus_dir" ]; then
    echo "error: corpus directory not found: $corpus_dir" >&2
    exit 2
fi

obj_dir=$(mktemp -d)
trap 'rm -rf -- "$obj_dir"' EXIT

shopt -s nullglob
programs=("$corpus_dir"/*.g)
shopt -u nullglob

if [ "${#programs[@]}" -eq 0 ]; then
    echo "error: no corpus programs found in $corpus_dir" >&2
    exit 2
fi

for program in "${programs[@]}"; do
    name=$(basename -- "$program" .g)
    flags=()
    case "$name" in
        macro-*)
            # See the header: stdlib-dependent source, compile-only path.
            flags+=(--with-stdlib)
            ;;
    esac
    # Training mode: one profraw per process (LLVM default naming would
    # collide across processes; %p pins it per-PID).
    if [ -n "$TRAIN_DIR" ]; then
        mkdir -p -- "$TRAIN_DIR"
        export LLVM_PROFILE_FILE="$TRAIN_DIR/glyim-%p.profraw"
    fi
    # Cold cache per compile — see the header. The dir is removed with the
    # obj dir at exit; per-compile subdirs keep the loop hermetic.
    cache_dir=$(mktemp -d)
    GLYIM_CACHE_DIR="$cache_dir" \
        "$GLYIM_BIN" "$program" --emit=obj -o "$obj_dir/$name.o" "${flags[@]+"${flags[@]}"}" > /dev/null
    rm -rf -- "$cache_dir" "$obj_dir/$name.o"
done
