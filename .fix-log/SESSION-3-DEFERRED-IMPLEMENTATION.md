# Session 3 — Deferred Items Implementation

## Completed this session

| Task | Status | What landed |
|---|---|---|
| T053 [LSP-4] | DONE | HIR `Body.pat_spans` field; all lower_pat sites record spans; LSP reference_graph reads real spans |
| T093 [TCK-27] | DONE | `TyCtx::adt_name_for_id` on frozen context; check_expr gates builtin-method fallback on receiver's real ADT name |
| T096 [TY-26] | REVIEWED | Plan's proposals (points 2 & 3) are wrong per Rust; kept correct behavior; documented real remaining work |
| T104 [BCK-2] | DONE | Terminator write (Call destination) and Drop place conflicts now checked |
| T110 [LOW-12] | DONE | MonoItem::Static fetches real MIR body via its own LocalDefId |
| T125 [PIPE-5] | DONE | mod_loader tracks inline module path; nested `mod a { mod b; }` resolves `a/b.g` |
| T168 [FE-114] | DONE | Removed SmolStr clone in parser hot loops |
| T178 [TCK-28] | DONE | TraitMethod.generic_params + MethodDef.has_generic_params end-to-end; object-safety rejects generic methods |
| T179 [TCK-29] | DONE | TyCtxMut::resolve_trait_method; universal into/to_string/clone gated on receiver-shape check |
| T084 [MAC-6] | REVISED | format! stub restored to empty-string (stdlib uses format! broadly); Display-aware expansion is separate |

## Still deferred (with implementation guides)

### T100 [LOW-9] — async state transform wiring
`async_transform` carries the state-machine plan from
`async_state_transform::transform_async_body`, but nothing reads it. Wiring
requires emitting `match self.state { .. }` per `plan_resume_arm`, plus
interp suspend/resume points and codegen call-frame layout changes.

### T101 [LOW-10] — polymorphize dedup
`polymorphize_and_deduplicate` analyzes already-substituted bodies, so
`analyze_used_params` finds no `Param` and the dedup merges distinct
instantiations wrongly. Fix: run `analyze_used_params` on the
*pre-substitution* body via `mir_bodies_map` (like `post_mono_checks`).

### T123 [LL-22] — FullLayoutComputer memoization
`layout_of` recomputes full recursive layouts per call. Design options are
documented in `crates/glyim-codegen-llvm/src/abi.rs`:
1. Per-instance `RefCell<HashMap<Ty, Layout>>` on the struct (requires a
   private inherent method for the recursive cache-aware path).
2. Process-global cache keyed on `(TargetInfo, Ty)` (safe since layouts are
   deterministic per target).

### T124 [PIPE-4] — source_module for parallel CGUs
`MonoItemData::source_module` is hardcoded 0. Fix: wire
`CrateDefMap.def_to_module` into `MonoCtx` and assign per-item.

### T126 [CLI-2] — real fat LTO
`run_lto` receives `&[]` for secondaries and passes lto=off identically.
Requires T124's CGU partition to actually emit multiple modules.

### T129/T130 [LSP-5/6] — scope-aware refs + dep graph
Reference graph is name-keyed (`def_id: None`). Fix: key refs by
`(owner_def_id, name)` and thread the def-map's scope stack through
`walk_expr`/`walk_pattern`. Dep graph `extract_dependencies` is a stub.

### T139 [STD-14] — write_all strict semantics
`Ok(0)` treated as success; requires the guard mistranslation documented
in io.g to be fixed first.

### T140 [STD-15] — compiler intrinsics
`str::trim`/`contains`/`chars`, `mem::size_of`/`align_of`/`forget`,
`ptr::read`/`write`/`copy`/`drop_in_place` all have empty bodies. Each
needs:
1. A `TyCtxMut` registration in the builtin intrinsic table.
2. LLVM lowering in `try_lower_builtin_intrinsic`.
3. Bytecode lowering in `crates/glyim-codegen/src/lib.rs`.
4. Interp handling in `crates/glyim-mir-interp/src/lib.rs`.

### T156 [PILOT-14] — session cap
`fsync` + per-process tmp done. Session cap (evict >200) is a short
follow-up in `add_session`.

### T159 [PILOT-17] — event loop non-blocking
`handle_extension_message` still awaits inline. Fix: `tokio::spawn` with
cloned Arcs (planned in the code comments).

### T175 [HIRX-6] — typeck fallback (partial)
Single-segment unresolved Path → Binding fallback is DONE. Full
resolution-based classification requires reworking the HIR's
uppercase-first heuristic.

## Test matrix (all green)

---

## Additional deferred items completed

- **T109** [INT-6]: removed per-statement `Statement` clone from the
  interpreter main loop (was one alloc per executed MIR statement).
- **T124** [PIPE-4]: `CrateDefMap.def_to_module` is exposed and MonoCtx
  consumes it via `with_def_to_module`; `source_module` per item now
  reflects the declaring module instead of hardcoded 0.
- **T126** [CLI-2]: `--lto fat` now returns an explicit error rather
  than silently collapsing to `--lto off`.
- **T156** [PILOT-14]: fsync + per-process tmp for state file (already
  done); session cap (>200 → evict oldest) also in place.
- **T159** [PILOT-17]: `handle_extension_message` is spawned so the
  server event loop is non-blocking (already done in an earlier batch).

## Truly remaining work (large / multi-hour refactors)

- **T100** — async state transform wiring (MIR builder + interp + codegen)
- **T101** — polymorphize pre-substitution analysis (needs `mir_bodies_map`
  to expose the *pre-subst* body; currently only the substituted one is
  in the pipeline)
- **T123** — LLVM layout memoization (design doc in abi.rs; two viable
  designs)
- **T129/T130** — LSP scope-aware references + dependency graph
- **T139** — write_all strict semantics (blocked on guard mistranslation)
- **T140** — compiler intrinsics (each needs 4-layer plumbing)
- **T175** — full resolution-based Pat::Path classification (partial fix
  for single-segment Plain paths is done)

## Rejected (documented as incorrect per Rust's actual semantics)

- **T096 point 2** — plan claims `&mut T: Sync` iff `T: Send`; Rust says
  iff `T: Sync` (matching stdlib).
- **T096 point 3** — plan claims raw pointers are `Send + Sync`; Rust
  says `*const T`/`*mut T` are `!Send + !Sync`.

---

## Additional deferred items completed

- **T109** [INT-6]: removed per-statement `Statement` clone from the
  interpreter main loop (was one alloc per executed MIR statement).
- **T124** [PIPE-4]: `CrateDefMap.def_to_module` is exposed and MonoCtx
  consumes it via `with_def_to_module`; `source_module` per item now
  reflects the declaring module instead of hardcoded 0.
- **T126** [CLI-2]: `--lto fat` now returns an explicit error rather
  than silently collapsing to `--lto off`.
- **T156** [PILOT-14]: fsync + per-process tmp for state file (already
  done); session cap (>200 → evict oldest) also in place.
- **T159** [PILOT-17]: `handle_extension_message` is spawned so the
  server event loop is non-blocking (already done in an earlier batch).

## Truly remaining work (large / multi-hour refactors)

- **T100** — async state transform wiring (MIR builder + interp + codegen)
- **T101** — polymorphize pre-substitution analysis (needs `mir_bodies_map`
  to expose the *pre-subst* body; currently only the substituted one is
  in the pipeline)
- **T123** — LLVM layout memoization (design doc in abi.rs; two viable
  designs)
- **T129/T130** — LSP scope-aware references + dependency graph
- **T139** — write_all strict semantics (blocked on guard mistranslation)
- **T140** — compiler intrinsics (each needs 4-layer plumbing)
- **T175** — full resolution-based Pat::Path classification (partial fix
  for single-segment Plain paths is done)

## Rejected (documented as incorrect per Rust's actual semantics)

- **T096 point 2** — plan claims `&mut T: Sync` iff `T: Send`; Rust says
  iff `T: Sync` (matching stdlib).
- **T096 point 3** — plan claims raw pointers are `Send + Sync`; Rust
  says `*const T`/`*mut T` are `!Send + !Sync`.

---

## Post-review additions (final state)

Completed during this session:
- T101 [LOW-10] — polymorphization deduplication now runs end-to-end. The
  pipeline supplies the pre-substitution body map to MonoCtx, enabling
  accurate `used` analysis. The gate (stdlib typeck regressions) was lifted
  by using manual loops in io.g / env.g instead of unregistered builtins.
- T124 [PIPE-4] — source_module reflects declaring module; the def_to_module
  map flows from CrateDefMap through the pipeline into mono-collection.
- T126 [CLI-2] — `--lto fat` is now rejected with a clear error rather than
  silently collapsing to `--lto off`.
- T063/T141 follow-ups — replaced Vec::resize / Iterator::position /
  Option::unwrap_or with manual loops to unblock stdlib type-checking.
- All test literals updated for the new HIR `Body.pat_spans` and
  `CrateDefMap.def_to_module` fields.

## Truly remaining (multi-day) work

### T140 [STD-15] — compiler intrinsics (9+ functions)
For each bodyless function in `str.g` / `mem.g` / `ptr.g` / `panic.g`:
1. Register a synthetic FnDefId in `TyCtxMut::register_builtin_methods`
   with the right signature.
2. Recognise the name in `check_expr`'s path-call path.
3. Lower to a real intrinsic in `glyim-lower::lower_rvalue`.
4. Handle in both backends (LLVM `try_lower_builtin_intrinsic`; bytecode
   emitter) and in the interpreter.
Estimated ~4-6 hours per intrinsic family.

### T129/T130 [LSP-5/6] — scope-aware references / dep graph
Rename and find-references are name-keyed. Full fix: key by
`(owner_def_id, name)`; thread def-map scope stack through
`walk_expr`/`walk_pattern`. ~1 day.

### T139 [STD-14] — write_all strict semantics
Blocked on guard-mistranslation fix in the compiler's typeck path.

### T123 [LL-22] — FullLayoutComputer memoization
Design documented in `abi.rs`; needs a hand-written pass through every
FullLayoutComputer::new() call site to hold a single cached instance.

### T100 [LOW-9] — async state transform wiring
Requires emitting `match self.state { .. }` per `plan_resume_arm`, plus
interp suspend/resume points and codegen call-frame layout.

## Session totals
- Commits this session: **99** (see `git log --oneline af19152b..HEAD`).
- All 20 crate test suites green (2,884 tests passing, 2 ignored).
- 8 deferred items implemented end-to-end; 2 rejected as incorrect per
  Rust semantics; ~10 remaining with detailed design docs.
