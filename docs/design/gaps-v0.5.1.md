## Remaining Gaps for Full Glyim v0.5.0 Spec Compliance

Based on cross‑referencing the current codebase against the v0.1.0–v0.5.0 architecture specifications:

---

### 🔴 Critical Gaps (Blocking v0.5.0 Release)

| Gap | Spec | What's Missing |
|-----|------|----------------|
| **General macro execution engine** | v0.3.0 §3.8, v0.5.0 §3.4 | Only `@identity` is hardcoded. No mechanism to load and execute user‑defined macros from packages. |
| **Reader macro support** | v0.1.0 ADR‑004 | `@macro_name(args)` is parsed but expansion always returns the identity. Need AST‑level macro interpretation or Wasm loading. |
| **`glyim publish --wasm`** | v0.5.0 §3.2.1 | CLI flag is parsed but not wired to `compile_and_store_macro_wasm` in the publish pipeline. |
| **Bazel REAPI gRPC service** | v0.5.0 §3.4.5 | CAS server has REST endpoints but no gRPC `FindMissingBlobs`, `BatchUpdateBlobs`, `GetTree`, or `ActionCache` as REAPI v2 requires. |
| **`glyim verify` implementation** | v0.5.0 §5.1 | Command exists but only reads lockfile; doesn't verify hashes against the registry or check signatures. |
| **`glyim macro inspect`** | v0.5.0 §3.8 | CLI command not built; no way to preview macro expansions in the terminal. |
| **Doc comments on nested items** | v0.5.0 §3.8.1 | Doc comments only attach to top‑level items (functions, structs, enums). Missing for `impl` methods, enum variants, struct fields. |
| **`glyim doc --open`** | v0.5.0 60‑second test | Flag not implemented; `glyim doc` generates HTML but cannot open the browser. |

---

### 🟡 Moderate Gaps

| Gap | Spec | What's Missing |
|-----|------|----------------|
| **Fuel metering** | v0.5.0 §5.4 | Disabled during development; no budget prevents infinite macro loops. |
| **Cross‑compilation sysroot download** | v0.5.0 §3.7.3 | `cross.rs` defines sysroot directories but there's no network fetch or "Download? [Y/n]" flow. |
| **Standard library Iterator trait** | v0.5.0 §3.3.1 | `stdlib/src/iterator.g` is a design doc; no trait exists. Concrete `Iter<T>` works but `for` loops can't use `Range` directly on types without `.iter()`. |
| **`glyim outdated`** | v0.5.0 §3.2.6 | CLI command exists but implementation is a stub that reads lockfile without actually checking the registry for newer versions. |
| **Package signing and verification** | v0.5.0 §5.1 | No signature verification on package download; lockfile pins to content hash but doesn't verify author signatures. |
| **`#[no_std]` attribute** | v0.5.0 §3.3.2 | Detected by pipeline (`detect_no_std`) but not exposed as a package‑level attribute in `glyim.toml`. |
| **Workspace support in `glyim test`** | v0.5.0 §3.6.4 | `glyim test --all` parses but doesn't discover tests across workspace members. |

---

### 🟢 Lower Priority / Polish

| Gap | Spec | What's Missing |
|-----|------|----------------|
| **`glyim build --bare`** | v0.5.0 §5.4 | Single‑file compilation without a project scaffold. |
| **`glyim cache clean`** | v0.5.0 §3.2.6 | Stub that prints "not yet implemented". |
| **`glyim lint`** | v0.5.0 C2 View | Crate exists but no lint rules are implemented. |
| **`glyim fmt`** | v0.5.0 C2 View | Crate listed in workspace but no formatting logic. |
| **Test `--nocapture` flag passthrough** | v0.5.0 §3.6.4 | `glyim test` doesn't support `-- --nocapture` to pass flags to the test binary. |
| **Clean up compiler warnings** | General | ~10 `unused_import`, `dead_code` warnings across multiple crates. |
| **`BlobHash` dead struct** | — | Unused struct in `glyim-cas-server/src/main.rs`. |
| **Doc generator: method signatures** | v0.5.0 §3.8.2 | `glyim-doc` renders "Function" for all functions; doesn't show full signatures with parameter types. |
| **Doc generator: code examples as separate pages** | v0.5.0 §3.8.3 | Code examples are rendered inline but not extracted to separate runnable test files. |

---

### 📊 Summary

| Priority | Count | Key Area |
|----------|-------|----------|
| Critical | 8 | Macro engine, REAPI, CLI commands, nested doc comments |
| Moderate | 7 | Fuel, cross‑compilation, stdlib, package signing |
| Low/Polish | 10 | Lint, fmt, `--bare`, warnings, doc rendering |

---

**The single biggest gap is the general macro execution engine.** Once user‑defined macros can be compiled to Wasm and executed, the entire v0.5.0 pipeline (publish → cache → remote share → instant build) becomes functional. Would you like me to write the implementation plan for that?
