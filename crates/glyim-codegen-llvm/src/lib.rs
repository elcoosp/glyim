//! Crate root.
// Stylistic clippy lints suppressed crate-wide (test-noise lints).
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::vec_init_then_push,
    clippy::assertions_on_constants,
    clippy::type_complexity,
    clippy::too_many_arguments,
    clippy::manual_c_str_literals,
    clippy::doc_lazy_continuation,
    clippy::empty_line_after_doc_comments,
    clippy::manual_strip,
    clippy::needless_range_loop,
    clippy::unnecessary_cast,
    clippy::clone_on_copy,
    clippy::mutable_key_type,
    clippy::only_used_in_recursion,
    clippy::let_unit_value,
    clippy::unnecessary_literal_unwrap,
    clippy::format_in_format_args,
    clippy::permissions_set_readonly_false,
    clippy::needless_lifetimes,
    clippy::collapsible_if
)]
use crate::passes::LtoKind;
use glyim_codegen::CodegenBackend;
use glyim_core::TargetInfo;
use glyim_diag::{CompResult, GlyimDiagnostic};
use glyim_mir::Body;
use glyim_span::{FileId, HygieneCtx};
use glyim_type::TyCtx;
use inkwell::context::Context;
use inkwell::targets::{InitializationConfig, Target, TargetTriple};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod abi;
mod debug;
mod lower;
/// passes.
pub mod passes;
pub mod seh_ffi;
mod types;

/// LlvmBackend.
pub struct LlvmBackend {
    context: Context,
    target_triple: String,
    ty_ctx_handle: Option<glyim_db::TyCtxHandle>,
    target_info: TargetInfo,
    debug_info: bool,
    source_map: HashMap<FileId, (String, String)>,
    opt_level: u8,
    opt_for_size: bool,
    hygiene_ctx: Option<HygieneCtx>,
    /// Link-time optimization strategy requested for this compilation
    /// (Phase 10.2). `None` is the default; `Fat` merges modules inside the
    /// compiler; `Thin` is a tracked gap (linker-driver integration).
    lto: crate::passes::LtoKind,
    /// Local `DefId` raw index of the crate's entry `main` function, if known.
    /// When set, codegen also emits a C-ABI `main` symbol (the OS/libc entry
    /// point) that calls the glyim `main` body (Phase: native executable
    /// output via `--emit=exec`). `None` means no entry symbol is required
    /// (library/cdylib/object-only emission).
    entry_main: Option<u32>,
}

impl Default for LlvmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl LlvmBackend {
    /// new.
    pub fn new() -> Self {
        Target::initialize_all(&InitializationConfig::default());
        let target_info = TargetInfo::default();
        let default_ctx = glyim_type::TyCtxMut::new(glyim_core::Interner::default()).freeze();
        Self {
            context: Context::create(),
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            ty_ctx_handle: Some(Arc::new(std::sync::RwLock::new(Some(Arc::new(
                default_ctx,
            ))))),
            target_info,
            debug_info: false,
            source_map: HashMap::new(),
            opt_level: 0,
            opt_for_size: false,
            hygiene_ctx: None,
            lto: crate::passes::LtoKind::None,
            entry_main: None,
        }
    }

    /// with_db.
    pub fn with_db(db: &glyim_db::Database) -> Self {
        Target::initialize_all(&InitializationConfig::default());
        // Read the triple from the DB's crate config, not a hardcoded
        // default. Otherwise every `with_db`-constructed backend emits
        // artifacts for Linux regardless of `--target`.
        let target_triple = db.config().target_triple.clone();
        let target_info = TargetInfo::from_triple(&target_triple);
        Self {
            context: Context::create(),
            target_triple,
            ty_ctx_handle: Some(db.ty_ctx_handle()),
            target_info,
            debug_info: false,
            source_map: HashMap::new(),
            opt_level: 0,
            opt_for_size: false,
            hygiene_ctx: None,
            lto: crate::passes::LtoKind::None,
            entry_main: None,
        }
    }

    /// with_hygiene_ctx.
    pub fn with_hygiene_ctx(mut self, hygiene: HygieneCtx) -> Self {
        self.hygiene_ctx = Some(hygiene);
        self
    }

    /// lower_bodies_to_module.
    pub fn lower_bodies_to_module<'ctx>(
        &self,
        context: &'ctx Context,
        bodies: &[Arc<Body>],
    ) -> CompResult<inkwell::module::Module<'ctx>> {
        let module = context.create_module("glyim_module");
        let triple = inkwell::targets::TargetTriple::create(&self.target_triple);
        module.set_triple(&triple);
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let ty_ctx = ty_ctx.as_ref();
        for body in bodies {
            crate::lower::lower_body(
                context,
                &module,
                body,
                self.target_info.clone(),
                ty_ctx,
                self.debug_info,
                self.source_map.clone(),
                self.hygiene_ctx.clone(),
                self.entry_main,
            )?;
        }
        Ok(module)
    }

    /// with_target.
    pub fn with_target(mut self, target_triple: impl Into<String>) -> Self {
        let triple = target_triple.into();
        self.target_info = TargetInfo::from_triple(&triple);
        self.target_triple = triple;
        self
    }

    /// with_ty_ctx_handle.
    pub fn with_ty_ctx_handle(mut self, handle: glyim_db::TyCtxHandle) -> Self {
        self.ty_ctx_handle = Some(handle);
        self
    }

    /// with_ty_ctx.
    pub fn with_ty_ctx(mut self, ctx: TyCtx) -> Self {
        let handle = Arc::new(std::sync::RwLock::new(Some(Arc::new(ctx))));
        self.ty_ctx_handle = Some(handle);
        self
    }

    /// with_debug_info.
    pub fn with_debug_info(mut self, enable: bool) -> Self {
        self.debug_info = enable;
        self
    }

    /// with_source_map.
    pub fn with_source_map(mut self, map: HashMap<FileId, (String, String)>) -> Self {
        self.source_map = map;
        self
    }

    /// with_opt_level.
    pub fn with_opt_level(mut self, level: u8) -> Self {
        self.opt_level = level;
        self
    }

    /// with_opt_for_size.
    pub fn with_opt_for_size(mut self, size: bool) -> Self {
        self.opt_for_size = size;
        self
    }

    /// Request a link-time optimization strategy for this compilation
    /// (Phase 10.2). `LtoKind::None` is the default; `Fat` merges modules
    /// inside the compiler; `Thin` is a tracked gap (linker-driver integration)
    /// and will surface an error at codegen time rather than silently no-op.
    pub fn with_lto(mut self, lto: crate::passes::LtoKind) -> Self {
        self.lto = lto;
        self
    }

    /// Declare which function (by its `LocalDefId` raw index) is the crate's
    /// entry `main`. When set, codegen emits a C-ABI `main` symbol that calls
    /// the glyim `main` body and returns 0, making the produced object linkable
    /// into a runnable executable via `--emit=exec` (Phase: native executable).
    pub fn with_entry_main(mut self, local_def_id_raw: u32) -> Self {
        self.entry_main = Some(local_def_id_raw);
        self
    }

    /// Generate LLVM IR for a single MIR body without needing to set the TyCtx in the backend.
    pub fn emit_ir_to_string(&self, ctx: &TyCtx, body: &Body) -> CompResult<String> {
        let context = Context::create();
        let module = self.lower_body_to_module_with_ctx(&context, body, ctx)?;
        Ok(module.print_to_string().to_string())
    }

    /// emit_ir_to_string_with_handle.
    pub fn emit_ir_to_string_with_handle(&self, body: &Body) -> CompResult<String> {
        let context = Context::create();
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let module = self.lower_body_to_module_with_ctx(&context, body, ty_ctx.as_ref())?;
        Ok(module.print_to_string().to_string())
    }

    /// Lower the given MIR bodies into a single LLVM module and emit **assembly**
    /// (`.s`) to `output`, honoring the configured target triple and opt level.
    /// Mirrors [`CodegenBackend::generate`] but uses `FileType::Assembly` instead
    /// of `FileType::Object` (plan §18.3: `--emit=asm`).
    pub fn emit_assembly(&self, bodies: &[Arc<Body>], output: &Path) -> CompResult<()> {
        let context = Context::create();
        let module = context.create_module("glyim_module");
        let triple = TargetTriple::create(&self.target_triple);
        module.set_triple(&triple);
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let ty_ctx = ty_ctx.as_ref();
        for body in bodies.iter() {
            crate::lower::lower_body(
                &context,
                &module,
                body,
                self.target_info.clone(),
                ty_ctx,
                self.debug_info,
                self.source_map.clone(),
                self.hygiene_ctx.clone(),
                self.entry_main,
            )?;
        }
        let target = Target::from_triple(&triple).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "Target error: {}",
                e
            ))]
        })?;
        let opt_level = match self.opt_level {
            0 => inkwell::OptimizationLevel::None,
            1 => inkwell::OptimizationLevel::Less,
            2 => inkwell::OptimizationLevel::Default,
            _ => inkwell::OptimizationLevel::Aggressive,
        };
        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt_level,
                inkwell::targets::RelocMode::Default,
                inkwell::targets::CodeModel::Default,
            )
            .ok_or_else(|| {
                vec![GlyimDiagnostic::internal_error(
                    "Failed to create target machine",
                )]
            })?;
        self.run_passes_on_module(&module, &target_machine)
            .map_err(|e| vec![GlyimDiagnostic::internal_error(e)])?;
        target_machine
            .write_to_file(&module, inkwell::targets::FileType::Assembly, output)
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "Failed to write assembly file: {:?}",
                    e
                ))]
            })?;
        Ok(())
    }

    /// Lower a single body to an LLVM module using the provided TyCtx.
    pub fn lower_body_to_module_with_ctx<'ctx>(
        &self,
        context: &'ctx Context,
        body: &Body,
        ctx: &TyCtx,
    ) -> CompResult<inkwell::module::Module<'ctx>> {
        let module = context.create_module("glyim_module");
        let triple = TargetTriple::create(&self.target_triple);
        module.set_triple(&triple);
        crate::lower::lower_body(
            context,
            &module,
            body,
            self.target_info.clone(),
            ctx,
            self.debug_info,
            self.source_map.clone(),
            self.hygiene_ctx.clone(),
            self.entry_main,
        )?;
        Ok(module)
    }

    pub(crate) fn run_passes_on_module<'ctx>(
        &self,
        module: &inkwell::module::Module<'ctx>,
        target_machine: &inkwell::targets::TargetMachine,
    ) -> Result<(), String> {
        // Honour the requested link-time optimization strategy (Phase 10.2).
        // `run_lto` treats `None`/`Fat` as running the standard pipeline once
        // (Fat with no secondary modules is just a single-module pass run), and
        // surfaces `Thin` as an explicit tracked-gap error rather than silently
        // no-op. Full multi-module merge requires the multi-CGU compilation
        // driver, which is a tracked gap (KNOWN_GAPS.md Phase 10.2).
        crate::passes::run_lto(
            module,
            &[],
            self.lto,
            target_machine,
            self.opt_level,
            self.opt_for_size,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn lower_body_to_module<'ctx>(
        &self,
        context: &'ctx Context,
        body: &Body,
    ) -> CompResult<inkwell::module::Module<'ctx>> {
        let module = context.create_module("test_module");
        let triple = inkwell::targets::TargetTriple::create(&self.target_triple);
        module.set_triple(&triple);
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let ty_ctx = ty_ctx.as_ref();
        crate::lower::lower_body(
            context,
            &module,
            body,
            self.target_info.clone(),
            ty_ctx,
            self.debug_info,
            self.source_map.clone(),
            self.hygiene_ctx.clone(),
            self.entry_main,
        )?;
        Ok(module)
    }

    #[allow(dead_code)]
    pub(crate) fn generate_ir(&self, body: &Body) -> CompResult<String> {
        let context = Context::create();
        let module = self.lower_body_to_module(&context, body)?;
        Ok(module.print_to_string().to_string())
    }

    /// Emit one ThinLTO bitcode file per body (per codegen unit) into
    /// `out_dir`, returning the written `.bc` paths in body order.
    ///
    /// Each CGU's bitcode carries its own module summary so `glyim-cli`'s
    /// thin-link driver (`linker::thin_lto_link`, which shells out to
    /// `llvm-lto2`) can combine them incrementally across CGUs — this is the
    /// backend half of `LtoKind::Thin`. The bitcode is intentionally emitted
    /// *unoptimized*; cross-CGU optimization happens during the thin-link, not
    /// here (this is what distinguishes ThinLTO from `Fat` LTO, which merges
    /// everything into one module up front). See `passes::emit_thinlto_bitcode`
    /// for the per-module writer and `KNOWN_GAPS.md` Phase 10.2 for the design.
    pub fn emit_thinlto_bitcode_files(
        &self,
        bodies: &[Arc<Body>],
        out_dir: &Path,
    ) -> CompResult<Vec<PathBuf>> {
        let context = &self.context;
        let triple = TargetTriple::create(&self.target_triple);
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let ty_ctx = ty_ctx.as_ref();

        let mut paths = Vec::with_capacity(bodies.len());
        // The bitcode writer needs a data layout on the module; LLVM 22 emits an
        // empty (0-byte) bitcode file for a content-bearing module that has no
        // layout string set. Derive it from the target so the per-module summary
        // the thin-link consumes (via `glyim-cli`'s `llvm-lto2` driver) is
        // well-formed.
        let target_machine = Target::from_triple(&triple)
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "Target error: {}",
                    e
                ))]
            })?
            .create_target_machine(
                &triple,
                "generic",
                "",
                inkwell::OptimizationLevel::None,
                inkwell::targets::RelocMode::Default,
                inkwell::targets::CodeModel::Default,
            )
            .ok_or_else(|| {
                vec![GlyimDiagnostic::internal_error(
                    "Failed to create target machine",
                )]
            })?;
        let data_layout = target_machine.get_target_data().get_data_layout();
        for (i, body) in bodies.iter().enumerate() {
            let module = context.create_module(&format!("glyim_cgu_{}", i));
            module.set_triple(&triple);
            module.set_data_layout(&data_layout);
            crate::lower::lower_body(
                context,
                &module,
                body,
                self.target_info.clone(),
                ty_ctx,
                self.debug_info,
                self.source_map.clone(),
                self.hygiene_ctx.clone(),
                self.entry_main,
            )?;
            // No optimization here: ThinLTO optimizes per-CGU during the
            // thin-link once all summaries are visible.
            let out_path = out_dir.join(format!("cgu_{}.bc", i));
            crate::passes::emit_thinlto_bitcode(&module, &target_machine, &out_path)
                .map_err(|e| vec![GlyimDiagnostic::internal_error(e)])?;
            paths.push(out_path);
        }
        Ok(paths)
    }
}

impl CodegenBackend for LlvmBackend {
    fn name(&self) -> &'static str {
        "llvm"
    }

    fn generate(&self, bodies: &[Arc<Body>], output: &Path) -> CompResult<()> {
        // ThinLTO does not produce a single merged object here: per-CGU bitcode
        // emission + the thin-link are driven by `glyim-cli` via
        // `emit_thinlto_bitcode_files` + the `thin_lto_link` driver. Bailing out
        // early keeps `compile_file_with_artifacts` (which calls this) from
        // writing a redundant merged object that the thin path would overwrite.
        if self.lto == LtoKind::Thin {
            return Ok(());
        }
        let context = &self.context;
        let module = context.create_module("glyim_module");
        let triple = TargetTriple::create(&self.target_triple);
        module.set_triple(&triple);
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let ty_ctx = ty_ctx.as_ref();
        for body in bodies.iter() {
            crate::lower::lower_body(
                context,
                &module,
                body,
                self.target_info.clone(),
                ty_ctx,
                self.debug_info,
                self.source_map.clone(),
                self.hygiene_ctx.clone(),
                self.entry_main,
            )?;
        }
        let target = Target::from_triple(&triple).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "Target error: {}",
                e
            ))]
        })?;
        let opt_level = match self.opt_level {
            0 => inkwell::OptimizationLevel::None,
            1 => inkwell::OptimizationLevel::Less,
            2 => inkwell::OptimizationLevel::Default,
            _ => inkwell::OptimizationLevel::Aggressive,
        };
        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt_level,
                inkwell::targets::RelocMode::Default,
                inkwell::targets::CodeModel::Default,
            )
            .ok_or_else(|| {
                vec![GlyimDiagnostic::internal_error(
                    "Failed to create target machine",
                )]
            })?;
        self.run_passes_on_module(&module, &target_machine)
            .map_err(|e| vec![GlyimDiagnostic::internal_error(e)])?;
        target_machine
            .write_to_file(&module, inkwell::targets::FileType::Object, output)
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "Failed to write object file: {:?}",
                    e
                ))]
            })?;
        // `write_to_file` (LLVM's raw_fd_ostream) writes via write(2) then
        // close(2). On macOS `close(2)` is asynchronous: the file's *length*
        // is visible to stat(2) immediately, but the page-cache contents may
        // still be pending, so a linker that mmaps the file can read zeroed
        // pages and report `ld: file is empty in <path>` even though the file
        // has bytes. Repro: `cargo run -p glyim-cli -- two_step.g ... --emit
        // exec` fails ~30% of the time; the same invocation of the built
        // binary directly fails 0/50.
        //
        // Reopen for *read+write* and `sync_all`. Opening read-only (the
        // obvious first attempt) does NOT work: fsync on a read-only fd is a
        // no-op on macOS, so the pending pages stay pending.
        if let Ok(f) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(output)
        {
            let _ = f.sync_all();
        }
        Ok(())
    }

    fn generate_function(&self, body: &Arc<Body>) -> CompResult<Vec<u8>> {
        let context = &self.context;
        let module = context.create_module("glyim_func");
        let triple = TargetTriple::create(&self.target_triple);
        module.set_triple(&triple);
        let ty_ctx = self
            .ty_ctx_handle
            .as_ref()
            .and_then(|h| h.read().unwrap().clone())
            .ok_or_else(|| vec![GlyimDiagnostic::internal_error("no TyCtx available")])?;
        let ty_ctx = ty_ctx.as_ref();
        crate::lower::lower_body(
            context,
            &module,
            body,
            self.target_info.clone(),
            ty_ctx,
            self.debug_info,
            self.source_map.clone(),
            self.hygiene_ctx.clone(),
            self.entry_main,
        )?;
        let target = Target::from_triple(&triple).map_err(|e| {
            vec![GlyimDiagnostic::internal_error(format!(
                "Target error: {}",
                e
            ))]
        })?;
        let opt_level = match self.opt_level {
            0 => inkwell::OptimizationLevel::None,
            1 => inkwell::OptimizationLevel::Less,
            2 => inkwell::OptimizationLevel::Default,
            _ => inkwell::OptimizationLevel::Aggressive,
        };
        let target_machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt_level,
                inkwell::targets::RelocMode::Default,
                inkwell::targets::CodeModel::Default,
            )
            .ok_or_else(|| {
                vec![GlyimDiagnostic::internal_error(
                    "Failed to create target machine",
                )]
            })?;
        self.run_passes_on_module(&module, &target_machine)
            .map_err(|e| vec![GlyimDiagnostic::internal_error(e)])?;
        target_machine
            .write_to_memory_buffer(&module, inkwell::targets::FileType::Object)
            .map(|buf| buf.as_slice().to_vec())
            .map_err(|e| {
                vec![GlyimDiagnostic::internal_error(format!(
                    "Failed to generate object code: {:?}",
                    e
                ))]
            })
    }
}

#[cfg(test)]
mod tests;
