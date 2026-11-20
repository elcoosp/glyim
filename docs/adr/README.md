# Architecture Decision Records

Index of the glyim ADRs. Every record below was created as **Proposed** in M0.4 from the master plan's decision register (`docs/roadmaps/glyim-master-plan.md` §8) and must be *accepted with evidence* before its dependent work starts (plan §7.2 rule 5). The canonical file layout is `NNNN-title.md`; the status table is the single place to check which decisions are live.

## Index

| ADR | Title | File | Status |
|---|---|---|---|
| ADR-0001 | Ledger data formats | [`0001-ledger-data-formats.md`](0001-ledger-data-formats.md) | Proposed |
| ADR-0002 | Query engine | [`0002-query-engine.md`](0002-query-engine.md) | Proposed |
| ADR-0003 | Concurrent `TyCtx` | [`0003-concurrent-tyctx.md`](0003-concurrent-tyctx.md) | Proposed |
| ADR-0004 | Remove bytecode backend + VM | [`0004-remove-bytecode-backend-and-vm.md`](0004-remove-bytecode-backend-and-vm.md) | Proposed |
| ADR-0005 | Panic model: abort only | [`0005-panic-model.md`](0005-panic-model.md) | Proposed |
| ADR-0006 | Integer overflow semantics | [`0006-integer-overflow.md`](0006-integer-overflow.md) | Proposed |
| ADR-0007 | Proc-macro isolation | [`0007-proc-macro-isolation.md`](0007-proc-macro-isolation.md) | Proposed |
| ADR-0008 | Interpreter memory model | [`0008-interpreter-memory-model.md`](0008-interpreter-memory-model.md) | Proposed |
| ADR-0009 | Mid-end optimizer architecture | [`0009-mid-end-optimizer.md`](0009-mid-end-optimizer.md) | Proposed |
| ADR-0010 | Fast debug backend | [`0010-fast-debug-backend.md`](0010-fast-debug-backend.md) | Proposed |
| ADR-0011 | Metadata format (`.gmeta`) | [`0011-metadata-format.md`](0011-metadata-format.md) | Proposed |
| ADR-0012 | Hashing policy | [`0012-hashing.md`](0012-hashing.md) | Proposed |
| ADR-0013 | Evaluation & drop order | [`0013-evaluation-and-drop-order.md`](0013-evaluation-and-drop-order.md) | Proposed |
| ADR-0014 | Debug-info policy | [`0014-debug-info-policy.md`](0014-debug-info-policy.md) | Proposed |
| ADR-0015 | Platform tiers | [`0015-platform-tiers.md`](0015-platform-tiers.md) | Proposed |

## Status summary

All **15** records are `Proposed`. An ADR moves to `Accepted` only with its evidence attached (spike numbers, benchmark, or the falsification test it survived); a `Falsified` record is superseded by its successor and stays here for the history.

## How to add or retire an ADR

1. Copy the header shape of any record above (Title, Status, Context, Decision, Consequences).
2. Record the *proposed default* from the master plan §8 row, or a new decision with its own falsified-if clause, in `docs/roadmaps/glyim-master-plan.md` §8 and here in the same change.
3. Never edit an accepted ADR's Decision in place — supersede it with a new record and mark the old one `Superseded by ADR-NNNN`.

The ADR-completeness contract is enforced by the `adr_registry_completes_the_plan_register` test in `tools/glyim-metrics/tests/adr_registry.rs`: every ADR ID in the plan's §8 register must have a file here, every file must carry the canonical headers, and this index must list all of them.
