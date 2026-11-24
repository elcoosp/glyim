# Glyim Pilot Error Codes

| Code   | Category         | Description                                        |
|--------|------------------|----------------------------------------------------|
| E0100  | Protocol/Parse   | Generic protocol parse error                       |
| E0201  | Apply/Find       | FIND text not found in file                        |
| E0202  | Apply/Find       | FIND text found multiple times (expected exactly 1)|
| E0203  | Apply/File       | Target file not found                              |
| E0204  | Apply/I/O        | I/O error during file apply                        |
| E0205  | Apply/Task       | Spawned task join failure (panic or cancellation)  |
| E0206  | Apply/Rollback   | Apply failed and was rolled back                   |
| E0300  | Security         | Path escapes worktree root                         |
| E0400  | Git              | Git operation failed                               |
| E0500  | Gate/Infra       | Gate infrastructure failure (tool missing, timeout) |
| E0600  | Config           | Configuration error                                |
| E0700  | Session          | Session state error                                |
| E0800  | I/O              | General I/O error                                  |
| E0900  | Limits           | Apply limits exceeded                              |
| E1000  | Queue            | Ledger task queue could not answer "what is next"  |

## Guidelines

- **E05xx** (Gate infrastructure) vs semantic gate failure: `Err(E05xx)`
  means the gate could not run at all. `Ok(GateResult { passed: false })`
  means the gate ran but found violations. Never conflate these.

- **E02xx** (Apply) errors are recoverable via rollback. E0206 specifically
  indicates a partial apply was detected and all changes were reverted.

- **E03xx** (Security) errors indicate path traversal attempts and should
  be logged at WARN level for security auditing.
