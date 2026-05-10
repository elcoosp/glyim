//! Item specialization: produce concrete definitions from generic ones.

use crate::item::{EnumDef, StructDef};
use crate::monomorphize::concretize;
use crate::monomorphize::index::MonoIndex;
use crate::monomorphize::mangle_table::MangleTable;
use crate::types::HirType;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

pub fn specialize_struct(
    generic: &StructDef,
    type_args: &[HirType],
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> StructDef {
    let sub = concretize::build_subst(&generic.type_params, type_args);
    let mut result = generic.clone();
    result.type_params.clear();
    result.name = mangle_table.mangle(generic.name, type_args, interner);
    for field in &mut result.fields {
        field.ty = concretize::substitute_and_concretize(&field.ty, &sub, index, mangle_table, interner);
    }
    result
}

pub fn specialize_enum(
    generic: &EnumDef,
    type_args: &[HirType],
    index: &MonoIndex,
    mangle_table: &mut MangleTable,
    interner: &mut Interner,
) -> EnumDef {
    let sub = concretize::build_subst(&generic.type_params, type_args);
    let mut result = generic.clone();
    result.type_params.clear();
    result.name = mangle_table.mangle(generic.name, type_args, interner);
    for variant in &mut result.variants {
        for field in &mut variant.fields {
            field.ty = concretize::substitute_and_concretize(&field.ty, &sub, index, mangle_table, interner);
        }
    }
    result
}

pub fn build_fn_subst(
    generic_fn: &crate::node::HirFn,
    type_args: &[HirType],
    index: &MonoIndex,
    _interner: &Interner,
) -> HashMap<Symbol, HirType> {
    let mut sub = concretize::build_subst(&generic_fn.type_params, type_args);
    if !generic_fn.params.is_empty() {
        let first_param_ty = &generic_fn.params[0].1;
        match first_param_ty {
            HirType::Generic(struct_sym, _) => {
                if let Some(info) = index.find_struct(*struct_sym) {
                    for (i, tp) in info.type_params.iter().enumerate() {
                        if let Some(ct) = type_args.get(i) {
                            sub.entry(*tp).or_insert(ct.clone());
                        }
                    }
                }
            }
            HirType::RawPtr(inner) => {
                if let HirType::Generic(struct_sym, _) = inner.as_ref() {
                    if let Some(info) = index.find_struct(*struct_sym) {
                        for (i, tp) in info.type_params.iter().enumerate() {
                            if let Some(ct) = type_args.get(i) {
                                sub.entry(*tp).or_insert(ct.clone());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    sub
}
