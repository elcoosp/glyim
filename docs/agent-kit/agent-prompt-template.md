You are implementing Stream {ID}: {NAME} for the Glyim compiler.

## Instructions

1. Read your stream brief completely before writing any code.
2. You MUST follow the Test-Driven Development (TDD) workflow:
   a. Create the `src/tests/` module structure in your owned crate.
   b. Write ALL test cases from your brief in `src/tests/` — NOT in the `tests/` directory.
   c. Verify your tests compile with `cargo check -p <crate>`. They may fail at runtime until implementation is done.
3. Implement features until all tests pass.
4. Run ALL verification commands from your brief:
   ```bash
   cargo test -p <crate>
   cargo clippy -p <crate> -- -D warnings
   cargo fmt --check -p <crate>
   cargo check --workspace
   ```
5. **IMPORTANT – Worktree Usage (CORRECTED):**
   - Your first script MUST create a git worktree using `git worktree add --detach` (NOT `git worktree add ... main` directly, as this fails because main is already checked out).
   - Worktree location: `../glyim-worktrees/stream-{ID}/`
   - After creating the worktree, cd into it and create your branch `stream-{ID}/v0.1.0`
   - All subsequent scripts MUST cd into that worktree before executing any git or cargo commands.
   - See the `plan-to-cat-scripts` skill for the exact corrected structure.
6. Output: Provide complete modified files using the bash script format. Never truncate, never use placeholders, never omit lines.

## Output Skill: plan-to-cat-scripts (MANDATORY)

You MUST follow the plan-to-cat-scripts skill exactly. This is non-negotiable.

### Skill Summary
- Every message is exactly one fenced bash code block -- no other text, no explanations.
- First script: Set STREAM_ID, create worktree `../glyim-worktrees/stream-${STREAM_ID}/`, cd into it, create branch stream-${STREAM_ID}/v0.1.0 from main.
- Subsequent scripts: Set STREAM_ID, cd into worktree, assume branch already checked out.
- Fix scripts: Set STREAM_ID, cd into worktree, assume branch already checked out.
- File writes: Use heredoc with unique delimiters that do not appear in content.
- Patches: Trivial single-line use sed. Everything else use Python with temp files.
- No hash-comment lines: Every action logged with echo.
- No truncation: Set INCOMPLETE=true and continue in next message.
- Compile check: Runs at end, failure blocks commit but never halts script.
- Commits: Prefixed with `stream-${STREAM_ID}:`.
- Error logs: User pastes terminal output then you emit one surgical fix script.

### Critical Rules
- NEVER modify pub interfaces in crates you do not own.
- NEVER use todo!() in non-test code -- use tracing::warn!("STUB: reason").
- NEVER write integration tests in tests/ -- write unit tests in src/tests/.
- ALWAYS use glyim-test dev-dependency for test helpers, mocks, and assertions.
- ALWAYS use sentinels (Ty::ERROR, Ty::NEVER, etc.) instead of Ty::from_raw().
- ALWAYS use ctx.intern_substitution(vec![...]) instead of Substitution::from_raw().
- ALWAYS pass &mut TyCtxMut to InferenceTable methods.
- ALWAYS emit one fenced bash code block per message -- no other text.
- ALWAYS use echo for logging -- no hash-comment lines.
- ALWAYS write complete file content -- never truncate, never use placeholders.
- ALWAYS create the stream worktree and branch in the first script.
- ALWAYS prefix commit messages with `stream-{ID}:`.
- NEVER use `cat >` to overwrite src/tests/mod.rs — ALWAYS use the Python safe-append pattern from SKILL_PLAN_TO_CAT_SCRIPTS section 1.5. This file is shared across streams and overwriting it silently deletes other agents' test registrations.
- ALWAYS read tests/mod.rs before modifying it — only append new mod declarations, never remove existing ones.
- ALWAYS run `cargo test -p <crate>` (the full crate test suite), not just your own test module. Your changes must not break existing tests from other streams.
- ALWAYS check for orphaned test files before committing — any .rs file in src/tests/ not declared in mod.rs indicates a previous overwrite accident.
```

---

## File 5: `stream-template.md`

```markdown
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

1. In `crates/{CRATE_NAME}/src/lib.rs`, idempotently add the test module (do NOT overwrite the file):

   ```bash
   echo "Ensuring #[cfg(test)] mod tests; exists in lib.rs"
   python3 - "crates/{CRATE_NAME}/src/lib.rs" << 'LIB_MOD_PYEOF'
   import sys
   path = sys.argv[1]
   with open(path, 'r') as f:
       content = f.read()
   if 'mod tests' not in content:
       if not content.endswith('\n'):
           content += '\n'
       content += '\n#[cfg(test)]\nmod tests;\n'
       with open(path, 'w') as f:
           f.write(content)
       print("Added #[cfg(test)] mod tests; to lib.rs")
   else:
       print("mod tests already present in lib.rs, skipping")
   LIB_MOD_PYEOF
   ```

2. **Safe‑append** your test module declarations to `crates/{CRATE_NAME}/src/tests/mod.rs`. **NEVER overwrite this file** — it is shared across streams:

   ```bash
   echo "Safely appending modules to tests/mod.rs"
   python3 - "crates/{CRATE_NAME}/src/tests/mod.rs" << 'SAFE_APPEND_PYEOF'
   import sys
   path = sys.argv[1]
   new_mods = ["mod module_a;", "mod module_b;"]
   try:
       with open(path, 'r') as f:
           existing = f.read()
   except FileNotFoundError:
       existing = ""
   existing_lines = {line.strip() for line in existing.splitlines() if line.strip() and not line.strip().startswith('//')}
   with open(path, 'w') as f:
       if existing:
           f.write(existing)
           if not existing.endswith('\n'):
               f.write('\n')
       for mod_line in new_mods:
           if mod_line not in existing_lines:
               f.write(mod_line + '\n')
   SAFE_APPEND_PYEOF
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
