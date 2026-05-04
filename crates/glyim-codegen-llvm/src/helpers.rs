//! Safe wrappers for common LLVM codegen operations.
//! Reduces risk of segfaults from manual GEP, uninitialized memory, etc.

use inkwell::types::StructType;
use inkwell::values::{IntValue, PointerValue};
use crate::Codegen;

impl<'ctx> Codegen<'ctx> {
    /// Returns a pointer to the nth field of a struct (0-based).
    pub fn struct_field_ptr(
        &self,
        struct_ptr: PointerValue<'ctx>,
        struct_type: StructType<'ctx>,
        field_idx: u32,
    ) -> Result<PointerValue<'ctx>, String> {
        let field_count = struct_type.count_fields();
        assert!(
            field_idx < field_count,
            "field index {} out of bounds for struct with {} fields",
            field_idx, field_count
        );
        let indices = &[
            self.i32_type.const_int(0, false),
            self.i32_type.const_int(field_idx as u64, false),
        ];
        unsafe {
            self.builder
                .build_gep(struct_type, struct_ptr, indices, "field_ptr")
                .map_err(|e| e.to_string())
        }
    }

    /// Returns a pointer to the nth field of a tuple (represented as an anonymous struct).
    pub fn tuple_field_ptr(
        &self,
        tuple_ptr: PointerValue<'ctx>,
        field_count: u32,
        field_idx: u32,
    ) -> Result<PointerValue<'ctx>, String> {
        let field_types = vec![self.i64_type.into(); field_count as usize];
        let struct_type = self.context.struct_type(&field_types, false);
        self.struct_field_ptr(tuple_ptr, struct_type, field_idx)
    }

    /// Build an alloca for i64 and immediately store zero into it.
    pub fn build_zeroed_alloca(&self, name: &str) -> Result<PointerValue<'ctx>, String> {
        let ptr = self
            .builder
            .build_alloca(self.i64_type, name)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(ptr, self.i64_type.const_int(0, false))
            .map_err(|e| e.to_string())?;
        Ok(ptr)
    }

    /// Allocate a heap block for a struct, zero-fill it, and return an i64 handle.
    pub fn zero_init_struct_on_heap(
        &self,
        struct_type: StructType<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        let size_val = struct_type
            .size_of()
            .unwrap_or_else(|| self.i64_type.const_int(0, false));
        let alloc_fn = self
            .module
            .get_function("__glyim_alloc")
            .or_else(|| self.module.get_function("malloc"))
            .ok_or("malloc/__glyim_alloc not found")?;
        let call_result = self
            .builder
            .build_call(alloc_fn, &[size_val.into()], "zero_init_alloc")
            .map_err(|e| e.to_string())?
            .try_as_basic_value();
        let ptr = match call_result {
            inkwell::values::ValueKind::Basic(basic_val) => basic_val.into_pointer_value(),
            _ => return Err("allocation returned non-pointer".into()),
        };
        let zero = self.i64_type.const_int(0, false);
        let field_count = struct_type.count_fields();
        for i in 0..field_count {
            let field_ptr = self.struct_field_ptr(ptr, struct_type, i)?;
            self.builder
                .build_store(field_ptr, zero)
                .map_err(|e| e.to_string())?;
        }
        self.builder
            .build_ptr_to_int(ptr, self.i64_type, "zero_struct_handle")
            .map_err(|e| e.to_string())
    }
}
