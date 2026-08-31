//! Command implementations for glyip.
//!
//! Each public function corresponds to a CLI sub-command and orchestrates
//! configuration loading, dependency resolution, compilation, and output.

use glyim_db::{CrateConfig, Database};
use glyim_pipeline::MirCompilation;
use glyim_core::def_id::DefId;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use tracing::info;

use crate::cache::Cache;
use crate::config::{BuildOptions, GlyipToml, NewOptions, RunOptions, TestOptions};
use crate::dep::DependencyResolver;
use crate::error::{GlyipError, GlyipResult};
use crate::lockfile::Lockfile;

// ---------------------------------------------------------------------------
// `glyip new`
// ---------------------------------------------------------------------------

/// Result of a successful `glyip new` invocation.
#[derive(Debug)]
pub struct NewResult {
    /// Absolute path to the newly created project directory.
    pub path: PathBuf,
}

/// Create a new Glyim project at the given path.
///
/// `dir` is the parent directory in which the project folder (named `name`)
/// is created. When `None`, the current working directory is used. Prefer
/// passing an explicit `dir` in callers that don't want to mutate the
/// process-global working directory (e.g. tests, which otherwise race under
/// parallel execution).
pub fn cmd_new(name: &str, opts: &NewOptions, dir: Option<&Path>) -> GlyipResult<NewResult> {
    let base = match dir {
        Some(d) => d.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let dir = base.join(name);
    if dir.exists() {
        return Err(GlyipError::ProjectAlreadyExists(dir));
    }

    info!(
        "Creating new {} project '{}' at {}",
        if opts.lib { "library" } else { "binary" },
        name,
        dir.display()
    );

    // Create directory tree.
    fs::create_dir_all(dir.join("src"))?;
    fs::create_dir_all(dir.join("tests"))?;

    // Write Glyip.toml.
    let config = GlyipToml {
        package: crate::config::PackageConfig {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            edition: opts.edition.clone(),
            authors: Vec::new(),
            description: None,
            bin: if opts.lib {
                None
            } else {
                Some(vec![crate::config::BinTarget {
                    name: name.to_string(),
                    path: Some(PathBuf::from("src/main.g")),
                }])
            },
            lib: if opts.lib {
                Some(crate::config::LibTarget {
                    path: Some(PathBuf::from("src/lib.g")),
                })
            } else {
                None
            },
        },
        dependencies: BTreeMap::new(),
        dev_dependencies: BTreeMap::new(),
    };
    config.write_to_dir(&dir)?;

    // Write source entry point.
    if opts.lib {
        let lib_src = dir.join("src/lib.g");
        fs::write(&lib_src, "// Library entry point\n")?;
    } else {
        let main_src = dir.join("src/main.g");
        fs::write(&main_src, "fn main() {}\n")?;
    }

    // Write a placeholder test file.
    let test_src = dir.join("tests/integration.g");
    fs::write(&test_src, "// Integration tests\n")?;

    // Create an empty lockfile.
    Lockfile::new().write_to_dir(&dir)?;

    info!("Project '{}' created successfully", name);
    Ok(NewResult { path: dir })
}

// ---------------------------------------------------------------------------
// `glyip build`
// ---------------------------------------------------------------------------

/// Result of a successful `glyip build` invocation.
#[derive(Debug)]
pub struct BuildResult {
    /// Path to the output artifact (binary or library).
    pub output: PathBuf,
    /// Number of diagnostics (errors + warnings) emitted.
    pub diagnostic_count: usize,
    /// Whether the build was incremental (some files were cached).
    pub incremental: bool,
}

/// Build the project in the current directory (or `project_dir`).
pub fn cmd_build(project_dir: &Path, opts: &BuildOptions) -> GlyipResult<BuildResult> {
    let config = GlyipToml::read_from_dir(project_dir)?;
    let mut cache = Cache::new(project_dir)?;

    info!(
        "Building {} v{}",
        config.package.name, config.package.version
    );

    // Dependency resolution.
    let resolver = DependencyResolver::new_no_index();
    let _lockfile = resolver.resolve(&config, project_dir)?;
    info!("Dependencies resolved");

    // Incremental check.
    cache.set_build_config(opts);
    let incremental = !cache.needs_rebuild()?;
    if incremental {
        info!("No source changes detected — skipping compilation");
        let output = cache.output_binary_for_target(
            &config.package.name,
            opts.release,
            opts.target.as_deref(),
        );
        if output.exists() {
            return Ok(BuildResult {
                output,
                diagnostic_count: 0,
                incremental: true,
            });
        }
        // Fall through to recompile if artifact is missing despite clean fingerprints.
        info!("Artifact missing — recompiling");
    }

    // Find entry point.
    let entry = find_entry_point(project_dir, &config)?;

    // Run the compiler pipeline.
    let (output, diag_count) = compile_source(project_dir, &entry, &config, opts, &mut cache)?;

    // Mark build as successful.
    cache.mark_built()?;

    Ok(BuildResult {
        output,
        diagnostic_count: diag_count,
        incremental: false,
    })
}

// ---------------------------------------------------------------------------
// `glyip test`
// ---------------------------------------------------------------------------

/// Result of a successful `glyip test` invocation.
#[derive(Debug)]
pub struct TestResult {
    /// Total tests discovered.
    pub total: usize,
    /// Tests that passed.
    pub passed: usize,
    /// Tests that failed.
    pub failed: usize,
    /// Tests that were ignored / skipped.
    pub ignored: usize,
}

/// Build and run the project's tests.
///
/// Discovers individual test functions within each `.g` source file using
/// [`crate::test_discovery::FileTestDiscovery`]. Reports per-function
/// results when possible, falling back to per-file results for compilation
/// failures.
pub fn cmd_test(project_dir: &Path, opts: &TestOptions) -> GlyipResult<TestResult> {
    let config = GlyipToml::read_from_dir(project_dir)?;

    info!(
        "Testing {} v{}",
        config.package.name, config.package.version
    );

    // Resolve dependencies.
    let resolver = DependencyResolver::new_no_index();
    let _lockfile = resolver.resolve(&config, project_dir)?;

    // Find test entry points.
    let test_dir = project_dir.join("tests");
    let mut test_files: Vec<PathBuf> = Vec::new();
    if test_dir.exists() {
        collect_source_files(&test_dir, &mut test_files)?;
    }

    // Also look for inline test modules in src/.
    let src_dir = project_dir.join("src");
    if src_dir.exists() {
        collect_source_files(&src_dir, &mut test_files)?;
    }

    if test_files.is_empty() {
        info!("No test files found");
        return Ok(TestResult {
            total: 0,
            passed: 0,
            failed: 0,
            ignored: 0,
        });
    }

    // Discover individual test functions across all files.
    let mut all_discovered: Vec<crate::test_discovery::DiscoveredTest> = Vec::new();
    for test_file in &test_files {
        if let Ok(discovery) = crate::test_discovery::FileTestDiscovery::scan(test_file) {
            all_discovered.extend(discovery.tests);
        }
    }

    let mut total = 0usize;
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut ignored = 0usize;

    // Compile each test file to MIR once, then run the specific test body for
    // every discovered test function via the in-process MIR interpreter. The
    // bytecode backend emits glyim's own opcode format (not a native object),
    // so runtime execution goes through the interpreter rather than a linker.
    let mut mir_cache: HashMap<PathBuf, Option<MirCompilation>> = HashMap::new();

    for discovered_test in &all_discovered {
        // Apply name filter: non-matching tests are reported as ignored.
        if let Some(ref filter) = opts.filter
            && !discovered_test.name.contains(filter.as_str())
        {
            ignored += 1;
            continue;
        }

        // `#[ignore]` tests are skipped unless explicitly requested.
        if discovered_test.ignored && !opts.run_ignored {
            ignored += 1;
            continue;
        }

        total += 1;

        if opts.no_run {
            continue;
        }

        if opts.compiled {
            // Plan §23.1: compiled-binary execution. Compile the whole test
            // file to a native executable and run it as an isolated subprocess;
            // pass/fail is determined by the child's exit code. The
            // interpreter loop below is skipped entirely for this mode.
            match compile_and_run_compiled(&discovered_test.file, &config, std::time::Duration::from_secs(30)) {
                Ok(()) => passed += 1,
                Err(e) => {
                    eprintln!("compiled test `{}` failed: {e}", discovered_test.name);
                    failed += 1;
                }
            }
            continue;
        }

        // Compile (or reuse a cached compilation of) the containing file.
        let mir = mir_cache.entry(discovered_test.file.clone()).or_insert_with(|| {
            let mut db = make_test_db(&config);
            match glyim_pipeline::compile_file_to_mir(&mut db, &discovered_test.file) {
                Ok(m) => Some(m),
                Err(diags) => {
                    eprintln!("[DBG] compile error diags:");
                    for d in &diags {
                        eprintln!("[DBG]   {:?}", d.message);
                    }
                    None
                }
            }
        });

        let Some(compilation) = mir else {
            failed += 1;
            continue;
        };

        // Resolve the discovered test name to the function's DefId and run it.
        let Some(def_id) = resolve_test_def_id(compilation, &discovered_test.name) else {
            failed += 1;
            continue;
        };
        let Some(body) = compilation.bodies.get(&def_id) else {
            failed += 1;
            continue;
        };

        let mut interpreter = glyim_mir_interp::Interpreter::new(compilation.ty_ctx.as_ref());
        for (id, b) in &compilation.bodies {
            interpreter.add_function(*id, (**b).clone());
        }
        match interpreter.run_body(body) {
            Ok(()) => passed += 1,
            Err(_) => failed += 1,
        }
    }

    info!(
        "Test results: {} passed, {} failed, {} ignored (of {} total)",
        passed, failed, ignored, total
    );
    Ok(TestResult {
        total,
        passed,
        failed,
        ignored,
    })
}

/// Build a fresh compiler `Database` for compiling a single test file to MIR.
fn make_test_db(config: &GlyipToml) -> Database {
    let crate_config = CrateConfig {
        name: config.package.name.clone(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        opt_level: 0,
    };
    Database::new(crate_config)
}

/// Resolve a discovered test function name to its `DefId` within a compiled
/// crate, so the matching MIR body can be executed by the interpreter.
fn resolve_test_def_id(compilation: &MirCompilation, name: &str) -> Option<DefId> {
    for module in compilation.def_map.modules.iter() {
        for (name_key, (local_id, _visibility, _span)) in &module.scope.values {
            if compilation.def_map.interner.resolve(*name_key) == name {
                return Some(DefId::new(compilation.def_map.krate, *local_id));
            }
        }
    }
    None
}

/// Plan §23.1: compiled-binary execution path.
///
/// Compiles `file` to a native object file via the real LLVM backend, links it
/// into a native executable with the system linker (`glyim_cli::linker`, the
/// same path the `glyim-test` harness uses), and runs that executable as a real,
/// isolated subprocess. This exercises actual compiled-binary behavior —
/// panics, aborts, FFI, and process exit codes — which the in-process MIR
/// interpreter cannot faithfully reproduce. The test program's `main` is
/// expected to exit `0` on success and non-zero on failure.
///
/// Returns `Ok(())` when the child process exits `0`, `Err` otherwise (non-zero
/// exit, crash, compile failure, link failure, or timeout). On any failure to
/// even produce a runnable binary this yields `Err` so the test is reported as
/// failed rather than silently skipped.
///
/// NOTE: the compiler currently targets `x86_64-unknown-linux-gnu` (the only
/// triple wired through the codegen/ABI layers that yields a linkable object
/// with a `main` entry point). The linked artifact is a Linux ELF, so this path
/// is only meaningfully executable on a Linux host (e.g. the CI `test` job). On
/// a non-Linux host the link step fails and the function returns `Err`
/// describing the host mismatch — which is the correct, honest outcome rather
/// than a silent pass.
fn compile_and_run_compiled(
    file: &Path,
    config: &GlyipToml,
    timeout: std::time::Duration,
) -> Result<(), String> {
    let crate_config = CrateConfig {
        name: config.package.name.clone(),
        target_triple: host_target_triple(),
        opt_level: 0,
    };
    let mut db = Database::new(crate_config);

    let output_path = std::env::temp_dir().join(format!(
        "glyim_test_compiled_{}.o",
        file.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
    ));
    let exe_path = output_path.with_extension("");

    let backend: Box<dyn glyim_codegen::CodegenBackend> = {
        // §1.9: when the source has an entry `main`, tell the backend to emit a
        // C-ABI `main` symbol (via `--emit=exec` semantics) so the produced
        // object links into a runnable binary. Without this the object has no
        // `main` and the native link fails ("undefined reference to main").
        // Target the host triple so the produced object links and runs on the
        // dev machine (see `host_target_triple`).
        let mut llvm = glyim_codegen_llvm::LlvmBackend::new()
            .with_target(host_target_triple());
        if let Some(main_id) =
            glyim_pipeline::Pipeline::entry_main_local_id(&mut db, file)
        {
            llvm = llvm.with_entry_main(main_id);
        }
        Box::new(llvm)
    };

    // Stage 1: compile source -> object file via the full pipeline.
    match glyim_pipeline::Pipeline::compile_file(&mut db, file, backend.as_ref(), &output_path, None, None) {
        Ok(_) => {}
        Err(diags) => {
            return Err(format!(
                "compilation failed: {}",
                diags
                    .iter()
                    .map(|d| d.message.clone())
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
    }

    // Stage 2: link the object file into a real executable (same path as the
    // `glyim-test` harness' `PipelineCompiler`). The linker is selected by
    // `glyim_cli::linker` (cc/clang/gcc on Unix, link.exe on Windows).
    if let Err(link_err) =
        glyim_cli::linker::invoke_linker(&output_path, &exe_path, None, None, None)
    {
        return Err(format!(
            "linking compiled test binary failed: {link_err} \
             (the compiler targets x86_64-unknown-linux-gnu; linking may be \
             unsupported on this host)"
        ));
    }

    // Stage 3: execute the linked binary as an isolated subprocess.
    let mut child = Command::new(&exe_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn compiled test binary: {e}"))?;

    // Per-test timeout: run the child in its own thread and wait on a channel
    // with a deadline so a hanging test cannot block the runner. Keep the `Child`
    // behind a shared handle so the main thread can kill it portably on timeout
    // (plan §1.4: `Child::kill()` is cross-platform; shelling out to a `kill`
    // binary is Unix-only, sends SIGTERM (ignorable), and swallows errors).
    let child = Arc::new(Mutex::new(Some(child)));
    let child_wait = child.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Option<ExitStatus>>();
    std::thread::spawn(move || {
        let status = child_wait
            .lock()
            .unwrap()
            .take()
            .and_then(|mut c| c.wait().ok());
        let _ = tx.send(status);
    });

    let status = match rx.recv_timeout(timeout) {
        Ok(Some(status)) => status,
        Ok(None) => {
            return Err("test subprocess wait failed".to_string());
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            if let Some(mut c) = child.lock().unwrap().take() {
                let _ = c.kill();
            }
            return Err(format!("test timed out after {}s", timeout.as_secs()));
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            return Err("test subprocess channel disconnected".to_string());
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err(format!("compiled test exited with status {status}"))
    }
}

// ---------------------------------------------------------------------------
// `glyip run`
// ---------------------------------------------------------------------------

/// Result of a successful `glyip run` invocation.
#[derive(Debug)]
pub struct RunResult {
    /// Path to the compiled binary.
    pub binary: PathBuf,
    /// Exit code of the executed binary.
    pub exit_code: i32,
}

/// Build and run the project's binary target.
pub fn cmd_run(project_dir: &Path, opts: &RunOptions) -> GlyipResult<RunResult> {
    let config = GlyipToml::read_from_dir(project_dir)?;

    info!(
        "Running {} v{}",
        config.package.name, config.package.version
    );

    // Resolve dependencies.
    let resolver = DependencyResolver::new_no_index();
    let _lockfile = resolver.resolve(&config, project_dir)?;

    // Build first.
    let build_opts = BuildOptions {
        release: opts.release,
        target: opts.target.clone(),
        backend: opts.backend.clone(),
        opt_level: if opts.release { 2 } else { 0 },
        lto: opts.lto,
    };
    let build_result = cmd_build(project_dir, &build_opts)?;

    // Execute the binary.
    info!("Executing {}", build_result.output.display());
    // Build-time environment: the compiled artifact may depend on a glyim
    // runtime search path. Today there is no separate runtime library, so
    // `env` is empty; the placeholder is wired (plan §21.4) so runtime vars
    // can be injected here without touching the call sites later.
    let run_env: HashMap<String, String> = HashMap::new();
    let exit_code = run_binary(&build_result.output, &opts.args, &run_env)?;

    Ok(RunResult {
        binary: build_result.output,
        exit_code,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Find the main entry point for the project.
fn find_entry_point(project_dir: &Path, config: &GlyipToml) -> GlyipResult<PathBuf> {
    // Check explicit bin target path.
    if let Some(ref bins) = config.package.bin
        && let Some(first_bin) = bins.first()
        && let Some(ref path) = first_bin.path
    {
        let full = project_dir.join(path);
        if full.exists() {
            return Ok(full);
        }
    }

    // Check default locations.
    let candidates = [
        project_dir.join("src/main.g"),
        project_dir.join("src/lib.g"),
    ];
    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    Err(GlyipError::NoEntryPoint(project_dir.to_path_buf()))
}

/// Collect all `.g` source files under a directory (recursive).
fn collect_source_files(dir: &Path, out: &mut Vec<PathBuf>) -> GlyipResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_source_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "g") {
            out.push(path);
        }
    }
    Ok(())
}

/// Compile a single source file using the compiler pipeline.
fn compile_source(
    _project_dir: &Path,
    entry: &Path,
    config: &GlyipToml,
    opts: &BuildOptions,
    cache: &mut Cache,
) -> GlyipResult<(PathBuf, usize)> {
    let _source = fs::read_to_string(entry)?;

    // Set up the compiler database.
    let crate_config = CrateConfig {
        name: config.package.name.clone(),
        target_triple: opts
            .target
            .clone()
            .unwrap_or_else(|| "x86_64-unknown-linux-gnu".to_string()),
        opt_level: opts.opt_level,
    };
    let mut db = Database::new(crate_config);

    // Select the codegen backend.
    #[cfg(feature = "llvm")]
    let backend: Box<dyn glyim_codegen::CodegenBackend> = if opts.backend == "llvm" {
        Box::new(glyim_codegen_llvm::LlvmBackend::new().with_lto(
            opts.lto.unwrap_or(glyim_codegen_llvm::passes::LtoKind::None),
        ))
    } else {
        let ctx = glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze();
        Box::new(glyim_codegen::BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(ctx),
            glyim_core::TargetInfo::default(),
        ))
    };
    #[cfg(not(feature = "llvm"))]
    let backend: Box<dyn glyim_codegen::CodegenBackend> = {
        if opts.backend == "llvm" {
            tracing::warn!(
                "LLVM backend requested but not compiled in, falling back to bytecode"
            );
        }
        let ctx = glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze();
        Box::new(glyim_codegen::BytecodeBackend::with_ty_ctx(
            std::sync::Arc::new(ctx),
            glyim_core::TargetInfo::default(),
        ))
    };

    // Run the pipeline.
    let output_dir = cache.output_dir_for_target(opts.release, opts.target.as_deref());
    fs::create_dir_all(&output_dir)?;
    let output_path = output_dir.join(&config.package.name);

    match glyim_pipeline::Pipeline::compile_file(&mut db, entry, backend.as_ref(), &output_path, None, None) {
        Ok(()) => {
            info!("Compilation succeeded: {}", output_path.display());
            Ok((output_path, 0))
        }
        Err(diags) => Err(GlyipError::BuildFailed(diags)),
    }
}

/// Run a compiled binary and return its exit code.
///
/// `env` is an explicit set of environment variables to set on the child
/// process (forwarded in addition to the process's inherited environment —
/// see [`std::process::Command::envs`]). When `glyip run` later gains a
/// runtime library, the runtime search path and any runtime configuration
/// vars are injected here (de-stubbing plan §21.4) rather than relying on
/// ambient shell state.
fn run_binary(binary: &Path, args: &[String], env: &HashMap<String, String>) -> GlyipResult<i32> {
    if !binary.exists() {
        return Err(GlyipError::Other(format!(
            "binary not found: {}",
            binary.display()
        )));
    }

    let status = std::process::Command::new(binary)
        .args(args)
        .envs(env.iter().map(|(k, v)| (k.clone(), v.clone())))
        .status()
        .map_err(GlyipError::Io)?;

    Ok(status.code().unwrap_or(-1))
}

/// Host triple used by the compiled-binary execution harness. Targeting the
/// host (instead of a hardcoded `x86_64-unknown-linux-gnu`) lets the produced
/// object actually link and *run* on the dev machine — so destub regression
/// fixtures verify real program behavior (stdout/exit code), not just that
/// the compiler accepted the source.
fn host_target_triple() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "macos" => format!("{arch}-apple-darwin"),
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        other => format!("{arch}-unknown-{other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::NewOptions;
    use tempfile::TempDir;

    /// Create a real `cmd_new` project whose `src/main.g` contains `body`,
    /// mirroring the `glyip run`/`glyip build` path that is already tested
    /// green (S23-T04). Returns the project dir and the package config.
    fn project_with_main(body: &str) -> (TempDir, GlyipToml, PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let name = format!("compiled_{}", std::process::id());
        let project = cmd_new(&name, &NewOptions::default(), Some(dir.path()))
            .expect("cmd_new");
        let main = project.path.join("src/main.g");
        std::fs::write(&main, body).expect("write main.g");
        let config = GlyipToml::read_from_dir(&project.path).expect("read config");
        (dir, config, main)
    }

    #[test]
    fn compile_and_run_compiled_runs_real_binary() {
        // Plan §23.1: the compiled-binary execution path must compile a source
        // file to an object, link it into a real executable, and run it as an
        // isolated subprocess — verifying execution by the child's exit code.
        //
        // The harness targets the host triple (see `host_target_triple`), so the
        // produced object links and *runs* on the dev machine regardless of OS
        // (Linux or macOS). We assert it exits 0 on every host — the honest,
        // runnable outcome. (Historically this was hardcoded to a Linux ELF and
        // only ran on Linux; on other hosts it returned Err for a host-mismatch
        // link, which we no longer need.)
        let (_dir, config, main) = project_with_main("fn main() {}\n");

        let result = compile_and_run_compiled(
            &main,
            &config,
            std::time::Duration::from_secs(30),
        );

        assert!(
            result.is_ok(),
            "compiled binary should exit 0 and be reported as passing: {:?}",
            result.err()
        );
    }

    #[test]
    fn compile_and_run_compiled_reports_failure() {
        // A program whose `main` references an unresolved symbol must be reported
        // as a failure by the compiled-binary path, never silently passed.
        //
        // On Linux the unresolved-symbol binary fails at link or run time; on a
        // non-Linux host the (host-mismatch) link failure already yields Err.
        // Either way the path must NOT report success — that is the invariant
        // we assert on every host.
        let (_dir, config, main) =
            project_with_main("fn main() { undefined_symbol_xyz(); }\n");

        let result = compile_and_run_compiled(
            &main,
            &config,
            std::time::Duration::from_secs(30),
        );

        assert!(
            result.is_err(),
            "compiled binary that fails to run/link must be reported as a failure"
        );
    }
}
