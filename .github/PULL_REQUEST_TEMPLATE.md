<!--
PR body checklist — reviewer-facing. Every section below is enforced by the
`check-templates` CI job (which also verifies this template references the
agent operating protocol in AGENTS.md) and by the `ledger-diff` regression
gate. A PR that cannot fill a section says so explicitly — empty sections
fail review, and "not applicable" is a fillable answer, a blank one is not.
The work loop and the universal definition of done live in AGENTS.md;
start there before writing the code this template describes.
-->

## What and why

<!-- One paragraph: which work item (WI) this advances, the user-visible or
     readiness-level effect, and the smallest summary of the approach. -->

## Ledger diff

<!-- Mandatory (plan M1.10 gate). Paste the output of
     `cargo run -p glyim-ledger -- diff <base-tree> .`:
     level changes, gap changes, evidence changes. Regressions need an
     unexpired `ledger/waivers.toml` waiver; anything else must show no
     regressions. -->

```text
(paste `ledger diff` output here)
```

## Perf delta

<!-- Mandatory (plan §7.1 step 7, M2.5). Paste the `perf-compare` summary, or
     state "not perf-relevant" with one sentence of justification. A
     deliberate regression >2% per benchmark (or >0.5% geo-mean) must be
     argued for, not hidden. -->

```text
(paste perf-compare output, or the not-perf-relevant justification)
```

## Bug-class record

<!-- Mandatory for bug-fix PRs (constitution C8): the bug class, root cause,
     and the defence type (type / constructor / validator / lint / oracle /
     generator / metamorphic) with its location. If the defence is deferred,
     link the prevention-debt item. For non-bug-fix PRs write "n/a". -->

- Class:
- Root cause:
- Defence (type/constructor/validator/lint/oracle/generator/metamorphic):

## Breaking-change checklist

<!-- Mandatory for breaking PRs (plan §7.2 rule 4, C10). Tick every line or
     write "not breaking" with the API surface you checked. -->

- [ ] Codemod script under `scripts/codemods/` (or "not needed" + why)
- [ ] All call sites migrated in the same PR
- [ ] `docs/migrations/NNNN-title.md` written
- [ ] Ledger diff shows the readiness effect of the break
- [ ] ADR linked if a decision was made

## Definition of done

<!-- The universal DoD lives in AGENTS.md §4. `just verify` runs the
     mechanical subset locally; confirm the gates below actually ran. -->

- [ ] `just verify` green (fmt, clippy -D warnings, tests, ledger check)
- [ ] No new entries in panic/unsafe/wildcard/HashMap ratchets
- [ ] Docs updated (including `--explain` output if a face changed)
