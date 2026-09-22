## TIER 3 — Build tool (`glyip`)

### 3.1 Non-transitive dependency resolution — `glyip/src/dep.rs`

**Current (`DependencyResolver::resolve`, line ~313, confirmed):** the
BFS queue (`visit_stack`) is seeded **only** from `config.all_dependencies()`
(the root project's direct deps) — the `// TODO: implement transitive
resolution` comment (appearing twice, verbatim, at line ~342-343 — dedupe
that while you're in there) is never acted on: after resolving each dep,
its own dependencies are never pushed onto the stack.

**Root cause detail (confirmed):** `IndexEntry` (the registry-index
metadata struct, line ~17) has **no `dependencies` field** — even if the
BFS tried to enqueue a resolved crate's deps, there's nowhere to read them
from for registry deps. And `resolve_path_dep` (line ~435) always returns
`LockedCrate { dependencies: BTreeMap::new(), .. }` — it reads the sub-
project's `GlyipToml` (to get its version) but throws away
`config.dependencies`/`config.dev_dependencies` instead of enqueuing them.

**Fix — three coordinated changes:**

1. **`IndexEntry` gains a `dependencies` field:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexEntry {
    pub name: String,
    pub versions: Vec<String>,
    #[serde(default)]
    pub checksums: HashMap<String, String>,
    #[serde(default)]
    pub dependencies: HashMap<String, Vec<IndexDependency>>, // NEW: version -> deps for that version
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexDependency {
    pub name: String,
    pub version_req: Option<String>,
}
```
   `#[serde(default)]` keeps this backward-compatible with existing
   `.json` index files on disk (they'll just deserialize with an empty
   map — no migration script needed).

2. **`resolve_registry_dep` returns the resolved crate's own deps** so the
   caller can enqueue them, instead of the caller having no way to find out.
   Change its return type from `GlyipResult<LockedCrate>` to
   `GlyipResult<(LockedCrate, Vec<(String, Option<String>)>)>` — the second
   element is `(dep_name, version_req)` pairs read from
   `entry.dependencies.get(&version)` (registry path) — for the local-index
   branch, look up `self.index.get(name).dependencies.get(&version)`
   the same way. Populate `LockedCrate.dependencies` (a `BTreeMap<String,
   String>` per `lockfile.rs` — confirm the exact field type there) from
   this same list instead of leaving it `BTreeMap::new()`.

3. **`resolve_path_dep` returns its sub-deps too** — same return-type
   change, reading straight from the already-parsed `config.dependencies`
   / `config.dev_dependencies` (it already parses this `GlyipToml`, it's
   just discarding the field).

4. **`resolve()`'s loop enqueues what it gets back:**
```rust
while let Some((name, version_req, path)) = visit_stack.pop_front() {
    // ...unchanged dedup/key logic...
    let (locked, sub_deps) = if let Some(p) = path {
        self.resolve_path_dep(&name, &abs_path)?
    } else {
        self.resolve_registry_dep(&name, version_req.as_deref())?
    };
    for (dep_name, dep_version_req) in sub_deps {
        // Path-relative sub-dependencies need the sub-crate's own directory
        // as the base for resolving *their* path deps; registry sub-deps
        // have no path. Track this alongside name/version in the queue
        // tuple — extend visit_stack's element type with an Option<PathBuf>
        // "resolved from" base dir if the enqueued dep is itself a path dep
        // declared relative to a non-root project (nested path deps).
        visit_stack.push_back((dep_name, dep_version_req, None));
    }
    lockfile.add_crate(locked);
}
```

5. **Diamond-dependency version reconciliation is explicitly out of scope
   for this fix** — the `visited: HashSet<String>` dedup keyed on
   `"{name}-{version}"` already means two different requested versions of
   the same crate resolve as two separate lockfile entries (this matches
   real Cargo's non-unification behavior for incompatible majors, and is
   fine); don't attempt semver-range unification as part of this item, it's
   a separate, larger feature.

**Verify:** three-crate fixture (`root` depends on `a`, `a` depends on `b`)
using a local `CrateIndex` seeded with `IndexEntry.dependencies` for `a`;
assert the resulting `Lockfile` contains `b` even though `root`'s
`Glyip.toml` never mentions it directly.

---

### 3.2 `cmd_test` never executes anything — `glyip/src/commands.rs` + `glyip/src/test_discovery.rs`

**Current (confirmed at line ~254-278):** `Ok(_) => passed += 1` on
*successful compilation* — no test function is ever run; a `#[test] fn
test_foo() { assert!(false); }` is reported "passed" today as long as it
compiles.

**Root cause:** `compile_source` builds an object file via the bytecode/LLVM
backend but nothing links or executes it — same missing link step as Tier
7.2 (`glyim-test`'s harness). Fix both from the same underlying capability
so they don't drift: a single "compile then link then run, per test
function" pipeline.

**Fix:**

1. **Give each discovered test its own callable entry point.** Right now
   `FileTestDiscovery::scan` (`test_discovery.rs`) text-matches `fn
   test_foo(` but `compile_source` compiles the *whole containing file* and
   has no way to invoke just `test_foo`. The cleanest production shape:
   compile the file once, then for each discovered test in it, generate a
   tiny synthetic `main` that calls that one test function and reports
   success/panic via process exit code — mirroring how Rust's own libtest
   harness works (each `#[test]` fn becomes its own subprocess entry via a
   generated harness `main`, not a call from the build tool's own process).
   Concretely: after compiling the module, for each `DiscoveredTest`,
   compile a tiny synthetic wrapper source string:
   ```rust
   // in commands.rs, near compile_source
   fn make_test_harness_source(module_path: &str, test_fn_name: &str) -> String {
       format!(
           "import \"{module_path}\";\n\nfn main() {{\n    {test_fn_name}();\n}}\n"
       )
   }
   ```
   (Check `glyim-frontend`'s actual import/module syntax before using
   literally `import "..."` — grep the `.g` stdlib files, e.g.
   `glyim-lang-std/lib/*.g`, for how they reference other modules, and use
   that exact syntax. If cross-file `import` isn't supported by the
   frontend yet, the simpler and still-correct alternative is: since
   `test_discovery.rs` already has line numbers, textually extract *just*
   that one function's source span plus the file's `use`/`import` header
   lines, and wrap only that in a synthetic single-file harness — more
   fragile but requires no new frontend feature.)

2. **Actually link and run it.** Add, next to `compile_source`, a
   `link_and_run(object_path, project_dir) -> GlyipResult<i32>` that calls
   `glyim_cli::linker::invoke_linker` (see Tier 7.2 for making that `pub`)
   to produce a real executable in a temp dir, then `std::process::Command`
   -runs it with a timeout (reuse whatever timeout/kill logic
   `glyim-test/src/harness/executor.rs`'s `ProgramRunner` already has —
   don't write a second implementation; if `ProgramRunner` lives in
   `glyim-test` and `glyip` can't depend on `glyim-test` (check: `glyip`'s
   `[dev-dependencies]` already includes `glyim-test`, but that's
   dev-only — for production `glyip test`, either promote `ProgramRunner`
   out of `glyim-test` into a small shared crate, e.g. a new
   `glyim-runtime-exec` helper module inside the already-existing
   `glyim-runtime` crate, or accept a duplicated ~30-line
   `Command::spawn` + timeout wrapper in `glyip` — the latter is fine for a
   first pass, leave a `// TODO(dedupe): shares logic with
   glyim-test::harness::executor::ProgramRunner` comment).

3. **Replace the pass/fail logic:**
```rust
match compile_source(project_dir, &discovered_test.file, &config, &build_opts, &mut cache) {
    Ok((object_path, _)) => {
        match link_and_run_test(&object_path, &discovered_test, project_dir) {
            Ok(0) => passed += 1,
            Ok(_nonzero) => failed += 1,       // test panicked / assertion failed
            Err(e) => { warn!("failed to run test '{}': {}", discovered_test.name, e); failed += 1; }
        }
    }
    Err(GlyimError::BuildFailed(_diags)) => failed += 1,
    Err(e) => { warn!(...); failed += 1; }
}
```

4. **`#[ignore]` support** (implied by the `ignored` counter already existing
   in `TestResult` but currently only ever incremented by the `--filter`
   path, never by an actual `#[ignore]` attribute): extend
   `FileTestDiscovery::scan`'s text-based attribute detection (same style as
   the existing `#[test]` detection at line ~40) to also recognize
   `#[ignore]` immediately preceding a test fn, store it on
   `DiscoveredTest.ignored: bool`, and skip execution (counting it as
   `ignored`, not `passed`) unless `opts` has a "run ignored" flag mirroring
   `cargo test -- --ignored`.

**Verify:** a fixture project with one passing test, one
`assert!(false)`-failing test, and one `#[ignore]`d test; `glyip test`
must report `1 passed, 1 failed, 1 ignored` — today it reports `3 passed`
(everything compiles) or worse if any of them fail to compile.

---

### 3.3 `HttpRegistryClient` feature-gated, no fallback message

This one is **already correctly implemented** (confirmed: full
`fetch_index`/`download_crate` with real `reqwest`/`flate2`/`tar` handling
behind `#[cfg(feature = "registry")]`). The report's complaint is only that
it's *optional* — that's a legitimate packaging decision (not every build
needs network deps), not a stub. The one real gap: when the feature is
disabled and a dependency isn't in the local index, `DependencyResolver`
should fail with a clear, actionable error rather than whatever generic
`DependencyNotFound` it produces today. Check `resolve_registry_dep`'s
final `Err(_) if self.registry_client.is_some()` guard — the `else`
implicit branch (`registry_client` is `None`) already correctly falls
through to `Err(e)` from `index.resolve_version`; just confirm that
error message mentions the `registry` feature being disabled as a possible
cause, e.g. wrap it:
```rust
Err(e) if self.registry_client.is_none() => Err(GlyipError::DependencyNotFound {
    name: name.to_string(),
    version: version_req.map(|v| format!("{v} (hint: no registry client configured — build glyip with `--features registry` or provide a local index)")),
}),
```
Small, mechanical, low priority — do this last in Tier 3.
