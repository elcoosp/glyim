# 4. Performance / Production Gaps

## 4.1 Mono-Cache Deduplication (Polymorphization) — Multi-CGU

**Depends on:** §1.1, §1.9 (codegen pipeline changes compound with this).

### Current state

`glyim-pipeline/src/mono_cache.rs::MonoCtx::polymorphize_and_deduplicate`
is real: it calls `glyim-lower`'s `polymorphize::deduplicate` over all
collected `MonoItem`s and rebuilds `self.items`/`self.cache` from the
deduplicated result. This is genuinely working polymorphization +
in-memory dedup. What's missing (matches the report precisely): everything
still funnels into **one** codegen unit (CGU) — there is no partitioning of
`self.items` into multiple CGUs that could be lowered/optimized in
parallel, and therefore no cross-CGU dedup cache is needed *yet* because
there's only ever one "CGU" today in practice.

### Target design

Partition mono items into N CGUs (a simple, proven strategy: hash-based
bucketing by `MonoItemId`/def-path, capped at a configurable
`--codegen-units` count, mirroring rustc's `-C codegen-units` semantics),
compile each CGU to its own LLVM `Module` in its own thread, and merge at
link time (this composes directly with §1.2's Fat/Thin LTO work — Fat LTO's
existing `link_in_module` merge step is *exactly* the mechanism needed to
combine multiple real CGU modules, not just LTO-requested merges).

### Step-by-step instructions

**Step 0.** `grep -n "fn collect\|MonoCtx::collect\|codegen_units\|--codegen-units" glyim-pipeline/src glyim-cli/src`
to find (a) where `MonoCtx::items` is currently handed, as one unit, to
`glyim-codegen-llvm`, and (b) whether a `--codegen-units` CLI flag already
exists (unused) or needs adding.

**Step 1. Add a partitioning function** in `glyim-pipeline/src/mono_cache.rs`:

```rust
impl MonoCtx {
    /// Partition collected, deduplicated mono items into `n` roughly
    /// equal-sized codegen units. Must be called AFTER
    /// `polymorphize_and_deduplicate` (partitioning duplicates would defeat
    /// the whole point of deduplication having already run).
    pub fn partition_into_cgus(&self, n: usize) -> Vec<Vec<MonoItemId>> {
        let n = n.max(1);
        let mut cgus: Vec<Vec<MonoItemId>> = vec![Vec::new(); n];
        // Stable, content-addressed bucketing (NOT simple round-robin by
        // insertion order) so that incremental recompiles route the same
        // item to the same CGU run-to-run whenever possible, keeping
        // per-CGU object-file caching (glyip's fingerprinting, §4.2)
        // effective across builds.
        for (idx, data) in self.items.iter_enumerated() {
            let bucket = stable_hash(&data.item) as usize % n;
            cgus[bucket].push(idx);
        }
        cgus
    }
}

fn stable_hash(item: &MonoItem) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    item.hash(&mut h); // requires MonoItem: Hash — check it already derives
                        // Hash (it's used as a HashMap key in `self.cache`
                        // per the existing code, so it already does).
    h.finish()
}
```

**Step 2. Add `--codegen-units` to `glyim-cli`** (default matching
`std::thread::available_parallelism()`, capped e.g. at 16, mirroring
rustc's default policy) and thread it to `MonoCtx::partition_into_cgus`.

**Step 3. Compile CGUs in parallel.** Use `std::thread::scope` (no new
dependency needed) or `rayon` if already a workspace dependency
(`grep -n "^rayon" Cargo.toml`):

```rust
// glyim-cli/src/lib.rs, wherever codegen is currently invoked once for all
// items — replace the single call with:
let cgus = mono_ctx.partition_into_cgus(codegen_units);
let modules: Vec<_> = std::thread::scope(|s| {
    let handles: Vec<_> = cgus
        .into_iter()
        .enumerate()
        .map(|(i, item_ids)| {
            let ctx = &ty_ctx; // shared, read-only after typeck — confirm
                                 // TyCtx is genuinely immutable/Send+Sync
                                 // post-freeze before relying on this; if
                                 // it isn't yet, that's a prerequisite fix,
                                 // not something to route around with
                                 // unsafe Send impls.
            s.spawn(move || {
                let backend = LlvmBackend::new(ctx, format!("cgu{i}"));
                backend.codegen_items(&item_ids)
            })
        })
        .collect();
    handles.into_iter().map(|h| h.join().unwrap()).collect()
});
```

**Step 4. Merge at link time.** Feed `modules` (now genuinely N separate
`Module`s instead of always-one) into the exact same Fat-LTO
`link_in_module` path from §1.2 when `codegen_units > 1` and the user
didn't separately request `--lto=thin` — i.e., multi-CGU-without-explicit-LTO
still needs *some* merge step to produce one linked binary from N object
files, which is just... N object files passed to the system linker
directly (no LLVM-level merge needed at all when not doing LTO — each CGU's
`Module` becomes its own `.o`, and the *native* linker (`cc`/`ld`) combines
them, exactly like a multi-file C project). Only Fat LTO needs the
in-process `link_in_module` step; plain multi-CGU compilation does not.
Confirm `glyim-cli`'s linker invocation (`glyim-cli/src/linker.rs`) already
accepts multiple object files (it must, if it links against any runtime
objects today per §1.9) — if so, Step 4 requires **no new code**, just
passing N objects instead of 1.

### Tests

```rust
#[test]
fn partition_into_cgus_is_a_true_partition() {
    // Every MonoItemId appears in exactly one bucket; union of all buckets
    // equals the full item set; no item is duplicated or dropped.
}

#[test]
fn partition_is_stable_across_calls_with_same_input() {
    // Calling partition_into_cgus(4) twice on the same MonoCtx produces
    // identical bucket assignments (validates the content-addressed
    // hashing claim, not insertion-order-dependent behavior).
}

#[test]
fn multi_cgu_build_produces_identical_binary_behavior_to_single_cgu() {
    // Compile a representative multi-function program with
    // --codegen-units=1 and --codegen-units=4, run both binaries, assert
    // identical output — the correctness contract that matters: CGU count
    // must never change observable behavior, only build parallelism.
}
```

### Acceptance criteria

- [ ] `--codegen-units=N` compiles in N parallel `Module`s instead of one.
- [ ] `N=1` behavior is byte-for-byte unchanged from before this change
      (regression guard).
- [ ] Partitioning is deterministic/content-addressed, not
      insertion-order-dependent (helps §4.2's per-CGU fingerprinting reuse
      object files across incremental builds, once that's wired — not
      required by this section, but don't foreclose it).
- [ ] Multi-CGU and single-CGU builds of the same program produce
      identical runtime behavior.

---

## 4.2 Incremental Compilation — Fingerprinting Compiler Flags

### Current state

`glyip/src/fingerprint.rs`'s `FileFingerprint` already stores `hash`,
`mtime`, and (per the struct's other fields, `size`) — real content-hash
based invalidation, not just mtime (good: mtime-only would be unreliable
across checkouts/CI cache restores). `has_any_changed` already extends
invalidation to "the project manifest / build scripts" (`config_files(dir)`)
per its own "plan §23.3" comment — so dependency/manifest changes already
invalidate the cache. The **actual** remaining gap, precisely as the report
states: `--target`, `--opt-level`, and other **CLI flags** are not part of
the fingerprint at all, so `glyim build --release` after `glyim build`
(debug) with unchanged sources incorrectly reuses stale incremental state.

### Step-by-step instructions

**Step 0.** `grep -n "struct FileFingerprint\|struct FingerprintStore\|fn has_changed\|fn update\b" glyip/src/fingerprint.rs`
to see the exact persisted format (per the earlier grep, it's a simple
`path=...\nhash=...\nmtime=...\nsize=...\n` text format per entry).

**Step 1. Add a single, separate "build config fingerprint"** alongside the
per-file fingerprints, rather than folding flags into every file's
fingerprint (much simpler: one config hash gates the *entire* incremental
store, not per-file):

```rust
// glyip/src/fingerprint.rs

/// The subset of build configuration that affects codegen output and
/// therefore must invalidate ALL cached incremental artifacts when changed
/// — even though none of it is a source *file* that `has_any_changed`
/// otherwise tracks (plan: closes the "changing compiler flags may not
/// trigger a rebuild" gap).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BuildConfigFingerprint {
    pub target: String,
    pub opt_level: u8,
    pub lto: String,          // "none" | "fat" | "thin"
    pub codegen_units: usize,
    pub debug_assertions: bool,
    pub panic_strategy: String, // "unwind" | "abort"
}

impl BuildConfigFingerprint {
    pub fn hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.hash(&mut h);
        h.finish()
    }
}
```

**Step 2. Persist and check it** as one extra file next to the existing
per-file fingerprint store (`grep -n "fn store_path\|fn load\|fn save"
glyip/src/fingerprint.rs` for the existing store's file-location
convention, and match it — e.g. `.glyim/fingerprints/config.txt`):

```rust
impl FingerprintStore {
    /// Returns true if the build config differs from what's on disk (or
    /// nothing is on disk yet — first build is always "changed"). This
    /// must be checked in addition to, not instead of, `has_any_changed`.
    pub fn config_has_changed(&self, dir: &Path, config: &BuildConfigFingerprint) -> GlyipResult<bool> {
        let path = config_fingerprint_path(dir);
        if !path.exists() {
            return Ok(true);
        }
        let stored: u64 = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        Ok(stored != config.hash())
    }

    pub fn update_config(&self, dir: &Path, config: &BuildConfigFingerprint) -> GlyipResult<()> {
        let path = config_fingerprint_path(dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, config.hash().to_string())?;
        Ok(())
    }
}

fn config_fingerprint_path(dir: &Path) -> PathBuf {
    dir.join(".glyim").join("fingerprints").join("config.txt")
}
```

**Step 3. Wire it into the incremental-decision call site.** Find where
`has_any_changed` currently gates a rebuild decision (`grep -rn
"has_any_changed" glyip/src`) and require **both**:

```rust
let sources_changed = fingerprints.has_any_changed(&src_dir, "g")?;
let config = BuildConfigFingerprint {
    target: cli_opts.target.clone(),
    opt_level: cli_opts.opt_level,
    lto: format!("{:?}", cli_opts.lto),
    codegen_units: cli_opts.codegen_units,
    debug_assertions: cli_opts.debug_assertions,
    panic_strategy: cli_opts.panic_strategy.clone(),
};
let config_changed = fingerprints.config_has_changed(&src_dir, &config)?;
if sources_changed || config_changed {
    // full rebuild
    fingerprints.update_all(&src_dir, "g")?;
    fingerprints.update_config(&src_dir, &config)?;
} else {
    // reuse cached artifacts
}
```

### Tests

```rust
#[test]
fn opt_level_change_alone_triggers_rebuild() {
    let dir = tempdir_with_unchanged_sources();
    let store = FingerprintStore::load_or_new(&dir).unwrap();
    let debug_cfg = BuildConfigFingerprint { opt_level: 0, ..default_cfg() };
    store.update_config(&dir, &debug_cfg).unwrap();
    let release_cfg = BuildConfigFingerprint { opt_level: 3, ..default_cfg() };
    assert!(store.config_has_changed(&dir, &release_cfg).unwrap());
}

#[test]
fn identical_config_does_not_trigger_rebuild() {
    let dir = tempdir_with_unchanged_sources();
    let store = FingerprintStore::load_or_new(&dir).unwrap();
    let cfg = default_cfg();
    store.update_config(&dir, &cfg).unwrap();
    assert!(!store.config_has_changed(&dir, &cfg).unwrap());
}

#[test]
fn first_build_with_no_stored_config_is_treated_as_changed() {
    let dir = tempdir_with_unchanged_sources(); // no .glyim/ dir yet
    let store = FingerprintStore::load_or_new(&dir).unwrap();
    assert!(store.config_has_changed(&dir, &default_cfg()).unwrap());
}
```

### Acceptance criteria

- [ ] Changing `--opt-level`, `--target`, `--lto`, `--codegen-units`,
      `--panic` with unchanged sources now correctly triggers a full
      rebuild.
- [ ] Unchanged config + unchanged sources still uses the incremental
      fast-path (no regression to the common case's speed).
- [ ] Config fingerprint file lives alongside the existing per-file store
      under the same `.glyim/` convention.

---

## 4.3 Dependency Resolution — SemVer Conflict Detection / Resolver

### Current state

`glyip/src/dep.rs::check_version_conflicts` is real and correct **as a
conflict detector**: it collects every version requirement string per
crate name across the graph, and for crates with 2+ *distinct* requirement
strings, checks whether the version actually recorded in the `Lockfile`
satisfies **all** of them (via the `semver` crate's `Version`/`VersionReq`
— confirmed dependency use). This detection logic does not need changing.
The report's real complaint is about **resolution**, not detection: when
the lockfile is missing an entry for some dependency, the resolver "falls
back to resolving a new version, which may not be deterministic," and more
generally there's no real constraint solver — it can fail on complex
graphs where a valid assignment exists but isn't found by whatever simple
"pick latest compatible" strategy is used today.

### Step-by-step instructions

**Step 0.** `grep -n "fn resolve\|fn pick_version\|latest.*compatible" glyip/src/dep.rs`
to find the actual version-selection code path (distinct from
`check_version_conflicts`, which only *validates* an already-chosen
lockfile, per Step 0's re-read above).

**Step 1. Make "pick latest compatible" deterministic first** (the
narrower, higher-value fix — a full SAT/PubGrub-style solver is a large
follow-on project; determinism is a correctness bug fixable now).
Whatever "resolve a new version" code path exists, ensure it:
(a) sorts all candidate versions for a crate **deterministically**
(descending semver order, with a defined tie-break for otherwise-equal
versions, e.g. build-metadata or registry insertion order — `semver::Version`
already implements `Ord` correctly per semver precedence rules, so simply
`candidates.sort_by(|a, b| b.cmp(a))` before picking `candidates[0]` is
enough if it isn't already doing this), and (b) is a **pure function of
its inputs** (the full requirement set + the available version list) with
no dependency on `HashMap` iteration order — audit for any `HashMap`
iteration in the resolution path (`grep -n "HashMap" glyip/src/dep.rs`) and
replace with `BTreeMap`/sorted-`Vec` iteration wherever the iteration order
could affect which version gets picked.

```rust
// Before (illustrative of the risk, adapt to the real code found in Step 0):
for (name, candidates) in unresolved.iter() { /* HashMap iteration order! */ }

// After:
let mut names: Vec<_> = unresolved.keys().collect();
names.sort(); // deterministic crate-name order
for name in names {
    let mut candidates = unresolved[name].clone();
    candidates.sort_by(|a, b| b.cmp(a)); // deterministic version order, highest first
    let picked = candidates
        .into_iter()
        .find(|v| requirements_for(name).iter().all(|r| r.matches(v)));
    // ...
}
```

**Step 2. Add a resolution-failure diagnostic that explains *why*,** not
just that it failed — for complex graphs the simple greedy strategy can
fail even when a valid assignment exists (the "may fail on complex graphs"
caveat); when that happens, the error should say which two requirements
are irreconcilable given the versions actually available, not just "no
compatible version found":

```rust
GlyipError::DependencyConflict {
    crate_name: name.clone(),
    // list every requirement string AND which requester (parent crate)
    // introduced it, so the user can see the actual conflicting edges in
    // the dependency graph instead of just the crate name — thread the
    // "who requires this" info through `collected_reqs` if it isn't
    // already carried (check its type: `HashMap<String, Vec<String>>` per
    // the earlier grep has NO requester info today — extend it to
    // `HashMap<String, Vec<(String /* requester */, String /* req */)>>`).
    requirements: reqs_with_requesters,
}
```

**Step 3. Document the real remaining gap honestly.** A full SAT/PubGrub
resolver (true backtracking search over the whole dependency graph,
matching what Cargo's actual resolver does) is a substantial, separately
scoped project — do not attempt it inside this fix. Update
`KNOWN_GAPS.md` with a precise description: "greedy highest-compatible
resolution, now deterministic; does not backtrack across sibling
dependency choices, so some satisfiable graphs may still report a false
conflict. Full backtracking resolver tracked separately." This mirrors the
codebase's own established pattern (see ThinLTO, SEH) of shipping a
correct, narrower capability with an honest, explicit boundary rather than
a silently-incomplete "it usually works."

### Tests

```rust
#[test]
fn resolution_is_deterministic_across_runs() {
    let deps = sample_dependency_graph_with_multiple_candidates();
    let a = resolve(deps.clone()).unwrap();
    let b = resolve(deps).unwrap();
    assert_eq!(a, b); // same lockfile, every time, same input
}

#[test]
fn conflict_error_names_both_requesters() {
    // crate A requires foo = "1.0", crate B requires foo = "2.0" — assert
    // the error message/struct identifies BOTH A and B, not just "foo".
}

#[test]
fn missing_lockfile_entry_resolves_to_highest_compatible_deterministically() {
    let deps = graph_with_one_missing_lockfile_entry();
    let resolved_a = resolve(deps.clone()).unwrap();
    let resolved_b = resolve(deps).unwrap();
    assert_eq!(resolved_a, resolved_b);
}
```

### Acceptance criteria

- [ ] Resolution of the same input graph always produces the same
      lockfile, run after run (no `HashMap`-iteration-order flakiness).
- [ ] Conflict errors name every requester, not just the crate name.
- [ ] `KNOWN_GAPS.md` explicitly documents the "no backtracking" boundary.

---

# 5. Missing Error Handling / Diagnostics

### 5.1 LSP "Add missing match arms" — `unimplemented!()` stub quality

`glyim-lsp/src/code_action.rs`'s missing-match-arm quick-fix
(`parse_missing_variants` + arm synthesis loop) is already a reasonable,
intentional match to rustc's own `unimplemented!()`-skeleton convention —
**no functional change needed**. Two small, low-risk polish items:

1. **Struct/tuple variant field skeletons.** Currently every arm is
   `VariantName => unimplemented!(),` regardless of whether the variant
   has fields that need pattern-binding syntax (`VariantName(a, b) =>
   unimplemented!(),` / `VariantName { x, y } => unimplemented!(),`).
   Without field bindings, a variant with fields produces a
   non-compiling stub (`Foo::Bar => ...` when `Bar` has associated data is
   a pattern arity error, not just an intentionally-unfinished body).
   `grep -n "fn parse_missing_variants"` and check whether variant *shape*
   (unit/tuple/struct + field count) is available from the diagnostic
   message being parsed — if the diagnostic only carries variant *names*,
   extend the diagnostic that produces `diag.message` (in
   `glyim-typeck/src/check_expr.rs`, §5.2 below) to also carry each
   variant's shape, then use it here to synthesize a *compiling* skeleton:
   `Foo::Bar(_, _) => unimplemented!(),`.
2. **Insert `todo!()` instead of `unimplemented!()`, matching Rust
   convention** where `todo!()` is idiomatic for "not yet implemented"
   scaffolding and `unimplemented!()` is idiomatic for "deliberately will
   never implement this variant" — this is a one-line string change with
   no functional risk; do it only if Glyim's own std-lib actually defines
   both macros with that convention (`grep -rn "macro_rules! todo\|macro_rules! unimplemented"`
   in the runtime/std crate) — otherwise leave as `unimplemented!()`.

```rust
// glyim-lsp/src/code_action.rs
for v in &variants {
    let pattern = match variant_shapes.get(v) {
        Some(VariantShape::Unit) | None => v.clone(),
        Some(VariantShape::Tuple(n)) => format!("{v}({})", vec!["_"; *n].join(", ")),
        Some(VariantShape::Struct(fields)) => format!("{v} {{ {} }}", fields.join(", ")),
    };
    arms.push_str(&format!("    {} => unimplemented!(),\n", pattern));
}
```

**Tests:**
```rust
#[test]
fn tuple_variant_gets_compiling_placeholder_pattern() {
    // enum E { A, B(i32, i32) } — non-exhaustive match missing both — assert
    // the generated arm for B is `B(_, _) => unimplemented!(),`, not `B =>
    // unimplemented!(),` (which wouldn't compile).
}
```

**Acceptance criteria:**
- [ ] Generated quick-fix arms always compile (correct arity/pattern shape
      per variant).
- [ ] Existing unit-variant behavior unchanged.

### 5.2 Non-exhaustive match diagnostic — no LSP-actionable payload

`glyim-typeck/src/check_expr.rs`'s non-exhaustive-match diagnostic lists
missing variant *names* in its message string, which `code_action.rs` then
**re-parses out of prose** (`parse_missing_variants(&diag.message)`) — a
fragile string-parsing coupling between two crates that should instead
share a typed payload.

**Step-by-step:**

1. `grep -n "non-exhaustive match: missing variants" glyim-typeck/src/check_expr.rs`
   to find the diagnostic construction site.
2. Check whether `GlyimDiagnostic` (from `glyim-diag`) supports structured,
   typed side-data alongside its message (`grep -n "struct GlyimDiagnostic"
   glyim-diag/src/lib.rs` — look for something like a `related_info: Vec<..>`
   or an `extra: Option<Box<dyn Any>>` field; if none exists, add one, e.g.
   `pub structured: Option<StructuredDiagnosticData>` with a
   `StructuredDiagnosticData::MissingMatchVariants(Vec<VariantInfo>)` variant
   where `VariantInfo { name: String, shape: VariantShape }`).
3. Populate it at the `check_expr.rs` construction site instead of (or in
   addition to, for backward-compat/other consumers) the prose message.
4. Change `code_action.rs`'s `parse_missing_variants` call to read the
   structured field when present, falling back to prose-parsing only for
   diagnostics that don't (yet) carry it — a safe, additive migration.

**Tests:** a round-trip test constructing the diagnostic in
`glyim-typeck`, feeding it through the LSP code-action pipeline, and
asserting the generated arms match the *typed* variant shapes rather than
a re-parsed string.

**Acceptance criteria:**
- [ ] `code_action.rs` no longer parses variant names out of a prose
      message string when structured data is available.
- [ ] §5.1's arity-correct skeletons become possible because variant shape
      is now available end-to-end.

### 5.3 `glyim-pipeline`'s early-return diagnostic accumulation

`glyim-pipeline/src/lib.rs`'s `sink_cell`/`has_errors`/early-return pattern
(verified real and consistently applied at every pipeline stage: def-map,
HIR, typeck, lower, borrowck, const-borrowck) is already correct and needs
**no change** — this item is confirmed fine per the report's own
assessment ("Seems fine"). No action item here beyond a regression test
confirming a failure at stage N genuinely skips stages N+1..end (cheap
insurance against a future refactor accidentally removing an early-return
check):

```rust
#[test]
fn typeck_failure_skips_lowering_and_borrowck() {
    // Compile a program with a real type error; assert the pipeline result
    // contains typeck diagnostics AND that lowering/borrowck never ran
    // (e.g. via a call-counting instrumentation hook, or simply asserting
    // no lower/borrowck-specific diagnostics are present even for a
    // program that WOULD also fail borrowck if it got that far).
}
```

---

# 6. Documentation and Visibility

### 6.1 `KNOWN_GAPS.md`

The report notes `KNOWN_GAPS.md` is referenced throughout the codebase's
own comments (`Plan §5`, `Plan §7.2`, `Plan §13.2`, `Plan §19.1`, `Plan
§23.3`, etc.) but was not included in the dump — it either exists in the
repo root and was simply not part of this export, or needs to be created.

**Step 1.** `ls KNOWN_GAPS.md` at the repo root. If present, read it and
reconcile every phase/section number referenced throughout the codebase
comments (`grep -rn "Plan §\|KNOWN_GAPS" --include=*.rs . | sed -E 's/.*(Plan §[0-9.]+|KNOWN_GAPS\.md[^"]*)/\1/' | sort -u`)
against its actual table of contents — file a note for any `Plan §N.M`
referenced in code with no corresponding entry in the doc (a doc/code drift
bug, cheap to fix, easy to miss).

**Step 2.** If absent, create it now, seeded from every `Plan §...`
reference found by the grep above, cross-referenced against this
implementation plan's own section numbers, e.g.:

```markdown
# KNOWN_GAPS.md

Tracks intentionally deferred or partially-implemented features, referenced
from source comments as `Plan §N.M`. Status: Open | In Progress | Closed.

## Phase 4-5: Const-eval and async
- §4.2 User-defined `const fn` support — Closed (glyim-const-eval BodyFn)
- §5 Async multi-poll state machine — In Progress (see IMPL_PLAN.md §1.1)
...

## Phase 7: Unwinding
- §7.2 Cross-frame unwinding (interpreter) — Closed, hardened (IMPL_PLAN.md §1.4)

## Phase 8-9: Trait solving
- §8.1 `Sized` structural check — Closed (IMPL_PLAN.md §2.5)
- §8/9.4 Projection normalization for auto-traits — Closed (IMPL_PLAN.md §2.7)

## Phase 10: Codegen units / LTO
- §10.2 Multi-CGU compilation — Closed (IMPL_PLAN.md §4.1)
- §10.2 ThinLTO — Closed (IMPL_PLAN.md §1.2)

## Phase 13: Casts
- §13.2 Const-eval / typeck cast-legality unification — Closed (IMPL_PLAN.md §1.7)
- §13.3 Uniform iteration desugaring — Closed (pre-existing)

## Phase 14: Interpreter
- §14.2 Single-frame cleanup — Closed (pre-existing)

## Phase 19: Codegen ABI
- §19.1 Personality/SEH funclets — <A: Closed | B: Open, tracked> (IMPL_PLAN.md §1.3)
- §19.2 Three-way personality selection — Closed (pre-existing)
- §19.4 Custom LLVM pass pipeline string — Closed (pre-existing)

## Phase 23: glyip build tool
- §23.2 SemVer conflict detection — Closed, deterministic resolver still greedy (IMPL_PLAN.md §4.3)
- §23.3 Fingerprint manifest invalidation — Closed; now also covers compiler flags (IMPL_PLAN.md §4.2)

## Untracked / newly identified in this plan
- §1.5 Windows proc-macro loading — Closed (IMPL_PLAN.md §1.5)
- §1.6 Dynamic range slicing — Closed (IMPL_PLAN.md §1.6)
- §1.8 Per-projection drop elaboration — Closed, field-level only (IMPL_PLAN.md §1.8)
- §1.9 Native exec entry point test — Closed (IMPL_PLAN.md §1.9)
- §2.1 Non-Unix getppid — Closed (IMPL_PLAN.md §2.2)
- §2.3 Windows path encoding — Closed (IMPL_PLAN.md §2.3)
- §2.4 Bytecode opt-level — Closed (IMPL_PLAN.md §2.4)
- §2.6 HRTB structural-eq recursion guard — Closed (IMPL_PLAN.md §2.6)
```

**Acceptance criteria:**
- [ ] `KNOWN_GAPS.md` exists, is accurate, and every `Plan §N.M` comment
      in source has a matching entry.
- [ ] Every item closed by this plan is marked "Closed" with a pointer back
      to this document's section, not left silently stale.

### 6.2 `#![allow(clippy::...)]` blanket suppressions

Several crates (confirmed in `glyim-mir-interp/src/lib.rs`'s crate-root
`#![allow(...)]` block, 20+ lint names) suppress clippy lints crate-wide.
This is acceptable for now per the report, but production polish should
convert broad crate-level allows into narrow, per-call-site allows with a
one-line justification, so future violations of *other* instances of the
same lint aren't silently masked. This is a mechanical, low-risk cleanup
task, lowest priority in this plan — do it last, one crate at a time, and
only after every functional item above is closed:

```bash
# For each crate with a blanket #![allow(...)]:
cargo clippy -p <crate> -- -W clippy::<lint_name>   # temporarily promote to warn
# For each surviving warning, either fix it or add a scoped
# #[allow(clippy::<lint_name>)] // <reason> directly above the offending
# line/item instead of the crate-wide allow.
```

Do this incrementally, crate by crate, each as its own small PR — do not
attempt all crates in one change.

---

# 7. Milestones

| Milestone | Sections | Exit criteria |
|---|---|---|
| M1 — Correctness hardening | 1.6, 1.7, 1.8, 1.4 | All new tests green; `cargo test --workspace` clean |
| M2 — Platform completeness | 1.5, 2.2, 2.3, 1.9 | Windows CI job added and green; native-exec test un-ignored |
| M3 — Unwinding & LTO | 1.3, 1.2 | SEH spike result documented; ThinLTO end-to-end test green |
| M4 — Async | 1.1 | Multi-poll state machine tests green; single-poll path unregressed |
| M5 — Trait system & codegen quality | 2.4, 2.5, 2.6, 2.7 | All new tests green |
| M6 — Build system scale-out | 4.1, 4.2, 4.3 | Multi-CGU build green; flag-invalidation tests green; deterministic resolver tests green |
| M7 — Diagnostics & docs polish | 5, 6, 2.1 | LSP quick-fix arity tests green; `KNOWN_GAPS.md` complete and accurate |

Each milestone should land as its own set of PRs (one per section within
it), gated by `cargo test --workspace` and (once added) the Windows CI job,
before the next milestone begins.

---

# 8. CI additions required by this plan

Add to the CI configuration (`.github/workflows/*.yml` — locate the
existing workflow file(s) first: `find . -path '*/workflows/*.yml'`):

```yaml
  windows-tests:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - name: Run workspace tests (Windows-relevant subset)
        run: cargo test -p glyim-proc-macro -p glyim-runtime -p glyip --workspace-exclude glyim-codegen-llvm
        # glyim-codegen-llvm's Windows SEH work (§1.3) needs its own gated
        # job once LLVM 22 + funclet support is confirmed available on the
        # Windows runner image — add a second job for it once §1.3's spike
        # (Step 1) concludes, rather than blocking this job on it now.

  thinlto-integration:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install LLVM 22 (incl. llvm-lto2)
        run: <match whatever LLVM install step the existing CI already uses>
      - name: ThinLTO end-to-end test
        run: cargo test -p glyim-cli thin_lto_end_to_end -- --ignored
```

---

# Appendix — Verified vs. report-claimed status summary

For transparency, here is where this plan's direct reading of the code
disagrees with the report's characterization (all report claims were
independently re-verified against the actual dump before writing this
plan):

| Item | Report says | Actually found |
|---|---|---|
| §1.4 Cross-frame unwinding | "out of scope... not supported" | Substantially implemented (`unwind_step` walks the call stack); this plan treats it as **hardening** (one real resume-target bug fixed) rather than new implementation. |
| §1.7 Const-eval cast legality | "uses a primitive allowlist... needs separate change" | `is_valid_cast` gate is already wired when `with_ty_ctx` is used; the real remaining gap is (a) ensuring every production call site uses `with_ty_ctx`, and (b) threading the precise THIR source type instead of reconstructing it from the runtime value. |
| §2.6 HRTB `can_coerce` | "workaround... may be incomplete for recursive types" | Confirmed accurate; this plan adds the missing depth guard. |
| §4.1 Multi-CGU | "codegen units are not fully parallelized" | Confirmed accurate; `polymorphize_and_deduplicate` itself is correct and unchanged by this plan — only true partitioning is added. |

All other items in this plan matched the report's description closely and
were implemented/planned as described.
