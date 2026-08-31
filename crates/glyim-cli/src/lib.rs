//! Crate root.
use clap::Parser;
use glyim_codegen::BytecodeBackend;
use glyim_codegen_llvm::LlvmBackend;
use glyim_codegen_llvm::passes::LtoKind;
use glyim_db::{CrateConfig, Database};
use glyim_pipeline::Pipeline;
use std::path::PathBuf;

/// linker.
pub mod linker;

#[derive(Parser, Debug)]
#[command(name = "glyim", version, about = "The Glyim compiler")]
/// CliArgs.
pub struct CliArgs {
    #[arg(value_name = "INPUT")]
/// Struct.
    pub input: PathBuf,
    #[arg(short, long)]
/// Struct.
    pub output: Option<PathBuf>,
    #[arg(long, value_name = "EMIT", default_value = "obj")]
/// Struct.
    pub emit: String,
    #[arg(short = 'O', long = "opt-level", default_value = "0")]
/// Struct.
    pub opt_level: u8,
    #[arg(long = "target")]
/// Struct.
    pub target: Option<String>,
    #[arg(long = "backend", default_value = "llvm")]
/// Struct.
    pub backend: String,
    #[arg(long = "linker")]
/// Struct.
    pub linker: Option<String>,
    #[arg(long = "link-flags")]
/// Struct.
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
}

/// run.
pub fn run() -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {
    // Capture args (incl. `--error-format`) before entering the panic catcher,
    // so we can print any returned diagnostics in the requested format.
    let cli_args = CliArgs::parse();
    let error_format = cli_args.error_format.clone();
    // Plan §3.1.2b: catch unexpected panics in the driver and turn them into a
    // structured ICE report instead of a raw Rust backtrace. A language
    // toolchain must never crash on user input with an unreadable stack trace —
    // an ICE with the compiler version + input + backtrace path is the
    // production-grade behaviour (rustc's `RUST_BACKTRACE` model).
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_with_args(cli_args)
    }));
    match result {
        Ok(inner) => {
            if let Err(diags) = &inner {
                print_diagnostics(diags, &error_format);
            }
            inner
        }
        Err(_payload) => {
            let backtrace = std::backtrace::Backtrace::force_capture();
            let report = format!(
                "glyim internal compiler error (ICE): the compiler panicked.\n\
                 version: {}\n\
                 Please file a bug report. Backtrace saved below.\n\n{:?}",
                env!("CARGO_PKG_VERSION"),
                backtrace,
            );
            let path = std::env::temp_dir().join("glyim-ice.txt");
            let _ = std::fs::write(&path, &report);
            eprintln!(
                "error: glyim panicked (ICE). Backtrace saved to {}:

please file a bug report at <https://github.com/elcoosp/glyim-v2/issues> \
with the input file and the backtrace.",
                path.display()
            );
            Err(vec![glyim_diag::GlyimDiagnostic::internal_error(report)])
        }
    }
}

/// Print `diags` in the requested format (plan §3.4). `human` renders each via
/// its `Display` impl (miette-styled); `json` emits one JSON object per
/// diagnostic on its own line, suitable for editor/LSP consumption.
fn print_diagnostics(diags: &[glyim_diag::GlyimDiagnostic], format: &str) {
    if format == "json" {
        for d in diags {
            let spans: Vec<serde_json::Value> = std::iter::once(&d.span.primary)
                .chain(d.span.secondary.iter().map(|(s, _)| s))
                .map(|s| {
                    let r = s.range();
                    serde_json::json!({ "lo": r.start, "hi": r.end })
                })
                .collect();
            let obj = serde_json::json!({
                "code": format!("{}", d.code),
                "severity": format!("{:?}", d.severity),
                "message": d.message,
                "spans": spans,
            });
            eprintln!("{}", obj);
        }
    } else {
        for d in diags {
            eprintln!("{}", d);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EmitKind {
    Obj,
    Exec,
    Mir,
    LlvmIr,
    Asm,
    /// Emit a position-independent shared library (cdylib). Phase 9.2: this is
    /// the host artifact a procedural-macro crate compiles to so it can be
    /// `dlopen`ed by `glyim_proc_macro::load_cdylib` during macro expansion.
    Cdylib,
}

impl EmitKind {
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "obj" => Ok(EmitKind::Obj),
            "exec" => Ok(EmitKind::Exec),
            "mir" => Ok(EmitKind::Mir),
            "llvm-ir" => Ok(EmitKind::LlvmIr),
            "asm" => Ok(EmitKind::Asm),
            "cdylib" => Ok(EmitKind::Cdylib),
            _ => Err(format!(
                "invalid value for --emit: '{}' (expected one of: obj, exec, mir, llvm-ir, asm, cdylib)",
                s
            )),
        }
    }
}

pub(crate) fn run_with_args(args: CliArgs) -> Result<(), Vec<glyim_diag::GlyimDiagnostic>> {
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
        EmitKind::Obj | EmitKind::Exec => {
            let obj = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                p.set_extension("o");
                p
            });
            let final_out = if emit == EmitKind::Exec {
                args.output.clone().unwrap_or_else(|| {
                    let mut p = input.clone();
                    p.set_extension("");
                    p
                })
            } else {
                obj.clone()
            };
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
        EmitKind::Mir | EmitKind::LlvmIr => {
            let out = args.output.clone().unwrap_or_else(|| {
                let mut p = input.clone();
                let ext = if emit == EmitKind::Mir { "mir" } else { "ll" };
                p.set_extension(ext);
                p
            });
            (out, None)
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

    let target_triple = args
        .target
        .clone()
        .unwrap_or_else(|| "x86_64-unknown-linux-gnu".to_string());

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
                format!(
                    "invalid value for --lto: '{}' (expected one of: off, fat, thin)",
                    other
                ),
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

    let mut db = Database::new(config);

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
    if emit == EmitKind::Mir {
        return glyim_pipeline::emit_mir(&mut db, input, &object_path);
    } else if emit == EmitKind::LlvmIr {
        return glyim_pipeline::emit_llvm_ir(&mut db, input, &object_path);
    } else if emit == EmitKind::Asm {
        return glyim_pipeline::emit_asm(&mut db, input, &object_path);
    }

    // Build the LLVM backend concretely so the Thin path can drive per-CGU
    // bitcode emission (`emit_thinlto_bitcode_files`) directly. For the
    // non-Thin paths it is boxed into the `CodegenBackend` trait object.
    let entry_main = glyim_pipeline::Pipeline::entry_main_local_id(&mut db, input);
    let target_info = glyim_core::TargetInfo::from_triple(&target_triple);
    let mut llvm = LlvmBackend::with_db(&db)
        .with_target(&target_triple)
        .with_opt_level(args.opt_level)
        .with_opt_for_size(false)
        .with_lto(lto);
    // Only emit a C-ABI `main` entry symbol for `--emit=exec`; a cdylib or
    // plain object must not carry a `main` (it would be an unused/conflicting
    // entry point). `obj`/`cdylib` consumers link `main` themselves if needed.
    if let (Some(main_id), EmitKind::Exec) = (entry_main, emit) {
        llvm = llvm.with_entry_main(main_id);
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
                    "ThinLTO per-CGU bitcode emission failed: {:?}",
                    e
                ))]
            })?;
        let thin_objects =
            linker::thin_lto_link(&bitcode_paths, args.opt_level, &bitcode_dir).map_err(|e| {
                vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                    "ThinLTO thin-link failed: {}",
                    e
                ))]
            })?;
        let thin_obj = thin_objects.into_iter().next().ok_or_else(|| {
            vec![glyim_diag::GlyimDiagnostic::internal_error(
                "ThinLTO thin-link produced no object files",
            )]
        })?;
        std::fs::copy(&thin_obj, &object_path).map_err(|e| {
            vec![glyim_diag::GlyimDiagnostic::internal_error(format!(
                "ThinLTO: failed to copy thin-linked object to {}: {}",
                object_path.display(),
                e
            ))]
        })?;
    } else if args.backend == "bytecode" {
        if args.opt_level > 0 {
            tracing::warn!(
                "bytecode backend opt-level currently has no effect; reserved for future peephole passes"
            );
        }
        let ctx = glyim_type::TyCtxMut::new(db.interner().clone()).freeze();
        let backend: Box<dyn glyim_codegen::CodegenBackend> =
            Box::new(BytecodeBackend::with_ty_ctx(std::sync::Arc::new(ctx), target_info));
        Pipeline::compile_file(&mut db, input, &*backend, &object_path, args.codegen_units, proc_registry.as_ref())?;
    } else {
        let backend: Box<dyn glyim_codegen::CodegenBackend> = Box::new(llvm);
        Pipeline::compile_file(&mut db, input, &*backend, &object_path, args.codegen_units, proc_registry.as_ref())?;
    }

    if emit == EmitKind::Exec || emit == EmitKind::Cdylib {
        let final_path = final_output_path.expect("emit should have final output");
        // `cdylib` produces a position-independent shared library (`-shared`);
        // `exec` produces a runnable binary. Phase 9.2: the cdylib is the host
        // artifact a proc-macro crate compiles to so `load_cdylib` can dlopen it.
        let extra_flags = if emit == EmitKind::Cdylib {
            Some("-shared")
        } else {
            None
        };
        linker::invoke_linker(
            &object_path,
            &final_path,
            args.linker.as_deref(),
            extra_flags.or(args.link_flags.as_deref()),
            args.target.as_deref(),
        )
        .map_err(|e| vec![glyim_diag::GlyimDiagnostic::internal_error(&e)])?;
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
            return format!("{}-{}", a, o);
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
        // Compile the proc-macro crate for the host to a cdylib.
        let cdylib_path = compile_proc_macro_dep(dep, &host_triple)?;
        // dlopen it and merge its registered macros into the combined registry.
        let loaded = glyim_proc_macro::load_cdylib(cdylib_path.to_str().unwrap_or_default())
            .map_err(|e| format!("failed to load proc-macro cdylib for {}: {e}", dep.display()))?;
        combined.merge(&loaded.registry);
    }
    Ok(combined)
}

/// Compile a single proc-macro dependency crate for `host_triple` into a
/// position-independent shared library, returning the produced cdylib path.
fn compile_proc_macro_dep(
    dep: &std::path::Path,
    host_triple: &str,
) -> Result<std::path::PathBuf, String> {
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
    };
    run_with_args(args).map_err(|diags| {
        let msg = diags
            .iter()
            .map(|d| d.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        format!("proc-macro dep {} failed to compile: {}", dep.display(), msg)
    })?;
    Ok(cdylib_path)
}

#[cfg(test)]
mod tests;
