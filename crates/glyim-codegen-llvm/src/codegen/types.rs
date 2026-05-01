use crate::Codegen;
use glyim_hir::HirType;
use inkwell::types::BasicTypeEnum;
use inkwell::AddressSpace;

impl<'ctx> Codegen<'ctx> {
    /// Convert HirType to LLVM BasicTypeEnum for codegen.
    #[allow(dead_code)]
    pub(crate) fn hir_type_to_llvm(&self, ty: &HirType) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            HirType::Int => Some(self.i64_type.into()),
            HirType::Bool => Some(self.i64_type.into()),
            HirType::Float => Some(self.f64_type.into()),
            HirType::Str => {
                let fat_ptr = self.context.struct_type(
                    &[
                        self.context.ptr_type(AddressSpace::from(0u16)).into(),
                        self.i64_type.into(),
                    ],
                    false,
                );
                Some(fat_ptr.into())
            }
            HirType::Unit => Some(self.i64_type.into()),
            HirType::Named(sym) => self
                .struct_types
                .borrow()
                .get(sym)
                .map(|st| (*st).into())
                .or_else(|| {
                    self.enum_struct_types
                        .borrow()
                        .get(sym)
                        .map(|st| (*st).into())
                }),
            HirType::Generic(sym, _args) => {
                // Try the base struct type first; if it’s a concrete monomorphised version,
                // it will have been registered under the mangled name.
                let mangled_sym = *sym;
                self.struct_types
                    .borrow()
                    .get(&mangled_sym)
                    .map(|st| (*st).into())
                    .or_else(|| {
                        self.enum_struct_types
                            .borrow()
                            .get(&mangled_sym)
                            .map(|st| (*st).into())
                    })
                    .or_else(|| {
                        // Fallback: try the original (non-mangled) name in case it's a generic
                        // that was not monomorphised yet, but still has a struct definition.
                        self.struct_types
                            .borrow()
                            .get(sym)
                            .map(|st| (*st).into())
                            .or_else(|| {
                                self.enum_struct_types
                                    .borrow()
                                    .get(sym)
                                    .map(|st| (*st).into())
                            })
                    })
            }
            HirType::Tuple(elems) => {
                let field_types: Vec<_> = elems
                    .iter()
                    .filter_map(|e| self.hir_type_to_llvm(e))
                    .collect();
                if field_types.is_empty() {
                    Some(self.i64_type.into())
                } else {
                    Some(self.context.struct_type(&field_types, false).into())
                }
            }
            HirType::RawPtr(_) => {
                // All raw pointers are represented as i8* at the LLVM level.
                Some(self.context.ptr_type(AddressSpace::from(0u16)).into())
            }
            HirType::Never | HirType::Opaque(_) => {
                // Uninhabited types have zero size; we return i8* to allow sizeof.
                Some(self.context.i8_type().into())
            }
            _ => Some(self.i64_type.into()),
        }
    }
}

pub(crate) fn codegen_struct_def(cg: &Codegen, def: &glyim_hir::item::StructDef) {
    let field_types: Vec<BasicTypeEnum> = def
        .fields
        .iter()
        .map(|_| BasicTypeEnum::IntType(cg.i64_type))
        .collect();
    let struct_type = cg.context.struct_type(&field_types, false);
    cg.struct_types.borrow_mut().insert(def.name, struct_type);
    let mut index_map = cg.struct_field_indices.borrow_mut();
    for (i, field) in def.fields.iter().enumerate() {
        index_map.insert((def.name, field.name), i);
    }
}

pub(crate) fn codegen_enum_def(cg: &Codegen, def: &glyim_hir::item::EnumDef) {
    let max_fields = def
        .variants
        .iter()
        .map(|v| v.fields.len())
        .max()
        .unwrap_or(0);
    let payload_bytes = (max_fields as u32) * 8;
    let tag_type = cg.i32_type;
    let payload_type = cg.context.i8_type().array_type(payload_bytes);
    let enum_struct_type = cg.context.struct_type(
        &[
            BasicTypeEnum::IntType(tag_type),
            BasicTypeEnum::ArrayType(payload_type),
        ],
        false,
    );
    cg.enum_types
        .borrow_mut()
        .insert(def.name, (tag_type, payload_type));
    cg.enum_struct_types
        .borrow_mut()
        .insert(def.name, enum_struct_type);
    let mut tag_map = cg.enum_variant_tags.borrow_mut();
    for (i, variant) in def.variants.iter().enumerate() {
        tag_map.insert((def.name, variant.name), i as u32);
    }
}
