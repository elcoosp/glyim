//! CLI for the M0.6 metrics baseline.
//!
//! Measure a repo, write the JSON baseline, or check the current tree
//! against one (exit status 1 on any ratchet regression — this is what
//! CI's `metrics-ratchet` job runs).

use std::fmt::Write as _;
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

fn parse_args() -> Result<Args, String> {
    let mut root = None;
    let mut check = None;
    let mut write = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--check" => {
                check = Some(PathBuf::from(it.next().ok_or("--check needs a path")?));
            }
            "--write" => {
                write = Some(PathBuf::from(it.next().ok_or("--write needs a path")?));
            }
            "-h" | "--help" => return Err(usage()),
            other => {
                if other.starts_with('-') {
                    return Err(format!("unknown flag {other}\n{}", usage()));
                }
                if root.is_some() {
                    return Err(format!("unexpected extra argument {other}\n{}", usage()));
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

fn run() -> Result<String, String> {
    let args = parse_args()?;
    let current = scan_tree(&args.root);

    if let Some(path) = args.check {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read baseline {}: {e}", path.display()))?;
        let value: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("baseline is not JSON: {e}"))?;
        let baseline = RepoMetrics::from_json(&value)
            .map_err(|e| format!("baseline has unexpected shape: {e}"))?;
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
        return Err(msg);
    }

    let json =
        serde_json::to_string_pretty(&current.to_json()).map_err(|e| format!("serialize: {e}"))?;
    if let Some(path) = args.write {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, format!("{json}\n"))
            .map_err(|e| format!("write {}: {e}", path.display()))?;
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
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}
