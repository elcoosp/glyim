# Security Policy

## Reporting a Vulnerability

**Do not open a public issue for a security bug.**

Email the maintainer at the address listed on the GitHub profile
(`github.com/elcoosp`) with:

- A minimal reproduction (source file or PR that demonstrates the
  bug),
- The compiler version (`glyim --version`),
- A description of the impact (crash, arbitrary code execution, data
  disclosure, denial of service, etc.),
- Whether the bug affects a *compiled program* at runtime, or the
  *compiler* itself while processing untrusted input.

You will receive an acknowledgement within ~7 days and a plan for the
fix timeline.

## Scope

The following are **in scope** as security bugs:

- **Compiler-side memory safety** — a `#[forbid(unsafe_code)]`-style
  panic in the compiler driver while parsing, type-checking, lowering,
  or code-gen a hostile input. A crashed compiler with a controlled
  ICE is not a security issue; a compiler that reads out-of-bounds or
  executes arbitrary code is.
- **Silent miscompiles with security impact** — a program that
  compiles cleanly but produces wrong output in a way that a program
  author would reasonably not notice (e.g. a bounds check silently
  elided, a `drop` not run).
- **Runtime library issues** (`glyim-runtime`) — undefined behavior in
  a runtime primitive reachable from a safe program.
- **Supply chain** — a malicious crate accepted by `glyip` without
  checksum verification, or a registry compromise. (Checksum
  verification is implemented in `crates/glyip/src/dep.rs`'s
  `verify_checksum`; if you find a bypass, that is in scope.)
- **LSP / proc-macro isolation** — a malicious proc-macro crate
  escaping its intended privilege boundary (currently: proc macros
  run as arbitrary Rust, so the boundary is "the user opted in to
  compiling with a proc-macro dependency").

## Out of scope

- **Undefined behavior that Rust itself would also exhibit** —
  `unsafe` code the user wrote is the user's responsibility.
- **Denial of service via a pathological input file** — a file that
  takes N minutes to compile is a bug, but it is not a security
  vulnerability.
- **Missing language features** — the KNOWN_GAPS file documents
  dozens of incomplete features. A feature that rejects at compile
  time is not a security issue.
- **Frontend-only bugs** — a parser panic on adversarial input is a
  robustness bug (reportable as an ordinary issue), not a security
  issue, unless it leads to memory unsafety or code execution.

## Disclosure

We follow a 90-day coordinated disclosure window from acknowledgement,
or shorter if a fix ships sooner. Credit will be given to the reporter
unless requested otherwise.

## Hardening status

- **Compilation cache** (`crates/glyim-db/src/cache.rs`) — SHA-256
  content-addressed; no path traversal (keys are hex strings;
  `<root>/objects/<key>.o`).
- **glyip lockfile checksums** — implemented
  (`verify_checksum`); a missing checksum refuses the download rather
  than proceeding.
- **Proc macros** — execute as arbitrary Rust. Users must
  opt in to a proc-macro dependency explicitly
  (`--proc-macro-deps`); the isolation model is "the user asked for
  it".
- **LSP** — the LSP server runs in the same process as the editor's
  LSP client; a hostile workspace is not a security boundary (matches
  rust-analyzer's model). Report *crashes* as ordinary bugs.

