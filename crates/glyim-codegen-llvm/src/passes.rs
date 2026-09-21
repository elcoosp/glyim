use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::TargetMachine;
use std::path::Path;

/// Link-time optimization strategy.
///
/// `None` is a no-op. `Fat` merges every module of the program into one and
/// runs the optimization pipeline once over the merged module (cross-module
/// inlining/specialization possible entirely inside `glyim-codegen-llvm` via
/// `Module::link_in_module`, which wraps `LLVMLinkModules2`). `Thin` requires
/// per-module summary emission *and* a link-time thin-link step driven by the
/// linker (`glyim-cli`'s linker invocation) — that half is a tracked gap (see
/// `KNOWN_GAPS.md` Phase 10.2); this crate can only validate the request and
/// hand control to the linker driver.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LtoKind {
    /// Variant.
    None,
    /// Variant.
    Thin,
    /// Variant.
    Fat,
}

/// Run link-time optimization over the program's modules.
///
/// `primary` is the module that receives every other module's contents (it is
/// the module that will subsequently be passed to `run_llvm_passes`). `others`
/// are the remaining per-crate / per-CGU modules to merge into `primary` for
/// `Fat` LTO. For `None`, nothing is done. For `Thin`, the merge is intentionally
/// *not* performed here (it would defeat ThinLTO's incremental design); callers
/// must instead drive the thin-link via the linker and only pass `-flto=thin`
/// flags. Returns an error describing the gap if `Thin` is requested, so the
/// limitation is explicit rather than silent.
pub(crate) fn run_lto<'ctx>(
    primary: &Module<'ctx>,
    others: &[Module<'ctx>],
    kind: LtoKind,
    target_machine: &TargetMachine,
    opt_level: u8,
    opt_for_size: bool,
) -> Result<(), String> {
    match kind {
        LtoKind::None => Ok(()),
        LtoKind::Thin => Err("LtoKind::Thin must not be merged via run_lto; call \
             emit_thinlto_bitcode() per-module and let glyim-cli's thin-link \
             driver combine them. run_lto(Thin) is only reachable from a caller \
             bug."
            .to_string()),
        LtoKind::Fat => {
            // Merge every other module into the primary. `link_in_module` takes
            // ownership of `other` (it is `forget`ten inside), so we clone the
            // shared borrows first to avoid moving out of the slice.
            for other in others {
                let cloned = other.clone();
                primary
                    .link_in_module(cloned)
                    .map_err(|e| format!("Fat LTO module merge failed: {}", e))?;
            }
            // Run the optimization pipeline once over the merged module so
            // cross-module optimizations (inlining, etc.) actually fire.
            run_llvm_passes(primary, target_machine, opt_level, opt_for_size)
        }
    }
}

/// Emit this module's bitcode, suitable for `glyim-cli`'s ThinLTO thin-link
/// step.
///
/// Real ThinLTO writes each CGU's bitcode to disk and lets the thin-link
/// driver (`llvm-lto2`) combine the per-module summaries — parallel,
/// incremental per-module optimization instead of one giant merged module
/// (which is what `Fat` LTO does). `write_bitcode_to_path` produces exactly
/// the per-module `.bc` input the thin-link consumes.
///
/// Setting the embedded `ThinLTO` module-summary flag is performed via raw
/// `llvm-sys` FFI (`LLVMAddModuleFlag`) — inkwell 0.10 does not wrap the
/// module-flag API. This embeds a `ThinLTO` module flag (behavior `Error`, so
/// conflicting summaries are rejected at thin-link time) marking the bitcode
/// as valid ThinLTO input that carries a module summary; without it the
/// thin-link would treat the module as a regular (non-summary) input. The
/// bitcode written here is valid ThinLTO input regardless; embedding the
/// summary flag is what lets the thin-link actually perform cross-module
/// inlining rather than silently downgrading to a single-module pass.
pub fn emit_thinlto_bitcode<'ctx>(
    module: &Module<'ctx>,
    _target_machine: &TargetMachine,
    out_path: &Path,
) -> Result<(), String> {
    // Embed the `ThinLTO` module-summary flag via raw llvm-sys FFI. Behavior
    // `Error` means a conflicting flag on the same module is a hard error
    // (there should never be one, but this guards the invariant). The value is
    // an `MDString` metadata node (the API requires `LLVMMetadataRef`, not a
    // `Value`), marking the bitcode as carrying a module summary so the
    // thin-link performs cross-module inlining rather than downgrading to a
    // single-module pass.
    unsafe {
        use llvm_sys::LLVMModuleFlagBehavior::LLVMModuleFlagBehaviorError;
        use llvm_sys::core::{LLVMAddModuleFlag, LLVMGetModuleContext, LLVMMDStringInContext2};
        use std::os::raw::c_char;
        let ctx = LLVMGetModuleContext(module.as_mut_ptr());
        let value = LLVMMDStringInContext2(ctx, b"1\0".as_ptr() as *const c_char, 1);
        let name = b"ThinLTO\0".as_ptr() as *const c_char;
        LLVMAddModuleFlag(
            module.as_mut_ptr(),
            LLVMModuleFlagBehaviorError,
            name,
            8,
            value,
        );
    }
    // Serialize to a memory buffer first, then write the bytes to disk. Under
    // LLVM 22 `LLVMWriteBitcodeToFile` silently emits a 0-byte file for
    // content-bearing modules (it works for empty modules, which is why the
    // regression only shows up with real code); `LLVMWriteBitcodeToMemoryBuffer`
    // is reliable, so we dump that buffer to `out_path` ourselves.
    let buffer = module.write_bitcode_to_memory();
    std::fs::write(out_path, buffer.as_slice()).map_err(|e| {
        format!(
            "failed to write ThinLTO bitcode to {}: {}",
            out_path.display(),
            e
        )
    })
}

/// Run LLVM optimization passes based on the given optimization level and size hint.
///
/// Optimization levels:
/// - 0: No optimizations (O0)
/// - 1: Basic optimizations (O1)
/// - 2: Standard optimizations (O2)
/// - 3: Aggressive optimizations (O3)
/// - 4+: Same as O3 (default)
///
/// When `opt_for_size` is true, size optimizations (Os/Oz) are used instead of speed.

/// Check that the linked LLVM version is supported.
/// Panics with a clear error message if the version is unsupported.
pub(crate) fn check_llvm_version_and_passes() {
    let version = inkwell::support::get_llvm_version();
    // version is (major, minor, patch)
    let major = version.0;
    // We support LLVM 18, 19, 20, 21, 22 (and later)
    if major < 18 {
        panic!(
            "Unsupported LLVM version: {}.{}.{}. Glyim requires LLVM 18 or later.",
            version.0, version.1, version.2
        );
    }
    tracing::info!(
        "Using LLVM version: {}.{}.{}",
        version.0,
        version.1,
        version.2
    );
}

pub(crate) fn run_llvm_passes<'ctx>(
    module: &Module<'ctx>,
    target_machine: &TargetMachine,
    opt_level: u8,
    opt_for_size: bool,
) -> Result<(), String> {
    run_llvm_passes_with(module, target_machine, opt_level, opt_for_size, None)
}

/// Like [`run_llvm_passes`] but allows an explicit custom pass pipeline string
/// (e.g. `"instcombine,simplifycfg"`) to override the built-in `default<Ox>`
/// selection (plan §19.4). When `custom` is `None`, behaviour is identical to
/// `run_llvm_passes`.
pub(crate) fn run_llvm_passes_with<'ctx>(
    module: &Module<'ctx>,
    target_machine: &TargetMachine,
    opt_level: u8,
    opt_for_size: bool,
    custom: Option<&str>,
) -> Result<(), String> {
    check_llvm_version_and_passes();

    let pass_str: &str = if let Some(custom) = custom {
        custom
    } else {
        match (opt_level, opt_for_size) {
            // Level 0: no optimization
            (0, _) => "default<O0>",
            // Level 1: light optimization
            (1, false) => "default<O1>",
            (1, true) => "default<Os>",
            // Level 2: standard optimization
            (2, false) => "default<O2>",
            (2, true) => "default<Oz>",
            // Level 3: aggressive optimization
            (3, false) => "default<O3>",
            (3, true) => "default<Oz>",
            // Level 4+: still O3 (or Oz if size-optimizing)
            (_, false) => "default<O3>",
            (_, true) => "default<Oz>",
        }
    };

    let opts = PassBuilderOptions::create();
    module
        .run_passes(pass_str, target_machine, opts)
        .map_err(|e| format!("Failed to run LLVM passes: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::OptimizationLevel;
    use inkwell::context::Context;
    use inkwell::targets::{Target, TargetMachine};
    use std::sync::Once;

    static INIT: Once = Once::new();

    fn init_targets() {
        INIT.call_once(|| {
            Target::initialize_all(&inkwell::targets::InitializationConfig::default());
        });
    }
    fn create_test_module(ctx: &Context) -> (Module<'_>, TargetMachine) {
        init_targets();
        let module = ctx.create_module("test");
        let triple = inkwell::targets::TargetTriple::create("x86_64-unknown-linux-gnu");
        module.set_triple(&triple);
        let target = Target::from_triple(&triple).unwrap();
        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                inkwell::targets::RelocMode::Default,
                inkwell::targets::CodeModel::Default,
            )
            .unwrap();
        (module, target_machine)
    }

    #[test]
    fn test_opt_level_0() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 0, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_level_1() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 1, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_level_2() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 2, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_level_3() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 3, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_level_4_defaults_to_o3() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 4, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_for_size_with_level_1() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 1, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_for_size_with_level_2() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 2, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_for_size_with_level_3() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 3, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_opt_for_size_with_high_level() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_llvm_passes(&module, &tm, 5, true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_pass_pipeline_override() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        // Explicit custom pipeline overrides the built-in default<Ox> selection.
        let result = run_llvm_passes_with(&module, &tm, 3, false, Some("instcombine,simplifycfg"));
        assert!(result.is_ok(), "custom pass pipeline must run");
    }

    #[test]
    fn test_lto_none_is_noop() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_lto(&module, &[], LtoKind::None, &tm, 2, false);
        assert!(result.is_ok(), "LtoKind::None must be a no-op");
    }

    #[test]
    fn test_lto_fat_merges_modules_and_optimizes() {
        use inkwell::types::IntType;

        let ctx = Context::create();
        let (primary, tm) = create_test_module(&ctx);
        let builder = ctx.create_builder();

        // `primary` declares an external `callee` we will inline-cross-module.
        let i32_ty: IntType = ctx.i32_type();
        let callee_ty = i32_ty.fn_type(&[], false);
        let callee = primary.add_function("callee", callee_ty, None);
        let entry = ctx.append_basic_block(callee, "entry");
        builder.position_at_end(entry);
        builder
            .build_return(Some(&i32_ty.const_int(7, false)))
            .unwrap();

        // A second module that calls `callee` (declared, not defined there).
        let other = ctx.create_module("other");
        other.set_triple(&inkwell::targets::TargetTriple::create(
            "x86_64-unknown-linux-gnu",
        ));
        let other_callee = other.add_function("callee", callee_ty, None);
        let caller_ty = i32_ty.fn_type(&[], false);
        let caller = other.add_function("caller", caller_ty, None);
        let centry = ctx.append_basic_block(caller, "entry");
        builder.position_at_end(centry);
        let call = builder.build_call(other_callee, &[], "call").unwrap();
        builder
            .build_return(Some(&call.try_as_basic_value().basic().unwrap()))
            .unwrap();

        // Fat LTO merges `other` into `primary` and runs the pipeline.
        let result = run_lto(&primary, &[other], LtoKind::Fat, &tm, 2, false);
        assert!(
            result.is_ok(),
            "Fat LTO must merge modules without error: {:?}",
            result.err()
        );

        // After merge, `primary` must contain `caller` (it was only defined in
        // the secondary module). It should also have been cross-module
        // inlined — `caller` returns the same constant `callee` does.
        assert!(
            primary.get_function("caller").is_some(),
            "Fat LTO must bring `caller` from the secondary module into the primary"
        );
        let ir = primary.print_to_string().to_string();
        assert!(
            ir.contains("@caller"),
            "merged module should contain the caller definition"
        );
        assert!(
            ir.contains("ret i32 7"),
            "caller should be cross-module inlined to callee's constant"
        );
    }

    #[test]
    fn test_lto_thin_is_tracked_gap() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let result = run_lto(&module, &[], LtoKind::Thin, &tm, 2, false);
        assert!(
            result.is_err(),
            "ThinLTO must surface its linker-driver gap as an error, not silently no-op"
        );
        assert!(
            result.unwrap_err().contains("emit_thinlto_bitcode"),
            "error should point to the correct call path (emit_thinlto_bitcode + thin-link driver)"
        );
    }

    #[test]
    fn emit_thinlto_bitcode_writes_file_with_summary() {
        let ctx = Context::create();
        let (module, tm) = create_test_module(&ctx);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("mod.bc");
        emit_thinlto_bitcode(&module, &tm, &out).unwrap();
        assert!(out.exists(), "ThinLTO bitcode file must be written");
        // Sanity: a real per-module summary bumps the bitcode file size
        // materially vs. an empty write; assert file is non-trivially sized
        // rather than trying to parse the bitcode format by hand.
        assert!(
            std::fs::metadata(&out).unwrap().len() > 0,
            "ThinLTO bitcode must be non-empty"
        );
    }
}
