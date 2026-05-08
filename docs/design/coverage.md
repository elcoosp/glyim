Yes, our hand‑rolled coverage pipeline opens the door to improvements that **profraw alone cannot deliver** because profraw is generic for LLVM-based languages and doesn’t understand Glyim’s HIR, macro expansion, or incremental compilation story. Here are concrete areas where we can surpass profraw for our custom language needs:

---

### 1. **Source‑to‑source macro‑aware coverage**

**Problem:** Profraw works on the final LLVM IR, which is after macro expansion, monomorphization, and desugaring. So if a user writes `@identity(1 + 2)`, profraw only sees `1 + 2` and attributes coverage to the expanded location, not the macro call site.

**Our advantage:** We can build a mapping from the HIR back to the original pre‑macro source. Our `CoverageInstrumenter` already records `SourceLocation` with line/col; we could additionally track macro expansion hierarchies and produce reports that highlight which macros were covered, or show coverage on the original source with macro calls inlined in the report.

---

### 2. **Didactic / learner‑focused coverage**

**Problem:** Profraw just says “this line wasn’t executed”. It doesn’t explain *why* – e.g., “this match arm was never taken because the scrutinee is always `Option::Some` due to the previous line”.

**Our advantage:** Because we own the HIR and type information, we could generate **suggestions** alongside uncovered lines. For example:
- “This `if` branch was never taken. The condition `x > 0` is always false because `x` is assigned `0` on line 3.”
- “This match arm is unreachable because the scrutinee is always `Ok` according to the inferred type.”

This would be a killer feature for an educational language like Glyim.

---

### 3. **Real‑time streaming coverage for the editor**

**Problem:** Profraw dumps coverage only at process exit. Live coverage during a debugging session or in a LSP scenario requires polling or complex server‑side integration.

**Our advantage:** We already have a `__glyim_cov_flush` that can be called at any time, not just at exit. We could add a **websocket endpoint** (or LSP notification) that sends incremental counter updates to the editor while the program is running, giving a live “heat map” of executed lines. Combined with our LSP, this could be a “coverage while you type” experience.

---

### 4. **Incremental coverage persistence**

**Problem:** Profraw files are monolithic and tied to a single binary. In an incremental compiler like ours, you don’t want to re‑compute coverage for unchanged functions across builds.

**Our advantage:** We already have per‑item hashing (semantic hashes, fingerprinting). We could persist coverage counters alongside the incremental cache (`.glyim/incremental`) and only reset counters for functions that were actually modified. This gives a **cumulative coverage across multiple builds**, useful in CI where you run tests after each commit and want to see a rolling coverage report.

---

### 5. **Function‑, module‑, and tier‑level granularity**

**Problem:** Profraw is all‑or‑nothing (you instrument the whole binary). You can’t easily instrument only hot functions after JIT tier promotion, or instrument only a specific module under a feature flag.

**Our advantage:** Our `CoverageMode` (Off, Function, Branch, Full) already allows fine‑grained control. We could extend this to:
- **Tier‑driven coverage:** Only start instrumenting after a function is promoted to a higher JIT tier.
- **Per‑module coverage:** `--coverage my_crate::specific_fn` to only instrument that function and its callees.
- **Coverage budgets:** Limit the number of counters so that coverage instrumentation has predictable overhead.

---

### 6. **HIR‑annotated coverage (desugaring‑aware)**

**Problem:** Profraw can’t show coverage of desugared constructs like `for x in iter` → `while` loop, or `?` operators → `match`. The user writes `for x in iter { … }` but the coverage report shows the desugared pattern, confusing beginners.

**Our advantage:** We can annotate the HIR with source spans that map each desugared piece back to its original syntactic form. Our coverage report could display the original source line for `for x in iter { … }` while still attributing the counter increments from the desugared `while` loop and `match`. This bridges the gap between what the user wrote and what the compiler generated.

---

### 7. **Lightweight, zero‑dependency runtime**

**Problem:** Profraw requires linking `libclang_rt.profile_*.a` into every binary. For a JIT‑based language like Glyim, that’s an extra dependency and may not even be available on all platforms.

**Our advantage:** Our current implementation is completely self‑contained – no external libraries needed. It works identically in JIT and AOT compilation without any extra linking step, making Glyim a breeze to deploy.

---

### Next steps

We’ve already built the foundation for all of these. The next practical improvements would be:

1.  **Macro‑aware source mapping** (record macro call sites in the instrumenter).
2.  **Incremental coverage persistence** (store counters per function hash).
3.  **While‑loop branch instrumentation** (the immediate task we were doing).

Shall we continue with the while‑loop branch instrumentation to finish the basic branch coverage, then tackle one of the above enhancements?
