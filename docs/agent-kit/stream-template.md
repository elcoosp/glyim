# Stream {ID}: {NAME}

## Mission
Implement {SCOPE_SUMMARY} for the Glyim compiler.

## What You Own
{OWNED_CRATES_AND_MODULES}

## What You May NOT Modify
Any `pub` interface listed in `CONTRACTS_LOCKED.md` for crates you do NOT own.
Specifically, you MUST NOT change:
{LOCKED_INTERFACE_LIST}

## What You May Do Freely
- Add `pub(crate)` helper functions in crates you own
- Add new private modules in crates you own
- Add new `pub` items in NEW modules you create within crates you own
- Add dev-dependencies for testing

## Test Location (MANDATORY)

**ALL tests MUST be written as unit tests in `src/tests/` within the crate.**

NEVER use the `tests/` directory at the crate root (integration tests). Unit tests compile faster because they share the crate's private API.

### Setup Pattern

1. In `crates/{CRATE_NAME}/src/lib.rs`, add:
   ```rust
   #[cfg(test)]
   mod tests;
   ```

2. Create `crates/{CRATE_NAME}/src/tests/mod.rs` declaring submodules:
   ```rust
   mod module_a;
   mod module_b;
   ```

3. Each test file contains `#[test]` functions with full access to `pub(crate)` items.

4. Add `glyim-test` as a dev-dependency:
   ```toml
   [dev-dependencies]
   glyim-test = { workspace = true }
   ```

5. Read `docs/agent-kit/GLYIM_TEST_INSTRUCTIONS.md` for full API reference.

## Test Plan (Write These FIRST)
{TEST_CASES}

## Mocking Strategy
{MOCKING_INSTRUCTIONS}

## Implementation Priority
1. Create `src/tests/` module structure and wire up in `lib.rs`
2. Write ALL tests above — verify they compile with `cargo check -p {CRATE_NAME}`
3. Implement until all tests pass
4. Remove all `todo!()` and silent stubs from non-test code
5. Run `cargo clippy -p {CRATE_NAME}` — fix all warnings
6. Run `cargo fmt`

## Verification Commands
```bash
cargo test -p {CRATE_NAME}
cargo clippy -p {CRATE_NAME} -- -D warnings
cargo fmt --check -p {CRATE_NAME}
cargo check --workspace
```

## Definition of Done
- [ ] All test cases pass
- [ ] Tests are in `src/tests/` (NOT `tests/`)
- [ ] Zero `todo!()` in non-test code
- [ ] Zero clippy warnings
- [ ] `cargo fmt` applied
- [ ] `cargo check --workspace` succeeds
- [ ] PR created against `main` with branch `stream-{ID}/v0.1.0`

## Dependencies on Other Streams
{UPSTREAM_DEPENDENCIES}

## Downstream Consumers
{WHO_USES_YOUR_OUTPUT}
