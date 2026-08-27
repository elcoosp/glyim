#![allow(dead_code)]
use glyim_core::TargetInfo;
use glyim_layout::{Layout, LayoutComputer, SimpleLayoutComputer};
use glyim_mir::VarDebugInfo;
use glyim_span::{FileId, HygieneCtx, Span};
use glyim_type::TyCtx;
use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::debug_info::{
    AsDIScope, DIFile, DIFlags, DIFlagsConstants, DIScope, DISubprogram, DIType, DWARFEmissionKind,
    DWARFSourceLanguage, DebugInfoBuilder,
};
use inkwell::values::FunctionValue;
use inkwell::values::AsValueRef;
use std::collections::HashMap;

/// Walk back through macro expansions to find the original source location.
pub(crate) fn resolve_span_to_location(mut span: Span, hygiene: &HygieneCtx) -> Span {
    if span.is_dummy() {
        return Span::DUMMY;
    }
    while !span.ctx.is_root() {
        let expn_id = span.ctx.expn_id();
        match hygiene.expn_data(expn_id) {
            Some(expn_data) => span = expn_data.call_site,
            None => break,
        }
    }
    span
}

pub(crate) struct DebugInfoCtx<'ctx> {
    pub(crate) builder: DebugInfoBuilder<'ctx>,
    pub(crate) compile_unit_scope: DIScope<'ctx>,
    pub(crate) subprogram: Option<DISubprogram<'ctx>>,
    files: HashMap<FileId, DIFile<'ctx>>,
    source_map: HashMap<FileId, (String, String)>,
    pub(crate) enabled: bool,
    hygiene: Option<HygieneCtx>,
    type_cache: HashMap<glyim_type::Ty, DIType<'ctx>>,
    line_tables: HashMap<FileId, Vec<usize>>, // line starts
}

impl<'ctx> DebugInfoCtx<'ctx> {
    pub(crate) fn new(
        _context: &'ctx Context,
        module: &inkwell::module::Module<'ctx>,
        source_map: HashMap<FileId, (String, String)>,
        enable: bool,
        hygiene: Option<HygieneCtx>,
    ) -> Self {
        let (builder, compile_unit) = module.create_debug_info_builder(
            true,
            DWARFSourceLanguage::Rust,
            "test.g",
            ".",
            "glyim",
            false,
            "",
            0u32,
            "",
            DWARFEmissionKind::Full,
            0u32,
            true,
            false,
            "",
            "",
        );
        let compile_unit_scope = compile_unit.as_debug_info_scope();
        let mut files = HashMap::new();
        let mut line_tables = HashMap::new();
        for (file_id, (path, source)) in &source_map {
            let dir = std::path::Path::new(path)
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or(".");
            let filename = std::path::Path::new(path)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(path);
            let file = builder.create_file(filename, dir);
            files.insert(*file_id, file);
            // Build line table.
            let mut starts = vec![0];
            let mut offset = 0;
            for ch in source.chars() {
                if ch == '\n' {
                    offset += 1;
                    starts.push(offset);
                } else {
                    offset += ch.len_utf8();
                }
            }
            line_tables.insert(*file_id, starts);
        }
        DebugInfoCtx {
            builder,
            compile_unit_scope,
            subprogram: None,
            files,
            source_map,
            enabled: enable,
            hygiene,
            type_cache: HashMap::new(),
            line_tables,
        }
    }

    pub(crate) fn set_function(
        &mut self,
        _context: &'ctx Context,
        func: &FunctionValue<'ctx>,
        name: &str,
        file_id: FileId,
        line: u32,
    ) {
        if !self.enabled {
            return;
        }
        let file = self.get_file(file_id);
        let subroutine_type = self
            .builder
            .create_subroutine_type(file, None, &[], DIFlags::ZERO);
        let subprogram = self.builder.create_function(
            self.compile_unit_scope,
            name,
            Some(name),
            file,
            line,
            subroutine_type,
            false,
            true,
            line,
            DIFlags::ZERO,
            false,
        );
        func.set_subprogram(subprogram);
        self.subprogram = Some(subprogram);
    }

    pub(crate) fn location_for_span(
        &self,
        context: &'ctx Context,
        span: &Span,
    ) -> Option<inkwell::debug_info::DILocation<'ctx>> {
        if !self.enabled || self.subprogram.is_none() {
            return None;
        }
        let resolved_span = if let Some(ref hygiene) = self.hygiene {
            resolve_span_to_location(*span, hygiene)
        } else {
            *span
        };
        let (line, col) = self.span_to_line_col(&resolved_span)?;
        let scope = self.subprogram.unwrap().as_debug_info_scope();
        Some(
            self.builder
                .create_debug_location(context, line, col, scope, None),
        )
    }

    /// Get a debug type for a given Ty, creating it if necessary.
    pub(crate) fn debug_type_for_ty(
        &mut self,
        context: &'ctx Context,
        ty: glyim_type::Ty,
        ty_ctx: &TyCtx,
    ) -> DIType<'ctx> {
        use glyim_type::{GenericArg, TyKind};
        let target = TargetInfo::default();
        let layout_computer = SimpleLayoutComputer::new(ty_ctx, target.clone());

        if let Some(cached) = self.type_cache.get(&ty) {
            return *cached;
        }

        let file = self.get_file(FileId::from_raw(0));
        let di_type = match ty_ctx.ty_kind(ty) {
            TyKind::Bool => self
                .builder
                .create_basic_type("bool", 8, 0x04, 0)
                .unwrap()
                .as_type(),
            TyKind::Int(i) => {
                let bits = i.bit_width(&target);
                self.builder
                    .create_basic_type(i.name(), bits.into(), 0x04, 0)
                    .unwrap()
                    .as_type()
            }
            TyKind::Uint(u) => {
                let bits = u.bit_width(&target);
                self.builder
                    .create_basic_type(u.name(), bits.into(), 0x04, 0)
                    .unwrap()
                    .as_type()
            }
            TyKind::Float(f) => {
                let bits = f.bit_width();
                self.builder
                    .create_basic_type(f.name(), bits.into(), 0x05, 0)
                    .unwrap()
                    .as_type()
            }
            TyKind::Char => self
                .builder
                .create_basic_type("char", 32, 0x04, 0)
                .unwrap()
                .as_type(),
            TyKind::String => {
                // str is represented as a fat pointer? Actually it's a DIFile? We'll treat as opaque.
                self.builder
                    .create_basic_type("str", 8, 0x02, 0)
                    .unwrap()
                    .as_type()
            }
            TyKind::Never | TyKind::Unit => self
                .builder
                .create_basic_type("()", 8, 0x02, 0)
                .unwrap()
                .as_type(),
            TyKind::Ref(region, inner, mutability) => {
                let inner_di = self.debug_type_for_ty(context, *inner, ty_ctx);
                let name = if mutability.is_mut() {
                    format!("&mut {:?}", region)
                } else {
                    format!("&{:?}", region)
                };
                // A reference is a real DWARF pointer to its pointee type.
                self.builder
                    .create_pointer_type(&name, inner_di, 64, 64, AddressSpace::default())
                    .as_type()
            }
            TyKind::RawPtr(inner, mutability) => {
                let inner_di = self.debug_type_for_ty(context, *inner, ty_ctx);
                let name = if mutability.is_mut() {
                    format!("*mut {:?}", inner_di)
                } else {
                    format!("*const {:?}", inner_di)
                };
                // A raw pointer is a real DWARF pointer to its pointee type.
                self.builder
                    .create_pointer_type(&name, inner_di, 64, 64, AddressSpace::default())
                    .as_type()
            }
            TyKind::Slice(elem_ty) => {
                // Slice is represented as {ptr, len} where ptr is a real
                // DWARF pointer to the element type.
                let elem_di = self.debug_type_for_ty(context, *elem_ty, ty_ctx);
                let ptr_ty = self
                    .builder
                    .create_pointer_type("ptr", elem_di, 64, 64, AddressSpace::default())
                    .as_type();
                let len_ty = self
                    .builder
                    .create_basic_type("usize", 64, 0x04, 0)
                    .unwrap()
                    .as_type();
                self.builder
                    .create_struct_type(
                        self.compile_unit_scope,
                        &format!("[{:?}]", elem_di),
                        file,
                        0,
                        128,
                        64,
                        0,
                        None,
                        &[ptr_ty, len_ty],
                        0,
                        None,
                        "glyim",
                    )
                    .as_type()
            }
            TyKind::Array(elem_ty, count) => {
                let count_val = match &count.kind {
                    glyim_type::ConstKind::Uint(n) => *n as u64,
                    glyim_type::ConstKind::Int(n) if *n >= 0 => *n as u64,
                    _ => 0,
                };
                let elem_di = self.debug_type_for_ty(context, *elem_ty, ty_ctx);
                self.builder
                    .create_array_type(elem_di, count_val, 0, &[])
                    .as_type()
            }
            TyKind::FnPtr(_sig) => {
                // Simplification: treat as opaque pointer.
                self.builder
                    .create_basic_type("fn", 64, 0x02, 0)
                    .unwrap()
                    .as_type()
            }
            TyKind::Tuple(substs) => {
                let args = ty_ctx.substitution_args(*substs);
                let mut field_types = Vec::new();
                for arg in args {
                    if let GenericArg::Ty(t) = arg {
                        field_types.push(self.debug_type_for_ty(context, *t, ty_ctx));
                    }
                }
                let name = format!("({})", field_types.len());
                let layout = layout_computer.layout_of(ty).unwrap_or(Layout::unit());
                let size_bits = layout.size.0 * 8;
                let align_bits = layout.align.0 * 8;
                self.builder
                    .create_struct_type(
                        self.compile_unit_scope,
                        &name,
                        file,
                        0,
                        size_bits,
                        align_bits.try_into().unwrap(),
                        0,
                        None,
                        field_types.as_slice(),
                        0,
                        None,
                        "glyim",
                    )
                    .as_type()
            }
            TyKind::Adt(adt_id, _substs) => {
                let adt_def_opt = ty_ctx.adt_def(*adt_id);
                let name = format!("Adt{}", adt_id.to_raw());
                let layout = layout_computer.layout_of(ty).unwrap_or(Layout::unit());
                let size_bits = layout.size.0 * 8;
                let align_bits = layout.align.0 * 8;
                if let Some(adt_def) = adt_def_opt {
                    if adt_def.variants.len() == 1 {
                        let variant = &adt_def.variants[0];
                        let mut field_types = Vec::new();
                        for field in variant.fields.iter() {
                            let field_ty = self.debug_type_for_ty(context, field.ty, ty_ctx);
                            field_types.push(field_ty);
                        }
                        self.builder
                            .create_struct_type(
                                self.compile_unit_scope,
                                &name,
                                file,
                                0,
                                size_bits,
                                align_bits.try_into().unwrap(),
                                0,
                                None,
                                field_types.as_slice(),
                                0,
                                None,
                                "glyim",
                            )
                            .as_type()
                    } else {
                        // Enum: emit a real DWARF union of its variants, wrapped
                        // in an outer struct alongside a discriminant member
                        // (Phase 6.2, unstub-5). Each variant becomes a struct
                        // type with named, typed members so debuggers can show
                        // the active variant's fields instead of an opaque blob.
                        let mut variant_member_types: Vec<DIType<'ctx>> = Vec::new();
                        for (i, variant) in adt_def.variants.iter().enumerate() {
                            let variant_name =
                                format!("{}_V{}", name, ty_ctx.name_str(variant.name));
                            let mut variant_field_types: Vec<DIType<'ctx>> = Vec::new();
                            for field in variant.fields.iter() {
                                let field_di =
                                    self.debug_type_for_ty(context, field.ty, ty_ctx);
                                variant_field_types.push(field_di);
                            }
                            let variant_di = self
                                .builder
                                .create_struct_type(
                                    self.compile_unit_scope,
                                    &variant_name,
                                    file,
                                    0,
                                    size_bits,
                                    align_bits.try_into().unwrap(),
                                    0,
                                    None,
                                    variant_field_types.as_slice(),
                                    0,
                                    None,
                                    "glyim",
                                )
                                .as_type();
                            variant_member_types.push(variant_di);
                            let _ = i;
                        }
                        // Discriminant member: width must agree with the tag
                        // type layout/codegen actually uses
                        // (glyim-layout::discriminant_info / the U8/U16/U32/U64
                        // scheme in glyim-codegen-llvm/abi.rs), not a hardcoded
                        // 8 bits — otherwise a debugger misreads the discriminant
                        // of any enum with more than 256 variants.
                        let variant_count = adt_def.variants.len();
                        let discr_bits: u64 = if variant_count <= 256 {
                            8
                        } else if variant_count <= 65_536 {
                            16
                        } else if variant_count <= 4_294_967_296 {
                            32
                        } else {
                            64
                        };
                        let discr_di = self
                            .builder
                            .create_basic_type("discriminant", discr_bits, 0x04, 0)
                            .unwrap()
                            .as_type();
                        let union_di = self
                            .builder
                            .create_union_type(
                                self.compile_unit_scope,
                                &format!("{}_variants", name),
                                file,
                                0,
                                size_bits,
                                align_bits.try_into().unwrap(),
                                DIFlags::ZERO,
                                variant_member_types.as_slice(),
                                0,
                                "",
                            )
                            .as_type();
                        self.builder
                            .create_struct_type(
                                self.compile_unit_scope,
                                &name,
                                file,
                                0,
                                size_bits,
                                align_bits.try_into().unwrap(),
                                0,
                                None,
                                &[discr_di, union_di],
                                0,
                                None,
                                "glyim",
                            )
                            .as_type()
                    }
                } else {
                    self.builder
                        .create_basic_type("<unresolved>", 32, 0x05, 0)
                        .unwrap()
                        .as_type()
                }
            }
            TyKind::Closure(closure_id, _substs) => {
                // A closure's captured environment is registered as a synthetic
                // ADT (one field per captured variable) via
                // `TyCtxMut::register_closure`. If we have that mapping, render
                // it exactly like the `Adt` arm so debuggers can inspect each
                // captured field by name/type (Phase 6.2, unstub-5). Otherwise
                // fall back to an opaque struct.
                if let Some(adt_id) = ty_ctx.closure_adt(*closure_id) {
                    let adt_def_opt = ty_ctx.adt_def(adt_id);
                    let name = format!("closure{}", closure_id.to_raw());
                    let layout = layout_computer.layout_of(ty).unwrap_or(Layout::unit());
                    let size_bits = layout.size.0 * 8;
                    let align_bits = layout.align.0 * 8;
                    if let Some(adt_def) = adt_def_opt {
                        if adt_def.variants.len() == 1 {
                            let variant = &adt_def.variants[0];
                            let mut field_types = Vec::new();
                            for field in variant.fields.iter() {
                                let field_ty = self.debug_type_for_ty(context, field.ty, ty_ctx);
                                field_types.push(field_ty);
                            }
                            self.builder
                                .create_struct_type(
                                    self.compile_unit_scope,
                                    &name,
                                    file,
                                    0,
                                    size_bits,
                                    align_bits.try_into().unwrap(),
                                    0,
                                    None,
                                    field_types.as_slice(),
                                    0,
                                    None,
                                    "glyim",
                                )
                                .as_type()
                        } else {
                            // Multi-variant (shouldn't happen for closures);
                            // emit the opaque fallback.
                            self.builder
                                .create_struct_type(
                                    self.compile_unit_scope,
                                    &name,
                                    file,
                                    0,
                                    size_bits,
                                    align_bits.try_into().unwrap(),
                                    0,
                                    None,
                                    &[],
                                    0,
                                    None,
                                    "glyim",
                                )
                                .as_type()
                        }
                    } else {
                        self.builder
                            .create_struct_type(
                                self.compile_unit_scope,
                                &name,
                                file,
                                0,
                                size_bits,
                                align_bits.try_into().unwrap(),
                                0,
                                None,
                                &[],
                                0,
                                None,
                                "glyim",
                            )
                            .as_type()
                    }
                } else {
                    let name = "closure";
                    // Treat as opaque struct.
                    let layout = layout_computer.layout_of(ty).unwrap_or(Layout::unit());
                    let size_bits = layout.size.0 * 8;
                    let align_bits = layout.align.0 * 8;
                    self.builder
                        .create_struct_type(
                            self.compile_unit_scope,
                            name,
                            file,
                            0,
                            size_bits,
                            align_bits.try_into().unwrap(),
                            0,
                            None,
                            &[],
                            0,
                            None,
                            "glyim",
                        )
                        .as_type()
                }
            }
            TyKind::Dynamic(_, _) => {
                // Trait object: { data_ptr, vtable_ptr }
                let ptr_ty = self
                    .builder
                    .create_basic_type("ptr", 64, 0x02, 0)
                    .unwrap()
                    .as_type();
                self.builder
                    .create_struct_type(
                        self.compile_unit_scope,
                        "dyn",
                        file,
                        0,
                        128,
                        64,
                        0,
                        None,
                        &[ptr_ty, ptr_ty],
                        0,
                        None,
                        "glyim",
                    )
                    .as_type()
            }
            _ => self
                .builder
                .create_basic_type("<unresolved>", 32, 0x05, 0)
                .unwrap()
                .as_type(),
        };
        self.type_cache.insert(ty, di_type);
        di_type
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn declare_local(
        &mut self,
        context: &'ctx Context,
        alloca: inkwell::values::PointerValue<'ctx>,
        var_info: &VarDebugInfo,
        ty: glyim_type::Ty,
        span: glyim_span::Span,
        ty_ctx: &TyCtx,
        block: inkwell::basic_block::BasicBlock<'ctx>,
    ) {
        if !self.enabled || self.subprogram.is_none() {
            return;
        }

        let scope = self.subprogram.unwrap().as_debug_info_scope();
        let basic_ty = self.debug_type_for_ty(context, ty, ty_ctx);
        let name = ty_ctx.name_str(var_info.name);
        let (line, col) = self.span_to_line_col(&span).unwrap_or((1, 1));
        let file = self.get_file(span.file);

        let divar = self.builder.create_auto_variable(
            scope,
            name,
            file,
            line,
            basic_ty,
            true,
            DIFlags::ZERO,
            32,
        );

        let loc = self
            .builder
            .create_debug_location(context, line, col, scope, None);
        let expr = self.builder.create_expression(vec![]);

        // NOTE: we deliberately bypass `inkwell`'s `insert_declare_at_end`
        // wrapper here. Under the `llvm22-1` feature inkwell aliases
        // `LLVMDIBuilderInsertDeclareRecordAtEnd` to that name and then wraps
        // the result in `InstructionValue::new(value_ref as LLVMValueRef)`. In
        // LLVM 22 the call returns an `LLVMDbgRecordRef` (the "new debug info
        // format"), not a `Value`, so the cast makes `is_instruction()` return
        // null and the `debug_assert!` inside `InstructionValue::new` panics
        // non-deterministically (the flaky `test_debug_declare_local_emits_`
        // `intrinsic` failure). We never use the return value, so we call the
        // correct LLVM-22 API directly via `inkwell::llvm_sys` (the exact same
        // crate instance inkwell links, so the raw pointer types match) and
        // drop the result — emitting the `llvm.dbg.declare` record without the
        // broken `InstructionValue` wrapping.
        unsafe {
            inkwell::llvm_sys::debuginfo::LLVMDIBuilderInsertDeclareRecordAtEnd(
                self.builder.as_mut_ptr(),
                alloca.as_value_ref(),
                divar.as_mut_ptr(),
                expr.as_mut_ptr(),
                loc.as_mut_ptr(),
                block.as_mut_ptr(),
            );
        }
    }

    pub(crate) fn finalize(self) {
        if self.enabled {
            self.builder.finalize();
        }
    }

    fn get_file(&self, file_id: FileId) -> DIFile<'ctx> {
        self.files.get(&file_id).copied().unwrap_or_else(|| {
            self.files
                .values()
                .next()
                .copied()
                .unwrap_or_else(|| self.builder.create_file("unknown.g", "."))
        })
    }

    fn span_to_line_col(&self, span: &Span) -> Option<(u32, u32)> {
        if span.is_dummy() {
            return None;
        }
        let file_id = span.file;
        let (_path, _source) = self.source_map.get(&file_id)?;
        let offset: usize = span.lo.to_usize();

        // Use the precomputed line table.
        let starts = self.line_tables.get(&file_id)?;
        // binary search for the line.
        let line_idx = match starts.binary_search(&offset) {
            Ok(idx) => idx,
            Err(idx) => idx - 1,
        };
        if line_idx >= starts.len() {
            return None;
        }
        let line = line_idx as u32 + 1;
        let col = offset - starts[line_idx];
        Some((line, col as u32 + 1))
    }
}
