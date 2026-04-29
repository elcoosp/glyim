use inkwell::debug_info::{
    DebugInfoBuilder, DICompileUnit, DIFile, DISubprogram, DILocation, DILocalVariable,
    DWARFEmissionKind, DWARFSourceLanguage, DIFlagsConstants, AsDIScope,
};
use inkwell::context::{AsContextRef, ContextRef};
use inkwell::module::Module;
use inkwell::values::PointerValue;
use std::cell::RefCell;
use std::collections::HashMap;
use glyim_interner::Symbol;

pub struct DebugInfoGen<'ctx> {
    context: ContextRef<'ctx>,
    dibuilder: DebugInfoBuilder<'ctx>,
    compile_unit: DICompileUnit<'ctx>,
    file: DIFile<'ctx>,
    fn_subprograms: RefCell<HashMap<Symbol, DISubprogram<'ctx>>>,
}

impl<'ctx> DebugInfoGen<'ctx> {
    pub fn new(
        module: &Module<'ctx>,
        file_name: &str,
    ) -> Result<Self, String> {
        let (dibuilder, compile_unit) = module.create_debug_info_builder(
            true,
            DWARFSourceLanguage::C,
            file_name,
            ".",
            "glyim v0.5.1",
            false,
            "",
            0,
            "",
            DWARFEmissionKind::Full,
            0,
            false,
            false,
            "",
            "",
        );

        let file = compile_unit.get_file();
        let context = module.get_context();

        Ok(Self {
            context,
            dibuilder,
            compile_unit,
            file,
            fn_subprograms: RefCell::new(HashMap::new()),
        })
    }

    pub fn create_subprogram(
        &self,
        name: &str,
        line: u32,
        is_artificial: bool,
    ) -> Result<DISubprogram<'ctx>, String> {
        let i64_type = self
            .dibuilder
            .create_basic_type("i64", 64, 64, 0x05)
            .map_err(|e| format!("create_basic_type: {e}"))?;

        let subroutine_type = self
            .dibuilder
            .create_subroutine_type(
                self.file,
                Some(i64_type.as_type()),
                &[],
                if is_artificial { DIFlagsConstants::ARTIFICIAL } else { DIFlagsConstants::ZERO },
            );

        let subprogram = self
            .dibuilder
            .create_function(
                self.compile_unit.as_debug_info_scope(),
                name,
                None,
                self.file,
                line,
                subroutine_type,
                false,
                true,
                line,
                if is_artificial { DIFlagsConstants::ARTIFICIAL } else { DIFlagsConstants::ZERO },
                false,
            );

        Ok(subprogram)
    }

    pub fn register_subprogram(&self, name: Symbol, subprogram: DISubprogram<'ctx>) {
        self.fn_subprograms.borrow_mut().insert(name, subprogram);
    }

    pub fn get_subprogram(&self, name: Symbol) -> Option<DISubprogram<'ctx>> {
        self.fn_subprograms.borrow().get(&name).copied()
    }

    pub fn create_location(
        &self,
        subprogram: DISubprogram<'ctx>,
        line: u32,
        column: u32,
    ) -> Result<DILocation<'ctx>, String> {
        let loc = self
            .dibuilder
            .create_debug_location(
                self.context,
                line,
                column,
                subprogram.as_debug_info_scope(),
                None,
            );
        Ok(loc)
    }

    pub fn create_local_variable(
        &self,
        name: &str,
        subprogram: DISubprogram<'ctx>,
        line: u32,
    ) -> Result<DILocalVariable<'ctx>, String> {
        let i64_type = self
            .dibuilder
            .create_basic_type("i64", 64, 64, 0x05)
            .map_err(|e| format!("create_basic_type: {e}"))?;

        let var = self
            .dibuilder
            .create_auto_variable(
                subprogram.as_debug_info_scope(),
                name,
                self.file,
                line,
                i64_type.as_type(),
                true,
                DIFlagsConstants::ZERO,
                0,
            );
        Ok(var)
    }

    /// Insert llvm.dbg.declare by manually building the intrinsic call.
    /// This avoids the DbgRecord panic in inkwell 0.9 + LLVM 22.
    pub fn insert_declare(
        &self,
        builder: &inkwell::builder::Builder<'ctx>,
        module: &Module<'ctx>,
        variable: DILocalVariable<'ctx>,
        ptr_value: PointerValue<'ctx>,
        location: DILocation<'ctx>,
    ) -> Result<(), String> {
        let declare_fn = module
            .get_function("llvm.dbg.declare")
            .ok_or_else(|| "llvm.dbg.declare not declared".to_string())?;

        let ctx_ref = module.get_context();
        let ctx_ptr = ctx_ref.as_ctx_ref();

        // Convert debug metadata to MetadataValue using unsafe FFI
        let var_md = unsafe {
            inkwell::values::MetadataValue::new(
                llvm_sys::core::LLVMMetadataAsValue(
                    ctx_ptr,
                    variable.as_mut_ptr(),
                )
            )
        };

        let loc_md = unsafe {
            inkwell::values::MetadataValue::new(
                llvm_sys::core::LLVMMetadataAsValue(
                    ctx_ptr,
                    location.as_mut_ptr(),
                )
            )
        };

        let empty_md = module.get_context().metadata_node(&[]);

        builder
            .build_call(
                declare_fn,
                &[
                    ptr_value.into(),
                    var_md.into(),
                    empty_md.into(),
                    loc_md.into(),
                ],
                "",
            )
            .map_err(|e| format!("dbg.declare call: {e}"))?;

        Ok(())
    }

    pub fn finalize(&self) {
        self.dibuilder.finalize();
    }

    pub fn byte_offset_to_line(source: &str, offset: usize) -> u32 {
        let mut line = 1u32;
        for (i, ch) in source.char_indices() {
            if i >= offset {
                break;
            }
            if ch == '\n' {
                line += 1;
            }
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;

    #[test]
    fn byte_offset_to_line_start() {
        assert_eq!(DebugInfoGen::byte_offset_to_line("hello\nworld", 0), 1);
    }

    #[test]
    fn byte_offset_to_line_second_line() {
        assert_eq!(DebugInfoGen::byte_offset_to_line("hello\nworld", 6), 2);
    }

    #[test]
    fn byte_offset_to_line_past_end() {
        assert_eq!(DebugInfoGen::byte_offset_to_line("hello\nworld", 100), 2);
    }

    #[test]
    fn byte_offset_to_line_empty() {
        assert_eq!(DebugInfoGen::byte_offset_to_line("", 0), 1);
    }

    #[test]
    fn byte_offset_to_line_multiple_newlines() {
        assert_eq!(DebugInfoGen::byte_offset_to_line("a\nb\nc\nd", 4), 3);
    }

    #[test]
    fn debug_info_gen_new_does_not_panic() {
        let ctx = Context::create();
        let module = ctx.create_module("test");
        match DebugInfoGen::new(&module, "test.g") {
            Ok(_) => {},
            Err(e) => {
                eprintln!("DebugInfoGen::new() failed: {e}");
            }
        }
        drop(module);
        drop(ctx);
    }
}
