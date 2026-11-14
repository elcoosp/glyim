//! CLI for the Readiness Ledger (plan M1.1).
//!
//! `glyim-ledger check [ROOT]` is the registry gate CI runs: it loads the
//! ledger under `ROOT`, validates it, cross-checks the M1.3 inventory
//! extractors (plan §3.5), prints every diagnostic sorted, and exits 1 if
//! any error-severity diagnostic exists. `glyim-ledger gaps [ROOT]`
//! renders the M1.7 gap-registry witness report (witness debt is printed,
//! not failed). `glyim-ledger next [ROOT] [--lane L] [--n N]` prints the
//! plan §3.9 deterministic work queue as the `glyim.ledger-next/1` JSON
//! contract `glyim-pilot` consumes. `glyim-ledger schema [OUT]` exports
//! the JSON-Schema document for both registry entry types.
//! `glyim-ledger inventory [ROOT]` reports the source-derived inventory
//! and its claim/ignore status without full registry validation.
//! `glyim-ledger compute [ROOT] [--evidence PATH] [--format text|json]
//! [--snapshot PATH]` runs the plan M1.8 level-computation engine and
//! prints per-feature readiness levels computed from evidence and
//! `ledger/policy.toml`; `--snapshot PATH` also persists the
//! `glyim.ledger-snapshot/1` document of the run. `glyim-ledger status
//! [`PATH`]` renders the computed
//! status as the marker-delimited Markdown block (plan M1.9 first slice)
//! or the `glyim.status/1` JSON contract; `--out PATH` writes the
//! rendered document (default `STATUS.md` / `STATUS.json` by face) and
//! `--check` fails on any drift between the committed document and the
//! computed status (the plan M1.10 "STATUS is up to date" gate).
//! `glyim-ledger diff <BASE>
//! <CURRENT> [ROOT] [--format text|json]` is the plan M1.10
//! readiness-regression gate: it compares two snapshot documents and
//! fails on any computed-level drop that no unexpired waiver in
//! `ledger/waivers.toml` covers (constitution C9). `glyim-ledger gates
//! [ROOT] [--evidence PATH] [--format text|json]` is the plan M1.12
//! release-gate report: it evaluates every `RG-NN` gate declared in
//! `ledger/gates.toml` against the current evidence and prints all 24 —
//! `unmeasured` gates are displayed, never silently passed.
//! `glyim-ledger verify [ROOT] [--evidence PATH] [--format text|json]` is
//! the plan M1.13 determinism self-check: every ledger pipeline runs
//! twice from independent load passes and any rendered-byte drift fails
//! the command. `glyim-ledger render-gaps [ROOT] [--format md|json]
//! [--out PATH] [--check]` is the plan M1.9 remainder: it generates
//! `KNOWN_GAPS.md` (or the `glyim.known-gaps/1` JSON face) from the gap
//! registry — `--out` writes the rendered document, `--check` fails on
//! any drift between the committed document and the render.
//! `glyim-ledger gaps witness [ROOT] --id <GAP-ID> --witness <PATH>` is
//! the gap-lifecycle write face: it attaches the witness fixture that
//! reproduces one unwitnessed gap, editing exactly the matched `[[gap]]`
//! block through the `GapWitnessRewriter` seam (the curated header prose
//! preserved byte-for-byte) and refreshing the generated document in the
//! same run. `glyim-ledger gaps retire [ROOT] --id <GAP-ID>` is the
//! closure half: it removes one gap's block entirely through the
//! `GapRetirer` seam — closed entries retire to the git history of the
//! registry — with the same generated-document refresh.
//! `glyim-ledger evidence record [--out PATH] [--format text|json]
//! <SRC...>` is the plan §3.4 evidence-store curation face: it validates
//! harness-emitted records against the registry and merges them into the
//! committed store (whole-record dedupe, canonical order).
//! `glyim-ledger evidence check [ROOT] [--format text|json]` validates
//! the committed store and reports its coverage; error diagnostics fail
//! the exit code. `glyim-ledger waivers [ROOT] [--format text|json]`
//! loads `ledger/waivers.toml` and reports every waiver's expiry verdict
//! — expired waivers and load diagnostics fail the exit code
//! (constitution C9: expired exceptions fail CI even when no regression
//! needs covering yet). `waivers grant [ROOT] --feature <ID> --reason
//! <TEXT> --issue <URL> [--days N]` is the curated write face: it
//! validates the request against the registry and the C9 horizon, then
//! appends the canonical `[[waiver]]` block to `ledger/waivers.toml`
//! (deterministic `WVR-<date>-<slug>` ID, never a silent overwrite).
//! `glyim-ledger release-check <VERSION> [ROOT] [--format text|json]` is
//! the plan §10 final release gate: it composes the M1.12 gate report,
//! the M1.13 determinism self-check, and the C9 waiver health into one
//! verdict — every declared gate green (`unmeasured` blocks: displayed,
//! never silently passed), evidence reproducible, no waiver active or
//! expired, and no error diagnostic across the composed faces.
//! `glyim-ledger waivers prune [ROOT] [--format text|json]` is the
//! batch-retirement write face: it removes every expired `[[waiver]]`
//! block through the `WaiverPruner` seam and rewrites the remaining
//! entries canonically (a non-clean document is refused; a document with
//! no expired waiver is never rewritten).

use std::process::ExitCode;

use glyim_ledger::{
    Command, LedgerCliError, SnapshotStore, format_outcome, parse_args, write_schema,
};

/// Print a typed CLI error with its `miette` help line (when one exists).
fn report(err: &LedgerCliError) {
    eprintln!("{err}");
    if let Some(help) = miette::Diagnostic::help(err) {
        eprintln!("  help: {help}");
    }
}

/// Serialize the built-in schema document, routing any failure through the
/// typed [`LedgerCliError`] seam so every failure path carries the same
/// `help:` line and exit contract (the former stdout branch reported an
/// ad-hoc `eprintln!` instead).
fn schema_stdout() -> Result<String, LedgerCliError> {
    serde_json::to_string_pretty(&ledger_schema::registry_schema())
        .map(|text| text + "\n")
        .map_err(|err| LedgerCliError::SchemaSerialize {
            detail: err.to_string(),
        })
}

fn main() -> ExitCode {
    let command = match parse_args(std::env::args().skip(1)) {
        Ok(command) => command,
        Err(err) => {
            report(&err);
            return ExitCode::from(2);
        }
    };

    match command {
        Command::Check { root } => {
            let outcome = glyim_ledger::run_check(&root);
            print!("{}", format_outcome(&outcome));
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Gaps { root } => {
            let outcome = glyim_ledger::run_gaps(&root);
            print!("{}", glyim_ledger::format_gap_report(&outcome));
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::GapsWitness {
            root,
            id,
            witness,
            format,
        } => {
            // The write face routes every rejection through the typed
            // error seam (the revoke-face dispatch shape — nothing was
            // written on a rejection). A successful witness refreshes the
            // generated KNOWN_GAPS.md in the same run through the same
            // `run_render_gaps` pipeline the `--check` gate byte-compares
            // against: the face never leaves the committed document
            // silently drifted from the registry it just edited.
            let request = glyim_ledger::GapsWitnessRequest { id, witness };
            match glyim_ledger::run_gaps_witness(
                &root,
                &request,
                &glyim_ledger::FileGapWitnessRewriter,
            ) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::GapsFormat::Text => {
                            print!("{}", glyim_ledger::format_gaps_witness_text(&outcome));
                        }
                        glyim_ledger::GapsFormat::Json => {
                            print!("{}", glyim_ledger::format_gaps_witness_json(&outcome));
                        }
                    }
                    match glyim_ledger::run_render_gaps(
                        &root,
                        glyim_ledger::RenderGapsFormat::Markdown,
                        Some(&root.join(glyim_ledger::KNOWN_GAPS_MD_TARGET)),
                        false,
                    ) {
                        Ok(_) => ExitCode::SUCCESS,
                        Err(err) => {
                            report(&err);
                            ExitCode::FAILURE
                        }
                    }
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::GapsRetire { root, id, format } => {
            // The closure half of the gap lifecycle, wired through the
            // same typed error seam and the same generated-document
            // refresh as the witness face (a retired gap never leaves
            // KNOWN_GAPS.md silently drifted).
            let request = glyim_ledger::GapsRetireRequest { id };
            match glyim_ledger::run_gaps_retire(&root, &request, &glyim_ledger::FileGapRetirer) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::GapsFormat::Text => {
                            print!("{}", glyim_ledger::format_gaps_retire_text(&outcome));
                        }
                        glyim_ledger::GapsFormat::Json => {
                            print!("{}", glyim_ledger::format_gaps_retire_json(&outcome));
                        }
                    }
                    match glyim_ledger::run_render_gaps(
                        &root,
                        glyim_ledger::RenderGapsFormat::Markdown,
                        Some(&root.join(glyim_ledger::KNOWN_GAPS_MD_TARGET)),
                        false,
                    ) {
                        Ok(_) => ExitCode::SUCCESS,
                        Err(err) => {
                            report(&err);
                            ExitCode::FAILURE
                        }
                    }
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::Next { root, lane, limit } => {
            let outcome = glyim_ledger::run_next(&root);
            if outcome.is_ok() {
                let selected = glyim_ledger::filter(&outcome, lane, limit);
                print!("{}", glyim_ledger::format_next_json(&selected));
                ExitCode::SUCCESS
            } else {
                // Registry diagnostics are prose, not queue items: report
                // them through the text channel and fail like `check`.
                for diagnostic in &outcome.diagnostics {
                    eprintln!("{diagnostic}");
                }
                ExitCode::FAILURE
            }
        }
        Command::Explain { id, root } => {
            let outcome = glyim_ledger::run_explain(&root, &id);
            print!("{}", glyim_ledger::format_explain_report(&outcome));
            // A registry load failure fails like `check`; an unknown ID
            // prints the "no registry entry" report and also fails, so a
            // typo'd ID never exits green in a scripted pipeline.
            if outcome.is_ok() && outcome.entry.is_some() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Compute {
            root,
            evidence,
            format,
            snapshot,
        } => {
            let outcome = glyim_ledger::run_compute(&root, evidence.as_deref());
            // Persist the readiness snapshot when requested (`diff`'s input
            // face). A failed write is a CLI error, not a diagnostic: the
            // operator asked for a document that did not land.
            if let Some(snapshot_path) = snapshot {
                let snapshot = glyim_ledger::ComputeSnapshot::from_outcome(&outcome);
                if let Err(diagnostic) =
                    glyim_ledger::FileSnapshotStore.save(&snapshot_path, &snapshot)
                {
                    let err = LedgerCliError::SnapshotWrite {
                        path: snapshot_path.display().to_string(),
                        detail: diagnostic.message,
                    };
                    report(&err);
                    return ExitCode::FAILURE;
                }
            }
            match format {
                glyim_ledger::ComputeFormat::Text => {
                    print!("{}", glyim_ledger::format_compute_report(&outcome));
                }
                glyim_ledger::ComputeFormat::Json => {
                    print!("{}", glyim_ledger::format_compute_json(&outcome));
                }
            }
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Status {
            root,
            evidence,
            format,
            out,
            check,
        } => {
            // The write/check face routes drift and write failures
            // through the typed error seam (a drifted committed document
            // is a CLI failure like a failed snapshot write), while
            // registry/evidence diagnostics fail like `check` after the
            // document still renders — the debt is displayed, not silent
            // (plan M1.10).
            match glyim_ledger::run_status_out(
                &root,
                evidence.as_deref(),
                format,
                out.as_deref(),
                check,
            ) {
                Ok(rendered) => {
                    match (rendered.target.as_deref(), rendered.checked) {
                        (Some(target), true) => {
                            println!(
                                "status check OK: {} matches the computed status ({} feature(s))",
                                target.display(),
                                rendered.outcome.compute.features,
                            );
                        }
                        (Some(target), false) => {
                            println!(
                                "wrote {} from the computed status ({} feature(s))",
                                target.display(),
                                rendered.outcome.compute.features,
                            );
                        }
                        (None, false) => match format {
                            glyim_ledger::StatusFormat::Markdown => {
                                print!(
                                    "{}",
                                    glyim_ledger::format_status_markdown(&rendered.outcome.compute)
                                );
                            }
                            glyim_ledger::StatusFormat::Json => {
                                print!(
                                    "{}",
                                    glyim_ledger::format_status_json(&rendered.outcome.compute)
                                );
                            }
                        },
                        (None, true) => {
                            // Unreachable by construction (`--check` always
                            // names its target), but stay deterministic.
                            println!(
                                "status check OK ({} feature(s))",
                                rendered.outcome.compute.features,
                            );
                        }
                    }
                    if rendered.outcome.is_ok() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::Diff {
            root,
            baseline,
            current,
            format,
        } => {
            let outcome = glyim_ledger::run_diff(&root, &baseline, &current);
            match format {
                glyim_ledger::DiffFormat::Text => {
                    print!("{}", glyim_ledger::format_diff_text(&outcome));
                }
                glyim_ledger::DiffFormat::Json => {
                    print!("{}", glyim_ledger::format_diff_json(&outcome));
                }
            }
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Gates {
            root,
            evidence,
            format,
        } => {
            let outcome = glyim_ledger::run_gates(&root, evidence.as_deref());
            match format {
                glyim_ledger::GatesFormat::Text => {
                    print!("{}", glyim_ledger::format_gates_text(&outcome));
                }
                glyim_ledger::GatesFormat::Json => {
                    print!("{}", glyim_ledger::format_gates_json(&outcome));
                }
            }
            // Load/validation diagnostics fail like `check`; gate verdicts
            // are report content — `unmeasured` must display without
            // failing (plan M1.12), and a failing release gate is CI's
            // business, wired through the later RG-gate slices.
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Verify {
            root,
            evidence,
            format,
        } => {
            let outcome = glyim_ledger::verify_determinism(&root, evidence.as_deref());
            match format {
                glyim_ledger::VerifyFormat::Text => {
                    print!("{}", glyim_ledger::format_verify_text(&outcome));
                }
                glyim_ledger::VerifyFormat::Json => {
                    print!("{}", glyim_ledger::format_verify_json(&outcome));
                }
            }
            // The self-check *is* a gate: a drifted check or an
            // error-severity diagnostic fails the command (plan M1.13 —
            // "run compute twice → identical bytes").
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::RenderGaps {
            root,
            format,
            out,
            check,
        } => {
            // The drift gate fails through the typed error seam (a drifted
            // committed document is a CLI failure like a failed snapshot
            // write), while registry diagnostics fail like `check` after
            // the document still renders — the debt is displayed, not
            // silent (plan M1.9).
            match glyim_ledger::run_render_gaps(&root, format, out.as_deref(), check) {
                Ok(rendered) => {
                    match (rendered.target.as_deref(), rendered.checked) {
                        (Some(target), true) => {
                            println!(
                                "render-gaps check OK: {} matches the registry ({} gap(s))",
                                target.display(),
                                rendered.outcome.gaps.len(),
                            );
                        }
                        (Some(target), false) => {
                            println!(
                                "wrote {} from the gap registry ({} gap(s))",
                                target.display(),
                                rendered.outcome.gaps.len(),
                            );
                        }
                        (None, false) => {
                            print!("{}", rendered.rendered);
                        }
                        (None, true) => {
                            // Unreachable by construction (`--check` always
                            // names its target), but stay deterministic.
                            println!(
                                "render-gaps check OK ({} gap(s))",
                                rendered.outcome.gaps.len(),
                            );
                        }
                    }
                    if rendered.outcome.is_ok() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::EvidenceRecord {
            sources,
            out,
            format,
        } => {
            // The record face operates on the registry at the working
            // directory (every subcommand's default root; the library
            // entry point takes an explicit root for tests/examples).
            match glyim_ledger::run_evidence_record(
                std::path::Path::new("."),
                &sources,
                &out,
                &glyim_ledger::FileEvidenceStore,
            ) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::EvidenceFormat::Text => {
                            print!("{}", glyim_ledger::format_evidence_record_text(&outcome));
                        }
                        glyim_ledger::EvidenceFormat::Json => {
                            print!("{}", glyim_ledger::format_evidence_record_json(&outcome));
                        }
                    }
                    // Invalid or unknown-feature records are verdicts:
                    // report every one of them, then fail like `check`.
                    if outcome.is_ok() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::EvidenceCheck { root, format } => {
            let report = glyim_ledger::run_evidence_check(&root);
            match format {
                glyim_ledger::EvidenceFormat::Text => {
                    print!("{}", glyim_ledger::format_evidence_check_text(&report));
                }
                glyim_ledger::EvidenceFormat::Json => {
                    print!("{}", glyim_ledger::format_evidence_check_json(&report));
                }
            }
            // The check face is a gate: an invalid committed record (or an
            // unknown feature ID) fails the command after reporting it.
            if report.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::EvidenceAudit { root, format } => {
            // The same store-discovery path `evidence check` uses, so the
            // two faces cannot disagree about what the store contains;
            // the file-backed fingerprint digests the real fixture bytes.
            let outcome = glyim_ledger::run_evidence_audit(
                &root,
                &glyim_ledger::ProjectEvidenceSource::new(&root, None),
                &glyim_ledger::FileFixtureFingerprint,
            );
            match format {
                glyim_ledger::EvidenceFormat::Text => {
                    print!("{}", glyim_ledger::format_evidence_audit_text(&outcome));
                }
                glyim_ledger::EvidenceFormat::Json => {
                    print!("{}", glyim_ledger::format_evidence_audit_json(&outcome));
                }
            }
            // The audit face is a gate: a store that outlived its fixtures
            // (missing, drifted, unreadable) fails after reporting every
            // finding.
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Waivers { root, format } => {
            // The file-backed source is the only CLI wiring: the same
            // `load_waivers` loader and diagnostics the `diff` gate uses,
            // so the health face and the regression gate cannot disagree
            // about what `ledger/waivers.toml` says.
            let outcome = glyim_ledger::run_waivers(&root, &glyim_ledger::FileWaiverSource);
            match format {
                glyim_ledger::WaiversFormat::Text => {
                    print!("{}", glyim_ledger::format_waivers_text(&outcome));
                }
                glyim_ledger::WaiversFormat::Json => {
                    print!("{}", glyim_ledger::format_waivers_json(&outcome));
                }
            }
            // The health face is a gate: an expired waiver fails per C9
            // even when no regression needs covering, and a load
            // diagnostic fails like `check`.
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::WaiversGrant {
            root,
            feature,
            reason,
            issue,
            days,
            format,
        } => {
            // The write face routes every rejection through the typed
            // error seam (a rejected grant is a CLI failure, not a
            // diagnostic — nothing was written).
            let request = glyim_ledger::WaiversGrantRequest {
                feature,
                reason,
                issue,
                days: days.unwrap_or(glyim_ledger::DEFAULT_WAIVER_DAYS),
            };
            match glyim_ledger::run_waivers_grant(
                &root,
                &request,
                &glyim_ledger::FileWaiverSource,
                &glyim_ledger::FileWaiverSink,
            ) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::WaiversFormat::Text => {
                            print!("{}", glyim_ledger::format_waivers_grant_text(&outcome));
                        }
                        glyim_ledger::WaiversFormat::Json => {
                            print!("{}", glyim_ledger::format_waivers_grant_json(&outcome));
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::WaiversRevoke { root, id, format } => {
            // The write face routes every rejection through the typed
            // error seam (a rejected revoke is a CLI failure, not a
            // diagnostic — nothing was rewritten; the grant-face
            // dispatch shape).
            let request = glyim_ledger::WaiversRevokeRequest { id };
            match glyim_ledger::run_waivers_revoke(
                &root,
                &request,
                &glyim_ledger::FileWaiverSource,
                &glyim_ledger::FileWaiverRewriter,
            ) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::WaiversFormat::Text => {
                            print!("{}", glyim_ledger::format_waivers_revoke_text(&outcome));
                        }
                        glyim_ledger::WaiversFormat::Json => {
                            print!("{}", glyim_ledger::format_waivers_revoke_json(&outcome));
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::WaiversExtend {
            root,
            id,
            days,
            format,
        } => {
            // The write face routes every rejection through the typed
            // error seam (a rejected extend is a CLI failure, not a
            // diagnostic — nothing was rewritten; the revoke-face
            // dispatch shape). The `--days` default resolves here (the
            // parser stays flag-optional, the grant-face shape).
            let request = glyim_ledger::WaiversExtendRequest {
                id,
                days: days.unwrap_or(glyim_ledger::DEFAULT_WAIVER_DAYS),
            };
            match glyim_ledger::run_waivers_extend(
                &root,
                &request,
                &glyim_ledger::FileWaiverSource,
                &glyim_ledger::FileWaiverExtender,
            ) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::WaiversFormat::Text => {
                            print!("{}", glyim_ledger::format_waivers_extend_text(&outcome));
                        }
                        glyim_ledger::WaiversFormat::Json => {
                            print!("{}", glyim_ledger::format_waivers_extend_json(&outcome));
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::Handoff {
            root,
            items,
            format,
            out,
            check,
        } => {
            // The write/check face routes drift and write failures
            // through the typed error seam (a drifted committed document
            // is a CLI failure like a failed snapshot write), while the
            // composed faces' diagnostics fail like `check` after the
            // document still renders — the debt is displayed, not silent
            // (plan §3.7; the status-face dispatch shape).
            match glyim_ledger::run_handoff_out(
                &root,
                &glyim_ledger::FileHandoffSource,
                items.unwrap_or(glyim_ledger::HANDOFF_DEFAULT_ITEMS),
                format,
                out.as_deref(),
                check,
            ) {
                Ok(rendered) => {
                    match (rendered.target.as_deref(), rendered.checked) {
                        (Some(target), true) => {
                            println!(
                                "handoff check OK: {} matches the composed state \
                                 ({} feature(s), {} queue item(s))",
                                target.display(),
                                rendered.outcome.features,
                                rendered.outcome.queue_total,
                            );
                        }
                        (Some(target), false) => {
                            println!(
                                "wrote {} from the composed state ({} feature(s), {} queue item(s))",
                                target.display(),
                                rendered.outcome.features,
                                rendered.outcome.queue_total,
                            );
                        }
                        (None, false) => match format {
                            glyim_ledger::HandoffFormat::Markdown => {
                                print!("{}", glyim_ledger::format_handoff_text(&rendered.outcome));
                            }
                            glyim_ledger::HandoffFormat::Json => {
                                print!("{}", glyim_ledger::format_handoff_json(&rendered.outcome));
                            }
                        },
                        (None, true) => {
                            // Unreachable by construction (`--check` always
                            // names its target), but stay deterministic.
                            println!(
                                "handoff check OK ({} feature(s))",
                                rendered.outcome.features,
                            );
                        }
                    }
                    if rendered.outcome.is_ok() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::ReleaseCheck {
            version,
            root,
            format,
        } => {
            // The plan §10 final release gate: the composed faces render
            // their full report when the verdict is red — the debt is
            // displayed, never silent — and the exit code carries the
            // verdict (the `verify` dispatch shape: the gate *is* the
            // exit code).
            let outcome = glyim_ledger::run_release_check(
                &root,
                version,
                &glyim_ledger::FileReleaseCheckSource,
            );
            match format {
                glyim_ledger::ReleaseCheckFormat::Text => {
                    print!("{}", glyim_ledger::format_release_check_text(&outcome));
                }
                glyim_ledger::ReleaseCheckFormat::Json => {
                    print!("{}", glyim_ledger::format_release_check_json(&outcome));
                }
            }
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::WaiversPrune { root, format } => {
            // The batch-retirement write face routes every rejection
            // through the typed error seam (a refused prune is a CLI
            // failure — nothing was rewritten; the revoke-face dispatch
            // shape).
            match glyim_ledger::run_waivers_prune(
                &root,
                &glyim_ledger::FileWaiverSource,
                &glyim_ledger::FileWaiverPruner,
            ) {
                Ok(outcome) => {
                    match format {
                        glyim_ledger::WaiversFormat::Text => {
                            print!("{}", glyim_ledger::format_waivers_prune_text(&outcome));
                        }
                        glyim_ledger::WaiversFormat::Json => {
                            print!("{}", glyim_ledger::format_waivers_prune_json(&outcome));
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            }
        }
        Command::Inventory { root } => {
            let outcome = glyim_ledger::run_inventory(&root);
            print!("{}", glyim_ledger::format_inventory(&outcome));
            if outcome.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Command::Schema { out } => match out {
            Some(path) => match write_schema(&path) {
                Ok(summary) => {
                    println!("{summary}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            },
            None => match schema_stdout() {
                Ok(text) => {
                    print!("{text}");
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    report(&err);
                    ExitCode::FAILURE
                }
            },
        },
    }
}
