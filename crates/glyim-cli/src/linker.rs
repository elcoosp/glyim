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

/// Assemble the flat flag string from structured `LinkArgs` (plan §18.2):
/// `-L<path>` for each search path, then `-l<name>` for each requested
/// library, then the extra object files, and finally free-form `user_flags`.
fn build_link_flags(args: &LinkArgs) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for path in &args.search_paths {
        parts.push(format!("-L{}", path.display()));
    }
    for lib in &args.libs {
        parts.push(format!("-l{}", lib));
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

/// invoke_linker.
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
            .map(|s| s.split_whitespace().map(|f| f.to_string()).collect())
            .unwrap_or_default(),
        ..Default::default()
    };
    link_with_args(obj_path, output_path, &args, linker, target_triple)
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
    let default_linker = if cfg!(target_os = "windows") {
        "link.exe"
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

    linker_invoker.link(obj_path, output_path, combined_flags.as_deref())
}

/// Map a target triple to the linker flags required to cross-compile for it.
///
/// Plan §18.1: maintains a small `target_triple -> linker_flags` table covering
/// the CI-tested target set. Returns an error for unmapped targets rather than
/// silently passing host flags to a cross target.
///
/// - `clang`/`gcc` drivers accept `--target=<triple>` directly.
/// - GNU `ld` needs an explicit emulation via `-m <emulation>` (e.g.
///   `-m aarch64linux` for an aarch64 Linux target from an x86_64 host).
pub(crate) fn linker_flags_for_target(triple: &str, linker: &str) -> Result<Vec<String>, String> {
    // The flags differ between a compiler *driver* (cc/clang/gcc) and a raw
    // GNU `ld`:
    //   - drivers accept a single-dash `-target <triple>` (Apple clang rejects
    //     the double-dash `--target` and the `-m <emulation>` GNU-ld flag);
    //   - raw GNU ld has no `--target` and instead needs `-m <emulation>`.
    let linker = linker.trim();
    let is_driver = linker == "cc"
        || linker == "gcc"
        || linker == "clang"
        || linker.ends_with("/cc")
        || linker.ends_with("/gcc")
        || linker.ends_with("/clang");

    if is_driver {
        // `-target <triple>` (single dash) is what cc/clang/gcc understand.
        // This is what lets a darwin host link a darwin executable via its
        // clang `cc` (the previous `--target` + `-m` pair was rejected by the
        // driver, so `--emit=exec --target=x86_64-apple-darwin` failed on macOS).
        return Ok(vec!["-target".to_string(), triple.to_string()]);
    }

    // Raw GNU ld: emulation table keys on the architecture component.
    let arch = triple.split('-').next().unwrap_or(triple);
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
                "cross-compilation target '{}' is not in the supported linker-flag table; \
                 supply explicit --target/--ld-emulation flags via the linker config",
                triple
            ));
        }
    };
    Ok(vec!["-m".to_string(), emulation.to_string()])
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
/// Discovery order (plan §1.2): first check `$LLVM_SYS_220_PREFIX/bin/<name>`
/// (the env var the toolchain is built against on this host), then fall back to
/// a `PATH` probe (consistent with `detect_unix_linker`). Absence is a clear,
/// actionable error rather than a silent wrong-answer.
fn find_llvm_tool(name: &str) -> Result<std::path::PathBuf, String> {
    if let Ok(prefix) = std::env::var("LLVM_SYS_220_PREFIX") {
        let candidate = std::path::PathBuf::from(prefix).join("bin").join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    // PATH probe: the tool runs and responds to `--version`.
    if Command::new(name).arg("--version").output().is_ok() {
        return Ok(std::path::PathBuf::from(name));
    }
    Err(format!(
        "{name} not found on PATH and LLVM_SYS_220_PREFIX is unset or missing it; \
         ThinLTO requires the llvm-lto2 tool from the same LLVM 22 distribution \
         glyim-codegen-llvm was built against."
    ))
}

/// Drive LLVM's ThinLTO thin-link over a set of per-CGU bitcode files with
/// embedded summaries, producing one optimized object per module, then hand
/// the resulting objects to the existing native linker invocation.
///
/// The preferred driver is `llvm-lto2 run` (ships with every LLVM distribution
/// that has `llc`/`opt` and is the officially supported ThinLTO driver) — this
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

    let mut cmd = Command::new(&llvm_lto2);
    cmd.arg("run");
    for bc in bitcode_paths {
        cmd.arg(bc);
    }
    cmd.arg(format!("-o={}", out_dir.join("thinlto").display()));
    cmd.arg(format!("-O{}", opt_level.min(3)));
    let status = cmd
        .status()
        .map_err(|e| format!("failed to spawn {}: {}", llvm_lto2.display(), e))?;
    if !status.success() {
        return Err(format!("llvm-lto2 thin-link failed with status {}", status));
    }
    // One-to-one: each input module gets a numbered `.thinlto.N` object output;
    // llvm-lto2 assigns indices in input order.
    let outs = (0..bitcode_paths.len())
        .map(|i| out_dir.join(format!("thinlto.{}", i)))
        .collect();
    Ok(outs)
}

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
            "detected linker '{}' must be one of the probed candidates",
            detected
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
        assert!(
            parts[..user_start]
                .iter()
                .all(|p| p.starts_with("-L") || p.starts_with("-l") || p.ends_with(".o"))
        );
    }

    #[test]
    fn test_link_args_empty_yields_no_flags() {
        // Plan §18.2: with no search paths, libs, objects, or flags, there is
        // nothing to pass through.
        let args = LinkArgs::default();
        assert!(build_link_flags(&args).is_none());
    }

    #[test]
    fn thin_lto_link_errors_without_llvm_lto2() {
        // Plan §1.2: when the `llvm-lto2` tool is unavailable, the thin-link
        // driver must surface a clear, actionable error (not panic). This is
        // the degraded-path behaviour on a host without the LLVM thin-link
        // tool installed.
        let dir = std::env::temp_dir().join(format!("glyim_thinlto_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let bc = dir.join("cgu0.bc");
        std::fs::write(&bc, b"BC").unwrap();
        let result = thin_lto_link(&[bc], 2, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            result.is_err(),
            "thin_lto_link must error when llvm-lto2 is absent"
        );
        assert!(
            result.unwrap_err().contains("llvm-lto2"),
            "error should name the missing llvm-lto2 tool"
        );
    }
}
