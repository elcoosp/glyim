//! Crate root.
use clap::Parser;
use glyim_codegen::BytecodeBackend;
use glyim_codegen_llvm::LlvmBackend;
use glyim_codegen_llvm::passes::LtoKind;
use glyim_db::{CrateConfig, Database};
use std::path::PathBuf;

/// linker.
pub mod linker;

#[derive(Parser, Debug)]
#[command(name = "glyim", version, about = "The Glyim compiler")]
/// `CliArgs`.
pub struct CliArgs {
    #[arg(value_name = "INPUT")]
    /// `Struct`.
    pub input: PathBuf,
    #[arg(short, long)]
    /// `Struct`.
    pub output: Option<PathBuf>,
    #[arg(long, value_name = "EMIT", default_value = "obj")]
    /// `Struct`.
    pub emit: String,
    #[arg(short = 'O', long = "opt-level", default_value = "0")]
    /// `Struct`.
    pub opt_level: u8,
    #[arg(long = "target")]
    /// `Struct`.
    pub target: Option<String>,
    #[arg(long = "backend", default_value = "llvm")]
    /// `Struct`.
    pub backend: String,
    #[arg(long = "linker")]
    /// `Struct`.
    pub linker: Option<String>,
    #[arg(long = "link-flags")]
    /// `Struct`.
    pub link_flags: Option<String>,
    /// Link-time optimization strategy: `off` (default), `fat` (in-compiler
    /// module merge + optimize), or `thin` (tracked gap — requires linker
    /// driver integration; surfaces an explicit error rather than silently
    /// no-op). Phase 10.2.
    #[arg(long = "lto", default_value = "off")]
    pub lto: String,
    /// Number of codegen units (CGUs) to partition monomorphized items into
    /// for parallel code generation. Defaults to the available parallelism
    /// (capped at 16), mirroring rustc's `-C codegen-units` policy. (Phase
    /// 10.2 / feature-gaps §4.1.)
    #[arg(long = "codegen-units")]
    pub codegen_units: Option<usize>,
    /// Comma-separated list of proc-macro dependency crate source files. Each
    /// is compiled for the HOST triple to a cdylib (the two-stage proc-macro
    /// build, Phase 8 / plan §9.2) and `dlopen`ed via `glyim_proc_macro`,
    /// then merged into a single [`glyim_proc_macro::Registry`] that drives
    /// procedural-macro expansion of the primary crate.
    #[arg(long = "proc-macro-deps")]
    pub proc_macro_deps: Option<String>,
    /// Diagnostic output format: `human` (default, miette-rendered) or `json`
    /// (one JSON object per diagnostic, machine-readable for editors/LSP).
    /// Plan §3.4.
    #[arg(long = "error-format", default_value = "human")]
    pub error_format: String,
    /// Prepend the assembled Glyim standard library (plus a generated
    /// prelude of `use X::*;` statements) to the input source, so
    /// `println`, `Vec`, `Option`, `Result`, `String`, `Box`, `Rc`,
    /// `Iterator` and friends resolve without explicit `use` statements.
    /// Interim CLI-level equivalent of a default prelude; `glyip`'s
    /// project template will grow a native equivalent in v0.2.
    #[arg(long = "with-stdlib", default_value_t = false)]
    pub with_stdlib: bool,
    /// §3.5.30: suppress the named lint(s). Repeatable.
    #[arg(short = 'A', long = "allow", value_name = "LINT")]
    pub allow: Vec<String>,
    /// §3.5.30: set the named lint(s) to warning level (default).
    #[arg(short = 'W', long = "warn", value_name = "LINT")]
    pub warn: Vec<String>,
    /// §3.5.30: promote the named lint(s) to error level.
    #[arg(short = 'D', long = "deny", value_name = "LINT")]
    pub deny: Vec<String>,
    /// Print a per-phase wall-clock table (load/parse/expand/…/codegen) to
    /// stderr after a successful compile. Plan M2.1: `--time-passes`.
    /// On a compile-cache hit nothing recompiles, so only a note is
    /// printed instead of a table.
    #[arg(long = "time-passes")]
    pub time_passes: bool,
    /// Write the per-phase profile as a Chrome Trace JSON event list
    /// (openable in Perfetto / `chrome://tracing`) to `FILE`, defaulting
    /// to `trace.json` in the working directory. Plan M2.1:
    /// `--self-profile`. The value is `=`-bound (`--self-profile=t.json`)
    /// so the positional INPUT can never be swallowed as the filename;
    /// a bare `--self-profile` writes `./trace.json`. Never written on a
    /// compile-cache hit.
    #[arg(long = "self-profile", value_name = "FILE", require_equals = true, num_args = 0..=1, default_missing_value = "trace.json")]
    pub self_profile: Option<PathBuf>,
}

/// Prepend the assembled minimal standard library to `input`'s VFS content.
/// After this call the pipeline reads `stdlib + \n + user_source` instead of
/// the raw file, which puts `println`, `Vec`, `Option`, `Result`, … in scope
/// without explicit `use` statements.
///
/// Public so `glyip build`/`glyip run` can share the same path as
/// `glyim-cli --with-stdlib`.
pub fn inject_assembled_stdlib(
    db: &mut Database,
    input: &std::path::Path,
) -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {
    let user = std::fs::read_to_string(input).map_err(|e| {
        vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
            "read {}: {e}",
            input.display()
        ))]
    })?;
    let stdlib = glyim_lang_std::std_source_assembled_minimal();
    // NOTE: we deliberately do NOT emit a `use X::*;` prelude here.
    // `std_source_assembled` already emits a `pub use X::Item;` block at
    // the crate root for every user-facing name, so bare `Option`,
    // `Result`, `Iterator`, `println`, … already resolve in the appended
    // user source. Adding a `use iter::*;`-style glob on top re-imports
    // those same names via a second path and shadows the module-local
    // generic definitions (`Once<T>`, `Repeat<T>`, …), causing the
    // "generic type `Once` expects 0 type argument(s)" cascade.
    let combined = format!("{stdlib}\n{user}\n");
    db.vfs()
        .add_file_content(input, std::sync::Arc::from(combined));
    Ok(())
}

/// CLI entry point: parses argv (honouring `-W help` before clap enforces
/// the required `<INPUT>` argument), dispatches the requested emit mode,
/// and maps compile failures onto the process exit code.
///
/// On success the payload is the **process exit code** the binary should
/// terminate with: 0 for every code-producing mode, and the interpreted
/// program's own (masked) exit code for `--emit=run` — so
/// `glyim --emit=run prog.g` is scriptable exactly like running a linked
/// `--emit=exec` binary. `Err` still means "driver failure" (compile
/// error, CLI misuse); the diagnostics have already been printed in the
/// selected `--error-format` by the time `Err` is returned.
pub fn run() -> Result<i32, Vec<glyim_diag::GlyimDiagnostic>> {
    // §3.5.30 framework: `-W help` must be answered before clap enforces
    // the required `<INPUT>` arg — otherwise `glyim -W help` errors out
    // instead of listing lints. Scan argv directly, then return.
    {
        let argv: Vec<String> = std::env::args().collect();
        for i in 0..argv.len() {
            let a = argv[i].as_str();
            let help_requested = match a {
                "-W" | "--warn" => argv.get(i + 1).is_some_and(|s| s == "help"),
                _ => a == "-W=help" || a == "--warn=help",
            };
            if help_requested {
                eprintln!("Available lints:");
                for id in glyim_core::lint::LintId::ALL {
                    eprintln!("  {}", id.as_str());
                }
                eprintln!();
                eprintln!(
                    "Aliases: `unsafe` = `unsafe_fn_call`, `unused_generics` = `unused_generic_params`."
                );
                return Ok(0);
            }
        }
    }

    // Capture args (incl. `error-format`) before entering the panic catcher,
    // so we can print any returned diagnostics in the requested format.
    let cli_args = CliArgs::parse();
    let error_format = cli_args.error_format.clone();

    // T-ICE-6: install a hook that *silences* the default panic printer and
    // records the payload. During catch_unwind the default hook would print
    // a raw `thread 'main' panicked at …` line on top of our ICE report; the
    // ICE report itself is the correct user-facing output, and it should
    // contain the panic message.
    // Capture BOTH the panic message AND the backtrace inside the hook.
    // `Backtrace::force_capture()` at the post-unwind point (the previous
    // design) captured the *unwound* stack — the frames from
    // `catch_unwind` down to the ICE reporter, NOT the panic site. That
    // made every ICE report's backtrace useless (session 33 discovered
    // this while diagnosing §3.9/§3.10). Inside `set_hook`, the stack is
    // still at the panic site.
    let panic_message: std::sync::Arc<
        std::sync::Mutex<Option<(String, std::backtrace::Backtrace)>>,
    > = std::sync::Arc::new(std::sync::Mutex::new(None));
    {
        let sink = panic_message.clone();
        std::panic::set_hook(Box::new(move |info| {
            let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = info.payload().downcast_ref::<String>() {
                s.clone()
            } else {
                "<non-string panic payload>".to_string()
            };
            let bt = std::backtrace::Backtrace::force_capture();
            if let Ok(mut slot) = sink.lock() {
                *slot = Some((msg, bt));
            }
        }));
    }

    // Plan §3.1.2b: catch unexpected panics in the driver and turn them into a
    // structured ICE report instead of a raw Rust backtrace. A language
    // toolchain must never crash on user input with an unreadable stack trace —
    // an ICE with the compiler version + input + backtrace path is the
    // production-grade behaviour (rustc's `RUST_BACKTRACE` model).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_with_args(cli_args)));
    let _ = std::panic::take_hook();
    match result {
        Ok(inner) => {
            if let Err(diags) = &inner {
                print_diagnostics(diags, &error_format);
            }
            inner
        }
        Err(_payload) => {
            let (panic_msg, backtrace) = panic_message
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .unwrap_or_else(|| {
                    (
                        "<unknown>".to_string(),
                        std::backtrace::Backtrace::force_capture(),
                    )
                });
            let report = format!(
                "glyim internal compiler error (ICE): the compiler panicked.\n\
                 version: {}\n\
                 panic message: {}\n\
                 Please file a bug report. Backtrace saved below.\n\n{:?}",
                env!("CARGO_PKG_VERSION"),
                panic_msg,
                backtrace,
            );
            // P1-O4 (KNOWN_GAPS §5): use a unique ICE path instead of the
            // fixed `glyim-ice.txt` (which two concurrent compilations would
            // overwrite for each other, and which a symlink could redirect).
            // `tempfile::Builder::keep` creates a unique file with the given
            // prefix and returns its path.
            let ice_path = tempfile::Builder::new()
                .prefix("glyim-ice-")
                .suffix(".txt")
                .tempfile()
                .ok()
                .and_then(|f| f.keep().ok().map(|(_, p)| p));
            let path_display = if let Some(p) = &ice_path {
                let _ = std::fs::write(p, &report);
                p.display().to_string()
            } else {
                // Fallback if tempfile fails — still try the old path.
                let p = std::env::temp_dir().join("glyim-ice.txt");
                let _ = std::fs::write(&p, &report);
                p.display().to_string()
            };
            eprintln!(
                "error: glyim panicked (ICE). Backtrace saved to {path_display}:

please file a bug report at <https://github.com/elcoosp/glyim/issues> \
with the input file and the backtrace."
            );
            // P1-O4: an ICE is a compiler bug, not a user error — exit 101
            // (Rust's conventional ICE code) so scripts can distinguish it
            // from a compile error (1) or CLI misuse (2). The message above
            // is the user-facing signal; the returned diagnostics are kept
            // for API compatibility but are not re-printed.
            let _ = glyim_diag::GlyimDiagnostic::internal_error(report);
            std::process::exit(101);
        }
    }
}

/// Print `diags` in the requested format (plan §3.4). `human` renders each via
/// its `Display` impl (miette-styled); `json` emits one JSON object per
/// diagnostic on its own line, suitable for editor/LSP consumption.
fn print_diagnostics(diags: &[glyim_diag::GlyimDiagnostic], format: &str) {
    if format == "json" {
        // P1-O5 (KNOWN_GAPS §5): JSON diagnostics schema v1.
        //
        // Each diagnostic is emitted as **one JSON object per line** on
        // **stdout** (Rust's `--message-format=json` convention — a
        // consumer piping stdout gets clean, machine-parseable records;
        // stderr is reserved for human-readable notes and is empty in
        // JSON mode).
        //
        // Schema:
        //   {
        //     "version": 1,                // bump on incompatible change
        //     "code": "T0001",             // ErrorCode Display (unchanged)
        //     "severity": "error",         // lowercase; error/warning/note/help
        //     "message": "...",
        //     "spans": [
        //       {
        //         "file": 4294967295,      // FileId raw (mirrors the
        //                                  // compiler's internal id — LSP
        //                                  // maps it back to a path via
        //                                  // the file's SourceMap, same as
        //                                  // `--error-format=human`'s
        //                                  // `FileId(N)` display)
        //         "lo": 114788,            // byte offset
        //         "hi": 114805
        //       }, ...
        //     ]
        //   }
        //
        // Previously: no `version`, no `file`, severity was `Debug`
        // (capital "Error" instead of lowercase — mismatched against
        // common editor / CI JSON schemas), and the output went to
        // stderr (Rust puts it on stdout).
        for d in diags {
            let spans: Vec<serde_json::Value> = std::iter::once(&d.span.primary)
                .chain(d.span.secondary.iter().map(|(s, _)| s))
                .map(|s| {
                    let r = s.range();
                    serde_json::json!({
                        "file": s.file.to_raw(),
                        "lo": r.start,
                        "hi": r.end,
                    })
                })
                .collect();
            let severity_str = match d.severity {
                glyim_diag::DiagSeverity::Error => "error",
                glyim_diag::DiagSeverity::Warning => "warning",
                glyim_diag::DiagSeverity::Note => "note",
                glyim_diag::DiagSeverity::Help => "help",
            };
            let obj = serde_json::json!({
                "version": 1,
                "code": format!("{}", d.code),
                "severity": severity_str,
                "message": d.message,
                "spans": spans,
            });
            println!("{obj}");
        }
    } else {
        for d in diags {
            eprintln!("{d}");
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmitKind {
    Obj,
    Exec,
    Mir,
    LlvmIr,
    Syntax,
    Asm,
    /// Emit a position-independent shared library (cdylib). Phase 9.2: this is
    /// the host artifact a procedural-macro crate compiles to so it can be
    /// `dlopen`ed by `glyim_proc_macro::load_cdylib` during macro expansion.
    Cdylib,
    /// Emit a schema-versioned JSON measurement report (per-phase wall
    /// clock + mono/warning counters) instead of code. Plan M2.1:
    /// `--emit=stats` runs the analysis-only front half — no backend
    /// executes, so no object is produced.
    Stats,
    /// Execute the program in-process through the MIR interpreter
    /// (M2.4). No object, no linker, no `glyim-runtime` staticlib, no
    /// LLVM toolchain: the same portable run path the test harness uses.
    /// The program's exit code (masked to `0xff`, panics → 101) becomes
    /// the driver's exit code, exactly as for a linked `--emit=exec`
    /// binary. Only valid with `--backend=interp` (or the default,
    /// which the run path treats as "use the interpreter").
    Run,
}

impl EmitKind {
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "obj" => Ok(EmitKind::Obj),
            "exec" => Ok(EmitKind::Exec),
            "mir" => Ok(EmitKind::Mir),
            "llvm-ir" => Ok(EmitKind::LlvmIr),
            "syntax" => Ok(EmitKind::Syntax),
            "asm" => Ok(EmitKind::Asm),
            "cdylib" => Ok(EmitKind::Cdylib),
            "stats" => Ok(EmitKind::Stats),
            "run" => Ok(EmitKind::Run),
            _ => Err(format!(
                "invalid value for --emit: '{s}' (expected one of: obj, exec, mir, llvm-ir, asm, cdylib, syntax, stats, run)"
            )),
        }
    }
}

pub(crate) fn run_with_args(args: CliArgs) -> Result<i32, Vec<glyim_diag::GlyimDiagnostic>> {
    // M2.4: the success payload is the **process exit code** the driver
    // should terminate with. Every code-producing mode returns 0; `run`
    // mode returns the interpreted program's own (masked) exit code so
    // `glyim --emit=run prog.g` is scriptable exactly like running a
    // linked `--emit=exec` binary.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .ok();

    let input = &args.input;
    let emit = match EmitKind::from_str(&args.emit) {
        Ok(k) => k,
        Err(msg) => {
            return Err(vec![glyim_diag::GlyimDiagnostic::parse_error(
                glyim_diag::Span::DUMMY,
                msg,
            )]);
        }
    };

    // Determine output paths based on emit mode
    let (object_path, final_output_path) = match emit {
        EmitKind::Obj => {
            let obj = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("o");
                p
            });
            (obj.clone(), Some(obj))
        }
        EmitKind::Exec => {
            // For an executable, the LLVM backend writes a *relocatable
            // object*, which the linker then links into the final binary.
            // These must be distinct paths: `link_with_args` opens its
            // output with truncation before reading the input, so using the
            // same path for both truncates the object to zero bytes before
            // the linker reads it (`ld: file is empty in <path>`). Derive
            // the object path from the final output (or the source) with an
            // `.o` extension so the two never collide.
            let final_out = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("");
                p
            });
            let obj = final_out.with_extension("o");
            (obj, Some(final_out))
        }
        EmitKind::Cdylib => {
            // Compile to a private object, then link it into a shared library.
            let obj = {
                let mut p = input.clone();
                p.set_extension("o");
                p
            };
            let final_out = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension(if cfg!(target_os = "macos") {
                    "dylib"
                } else {
                    "so"
                });
                p
            });
            (obj, Some(final_out))
        }
        EmitKind::Mir | EmitKind::LlvmIr | EmitKind::Syntax => {
            let out = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                let ext = match emit {
                    EmitKind::Mir => "mir",
                    EmitKind::Syntax => "syntax",
                    _ => "ll",
                };
                p.set_extension(ext);
                p
            });
            (out, None)
        }
        EmitKind::Stats => {
            // Machine-readable report: default to `<input>.stats.json` so
            // the file never collides with a source or object file.
            let out = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("stats.json");
                p
            });
            (out, None)
        }
        EmitKind::Run => {
            // M2.4: run mode executes in-process and writes no artifact.
            // `--output` is rejected below, so this path is never written;
            // the placeholder keeps the `(object_path, final_output_path)`
            // shape uniform for the early-return branch that follows.
            let placeholder = {
                let mut p = input.clone();
                p.set_extension("run");
                p
            };
            (placeholder, None)
        }
        EmitKind::Asm => {
            let out = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("s");
                p
            });
            (out, None)
        }
    };

    // Script 664: default to the *host* triple via the existing
    // `host_target_triple()` helper (line 626). The previous hardcoded
    // `x86_64-unknown-linux-gnu` produced an ELF object on every host,
    // so the local `cc`/`ld64` on macOS/Windows refused to link
    // (`ld: unknown file type`). Falls back inside the helper to the
    // historical default if `rustc -vV` cannot be queried.
    let target_triple = args.target.clone().unwrap_or_else(host_target_triple);

    // Parse the requested LTO strategy (Phase 10.2). `Thin` is a tracked gap
    // (linker-driver integration); surface it as an explicit error rather than
    // silently degrading to a no-op.
    let lto = match args.lto.as_str() {
        "off" | "none" | "false" => LtoKind::None,
        "fat" => LtoKind::Fat,
        "thin" => LtoKind::Thin,
        other => {
            return Err(vec![glyim_diag::GlyimDiagnostic::parse_error(
                glyim_diag::Span::DUMMY,
                format!("invalid value for --lto: '{other}' (expected one of: off, fat, thin)"),
            )]);
        }
    };

    let config = CrateConfig {
        name: args
            .input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        target_triple: target_triple.clone(),
        opt_level: args.opt_level,
    };

    // M2.4: `--emit=run` mode validation. The interpreter is the run
    // backend; the bytecode backend is a *codegen* backend whose raw
    // opcode output has nothing to do with run mode, and `--output` has
    // no artifact to name. Both are explicit errors rather than silent
    // no-ops, mirroring the `--backend=bytecode` + `--emit=exec` rule.
    if emit == EmitKind::Run {
        if args.backend == "bytecode" {
            return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(
                "`--backend=bytecode` cannot run: the bytecode backend is a codegen \
                 backend and `--emit=run` executes in-process through the MIR \
                 interpreter, writing no artifact. Drop `--backend=bytecode` (or \
                 use `--backend=interp` explicitly), or use `--emit=obj` to emit \
                 bytecode modules.",
            )]);
        }
        if args.output.is_some() {
            return Err(vec![
                glyim_diag::GlyimDiagnostic::internal_error(
                    "`--emit=run` executes the program in-process and writes no \
                 artifact, so `--output` does not apply. Remove `--output`, or \
                 use `--emit=obj`/`--emit=exec` to produce a file.",
                )
                .with_suggestion(glyim_diag::Suggestion {
                    message: "drop --output; run mode's observable output is the \
                          program's stdout/stderr and its exit code"
                        .to_string(),
                    replacements: Vec::new(),
                    applicability: glyim_diag::Applicability::MaybeIncorrect,
                }),
            ]);
        }
    }
    // M2.4: `--backend=interp` names the in-process interpreter. It does
    // not emit code, so any other emit kind is an explicit error pointing
    // at the one combination it supports.
    if args.backend == "interp" && emit != EmitKind::Run {
        return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(
            "`--backend=interp` executes through the in-process MIR interpreter and \
             does not emit code, so it is only valid with `--emit=run`. Use \
             `--emit=run` to execute, or drop `--backend=interp` for a \
             code-producing emit kind (llvm is the default backend).",
        )]);
    }

    let mut db = Database::new(config);
    // §3.5.30 framework: translate `-A/-W/-D <lint>`.
    {
        let mut lint_map = glyim_core::lint::LintMap::new();
        glyim_core::lint::apply_named(
            &mut lint_map,
            &args.allow,
            glyim_core::lint::LintLevel::Allow,
        );
        glyim_core::lint::apply_named(&mut lint_map, &args.warn, glyim_core::lint::LintLevel::Warn);
        glyim_core::lint::apply_named(&mut lint_map, &args.deny, glyim_core::lint::LintLevel::Deny);
        db.set_lints(lint_map);
    }
    // Tier 1.1: --with-stdlib prepends the assembled stdlib + prelude
    if args.with_stdlib
        && let Err(diags) = inject_assembled_stdlib(&mut db, input)
    {
        for d in diags {
            eprintln!("{}", d.message);
        }
        return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(
            "--with-stdlib: failed to inject assembled stdlib",
        )]);
    }

    // Phase 8 / plan §9.2: if the user listed proc-macro dependency crates,
    // run the two-stage proc-macro build (compile each for the HOST triple to
    // a cdylib, then dlopen and merge into one Registry) so the primary crate's
    // procedural-macro invocations can be expanded during compilation. The
    // registry is `None` when no deps are supplied, leaving the pipeline's
    // behavior unchanged (no expansion pass runs).
    let proc_registry: Option<glyim_proc_macro::Registry> = match &args.proc_macro_deps {
        Some(list) if !list.trim().is_empty() => {
            let deps: Vec<std::path::PathBuf> = list
                .split(',')
                .map(|s| std::path::PathBuf::from(s.trim()))
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
            match build_proc_macro_dependencies(&deps) {
                Ok(reg) => Some(reg),
                Err(e) => {
                    return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                        "proc-macro dependency build failed: {e}"
                    ))]);
                }
            }
        }
        _ => None,
    };

    // Early return for MIR, LLVM IR, and assembly emit
    // T127-PATCHED [CLI-3]: pass the proc-macro registry through to the
    // emit helpers. Previously they hardcoded `None`, so a compile with
    // `--proc-macro-deps foo.g --emit=llvm-ir` paid for the cdylib build
    // and dlopen and then produced IR without macro expansion.
    let proc_registry_ref = proc_registry.as_ref();
    if emit == EmitKind::Stats {
        // M2.1: analysis-only measurement — run the front half, write the
        // schema-versioned JSON report, and honour --time-passes /
        // --self-profile exactly like the code-producing modes.
        let report = glyim_pipeline::compile_stats(&mut db, input, proc_registry_ref)?;
        std::fs::write(&object_path, report.render_json()).map_err(|e| {
            vec![
                glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "--emit=stats: failed to write {}: {e}",
                    object_path.display()
                ))
                .with_suggestion(glyim_diag::Suggestion {
                    message: format!(
                        "point --output at a writable path (the parent directory of {} must exist)",
                        object_path.display()
                    ),
                    replacements: Vec::new(),
                    applicability: glyim_diag::Applicability::MaybeIncorrect,
                }),
            ]
        })?;
        write_profile_outputs(&args, &report.profile)?;
        return Ok(0);
    }
    if emit == EmitKind::Mir {
        let profile = glyim_pipeline::emit_mir(&mut db, input, &object_path, proc_registry_ref)?;
        write_profile_outputs(&args, &profile)?;
        return Ok(0);
    } else if emit == EmitKind::Syntax {
        let profile = glyim_pipeline::emit_syntax(&mut db, input, &object_path, proc_registry_ref)?;
        write_profile_outputs(&args, &profile)?;
        return Ok(0);
    } else if emit == EmitKind::LlvmIr {
        let profile =
            glyim_pipeline::emit_llvm_ir(&mut db, input, &object_path, proc_registry_ref)?;
        write_profile_outputs(&args, &profile)?;
        return Ok(0);
    } else if emit == EmitKind::Asm {
        let profile = glyim_pipeline::emit_asm(&mut db, input, &object_path, proc_registry_ref)?;
        write_profile_outputs(&args, &profile)?;
        return Ok(0);
    } else if emit == EmitKind::Run {
        // M2.4: in-process execution. Derives from the shared front half
        // via `compile_file_to_mir_with_proc_registry` (the same
        // "every mode derives from one prepared compilation" rule the
        // other emit modes follow), resolves `main` through the same
        // registry-threaded entry pre-pass the `--emit=exec` path uses,
        // and hands the monomorphized bodies to the canonical
        // `glyim_mir_interp::run_program` process contract shared with
        // the test harness's `InterpRunner`.
        let entry_main = glyim_pipeline::Pipeline::entry_main_local_id_with_proc_registry(
            &mut db,
            input,
            proc_registry_ref,
        );
        let Some(entry_main) = entry_main else {
            return Err(vec![
                glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "--emit=run: no `fn main` found in {}; the interpreter needs an \
                 entry point to execute",
                    input.display()
                ))
                .with_suggestion(glyim_diag::Suggestion {
                    message: "define `fn main` in the program, or use another --emit kind \
                          (obj/exec/mir/…) to compile this file without running it"
                        .to_string(),
                    replacements: Vec::new(),
                    applicability: glyim_diag::Applicability::MaybeIncorrect,
                }),
            ]);
        };
        let compilation = glyim_pipeline::compile_file_to_mir_with_proc_registry(
            &mut db,
            input,
            proc_registry_ref,
        )?;
        // Same warning-rendering rule as the code-producing modes: a
        // successful compile's non-error diagnostics must reach the user.
        print_diagnostics(&compilation.warnings, &args.error_format);
        write_profile_outputs(&args, &compilation.profile)?;
        let bodies: Vec<_> = compilation.bodies.values().cloned().collect();
        let outcome =
            glyim_mir_interp::run_program(&bodies, compilation.ty_ctx.as_ref(), Some(entry_main));
        tracing::debug!(
            exit_code = outcome.exit_code,
            kind = ?outcome.kind,
            "interpreted program finished"
        );
        // Forward the program's streams verbatim, then adopt its exit
        // code: `glyim --emit=run prog.g` is scriptable like the linked
        // `--emit=exec` binary. stdout is flushed before stderr so the
        // two never interleave across buffers.
        {
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = out.write_all(&outcome.stdout);
            let _ = out.flush();
            let mut err_out = std::io::stderr();
            let _ = err_out.write_all(&outcome.stderr);
            let _ = err_out.flush();
        }
        return Ok(outcome.exit_code);
    }

    // Build the LLVM backend concretely so the Thin path can drive per-CGU
    // bitcode emission (`emit_thinlto_bitcode_files`) directly. For the
    // non-Thin paths it is boxed into the `CodegenBackend` trait object.
    // P0-2 (production-readiness plan): thread the proc registry into the
    // entry pre-pass. Without it, an item-emitting proc macro shifts the
    // LocalDefId numbering so the id returned here points at the wrong
    // item — miscompiling `main`.
    let entry_main = glyim_pipeline::Pipeline::entry_main_local_id_with_proc_registry(
        &mut db,
        input,
        proc_registry_ref,
    );
    let target_info = glyim_core::TargetInfo::from_triple(&target_triple);
    let mut llvm = LlvmBackend::with_db(&db)
        .with_target(&target_triple)
        .with_opt_level(args.opt_level)
        .with_opt_for_size(false)
        .with_lto(lto);
    // P2-A6 (KNOWN_GAPS §8.1): only emit a C-ABI `main` entry symbol for
    // `--emit=exec`. A cdylib or plain object must not carry a `main` — it
    // would be an unused/conflicting entry point, and `obj`/`cdylib`
    // consumers link `main` themselves if needed. Before this fix the
    // match included `EmitKind::Obj`, so `--emit=obj` produced an object
    // with `main` (contradicting this comment), AND that object shared a
    // compile-cache key with `--emit=cdylib` (whose cache key records
    // `has_entry_main=false` because the key only checks `Exec`) —
    // meaning a cached `--emit=cdylib` object and a cached `--emit=obj`
    // object with the *same input* would be silently mixed up.
    if let (Some(main_id), EmitKind::Exec) = (entry_main, emit) {
        llvm = llvm.with_entry_main(main_id);
    }

    // ---- Incremental cache (obj / exec, non-Thin) ----
    //
    // A whole-crate content-addressed cache. The key is the *flattened* crate
    // source (external modules already spliced in), the target triple, the
    // optimization level, and whether a C-ABI `main` wrapper was requested —
    // every input that changes the produced object. On a hit we copy the
    // cached object into `object_path` and skip the entire pipeline. On a
    // miss we compile as usual and store the result.
    //
    // Scope: only `obj`/`exec` on the LLVM backend without ThinLTO. The
    // `mir`/`llvm-ir`/`asm` paths return above; ThinLTO produces per-CGU
    // bitcode whose combination depends on the link driver, which is out of
    // this first rung's scope.
    let cacheable = matches!(emit, EmitKind::Obj | EmitKind::Exec)
        && args.backend != "bytecode"
        && lto != LtoKind::Thin;
    let cache = if cacheable {
        Some(glyim_db::cache::CompileCache::open_default())
    } else {
        None
    };
    let mut cache_key: Option<String> = None;
    if cacheable && let Some(c) = cache.as_ref() {
        // T128-PATCHED [CLI-4]: hash each proc-macro dependency file's
        // bytes into the cache key. Also mix the LTO kind so fat/incremental
        // (once implemented) do not serve cross-config objects.
        let mut extra_key_bytes: Vec<u8> = Vec::new();
        if let Some(list) = args.proc_macro_deps.as_deref() {
            for p in list.split(',').map(str::trim) {
                if p.is_empty() {
                    continue;
                }
                extra_key_bytes.extend_from_slice(b"\x1fpm\x1f");
                extra_key_bytes.extend_from_slice(p.as_bytes());
                extra_key_bytes.extend_from_slice(b"\x1f");
                if let Ok(bytes) = std::fs::read(p) {
                    extra_key_bytes.extend_from_slice(&bytes);
                }
            }
        }
        // P1-O2 (production-readiness plan): codegen_units affects the
        // emitted object, so include it in the cache key. Without this a
        // build with a different `--codegen-units` silently serves the
        // previous object.
        extra_key_bytes.extend_from_slice(b"\x1fcgu\x1f");
        extra_key_bytes.extend_from_slice(&args.codegen_units.unwrap_or(0).to_le_bytes());
        extra_key_bytes.extend_from_slice(b"\x1flto\x1f");
        extra_key_bytes.push(match lto {
            glyim_codegen_llvm::passes::LtoKind::None => 0,
            glyim_codegen_llvm::passes::LtoKind::Thin => 1,
            glyim_codegen_llvm::passes::LtoKind::Fat => 2,
        });
        // §3.5.30 framework: mix the lint config into the cache key.
        // Without this, `-A unsafe`/`-D unsafe` on an already-cached
        // source silently serves the previously-built object and the
        // flag has no effect. Lints are a SORTED list of `(id, level)`
        // pairs so flag ordering (`-A x -D y` vs `-D y -A x`) does not
        // affect the key.
        {
            let mut lint_entries: Vec<(u8, u8)> = db
                .lints()
                .iter()
                .map(|(id, lvl)| {
                    let id_b = match id {
                        glyim_core::lint::LintId::UnsafeFnCall => 0u8,
                        glyim_core::lint::LintId::UnusedGenericParams => 1u8,
                    };
                    let lvl_b = match lvl {
                        glyim_core::lint::LintLevel::Allow => 0u8,
                        glyim_core::lint::LintLevel::Warn => 1u8,
                        glyim_core::lint::LintLevel::Deny => 2u8,
                    };
                    (id_b, lvl_b)
                })
                .collect();
            lint_entries.sort_unstable();
            extra_key_bytes.extend_from_slice(b"\x1flints\x1f");
            for (id_b, lvl_b) in &lint_entries {
                extra_key_bytes.push(*id_b);
                extra_key_bytes.push(*lvl_b);
            }
        }
        let key = glyim_pipeline::Pipeline::compute_cache_key(
            &mut db,
            input,
            &target_triple,
            args.opt_level,
            matches!(emit, EmitKind::Exec) && entry_main.is_some(),
            &extra_key_bytes,
        );
        if let Some(k) = key {
            if let Some(hit) = c.lookup(&k) {
                // Copy the cached object to the requested path. Best effort:
                // any I/O error falls through to a fresh compile.
                if std::fs::copy(&hit, &object_path).is_ok() {
                    // Unconditional stderr message: a user piping stdout wants
                    // to know why nothing recompiled, and this must not depend
                    // on `RUST_LOG` being configured. Matches Cargo's `Fresh`
                    // line on stderr.
                    eprintln!("cache hit — skipping compilation");
                    // Skip the compile blocks; jump straight to link (if any).
                    return finalize_after_object(
                        emit,
                        &object_path,
                        final_output_path.as_deref(),
                        &args,
                    )
                    .map(|()| 0);
                }
            }
            cache_key = Some(k);
        }
    }

    // ThinLTO: emit one bitcode file per codegen unit, then run the thin-link
    // driver (`thin_lto_link`, which shells out to `llvm-lto2`) to combine them
    // incrementally. The thin-linked object is written to `object_path` for the
    // final linker step below. `compile_file_with_artifacts` runs the full
    // pipeline; its internal `generate` call short-circuits for Thin (see
    // `LlvmBackend::generate`) so it does not write a redundant merged object.
    if lto == LtoKind::Thin {
        if args.backend == "bytecode" {
            return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(
                "ThinLTO requires the LLVM backend; `--backend=bytecode` cannot emit \
                 per-CGU bitcode. Use the default (LLVM) backend for `--lto=thin`.",
            )]);
        }
        let artifacts = glyim_pipeline::Pipeline::compile_file_with_artifacts(
            &mut db,
            input,
            &llvm,
            &object_path,
            args.codegen_units,
            proc_registry.as_ref(),
        )?;
        print_diagnostics(&artifacts.warnings, &args.error_format);
        write_profile_outputs(&args, &artifacts.profile)?;
        let bitcode_dir = object_path.with_extension("thin-bc");
        std::fs::create_dir_all(&bitcode_dir).map_err(|e| {
            vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                "ThinLTO: failed to create bitcode dir {}: {}",
                bitcode_dir.display(),
                e
            ))]
        })?;
        let bitcode_paths = llvm
            .emit_thinlto_bitcode_files(&artifacts.mir_bodies, &bitcode_dir)
            .map_err(|e| {
                vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "ThinLTO per-CGU bitcode emission failed: {e:?}"
                ))]
            })?;
        let thin_objects = linker::thin_lto_link(&bitcode_paths, args.opt_level, &bitcode_dir)
            .map_err(|e| {
                vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "ThinLTO thin-link failed: {e}"
                ))]
            })?;
        if thin_objects.is_empty() {
            return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(
                "ThinLTO thin-link produced no object files",
            )]);
        }
        // `llvm-lto2 run` can emit one object per output module. The rest
        // of the compile pipeline (cache, `finalize_after_object`, the
        // linker invocation) is written against a single `object_path`, so
        // when the thin-link produces more than one object we merge them
        // into a single relocatable object with `ld -r` before writing
        // `object_path`. That keeps the cache and link paths uniform and
        // guarantees no post-thin-link object is dropped.
        if thin_objects.len() == 1 {
            std::fs::copy(&thin_objects[0], &object_path).map_err(|e| {
                vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "ThinLTO: failed to copy thin-linked object to {}: {}",
                    object_path.display(),
                    e
                ))]
            })?;
        } else {
            // Merge: `link_with_args` runs the platform linker with `-r`
            // (partial link) so the resulting `object_path` is a single
            // relocatable object containing every thin-linked module.
            let mut merge_args = linker::LinkArgs::default();
            merge_args
                .objects
                .extend(thin_objects.iter().skip(1).cloned());
            merge_args.user_flags.push("-r".to_string());
            linker::link_with_args(
                &thin_objects[0],
                &object_path,
                &merge_args,
                args.linker.as_deref(),
                args.target.as_deref(),
            )
            .map_err(|e| {
                let annotated = linker::annotate_link_error_if_stale_runtime(e);
                vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "ThinLTO: failed to merge {} thin-linked objects into {}: {}",
                    thin_objects.len(),
                    object_path.display(),
                    annotated
                ))]
            })?;
        }
    } else if args.backend == "bytecode" {
        if args.opt_level > 0 {
            tracing::warn!(
                "bytecode backend opt-level currently has no effect; reserved for future peephole passes"
            );
        }
        // The bytecode backend reads layouts through the *shared*
        // `TyCtxHandle` the pipeline publishes, so it must be constructed
        // with that handle — not with an empty `TyCtxMut::new().freeze()`
        // (which was the source of the `type_arena::ty_kind` OOB: every
        // `layout_of` indexed past the end of the empty arena). The
        // pipeline populates the handle during `compile_file`, which runs
        // after this constructor.
        let backend: Box<dyn glyim_codegen::CodegenBackend> = Box::new(
            BytecodeBackend::with_ty_ctx_handle(db.ty_ctx_handle(), target_info),
        );
        let artifacts = glyim_pipeline::Pipeline::compile_file_with_artifacts(
            &mut db,
            input,
            &*backend,
            &object_path,
            args.codegen_units,
            proc_registry.as_ref(),
        )?;
        print_diagnostics(&artifacts.warnings, &args.error_format);
        write_profile_outputs(&args, &artifacts.profile)?;
    } else {
        let backend: Box<dyn glyim_codegen::CodegenBackend> = Box::new(llvm);
        let artifacts = glyim_pipeline::Pipeline::compile_file_with_artifacts(
            &mut db,
            input,
            &*backend,
            &object_path,
            args.codegen_units,
            proc_registry.as_ref(),
        )?;
        print_diagnostics(&artifacts.warnings, &args.error_format);
        write_profile_outputs(&args, &artifacts.profile)?;
    }

    // Store the produced object in the cache for next time. Only reached
    // when the pipeline ran (a cache hit returned above), only on the
    // cacheable paths, and only after the compile succeeded (the `?` above
    // would have propagated any error before this point).
    if let (Some(c), Some(k)) = (cache.as_ref(), cache_key.as_ref())
        && object_path.exists()
    {
        c.store(k, &object_path);
    }

    // P1-C10 (production-readiness plan §6 / §13): the bytecode backend emits
    // raw VM opcode bytes, not a relocatable object. Passing that to `cc`
    // produces a bare "ld: unknown file type" error — an unhelpful leak of
    // backend internals. Reject the combination up front with a documented
    // message, mirroring the thin-LTO/bytecode check above. The VM itself
    // (correctness, wiring `glyim run --backend bytecode`) is a Phase 4
    // concern; today the honest outcome is a clean error.
    if args.backend == "bytecode" && matches!(emit, EmitKind::Exec | EmitKind::Cdylib) {
        return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(
            "`--backend=bytecode` cannot produce an executable or cdylib: it emits \
             raw VM opcode bytes, not a relocatable object, so the system linker \
             rejects the result. Use the default (LLVM) backend for `--emit=exec` \
             or `--emit=cdylib`, or `--emit=run` to execute in-process through \
             the interpreter without linking. (The bytecode backend is available \
             for `--emit=obj` as a testing tool — see KNOWN_GAPS §6 P1-C10.)",
        )]);
    }

    if emit == EmitKind::Exec || emit == EmitKind::Cdylib {
        let final_path = final_output_path.expect("emit should have final output");
        // `--emit=exec` binaries need the `glyim-runtime` static library for
        // the stdlib's `extern "C"` hooks (`glyim_stdout_write`,
        // `glyim_errno`, …). Add it as an extra object, before any
        // user-supplied flags. `-shared` for cdylib.
        // The runtime staticlib provides the stdlib's `extern "C"` hooks
        // (`glyim_stdout_write`, `glyim_errno`, `glyim_alloc`, …). Without it
        // the link fails with a cryptic `undefined _glyim_stdout_write`; report
        // the real cause instead and say how to fix it.
        let runtime_lib = linker::find_runtime_staticlib().ok_or_else(|| {
            vec![glyim_diag::GlyimDiagnostic::internal_error(
                "`--emit=exec`/`--emit=cdylib` needs the `glyim-runtime` static \
                 library (`libglyim_runtime.a`), which provides the stdlib's \
                 `extern \"C\"` hooks (`glyim_stdout_write`, …), but it was not \
                 found. Build it first with `cargo build -p glyim-runtime`, or \
                 point `GLYIM_RUNTIME_LIB` at an existing `libglyim_runtime.a`.",
            )]
        })?;
        let mut link_args = linker::LinkArgs::default();
        link_args.objects.push(runtime_lib);
        if emit == EmitKind::Cdylib {
            link_args.user_flags.push("-shared".to_string());
        }
        if let Some(flags) = args.link_flags.as_deref() {
            link_args
                .user_flags
                .extend(flags.split_whitespace().map(str::to_string));
        }
        linker::link_with_args(
            &object_path,
            &final_path,
            &link_args,
            args.linker.as_deref(),
            args.target.as_deref(),
        )
        .map_err(|e| {
            let annotated = linker::annotate_link_error_if_stale_runtime(e);
            vec![glyim_diag::GlyimDiagnostic::internal_error(&annotated)]
        })?;
    }

    // Code-producing modes exit 0 on success; `--emit=run` returned the
    // program's own exit code from its early-return branch above.
    Ok(0)
}

/// M2.1: emit the phase-profile side outputs after a successful compile.
///
/// * `--self-profile[=FILE]`: write the Chrome Trace JSON event list.
/// * `--time-passes`: print the fixed-width table on stderr.
///
/// Both are best-effort-friendly for tracing but strict for I/O: a trace
/// write failure is a real user error (the requested file could not be
/// produced) and surfaces as a diagnostic with a fix suggestion.
fn write_profile_outputs(
    args: &CliArgs,
    profile: &glyim_pipeline::PhaseProfile,
) -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {
    if let Some(trace_path) = &args.self_profile {
        let json = profile.render_chrome_trace();
        if let Err(e) = std::fs::write(trace_path, json) {
            return Err(vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                "--self-profile: failed to write {}: {e}",
                trace_path.display()
            ))
            .with_suggestion(glyim_diag::Suggestion {
                message: format!(
                    "point --self-profile at a writable path (the parent directory of {} must exist); the flag also accepts a bare --self-profile, which writes ./trace.json",
                    trace_path.display()
                ),
                replacements: Vec::new(),
                applicability: glyim_diag::Applicability::MaybeIncorrect,
            })]);
        }
        tracing::debug!(path = %trace_path.display(), "chrome trace written");
    }
    if args.time_passes {
        eprint!("{}", profile.render_text());
    }
    Ok(())
}

/// Finish a compile whose object already exists at `object_path` (e.g. a
/// cache hit): run the link step for `--emit=exec` / `--emit=cdylib`, and
/// no-op for `--emit=obj`. Mirrors the tail of the normal emit path so the
/// two are behaviourally identical.
fn finalize_after_object(
    emit: EmitKind,
    object_path: &std::path::Path,
    final_output_path: Option<&std::path::Path>,
    args: &CliArgs,
) -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {
    // M2.1: a cache hit means no phase ran, so there is no profile to
    // print or trace. Say so explicitly instead of leaving --time-passes
    // users wondering where their table went.
    if args.time_passes || args.self_profile.is_some() {
        eprintln!("cache hit — no phase profile recorded for this compile");
    }
    if emit == EmitKind::Exec || emit == EmitKind::Cdylib {
        let final_path = final_output_path.ok_or_else(|| {
            vec![glyim_diag::GlyimDiagnostic::internal_error(
                "emit should have a final output path",
            )]
        })?;
        // The runtime staticlib provides the stdlib's `extern "C"` hooks
        // (`glyim_stdout_write`, `glyim_errno`, `glyim_alloc`, …). Without it
        // the link fails with a cryptic `undefined _glyim_stdout_write`; report
        // the real cause instead and say how to fix it.
        let runtime_lib = linker::find_runtime_staticlib().ok_or_else(|| {
            vec![glyim_diag::GlyimDiagnostic::internal_error(
                "`--emit=exec`/`--emit=cdylib` needs the `glyim-runtime` static \
                 library (`libglyim_runtime.a`), which provides the stdlib's \
                 `extern \"C\"` hooks (`glyim_stdout_write`, …), but it was not \
                 found. Build it first with `cargo build -p glyim-runtime`, or \
                 point `GLYIM_RUNTIME_LIB` at an existing `libglyim_runtime.a`.",
            )]
        })?;
        let mut link_args = linker::LinkArgs::default();
        link_args.objects.push(runtime_lib);
        if emit == EmitKind::Cdylib {
            link_args.user_flags.push("-shared".to_string());
        }
        if let Some(flags) = args.link_flags.as_deref() {
            link_args
                .user_flags
                .extend(flags.split_whitespace().map(str::to_string));
        }
        linker::link_with_args(
            object_path,
            final_path,
            &link_args,
            args.linker.as_deref(),
            args.target.as_deref(),
        )
        .map_err(|e| {
            let annotated = linker::annotate_link_error_if_stale_runtime(e);
            vec![glyim_diag::GlyimDiagnostic::internal_error(&annotated)]
        })?;
    }
    Ok(())
}

/// Construct the Rust target triple for the build host, used to compile
/// proc-macro dependencies (which run on the host at compile time). Derived
/// from `std::env::consts` so it matches the machine executing the compiler.
fn host_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let env = std::env::consts::FAMILY;
    // Map to the conventional Rust triple components.
    let parts = match (arch, os, env) {
        ("x86_64", "macos", _) => ("x86_64", "apple-darwin", ""),
        ("aarch64", "macos", _) => ("aarch64", "apple-darwin", ""),
        ("x86_64", "linux", _) => ("x86_64", "unknown-linux-gnu", ""),
        ("aarch64", "linux", _) => ("aarch64", "unknown-linux-gnu", ""),
        ("x86_64", "windows", _) => ("x86_64", "pc-windows-msvc", ""),
        ("aarch64", "windows", _) => ("aarch64", "pc-windows-msvc", ""),
        _ => {
            // Best-effort fallback: lowercase arch + os.
            let a = arch.to_lowercase();
            let o = os.to_lowercase();
            return format!("{a}-{o}");
        }
    };
    let (arch_triple, os_triple, env_triple) = parts;
    if env_triple.is_empty() {
        format!("{arch_triple}-{os_triple}")
    } else {
        format!("{arch_triple}-{os_triple}-{env_triple}")
    }
}

/// Phase 8 / plan §9.2: two-stage proc-macro build orchestration.
///
/// For each proc-macro dependency source `dep`, compile it for the **HOST**
/// triple to a position-independent shared library (`--emit=cdylib`), then
/// `dlopen` it via [`glyim_proc_macro::load_cdylib`] and merge its macros into
/// a single combined [`glyim_proc_macro::Registry`]. The combined registry is
/// what drives procedural-macro expansion of the primary crate (threaded into
/// `Pipeline::compile_file` via `with_proc_registry`).
///
/// Compiling for the host (not the target) is essential: proc macros execute at
/// compile time on the build machine, so the cdylib must match the host's
/// architecture/ABI even when the final program targets a different triple.
fn build_proc_macro_dependencies(
    deps: &[std::path::PathBuf],
) -> Result<glyim_proc_macro::Registry, String> {
    let host_triple = host_target_triple();
    let mut combined = glyim_proc_macro::Registry::new();
    for dep in deps {
        // Compile the proc-macro crate for the host to a cdylib. RT-31: the
        // returned `TempDir` MUST be kept alive until after the dlopen — its
        // Drop deletes the directory, so dropping it early (as the old code
        // did by returning only the path) made `load_cdylib` open a deleted
        // path and always fail.
        let (_out_dir_keep_alive, cdylib_path) = compile_proc_macro_dep(dep, &host_triple)?;
        // dlopen it and merge its registered macros into the combined registry.
        let path_str = cdylib_path.to_str().ok_or_else(|| {
            format!(
                "proc-macro cdylib path for {} is not valid UTF-8",
                dep.display()
            )
        })?;
        let loaded = glyim_proc_macro::load_cdylib(path_str).map_err(|e| {
            format!(
                "failed to load proc-macro cdylib for {}: {e}",
                dep.display()
            )
        })?;
        combined.merge(&loaded.registry);
    }
    Ok(combined)
}

/// Compile a single proc-macro dependency crate for `host_triple` into a
/// position-independent shared library, returning the produced cdylib path.
fn compile_proc_macro_dep(
    dep: &std::path::Path,
    host_triple: &str,
) -> Result<(tempfile::TempDir, std::path::PathBuf), String> {
    let out_dir = tempfile::tempdir().map_err(|e| format!("failed to make temp dir: {e}"))?;
    let cdylib_path = out_dir.path().join("proc_macro_dep");
    let cdylib_path = if cfg!(target_os = "macos") {
        cdylib_path.with_extension("dylib")
    } else {
        cdylib_path.with_extension("so")
    };
    let args = CliArgs {
        input: dep.to_path_buf(),
        output: Some(cdylib_path.clone()),
        opt_level: 0,
        target: Some(host_triple.to_string()),
        backend: "llvm".to_string(),
        emit: "cdylib".to_string(),
        linker: None,
        link_flags: None,
        lto: "off".to_string(),
        codegen_units: None,
        proc_macro_deps: None,
        error_format: "human".to_string(),
        with_stdlib: false,
        allow: Vec::new(),
        warn: Vec::new(),
        deny: Vec::new(),
        time_passes: false,
        self_profile: None,
    };
    run_with_args(args).map_err(|diags| {
        let msg = diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "proc-macro dep {} failed to compile: {}",
            dep.display(),
            msg
        )
    })?;
    Ok((out_dir, cdylib_path))
}

#[cfg(test)]
mod tests;
