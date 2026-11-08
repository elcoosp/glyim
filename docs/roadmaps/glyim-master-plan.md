# Glyim Master Plan — Provable Status, Radical Speed, Bugs Hard by Construction

> **Audience:** an autonomous AI implementation agent (or a small fleet of them) plus the human maintainers who review its work.
> **Horizon:** ~60 weeks (≈14 months), 13 milestones (M0–M12), three coordinated workstreams.
> **Repo analysed:** `github.com/elcoosp/glyim` @ `2d0089d5` (35 workspace members, ≈235k lines of Rust under `crates/` including tests, ≈7.3k lines of Glyim stdlib source).
> **Plan date:** 2026-11-08. Week 1 = the week this plan is accepted.
> **API policy:** *Breaking changes are explicitly allowed and encouraged* whenever they make a bug class unrepresentable. There is no backward-compatibility window; every breaking change ships with a codemod and updates all call sites in the same PR.

---

## 0. TL;DR

Glyim already has an impressive amount of machinery (full frontend, trait solver, NLL borrowck, MIR, LLVM backend, LSP, package manager, ~4.6k unit tests, a 99-fixture LLVM-vs-interpreter differential gate). What it lacks is **three things that turn a promising compiler into a trustworthy one**:

| Gap | Today | Target |
|---|---|---|
| **Truth about status** | Prose files (`KNOWN_GAPS.md`, `HANDOFF.md`, `FIXES.md`, `docs/status/*`) that *admit they go stale* | A **Readiness Ledger**: status is a pure function of machine-checked evidence; no human-typed status fields exist |
| **Speed** | Straight-line 614-line driver, whole-crate SHA-256 cache, stdlib re-compiled from source on every run, zero compiler parallelism, no benchmarks | Query-based incremental + parallel compiler, precompiled stdlib with zero-copy metadata, stable CGUs + object cache, verified mid-end optimizer, deterministic perf gates |
| **Bug resistance** | 643 public `from_raw` ID constructors, 524 non-test `unwrap/expect`, 238 `unsafe` outside the runtime, panics as MIR validation, error-recovery sentinels that lower to zero values, three executors with different semantic models | Illegal states unrepresentable (typed IR pipeline, `ErrorGuaranteed`, private constructors), **one** semantic oracle, translation validation of optimizations, SMT-verified rewrite rules, generative + metamorphic + differential testing at scale |

The sequencing principle: **measure first (M1–M2), build the safety net (M4), then perform the big breaking refactors (M3, M5), then the performance architecture (M6–M8), then the deep semantic/optimizer work (M9–M11), then harden (M12).** Every milestone ends with machine-checked exit gates that are themselves tracked in the ledger.

### How to read this document

1. §1 is the evidence (what the code actually looks like). §2 is the constitution (rules that never bend).
2. §3, §4, §5 are the three workstreams as *designs*. They define IDs (`LG-*`, `PF-*`, `CT-*`/`CO-*`/`CP-*`) that §6's milestones reference.
3. §6 is the execution plan with work items and exit gates. §7 is the agent operating protocol. §8 lists the architecture decisions (ADRs) with proposed defaults. §9 risks. §10 release gates.
4. Appendices: bug-class→prevention map, ledger schemas, feature taxonomy seed, command cheat-sheet, references.

> **Honesty note.** The analysis below comes from static reading (directory survey, targeted greps, reading key files and the repo's own audit documents). The sandbox that produced this plan had no LLVM 23, so nothing was built or benchmarked. Every number in §1 is re-derived mechanically in **M0.6** and frozen in `metrics/baseline.json`; if a number is wrong, the baseline wins. Performance targets are expressed as *ratios against the M2 baseline*, not absolute numbers.

---

## 1. Diagnosis — what the codebase actually looks like

### 1.1 Architecture snapshot

```
source.g ─► lexer(Vec<Token>, SmolStr text) ─► Rowan CST ─► macro expand ─► def-map
         ─► HIR ─► typeck (InferenceTable + SimpleTraitSolver) ─► THIR ─► lower (MIR, mono, CGU split)
         ─► borrowck ─► glyim_opt::optimize (5 passes) ─► LLVM (inkwell) | bytecode backend ─► link (cc)
                                        MIR interpreter ◄── const-eval / tests / differential oracle
```

35 crates; largest: `glyim-codegen-llvm` 24k LOC, `glyim-typeck` 21k, `glyim-lower` 17k, `glyim-mir-interp` 15.6k, `glyim-type` 15.5k, `glyim-hir` 13.8k, `glyim-frontend` 13.4k, `glyim-borrowck` 10k, `glyim-solve` 9.9k.

### 1.2 Findings register

Each finding names its evidence so it can be re-checked (M0.6). `Fn` IDs are referenced by later sections.

| ID | Finding | Evidence |
|---|---|---|
| **F1** | **Status lives in prose and goes stale.** `KNOWN_GAPS.md` states its previous revision was stale (three entries described behaviour since implemented, one claim was plain wrong). `HANDOFF.md` (session 34) says the tree is "LIKELY DIRTY" with uncommitted debug instrumentation. Playbook §2.2 catalogues README/TECHSTACK drift. | `KNOWN_GAPS.md` header; `HANDOFF.md §0`; `docs/roadmaps/glyim-production-readiness-playbook.md §2.2` |
| **F2** | **No query layer despite the spec promising one.** `glyim-db::Database` is an interner + VFS + a `TyCtx` handle + a `Vec<String>` mono cache. The driver `Pipeline::compile_file_with_artifacts` is one ≈614-line straight-line function. Caching is whole-crate: SHA-256 of flattened source+flags → `.o`. | `crates/glyim-db/src/lib.rs`; `glyim-pipeline/src/lib.rs:372-985`; `glyim-db/src/cache.rs` header; `docs/specs/v0.1.0.md` ("Query-Based Compiler") |
| **F3** | **Single-crate world; stdlib recompiled from source every time.** `Database.krate = CrateId(0)`. `--with-stdlib` *prepends the assembled stdlib source* (≈7.3k lines of `.g`, 28 modules) to the user's program, so every compile re-parses/re-typechecks the stdlib. `glyip` can only produce objects, not linked executables. | `glyim-db/src/lib.rs`; `glyim-cli/src/lib.rs:491`; playbook X-01 |
| **F4** | **No compiler parallelism.** `rayon` is used only in the test harness. LLVM codegen is a sequential loop that creates one LLVM module *per body*; ThinLTO shells out to `llvm-lto2`. | `rg rayon`; `glyim-codegen-llvm/src/lib.rs:~469`; `README` LTO notes |
| **F5** | **No performance measurement at all.** No `benches/`, no criterion/divan/iai in any manifest, no `--time-passes`. `regalloc2` is a workspace dependency used by nothing. | `find . -name benches`; `grep regalloc2` |
| **F6** | **Thin optimizer.** `glyim_opt::optimize` clones the whole `Body`, runs 5 fixed passes (slice-desugar, const-prop, DCE, CFG-simplify, unreachable-elim) and `panic!`s on validation failure. No inlining, GVN, copy-prop, SROA, jump-threading, range/bounds analysis. | `glyim-opt/src/lib.rs:45-77` |
| **F7** | **Allocation-heavy frontend.** Every token carries a `SmolStr`; the whole `Vec<Token>` is materialised, then a Rowan green tree, then CST→HIR lowering. Global `ThreadedRodeo` interner. | `glyim-frontend/src/lexer.rs:14-35`; `glyim-core/src/interner.rs` |
| **F8** | **Type system is not parallel- or cache-ready.** Mutable `TyCtxMut` → frozen `TyCtx` model; arena is `Arc<TypeArena>` over raw-pointer storage with `unsafe impl Send/Sync` resting on a doc-comment invariant. The trait solver *linearly scans every impl of a trait* per obligation and again O(n²) for overlap checks; the only map in `solver.rs` is `lang_traits` (no result cache). | `glyim-type/src/type_arena.rs:70`; `glyim-solve/src/solver.rs:168-173, 426-441` |
| **F9** | **Invariants are conventions, not types.** `from_raw` is `pub` on every ID (643 call sites). `Span::new`'s `lo<=hi` guard was a `debug_assert` that vanished in release (FIXES §4). 524 non-test `unwrap/expect`; 238 `unsafe` outside `glyim-runtime`. MIR `Body`/`Statement` have all-public fields; backends take `Vec<(String, Arc<Body>)>`; `StatementKind::Nop` exists; `Rvalue::BinaryOp` heap-allocates a `Box<(Operand,Operand)>` per binary op. | `glyim-core/src/def_id.rs`; `glyim-mir/src/lib.rs`; `FIXES.md §4` |
| **F10** | **Error-recovery can silently miscompile.** Sentinel builtins lower to zero values on error arms; `ConstRef` constants are materialised as zero globals (playbook X-01). `TyKind::Projection` can reach LLVM codegen (ICE, HANDOFF §2). `LocalDefId` numbering shifted with proc-macro expansion order and picked the *wrong entry function* (KNOWN_GAPS §5) — IDs are neither stable nor position-independent. | playbook Appendix A; `HANDOFF.md §2`; `KNOWN_GAPS.md §5` |
| **F11** | **Three executors, three semantic models.** LLVM native; a *value-model* MIR interpreter that needed a "dead-frame archive" hack to agree with native; a bytecode VM (+8k-LOC bytecode backend) with its own wire format (audit found inverted branches, wire-format desync). Differential covers 99 fixtures. | `FIXES.md §2`; audit RT-3/4/11 |
| **F12** | **Bugs cluster by class, but fixes are one-offs.** The in-repo audit lists 193 findings: HIR 35, RT 33, INF 32, MIR 31, SOLVE 23, FE 20, LL 19; many are the *same* mistake repeated (missing variant arm, off-by-one in list parsing, signedness, stale dataflow facts). No mechanism prevents recurrence. | `docs/roadmaps/glyim-bug-and-performance-audit.md` |
| **F13** | **Testing is broad but not generative.** ≈4.6k `#[test]`, ≈290 `.g` fixtures (188 in `run-pass/behavior`), 30 no-ICE, 2 fuzz targets (`fuzz_parse`, `fuzz_compile`). No program generator, no metamorphic or translation-validation testing, no incremental-vs-clean oracle, mutation testing is "report only". CI does have differential, miri, msrv, lint, supply-chain, coverage jobs. | `crates/glyim-test/tests`, `fuzz/fuzz_targets`, `.github/workflows/ci.yml` |
| **F14** | **Lint/diagnostic infrastructure is embryonic.** Two lint IDs (`UnsafeFnCall`, `UnusedGenericParams`); ratchet lints only in a few crates; `ErrorCode{category, number}` is constructed ad hoc (no registry, no explain text). | `glyim-core/src/lint.rs`; `glyim-diag/src/lib.rs` |

### 1.3 Assets to build on (do not throw away)

- `glyim-test`: fixture discovery, `//~ ERROR` annotations, snapshots, JSON reporter (`GLYIM_TEST_JSON`), real-LLVM differential gate.
- `tools/glyim-pilot`: an agent orchestrator with gates (`contracts`, `architecture`, `mutation`, `coverage`, `clippy`, `fmt`, `dead_code`). The ledger plugs in as one more gate.
- `Ty(u32)` interned handle design, `IndexVec`, content-addressed cache with atomic writes + bounded size, proc-macro C ABI, `glyim-docs` (Astro site) for publishing status.
- The existing playbook/audit documents: they become *seed data* for the gap registry (M1, M4) rather than prose to maintain.

---

## 2. Constitution — rules that never bend

| # | Rule | Enforced by |
|---|---|---|
| **C1** | **Status is computed, never typed.** No human-editable `status`/`level`/`done` field exists anywhere in the registry schema. | schema `deny_unknown_fields`; `ledger check` |
| **C2** | **Every claim has a witness.** A feature "works" iff a passing fixture says so; a gap "exists" iff a *strict-xfail* fixture reproduces it. Closing a gap = the xfail starts passing (test fails until the gap entry is closed). | harness strict-xfail (M1.6) |
| **C3** | **Make illegal states unrepresentable before testing for them.** Prefer types > constructors > validators > lints > tests > docs, in that order. | review checklist §7 |
| **C4** | **No panics on user input, ever.** User-reachable failure = diagnostic or `ice!` with a repro bundle; `unwrap/expect/panic/unreachable/indexing` are denied outside allow-listed invariant sites. | workspace lints + ledger ratchet |
| **C5** | **One semantic truth.** One abstract-machine spec (GAM), one reference interpreter, one const-evaluator (the interpreter). Backends are differential-tested against it. | M9 |
| **C6** | **Deterministic by construction.** Same inputs ⇒ byte-identical outputs across thread counts, hash seeds, absolute paths, machines. No `HashMap` iteration in compiler core. | `disallowed-types`; chaos CI (CT-9) |
| **C7** | **Measure, then optimize; budgets are tests.** Every perf claim is an instruction-count delta from a committed benchmark; regressions fail CI. | M2 |
| **C8** | **Every bug fix leaves a class-level defence.** A fix PR must add (a) a witness fixture and (b) either a generator rule, a metamorphic relation, a validator check, or a lint that would have caught the *class*. | PR template; `ledger bugclass` (CP-1) |
| **C9** | **Gates never weaken.** Thresholds only tighten; waivers expire in ≤14 days. | `ledger diff` + `waivers.toml` |
| **C10** | **Breaking is fine; silent is not.** APIs may break freely, but every break has a codemod, a migration note, and a ledger diff. | §7 protocol |

---

## 3. Workstream A — The Readiness Ledger (deterministic production-readiness tracking)

**Goal:** answer "how production-ready is Glyim, per feature, per backend, per platform?" with a number you can reproduce on any machine from the commit hash, and make that answer drive the agent's work queue.

### 3.1 Core idea

```
 registry (what we CLAIM to build)      evidence (what the machine OBSERVED)
 ledger/features/**/*.toml              target/ledger/evidence.jsonl  (test harness, fuzz farm, bench)
 ledger/gaps/**/*.toml                  ledger/evidence/fuzz.jsonl    (append-only campaign records)
 ledger/policy.toml (level rules)       perf/history/*.jsonl
            │                                      │
            └──────────────► pure function  ◄──────┘
                         ledger compute (no clock, no randomness, sorted keys)
                                      │
            ┌─────────────────────────┼───────────────────────────┐
            ▼                         ▼                           ▼
     ledger/STATUS.json        ledger/STATUS.md            next-work queue (ledger next)
     (golden, committed)       (generated, committed)      release gates (ledger gates)
```

- **Registry** declares features, gaps, gates. It has **no status field** (C1).
- **Evidence** is produced by tools that run things (tests, fuzzers, benchmarks) and is content-addressed (fixture hash, compiler-binary hash, toolchain versions).
- **`ledger compute`** is a pure function of (registry, evidence, policy). CI re-runs it and fails if the committed `STATUS.json/md` differ (`git diff --exit-code`). Running it twice, on two machines, must give identical bytes (tested in M1.13).

### 3.2 Maturity levels (per feature × backend)

| Level | Name | Evidence required (all must hold, cumulative) |
|---|---|---|
| **L0** | Declared | Registry entry with spec reference and `target_level`. |
| **L1** | Parses | ≥1 parse-pass fixture; zero diagnostics; CST round-trip is lossless (`print(parse(x)) == x`). |
| **L2** | Checks | ≥1 positive typeck/borrowck fixture **and** ≥1 negative fixture per documented error code of the feature; diagnostics are span-valid. |
| **L3** | Interpreted | Run-pass fixtures on the reference interpreter (GAM) with stdout+exit-code expectations (fixtures with no expectations do not count). |
| **L4** | Native-verified | Run-pass on LLVM at `-O0` and `-O2`; LLVM≡interpreter differential agrees; the feature-tagged generator has run ≥ *N* CPU-hours of fuzzing with no ICE (N from policy). |
| **L5** | Hardened | Metamorphic suite clean; translation validation clean on the passes that touch it; mutation score of implementing code ≥ τ; perf budget exists and is met; `--explain`/docs exist; **zero open class-b gaps**; stability tag set. |

**Gap caps.** An open gap carries `caps = "L3"` (or similar) which *limits* the level of every feature in `affects`, regardless of other evidence. A silent-miscompile gap (class b) therefore automatically pulls a feature out of "native-verified".

### 3.3 The gap registry and strict-xfail witnesses

The repo's own taxonomy (KNOWN_GAPS): **class a** = deliberate hard error with a diagnostic; **class b** = silent wrong behaviour (worst); **class c** = deferred, no code path. The ledger adopts it verbatim.

```toml
# ledger/gaps/async/0007-await-in-loop.toml
[[gap]]
id        = "GAP-0007"
class     = "a"                     # a | b | c
title     = ".await inside a loop body rejected by async lowering"
affects   = ["LANG-ASYNC-AWAIT-LOOP"]
caps      = "L2"
witness   = "crates/glyim-test/tests/gaps/async_await_in_loop.g"
expect    = { diagnostic = "T0060" }   # what the witness must do WHILE the gap is open
opened_in = "baseline-2026-11"
target_milestone = "M10"
prevention = ["CO-5:generator-await-loops"]   # class-level defence that must land when closed (C8)
```

Witness fixture header: `//@ known-gap: GAP-0007`. The harness **inverts** the expectation: the fixture *passes* while the bug reproduces as declared and **fails** (loudly, "GAP-0007 appears fixed — close it and keep this as a regression test") the moment behaviour changes. This makes gap-closure deterministic and un-forgettable, and makes stale entries impossible (the previous `KNOWN_GAPS.md` failure mode).

Gaps without a reproducible witness are allowed only as `status = "unwitnessed"` and count as **witness debt**, a ratcheted metric.

### 3.4 Evidence records

One JSON line per (fixture × backend × opt-level) executed:

```json
{"schema":1,"commit":"2d0089d5","fixture":"crates/glyim-test/tests/run-pass/behavior/for_loop_over_slice.g",
 "fixture_hash":"blake3:9f…","features":["LANG-FOR-SLICE","LANG-SLICE-INDEX"],"mode":"run-pass",
 "backend":"llvm","opt":"O2","result":"pass","exit":3,"stdout_hash":"blake3:…",
 "instructions":184213377,"max_rss_kb":91244,
 "tool":{"glyim":"blake3:…","llvm":"23.1.x","rustc":"1.95.0","target":"x86_64-unknown-linux-gnu"}}
```

Non-deterministic activities (fuzzing, soak) are recorded as **campaign records** in an append-only file (`ledger/evidence/fuzz.jsonl`: campaign id, corpus hash, target, CPU-seconds, findings). The *computation* stays deterministic because campaigns are inputs.

### 3.5 Inventory extractors (drift is impossible)

`ledger inventory` enumerates, from source, everything that *should* map to a feature, and CI fails if any item is neither mapped nor explicitly ignored (with a reason) in `ledger/inventory-ignore.toml`:

| Source | Extractor | Examples |
|---|---|---|
| `SyntaxKind` (glyim-syntax) | `syn` | every grammar production/token kind |
| `TyKind`, `AdtKind`, `BuiltinTrait` | `syn` | type forms, builtin traits |
| MIR `StatementKind`, `Rvalue`, `TerminatorKind`, `ProjectionElem`, `Operand` | `syn` | per-variant backend-parity rows |
| Error codes / lints | registry files | `T0060`, `UnsafeFnCall` |
| CLI flags | `clap` introspection (`--help-json`) | `--lto`, `--emit` |
| Stdlib public API | Glyim's own parser on `glyim-lang-*/lib/*.g` | every `pub fn`/type/trait |
| Runtime FFI | `extern "C"` scan of `glyim-runtime` and stdlib | `glyim_alloc`, `glyim_panic` |

Adding a new MIR variant without registering a feature therefore *fails CI* — and because consumers must match exhaustively (CT-7) it also fails to compile until every backend handles it.

### 3.6 Construct-support matrix (backend parity, automatically)

Behind `--features ledger-trace`, backends and the interpreter bump a counter each time they lower/execute an IR variant. Fixture runs emit the counters into evidence. `ledger matrix` renders **variant × {GAM, LLVM-O0, LLVM-O2, Cranelift} × fixture-count**. A variant lowered by LLVM but executed by no fixture is flagged; a variant supported by one backend but not another is a visible parity gap, not a surprise.

### 3.7 CLI surface (`tools/glyim-ledger`)

| Command | Purpose |
|---|---|
| `ledger check` | Registry consistency: schema, unique IDs, every fixture tag resolves, every gap witness exists and is xfail-marked, every inventory item mapped, no orphan error codes. **Required CI check.** |
| `ledger run [--tier pr\|nightly\|weekly]` | Run the test tiers hermetically and write evidence. |
| `ledger compute` | Pure computation → `STATUS.json`, `STATUS.md`, generated `KNOWN_GAPS.md`. |
| `ledger diff <base> <head>` | Level/gate regressions; **fails CI on regression** unless an unexpired waiver exists. |
| `ledger next [--lane L] [--n 10]` | Deterministic prioritised work queue (see 3.9). |
| `ledger explain <ID>` | Why a feature/gap has its current level; what evidence is missing for the next level. |
| `ledger gates` | Release gates RG-01…RG-24 with pass/fail/unmeasured. |
| `ledger matrix` | Construct-support matrix. |
| `ledger bugclass` | Prevention-debt report (CP-1). |
| `ledger handoff` | Generates the (short) session handoff from state — replaces hand-written `HANDOFF.md`. |
| `ledger release-check vX.Y.Z` | Final release gate. |

### 3.8 Registry schema (feature)

```toml
# ledger/features/lang/for-loops.toml
[[feature]]
id            = "LANG-FOR-VEC-INTOITER"
title         = "`for x in vec` via IntoIterator::into_iter"
area          = "lang.control-flow"
tier          = "core"                      # core | std | tooling | experimental
spec          = "docs/spec/control-flow.md#for-loops"
target_level  = "L5"
backends      = ["gam", "llvm"]             # backends that must reach target_level
requires      = ["LANG-TRAIT-ASSOC-TYPES", "STD-VEC-INTOITER"]
errors        = ["T0113"]                   # error codes this feature may emit (must each have a negative fixture)
owner_lane    = "M"
```

Fixture tagging (all `.g` fixtures; codemod in M1.4):

```
//@ feature: LANG-FOR-VEC-INTOITER, STD-VEC-INTOITER
//@ mode: run-pass
//@ backends: gam, llvm
//@ expect-exit: 6
//@ expect-stdout: ""
```

### 3.9 `ledger next` — a deterministic work queue for the agent

Priority = lexicographic on:
1. Open **class-b gaps** (silent miscompiles) — always first.
2. Failing/regressed **release gates** with the smallest remaining distance.
3. **Witness debt** and **prevention debt** (fixed bugs lacking a class-level defence).
4. Features whose `target_level` is blocked only by *missing evidence* (cheapest wins first).
5. Features blocked by *implementation* (ordered by `requires` topological depth, then by number of dependants).

The output is JSON (item id, why, evidence needed, suggested files, suggested lane) consumed by `glyim-pilot`.

### 3.10 Readiness gates (RG) — the boolean definition of "production-ready"

Numbers marked *cal.* are calibrated from the M2/M4 baselines and then only tightened.

| ID | Gate | Pass condition |
|---|---|---|
| RG-01 | No ICE | 0 ICEs over the full fuzz corpus + generated programs; ≥ 1,000 cumulative CPU-hours of fuzzing, last 100 CPU-hours clean |
| RG-02 | Backend agreement | 0 disagreements GAM vs LLVM-O0 vs LLVM-O2 over all fixtures and ≥ 1M generated programs per release candidate |
| RG-03 | Optimizer validity | 0 translation-validation violations on the optimization pipeline over fixtures + ≥ 1M generated MIR bodies; TV "unknown" rate ≤ *cal.* |
| RG-04 | Determinism | Byte-identical outputs across threads {1,2,4,8,16}, hash seeds, path prefixes, two machines |
| RG-05 | Incremental soundness | 10k random edit sequences: incremental ≡ clean (diagnostics + object bytes) |
| RG-06 | Diagnostics quality | Every error code documented (`--explain`), has ≥1 negative fixture, spans valid, ordering deterministic |
| RG-07 | Panic/unsafe surface | `unwrap/expect/panic` outside allow-list = 0; `unsafe` only in allow-listed modules, each Miri-clean |
| RG-08 | Test strength | Mutation score ≥ *cal.* (start 60%, ratchet to ≥ 80%) on core crates |
| RG-09 | Feature closure | All `core` and `std` features ≥ L4; `core` ≥ L5 |
| RG-10 | Gap closure | 0 open class-b gaps; every class-a gap has diagnostic + docs; witness debt = 0 |
| RG-11 | Compile-time budgets | All benchmarks within budget (instruction count) |
| RG-12 | Scaling | Fitted complexity exponent ≤ 1.2 on every scaling generator |
| RG-13 | Memory | Peak RSS within budget on corpus |
| RG-14 | Incremental latency | Body-only edit: check ≤ *cal.* ms, debug build-to-exe ≤ *cal.* s on 50k-LOC crate |
| RG-15 | Generated-code performance | Geo-mean runtime within *cal.* of reference implementations on the runtime benchmark set |
| RG-16 | IDE latency | LSP p95: diagnostics ≤ 100 ms (single-body edit), completion ≤ 50 ms |
| RG-17 | Reproducible release | Bit-identical artifacts from two independent builds |
| RG-18 | Supply chain | `cargo deny` + `cargo vet`/audit clean; SBOM produced; signed artifacts |
| RG-19 | Platform matrix | Core features ≥ L4 on Tier-1 targets (linux-x86_64, linux-aarch64, macos-aarch64); Tier-2 (windows-x86_64) ≥ L3 |
| RG-20 | Spec coverage | ≥ 95% of GAM and language-reference rules have a positive and a negative test |
| RG-21 | Stdlib | 100% public items documented and tested; stdlib benchmark suite has budgets |
| RG-22 | Tooling | `glyip`, LSP, formatter features at their `target_level` |
| RG-23 | Stability | Edition policy written; public language surface tagged stable/unstable |
| RG-24 | Continuous fuzzing | Fuzz farm running on `main`; findings triaged < 7 days |

### 3.11 Anti-gaming (Goodhart) defences

- Fixtures **must assert** (exit code or stdout); expectation-free fixtures give no evidence.
- Mutation testing runs *on the tests that supply evidence* — a fixture suite that does not kill mutants does not raise a level (RG-08).
- `ledger diff` shows level *changes with the commits that caused them*; a level rise without new evidence is a bug in the ledger and is asserted impossible by property tests (monotonicity: adding passing evidence never lowers a level; removing evidence never raises one).
- Gates cannot be edited in the same PR that fails them (CODEOWNERS on `ledger/policy.toml`, `ledger/gates.toml`).

### 3.12 Retiring prose status

`KNOWN_GAPS.md` becomes a *generated* file (header: "DO NOT EDIT — generated by `ledger compute`"); `HANDOFF.md`, `FIXES.md`, `docs/status/*` are archived to `docs/archive/`; README's "Project Status" section is replaced by marker-delimited generated content; the `glyim-docs` Astro site renders `STATUS.json` as a live readiness page.

---

## 4. Workstream B — Performance (SOTA techniques, measured)

### 4.1 Guiding stance

1. **Measure before touching anything** (M2). The baseline decides which of the items below are worth doing and in what order; items are *hypotheses with gates*, not commitments.
2. **Phase-structured parallelism first, queries where they pay.** Query-based compilers give incremental reuse but have real overhead and demand language/IR design discipline; the 2026 critique of "query-everything" designs (matklad) and the Zig experience both point at the same recipe: parse and index files independently and in parallel, keep early phases order-independent and cheap, and apply fine-grained memoization at the boundaries where recomputation is expensive (signatures → bodies → codegen units). §4.3 adopts that hybrid.
3. **Instruction counts, not wall-clock, gate CI** (as rustc-perf does): deterministic, noise-free, comparable across runners. Wall-clock/RSS are reported, not gated, except in the nightly dedicated-runner job.
4. **Compile-time and generated-code speed are separate tracks** with separate budgets; every optimizer pass pays rent in both.

### 4.2 Target architecture

```
files ──(parallel, per-file)──► lex+parse ► flat syntax arena (+ lazy lossless tree for IDE/fmt)
         content-hash cutoff        │
                                    ▼
                  item tree (signatures only, fingerprinted) + macro expansion (cached by input hash)
                                    │
                    crate-level def collection + import fixpoint (deterministic)
                                    │
  ┌──────────────────── query layer: red-green, early cutoff, durability, cancellation ─────────────────┐
  │ signatures: type_of · fn_sig · generics · impl_index (SimplifiedType-keyed) · coherence              │
  │ per body, PARALLEL: typeck → thir → mir_build → borrowck → mir_opt (pass manager, verified)           │
  │ mono: collect (parallel worklist, interned Instance) → partition (stable CGUs)                        │
  │ per CGU, PARALLEL: LLVM/Cranelift codegen (thread-local contexts) → content-addressed object cache   │
  └──────────────────────────────────────────────────────────────────────────────────────────────────────┘
                                    │
          link (mold > wild > lld > cc)  ◄── precompiled stdlib: .gmeta (mmap, zero-copy) + objects
```

### 4.3 Measurement infrastructure (M2) — requirements

| Capability | Design |
|---|---|
| Phase timing | `--time-passes`; `tracing` spans at phase and (later) query granularity; `--self-profile` writes Chrome-trace JSON; flamegraph script |
| Stats | `--emit=stats`: tokens, syntax nodes, interned types/substs, obligations processed/cached, MIR blocks/statements, LLVM instructions, CGU sizes |
| Deterministic cost | `tools/glyim-bench` runs benchmarks under `valgrind --tool=callgrind`/`iai-callgrind` (or `perf stat -e instructions:u` where permitted) → instruction count, plus cycles, wall, max-RSS, allocation count/bytes (counting allocator, `GLYIM_ALLOC_STATS=1`) |
| Corpora | (a) **synthetic generators** parameterised by N: wide (many fns), deep (nesting), generic-heavy, trait-heavy, match-heavy, macro-heavy, closure-heavy; (b) **real**: stdlib self-compile, all fixtures, 20 curated programs; (c) **runtime benchmarks** (≥12: n-body, fannkuch, spectral-norm, binary-trees, mandelbrot, hashing, sorting, string building, JSON-like parse, matrix multiply, allocator stress, trait-object dispatch) with reference outputs |
| Complexity guard | Run generators at N, 2N, 4N, 8N; fit log-log slope of instruction counts per phase; **fail if exponent > budget** (catches accidental O(n²) such as the solver's per-obligation impl scan and overlap checks) |
| History & gating | `perf/history/*.jsonl` appended on `main`; PR job runs `perf-compare base head` (instructions per benchmark/phase); >1% regression on any benchmark = warn, >2% on a benchmark or >0.5% geo-mean = fail; improvements ratchet the budget down |
| Budgets file | `perf/budgets.toml` (per benchmark × phase), registered as ledger gate RG-11/12/13 |

### 4.4 Optimization catalogue

Legend — **Evidence**: why we believe it's worthwhile (repo fact or literature). **Gate**: what must be demonstrated to merge. All gains are *hypotheses until M2 numbers exist*.

#### Frontend

| ID | Technique | Evidence | Gate |
|---|---|---|---|
| **PF-1** | **Flat syntax arena.** Tokens as SoA `(kind:u8, start:u32, len:u32)` with text sliced from the source on demand (no per-token `SmolStr`); perfect-hash keyword lookup; table-driven lexer; compile path uses an index-based syntax arena; Rowan lossless tree is built *lazily* only for LSP/formatter | F7; Zig/Carbon-style data-oriented frontends | lex+parse throughput ≥ 3× baseline on 1M-LOC synthetic (rebaselined by ADR); allocation count ≥ 5× lower |
| **PF-2** | **Parallel per-file parse + expand**, results merged by `FileId` order | Zig parses files independently; rustc parallel frontend (reported ~30% aggregate on nightly, LWN 2024) | scaling ≥ 0.6×cores to 8 cores; output byte-identical |
| **PF-3** | **Interner & hash discipline.** Pre-interned keyword/lang-item constants (`Name::KW_FN`); sharded or per-thread caches; FxHash/foldhash for hot maps; `FxIndexMap` where iteration order matters; ban SipHash `HashMap` in compiler crates | F7; `set_mono_cache` clones every string into a `HashSet` | instruction-count win on name-resolution + typeck benchmarks |
| **PF-4** | **Item tree + early cutoff.** A per-file, signature-only summary with a fingerprint; body-only edits leave the fingerprint unchanged so no signature query reruns | rust-analyzer design; Salsa "backdating" | body edit invalidates only that body's queries (asserted by `--explain-rebuild`) |
| **PF-5** | **Macro-expansion cache** keyed by `(macro-def hash, input-token hash)`; token trees as arena slices (no re-lex); proc macros run out-of-process (CT-10) so they can be declared pure and cached | F10 (`LocalDefId` instability from expansion order); sandbox also enables caching | macro-heavy benchmark ≥ 5× faster on re-expansion |
| **PF-6** | **Name resolution as an indexed fixpoint** (rustc-style import resolution with change tracking; dense `IndexVec<ModuleId,_>`; no per-lookup `String`) | `glyim-def-map` 3.6k LOC, single-threaded | scaling exponent ≤ 1.1 on deep-module generator |
| **PF-11** | **Match compilation** via Maranget usefulness (exhaustiveness) + decision trees for lowering; avoid linear test chains | Maranget 2007/2008 (see refs). *Verify what exists first.* | match-heavy benchmark: fewer MIR blocks, same behaviour (differential) |

#### Type system

| ID | Technique | Evidence | Gate |
|---|---|---|---|
| **PF-7** | **Concurrent type context.** `TyCtx<'tcx>` with sharded interner + append-only chunked arenas; `Ty(u32)` handles retained; delete the freeze model, the raw-pointer arena and `unsafe impl Send/Sync` | F8; rustc's sharded interners | loom tests; ≥0.6×cores typeck scaling; zero `unsafe` in `glyim-type` outside a tiny audited append-only core (or a vetted crate) |
| **PF-8** | **Trait solver redesign.** Impl index keyed by `(TraitDefId, SimplifiedType)` + blanket-impl list (kills the linear scan and O(n²) overlap); **canonical queries** (variables renumbered) with a global result cache + per-inference-context cache; coinductive cycle handling with provisional cache | F8; rustc next-solver direction; chalk lessons | trait-heavy scaling exponent ≤ 1.1; solver cache hit-rate reported in `--emit=stats` |
| **PF-9** | **Inference engine.** Union-find with path compression + rank and an undo-log for snapshots/rollback; use existing `TypeFlags` for `HAS_INFER` fast paths in zonk/resolve; no `RefCell<Vec<Diagnostic>>` on the hot path | `glyim-solve/src/infer.rs` | typeck-heavy benchmark instruction win |
| **PF-10** | **Body-parallel typeck**: `typeck(body)` is a pure query returning `Arc<TypeckResults>` (dense `IndexVec` keyed by body-local ids) | F2/F4 | determinism matrix green |

#### Middle end

| ID | Technique | Evidence | Gate |
|---|---|---|---|
| **PF-12** | **MIR representation diet.** Inline `[Operand;2]` for binary ops (no `Box`), `SmallVec`/arena slices for aggregates and call args, interned projection lists for `Place`, compact `Operand` (≤16 B); passes mutate in place (no whole-`Body` clone) | F6, F9 | allocation count in MIR build/opt ≥ 4× lower |
| **PF-13** | **Borrowck.** Per-body parallel; region constraints condensed by SCC; sparse/chunked bitsets + RPO worklist for liveness/moves; **two-tier**: cheap location-insensitive analysis first, run the precise location-sensitive analysis only where the cheap one reports a potential error (the Polonius-alpha lineage: loan-reachability over a point-indexed graph, landed on rustc nightly in 2025) | `glyim-borrowck` 10k LOC; Polonius alpha | borrowck share of compile time ≤ *cal.*; accepts the NLL "problem case #3" family |
| **PF-14** | **Pass manager**: declared `REQUIRES/ENSURES` phases, analysis cache with invalidation, per-pass timing + instruction-delta stats, `--opt-bisect-limit=N`, verify-after-each-pass mode | F6 | see CT-4, CO-3 |
| **PF-15** | **Mono collection** with interned `Instance` (not `String` symbols), parallel demand-driven worklist, dedupe by interned id | `PipelineMonoCache{symbols: Vec<String>}` | mono-heavy benchmark win; no `String` in the mono API |
| **PF-24** | **Query engine + persistent object cache** (ADR-002) | F2 | incremental-vs-clean oracle green (CO-6) |

#### Optimizer (generated-code speed)

| ID | Technique | Evidence | Gate |
|---|---|---|---|
| **PF-30** | **Classic MIR suite**: copy-prop, GVN over pure ops, SROA, dest-prop, jump-threading, match-on-known-discriminant, dead-store elimination, redundant-drop removal, bounds-check elimination from range facts, conservative LICM | F6 | runtime geo-mean ≥ *cal.* better; TV clean |
| **PF-31** | **MIR inliner** (cost model, `#[inline]` hints, call-graph SCC cycle guard, cross-crate via metadata, debug-info preserving) | largest single lever for Rust-like languages | runtime benchmark win; compile-time budget respected |
| **PF-32** | **Acyclic e-graph mid-end (experimental, ADR-009).** Cranelift's *ægraph* design: pure operators live in an e-graph without location; rewrites applied once at construction (acyclic ⇒ linear passes), GVN/LICM/rematerialization fall out of the translation to/from the graph; rewrite rules in a declarative DSL; bounded by rule budget, class-size limit, extraction cost model. Alternatives: `egg`-style full equality saturation (powerful, but blow-up on associativity/commutativity), egglog (Datalog + EqSat), "destructive rewrites" research to limit growth | Fallin, EGRAPHS 2023 & 2026-04 write-up (production use, solves pass-ordering) | must beat PF-30 pipeline by ≥ *cal.*% runtime **or** ≥ *cal.*% fewer instructions at ≤ *cal.*% compile-time cost; else stays experimental |

#### Backend & link

| ID | Technique | Evidence | Gate |
|---|---|---|---|
| **PF-17** | **Stable CGU partitioning** (module-path-hash naming, size-estimate merging, generics placement) + **per-CGU parallel codegen** with thread-local LLVM `Context`s + content-addressed per-CGU object cache (key: CGU content hash + flags + LLVM version) | F4: today one module *per body* | codegen scaling ≥ 0.6×cores; edit one fn ⇒ ≤1 CGU recompiled |
| **PF-18** | **LLVM IR quality**: SSA values for non-address-taken scalar locals instead of alloca+mem2reg; `noundef/nonnull/align/dereferenceable/noalias` (only where the GAM validates the assumption), `nuw/nsw` per language semantics, `!range`, `cold` + `llvm.expect` on panic paths, `unnamed_addr`, internal linkage, function sections | LLVM's alloca-heavy IR makes SROA/mem2reg a top cost | LLVM time at -O0/-O2 ≥ *cal.*% lower; runtime unchanged or better; each attribute differential-tested on/off |
| **PF-19** | **LLVM pipeline control**: new pass manager with curated per-level pipelines; FastISel at -O0; in-process ThinLTO via LLVM's LTO C API (drop the `llvm-lto2` subprocess) | `glyim-cli` linker driver | ThinLTO e2e speedup; no subprocess |
| **PF-20** | **Linker selection**: auto-pick `mold` > `wild` > `lld` > `cc`; `--gc-sections`; split debuginfo; emit PIC so the `-no-pie` workaround disappears. Vendor-reported: mold ≈ 4.9× faster than lld at the median on large links (Aug 2026), wild is the Rust incremental-link candidate | linker is the wall at large scale | link-time share ≤ *cal.* on 100k-LOC program |
| **PF-21** | **Fast debug tier (ADR-010, gated spike).** Cranelift backend from MIR. *Caution:* rustc's Cranelift backend measured only ~5–12% end-to-end incremental gains over LLVM on a large project (Feb 2026) because the frontend dominates; our spike must show **≥2× backend-time reduction vs LLVM-O0 and ≥25% end-to-end** on typical programs or it is dropped. Stretch: copy-and-patch baseline; Zig-style in-place binary patching for incremental debug links (Zig reports sub-second incremental rebuilds of itself) | rust-lang 2026 fast-builds goals; Zig 0.15+ self-hosted backend | go/no-go rubric in ADR-010 |
| **PF-22** | **Output-code quality**: niche-filling enum layout (`Option<&T>` = pointer), field reordering, scalar-pair ABI, discriminant elision, devirtualisation of known `dyn`, `memcpy` avoidance (dest-prop), small-vec/small-string optimisations in stdlib | layout crate 2.7k LOC | verified vs LLVM `DataLayout` + GAM; runtime benchmarks |
| **PF-23** | **Runtime/stdlib algorithms**: SwissTable hash map with fast hashing, modern unstable sort (pdq/ipn/driftsort family), SIMD `memchr`/search, buffered I/O, cold panic paths, pluggable allocator | stdlib is 7.3k LOC of `.g` | stdlib benchmark budgets |

#### Distribution & tooling

| ID | Technique | Evidence | Gate |
|---|---|---|---|
| **PF-25** | **Separate compilation + zero-copy metadata + precompiled stdlib + shared generics + pipelined compilation** (downstream starts once `.gmeta` is out) | F3: stdlib re-compiled per run | hello-world cold compile ≥ 10× faster than today's `--with-stdlib` path (rebaselined) |
| **PF-26** | **`glyip` v2**: DAG scheduler with jobserver, content-addressed artifact cache (local + HTTP/S3 remote), hermetic keys, `--timings`, PubGrub-style backtracking resolver with explainable conflicts, links executables | README (greedy resolver, object-only builds) | workspace of 20 crates builds with correct parallelism; remote-cache hit ⇒ no compile |
| **PF-27** | **Self-optimisation of the compiler**: PGO (+BOLT/relayout) release builds trained on the bench corpus; jemalloc/mimalloc as global allocator; LTO fat for the shipped binary | rustc ships PGO'd builds | ≥ *cal.*% instruction reduction on benchmarks |
| **PF-28** | **IDE responsiveness**: LSP on the query DB with cancellation; syntax-only features answer from the lossless tree in <10 ms; incremental reparse of the smallest enclosing block; semantic results stream after | `glyim-lsp` has bespoke `symbol_index`/`dep_graph`/`reference_graph` caches | RG-16 |
| **PF-29** | **Interpreter speed** (after GAM, M9): pre-lower MIR to a compact internal instruction stream, cache layouts, avoid allocation in the hot loop; it doubles as the const-evaluator | `glyim-mir-interp` 15.6k LOC value-model | const-eval benchmark ≥ *cal.*× faster |

### 4.5 Budget targets (ratios against the M2 baseline; frozen by ADR after M2)

| Metric | Target (cumulative by milestone) |
|---|---|
| Lex+parse throughput | ≥ 3× (M5) |
| Cold check of 50k-LOC crate (excluding stdlib) | ≥ 3× faster with parallelism on 8 cores (M7) |
| Hello-world cold compile via precompiled stdlib | ≥ 10× faster than `--with-stdlib` (M8) |
| Body-only edit → diagnostics | ≤ 100 ms p95 on 50k-LOC crate (M6) |
| Body-only edit → debug executable | ≤ 1 s (M8, links via mold/wild) |
| Peak RSS | ≥ 2× lower than baseline on the stdlib self-compile (M7; arena fix already removed a ~44 MB/compile leak per FIXES §1) |
| Generated-code speed (runtime benchmark geo-mean at `-O2`) | ≥ *cal.*% better than M2 baseline from MIR opts alone (M10) |

---

## 5. Workstream C — Bugs hard to produce by construction

### 5.1 Defence in depth (apply strongest layer first, C3)

```
L1 TYPES        illegal states cannot be written      (CT-1…CT-6)
L2 CONSTRUCTORS invalid values cannot be built         (CT-1, CT-4)
L3 VALIDATORS   invalid IR is rejected at phase edges  (CT-4, CO-2)
L4 EXHAUSTION   forgotten cases fail to compile        (CT-7)
L5 ORACLES      wrong behaviour is detected            (CO-1…CO-4)
L6 GENERATION   inputs nobody imagined are tried       (CO-5…CO-7)
L7 PROCESS      each bug buys a class-level defence    (CP-1)
```

The in-repo audit's 193 findings map onto these layers in Appendix A; every one of its bug classes has at least one structural prevention below.

### 5.2 Type-level and construction-level (L1–L4)

#### CT-1 Private-constructor IDs and arena-issued handles *(breaking)*
- `from_raw`/`to_raw` become `pub(crate)` in `glyim-core`; IDs are produced only by `IndexVec::push -> I`, interners, or a `DefIdAllocator`. Outside the arena modules, `clippy.toml` `disallowed-methods` rejects `from_raw` (643 call sites migrate in M3.1 via codemod).
- Debug builds embed an 8-bit arena tag in IDs so *cross-arena misuse* panics at the use site; release builds keep `u32`/`NonZeroU32`.
- **IDs are position-independent** (M6.1): `HirId = (owner DefId, local index)` and `DefId` has a stable `DefPathHash`, so editing one body can never renumber another item (F10, KNOWN_GAPS §5).

#### CT-2 `ErrorGuaranteed` and typed pipeline results *(breaking)*
```rust
pub struct ErrorGuaranteed(());                 // unforgeable: only DiagCtxt can mint it
impl DiagCtxt { pub fn emit_err(&self, d: Diagnostic) -> ErrorGuaranteed { … } }
pub enum TyKind<'tcx> { …, Error(ErrorGuaranteed) }
pub fn typeck(body: BodyId) -> Result<TypedBody, ErrorGuaranteed>;   // lowering accepts only TypedBody
pub fn compile(..) -> Result<Artifact, ErrorGuaranteed>;
```
Error-recovery nodes exist *only before zonk*; `TypedBody` contains none. "Error type reached codegen without an emitted error" becomes an ICE with context, never a zero-valued global (kills F10's sentinel/`ConstRef` zero-materialisation class).

#### CT-3 Checked primitives
- `Span { file, lo, len: u32 }` — `lo > hi` is unrepresentable; offsets > `u32::MAX` produce a diagnostic, not wraparound (FIXES §4 class).
- `Size`, `Align`, `Offset` newtypes with checked arithmetic; `#![deny(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::as_conversions)]` in `glyim-layout`, `glyim-const-eval`, `glyim-mir-interp`, `glyim-span`, `glyim-codegen-llvm` (kills the `u128 as usize` aliasing class, P-04).

#### CT-4 Typestate IR pipeline *(breaking)*
- Stages are distinct types: `Parsed → Expanded → Resolved → Hir → TypedBody → Mir<Built> → Mir<Checked> → Mir<Optimized> → CodegenItem`. No struct with `Option` fields that later phases "fill in" (the shape of today's 614-line driver).
- `MonoTy`/`Instance`: codegen accepts only fully monomorphic, normalized, region-erased types, so `Projection`/`Param`/`Infer` **cannot** reach LLVM (HANDOFF's `TyKind::Projection reached LLVM codegen` ICE becomes a compile error).
- `Symbol` is a newtype minted by exactly one mangler; property test: mangling is injective and round-trips. Backends take `Vec<CodegenItem>`, not `Vec<(String, Arc<Body>)>`.
- MIR: private fields; `BodyBuilder::finish()` fails if any block lacks a terminator; `Place::project(tcx, elem) -> Result<Place, ProjectionError>` computes and stores the projected type; `Operand::constant(ty, kind)` checks agreement; `Rvalue::aggregate(kind, ops)` checks arity; `SwitchTargets::new` enforces unique values + otherwise; `StatementKind::Nop` is removed; phase-indexed invariants (`Mir<Checked>` has no `Drop` of maybe-init places; `Mir<Optimized>` has no `Subslice`/`ConstantIndex` — today enforced by `panic!`).

#### CT-5 Error-code registry
`errors/*.toml` (code, category, summary, long explanation, example fixtures) generates the `ErrorCode` enum, `glyim --explain E0123`, the docs page, and the ledger's per-feature `errors` cross-check. Ad-hoc codes are unrepresentable; a code without a negative fixture fails `ledger check`.

#### CT-6 Configuration makes illegal combinations unrepresentable *(breaking CLI)*
```rust
enum Backend { Llvm(LlvmOpts), Cranelift(ClifOpts) }   // LTO, target-cpu live inside LlvmOpts only
enum EmitKind { Obj, Exec, Mir, MirText, LlvmIr, Asm, Cdylib }
fn validate(backend:&Backend, emit:EmitKind) -> Result<ValidConfig, ConfigError>
```
"`--lto thin` with the bytecode backend" style errors cease to be a runtime check.

#### CT-7 Exhaustiveness and schema-generated IR code *(breaking)*
- `#![deny(clippy::wildcard_enum_match_arm)]` in every crate matching on IR enums (MIR, THIR, `TyKind`, `Rvalue`, …). Adding a variant fails compilation in every backend/pass/interpreter until handled — backend parity by construction.
- `glyim-irgen` generates from `ir/mir.schema` (and THIR): enum definitions, smart constructors, visitors/folders (immutable & mutable), the textual printer/parser (CO-2), serde for metadata, and validator scaffolding. Hand-written traversals — the origin of "visitor forgot a variant/local" bugs — disappear.

#### CT-8 Panic and unsafe policy
- Workspace `[lints]`: deny `unwrap_used, expect_used, panic, unreachable, todo, unimplemented, indexing_slicing` (IR crates index via typed `IndexVec`; the one `get_unchecked`-free implementation lives in `glyim-core`). Remaining sites need `// INVARIANT: <why>` and are listed in `ledger/panic-allowlist.toml`, ratcheted to zero outside genuine invariants.
- `ice!(phase, "msg", spans…)` is the only way to abort: it captures phase, query stack, spans, flags, and writes a **repro bundle** (source + flags + hashes) consumed by `glyim-reduce` (CO-7). No MIR-validation `panic!`: validators return `Result<(), Vec<Violation>>`.
- `unsafe` is `forbid`den workspace-wide with an allow-list (`ledger/unsafe-allowlist.toml`: crate+module). Allow-listed modules require `// SAFETY:` comments (`clippy::undocumented_unsafe_blocks`), `unsafe_op_in_unsafe_fn = deny`, run under Miri (and loom/TSAN for concurrency), and their count is ratcheted. Target: `glyim-runtime` FFI + a tiny LLVM-FFI shim only; the type-arena raw pointers disappear (PF-7).

#### CT-9 Determinism by construction
- `clippy.toml disallowed-types = ["std::collections::HashMap", "std::collections::HashSet"]` and `disallowed-methods = ["std::time::Instant::now", "std::time::SystemTime::now", …]` in compiler crates; use `FxIndexMap/Set` (iteration = insertion order) and a single timing module.
- Parallel stages merge results through ordered helpers (`par_map_collect_ordered`); diagnostics carry a stable sort key (file, span, code) and are emitted sorted.
- **Chaos mode** (`GLYIM_CHAOS=<seed>`): randomises residual hash seeds, rayon thread counts, injected yield points, and path prefixes; CI compiles the corpus under many seeds and requires byte-identical objects and diagnostics (RG-04).

#### CT-10 Proc-macro isolation *(breaking ABI)*
Replace `dlopen` of arbitrary cdylibs into the compiler process with an **out-of-process** macro host (RPC over pipes; crash/OOM of a macro cannot take down the compiler; resource limits via rlimit/cgroup; allowed filesystem/env by policy). Macros that declare themselves pure are cached (PF-5). Stretch: WASM sandbox (determinism for free) once Glyim targets `wasm32` (ADR-007).

### 5.3 Oracles and generation (L5–L6)

#### CO-1 One reference semantics: the Glyim Abstract Machine (GAM) + Miri-style interpreter
Write the GAM spec (`docs/spec/gam.md`): byte-addressed memory with allocation identity and provenance, init-ness tracking, alignment, integer semantics (ADR-006), the *explicit list of UB*, evaluation order, panic = abort (ADR-005), FFI boundary. Each rule has an ID (`GAM-MEM-014`) registered in the ledger and linked to tests (RG-20). Rewrite `glyim-mir-interp` around it (M9): typed loads/stores via layout, pointers = (alloc, offset, provenance), UB detection (OOB, use-after-free, uninit reads, misalignment, invalid `bool`/enum/ref validity). An optional aliasing model (Tree-Borrows-style) validates that borrowck results and emitted `noalias` agree. **Const-eval is the interpreter** (delete `glyim-const-eval`'s separate evaluator, 4.4k LOC) so there is exactly one implementation of arithmetic and casts.

#### CO-2 Textual MIR and pass-level testing
`.gmir` round-trippable text format (generated printer/parser from the IR schema). `glyim-mir-opt` runs chosen passes on a `.gmir` file; data-driven tests with `//~ CHECK:` lines (LLVM `opt`+FileCheck style). Property: `parse(print(m)) == m` for 100% of fixtures and for random valid MIR.

#### CO-3 Translation validation of MIR optimizations (`glyim-tv`)
Alive2-style *bounded* validator: given `before.gmir`/`after.gmir`, encode both with GAM semantics (bit-vectors; memory as byte arrays with allocation model; calls as uninterpreted functions with purity attributes; loops unrolled to a bound) and ask an SMT solver whether `after` refines `before` (poison/UB-aware). Runs under `GLYIM_TV=1` for every pass on every fixture and on generated bodies; SMT timeouts are tracked as an "unknown rate" metric (RG-03), not silent passes. Alive2 reports dozens of real LLVM miscompilations found this way; the same technique applies to our (much newer) optimizer.

#### CO-4 Verified rewrite rules
Every peephole/e-graph rule lives in a DSL (`rules/*.gopt`) and is **proven by SMT at `cargo test`** over widths {1,2,3,4,8,16,32,64} with GAM poison/UB semantics (Alive and Crocus are the precedents). A rule that cannot be proven cannot merge; a failing proof emits a counterexample as a witness fixture.

#### CO-5 `glyim-smith`: typed program generator (Csmith/YARPGen lineage)
Deterministic-by-seed generator of **well-typed, UB-free** programs (typed expression generation; explicit overflow semantics; bounded loops; no uninit reads), ending in a checksum print. Grows by feature: scalars/control-flow → structs/enums/match → generics → traits → closures → references with an ownership discipline → async. Each ledger feature gets a generator profile (RG-01 CPU-hours are accounted per profile). Failures auto-reduce (CO-7) and become witness fixtures.

#### CO-6 Metamorphic and invariance relations
| Relation | Transformation | Oracle |
|---|---|---|
| Opt-level invariance | `-O0` vs `-O2` vs `-O3` | same stdout/exit |
| CGU-count invariance | `--codegen-units 1` vs 16 | same behaviour |
| Thread invariance | 1 vs N threads | **byte-identical** output |
| Equivalence modulo inputs (EMI) | inject code under provably-dead conditions; delete provably-dead code | same behaviour |
| Hygiene/renaming | consistent alpha-renaming of locals/items | same behaviour |
| Item reordering / module splitting | permute items, split into modules | same behaviour |
| Desugaring equivalence | `for`/`?`/`while let`/closures vs their hand-desugared forms | same behaviour |
| **Incremental ≡ clean** | random edit sequences; after each edit incremental result vs clean build | identical diagnostics + identical object bytes (RG-05) |

#### CO-7 Structured fuzzing at every phase boundary + automatic reduction
Beyond today's two targets: grammar-based lexer/parser fuzzing (`arbitrary` over the grammar), type-preserving and type-breaking mutations of valid programs, random *valid* MIR through the builder (feeding passes, validator, LLVM lowering, interpreter, TV), metadata/proc-macro-protocol fuzzing (decoders must never crash on corrupt input). Continuous fuzz farm on `main` (RG-24). `glyim-reduce`: grammar-aware delta debugging on the CST (Perses-style) with a token-level fallback; CI attaches the minimal repro and files the gap + witness automatically.

#### CO-8 Mutation testing as a gate
`cargo-mutants` on the correctness-critical crates with per-crate score thresholds ratcheting up (RG-08); surviving mutants produce a worklist the agent clears by writing assertions, not by excluding mutants.

#### CO-9 ABI and layout cross-checks
(a) Compare `glyim-layout` against LLVM `DataLayout` for generated types; (b) *abi-cafe-style* cross-language tests: generate C structs/functions, compile with clang, call both directions, compare returned values and layouts across `repr`s and target triples.

#### CO-10 Slow-but-obviously-correct reference implementations (differential, test-only)
- Borrow checking: the formal rules as a Datalog program (`ascent`/`datafrog`) compared to the production borrowck on generated programs (accept/reject agreement).
- Trait solving: a naive logic-program solver vs the optimized solver on generated trait/impl sets.
- Unification: substitution-based naive unifier vs union-find.
- Layout: brute-force vs production.
- Const-eval vs interpreter vs LLVM constant folding for every operator.

### 5.4 Process level (L7)

- **CP-1 Class-level defence per bug.** Every bug-fix PR carries a *bug-class record* (`ledger/bugclasses/*.toml`: class name, root cause, defence type ∈ {type, constructor, validator, lint, oracle, generator, metamorphic}, link to the defence). A fixed bug without a defence is **prevention debt**, reported by `ledger bugclass` and prioritised in `ledger next`.
- **CP-2 Semantics ADRs** (ADR-005/006/013): decide once, write into GAM, enforce by tests — no more opt-level-dependent behaviour or implementation-defined evaluation order.
- **CP-3 Review checklist** (§7) enforces C3's order: "could a type, constructor, or exhaustive match have prevented this?" comes before "add a test".

### 5.5 Proposed language-semantics simplifications (each is an ADR; each shrinks the bug surface)

| Decision | Proposed default | Why |
|---|---|---|
| Panic model (ADR-005) | **abort only** | KNOWN_GAPS §2/§4: `glyim_panic` already exits 101; SEH cleanup edges are dead code; the interpreter's unwind path is off and breaks 14 tests. Deleting unwinding removes a whole semantic layer (landing pads, cleanup edges, drop-on-unwind). Alternative: implement unwinding completely — large, deferred. |
| Integer overflow (ADR-006) | **checked arithmetic in all profiles by default** (`+ - * << >>` panic on overflow); `wrapping_*`/`checked_*`/`saturating_*`/`unchecked_*` explicit; a crate-level `overflow = "wrap"` setting selects *defined* wrapping — never "UB in release" | Removes opt-level-dependent semantics ⇒ differential oracles across `-O` levels are meaningful. Cost is recovered by range analysis/BCE (PF-30). |
| Evaluation & drop order (ADR-013) | Fully specified left-to-right; temporaries drop at end of enclosing statement | Required for GAM; removes HIR-29-style double-evaluation class. |
| Proc-macro model (ADR-007) | Out-of-process RPC | CT-10. |
| `include!`, `concat_idents!` | Keep `include!` with path policy; drop or restrict `concat_idents!` | hygiene hazard; KNOWN_GAPS §5 interplay with `LocalDefId`. |

---

## 6. Milestones

### 6.0 Timeline and lanes

Parallel **lanes** keep agents out of each other's crates (`ledger/owners.toml` maps crates → lane): **L** ledger/infra/CI · **F** frontend (span, syntax, frontend, def-map, meta, hir) · **T** types (type, solve, typeck) · **M** middle (mir, lower, borrowck, opt, interp, const-eval) · **B** backend (layout, codegen-llvm, runtime, cli, glyip) · **Q** quality (test, fuzz, generators, TV).

```
Week:  1   5   10   15   20   25   30   35   40   45   50   55   60
M0   ███                                                        Baseline & rails
M1     ██████                                                   Readiness Ledger v1
M2      ██████                                                  Perf measurement + quick wins
M3          ████████                                            Core type-safety refactor I
M4            ████████                                          Safety net: generators & oracles v1
M5                  ██████████                                  Typed IR pipeline
M6                       ██████████                             Query engine & incremental
M7                            ██████████                        Parallel compiler
M8                                ██████████                    Separate compilation & ecosystem
M9                          ████████████████                    GAM interpreter & semantic spec
M10                                     ████████████           Verified optimizer
M11                                         ████████████       Backends, layout, output speed
M12                                                 ██████████  Hardening & 1.0 RC
```

Milestone dependency summary: **M1,M2 → M3,M4 → M5 → M6 → M7 → M8**; **M5 → M9 → M10 → M11**; **M4 feeds everything** (it is the refactoring safety net); **M12** needs all.

Notation: **WI** = work item. *DoD* = definition of done, always machine-checkable. Every WI also satisfies the universal DoD in §7.4. Breaking changes are flagged ⚠.

---

### M0 — Baseline & Rails (weeks 1–3) · lanes L, all

**Goal:** a reproducible environment, a frozen green baseline, a metrics snapshot, and the process scaffolding the rest depends on.

| WI | Work | DoD |
|---|---|---|
| M0.1 | Reproducible toolchain: `rust-toolchain.toml`, devcontainer/Nix shell with **LLVM 23 pinned** (inkwell rev ↔ llvm-sys 231 ↔ system LLVM triple-lock documented), `just bootstrap`, `just ci-local` mirroring CI | clean clone in container → `just ci-local` green |
| M0.2 | Baseline snapshot: tag `baseline-2026-11`; full test run recorded; the 2 known failing tests (`with_stdlib_flag_is_accepted`, `assembled_stdlib_compiles` per `docs/status`) triaged (fixed or registered as witnessed gaps later); flake detection by running the suite 5× | `metrics/test-baseline.json` committed; flaky tests quarantined with expiry |
| M0.3 | Docs triage: move `HANDOFF.md`, `FIXES.md`, `docs/status/*`, stale roadmaps to `docs/archive/` (keep the audit + playbook as *seed data*); fix `docs/TECHSTACK.md` drift; README status section gets generated-content markers | no root-level hand-maintained status files remain |
| M0.4 | ADR framework `docs/adr/NNNN-title.md` + template; create ADR-001…015 as **proposed** stubs with the defaults from §8 | stubs merged; ADR index generated |
| M0.5 | `AGENTS.md` (the §7 protocol), PR template (ledger diff, perf delta, bug-class record, breaking-change checklist), `just verify` | PR template enforced by a CI check on PR body sections |
| M0.6 | `scripts/metrics.rs`: mechanically derive LOC per crate, `unwrap/expect/panic/unreachable` counts (non-test), `unsafe` counts, `from_raw` call sites, public fields on IR structs, `HashMap` use, wildcard arms on IR enums, `String`-keyed maps, crate dependency graph | `metrics/baseline.json` committed; each ratchet metric has a CI check "must not increase" |
| M0.7 | CI hardening: concurrency groups, dependency caching, timeouts, `permissions:`, required-check list, tiered workflows (`pr`, `nightly`, `weekly`), artifact retention | PR tier ≤ 20 min; nightly/weekly skeletons exist |
| M0.8 | Dead-weight removal: unused deps (`cargo-machete`/`udeps`; `regalloc2` is unused), dead profile blocks, stray files | `cargo machete` clean |

**Exit gate:** `just ci-local` green from a clean container; `metrics/baseline.json` exists; ADR stubs merged.

---

### M1 — Readiness Ledger v1 (weeks 3–8) · lane L (+Q)

**Goal:** workstream A delivered: status is computed; prose status is retired.

| WI | Work | DoD |
|---|---|---|
| M1.1 | `tools/glyim-ledger` crate (+ `ledger-schema` with `deny_unknown_fields`, JSON-Schema export); `ledger check` skeleton | schema round-trip tests; `ledger check` runs in CI |
| M1.2 | **Feature registry seed** (~400 entries): taxonomy in Appendix C → auto-generated skeletons from inventory extractors, then curated from README feature list, `docs/specs`, `docs/design/*` | every inventory item mapped or ignored-with-reason |
| M1.3 | Inventory extractors (`syn`-based for Rust enums; Glyim-parser-based for stdlib API; clap introspection `--help-json`; error/lint registries; runtime FFI scan) | adding a MIR variant or CLI flag without registration fails `ledger check` (tested) |
| M1.4 | Fixture annotation grammar (`//@ feature/mode/backends/expect-*`) + **codemod tagging all ≈290 fixtures** (+ no-ICE and typeck dirs) | 100% of fixtures tagged; fixtures lacking expectations rejected |
| M1.5 | `glyim-test` emits evidence JSONL (extends `GLYIM_TEST_JSON`): backend, opt, result, hashes, instructions, RSS | evidence schema validated; run is hermetic (no clock/PRNG in records) |
| M1.6 | **Strict-xfail witness mechanism** (`//@ known-gap: GAP-NNNN`) | unit tests: witness that stops reproducing fails with "close GAP-NNNN" |
| M1.7 | Gap registry migration from `KNOWN_GAPS.md` (§§1–7), `HANDOFF.md §2`, playbook Appendix A / X-01 into `ledger/gaps/*.toml` with witness fixtures (unwitnessable ones flagged as witness debt) | ≥ 90% of entries witnessed; rest are explicit debt |
| M1.8 | Level-computation engine + `policy.toml` (levels L0–L5, gap caps, CPU-hour thresholds); golden tests; property tests (monotonicity) | `ledger compute` pure; monotonicity proptests pass |
| M1.9 | Renderers: `STATUS.md`, `STATUS.json`, generated `KNOWN_GAPS.md`, README block, Astro page in `glyim-docs`, `ledger explain`, `ledger next`, `ledger handoff` | outputs deterministic (golden) |
| M1.10 | CI wiring: `ledger check` (required), `ledger diff` regression gate with `waivers.toml` (expiry ≤ 14 days), "STATUS is up to date" check, nightly full-evidence job | regressions block merge in a test PR |
| M1.11 | `glyim-pilot` integration: `LedgerGate`; `ledger next` feeds the orchestrator task queue | pilot run consumes queue end-to-end on a sample item |
| M1.12 | Readiness gates RG-01…RG-24 declared in `ledger/gates.toml`; `unmeasured` allowed but displayed | `ledger gates` prints all 24 |
| M1.13 | Ledger self-validation: run `compute` twice and on two machines → identical bytes; fuzz the TOML/JSONL parsers; mutation-test `policy` evaluation | reproducibility CI job green |

**Exit gate:** `STATUS.md` generated from evidence; deleting a fixture regresses a level and fails CI (demonstrated in a test PR); `KNOWN_GAPS.md` is generated.

---

### M2 — Performance measurement + quick wins (weeks 4–9) · lanes L, B

| WI | Work | DoD |
|---|---|---|
| M2.1 | `--time-passes`, `--self-profile` (Chrome trace), `--emit=stats`; `tracing` spans at phase granularity | trace opens in Perfetto; stats stable across runs |
| M2.2 | `tools/glyim-bench`: callgrind/iai instruction counts (fallback `perf stat`), cycles, wall, RSS, alloc count/bytes (counting allocator) | CoV of instruction counts < 0.1% over 10 runs |
| M2.3 | Corpora: 7 synthetic generators (wide, deep, generic, trait, match, macro, closure), real programs (stdlib self-compile, all fixtures, 20 programs), **≥12 runtime benchmark programs** with reference outputs | corpora committed with hashes |
| M2.4 | Scaling analyzer (log-log fit at N,2N,4N,8N) | report lists exponent per phase; flags exponent > 1.2 |
| M2.5 | Perf history store + `perf-compare` + CI PR job (warn >1%, fail >2% per benchmark or >0.5% geo-mean) | deliberate regression in a test PR is caught |
| M2.6 | **Baseline report** `perf/BASELINE.md`: per-phase time, top-20 hot spots, allocation hot spots, LLVM time share, link time share, stdlib-prepend cost; initial `perf/budgets.toml` = baseline | report reviewed; budgets enforced |
| M2.7 | **Quick-win campaign** (each gated by measured instruction-count win, no >0.5% regressions elsewhere): FxHash on hot maps; `set_mono_cache` without per-item string clones; `optimize` without whole-`Body` clone; `Box<(Operand,Operand)>` removal; compiler built with `lto="fat"`, `codegen-units=1`, jemalloc/mimalloc; linker auto-select (mold/lld) | ≥ 5 wins landed with numbers in PR descriptions |
| M2.8 | PGO pipeline prototype for `glyim-cli` (training = bench corpus) | reproducible PGO build script; measured delta recorded |

**Exit gate:** `just bench-ci` deterministic; budgets file enforced in CI; baseline report merged; ratios in §4.5 re-stated as absolute numbers via ADR.

---

### M3 — Core type-safety refactor I (weeks 8–15) · lanes F, T, M, B (crate-by-crate, leaf → root) ⚠ breaking

| WI | Work | DoD |
|---|---|---|
| M3.1 | `glyim-core`: **CT-1** private-constructor IDs, arena-issued handles, debug arena tags; codemod all 643 `from_raw` sites; `disallowed-methods` lint | `from_raw` outside arena modules = 0 (ledger metric) |
| M3.2 | `glyim-span`: **CT-3** `Span{file,lo,len}`, checked offsets, overflow diagnostics; `Size/Align/Offset` newtypes in `glyim-layout` | `Span` invariants proptested; old `debug_assert` class gone |
| M3.3 | `glyim-diag`: `DiagCtxt`, **`ErrorGuaranteed`**, stable diagnostic ordering/dedup, **CT-5** error-code registry + `--explain`, **`ice!`** + repro bundles + panic hook (fixes P-07: single ICE report, correct repo URL) | all existing codes migrated into `errors/*.toml`; ad-hoc `ErrorCode{..}` construction forbidden by lint |
| M3.4 | `glyim-type`: `Ty::Error(ErrorGuaranteed)`; safe append-only arena (replace `Box::into_raw`/`unsafe impl Send/Sync` with a vetted/own safe-API structure); `MonoTy`/`NormalizedTy` scaffolding | `unsafe` count in `glyim-type` = 0 or inside allow-list; loom/Miri green |
| M3.5 | **Panic ratchet burn-down** (CT-8): workspace lints at `warn` → per-crate `deny` as each crate reaches zero; `// INVARIANT:` allow-list | non-test `unwrap/expect/panic` ≤ 20% of baseline at exit (0 by M5) |
| M3.6 | **Unsafe policy**: `forbid(unsafe_code)` workspace default, allow-list, `unsafe_op_in_unsafe_fn`, `undocumented_unsafe_blocks`; Miri job covers allow-listed modules | `unsafe` outside allow-list = 0 |
| M3.7 | **CT-7a** `deny(wildcard_enum_match_arm)` on IR enums; remove all wildcard arms (each removal makes a missing case explicit) | wildcard arms on IR enums = 0 |
| M3.8 | **CT-6** configuration redesign (`Backend`, `EmitKind`, validated `ValidConfig`) in `glyim-cli`/`glyip`/pipeline ⚠ CLI flags change | illegal combos fail to compile; CLI snapshot tests |
| M3.9 | **CT-9** determinism lints (`disallowed-types/methods`), `FxIndexMap` migration in compiler crates, ordered-diagnostics sort key | `HashMap` in compiler crates = 0; `GLYIM_CHAOS` mode implemented (no randomisation sources yet beyond hash seeds) |
| M3.10 | Property tests for foundations: `IndexVec`, interner, spans, `Size/Align`, substitution, unification | ≥ 200 properties; run in PR tier |

**Exit gate:** ratchet metrics at their M3 thresholds; full suite + differential green; no new fixtures regress in ledger.

---

### M4 — Safety net: generators and oracles v1 (weeks 10–18) · lane Q (+F,T,M,B support)

**Goal:** before the large refactors (M5+), build the machinery that finds the bugs those refactors would introduce — and the bugs already there.

| WI | Work | DoD |
|---|---|---|
| M4.1 | **Audit re-verification campaign:** for each of the 193 findings in `glyim-bug-and-performance-audit.md` (and playbook S1/S2 items) write a witness fixture; if it passes → keep as regression fixture and mark closed-with-commit; if it fails → register a gap (class a/b) | 193/193 triaged; every closed item has a regression fixture; every open item has a witnessed gap |
| M4.2 | **Decide ADR-006** (overflow semantics) and ADR-013 (evaluation/drop order) first; encode in a draft GAM | ADRs accepted |
| M4.3 | `glyim-smith` v1 (scalars, control flow, structs/enums/match, functions, generics) with checksum printing; seeds recorded | ≥ 10k programs/hour/core; 0 invalid (non-compiling) programs in a 100k sample |
| M4.4 | Differential driver: interp vs LLVM-O0 vs LLVM-O2 (bytecode until M5.8); parallel; crash-signature bucketing; nightly job | 100k programs/night; every disagreement auto-filed |
| M4.5 | `glyim-reduce` (CST delta-debugging, token fallback) wired to the driver: minimal repro + witness fixture + gap stub per new bug | median reduced size < 40 lines on a 20-bug sample |
| M4.6 | Structured fuzzers (lexer, grammar-based parser, mutation-based typeck, random valid MIR → passes/validator/lowering); smoke tier 2 min in PR CI; continuous farm on `main` | no-ICE corpus ≥ 10× larger; 0 open ICEs or each witnessed |
| M4.7 | Metamorphic suite v1: opt-level, CGU-count, thread-count, dead-code injection (EMI-lite), renaming, item reordering | relations run nightly over fixtures + generated programs |
| M4.8 | **Per-operator exhaustive tests**: every `BinOp/UnOp/Cast` × all pairs of 8-bit values (exhaustive), sampled wider widths, across const-eval, interpreter, LLVM-O0/O2 | signedness/shift/overflow disagreements = 0 or witnessed |
| M4.9 | ABI/layout cross-checks (CO-9) vs LLVM `DataLayout` and clang-compiled C | layout disagreements = 0 or witnessed |
| M4.10 | Mutation baseline with `cargo-mutants` on frontend/solve/typeck/lower/borrowck/opt/layout; start thresholds from measured scores; surviving-mutant worklist | scores recorded in ledger (RG-08 measured) |
| M4.11 | Reference oracles prototypes: Datalog borrowck core, naive unifier, brute-force layout (CO-10) | differential tests run in nightly tier |

**Exit gate:** nightly 100k-program differential with 0 *unregistered* disagreements; RG-01/02/08 move from unmeasured to measured.

---

### M5 — Typed IR pipeline (weeks 15–24) · lanes M, T, F ⚠ breaking

**Goal:** make the compiler's data flow *typed*; dissolve the 614-line driver; make invalid IR unconstructible.

| WI | Work | DoD |
|---|---|---|
| M5.1 | **Typed pipeline stages** (CT-4): `Parsed→Expanded→Resolved→Hir→TypedBody→Mir<Built>→Mir<Checked>→Mir<Optimized>→CodegenItem`; delete `compile_file_with_artifacts`'s monolith; per-stage `tracing` spans; errors flow as `Result<_, ErrorGuaranteed>` | driver function ≤ 60 lines; no `Option`-filled-later structs (lint + review) |
| M5.2 | **Typed THIR**: `TypedBody` holds `ResolvedTy` (no inference vars, no error types); `zonk -> Result`; construction API private | lowering accepts only `TypedBody`; error-arm sentinel lowering deleted |
| M5.3 | **`MonoTy`, `Instance`, `Symbol`**: single mangler (injective + round-trip proptest); backends consume `Vec<CodegenItem>` | `String` symbols absent from mid/back-end APIs |
| M5.4 | **MIR hardening** (CT-4): private fields, `BodyBuilder`, typed `Place` with stored `PlaceTy`, checked smart constructors, remove `Nop`, `SwitchTargets` invariants, `Mir<Phase>` typestate | public-field count on IR structs = 0 (ledger metric) |
| M5.5 | **MIR validator v2**: CFG, types, def-before-use/dominance, storage markers, call/aggregate arity, cleanup edges, drop flags, phase-specific post-conditions; returns `Result<(), Vec<Violation>>`; `--verify-ir` default on in debug/CI | validator never panics (fuzzed); catches every historical MIR bug in the audit as a unit test |
| M5.6 | **`glyim-irgen`**: generate enums, constructors, visitors/folders, printer/parser, serde, validator scaffolding from `ir/mir.schema` + THIR schema (CT-7b) | hand-written MIR traversals removed; adding a variant touches only the schema + semantic code |
| M5.7 | **Textual MIR** `.gmir` + `glyim-mir-opt` + FileCheck-style tests; migrate pass unit tests to data-driven fixtures (CO-2) | round-trip on 100% fixtures + random MIR |
| M5.8 | **Remove the bytecode backend and VM** (ADR-004) after the ledger shows no unique features; migrate/delete tests; −≈9k LOC and one whole executor | crates gone; ledger parity matrix has no bytecode column |
| M5.9 | **Kill silent error paths** (F10): sentinel builtins, `ConstRef` zero-globals → typed `ConstValue` pools emitted properly, or compile errors/ICE with witnessed gaps | class-b gap count from F10 = 0 |
| M5.10 | Panic ratchet to **0** outside `// INVARIANT` allow-list; MIR passes `Result`-returning | RG-07 partly green |
| M5.11 | Pass manager skeleton (PF-14) with `REQUIRES/ENSURES`, verify-after-each-pass, per-pass stats, `--opt-bisect-limit` | existing 5 passes ported; bisect finds an injected bad pass |

**Exit gate:** ledger metrics: public IR fields = 0, `from_raw` = 0 outside arenas, wildcard IR arms = 0, non-allowlisted panics = 0 in IR crates; validate-after-each-pass green on all fixtures + 100k generated programs; differential still 0 unregistered disagreements.

---

### M6 — Query engine & incremental compilation (weeks 22–32) · lanes F, T, M, L

| WI | Work | DoD |
|---|---|---|
| M6.0 | **ADR-002 spike (weeks 22–24):** prototype the frontend slice (parse → def-map → typeck of 3 corpus sizes) on (A) Salsa and (B) a purpose-built engine. Criteria: overhead vs straight-line ≤ 15%, memory, cycle handling, parallel execution, cancellation, durability, diagnostics replay, persistence, ergonomics. Default: **adopt Salsa** (used by rust-analyzer and ty), add persistence at the CGU/object-cache layer rather than persisting the dep-graph | ADR-002 accepted with numbers |
| M6.1 | **Stable identity:** `DefPathHash`, `HirId=(owner, local idx)`, `StableHasher`/blake3 fingerprints; kill order-dependent `LocalDefId` numbering | editing an unrelated body never changes another item's IDs (property test over random edits) |
| M6.2 | Inputs & durability: `SourceFile` (content hash), `CompilerConfig`, stdlib = high durability | invalidation test matrix |
| M6.3 | Item-tree + def-collection + import-resolution queries (PF-4, PF-6) | body-only edit leaves item-tree fingerprint unchanged |
| M6.4 | Signature queries and **body queries** (typeck → thir → mir_build → borrowck → mir_opt) with early cutoff (backdating) | `--explain-rebuild` shows minimal recompute set |
| M6.5 | Diagnostics as accumulator outputs replayed on cache hit, stable order | cached run prints identical diagnostics |
| M6.6 | Mono/CGU queries → **content-addressed CGU object cache** (bounded, atomic, GC'd); `glyim --incremental` default in `glyip build` | edit one fn ⇒ ≤ 1 CGU recompiled |
| M6.7 | **LSP on the query DB**: cancellation, snapshots; replace `symbol_index/dep_graph/reference_graph` ad-hoc caches or re-express as queries | RG-16 latency budgets measured; stress test with typing storms |
| M6.8 | **Incremental ≡ clean oracle** (CO-6): random token-/item-level edit sequences, compare diagnostics + object bytes; `GLYIM_INCR_VERIFY=1` re-computes a sample of cache hits and compares fingerprints | 10k sequences nightly, 0 disagreements |
| M6.9 | `--explain-rebuild` and dep-graph visualiser | documented output format |
| M6.10 | PF-2 parallel per-file parse/expand; PF-1 flat syntax arena + lazy lossless tree; PF-5 macro cache | throughput gates from §4.4 |

**Exit gate:** RG-05 measured green; body-edit → diagnostics ≤ 100 ms p95 on the 50k-LOC synthetic; `glyip build` after a one-line edit rebuilds ≤ 1 CGU.

---

### M7 — Parallel compiler (weeks 28–38) · lanes T, M, B, Q

| WI | Work | DoD |
|---|---|---|
| M7.1 | **ADR-003 + PF-7**: concurrent `TyCtx<'tcx>` (sharded interner, append-only arenas), no freeze model, loom tests | `unsafe`-free (or tiny audited core); loom green |
| M7.2 | `glyim-par`: ordered parallel helpers, scoped threads over rayon, panic/cancellation propagation, `-Zthreads` | determinism helper tests |
| M7.3 | Parallel typeck, borrowck, MIR build/opt, mono collection per body | each stage: speedup ≥ 0.6×cores to 8 cores; byte-identical output |
| M7.4 | **PF-8 trait solver redesign**: `(TraitDefId, SimplifiedType)` impl index, canonical queries + global/provisional caches, indexed coherence/overlap | trait-heavy exponent ≤ 1.1; differential vs naive solver (CO-10) green |
| M7.5 | PF-9 inference engine upgrade (union-find + undo log + flag fast paths) | typeck instruction win |
| M7.6 | **PF-17**: stable CGU partitioning, per-thread LLVM contexts, per-CGU parallel codegen + object cache integration | codegen scaling gate |
| M7.7 | **PF-19**: in-process ThinLTO (LTO C API), drop `llvm-lto2` subprocess; new-PM pipelines per opt level | ThinLTO e2e green; no subprocess |
| M7.8 | **PF-20** linker auto-selection (mold/wild/lld/cc), `--gc-sections`, PIC output (drop `-no-pie` hack) ⚠ default reloc model | link benchmark recorded; hardened binaries |
| M7.9 | **Chaos/determinism CI** (CT-9): threads {1,2,4,8,16} × seeds × path prefixes, byte-identical required; TSAN nightly (nightly toolchain); Miri on allow-listed unsafe | RG-04 measured green |
| M7.10 | PF-13 borrowck: SCC-condensed regions, sparse bitsets, two-tier location-insensitive/precise analysis | borrowck share ≤ *cal.*; Datalog oracle agreement (CO-10) |
| M7.11 | PF-12 MIR representation diet (inline binary-op operands, small-vec aggregates, interned projections, no whole-body clones) | allocation-count gates |

**Exit gate:** scaling targets (§4.5) met; RG-04 green; no data races under TSAN on the corpus.

---

### M8 — Separate compilation & ecosystem (weeks 32–42) · lanes B, F, T, L

| WI | Work | DoD |
|---|---|---|
| M8.1 | **Multi-crate** `CrateId` support: crate graph as query input, extern-crate resolution, visibility (`pub(crate)`), cross-crate coherence/orphan rules | two-crate and diamond fixtures |
| M8.2 | **`.gmeta` metadata** (ADR-011): zero-copy mmap tables (defs, signatures, generics, trait/impl indexes, const values, inlinable MIR, stable hashes), versioned + checksummed; **decoder fuzzed to never crash on corrupt input** | round-trip tests; fuzz target in farm |
| M8.3 | **Precompiled stdlib** (core/alloc/std) in a sysroot per (compiler hash, target, profile); **remove `--with-stdlib` source-prepending** ⚠ | hello-world cold compile ≥ 10× faster (rebaselined) |
| M8.4 | Shared generics: stdlib instantiations reused across crates by instance fingerprint; COMDAT/`linkonce_odr` dedupe at link | duplicated-code metric drops |
| M8.5 | Pipelined compilation: downstream starts when upstream `.gmeta` exists | `glyip --timings` shows overlap |
| M8.6 | **`glyip` v2**: DAG scheduler + jobserver, hermetic content-addressed keys, local + remote (HTTP/S3) artifact cache, links executables, workspaces, `--timings`, `glyip test` native by default | 20-crate workspace builds with correct parallelism; remote-cache hit ⇒ no compile |
| M8.7 | **PubGrub-style backtracking resolver** with explainable conflicts replaces greedy BFS; deterministic lockfile | resolver property tests vs reference on generated registries |
| M8.8 | Target matrix: linux-aarch64, macos-aarch64, windows-x86_64 CI jobs; linker drivers per target | RG-19 measured |

**Exit gate:** multi-crate workspace example, precompiled stdlib, remote-cache demo, all in CI.

---

### M9 — GAM semantic spec & reference interpreter (weeks 26–40) · lane M (+Q) — runs in parallel with M7/M8

| WI | Work | DoD |
|---|---|---|
| M9.1 | **GAM spec** (`docs/spec/gam.md`): memory model, integer semantics (ADR-006), UB list, evaluation order (ADR-013), panic=abort (ADR-005), FFI boundary; rules registered as `GAM-*` ledger items | every rule has an ID; spec-coverage metric live (RG-20) |
| M9.2 | **New interpreter core**: byte-addressed memory, allocation identity + provenance, typed loads/stores via layout, function pointers/vtables/statics, explicit checked arithmetic | passes all `run-pass` fixtures |
| M9.3 | **UB detection**: OOB, use-after-free, uninit reads, misalignment, invalid `bool`/enum/ref validity; each UB class has positive & negative tests | UB-class coverage in ledger |
| M9.4 | Optional **aliasing model** (`--ub-check=aliasing`, Tree-Borrows-style) to validate borrowck/`noalias` assumptions | gate for PF-18 `noalias` emission |
| M9.5 | **Const-eval = interpreter**: delete the separate evaluator in `glyim-const-eval` (and its `u128 as usize`-class bugs) ⚠ | one arithmetic implementation in the repo (grep-enforced) |
| M9.6 | Interpreter speed (PF-29): pre-lowered compact instruction stream, layout cache | const-eval benchmark ≥ *cal.*× faster |
| M9.7 | Delete the value-model interpreter, `dead_frames` archive and related hacks; port tests | old crate code removed |
| M9.8 | **Implement ADR-005 (panic = abort)**: remove SEH cleanup-pad dead code, interpreter unwind path, `cleanup` fields on terminators (or keep as `None`-only until unwinding is revisited); adapt `thread.g` semantics | KNOWN_GAPS §2/§4 entries closed by deletion |
| M9.9 | Spec-conformance generator: edge-case tests derived from each GAM rule | RG-20 ≥ 95% |

**Exit gate:** GAM-vs-LLVM differential on 1M generated programs: 0 unregistered disagreements; one interpreter, one const-evaluator.

---

### M10 — Verified optimizer (weeks 38–50) · lanes M, Q

| WI | Work | DoD |
|---|---|---|
| M10.1 | **Dataflow framework**: generic lattice solver, RPO worklist, bitset domains; monotonicity/termination property tests for each transfer function | all existing analyses ported |
| M10.2 | **SSA/analysis infrastructure**: `SsaLocals`, dominators (Cooper–Harvey–Kennedy), loop forest, simple alias analysis seeded by borrowck uniqueness facts | analyses verified against naive versions |
| M10.3 | **PF-30 classic suite**: copy-prop, GVN, SROA, dest-prop, jump-threading, match-on-known-discriminant, dead-store elim, redundant-drop removal, bounds-check elim (range facts), conservative LICM, shared `fold` module with the interpreter for constant folding | each pass individually toggleable and differential-tested (pass-on ≡ pass-off) |
| M10.4 | **PF-31 MIR inliner**: cost model, `#[inline]` hints, call-graph SCC cycle guard, closures, debug-info preserving, cross-crate via `.gmeta` | runtime benchmark geo-mean win; compile-time budget respected |
| M10.5 | **CO-4 rewrite-rule DSL + SMT verification** (`rules/*.gopt`, solver in dev-tooling only) | unprovable rule cannot merge; counterexamples become witnesses |
| M10.6 | **CO-3 `glyim-tv`** (bounded translation validation) wired per pass under `GLYIM_TV=1` | 0 violations on fixtures + 1M generated MIR bodies; unknown rate tracked |
| M10.7 | **PF-32 e-graph mid-end** (ADR-009, experimental): ægraph over pure scalar ops, rule DSL shared with M10.5, bounded saturation, extraction cost model | adopted only if it clears its gate; otherwise stays behind `-Zmir-egraph` |
| M10.8 | Pass-pipeline tuning per opt level on the bench corpus; `-Os`; `glyim-bisect` auto-bisects miscompiles to first bad pass/function | compile-time vs runtime frontier documented |
| M10.9 | Verified-by-differential gates in nightly: every pass on/off × generated corpus | RG-03 measured green |

**Exit gate:** runtime benchmark geo-mean ≥ *cal.*% better than M2 baseline with MIR opts alone; TV clean; compile-time budget not regressed beyond *cal.*%.

---

### M11 — Backends, layout, output speed (weeks 42–54) · lane B (+Q)

| WI | Work | DoD |
|---|---|---|
| M11.1 | **PF-18 LLVM IR quality** (SSA locals, attributes, `cold`/`expect`, linkage, sections); `noalias` only after M9.4 validates; every attribute class differential-tested on/off (`--no-llvm-attrs`) | LLVM time and IR size gates; no disagreements |
| M11.2 | **PF-22 layout work**: niche-filling enums, field reordering, scalar-pair/vector ABI, discriminant elision; verified vs LLVM `DataLayout` and GAM | `Option<&T>` is pointer-sized; layout oracle green |
| M11.3 | Debug-info quality (DWARF, CodeView) incl. inlined scopes; scripted gdb/lldb tests | debuginfo suite in CI |
| M11.4 | **PF-21 Cranelift spike** (ADR-010) with the go/no-go rubric (≥2× backend time reduction vs LLVM-O0 **and** ≥25% e2e) | ADR-010 closed either way; if go → `--backend cranelift` for `-O0` with differential coverage |
| M11.5 | Incremental link experiments (wild) and in-place patching spike behind flags | reported; adopted only on measured win |
| M11.6 | **PF-23 runtime/stdlib performance** (hash map, sort, search, I/O, allocator choice) with runtime benchmark budgets | stdlib budgets in `perf/budgets.toml` |
| M11.7 | **PF-27** PGO (+relayout) + jemalloc/mimalloc for shipped compiler; user-program PGO (`-C profile-generate/use`) as stretch | measured instruction reduction |
| M11.8 | Cross-target runtime tests (aarch64, macOS, Windows) with ABI cross-checks | RG-19 |

**Exit gate:** RG-15 measured; layout/ABI oracles green on all Tier-1 targets.

---

### M12 — Hardening & 1.0 release candidate (weeks 50–60) · all lanes

| WI | Work | DoD |
|---|---|---|
| M12.1 | **Soak:** continuous fuzzing to RG-01 thresholds; triage everything; clear class-b gaps to 0 | RG-01/02/10 green |
| M12.2 | **Feature closure:** all `core` features L5, `std` L4 | RG-09 green |
| M12.3 | Docs: language reference where each normative statement is tied to a feature or GAM rule; doctests compile via the harness; `--explain` for all codes; stdlib API docs 100% | RG-06/20/21 green |
| M12.4 | Release engineering: reproducible builds (bit-identical across two builders), SBOM, sigstore-signed artifacts, installers, versioning + edition policy | RG-17/18/23 green |
| M12.5 | Security review: threat model (compiler as a parser of untrusted input): recursion/time/memory limits, macro sandbox policy, `include!` path policy, supply chain | written model; limits fuzzed |
| M12.6 | Final perf tightening: budgets ratcheted to achieved values; public perf dashboard | RG-11…16 green |
| M12.7 | Independent re-derivation of M0 metrics; `ledger release-check v1.0.0` | passes; tag created |

---

## 7. Agent operating protocol

### 7.1 The work loop

```
ledger next --lane <L>            # 1. pick the top item (deterministic)
ledger explain <ID>               # 2. what evidence is missing / what blocks it
git switch -c wi/<id>-<slug>      # 3. one work item per branch/PR
<write the witness / failing test FIRST>   # 4. TDD: strict-xfail fixture, proptest, or perf benchmark
<implement>                       # 5. smallest change that satisfies DoD
just verify                       # 6. fmt + clippy(deny) + tests + ledger check + perf-compare
ledger diff origin/main HEAD      # 7. paste into the PR: level changes, gap changes, perf deltas
```

### 7.2 Rules

1. **Never weaken a gate.** Thresholds only tighten. A temporary exception requires a `ledger/waivers.toml` entry with an issue link and an expiry ≤ 14 days; expired waivers fail CI.
2. **Witness first.** Bugs are added to the gap registry with a reproducing fixture *before* the fix. A fix that makes the witness pass must also delete the xfail marker and close the gap (CI enforces).
3. **Class-level defence (C8).** Every bug-fix PR includes a bug-class record and the defence (type/constructor/validator/lint/oracle/generator/metamorphic). If the defence is large, open a prevention-debt item and link it; do not silently skip.
4. **Break freely, migrate fully.** Breaking changes are allowed (C10). In the *same PR*: codemod script under `scripts/codemods/`, all call sites updated, `docs/migrations/NNNN-title.md`, ledger diff.
5. **ADR before architecture.** Any item marked ADR in §6/§8 starts with a time-boxed spike (≤1 week) and ends with an accepted ADR containing numbers. Do not start the dependent work items before acceptance (default decisions in §8 are used only if the spike is explicitly skipped by a maintainer).
6. **Stop-and-report triggers:** a gate fails and the cause is unclear after one bisect; a perf regression cannot be explained by the diff; TV/differential/fuzzing finds a miscompile (→ witness + class-b gap first, it outranks feature work); a change would exceed 800 changed lines outside codemod output (split it); an ADR assumption is falsified.
7. **No hand-written status prose.** Do not create `HANDOFF`/`PROGRESS`/`STATUS` markdown. Run `ledger handoff` — it emits the state from the ledger.
8. **Lane discipline.** Edit crates owned by your lane (`ledger/owners.toml`); cross-lane changes go through a small interface PR first.
9. **Compute budgets.** PR tier ≤ 20 min; nightly runs the heavy oracles (differential, TV, metamorphic, mutation sample); weekly runs soak/fuzz farms and full mutation testing. Never put a nightly-only check in the "required" set without a PR-tier smoke version.
10. **Flaky tests are bugs.** Quarantine with an expiry and a ledger entry; never retry-until-green.

### 7.3 Review checklist (reviewer = human or reviewer-agent)

1. Could a **type** have prevented this? 2. A **constructor**? 3. A **validator** at a phase edge? 4. An **exhaustive match**? 5. Only then: is there a test, a generator rule, a metamorphic relation? 6. Perf delta shown? 7. Deterministic under chaos mode? 8. Ledger diff reviewed? 9. Docs/`--explain` updated? 10. Is any new `unsafe`/`unwrap`/`panic` justified in the allow-list?

### 7.4 Universal definition of done (every WI)

`cargo fmt --check` · `cargo clippy --workspace --all-targets -- -D warnings` · `cargo nextest run --workspace` · `ledger check` · `ledger diff` shows no regressions · `perf-compare` within budget · no new entries in panic/unsafe/wildcard/HashMap ratchets · docs and migration note updated · ADR linked if a decision was made.

---

## 8. Decision register (ADRs) with proposed defaults

Each ADR is created as *proposed* in M0.4 and must be *accepted with evidence* before its dependent work starts.

| ADR | Decision | Proposed default | Decide by | Falsified if… |
|---|---|---|---|---|
| 001 | Ledger data formats | TOML registries, JSONL evidence, JSON/MD outputs, pure `compute` | M1.1 | determinism test fails across machines |
| 002 | Query engine | Adopt **Salsa** + persist at CGU/object-cache layer; custom engine only if the spike shows >15% overhead or missing parallel/cancel support | M6.0 | spike numbers |
| 003 | Concurrent `TyCtx` | Sharded interner + append-only arenas, `'tcx` lifetime, `Ty(u32)` handles | M7.1 | loom/perf results |
| 004 | Remove bytecode backend + VM | **Remove** (one fewer executor; interpreter is the oracle, LLVM/Cranelift the codegens) | M5.8 | ledger shows a unique supported use case with an owner |
| 005 | Panic model | **Abort only**; delete unwinding machinery | M9.8 (decide at M4) | a requirement for `catch_unwind` semantics in std/threads |
| 006 | Integer overflow | **Checked always**; explicit `wrapping_*`; per-crate `overflow="wrap"` as *defined* semantics; never opt-level dependent | M4.2 | unacceptable runtime cost after BCE/range analysis (then offer defined-wrap default for release) |
| 007 | Proc-macro isolation | **Out-of-process RPC**; WASM as stretch | M5 (design), M7 | RPC overhead dominates macro-heavy builds |
| 008 | Interpreter memory model | Byte-addressed + provenance (Miri-style); aliasing model optional | M9.1 | prototype cannot meet const-eval speed gate |
| 009 | Mid-end optimizer | Classic suite first; **ægraph** experimental behind a gate | M10.7 | gate in §4.4 PF-32 |
| 010 | Fast debug backend | Cranelift spike with rubric (≥2× backend, ≥25% e2e) | M11.4 | rubric |
| 011 | Metadata format | Custom little-endian offset tables with `zerocopy`; `rkyv`-like only if it matches speed + validation story | M8.2 | decode/validation benchmarks |
| 012 | Hashing | FxHash/foldhash for in-memory maps + `FxIndexMap` for determinism; **blake3** for content fingerprints; resource limits for hostile input | M2/M3 | HashDoS reproduced on hostile identifiers |
| 013 | Evaluation & drop order | Fully specified left-to-right; temporaries end of statement | M4.2 | conflict with existing fixtures (then fix fixtures) |
| 014 | Debug-info policy | DWARF/CodeView line tables always; full variable info at `-g` | M11.3 | — |
| 015 | Platform tiers | Tier-1: linux-x86_64, linux-aarch64, macos-aarch64; Tier-2: windows-x86_64 | M8.8 | CI capacity |

---

## 9. Risks and mitigations

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | **Refactor blast radius** (M3, M5 touch every crate) | High / High | M4 safety net first; crate-by-crate leaf→root PRs with codemods; differential + chaos CI must stay green at every PR; no PR > 800 manual lines |
| R2 | **Query-engine overhead or fit** (the "queries everywhere" critique) | Med / High | M6.0 spike with numeric criteria; hybrid architecture (§4.1); early cutoff only where it pays; persisted caching at CGU granularity |
| R3 | **GAM interpreter rewrite is large** (15.6k LOC, value-model today) | High / Med | staged: runs fixtures first, UB detection second, aliasing last; old interpreter retained until parity (M9.7) |
| R4 | **Translation-validation unknown rate** (SMT timeouts) | High / Med | bounded semantics, per-pass small windows, report unknown rate; start as defence-in-depth, gate on *violations*, not unknowns |
| R5 | **LLVM/inkwell version coupling** (git-pinned inkwell, LLVM 23) | Med / Med | container-pinned toolchain; isolate FFI in one crate; `LTO` C-API use verified per LLVM bump; Cranelift tier reduces dependence |
| R6 | **Agent context loss across long horizon** | High / Med | ledger-as-memory (`ledger explain/next/handoff`), ADRs, small PRs, no prose status files |
| R7 | **Goodhart on the ledger** | Med / High | §3.11 defences: assertion-required fixtures, mutation-tested evidence, codeowners on policy |
| R8 | **CI cost/time explosion** | High / Med | three tiers; instruction-count (not wall-clock) perf gating; nightly sharding; cached corpora |
| R9 | **Perf work with no baseline = noise** | — | M2 precedes all perf items; every PR shows deltas |
| R10 | **Semantic-simplification scope creep** (ADR-005/006/013) | Med / Med | decide in M4, implement in M9; each is reversible via ADR; keep language surface unchanged otherwise |
| R11 | **Flaky nondeterminism from parallelism** | Med / High | chaos mode from M3, ordered merges, TSAN/loom, determinism gate RG-04 required from M7 |
| R12 | **Fuzz-farm compute availability** | Med / Med | start with ClusterFuzzLite/GH Actions cron; RG-01 CPU-hour accounting tolerant of slower accrual (milestone shifts, not gate weakening) |

---

## 10. Release criteria summary — what "production-ready v1.0" means

`ledger release-check v1.0.0` passes iff **all 24 readiness gates** (§3.10) are green on the release commit, evidence is reproducible (RG-04/17), and no waiver is active. The milestone→gate mapping:

| Gate | First measured | Required green |
|---|---|---|
| RG-01, 02 | M4 | M12 |
| RG-03 | M10 | M12 |
| RG-04 | M3 (chaos mode) / M7 (parallel) | M7 onward, always |
| RG-05 | M6 | M12 |
| RG-06 | M3 (error registry) | M12 |
| RG-07 | M3 | M5 (IR crates) → M12 (all) |
| RG-08 | M4 | M12 |
| RG-09, 10 | M1 | M12 |
| RG-11–13 | M2 | M8 → M12 |
| RG-14 | M6 | M8 → M12 |
| RG-15 | M10 | M11 → M12 |
| RG-16 | M6 | M12 |
| RG-17, 18 | M0 (supply chain), M12 | M12 |
| RG-19 | M8 | M12 |
| RG-20 | M9 | M12 |
| RG-21–24 | M8–M12 | M12 |

