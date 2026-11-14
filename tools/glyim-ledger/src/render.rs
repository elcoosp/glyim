//! Generated `KNOWN_GAPS.md` (plan §6 M1.9 remainder).
//!
//! Master plan M1.9 ("Renderers") lists the generated `KNOWN_GAPS.md` as
//! the slice after the `status` renderers, with the `DoD` "outputs
//! deterministic (golden)". The M1.7 migration slice made
//! `ledger/gaps/known/*.toml` the machine-checked status source of truth
//! and left `KNOWN_GAPS.md` as hand-maintained prose — which already
//! drifted once (the file's own audit note records the stale
//! `include!`/`--lto=thin` claims). This module closes that drift class:
//! the document is now a **pure function of the gap registry**, rendered
//! by [`KnownGapsRenderer`] implementations and gated by
//! `glyim-ledger render-gaps --check`.
//!
//! # The seam
//!
//! [`MarkdownGapRenderer`] produces the committed `KNOWN_GAPS.md` face;
//! [`JsonGapRenderer`] produces the `glyim.known-gaps/1` JSON contract for
//! the plan M1.9 Astro readiness page (the next consumer of the same
//! data). Both consume the M1.7 [`GapOutcome`] verbatim — the same load,
//! sort, and witness-debt pipeline `glyim-ledger gaps` prints — so there
//! is exactly one definition of "what the registry says".
//!
//! # Determinism
//!
//! No clock, no environment, no iteration-order dependence: sections are
//! ordered by gap ID (the order `run_gaps` establishes), and the field
//! order of the JSON face is the struct shape (serde). The tests pin a
//! full golden document and a repeat-render byte-equality check; the
//! `--check` gate re-renders and byte-compares against the committed
//! file, so any registry edit that skips the re-render fails CI (wired
//! with the M1.10 ledger-CI slices).
//!
//! # Wire-up
//!
//! `glyim-ledger render-gaps [ROOT] [--format md|json] [--out PATH]
//! [--check]`: with `--out` the rendered document is written (default
//! target `<ROOT>/KNOWN_GAPS.md` for Markdown), with `--check` the
//! rendered bytes are compared against the target and any drift fails
//! with [`crate::LedgerCliError::RenderDrift`]; with neither, the document
//! prints to stdout.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use ledger_schema::{GapClass, GapEntry, GapStatus, Sourced};

use crate::{GapOutcome, LedgerCliError, run_gaps};

/// Schema tag of the JSON face (the M1.9 Astro readiness page's input).
pub const KNOWN_GAPS_SCHEMA: &str = "glyim.known-gaps/1";

/// Provenance marker that opens the generated Markdown block.
pub const KNOWN_GAPS_MD_START: &str = "<!-- ledger:known-gaps:start";

/// Provenance marker that closes the generated Markdown block.
pub const KNOWN_GAPS_MD_END: &str = "<!-- ledger:known-gaps:end -->";

/// The committed document the Markdown face renders to (and `--check`
/// byte-compares against), relative to the project root — the same target
/// the gap-lifecycle write faces refresh after a registry rewrite.
pub const KNOWN_GAPS_MD_TARGET: &str = "KNOWN_GAPS.md";

/// Which face [`run_render_gaps`] renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderGapsFormat {
    /// The committed `KNOWN_GAPS.md` document.
    Markdown,
    /// The `glyim.known-gaps/1` JSON contract.
    Json,
}

impl RenderGapsFormat {
    /// Canonical flag spelling (`--format md|json`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }

    /// Parse the `--format` flag value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "md" => Some(Self::Markdown),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
}

/// Renders a [`GapOutcome`] into a document face.
///
/// The trait is the M1.9 renderer seam: implement it to add a face (the
/// Astro page's embedded JSON, a future HTML face) without touching the
/// load pipeline or the CLI wiring.
pub trait KnownGapsRenderer {
    /// Render the outcome into the target document (bytes are a pure
    /// function of the outcome — deterministic golden output).
    fn render(&self, outcome: &GapOutcome) -> String;
}

/// The committed `KNOWN_GAPS.md` face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkdownGapRenderer;

/// One row of the JSON face (`glyim.known-gaps/1`).
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct KnownGapDoc {
    /// Stable gap ID.
    pub id: String,
    /// Gap class letter (`a`/`b`/`c`).
    pub class: &'static str,
    /// Human-readable one-liner.
    pub title: String,
    /// Feature IDs the gap limits.
    pub affects: Vec<String>,
    /// Level ceiling the gap imposes (`L2` …).
    pub caps: String,
    /// Tag or ref where the gap was first recorded.
    pub opened_in: String,
    /// Witness status (`witnessed`/`unwitnessed`).
    pub status: &'static str,
    /// Witness fixture path, present only when witnessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness: Option<String>,
    /// Diagnostic the witness must produce, present only when declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expect_diagnostic: Option<String>,
}

/// The JSON face document: registry snapshot + per-gap rows.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct KnownGapsDocument {
    /// Schema tag, always [`KNOWN_GAPS_SCHEMA`].
    pub schema: &'static str,
    /// Number of open gap entries in the registry.
    pub open_gaps: usize,
    /// How many entries carry a strict-xfail witness.
    pub witnessed: usize,
    /// How many entries are explicit witness debt.
    pub witness_debt: usize,
    /// Registry IDs in the debt list (sorted by ID).
    pub witness_debt_ids: Vec<String>,
    /// One row per open gap, sorted by ID.
    pub gaps: Vec<KnownGapDoc>,
}

/// The `glyim.known-gaps/1` JSON face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonGapRenderer;

/// What [`run_render_gaps`] produced: the document bytes plus the load
/// outcome they were rendered from and the file operation performed.
#[derive(Debug)]
pub struct RenderGapsOutcome {
    /// The gap-registry outcome the document was rendered from.
    pub outcome: GapOutcome,
    /// The face that was rendered.
    pub format: RenderGapsFormat,
    /// The rendered document bytes (the exact bytes written/compared).
    pub rendered: String,
    /// The file written (`--out`) or checked (`--check`), when either ran.
    pub target: Option<PathBuf>,
    /// Whether a `--check` byte-comparison ran (it passed — drift fails
    /// with [`LedgerCliError::RenderDrift`] instead of returning here).
    pub checked: bool,
}

impl KnownGapsRenderer for MarkdownGapRenderer {
    fn render(&self, outcome: &GapOutcome) -> String {
        let witnessed = outcome.gaps.len() - outcome.unwitnessed.len();
        let mut text = String::new();
        let _ = writeln!(
            text,
            "{KNOWN_GAPS_MD_START} — generated by `glyim-ledger render-gaps`; DO NOT EDIT. -->"
        );
        text.push_str("<!-- Regenerate with `cargo run -p glyim-ledger -- render-gaps --out KNOWN_GAPS.md`; -->\n");
        text.push_str("<!-- `glyim-ledger render-gaps --check` fails on any drift (plan M1.9: outputs deterministic (golden)). -->\n\n");
        text.push_str("# Known gaps\n\n");
        text.push_str(
            "Machine-checked status source of truth: `ledger/gaps/known/*.toml` (plan\n\
             M1.7). This document is **generated** from that registry (the plan M1.9\n\
             remainder slice), so the file and the registry cannot disagree: every\n\
             registry edit is re-rendered and the `--check` gate fails on drift. The\n\
             registry holds only *open* gaps; closed entries retire to the git history\n\
             of this file and of the registry (the RG-10 verdict's contract, see\n\
             `tools/glyim-ledger/src/gates.rs`).\n\n",
        );
        let _ = writeln!(
            text,
            "Registry snapshot: {} open gaps — {} witnessed, {} unwitnessed (witness debt,\n\
             plan §3.3: allowed, but never silent).",
            outcome.gaps.len(),
            witnessed,
            outcome.unwitnessed.len(),
        );
        if !outcome.unwitnessed.is_empty() {
            text.push_str("\n## Witness debt\n\n");
            for id in &outcome.unwitnessed {
                if let Some(gap) = outcome.gaps.iter().find(|g| g.id() == id.as_str()) {
                    let _ = writeln!(text, "- {id} — {}", gap.entry.title);
                }
            }
        }
        for gap in &outcome.gaps {
            render_gap_section(&mut text, gap);
        }
        text.push('\n');
        text.push_str(KNOWN_GAPS_MD_END);
        text.push('\n');
        text
    }
}

/// Renders one gap's detail section (shared by the Markdown face).
fn render_gap_section(text: &mut String, gap: &Sourced<GapEntry>) {
    let entry = &gap.entry;
    let class_note = match entry.class {
        GapClass::A => "diagnosed: every use site gets a diagnostic",
        GapClass::B => "silent wrong behaviour — always first in the work queue",
        GapClass::C => "deferred, no code path, no diagnostic needed",
    };
    let _ = writeln!(text, "\n## {} — {}", entry.id, entry.title);
    let _ = writeln!(
        text,
        "\n- class: `{}` ({class_note})\n- affects: {}",
        entry.class.as_str(),
        entry
            .affects
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let _ = writeln!(
        text,
        "- caps affected features at `{}`\n- opened in: {}",
        entry.caps, entry.opened_in,
    );
    match entry.status {
        GapStatus::Witnessed => {
            text.push_str("- status: witnessed\n");
            if let Some(witness) = &entry.witness {
                let _ = writeln!(text, "- witness: `{witness}` (strict xfail)");
            }
            if let Some(expect) = &entry.expect {
                let _ = writeln!(
                    text,
                    "- expect: witness must produce diagnostic `{}`",
                    expect.diagnostic,
                );
            }
        }
        GapStatus::Unwitnessed => {
            text.push_str("- status: unwitnessed (witness debt)\n");
        }
    }
}

impl KnownGapsRenderer for JsonGapRenderer {
    fn render(&self, outcome: &GapOutcome) -> String {
        let document = KnownGapsDocument {
            schema: KNOWN_GAPS_SCHEMA,
            open_gaps: outcome.gaps.len(),
            witnessed: outcome.gaps.len() - outcome.unwitnessed.len(),
            witness_debt: outcome.unwitnessed.len(),
            witness_debt_ids: outcome.unwitnessed.clone(),
            gaps: outcome
                .gaps
                .iter()
                .map(|sourced| KnownGapDoc {
                    id: sourced.entry.id.clone(),
                    class: sourced.entry.class.as_str(),
                    title: sourced.entry.title.clone(),
                    affects: sourced.entry.affects.clone(),
                    caps: sourced.entry.caps.to_string(),
                    opened_in: sourced.entry.opened_in.clone(),
                    status: match sourced.entry.status {
                        GapStatus::Witnessed => "witnessed",
                        GapStatus::Unwitnessed => "unwitnessed",
                    },
                    witness: sourced.entry.witness.clone(),
                    expect_diagnostic: sourced
                        .entry
                        .expect
                        .as_ref()
                        .map(|expect| expect.diagnostic.clone()),
                })
                .collect(),
        };
        let mut text = serde_json::to_string_pretty(&document)
            .expect("the glyim.known-gaps/1 document is a serialisation-only struct");
        text.push('\n');
        text
    }
}

/// Load the gap registry at `root`, render the requested face, and perform
/// the `--out` write or the `--check` byte-comparison.
///
/// This is the [`run_gaps`] pipeline verbatim (load, validate, sort by ID,
/// witness-debt list) plus the render seam, so the document can never
/// disagree with what `glyim-ledger gaps` reports.
///
/// # Errors
///
/// [`LedgerCliError::RenderDrift`] when `--check` compares against a file
/// whose bytes differ from the render; [`LedgerCliError::RenderWrite`]
/// when the `--out` write fails.
pub fn run_render_gaps(
    root: &Path,
    format: RenderGapsFormat,
    out: Option<&Path>,
    check: bool,
) -> Result<RenderGapsOutcome, LedgerCliError> {
    let span = tracing::info_span!(
        "render_gaps",
        root = %root.display(),
        format = format.as_str(),
        check = check,
    );
    let _enter = span.enter();

    let outcome = run_gaps(root);
    let renderer: &dyn KnownGapsRenderer = match format {
        RenderGapsFormat::Markdown => &MarkdownGapRenderer,
        RenderGapsFormat::Json => &JsonGapRenderer,
    };
    let rendered = renderer.render(&outcome);
    tracing::debug!(
        bytes = rendered.len(),
        gaps = outcome.gaps.len(),
        "known-gaps document rendered"
    );

    let target = out.map(Path::to_path_buf);
    if check {
        let target = target.unwrap_or_else(|| root.join(KNOWN_GAPS_MD_TARGET));
        let committed =
            std::fs::read_to_string(&target).map_err(|err| LedgerCliError::RenderDrift {
                path: target.display().to_string(),
                detail: format!("cannot read the committed document: {err}"),
            })?;
        if committed != rendered {
            return Err(LedgerCliError::RenderDrift {
                path: target.display().to_string(),
                detail: format!(
                    "the committed document drifted from the registry render \
                     (committed {} bytes, render {} bytes)",
                    committed.len(),
                    rendered.len(),
                ),
            });
        }
        return Ok(RenderGapsOutcome {
            outcome,
            format,
            rendered,
            target: Some(target),
            checked: true,
        });
    }
    if let Some(target) = &target {
        std::fs::write(target, &rendered).map_err(|err| LedgerCliError::RenderWrite {
            path: target.display().to_string(),
            detail: err.to_string(),
        })?;
    }
    Ok(RenderGapsOutcome {
        outcome,
        format,
        rendered,
        target,
        checked: false,
    })
}

#[cfg(test)]
mod tests {
    //! Tests pin the M1.9 `DoD` ("outputs deterministic (golden)"): the
    //! renderers are pure functions of the [`GapOutcome`], a golden
    //! full-text fixture pins the Markdown face, the JSON face parses back
    //! into the `glyim.known-gaps/1` contract, and the `--check` drift gate
    //! fails a tampered document with the typed
    //! [`LedgerCliError::RenderDrift`] error.

    use std::path::{Path, PathBuf};

    use ledger_schema::{GapClass, GapEntry, GapExpectation, GapStatus, Level, Sourced};

    use super::{
        JsonGapRenderer, KNOWN_GAPS_MD_TARGET, KnownGapsRenderer, LedgerCliError,
        MarkdownGapRenderer, RenderGapsFormat,
    };
    use crate::{Command, GapOutcome, parse_args, usage};

    /// A registry fixture entry with only the fields the renderer consumes.
    fn gap_entry(id: &str, class: GapClass, title: &str, status: GapStatus) -> GapEntry {
        GapEntry {
            id: id.to_string(),
            class,
            title: title.to_string(),
            affects: vec!["LANG-TEST-FEATURE".to_string()],
            caps: Level::L2,
            witness: (status == GapStatus::Witnessed)
                .then(|| "crates/glyim-test/tests/compile-fail/test-known-gap.g".to_string()),
            expect: (class == GapClass::A).then(|| GapExpectation {
                diagnostic: "T0060".to_string(),
            }),
            opened_in: "v0.3.0".to_string(),
            target_milestone: None,
            prevention: Vec::new(),
            status,
        }
    }

    /// An in-memory [`GapOutcome`]: every entry declared from one synthetic
    /// registry file, sorted by ID (the same order `run_gaps` produces).
    fn gap_outcome(gaps: Vec<GapEntry>) -> GapOutcome {
        let mut sourced: Vec<Sourced<GapEntry>> = gaps
            .into_iter()
            .map(|entry| Sourced {
                path: PathBuf::from("ledger/gaps/known/test.toml"),
                entry,
            })
            .collect();
        sourced.sort_by(|a, b| a.id().cmp(b.id()));
        let unwitnessed = sourced
            .iter()
            .filter(|g| matches!(g.entry.status, GapStatus::Unwitnessed))
            .map(|g| g.id().to_string())
            .collect();
        GapOutcome {
            diagnostics: Vec::new(),
            gaps: sourced,
            unwitnessed,
        }
    }

    /// The golden fixture outcome: one witnessed class-a gap (with an
    /// expectation and a witness fixture) and one unwitnessed class-b gap
    /// (witness debt), so every Markdown branch renders at least once.
    fn golden_outcome() -> GapOutcome {
        gap_outcome(vec![
            gap_entry(
                "GAP-0001",
                GapClass::A,
                "await in a loop body is rejected (diagnostic T0060)",
                GapStatus::Witnessed,
            ),
            gap_entry(
                "GAP-0002",
                GapClass::B,
                "SEH cleanup edges are unreachable",
                GapStatus::Unwitnessed,
            ),
        ])
    }

    #[test]
    fn markdown_render_twice_is_byte_identical() {
        let renderer = MarkdownGapRenderer;
        let first = renderer.render(&golden_outcome());
        let second = renderer.render(&golden_outcome());
        assert_eq!(first, second, "M1.9 DoD: outputs deterministic (golden)");
    }

    #[test]
    fn markdown_golden_document_pins_the_full_face() {
        let text = MarkdownGapRenderer.render(&golden_outcome());
        let expected = "\
<!-- ledger:known-gaps:start — generated by `glyim-ledger render-gaps`; DO NOT EDIT. -->
<!-- Regenerate with `cargo run -p glyim-ledger -- render-gaps --out KNOWN_GAPS.md`; -->
<!-- `glyim-ledger render-gaps --check` fails on any drift (plan M1.9: outputs deterministic (golden)). -->

# Known gaps

Machine-checked status source of truth: `ledger/gaps/known/*.toml` (plan
M1.7). This document is **generated** from that registry (the plan M1.9
remainder slice), so the file and the registry cannot disagree: every
registry edit is re-rendered and the `--check` gate fails on drift. The
registry holds only *open* gaps; closed entries retire to the git history
of this file and of the registry (the RG-10 verdict's contract, see
`tools/glyim-ledger/src/gates.rs`).

Registry snapshot: 2 open gaps — 1 witnessed, 1 unwitnessed (witness debt,
plan §3.3: allowed, but never silent).

## Witness debt

- GAP-0002 — SEH cleanup edges are unreachable

## GAP-0001 — await in a loop body is rejected (diagnostic T0060)

- class: `a` (diagnosed: every use site gets a diagnostic)
- affects: `LANG-TEST-FEATURE`
- caps affected features at `L2`
- opened in: v0.3.0
- status: witnessed
- witness: `crates/glyim-test/tests/compile-fail/test-known-gap.g` (strict xfail)
- expect: witness must produce diagnostic `T0060`

## GAP-0002 — SEH cleanup edges are unreachable

- class: `b` (silent wrong behaviour — always first in the work queue)
- affects: `LANG-TEST-FEATURE`
- caps affected features at `L2`
- opened in: v0.3.0
- status: unwitnessed (witness debt)

<!-- ledger:known-gaps:end -->
";
        assert_eq!(text, expected);
    }

    #[test]
    fn markdown_witness_debt_section_is_omitted_when_empty() {
        let text = MarkdownGapRenderer.render(&gap_outcome(vec![gap_entry(
            "GAP-0009",
            GapClass::C,
            "deferred gap",
            GapStatus::Witnessed,
        )]));
        assert!(
            !text.contains("## Witness debt"),
            "no debt section when every gap is witnessed:\n{text}"
        );
        assert!(text.contains("Registry snapshot: 1 open gaps — 1 witnessed"));
    }

    #[test]
    fn json_contract_parses_back_with_schema_tag_and_counts() {
        let text = JsonGapRenderer.render(&golden_outcome());
        let value: serde_json::Value =
            serde_json::from_str(&text).expect("the glyim.known-gaps/1 face must be valid JSON");
        assert_eq!(value["schema"], "glyim.known-gaps/1");
        assert_eq!(value["open_gaps"], 2);
        assert_eq!(value["witnessed"], 1);
        assert_eq!(value["witness_debt"], 1);
        let gaps = value["gaps"].as_array().expect("gaps array");
        assert_eq!(gaps.len(), 2);
        assert_eq!(gaps[0]["id"], "GAP-0001");
        assert_eq!(gaps[0]["class"], "a");
        assert_eq!(gaps[0]["caps"], "L2");
        assert_eq!(gaps[0]["status"], "witnessed");
        assert_eq!(gaps[0]["expect_diagnostic"], "T0060");
        assert_eq!(gaps[1]["id"], "GAP-0002");
        assert_eq!(gaps[1]["status"], "unwitnessed");
        assert_eq!(value["witness_debt_ids"][0], "GAP-0002");
    }

    #[test]
    fn json_render_twice_is_byte_identical() {
        let first = JsonGapRenderer.render(&golden_outcome());
        let second = JsonGapRenderer.render(&golden_outcome());
        assert_eq!(first, second);
    }

    #[test]
    fn run_render_gaps_writes_then_check_passes_and_drift_fails() {
        let dir = tempfile::tempdir().expect("tmp");
        let root = dir.path();
        let target = root.join(KNOWN_GAPS_MD_TARGET);

        // Seed the committed document from the synthetic registry on disk so
        // `run_render_gaps` loads it (the outcome fixture mirrors the file).
        std::fs::create_dir_all(root.join("ledger/gaps/known")).expect("mkdir");
        std::fs::write(
            root.join("ledger/gaps/known/seed.toml"),
            "[[gap]]\n\
         id = \"GAP-0001\"\n\
         class = \"a\"\n\
         title = \"await in a loop body is rejected (diagnostic T0060)\"\n\
         affects = [\"LANG-TEST-FEATURE\"]\n\
         caps = \"L2\"\n\
         opened_in = \"v0.3.0\"\n\
         status = \"witnessed\"\n\
         witness = \"witness.g\"\n\
         expect = { diagnostic = \"T0060\" }\n",
        )
        .expect("write registry");
        std::fs::write(root.join("witness.g"), "//@ known-gap: GAP-0001\n").expect("write witness");

        // --out writes the document; a following --check passes.
        let written =
            super::run_render_gaps(root, RenderGapsFormat::Markdown, Some(&target), false)
                .expect("write pass");
        assert_eq!(written.target.as_deref(), Some(target.as_path()));
        assert!(target.exists(), "the rendered document must land");
        let on_disk = std::fs::read_to_string(&target).expect("read back");
        assert!(on_disk.contains("# Known gaps"));

        let checked = super::run_render_gaps(root, RenderGapsFormat::Markdown, Some(&target), true)
            .expect("check pass");
        assert!(checked.checked);

        // A hand edit drifts the document: the typed drift error names it.
        std::fs::write(&target, "# tampered\n").expect("tamper");
        let err = super::run_render_gaps(root, RenderGapsFormat::Markdown, Some(&target), true)
            .expect_err("drift must fail");
        match &err {
            LedgerCliError::RenderDrift { path, .. } => {
                assert_eq!(path, &target.display().to_string());
            }
            other => panic!("expected RenderDrift, got {other:?}"),
        }
    }

    #[test]
    fn run_render_gaps_stdout_face_has_no_target() {
        let dir = tempfile::tempdir().expect("tmp");
        let outcome = super::run_render_gaps(dir.path(), RenderGapsFormat::Json, None, false)
            .expect("stdout pass");
        assert!(outcome.target.is_none());
        assert!(!outcome.checked);
        assert!(outcome.rendered.contains("\"glyim.known-gaps/1\""));
    }

    #[test]
    fn parse_args_accepts_render_gaps_flags() {
        let command = parse_args(
            [
                "render-gaps",
                "docs",
                "--format",
                "json",
                "--out",
                "g.md",
                "--check",
            ]
            .into_iter()
            .map(String::from),
        )
        .expect("parse");
        match command {
            Command::RenderGaps {
                root,
                format,
                out,
                check,
            } => {
                assert_eq!(root, Path::new("docs"));
                assert_eq!(format, RenderGapsFormat::Json);
                assert_eq!(out, Some(PathBuf::from("g.md")));
                assert!(check);
            }
            other => panic!("expected RenderGaps, got {other:?}"),
        }

        let default = parse_args(["render-gaps"].into_iter().map(String::from)).expect("parse");
        assert_eq!(
            default,
            Command::RenderGaps {
                root: PathBuf::from("."),
                format: RenderGapsFormat::Markdown,
                out: None,
                check: false,
            }
        );
    }

    #[test]
    fn parse_args_rejects_bad_render_flags() {
        let err = parse_args(
            ["render-gaps", "--format", "yaml"]
                .into_iter()
                .map(String::from),
        )
        .expect_err("bad format");
        assert!(matches!(err, LedgerCliError::RenderBadFormat { .. }));

        let err = parse_args(["render-gaps", "--out"].into_iter().map(String::from))
            .expect_err("missing value");
        assert!(matches!(err, LedgerCliError::RenderMissingValue { .. }));

        let err = parse_args(["render-gaps", "a", "b"].into_iter().map(String::from))
            .expect_err("extra positional");
        assert!(matches!(err, LedgerCliError::ExtraArgument { .. }));
    }

    #[test]
    fn render_error_variants_carry_code_and_help() {
        let drift = LedgerCliError::RenderDrift {
            path: "KNOWN_GAPS.md".to_string(),
            detail: "expected 1204 bytes, found 12".to_string(),
        };
        assert_eq!(
            miette::Diagnostic::code(&drift)
                .map(|c| c.to_string())
                .as_deref(),
            Some("glyim-ledger::render_drift")
        );
        let help = miette::Diagnostic::help(&drift)
            .map(|h| h.to_string())
            .unwrap_or_default();
        assert!(help.contains("render-gaps --out"), "help: {help}");

        for err in [
            LedgerCliError::RenderWrite {
                path: "KNOWN_GAPS.md".to_string(),
                detail: "denied".to_string(),
            },
            LedgerCliError::RenderBadFormat {
                value: "yaml".to_string(),
                usage: usage(),
            },
            LedgerCliError::RenderMissingValue {
                flag: "--out".to_string(),
                usage: usage(),
            },
        ] {
            assert!(
                miette::Diagnostic::code(&err).is_some(),
                "{err:?} must carry a code"
            );
            assert!(
                miette::Diagnostic::help(&err).is_some(),
                "{err:?} must carry help"
            );
        }
    }

    #[test]
    fn usage_lists_render_gaps() {
        assert!(usage().contains("render-gaps"));
    }
}
