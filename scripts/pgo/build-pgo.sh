#!/usr/bin/env bash
# build-pgo.sh — the plan M2.8 PGO pipeline prototype for `glyim-cli`.
#
# Master plan §6 M2.8: "PGO pipeline prototype for `glyim-cli` (training =
# bench corpus) — DoD: reproducible PGO build script; measured delta
# recorded." This script IS that pipeline, and it stays inside the plan's
# existing measurement rails instead of inventing new ones:
#
#   1. instrumented build   RUSTFLAGS="-Cprofile-generate=DIR" (llvm-tools)
#   2. training             scripts/pgo/corpus-compile-driver.sh drives the
#                           committed M2.3 corpus (glyim.bench corpus check
#                           first: training data is the machine-checked one)
#   3. merge                rustc's own llvm-profdata (same LLVM that will
#                           consume the profile at -Cprofile-use)
#   4. baseline build       same release profile, no profile flags
#   5. optimized build      RUSTFLAGS="-Cprofile-use=merged.profdata"
#   6. measurement          `glyim-bench run` (M2.2 harness, wall/CoV gate)
#                           over both binaries, `glyim-bench diff` (M2.5
#                           seam) for per-metric verdicts
#   7. record              perf/pgo-report.md — versions, flags, raw means,
#                           and the diff verdicts, so the delta is committed
#                           evidence, not chat prose (constitution C1/C7)
#
# Reproducibility: fixed flag sets, deterministic training set (sorted
# corpus order, one `--emit=obj` compile per program), no clock in the
# recorded report. The delta itself is wall-clock on the host machine (this
# calibration environment has no `perf`/`valgrind`, so the M2.2 source
# auto-detects wall-only sampling exactly like `just bench-ci`); the report
# discloses the host, the source, and the iteration count.
#
# Usage: scripts/pgo/build-pgo.sh [--out PERF_PGO_REPORT] [--iterations N]
#                                  [--work-dir DIR] [--keep-work]
#
# Exit status: 0 when the pipeline completed and the delta was recorded;
# non-zero on any build/training/merge failure (the prototype never
# silently records a partial run).

set -euo pipefail

usage() {
    grep '^#' "$0" | sed -n '2,30p' | sed 's/^# \{0,1\}//'
    exit "${1-2}"
}

repo_root=$(cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repo_root"

OUT=perf/pgo-report.md
ITERATIONS=5
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/glyim-pgo.XXXXXX")
KEEP_WORK=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --out) OUT=$2; shift 2 ;;
        --iterations) ITERATIONS=$2; shift 2 ;;
        --work-dir) WORK_DIR=$2; KEEP_WORK=1; shift 2 ;;
        --keep-work) KEEP_WORK=1; shift ;;
        --help) usage 0 ;;
        *) echo "unknown option: $1" >&2; usage 2 ;;
    esac
done

DRIVER=scripts/pgo/corpus-compile-driver.sh
PROFRAW_DIR="$WORK_DIR/profraw"
TARGET_DIR="$WORK_DIR/target"
TOOLS_DIR="$WORK_DIR/tools-target"
BENCH_DIR="$WORK_DIR/bench"
mkdir -p "$PROFRAW_DIR" "$BENCH_DIR"

say() { printf '==> %s\n' "$*"; }

# ---- preflight -----------------------------------------------------------

say "preflight"
[ -x "$DRIVER" ] || { echo "missing driver: $DRIVER" >&2; exit 2; }

HOST_TARGET=$(rustc -vV | sed -n 's/^host: //p')
PROFDATA="$(rustc --print sysroot)/lib/rustlib/$HOST_TARGET/bin/llvm-profdata"
if [ ! -x "$PROFDATA" ]; then
    echo "llvm-profdata missing from the rustc sysroot; run:" >&2
    echo "  rustup component add llvm-tools-preview" >&2
    exit 2
fi

if [ -n "${LLVM_SYS_231_PREFIX:-}" ]; then
    "$LLVM_SYS_231_PREFIX/bin/llvm-config" --version > /dev/null 2>&1 || {
        echo "LLVM_SYS_231_PREFIX is set but its llvm-config does not run" >&2
        exit 2
    }
fi

cargo build -q -p glyim-runtime
cargo build -q --release -p glyim-bench --target-dir "$TOOLS_DIR"
BENCH_BIN="$TOOLS_DIR/release/glyim-bench"
[ -x "$BENCH_BIN" ] || BENCH_BIN="$TOOLS_DIR/debug/glyim-bench"

# Training data is the machine-checked corpus, not a hopeful directory.
"$BENCH_BIN" corpus check --dir tools/glyim-bench/corpus > /dev/null

RUSTC_V=$(rustc -vV | sed -n 's/^rustc //p')
LLVM_V=$("$PROFDATA" --version | grep -oE 'LLVM version [0-9][0-9.]*' | head -1)
COMMIT=$(git rev-parse HEAD)

# ---- builds ---------------------------------------------------------------

say "instrumented build (-Cprofile-generate)"
RUSTFLAGS="-Cprofile-generate=$PROFRAW_DIR" \
    cargo build -q --release -p glyim-cli --target-dir "$TARGET_DIR"
INSTRUMENTED="$TARGET_DIR/release/glyim-cli"

say "training on the committed corpus"
"$DRIVER" "$INSTRUMENTED" "$PROFRAW_DIR"

say "merge profile"
PROFDATA_FILE="$WORK_DIR/glyim.profdata"
"$PROFDATA" merge -output "$PROFDATA_FILE" "$PROFRAW_DIR"/*.profraw

say "baseline build (no profile flags)"
cargo build -q --release -p glyim-cli --target-dir "$TARGET_DIR"
BASELINE_BIN="$TARGET_DIR/release/glyim-cli"

say "optimized build (-Cprofile-use)"
RUSTFLAGS="-Cprofile-use=$PROFDATA_FILE" \
    cargo build -q --release -p glyim-cli --target-dir "$TARGET_DIR"
PGO_BIN="$TARGET_DIR/release/glyim-cli"

# ---- measurement ----------------------------------------------------------

# `glyim-bench run` exits non-zero when the M2.2 stability gate fails. On
# this calibration host the gate budget (0.1% CoV) is written for
# instruction counters and is not achievable for wall sampling — the same
# situation `perf/BASELINE.md` documents — so the gate failure is recorded,
# not fatal. A missing report, on the other hand, is.

say "baseline measurement ($ITERATIONS iterations, corpus driver)"
"$BENCH_BIN" run --iterations "$ITERATIONS" --json "$BENCH_DIR/base.json" \
    -- "$DRIVER" "$BASELINE_BIN" > "$BENCH_DIR/base-run.log" 2>&1 || true
[ -f "$BENCH_DIR/base.json" ] || {
    echo "baseline bench produced no report; see $BENCH_DIR/base-run.log" >&2
    exit 2
}

say "PGO measurement ($ITERATIONS iterations, corpus driver)"
"$BENCH_BIN" run --iterations "$ITERATIONS" --json "$BENCH_DIR/pgo.json" \
    -- "$DRIVER" "$PGO_BIN" > "$BENCH_DIR/pgo-run.log" 2>&1 || true
[ -f "$BENCH_DIR/pgo.json" ] || {
    echo "PGO bench produced no report; see $BENCH_DIR/pgo-run.log" >&2
    exit 2
}

say "diff verdicts"
# A regression verdict is a recorded outcome, not a pipeline failure: the
# prototype's contract is "measured delta recorded", whatever its sign.
"$BENCH_BIN" diff --json "$BENCH_DIR/diff.json" \
    "$BENCH_DIR/base.json" "$BENCH_DIR/pgo.json" > "$BENCH_DIR/diff.log" 2>&1 || true
[ -f "$BENCH_DIR/diff.json" ] || {
    echo "diff produced no outcome; see $BENCH_DIR/diff.log" >&2
    exit 2
}

# ---- record ---------------------------------------------------------------

say "record $OUT"
python3 - "$OUT" "$BENCH_DIR" "$ITERATIONS" "$COMMIT" "$RUSTC_V" "$LLVM_V" "$HOST_TARGET" <<'PY'
import json
import sys

out, bench_dir, iterations, commit, rustc_v, llvm_v, host = sys.argv[1:8]
base = json.load(open(f"{bench_dir}/base.json"))
pgo = json.load(open(f"{bench_dir}/pgo.json"))
diff = json.load(open(f"{bench_dir}/diff.json"))


def fmt_ns(mean_ns):
    if mean_ns is None:
        return "-"
    return f"{mean_ns / 1e6:.2f} ms"


base_wall = base["wall"]["mean"]
pgo_wall = pgo["wall"]["mean"]
base_cov = base["wall"]["cov_pct"]
pgo_cov = pgo["wall"]["cov_pct"]
rows = []
for program_diff in diff["diffs"]:
    for key in ("instructions", "wall"):
        metric = program_diff.get(key)
        if metric is not None:
            rows.append(metric)

lines = [
    "# M2.8 PGO pipeline prototype — measured delta",
    "",
    "Generated by `scripts/pgo/build-pgo.sh`; the committed record of one",
    "reproducible pipeline run (instrument → train on the committed M2.3",
    "corpus → merge → `-Cprofile-use`). Constitution C1: this file records",
    "what the machine measured, nothing else.",
    "",
    f"- base commit: `{commit}`",
    f"- toolchain: rustc `{rustc_v}`, profdata backend `{llvm_v}`",
    f"- host: `{host}`, counter source: `{base['source']}` (auto-detected by",
    "  the M2.2 harness; wall-only where `perf`/`valgrind` are absent)",
    f"- iterations per side: {iterations}; measured command: the corpus",
    "  driver (`scripts/pgo/corpus-compile-driver.sh`: every committed",
    "  corpus program × one `--emit=obj` compile, per iteration)",
    "- flags: baseline = workspace release profile; PGO = the same profile",
    "  plus `-Cprofile-generate` during training and `-Cprofile-use` after",
    "  merging; stability gates (budget 0.1% CoV, written for instruction",
    "  counters — wall-only here, see perf/BASELINE.md): baseline passed="
    f"{str(base['gate']['passed']).lower()}, PGO passed="
    f"{str(pgo['gate']['passed']).lower()}",
    "",
    "| build | wall mean | wall CoV |",
    "|---|---|---|",
    f"| baseline | {fmt_ns(base_wall)} | {base_cov}% |",
    f"| PGO (-Cprofile-use) | {fmt_ns(pgo_wall)} | {pgo_cov}% |",
    "",
    f"Overall diff gate (`glyim.bench-diff/1`, budget +{diff['max_delta_pct']}%):",
    f"passed={str(diff['passed']).lower()}",
    "",
    "Per-metric rows (wall-clock deltas are host-dependent; instruction",
    "rows appear when the counter source provides them):",
    "",
    "| metric | baseline mean (ns) | PGO mean (ns) | delta | verdict |",
    "|---|---|---|---|---|",
]
for metric in rows:
    lines.append(
        f"| {metric['metric']} | {metric['baseline']:.0f} | "
        f"{metric['current']:.0f} | {metric['delta_pct']:+.2f}% | "
        f"{metric['verdict']} |"
    )
lines += [
    "",
    "The measured delta is wall-clock on the recording host; the CI perf",
    "job is where instruction-level rows get ratcheted in once `perf`/",
    "`valgrind` sources are available (same pipeline, different M2.2",
    "counter source).",
    "",
]
with open(out, "w") as fh:
    fh.write("\n".join(lines))
print(f"recorded: {out}")
PY

if [ "$KEEP_WORK" -eq 0 ]; then
    say "clean work dir $WORK_DIR"
    rm -rf "$WORK_DIR"
else
    say "kept work dir $WORK_DIR (reports in $BENCH_DIR)"
fi

say "done"
