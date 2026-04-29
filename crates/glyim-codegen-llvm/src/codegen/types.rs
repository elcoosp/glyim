use crate::Codegen;
use glyim_hir::HirType;
use inkwell::types::BasicTypeEnum;
use inkwell::AddressSpace;

impl<'ctx> Codegen<'ctx> {
    /// Convert HirType to LLVM BasicTypeEnum for codegen.
    pub(crate) fn hir_type_to_llvm(&self, ty: &HirType) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            HirType::Int => Some(self.i64_type.into()),
            HirType::Bool => Some(self.i64_type.into()),
            HirType::Float => Some(self.f64_type.into()),
            HirType::Str => {
                let fat_ptr = self.context.struct_type(&[
                    self.context.ptr_type(AddressSpace::from(0u16)).into(),
                    self.i64_type.into(),
                ], false);
                Some(fat_ptr.into())
            }
            HirType::Unit => Some(self.i64_type.into()),
            HirType::Named(sym) => {
                self.struct_types.borrow().get(sym).map(|st| (*st).into())
                    .or_else(|| self.enum_struct_types.borrow().get(sym).map(|st| (*st).into()))
            }
            HirType::Generic(sym, _args) => {
                self.struct_types.borrow().get(sym).map(|st| (*st).into())
                    .or_else(|| self.enum_struct_types.borrow().get(sym).map(|st| (*st).into()))
            }
            HirType::Tuple(elems) => {
                let field_types: Vec<_> = elems.iter()
                    .filter_map(|e| self.hir_type_to_llvm(e))
                    .collect();
                if field_types.is_empty() { Some(self.i64_type.into()) }
                else { Some(self.context.struct_type(&field_types, false).into()) }
            }
            HirType::RawPtr { .. } => Some(self.context.ptr_type(AddressSpace::from(0u16)).into()),
            _ => Some(self.i64_type.into()),
        }
    }
}

pub(crate) fn register_builtin_enums<'ctx>(cg: &mut Codegen) {
    let tag_type = cg.i32_type;
    let payload_type = cg.context.i8_type().array_type(8);
    let enum_struct_type = cg.context.struct_type(
        &[BasicTypeEnum::IntType(tag_type), BasicTypeEnum::ArrayType(payload_type)],
        false,
    );
    let option_name = cg.interner.intern("Option");
    cg.enum_types.borrow_mut().insert(option_name, (tag_type, payload_type));
    cg.enum_struct_types.borrow_mut().insert(option_name, enum_struct_type);
    let mut tag_map = cg.enum_variant_tags.borrow_mut();
    tag_map.insert((option_name, cg.interner.intern("None")), 0);
    tag_map.insert((option_name, cg.interner.intern("Some")), 1);
    let result_name = cg.interner.intern("Result");
    cg.enum_types.borrow_mut().insert(result_name, (tag_type, payload_type));
    cg.enum_struct_types.borrow_mut().insert(result_name, enum_struct_type);
    tag_map.insert((result_name, cg.interner.intern("Ok")), 0);
    tag_map.insert((result_name, cg.interner.intern("Err")), 1);
}

pub(crate) fn codegen_struct_def<'ctx>(cg: &Codegen, def: &glyim_hir::item::StructDef) {
    let field_types: Vec<BasicTypeEnum> = def.fields.iter()
        .map(|_| BasicTypeEnum::IntType(cg.i64_type)).collect();
    let struct_type = cg.context.struct_type(&field_types, false);
    cg.struct_types.borrow_mut().insert(def.name, struct_type);
    let mut index_map = cg.struct_field_indices.borrow_mut();
    for (i, field) in def.fields.iter().enumerate() {
        index_map.insert((def.name, field.name), i);
    }
}

pub(crate) fn codegen_enum_def<'ctx>(cg: &Codegen, def: &glyim_hir::item::EnumDef) {
    let max_fields = def.variants.iter().map(|v| v.fields.len()).max().unwrap_or(0);
    let payload_bytes = (max_fields as u32) * 8;
    let tag_type = cg.i32_type;
    let payload_type = cg.context.i8_type().array_type(payload_bytes);
    let enum_struct_type = cg.context.struct_type(
        &[BasicTypeEnum::IntType(tag_type), BasicTypeEnum::ArrayType(payload_type)],
        false,
    );
    cg.enum_types.borrow_mut().insert(def.name, (tag_type, payload_type));
    cg.enum_struct_types.borrow_mut().insert(def.name, enum_struct_type);
    let mut tag_map = cg.enum_variant_tags.borrow_mut();
    for (i, variant) in def.variants.iter().enumerate() {
        tag_map.insert((def.name, variant.name), i as u32);
    }
}
