//! CLI for the M0.6 metrics baseline.
//!
//! Measure a repo, write the JSON baseline, or check the current tree
//! against one (exit status 1 on any ratchet regression — this is what
//! CI's `metrics-ratchet` job runs).
//!
//! Errors are typed ([`MetricsCliError`]) with a `miette` `help()` per
//! variant — the same seam discipline as `glyim-test`'s evidence errors and
//! `glyim-ledger`'s CLI errors: a failure names the remedy.

use std::fmt;
use std::path::PathBuf;
use std::process::ExitCode;

use glyim_metrics::{Ratchet, RepoMetrics, scan_tree};

#[derive(Debug)]
struct Args {
    root: PathBuf,
    check: Option<PathBuf>,
    write: Option<PathBuf>,
}

fn usage() -> String {
    "usage: glyim-metrics [ROOT] [--check BASELINE.json] [--write OUT.json]\n\
     \x20 ROOT defaults to the current directory. With no flags the metrics\n\
     \x20 are printed to stdout as JSON."
        .into()
}

/// Typed error for every CLI seam in this binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricsCliError {
    /// `-h` / `--help` was requested.
    HelpRequested {
        /// The usage text.
        usage: String,
    },
    /// An unrecognized flag was passed.
    UnknownFlag {
        /// The offending flag.
        flag: String,
        /// The usage text.
        usage: String,
    },
    /// More than one ROOT positional was passed.
    ExtraArgument {
        /// The offending argument.
        arg: String,
        /// The usage text.
        usage: String,
    },
    /// A flag that needs a value got none.
    MissingValue {
        /// The flag that needed a value.
        flag: String,
    },
    /// The baseline file could not be read.
    BaselineRead {
        /// The baseline path.
        path: String,
        /// The IO error message.
        detail: String,
    },
    /// The baseline file is not valid JSON.
    BaselineJson {
        /// The parse error message.
        detail: String,
    },
    /// The baseline JSON has an unexpected shape.
    BaselineShape {
        /// The shape error message.
        detail: String,
    },
    /// The current metrics could not be serialized.
    Serialize {
        /// The serialization error message.
        detail: String,
    },
    /// The `--write` output directory could not be created.
    Mkdir {
        /// The directory path.
        path: String,
        /// The IO error message.
        detail: String,
    },
    /// The `--write` output file could not be written.
    Write {
        /// The output path.
        path: String,
        /// The IO error message.
        detail: String,
    },
    /// The ratchet found regressions against the baseline.
    RatchetFailed {
        /// The rendered regression list.
        detail: String,
    },
}

impl fmt::Display for MetricsCliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpRequested { usage } => f.write_str(usage),
            Self::UnknownFlag { flag, usage } => write!(f, "unknown flag {flag}\n{usage}"),
            Self::ExtraArgument { arg, usage } => {
                write!(f, "unexpected extra argument {arg}\n{usage}")
            }
            Self::MissingValue { flag } => write!(f, "{flag} needs a path"),
            Self::BaselineRead { path, detail } => {
                write!(f, "cannot read baseline {path}: {detail}")
            }
            Self::BaselineJson { detail } => write!(f, "baseline is not JSON: {detail}"),
            Self::BaselineShape { detail } => {
                write!(f, "baseline has unexpected shape: {detail}")
            }
            Self::Serialize { detail } => write!(f, "serialize: {detail}"),
            Self::Mkdir { path, detail } => write!(f, "mkdir {path}: {detail}"),
            Self::Write { path, detail } => write!(f, "write {path}: {detail}"),
            Self::RatchetFailed { detail } => f.write_str(detail),
        }
    }
}

impl std::error::Error for MetricsCliError {}

impl miette::Diagnostic for MetricsCliError {
    fn code<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        Some(Box::new(match self {
            Self::HelpRequested { .. } => "glyim-metrics::help",
            Self::UnknownFlag { .. } => "glyim-metrics::unknown_flag",
            Self::ExtraArgument { .. } => "glyim-metrics::extra_argument",
            Self::MissingValue { .. } => "glyim-metrics::missing_value",
            Self::BaselineRead { .. } => "glyim-metrics::baseline_read",
            Self::BaselineJson { .. } => "glyim-metrics::baseline_json",
            Self::BaselineShape { .. } => "glyim-metrics::baseline_shape",
            Self::Serialize { .. } => "glyim-metrics::serialize",
            Self::Mkdir { .. } => "glyim-metrics::mkdir",
            Self::Write { .. } => "glyim-metrics::write",
            Self::RatchetFailed { .. } => "glyim-metrics::ratchet_failed",
        }))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn fmt::Display + 'a>> {
        match self {
            Self::HelpRequested { .. } => None,
            Self::UnknownFlag { .. } => Some(Box::new(
                "the accepted flags are --check BASELINE.json, --write OUT.json, \
                 and -h/--help"
                    .to_string(),
            )),
            Self::ExtraArgument { .. } => Some(Box::new(
                "at most one ROOT positional is accepted; quote paths that \
                 contain spaces"
                    .to_string(),
            )),
            Self::MissingValue { flag } => {
                Some(Box::new(format!("pass the path right after `{flag}`")))
            }
            Self::BaselineRead { .. } => Some(Box::new(
                "baselines live at metrics/baseline.json in the repo root; \
                 check the path and its permissions"
                    .to_string(),
            )),
            Self::BaselineJson { .. } | Self::BaselineShape { .. } => Some(Box::new(
                "regenerate the baseline with \
                 `cargo run -p glyim-metrics -- . --write metrics/baseline.json`"
                    .to_string(),
            )),
            Self::Serialize { .. } => Some(Box::new(
                "serializing in-memory metrics cannot fail by construction — \
                 this is an internal bug, please report it"
                    .to_string(),
            )),
            Self::Mkdir { .. } | Self::Write { .. } => Some(Box::new(
                "check that the parent directory exists and is writable".to_string(),
            )),
            Self::RatchetFailed { .. } => Some(Box::new(
                "fix the regressions, or re-baseline deliberately with \
                 `cargo run -p glyim-metrics -- . --write metrics/baseline.json` \
                 in the same PR — gates never weaken (plan C9)"
                    .to_string(),
            )),
        }
    }
}

fn parse_args() -> Result<Args, MetricsCliError> {
    let mut root = None;
    let mut check = None;
    let mut write = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--check" => {
                check = Some(PathBuf::from(it.next().ok_or(
                    MetricsCliError::MissingValue {
                        flag: "--check".into(),
                    },
                )?));
            }
            "--write" => {
                write = Some(PathBuf::from(it.next().ok_or(
                    MetricsCliError::MissingValue {
                        flag: "--write".into(),
                    },
                )?));
            }
            "-h" | "--help" => {
                return Err(MetricsCliError::HelpRequested { usage: usage() });
            }
            other => {
                if other.starts_with('-') {
                    return Err(MetricsCliError::UnknownFlag {
                        flag: other.to_string(),
                        usage: usage(),
                    });
                }
                if root.is_some() {
                    return Err(MetricsCliError::ExtraArgument {
                        arg: other.to_string(),
                        usage: usage(),
                    });
                }
                root = Some(PathBuf::from(other));
            }
        }
    }
    Ok(Args {
        root: root.unwrap_or_else(|| PathBuf::from(".")),
        check,
        write,
    })
}

fn run() -> Result<String, MetricsCliError> {
    let args = parse_args()?;
    let current = scan_tree(&args.root);

    if let Some(path) = args.check {
        let raw = std::fs::read_to_string(&path).map_err(|e| MetricsCliError::BaselineRead {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| MetricsCliError::BaselineJson {
                detail: e.to_string(),
            })?;
        let baseline =
            RepoMetrics::from_json(&value).map_err(|e| MetricsCliError::BaselineShape {
                detail: e.to_string(),
            })?;
        let regressions = Ratchet::check(&current, &baseline);
        if regressions.is_empty() {
            return Ok(format!(
                "ratchet OK: no regression against {} ({} files, {} lines)",
                path.display(),
                current.rust_files,
                current.code_lines
            ));
        }
        let mut msg = format!(
            "{} metric regression(s) against {}:\n",
            regressions.len(),
            path.display()
        );
        for r in &regressions {
            let _ = writeln!(
                msg,
                "  - {}: baseline {} -> current {}",
                r.metric, r.baseline, r.current,
            );
        }
        return Err(MetricsCliError::RatchetFailed { detail: msg });
    }

    let json = serde_json::to_string_pretty(&current.to_json()).map_err(|e| {
        MetricsCliError::Serialize {
            detail: e.to_string(),
        }
    })?;
    if let Some(path) = args.write {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| MetricsCliError::Mkdir {
                path: parent.display().to_string(),
                detail: e.to_string(),
            })?;
        }
        std::fs::write(&path, format!("{json}\n")).map_err(|e| MetricsCliError::Write {
            path: path.display().to_string(),
            detail: e.to_string(),
        })?;
        return Ok(format!(
            "wrote {} ({} files, {} code lines, {} tests, {} inline allows)",
            path.display(),
            current.rust_files,
            current.code_lines,
            current.test_functions,
            current.inline_allows,
        ));
    }
    Ok(json)
}

fn main() -> ExitCode {
    match run() {
        Ok(msg) => {
            println!("{msg}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            if let Some(help) = miette::Diagnostic::help(&err) {
                eprintln!("  help: {help}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MetricsCliError, usage};

    #[test]
    fn every_failure_variant_has_help_text() {
        let errors = [
            MetricsCliError::UnknownFlag {
                flag: "--wat".into(),
                usage: usage(),
            },
            MetricsCliError::ExtraArgument {
                arg: "b".into(),
                usage: usage(),
            },
            MetricsCliError::MissingValue {
                flag: "--check".into(),
            },
            MetricsCliError::BaselineRead {
                path: "b.json".into(),
                detail: "missing".into(),
            },
            MetricsCliError::BaselineJson {
                detail: "bad".into(),
            },
            MetricsCliError::BaselineShape {
                detail: "shape".into(),
            },
            MetricsCliError::Serialize {
                detail: "boom".into(),
            },
            MetricsCliError::Mkdir {
                path: "d".into(),
                detail: "denied".into(),
            },
            MetricsCliError::Write {
                path: "o.json".into(),
                detail: "denied".into(),
            },
            MetricsCliError::RatchetFailed {
                detail: "regressions".into(),
            },
        ];
        for err in errors {
            assert!(!err.to_string().is_empty(), "{err:?} must render");
            let help =
                miette::Diagnostic::help(&err).expect("every failure variant carries help text");
            assert!(
                !help.to_string().is_empty(),
                "{err:?} help must be non-empty"
            );
        }
        // The help request is not a failure and deliberately has no help.
        let requested = MetricsCliError::HelpRequested { usage: usage() };
        assert!(miette::Diagnostic::help(&requested).is_none());
    }

    #[test]
    fn ratchet_failure_display_keeps_the_rendered_list() {
        let err = MetricsCliError::RatchetFailed {
            detail: "1 metric regression(s) against b.json:\n  - code_lines\n".into(),
        };
        assert!(err.to_string().starts_with("1 metric regression(s)"));
    }
}
