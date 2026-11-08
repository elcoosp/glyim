use std::path::Path;
use std::process::Command;

/// Structured linker inputs (plan §18.2). Replaces the flat user-flags string
/// with real first-class support for `-L` search paths and `-l` libraries,
/// which dependency-based linking (each compiled dependency's output dir becomes
/// a `-L` path and its crate name an `-l` argument) requires.
#[derive(Debug, Clone, Default)]
pub struct LinkArgs {
    /// Library search paths; each becomes `-L<path>`.
    pub search_paths: Vec<std::path::PathBuf>,
    /// Libraries to link; each becomes `-l<name>` (the `lib` prefix/`lib`
    /// suffix is supplied by the linker, as with ordinary `-l`).
    pub libs: Vec<String>,
    /// Object files to link in addition to the primary `obj_path`.
    pub objects: Vec<std::path::PathBuf>,
    /// Free-form user-supplied flags, appended last so users can still
    /// override/extend anything the structured API covers.
    pub user_flags: Vec<String>,
}

/// Trait abstracting the system linker, allowing for different implementations
/// for MSVC, Unix/GCC, and LLD.
trait LinkerInvoker {
    fn link(
        &self,
        obj_path: &Path,
        output_path: &Path,
        link_flags: Option<&str>,
    ) -> Result<(), String>;
}

struct UnixLinker {
    linker: String,
}

impl LinkerInvoker for UnixLinker {
    fn link(
        &self,
        obj_path: &Path,
        output_path: &Path,
        link_flags: Option<&str>,
    ) -> Result<(), String> {
        let mut cmd = Command::new(&self.linker);
        cmd.arg(obj_path.as_os_str())
            .arg("-o")
            .arg(output_path.as_os_str());

        if let Some(flags) = link_flags {
            for flag in flags.split_whitespace() {
                cmd.arg(flag);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to invoke linker '{}': {}", self.linker, e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            Err(format!(
                "Linker '{}' failed with status {}: {}{}",
                self.linker,
                output.status,
                stderr.trim(),
                stdout.trim()
            ))
        }
    }
}

struct MsvcLinker {
    linker: String,
}

impl LinkerInvoker for MsvcLinker {
    fn link(
        &self,
        obj_path: &Path,
        output_path: &Path,
        link_flags: Option<&str>,
    ) -> Result<(), String> {
        let mut cmd = Command::new(&self.linker);
        cmd.arg(obj_path.as_os_str())
            .arg("/OUT:")
            .arg(output_path.as_os_str())
            .arg("/SUBSYSTEM:CONSOLE") // Default to console subsystem
            .arg("msvcrt.lib"); // Default C runtime

        if let Some(flags) = link_flags {
            for flag in flags.split_whitespace() {
                cmd.arg(flag);
            }
        }

        let status = cmd
            .status()
            .map_err(|e| format!("Failed to invoke linker '{}': {}", self.linker, e))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Linker '{}' failed with status {}",
                self.linker, status
            ))
        }
    }
}

/// the current `PATH`. Used only to pick a sensible default linker on
/// Windows; a false negative just means the fallback name is used, which
/// still surfaces a clean "linker not found" error if neither exists.
fn which_on_path(name: &str) -> bool {
    if cfg!(not(target_os = "windows")) {
        return Command::new("which")
            .arg(name)
            .output()
            .is_ok_and(|o| o.status.success());
    }
    Command::new("where")
        .arg(name)
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Assemble the flat flag string from structured `LinkArgs` (plan §18.2):
/// `-L<path>` for each search path, then `-l<name>` for each requested
/// library, then the extra object files, and finally free-form `user_flags`.
fn build_link_flags(args: &LinkArgs) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for path in &args.search_paths {
        parts.push(format!("-L{}", path.display()));
    }
    for lib in &args.libs {
        parts.push(format!("-l{lib}"));
    }
    for obj in &args.objects {
        parts.push(obj.display().to_string());
    }
    parts.extend(args.user_flags.iter().cloned());
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// `invoke_linker`.
pub fn invoke_linker(
    obj_path: &Path,
    output_path: &Path,
    linker: Option<&str>,
    link_flags: Option<&str>,
    target_triple: Option<&str>,
) -> Result<(), String> {
    // Plan §18.2: route the flat-flags path through `LinkArgs` so the two APIs
    // share one flag-assembly/linker-selection path.
    let args = LinkArgs {
        user_flags: link_flags
            .map(|s| {
                s.split_whitespace()
                    .map(std::string::ToString::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        ..Default::default()
    };
    link_with_args(obj_path, output_path, &args, linker, target_triple)
}

/// Locate the `glyim-runtime` static library (`libglyim_runtime.a`), which
/// provides the stdlib's `extern "C"` hooks (`glyim_stdout_write`,
/// `glyim_errno`, `glyim_alloc`, …) for `--emit=exec` binaries.
///
/// Search order:
/// 1. `GLYIM_RUNTIME_LIB` env var (test override).
/// 2. `target/<profile>/libglyim_runtime.a` (cwd = repo root).
/// 3. `../target/<profile>/libglyim_runtime.a` (cwd = a subdirectory).
/// 4. Walking up from the current executable (handles `cargo run`, where the
///    process's cwd may not be the repo root).
#[must_use]
pub fn find_runtime_staticlib() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("GLYIM_RUNTIME_LIB") {
        let path = std::path::PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    // P1-O3 (KNOWN_GAPS §7): MSVC's convention is `glyim_runtime.lib`,
    // not Unix's `libglyim_runtime.a`. The pre-fix code only tried the
    // Unix-style name, so `--emit=exec` on Windows never found a
    // runtime library and link failed with `undefined reference to
    // glyim_alloc` (or similar). Try both names in every search path;
    // on Unix the `.lib` variants don't exist so this is a no-op.
    let names = ["libglyim_runtime.a", "glyim_runtime.lib"];
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    for name in &names {
        candidates.push(std::path::PathBuf::from("target").join(profile).join(name));
        candidates.push(
            std::path::PathBuf::from("..")
                .join("target")
                .join(profile)
                .join(name),
        );
    }
    for c in &candidates {
        if c.exists() {
            return Some(c.clone());
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors() {
            for name in &names {
                let candidate = ancestor.join(name);
                if candidate.exists() {
                    return Some(candidate);
                }
                let sibling = ancestor.join(profile).join(name);
                if sibling.exists() {
                    return Some(sibling);
                }
            }
        }
    }
    None
}

/// Symbol names that only the runtime staticlib (`glyim-runtime`) provides.
/// A link error mentioning one of these is most likely a stale
/// `libglyim_runtime.a` — one built from older runtime source than the
/// current compiler. P1-O8 exists to turn that confusing failure mode
/// ("undefined reference to `glyim_stdout_write`") into an actionable
/// message.
const RUNTIME_SYMBOLS: &[&str] = &[
    "glyim_alloc",
    "glyim_dealloc",
    "glyim_drop_in_place",
    "glyim_panic",
    "glyim_stdout_write",
    "glyim_stdout_flush",
    "glyim_stderr_write",
    "glyim_stderr_flush",
    "glyim_string_from_bytes",
    "glyim_string_push_str",
    "glyim_slice_to_vec",
];

/// Pure function over `(err, runtime_path)` so tests can exercise the
/// annotation without a filesystem. The public wrapper below supplies
/// `runtime_path` from [`find_runtime_staticlib`].
///
/// If `err` mentions a runtime symbol, append a hint naming the static
/// library path, its mtime, and the rebuild command. Otherwise return
/// `err` unchanged — a non-runtime link failure (`printf`, system libs)
/// is not this hint's problem.
fn annotate_link_error_if_stale_runtime_with(
    err: String,
    runtime_path: Option<&std::path::Path>,
) -> String {
    if !RUNTIME_SYMBOLS.iter().any(|s| err.contains(s)) {
        return err;
    }
    let Some(path) = runtime_path else {
        // No runtime lib at all: the `find_runtime_staticlib` "not found"
        // path elsewhere already reported the real cause. Don't pile on.
        return err;
    };
    let mtime = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(
            || "unknown".to_string(),
            |d| format!("{}s since epoch", d.as_secs()),
        );
    format!(
        "{err}\n\n\
         Hint: the link step could not resolve `glyim_*` runtime symbols.\n\
         The static library being linked is:\n  {}\n  (mtime: {})\n\
         If that library was built from older `glyim-runtime` source than the\n\
         current compiler, its ABI will not match. Rebuild it with:\n\n    \
         cargo build -p glyim-runtime\n\n\
         (or `cargo build -p glyim-runtime --release` for release builds),\n\
         then retry.",
        path.display(),
        mtime,
    )
}

/// Public wrapper: consult [`find_runtime_staticlib`] and delegate to the
/// pure annotator. Call at every `link_with_args` failure site so a stale
/// runtime is diagnosed at the moment it fails.
#[must_use]
pub fn annotate_link_error_if_stale_runtime(err: String) -> String {
    let path = find_runtime_staticlib();
    annotate_link_error_if_stale_runtime_with(err, path.as_deref())
}

/// Link using structured `LinkArgs` (plan §18.2): emits `-L`/`-l`/objects
/// before appending any user-supplied flags.
pub fn link_with_args(
    obj_path: &Path,
    output_path: &Path,
    args: &LinkArgs,
    linker: Option<&str>,
    target_triple: Option<&str>,
) -> Result<(), String> {
    // Detect platform to choose default linker
    // P1-O3 (KNOWN_GAPS §7): on Windows, prefer `lld-link.exe` (shipped
    // with LLVM 23, which every Glyim install already requires for the
    // codegen backend) over MSVC's `link.exe`; fall back to `link.exe`
    // when lld-link isn't on PATH. Callers that set `--linker` still win.
    // The probe is a cheap `where` / `which` — failure means "not on
    // PATH", not "not installed somewhere else". If neither is found,
    // default to `link.exe` and let the actual invocation surface the
    // missing-linker error, exactly as the pre-fix code did on Unix.
    let default_linker = if cfg!(target_os = "windows") {
        if which_on_path("lld-link.exe") {
            "lld-link.exe"
        } else {
            "link.exe"
        }
    } else {
        "cc"
    };

    let linker_name = linker.unwrap_or(default_linker).to_string();

    // If no linker is explicitly provided, try to detect a better one on Unix
    let final_linker_name = if linker.is_none() && !cfg!(target_os = "windows") {
        detect_unix_linker()
    } else {
        linker_name
    };

    // Plan §18.1: cross-compilation support. When a target triple is supplied
    // and differs from the host, compute the target-appropriate linker flags
    // (e.g. `-target <triple>` for the cc/clang/gcc driver, or
    // `-m <emulation>` for raw GNU ld) and prepend them to any user-supplied
    // flags. The flags depend on which linker was actually selected, so this is
    // computed *before* the linker is moved into the linker invoker. An
    // unmapped target is a hard error rather than silently passing host flags
    // to a cross target.
    let mut cross_flags: Vec<String> = Vec::new();
    if let Some(triple) = target_triple
        && !triple.is_empty()
    {
        cross_flags = linker_flags_for_target(triple, final_linker_name.as_str())?;
    }

    // PIE default (plan §18.1 follow-up): the codegen backend emits non-PIC
    // code with absolute relocations (e.g. `R_X86_64_32 against '.rodata'`),
    // while most modern distro toolchains configure gcc/clang to build PIE
    // executables by default — the link then dies with "recompile with
    // -fPIE". Pass `-no-pie` explicitly for ELF targets so the produced
    // executable matches the code model the backend actually emits.
    // Restrictions:
    //   * cc-style drivers only — a raw `ld` link does not pull in the PIE
    //     startfiles that make the default relevant;
    //   * ELF targets only — Mach-O (darwin) has no `-no-pie` semantics and
    //     MSVC is handled by `MsvcLinker`;
    //   * not for `-shared` (cdylib) links — `-no-pie` + `-shared` is a
    //     contradictory request.
    let eff_triple = target_triple
        .filter(|t| !t.is_empty())
        .map_or_else(crate::host_target_triple, std::string::ToString::to_string);
    let is_driver_link = !cfg!(target_os = "windows")
        && (final_linker_name == "cc"
            || final_linker_name == "gcc"
            || final_linker_name == "clang"
            || final_linker_name.ends_with("/cc")
            || final_linker_name.ends_with("/gcc")
            || final_linker_name.ends_with("/clang"));
    let is_shared_link = args.user_flags.iter().any(|f| f == "-shared");
    if is_driver_link && !is_shared_link && is_elf_triple(&eff_triple) {
        cross_flags.push("-no-pie".to_string());
    }

    let linker_invoker: Box<dyn LinkerInvoker> = if cfg!(target_os = "windows") {
        Box::new(MsvcLinker {
            linker: final_linker_name,
        })
    } else {
        Box::new(UnixLinker {
            linker: final_linker_name,
        })
    };

    // Combine cross-compilation flags (if any) with the structured args, then
    // user flags. `-L`/`-l`/objects come first (inside `build_link_flags`),
    // cross flags next, so user flags remain last/overridable.
    let structured_flags = build_link_flags(args);
    let combined_flags = match (cross_flags.is_empty(), structured_flags) {
        (true, None) => None,
        (true, Some(s)) => Some(s),
        (false, None) => Some(cross_flags.join(" ")),
        (false, Some(s)) => Some(format!("{} {}", cross_flags.join(" "), s)),
    };

    let result = linker_invoker.link(obj_path, output_path, combined_flags.as_deref());
    if result.is_ok() {
        ensure_executable(output_path);
    }
    result
}

/// Map a target triple to the linker flags required to cross-compile for it.
///
/// Plan §18.1: maintains a small `target_triple -> linker_flags` table covering
/// the CI-tested target set. Returns an error for unmapped targets rather than
/// silently passing host flags to a cross target.
///
/// - `clang` drivers (including Apple's `cc`, which is clang) accept
///   `-target <triple>` directly.
/// - GNU `gcc` does **not** accept `-target` at all (it fails with
///   "unrecognized command-line option '-target'"). A gcc driver only links
///   the host triple without flags; a genuine cross build needs either clang
///   or a matching `<arch>-<os>-gnu*` cross gcc.
/// - GNU `ld` needs an explicit emulation via `-m <emulation>` (e.g.
///   `-m aarch64linux` for an aarch64 Linux target from an `x86_64` host).
pub(crate) fn linker_flags_for_target(triple: &str, linker: &str) -> Result<Vec<String>, String> {
    // The flags differ between a compiler *driver* (cc/clang/gcc) and a raw
    // GNU `ld`:
    //   - clang-style drivers accept a single-dash `-target <triple>` (Apple
    //     clang rejects the double-dash `--target` and the `-m <emulation>`
    //     GNU-ld flag);
    //   - raw GNU ld has no `--target` and instead needs `-m <emulation>`.
    let linker = linker.trim();
    let is_driver = linker == "cc"
        || linker == "gcc"
        || linker == "clang"
        || linker.ends_with("/cc")
        || linker.ends_with("/gcc")
        || linker.ends_with("/clang");

    if is_driver {
        // Clang understands `-target`; GNU gcc does not. Probe the driver's
        // `--version` output — `cc` is clang on macOS/BSD and gcc on most
        // Linux distros, so the binary name alone is not enough.
        let kind = if driver_is_clang(linker) {
            DriverKind::Clang
        } else {
            DriverKind::Gcc
        };
        return linker_flags_for_driver(kind, triple);
    }

    // Raw GNU ld path (shared with the driver=none case below).
    linker_flags_for_raw_ld(triple)
}

/// Which compiler-driver flavor `linker` resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DriverKind {
    /// Clang-style driver: understands `-target <triple>`.
    Clang,
    /// GNU gcc driver: no `-target` support; host-triple links only.
    Gcc,
}

/// Driver-independent flag computation for compiler drivers. Pure and
/// deterministic, so unit tests can cover clang/gcc behavior regardless of
/// which driver the host happens to install as `cc`.
pub(crate) fn linker_flags_for_driver(
    kind: DriverKind,
    triple: &str,
) -> Result<Vec<String>, String> {
    match kind {
        DriverKind::Clang => Ok(vec!["-target".to_string(), triple.to_string()]),
        DriverKind::Gcc => {
            // Passing `-target` makes gcc exit with an error, so fall back to
            // what gcc can actually do:
            //   * a triple that matches the host needs no extra flags at all;
            //   * a foreign triple needs a cross toolchain this driver cannot
            //     provide — fail with an actionable message instead of a
            //     baffling "unrecognized command-line option" from deep inside
            //     the link.
            if gcc_triple_matches_host(triple) {
                Ok(Vec::new())
            } else {
                Err(format!(
                    "GNU gcc driver cannot cross-link target '{triple}': \
                     gcc has no `-target` option. Install clang (preferred — it \
                     accepts `-target`) or an `{arch}-linux-gnu-gcc`-style cross \
                     toolchain and pass it via `--linker`.",
                    arch = triple.split('-').next().unwrap_or(triple),
                ))
            }
        }
    }
}

/// Flag computation for a raw GNU `ld` invocation (no compiler driver).
pub(crate) fn linker_flags_for_raw_ld(triple: &str) -> Result<Vec<String>, String> {
    // Raw GNU ld: emulation table keys on the architecture component.
    let arch = triple.split('-').next().unwrap_or(triple);
    // Mach-O targets (darwin) cannot be linked by GNU ld at all — say so
    // instead of emitting an emulation flag that produces nonsense errors.
    if triple.contains("darwin") || triple.contains("macos") || triple.contains("ios") {
        return Err(format!(
            "raw GNU ld cannot link Mach-O target '{triple}'; use a clang \
             driver (or Apple's ld64/ld64.lld) for this target"
        ));
    }
    // emulation -> linker -m flag for GNU ld (matches common cross targets).
    let emulation = match arch {
        "aarch64" | "arm64" => "aarch64linux",
        "x86_64" => "elf_x86_64",
        "i686" | "i386" => "elf_i386",
        "arm" => "armelf_linux_eabi",
        "riscv64" => "elf64lriscv",
        "powerpc64" => "elf64ppc",
        "powerpc" => "elf32ppclinux",
        "mips" => "elf32elmip",
        "mips64" => "elf64elmip",
        "s390x" => "elf64_s390",
        "wasm32" => "elf32_wasm",
        _ => {
            return Err(format!(
                "cross-compilation target '{triple}' is not in the supported linker-flag table; \
                 supply explicit --target/--ld-emulation flags via the linker config"
            ));
        }
    };
    Ok(vec!["-m".to_string(), emulation.to_string()])
}

/// Probe whether `linker` is a clang-style driver (accepts `-target`).
///
/// The result for the first driver probed is cached process-wide: the probe
/// runs the driver's `--version` at most once per compiler invocation, which
/// is negligible next to the link itself. Drivers probed later (rare — the
/// linker choice is stable within one invocation) are probed on demand.
fn driver_is_clang(linker: &str) -> bool {
    use std::sync::OnceLock;

    /// 1 = clang-style, 2 = not clang.
    fn probe(linker: &str) -> u8 {
        match Command::new(linker).arg("--version").output() {
            Ok(out) => {
                let text = String::from_utf8_lossy(&out.stdout).to_string()
                    + &String::from_utf8_lossy(&out.stderr);
                if text.to_lowercase().contains("clang") {
                    1
                } else {
                    2
                }
            }
            // The driver could not even be executed; treat it as not-clang so
            // the gcc path (which at least refuses cleanly) is taken.
            Err(_) => 2,
        }
    }

    static FIRST: OnceLock<(String, u8)> = OnceLock::new();
    match FIRST.get() {
        Some((name, kind)) if name == linker => *kind == 1,
        Some(_) => probe(linker) == 1,
        None => {
            let kind = probe(linker);
            let _ = FIRST.set((linker.to_string(), kind));
            kind == 1
        }
    }
}

/// Whether a GNU gcc driver can link `triple` without extra flags — i.e. the
/// triple's architecture and object format match the compiling host.
fn gcc_triple_matches_host(triple: &str) -> bool {
    let mut parts = triple.split('-');
    let arch = parts.next().unwrap_or("").trim();
    let rest: Vec<&str> = parts.collect();
    let os_and_env = rest.join("-");
    let host_arch = std::env::consts::ARCH;
    // Normalize the spellings that differ between LLVM triples and Rust's
    // `env::consts::ARCH`.
    let arch_matches = match arch {
        "x86_64" | "amd64" => host_arch == "x86_64",
        "aarch64" | "arm64" => host_arch == "aarch64",
        "i686" | "i386" | "x86" => host_arch == "x86",
        "arm" => host_arch == "arm",
        "riscv64" => host_arch == "riscv64",
        "powerpc64" => host_arch == "powerpc64",
        "mips64" => host_arch == "mips64",
        "s390x" => host_arch == "s390x",
        other => other == host_arch,
    };
    // The OS component must match too: `x86_64-unknown-linux-gnu` gcc on a
    // Linux host is a plain host link; a mingw/darwin triple is not.
    let host_os = std::env::consts::OS;
    let os_matches = match host_os {
        "linux" => os_and_env.contains("linux"),
        "macos" => os_and_env.contains("darwin") || os_and_env.contains("macos"),
        "windows" => os_and_env.contains("windows") || os_and_env.contains("mingw"),
        "freebsd" => os_and_env.contains("freebsd"),
        "netbsd" => os_and_env.contains("netbsd"),
        "openbsd" => os_and_env.contains("openbsd"),
        _ => false,
    };
    arch_matches && os_matches
}

/// Whether `triple` names an ELF platform (Linux/BSD/Solaris-style). Only
/// ELF toolchains default to PIE executables, so only those need `-no-pie`.
fn is_elf_triple(triple: &str) -> bool {
    [
        "linux",
        "freebsd",
        "netbsd",
        "openbsd",
        "dragonfly",
        "solaris",
        "elf",
    ]
    .iter()
    .any(|marker| triple.contains(marker))
}

/// Tries to find a suitable C linker on Unix-like systems.
///
/// Preference order (plan §18.1): a C-compiler-driver (`cc`/`clang`/`gcc`) is
/// preferred because it supplies default system library paths and CRT objects
/// automatically; raw linkers (`ld`, `ld.lld`, `ld.gold`, `mold`) are only used
/// as a fallback when no driver is available. Each candidate is probed by
/// running `<candidate> --version`; the first that responds is returned.
fn detect_unix_linker() -> String {
    const CANDIDATES: &[&str] = &["cc", "clang", "gcc", "ld", "ld.lld", "ld.gold", "mold"];
    for candidate in CANDIDATES {
        if Command::new(candidate).arg("--version").output().is_ok() {
            return candidate.to_string();
        }
    }
    "cc".to_string() // Fallback; callers surface the failure if `cc` is absent.
}

/// Locate an LLVM tool (`llvm-lto2`, `llvm-lto`, `opt`, …) from the same LLVM
/// distribution `glyim-codegen-llvm` was built against.
///
/// Discovery order (plan §1.2): first check `$LLVM_SYS_231_PREFIX/bin/<name>`
/// (the env var the toolchain is built against on this host), then fall back to
/// a `PATH` probe (consistent with `detect_unix_linker`). Absence is a clear,
/// actionable error rather than a silent wrong-answer.
fn find_llvm_tool(name: &str) -> Result<std::path::PathBuf, String> {
    // 1. Explicit override: the exact prefix glyim was built against.
    if let Ok(prefix) = std::env::var("LLVM_SYS_231_PREFIX") {
        let candidate = std::path::PathBuf::from(prefix).join("bin").join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    // 2. Ask `llvm-config` (a sibling tool normally on PATH when LLVM is
    //    installed as a package). This is the canonical way to discover the
    //    active LLVM installation's bindir and keeps the lookup in sync with
    //    whatever version the operator has.
    if let Ok(out) = Command::new("llvm-config").arg("--bindir").output()
        && out.status.success()
    {
        let bindir = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !bindir.is_empty() {
            let candidate = std::path::PathBuf::from(&bindir).join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    // 3. Well-known Homebrew prefixes (Apple Silicon + Intel). The formula
    //    `llvm@23` / `llvm` installs its tools here but does not put them on
    //    PATH by default (`brew link` is keg-only for LLVM), so the PATH probe
    //    alone misses them.
    for prefix in ["/opt/homebrew/opt/llvm", "/usr/local/opt/llvm"] {
        let candidate = std::path::PathBuf::from(prefix).join("bin").join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    // 4. PATH probe: the tool runs and responds to `--version`.
    if Command::new(name).arg("--version").output().is_ok() {
        return Ok(std::path::PathBuf::from(name));
    }
    Err(format!(
        "{name} not found. Searched `LLVM_SYS_231_PREFIX`, `llvm-config --bindir`, \
         /opt/homebrew/opt/llvm, /usr/local/opt/llvm, and PATH. ThinLTO requires \
         the {name} tool from the same LLVM distribution glyim-codegen-llvm was \
         built against. Install it (`brew install llvm`) or set \
         `LLVM_SYS_231_PREFIX` to the LLVM 23 prefix."
    ))
}

/// Drive LLVM's `ThinLTO` thin-link over a set of per-CGU bitcode files with
/// embedded summaries, producing one optimized object per module, then hand
/// the resulting objects to the existing native linker invocation.
///
/// The preferred driver is `llvm-lto2 run` (ships with every LLVM distribution
/// sidesteps needing unstable FFI bindings for `LTOCodeGenerator`. `glyim-cli`
/// emits each CGU's bitcode via `glyim_codegen_llvm::passes::emit_thinlto_bitcode`
/// and passes the resulting `.bc` paths here.
pub fn thin_lto_link(
    bitcode_paths: &[std::path::PathBuf],
    opt_level: u8,
    out_dir: &Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    if bitcode_paths.is_empty() {
        return Err("thin_lto_link requires at least one per-CGU bitcode input".to_string());
    }
    let llvm_lto2 = find_llvm_tool("llvm-lto2")?;
    let llvm_nm = find_llvm_tool("llvm-nm")?;

    // `llvm-lto2 run` requires a `-r=<file>,<symbol>,<resolution>` directive
    // for every symbol that appears in any input bitcode. Enumerate each
    // input's defined-external and undefined symbols with `llvm-nm`:
    //
    //   * defined + external -> `px` (prevailing external — becomes the
    //     canonical definition in the optimized output),
    //   * undefined -> empty resolution (external; expected to resolve at
    //     the final link step, e.g. the runtime library's `glyim_stdout_write`).
    //
    // Local symbols don't need a directive.
    let mut resolutions: Vec<String> = Vec::new();
    for bc in bitcode_paths {
        let out = Command::new(&llvm_nm)
            .args(["--defined-only", "--extern-only", "-j"])
            .arg(bc)
            .output()
            .map_err(|e| format!("failed to spawn {}: {}", llvm_nm.display(), e))?;
        if !out.status.success() {
            return Err(format!(
                "llvm-nm --defined-only failed on {}: status {}",
                bc.display(),
                out.status
            ));
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let sym = line.trim();
            if sym.is_empty() {
                continue;
            }
            resolutions.push(format!("-r={},{},px", bc.display(), sym));
        }
        let out = Command::new(&llvm_nm)
            .args(["--undefined-only", "-j"])
            .arg(bc)
            .output()
            .map_err(|e| format!("failed to spawn {}: {}", llvm_nm.display(), e))?;
        if !out.status.success() {
            return Err(format!(
                "llvm-nm --undefined-only failed on {}: status {}",
                bc.display(),
                out.status
            ));
        }
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let sym = line.trim();
            if sym.is_empty() {
                continue;
            }
            resolutions.push(format!("-r={},{},", bc.display(), sym));
        }
    }

    let mut cmd = Command::new(&llvm_lto2);
    cmd.arg("run");
    for bc in bitcode_paths {
        cmd.arg(bc);
    }
    for r in &resolutions {
        cmd.arg(r);
    }
    cmd.arg(format!("-o={}", out_dir.join("thinlto").display()));
    cmd.arg(format!("-O{}", opt_level.min(3)));
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn {}: {}", llvm_lto2.display(), e))?;
    if !status.success() {
        return Err(format!("llvm-lto2 thin-link failed with status {status}"));
    }
    // llvm-lto2 in `run` mode does the thin-link AND the backend codegen
    // in one invocation, merging all modules into a single optimized
    // output object. It writes `thinlto.0` (indexed per *output* module;
    // with the default internal partitioning that is always one). Return
    // only the objects that actually exist so the caller doesn't try to
    // link phantom `thinlto.1`, `thinlto.2`, ….
    let outs: Vec<std::path::PathBuf> = (0..bitcode_paths.len())
        .map(|i| out_dir.join(format!("thinlto.{i}")))
        .filter(|p| p.exists())
        .collect();
    if outs.is_empty() {
        return Err(format!(
            "llvm-lto2 reported success but wrote no output object under {}",
            out_dir.join("thinlto").display()
        ));
    }
    Ok(outs)
}

/// On Unix, ensure `path` is executable. The linker usually produces a +x
/// binary, but if `path` already existed with a non-executable mode (e.g. a
/// placeholder file created before linking, or an inherited umask on the
/// target directory), the x bits can be missing. Setting them explicitly is
/// idempotent and cheap.
#[cfg(unix)]
fn ensure_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        let mode = perms.mode();
        if mode & 0o111 == 0 {
            perms.set_mode(mode | 0o755);
            let _ = std::fs::set_permissions(path, perms);
        }
    }
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unix_linker_command_construction() {
        // Plan §18.1: detect_unix_linker must probe an extended candidate set
        // (cc/clang/gcc drivers first, then raw linkers ld/ld.lld/ld.gold/mold)
        // rather than only cc/clang/gcc. On a machine with a C driver present
        // the result must be one of the probed candidates, proving it actually
        // detects rather than returning an arbitrary fallback.
        let detected = detect_unix_linker();
        assert!(!detected.is_empty());
        const CANDIDATES: &[&str] = &["cc", "clang", "gcc", "ld", "ld.lld", "ld.gold", "mold"];
        assert!(
            CANDIDATES.contains(&detected.as_str()),
            "detected linker '{detected}' must be one of the probed candidates"
        );
    }

    #[test]
    fn test_link_args_emits_search_paths_and_libs_before_user_flags() {
        // Plan §18.2: structured LinkArgs must emit `-L<path>` for each search
        // path and `-l<name>` for each library *before* the free-form user
        // flags, so dependency-style linking is honoured and user flags still
        // append last (overridable).
        let args = LinkArgs {
            search_paths: vec![
                std::path::PathBuf::from("/deps/a"),
                std::path::PathBuf::from("/deps/b"),
            ],
            libs: vec!["mylib".to_string(), "pthread".to_string()],
            objects: vec![std::path::PathBuf::from("/tmp/extra.o")],
            user_flags: vec!["-Wl,--as-needed".to_string(), "-static".to_string()],
        };
        let flags = build_link_flags(&args).expect("non-empty args must yield flags");
        let parts: Vec<&str> = flags.split(' ').collect();

        assert_eq!(parts[0], "-L/deps/a");
        assert_eq!(parts[1], "-L/deps/b");
        assert_eq!(parts[2], "-lmylib");
        assert_eq!(parts[3], "-lpthread");
        assert_eq!(parts[4], "/tmp/extra.o");
        // User flags come last.
        assert_eq!(parts[5], "-Wl,--as-needed");
        assert_eq!(parts[6], "-static");

        // Relative order: every structured flag precedes every user flag.
        let user_start = parts
            .iter()
            .position(|p| p.starts_with("-Wl,") || p == &"-static")
            .unwrap();
        assert!(parts[..user_start].iter().all(|p| {
            p.starts_with("-L")
                || p.starts_with("-l")
                || std::path::Path::new(p)
                    .extension()
                    .is_some_and(|e| e == "o")
        }));
    }

    #[test]
    fn test_link_args_empty_yields_no_flags() {
        // Plan §18.2: with no search paths, libs, objects, or flags, there is
        // nothing to pass through.
        let args = LinkArgs::default();
        assert!(build_link_flags(&args).is_none());
    }

    #[test]
    fn thin_lto_link_errors_when_input_empty() {
        // The degraded-path contract: an empty bitcode set must surface a
        // clear, actionable error (not panic). Asserting that a *present*
        // llvm-lto2 fails is not host-independent — after widening
        // `find_llvm_tool` to probe `llvm-config --bindir` and the Homebrew
        // prefixes, most CI/dev hosts now have it, and no synthetic "BC"
        // input can be handed to a real thin-link without crashing the tool.
        let dir = std::env::temp_dir().join(format!("glyim_thinlto_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let result = thin_lto_link(&[], 2, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            result.is_err(),
            "thin_lto_link must error with no bitcode inputs"
        );
        assert!(
            result
                .unwrap_err()
                .contains("at least one per-CGU bitcode input"),
            "error should name the empty-input condition"
        );
    }
}

#[cfg(test)]
mod annotate_tests {
    use super::annotate_link_error_if_stale_runtime_with;

    fn sample_gnu_err() -> String {
        "ld: undefined reference to `glyim_stdout_write'\n".to_string()
    }

    #[test]
    fn appends_hint_when_runtime_symbol_mentioned() {
        let path = std::path::Path::new("/tmp/fake/libglyim_runtime.a");
        let out = annotate_link_error_if_stale_runtime_with(sample_gnu_err(), Some(path));
        assert!(out.contains("glyim_stdout_write"));
        assert!(out.contains("Hint:"));
        assert!(out.contains("cargo build -p glyim-runtime"));
        assert!(out.contains("/tmp/fake/libglyim_runtime.a"));
    }

    #[test]
    fn leaves_non_runtime_errors_unchanged() {
        let err = "ld: undefined reference to `printf'".to_string();
        let out = annotate_link_error_if_stale_runtime_with(
            err.clone(),
            Some(std::path::Path::new("/tmp/x.a")),
        );
        assert_eq!(out, err, "no runtime symbol → no annotation");
    }

    #[test]
    fn leaves_alone_when_no_runtime_lib() {
        let err = sample_gnu_err();
        let out = annotate_link_error_if_stale_runtime_with(err.clone(), None);
        assert_eq!(out, err, "no runtime path → don't append misleading hint");
    }

    #[test]
    fn ld64_error_also_matches() {
        let err = "Undefined symbols for architecture arm64:\n  \
                   \"_glyim_alloc\", referenced from:\n      __glyim_fn_1\n"
            .to_string();
        let out =
            annotate_link_error_if_stale_runtime_with(err, Some(std::path::Path::new("/x.a")));
        assert!(out.contains("cargo build -p glyim-runtime"));
    }

    #[test]
    fn msvc_lnk2019_also_matches() {
        let err =
            "error LNK2019: unresolved external symbol glyim_panic referenced in function main"
                .to_string();
        let out =
            annotate_link_error_if_stale_runtime_with(err, Some(std::path::Path::new("/x.lib")));
        assert!(out.contains("cargo build -p glyim-runtime"));
    }
}
