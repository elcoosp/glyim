# Glyim Compiler Toolchain — Production Readiness & Performance Plan

**Scope reviewed:** 302 files / ~83k LOC across `glyim-*` (lexer → parser → HIR → trait
solver/typeck → MIR → borrowck → optimizer → LLVM/bytecode backends → runtime),
`glyim-lsp`, and the `glyip` package manager/build tool.

**Verdict:** This is a genuinely ambitious, well-organized rustc-style pipeline
(query-ish `TyCtx`/`TyCtxMut` split, proper HIR/MIR/THIR staging, a real
borrow-checker with two-phase borrows, a dedicated test harness crate). It is
*not* production grade yet. The biggest risks are one soundness bug in the type
interner, an O(n²) hot path in the same file, panic-driven error handling
throughout the front half of the pipeline, an under-specified concurrency story
(rayon is already in the codegen path), and a build tool (`glyip`) that hashes
full file contents on every invocation. None of this is a rewrite — it's
targeted, and I've laid it out in dependency order below.

---

## 1. Critical bugs found (fix first, before anything else)

### 1.1 `TypeArena` — real aliasing/soundness bug, not just a comment risk
**File:** `glyim-type/src/type_arena.rs`

The module doc explains, correctly, that the *previous* design (per-context
`Vec<TyKind>`) caused invalid handles across contexts. The fix — a shared
`&'static TypeArena` behind raw pointers — trades that bug for a worse one:

```rust
pub fn alloc_ty(&self, kind: TyKind, flags: TypeFlags) -> Ty { ... }   // &self !
pub fn ty_kind(&self, ty: Ty) -> &TyKind { unsafe { &*self.types } ... }
```

Every mutating method on `TypeArena` takes `&self`, not `&mut self`, and
readers take no lock at all. The design *relies* on the fact that `TyCtx`
and `TyCtxMut` are each single Rust values, so the borrow checker serializes
access to *them* — but `arena: &'static TypeArena` is a plain shared
reference that both a `TyCtx` and every `TyCtxMut` derived from it hold
**independently**. The borrow checker has no idea they alias the same heap
`Vec`s, because from its point of view they're different values. Concretely:

```rust
let ty_ctx = compilation.frozen_ty_ctx();      // holds arena: &'static TypeArena
let mut ty_ctx_mut = ty_ctx.to_mut();          // shares the SAME arena pointer

let k: &TyKind = ty_ctx.ty_kind(some_ty);      // borrows `ty_ctx`, not `ty_ctx_mut`
let _ = ty_ctx_mut.mk_ty(TyKind::Unit);        // mutably pushes into the SAME Vec<Box<TyKind>>
                                                // while `k` is still alive — the compiler
                                                // never flags this, because it's tracking
                                                // two different Rust values.
```

This is exactly the "aliasing class of bugs" the doc claims to have fixed —
it's just moved from "caught as an OOB panic" to "silently compiles, is a
Stacked-Borrows violation, and is a live data race the moment
`glyim-pipeline` parallelizes anything that touches `TyCtx` (it already pulls
in `rayon`, see §1.3)." In practice today the boxed `TyKind` payloads don't
move on `Vec` growth (only the pointer array does), so you won't see
corruption yet — but this is UB under the Rust abstract machine, will be
flagged by Miri, and is one `par_iter` away from being a real, hard-to-repro
data race.

**Fix:**
- Replace the hand-rolled `*mut Vec<Box<T>>` with an append-only structure
  that is *actually* designed for "push via `&self`, get back a `'static`-ish
  stable reference" — e.g. `elsa::FrozenVec<Box<TyKind>>` / `append-only-vec`,
  or a chunked slab (`Vec<Box<[MaybeUninit<TyKind>; CHUNK]>>` behind a single
  `RwLock` for the chunk-table only, data itself never touched after write).
  These crates are audited specifically for this pattern and remove the
  hand-written `unsafe`.
- Alternatively, if you want to keep it hand-rolled: gate **reads** through
  the same `write_gate` (a `RwLock` instead of `Mutex<()>` + raw pointer),
  so `ty_kind`/`substitution_args` take a read guard and `alloc_ty` takes a
  write guard. This reintroduces a small lock cost per read but is provably
  sound and is the honest translation of the "reads/writes are serialized"
  claim the comment already makes but doesn't enforce.
- Either way: add a `loom` test (for the concurrency model) and run the
  `glyim-type` test suite under `cargo miri test` in CI (see §5). This is the
  single highest-leverage soundness fix in the codebase and should land
  before any of the performance work below, since the performance work
  (§2.1) touches the exact same file.

### 1.2 `intern_substitution` is O(n) per call → O(n²) over a compilation
**File:** `glyim-type/src/type_arena.rs`, `intern_substitution`

```rust
if let Some(pos) = data.iter().position(|e| **e == small) {
    return Substitution::from_raw(pos as u32, len);
}
```

Every substitution list ever allocated is scanned linearly to check for
de-duplication, while the sibling `alloc_ty` correctly uses a `HashMap<TyKind, Ty>`
for the same purpose. Any program with a non-trivial number of generic
instantiations (which is the normal case once monomorphization runs) turns
type-substitution work into a quadratic pass. This is the top performance fix
in the type system — see §2.1 for the concrete replacement.

### 1.3 Dead parallel work in the codegen-unit pipeline
**File:** `glyim-pipeline/src/lib.rs`, around the `cgus.par_iter()` block

```rust
let _cgu_stats: Vec<(usize, usize)> = cgus
    .par_iter()
    .map(|cgu_indices| { /* compute body_count, total_locals */ })
    .collect();
```

The result is bound to `_cgu_stats` and never used again — this spins up a
rayon fork-join over every codegen unit purely to throw the answer away. On a
large program this is pure wasted CPU/scheduling overhead on the compiler's
own critical path. Either wire it into `-Z time-passes`/`--stats` output, or
delete it. See §2.4 for what real parallel codegen should look like here.

### 1.4 `glyip` test runner: non-portable, silent-failure process kill
**File:** `glyip/src/commands.rs` (subprocess timeout handling)

```rust
let _ = std::process::Command::new("kill")
    .arg(child_pid.to_string())
    .status();
```

`Child` already has a portable `.kill()` method (SIGKILL on Unix,
`TerminateProcess` on Windows). Shelling out to a `kill` binary by PID is:
Unix-only (breaks on Windows, where `glyip test` will simply never be able to
stop a hung test), sends SIGTERM rather than SIGKILL so a genuinely hung
process (the reason you're in this branch at all) can ignore it, and the
error is swallowed (`let _ =`) so a missing `kill` binary (minimal/containerized
CI images) fails silently and the runner will then hang on the still-open pipe.
**Fix:** keep the `Child` inside an `Arc<Mutex<Option<Child>>>` (or send it
back over a second channel) so the main thread can call `child.kill()`
directly; drop the external process spawn entirely.

### 1.5 `glyip` fingerprinting hashes full file content on every build
**File:** `glyip/src/fingerprint.rs`, `Fingerprint::from_file`

```rust
let content = fs::read(path)?;
let metadata = fs::metadata(path)?;
let mut hasher = Sha256::new();
hasher.update(&content);
```

This reads and SHA-256-hashes *every* source file's full bytes on *every*
invocation, including no-op incremental builds. Cargo's own trick (and the
right one here) is a two-tier check: compare `(size, mtime)` against the
stored fingerprint first (cheap `stat()`, no read), and only fall back to a
content hash when those disagree (handles clock skew / touch-without-edit).
For a workspace with thousands of `.g`/`.rs`-equivalent files this is the
difference between an incremental build being O(files-changed) vs.
O(total source bytes) on every single `glyip build`.

### 1.6 Reactor: fixed 50 ms poll timeout instead of a wake primitive
**File:** `glyim-runtime/src/reactor.rs`

```rust
// Block briefly for readiness. A non-zero timeout lets new
// registrations be picked up promptly without a dedicated wake channel.
if poll.poll(&mut events, Some(Duration::from_millis(50))).is_err() { continue; }
```

Every new I/O registration can incur up to 50 ms of added latency before the
reactor even looks at it, and the reactor thread wakes and does a syscall
every 50 ms even when the whole program is idle. `mio::Waker` exists
precisely to solve "wake poll() immediately when a new registration or
shutdown arrives" — register one `Waker` token in the same `Poll`, and have
the command-channel producer call `waker.wake()` after sending. This removes
both the latency tax and the idle busy-poll, and is a ~20-line change.

Also in the same function: `sources.lock().unwrap().insert(...)` immediately
followed by a second, separate `sources.lock().unwrap().get_mut(...)` —
two independent lock acquisitions where one held guard would do; harmless
today only because the reactor is single-writer, but it's a foot-gun the
moment that assumption changes.

### 1.7 Miscellaneous smaller correctness/robustness items to sweep
- **412** `unwrap()`/`expect()`/`panic!()` call sites outside `glyim-test`.
  A syntax error, an unresolved name, or a malformed CLI flag should never
  `panic!` a compiler — it should produce a `Diagnostic`. Section 3.1 gives
  the concrete remediation strategy (this is too large to fix file-by-file
  in this plan, but it's the single biggest item standing between "works on
  my machine" and "production grade").
- **230** `#[allow(dead_code)]` annotations. A handful are legitimate
  (`cache.len()` used only in future callers, cfg-gated platform code), but
  this density usually means half-finished features or copy-pasted
  scaffolding. Worth an audit pass with `cargo +nightly udeps` and
  `#[deny(dead_code)]` per-crate once the backlog is triaged — see §5.2.
- `glyim-runtime/src/lib.rs` and `fs.rs` are ~1,600 lines of near-identical,
  hand-written `#[unsafe(no_mangle)] pub unsafe extern "C" fn glyim_*` FFI
  shims (one per syscall: `fs_open`, `fs_read`, `fs_write`, `fs_rename`, …).
  Every one repeats the same "reconstruct a `&[u8]` from a raw pointer, map
  the `std::io::Result` to an `i32` errno-ish return" boilerplate by hand.
  This is exactly the surface where a copy-paste mistake becomes a real
  memory-safety hole, and it's completely mechanical — see §3.3 for a macro-
  based replacement that cuts the surface by ~70% and makes new syscalls a
  one-line addition instead of a 20-line unsafe block to review.
- No workspace-root `Cargo.toml` was present in the dump (every crate uses
  `edition.workspace = true` / `version.workspace = true`, implying one
  exists but wasn't included). Confirm it exists and carries the profile
  settings in §2.5 — if it doesn't, that's priority zero.

---

## 2. Performance plan

### 2.1 Fix the type/substitution interner (do this together with §1.1)
Replace both tables with the same shape:
```rust
type_index: DashMap<TyKind, Ty>,            // or HashMap behind the write lock
subst_index: DashMap<SmallVec<[GenericArg; 4]>, Substitution>,
```
using `DashMap` (sharded, lock-striped) instead of a single `Mutex<HashMap>`
removes the alloc-time global lock as a scaling bottleneck once you do
anything in parallel (mono, per-CGU codegen). Combined with the storage fix
in §1.1 this turns "intern a type/substitution" into a genuinely O(1)
amortized, thread-safe operation, which is a prerequisite for §2.4.

### 2.2 Interning and allocation hygiene elsewhere
- `694` `.clone()` call sites overall. A sampling pass shows a mix of
  legitimate small-value clones (`Ty` is presumably `Copy`-sized) and
  larger structural clones (`Vec<GenericArg>`, `TyKind`, MIR `Body` clones in
  the pipeline — e.g. `mono_items[idx].body.clone()` inside the CGU
  partitioning loop in `glyim-pipeline/src/lib.rs`). Wrap `Body` (and any
  other >~64-byte MIR/HIR node moved across the CGU boundary) in `Arc<Body>`
  end-to-end so partitioning is a pointer clone, not a deep MIR clone. Some
  of this already exists (`Arc<Body>` shows up in `MirCompilation`) — audit
  for the remaining raw clones and standardize.
- `AdtDef`/`FnSig`/`TraitDef` tables are `HashMap`s keyed by small `*Id`
  newtypes throughout `TyCtx`. Since these IDs are dense, sequentially
  allocated integers, swap to `glyim_core::arena::IndexVec` (already used for
  `regions: IndexVec<RegionVid, Region>`) wherever the key space is dense —
  this is a straight `HashMap` → `Vec`-indexed lookup win (no hashing, no
  probing, better cache locality) and the codebase already has the
  `IndexVec` primitive, it's just under-used outside `glyim-type::ty_ctx`.

### 2.3 Borrow checker liveness/loan analysis
`glyim-borrowck` implements a standard backward dataflow liveness pass over
`FixedBitSet`s per basic block — this is the right data structure choice.
Two things worth checking/adding:
- Confirm the dataflow fixed point uses a **worklist** (only re-visit blocks
  whose predecessors' state changed) rather than iterating all blocks to a
  fixed number of passes — worklist iteration is the standard win for CFGs
  with loops and is usually a 2-5x pass-count reduction on real programs.
- Loan-conflict detection is described as checking "each statement" against
  "active loans" — confirm this is indexed by `(place, loan)` rather than a
  linear scan of all loans per statement; for functions with many borrows
  this is the difference between linear and quadratic in loan count.

### 2.4 Real parallel codegen (replace §1.3's dead code with live work)
Codegen-unit partitioning already exists (`partition(&mono_items, max_cgus)`,
`compute_max_cgus()` using `available_parallelism()`) — the scaffolding for
parallel codegen is present but the `par_iter` block does nothing useful.
Once §1.1/§2.1 make `TyCtx` reads safe to share across threads (read-locked,
not raw-pointer), the actual `backend.generate(&all_bodies, out_path)` call
per CGU is the thing that should run in parallel (mirroring how rustc uses
independent LLVM contexts per CGU), not the stats computation. This is the
highest-value performance change in the whole plan for large programs, but
it is *gated* on the soundness fix in §1.1 — do not parallelize codegen
before that lands, or you will turn a latent UB bug into an observable one.

### 2.5 Build/link profile
Verify the (missing-from-dump) root `Cargo.toml` sets:
```toml
[profile.release]
lto = "thin"          # "fat" for the LLVM backend crate specifically if link time allows
codegen-units = 1      # or leave default + rely on workspace parallelism during dev
panic = "unwind"       # required if any crate catches panics across FFI; otherwise "abort" is faster
strip = "debuginfo"    # ship stripped release binaries for glyip/glyc

[profile.dev]
opt-level = 1          # glyim-codegen-llvm and the const-evaluator are hot even in debug builds
```
and that `glyim-codegen-llvm` isn't rebuilding/relinking LLVM's C++ bindings
on every `cargo check` — confirm the `llvm-sys`/equivalent build script is
cached via `sccache` in CI (see §5).

### 2.6 LSP responsiveness
`SourceMap::span_to_position` is already a precomputed `line_starts` binary
search — good, no change needed. The thing worth checking under load is
`glyim-lsp/src/reference_graph.rs` (787 lines, builds a reference graph) and
`symbol_index.rs`: confirm these are incremental (recompute only the changed
file's contribution on each keystroke) rather than rebuilt whole-workspace
on every edit, since that's the usual source of "LSP goes to 100% CPU on a
50k-line codebase" bug reports.

---

## 3. Path to "production grade"

### 3.1 Panic → Diagnostic conversion (the big one)
A production compiler must never crash on malformed *user* input — only on
genuine internal invariant violations, and even then it should ideally
produce an ICE report (like rustc's) rather than a raw Rust backtrace.
Concrete plan:
1. Draw the line: crates that consume **untrusted user input** directly
   (`glyim-frontend` lexer/parser, `glyim-hir` lowering, `glyim-typeck`,
   `glyim-meta` macro expansion) must return `Result`/push to `DiagSink` for
   every user-triggerable failure — no `unwrap()`/`expect()` on anything
   derived from source text. Crates operating purely on already-validated
   internal state (`glyim-mir` after `glyim-opt::validate`, codegen) may keep
   `debug_assert!`/`unreachable!()` for true invariants.
2. Add a `#[deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)]`
   lint gate on the "untrusted input" crate list from step 1, with narrowly
   scoped `#[allow(...)]` only where a comment justifies it (e.g. "array is
   non-empty by construction two lines up").
2b. Wrap `glyim-cli`'s top-level driver in `std::panic::catch_unwind`, and on
   panic emit a structured ICE report (compiler version, input file, panic
   message, backtrace) to a file and print a short "please file a bug at
   <url>, backtrace saved to <path>" message — this is the single highest-DX
   thing you can do for a language toolchain's users, and it's ~30 lines.
3. Track progress with a simple script counting `unwrap`/`expect`/`panic!`
   per crate in CI, failing if the count regresses (ratchet, not a hard
   ban — 412 sites won't be fixed in one PR).

### 3.2 Unsafe code audit and hardening
28 files contain `unsafe`. Priority order for audit + `# Safety` doc
comments (many already have partial ones, which is good practice — extend
it consistently):
1. `glyim-type/src/type_arena.rs` — §1.1, fix first.
2. `glyim-runtime/*` FFI boundary — every `extern "C"` function taking raw
   `(ptr, len)` pairs is a potential OOB read/write if the codegen backend
   ever emits a wrong length. Add `debug_assert!`-gated bounds sanity checks
   (e.g. reject `len > isize::MAX as usize`) and fuzz the FFI entry points
   directly (they're a perfect `cargo-fuzz` target: pure C ABI, no async).
3. `glyim-codegen-llvm/src/seh_ffi.rs` — Windows SEH FFI; confirm this is
   behind `#[cfg(windows)]` and has a Linux/macOS no-op stand-in that isn't
   silently miscompiled.
4. Run `cargo miri test -p glyim-type -p glyim-borrowck -p glyim-solve` in CI
   (Miri won't run through the LLVM/FFI-heavy crates, but the pure-Rust
   analysis crates are exactly where it's cheap to run and highest value).

### 3.3 Reduce the runtime FFI boilerplate with a macro
Replace the ~40 hand-written `glyim_fs_*`/`glyim_net_*`/`glyim_env_*`
functions with a declarative macro that generates the pointer/length
reconstruction and `Result`→`i32` mapping, e.g.:
```rust
ffi_fn! {
    fn glyim_fs_read(fd: i32, buf: *mut u8 [as buf_slice: buf_len: usize]) -> isize {
        |fd, buf_slice| read_fd(fd, buf_slice)
    }
}
```
This isn't just DX — every hand-written copy is a fresh chance to get a
`from_raw_parts` length wrong, and a macro means that mistake can only be
made once, in the macro definition, and is fixed everywhere at once.

### 3.4 Error handling / diagnostics consistency
`glyim-diag` exists as a dedicated crate — good. Verify (and standardize if
not already true):
- Every diagnostic has a stable, documented error code (`E0308`-style) so
  users/tools can grep/suppress by code.
- `glyim-cli` supports `--error-format=json` for editor/LSP consumption in
  addition to human-readable output (check if this already exists in
  `glyim-lsp/src/diagnostics.rs`'s conversion path and just needs exposing
  on the CLI too).

### 3.5 Concurrency story, written down
Right now the concurrency model is implicit: `TypeArena` assumes
single-threaded-per-compilation (§1.1), `glyip` spawns raw threads for test
timeouts (§1.4), the reactor runs a dedicated poll thread (§1.6), and
`glyim-pipeline` already imports `rayon`. Before adding more parallelism,
write a one-page ADR (architecture decision record) stating: what data is
allowed to cross thread boundaries (only `Arc<Body>`/interned `Copy` handles
after §1.1/§2.1), what the CGU-parallel codegen contract is (§2.4), and what
the runtime's threading model guarantees to `.g` programs. This is cheap
insurance against the next contributor reintroducing §1.1's bug in a new
form.

### 3.6 Resource limits / DoS hardening
A compiler is effectively a program that runs untrusted-ish input (a
malicious or just pathological source file). Check for, and add if missing:
- Recursion depth limits in the recursive-descent parser (`glyim-frontend`)
  and in type normalization/trait solving (`glyim-solve`) — deeply nested
  generics or macro expansions are the classic stack-overflow DoS vector for
  a compiler front end. `glyim-meta`'s macro expander in particular needs an
  expansion-count/depth limit (accidental or malicious infinite recursive
  macros are the #1 way real macro systems get DoS'd).
- The `check_large_mono_set(mono_ctx.items(), 1000)` check in
  `glyim-pipeline/src/lib.rs` is a good existing example of this pattern —
  make sure equivalent guards exist for macro expansion count and const-eval
  step count (`glyim-const-eval`), not just monomorphization set size.

---

## 4. Developer experience

1. **`glyip` UX parity with cargo**: `--offline`, `glyip tree` (dependency
   graph), colored/human-readable diagnostics by default with `--message-format
   json` for tooling, and a `glyip.lock` diff-friendly format (check
   `lockfile.rs` serializes keys in a stable/sorted order — unsorted map
   serialization is a classic "every lockfile regen is a huge diff" bug).
2. **Incremental build correctness**, not just speed: pair the fingerprint
   fast-path fix (§1.5) with a `glyip build --explain <target>` command that
   prints *why* a target rebuilt (which input's fingerprint changed) — this
   is disproportionately valuable for user trust in incremental builds and
   is cheap once fingerprints are already tracked per-file.
3. **Compiler UX**: `--explain <ERROR_CODE>` (rustc-style long-form
   explanations), suggested fixes as structured `Diagnostic` spans (check
   `glyim-lsp/src/code_action.rs` — if code actions already exist there for
   the LSP, surface the same suggestions in the plain CLI output).
4. **LSP polish**: confirm incremental re-analysis on keystroke (§2.6),
   and add `textDocument/inlayHint` for inferred types (`check_expr.rs`
   already computes exactly this information during typeck — it's a
   surfacing problem, not a new-analysis problem).
5. **Docs**: `glyim-core`, `glyim-type`, and `glyim-mir` already have strong
   module-level doc comments (the `type_arena.rs` header is genuinely
   excellent — keep that habit) — extend the same standard to
   `glyim-solve`/`glyim-hrtb` (trait solving is the hardest part of any
   compiler for new contributors to onboard onto, and is currently the
   least-commented major subsystem based on the files reviewed).
6. **CONTRIBUTING.md + architecture diagram**: a one-page "how a `.g` file
   flows through these 30 crates" doc pays for itself the first time a new
   contributor tries to find where to add a feature.

---

## 5. Testing & CI gates

### 5.1 What's already good
`glyim-test` is a real, dedicated test-infrastructure crate: snapshot tests
(CST snapshots present), a property-testing module (`arbitrary`, `unify`
checks), fixture builders, and mock contexts per subsystem
(`mock/borrowck_ctx.rs`, `mock/solver.rs`, …). This is more test
infrastructure than most production compilers start with — the plan below
is about *closing gaps*, not building from scratch.

### 5.2 Gaps to close
- **Miri** for `glyim-type`, `glyim-borrowck`, `glyim-solve` (pure-Rust,
  no FFI) in CI — catches exactly the class of bug in §1.1.
- **`cargo fuzz`** targets for: the lexer/parser (`glyim-frontend`), the
  macro expander (`glyim-meta`), and the runtime FFI boundary (§3.2.2).
  These are the three places untrusted bytes enter the system.
- **Compile-time regression tracking**: a small corpus of representative `.g`
  programs, timed on every CI run, with a regression threshold — this is
  what makes the performance work in §2 durable instead of "fixed once,
  regressed silently six months later."
- **`cargo +nightly udeps`** and a dead-code triage pass to work down the 230
  `#[allow(dead_code)]` sites (§1.7) — track as a burndown, not a blocker.
- **End-to-end pipeline tests** that go all the way from source text through
  the LLVM backend to a linked, *executed* binary with an asserted exit
  code/stdout (spot-check whether `glyim-test/src/harness` already does this
  via `interpreter_runner.rs`/`executor.rs` for the bytecode VM path, and
  make sure the LLVM path has equivalent end-to-end coverage, not just
  MIR-level snapshot assertions).

---

## 6. Suggested sequencing

| Phase | Work | Why this order |
|---|---|---|
| **0 — Stop the bleeding** | §1.1 (arena soundness), §1.4 (`kill` portability), §1.3 (delete dead parallel work) | Soundness bug and a correctness bug shipping today; cheap, isolated fixes |
| **1 — Foundations for perf** | §2.1 (interner fix, same file as 1.1), §2.2 (`Arc<Body>` audit), §1.5 (fingerprint fast path), §1.6 (reactor waker) | Everything in Phase 2 (parallel codegen) is unsafe to attempt until this lands |
| **2 — Scale out** | §2.3 (worklist liveness confirm/fix), §2.4 (real parallel codegen), §2.5 (release profile) | Now safe to parallelize because §1.1/§2.1 made shared reads sound and de-dup O(1) |
| **3 — Harden for real users** | §3.1 (panic→diagnostic sweep, ratcheted), §3.2 (unsafe audit + Miri + fuzz in CI), §3.6 (recursion/expansion limits) | This is what "production grade" actually means: it doesn't crash on bad/adversarial input |
| **4 — DX polish** | §4 (glyip UX, `--explain`, LSP inlay hints), §3.3 (FFI macro cleanup), docs pass | Compounding, ongoing; do alongside 2/3 rather than strictly after |
| **Ongoing** | §5 (CI gates: Miri, fuzz, perf corpus, dead-code burndown) | Once in place, these gates *are* the production-grade guarantee going forward |

---

### Bottom line
The architecture is sound and the team clearly already knows the hard parts
of compiler engineering (NLL-style borrowck, canonical type interning,
codegen-unit partitioning are not beginner moves). The gap to production
grade is concentrated in a small number of places — one real soundness bug
that doubles as the top performance fix (§1.1/§2.1), a build tool that does
more I/O than it needs to (§1.5), and a codebase-wide habit of `unwrap()` on
user input that needs to become `Result`/`Diagnostic` before this can be
handed to people who don't already know where the sharp edges are (§3.1).
None of this requires new architecture — it requires finishing the
architecture that's already there.
