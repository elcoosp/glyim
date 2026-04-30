use crate::typeck::types::{EnumInfo, StructInfo};
use crate::TypeChecker;
use glyim_hir::item::{EnumDef, ExternBlock, FnSig, StructDef};
use std::collections::HashMap;

impl TypeChecker {
    #[tracing::instrument(skip_all)]
#[tracing::instrument(skip_all)]
pub(crate) fn register_items(&mut self, hir: &glyim_hir::node::Hir) {
        for item in &hir.items {
            match item {
                glyim_hir::item::HirItem::Struct(s) => self.register_struct(s),
                glyim_hir::item::HirItem::Enum(e) => self.register_enum(e),
                glyim_hir::item::HirItem::Extern(ext) => self.register_extern(ext),
                glyim_hir::item::HirItem::Impl(imp) => self.register_impl(imp),
                glyim_hir::item::HirItem::Fn(f) => self.fns.push(f.clone()),
            }
        }
    }

    fn register_struct(&mut self, s: &StructDef) {
        let mut field_map = HashMap::new();
        for (i, field) in s.fields.iter().enumerate() {
            field_map.insert(field.name, i);
        }
        self.structs.insert(
            s.name,
            StructInfo {
                fields: s.fields.clone(),
                field_map,
                type_params: s.type_params.clone(),
            },
        );
    }

    fn register_enum(&mut self, e: &EnumDef) {
        let mut variant_map = HashMap::new();
        for (i, v) in e.variants.iter().enumerate() {
            variant_map.insert(v.name, i);
        }
        self.enums.insert(
            e.name,
            EnumInfo {
                variants: e.variants.clone(),
                variant_map,
                type_params: e.type_params.clone(),
            },
        );
    }

    fn register_impl(&mut self, imp: &glyim_hir::item::HirImplDef) {
        let methods: Vec<(glyim_interner::Symbol, glyim_hir::node::HirFn)> =
            imp.methods.iter().map(|m| (m.name, m.clone())).collect();
        self.impl_methods.insert(imp.target_name, methods);
    }

    fn register_extern(&mut self, ext: &ExternBlock) {
        for f in &ext.functions {
            self.extern_fns.insert(
                f.name,
                FnSig {
                    params: f.params.clone(),
                    ret: f.ret.clone(),
                },
            );
        }
    }
}
