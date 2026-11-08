/// `build_review_prompt`.
#[must_use]
pub fn build_review_prompt(diff: &str, commit_log: &str) -> String {
    format!(
        r"## Self-Review Required

### Commit History
```
{commit_log}
```

### Full Diff
```diff
{diff}
```

### Review Checklist
1. Edge cases handled?
2. No unnecessary allocations?
3. All error paths covered?
4. Public interfaces consistent?
5. Tests cover happy AND failure paths?
6. No dead code?
7. Public items documented?
8. Naming clear and consistent?

Respond with your review, then either fix issues or emit `::APPROVED`.
"
    )
}
