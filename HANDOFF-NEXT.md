# Handoff — `glyim-v2` stdlib hello-world (updated)

## Status

**Object path (`--emit=obj`): fully working.** `glyim-cli --with-stdlib
--emit=obj` on `fn main() { println("hello"); }` produces a valid 2112-byte
Mach-O arm64 relocatable object. **All 4172/4172 workspace tests pass.**

**Executable path (`--emit=exec`): produces a runnable binary, but it
segfaults at runtime before printing `hello`.** The link now succeeds and the
runtime hooks resolve (`_glyim_stdout_write` / `_glyim_errno` are defined
symbols in the binary), so the ABI-dispatch path reaches `glyim_stdout_write`.
The segfault happens inside that call, which points at the *arguments*
(buf/len/fd) being wrong — i.e. an ABI/codegen issue on the caller side.

## Commits landed this session
