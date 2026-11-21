# M2.6 baseline report — committed corpus budgets

docs/roadmaps/glyim-master-plan.md §6 M2.6: the initial `perf/budgets.toml`
is the baseline, and this document records where those numbers came from.
Every number below is a measurement of a real command on the committed
`tools/glyim-bench/corpus` programs (M2.3: 14 programs — all seven generator
families × seeds 0,1 at size 32), not an estimate.

## What is measured

One measured command per committed program:

```text
glyim --emit=run <corpus>/<family>-<seed>-32.g [--with-stdlib]
```

`--emit=run` executes in-process through the MIR interpreter (the GAM
reference runtime), so the budgeted quantity is the end-to-end interpreter
pipeline on each program — parse, check, lower, interpret. At size 32 the
program's own work is a small share of that, which is the point: the budget
watches the pipeline cost that the M2.7 quick-win campaign moves.

The M2.2 harness measured each command 10 times per run. Because the
harness requires the measured command to exit 0 while the corpus programs
exit with their pinned (masked) reference codes, each program is measured
through a generated wrapper that runs the program, enforces its pinned
reference output (the `//@ expect-exit` header — masked `% 256`, matching
the driver's masking — and the family's pinned stdout), and maps a match to
success. The wrapper's stable per-program name is also the budget key.

**12 of the 14 committed programs are measured.** The two `macro` programs
are committed and byte-verified by `corpus check`, but are not budgeted:
the CLI's `--emit=run --with-stdlib` path fails silently at HEAD on integer
`format!` substitutions (`format!("{56} + {12}")` → exit 1 with no
diagnostic; repro: `glyim --emit=run --with-stdlib
tools/glyim-bench/corpus/macro-0-32.g`). The same fixtures pass under the
`glyim-test` harness, which drives the pipeline directly rather than the
CLI. Fixing the CLI path (with a witness-first bug record, plan §7.2) is
queued with the next type-safety work; the budget rows land then.

## Measurement environment (baseline calibration)

| Property | Value |
|---|---|
| CPU | Intel(R) Xeon(R) Processor, 2 vCPU |
| Memory | 4 GiB |
| Kernel | 5.10.134-013.15.kangaroo.al8.x86_64 (Alibaba Linux 8) |
| Rust | 1.100.0 stable, release profile (`opt-level` 3), `debug=0` |
| LLVM | 23.1.4 (`LLVM_SYS_231_PREFIX` shim; the interpreter path does not link it, but building `glyim-cli` requires it) |
| Counter source | wall (`--source wall`; see the instruction-count note) |
| Date | 2026-11-21 |

**Wall noise.** This is a two-core shared filesystem sandbox; wall CoV over
the 10 samples ran 11–21% per program. The M2.2 stability gate's 0.1%
budget is written for instruction counters (deterministic by
construction) and is not achievable for wall on this hardware — the
committed absolute budgets therefore carry ×2 headroom, and the tight
regression detector is the relative matrix gate (fail > 2% per benchmark
or > 0.5% geo-mean, plan §6 M2.5). Instruction-count budgets (CoV-stable)
are the queued CI follow-up.

## Absolute budgets (`perf/budgets.toml`)

`max_wall_mean_ns` = measured mean × 2. The budgets file uses the committed
`glyim.bench-budgets/1` schema (`[[budget]] program / max_wall_mean_ns /
max_instructions_mean`); its `program` key is the measured command's
argv[0] (`./glyim-<family>-<seed>-32` under `target/bench-bin/`).

| Program | wall mean (ns) | budget (ns) | RSS max (KB) | CoV % |
|---|---:|---:|---:|---:|
| ./glyim-closure-0-32 | 16 696 269 | 33 392 537 | 21 668 | 20.95 |
| ./glyim-closure-1-32 | 16 803 991 | 33 607 982 | 21 732 | 11.16 |
| ./glyim-deep-0-32 | 14 275 932 | 28 551 864 | 21 944 | 14.18 |
| ./glyim-deep-1-32 | 14 047 716 | 28 095 431 | 21 948 | 14.25 |
| ./glyim-generic-0-32 | 14 075 459 | 28 150 917 | 21 340 | 13.61 |
| ./glyim-generic-1-32 | 13 630 834 | 27 261 668 | 21 328 | 12.18 |
| ./glyim-match-0-32 | 13 842 260 | 27 684 519 | 21 296 | 15.56 |
| ./glyim-match-1-32 | 13 727 838 | 27 455 675 | 21 384 | 13.92 |
| ./glyim-trait-0-32 | 19 878 104 | 39 756 208 | 21 964 | 21.09 |
| ./glyim-trait-1-32 | 19 048 926 | 38 097 851 | 22 080 | 15.65 |
| ./glyim-wide-0-32 | 13 125 715 | 26 251 429 | 21 204 | 13.71 |
| ./glyim-wide-1-32 | 13 481 285 | 26 962 570 | 21 112 | 14.46 |

Geo-mean wall ≈ 15.1 ms. RSS is measured per sample
(`getrusage(RUSAGE_CHILDREN)` max, KB) and recorded here because the
committed budget schema has no RSS column.

## Instruction counts

Instruction budgets are **not committed yet**: this environment has no
`perf` and no `valgrind`, so `glyim.bench/1` reports carry no instruction
stats and the budget gate would mark an instruction bound `Unmeasured`
(a wall-only report cannot satisfy an instruction budget — that is the
substrate's honesty rule, and committing an instruction number measured by
nothing would be worse than omitting it). The CI perf job
(`.github/workflows/perf.yml`, queued with the M2.5 milestone) runs on
runners where `perf` can be installed; switching the measurement to
`--source perf` there and ratcheting `max_instructions_mean` rows into
`perf/budgets.toml` is the queued follow-up.

## Reproducing

```text
just bench-ci          # corpus check + 10-iteration measurement + budget gate (+ matrix when history covers every program)
glyim-bench corpus check --dir tools/glyim-bench/corpus   # corpus integrity alone
```
