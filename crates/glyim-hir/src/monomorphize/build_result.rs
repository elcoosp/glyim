// crates/glyim-hir/src/monomorphize/build_result.rs
use super::*;
use crate::item::HirItem;
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    fn get_fn_type_params(&mut self, name: Symbol) -> Vec<Symbol> {
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) if f.name == name => return f.type_params.clone(),
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if m.name == name {
                            return m.type_params.clone();
                        }
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn build_result(mut self) -> MonoResult {
        let mut items = Vec::new();

        // Emit specialized structs first (with mangled names)
        let struct_specs: Vec<_> = self
            .struct_specs
            .iter()
            .filter(|((_, args), _)| !args.iter().any(|a| self.has_unresolved_type_param(a)))
            .map(|((orig_name, args), s)| (*orig_name, args.clone(), s.clone()))
            .collect();

        for (_, args, s) in struct_specs {
            let mangled = self.mangle_name(s.name, &args);
            let mut mono_s = s;
            mono_s.name = mangled;
            items.push(HirItem::Struct(mono_s));
        }

        // Emit specialized enums (with mangled names)
        let enum_specs: Vec<_> = self
            .enum_specs
            .iter()
            .filter(|((_, args), _)| !args.iter().any(|a| self.has_unresolved_type_param(a)))
            .map(|((orig_name, args), e)| (*orig_name, args.clone(), e.clone()))
            .collect();

        for (_, args, e) in enum_specs {
            let mangled = self.mangle_name(e.name, &args);
            let mut mono_e = e;
            mono_e.name = mangled;
            items.push(HirItem::Enum(mono_e));
        }

        // Build enum_spec_map from specialized enums
        let enum_specs_for_map: Vec<_> = self
            .enum_specs
            .iter()
            .filter(|((_, args), _)| !args.iter().any(|a| self.has_unresolved_type_param(a)))
            .map(|((orig_name, args), _)| (*orig_name, args.clone()))
            .collect();

        let mut enum_spec_map: HashMap<(Symbol, Vec<HirType>), Symbol> = HashMap::new();
        for (orig_name, args) in enum_specs_for_map {
            let mangled = self.mangle_name(orig_name, &args);
            enum_spec_map.insert((orig_name, args.clone()), mangled);
        }

        // Build fn_mangle_map from specialized functions
        let fn_specs: Vec<_> = self
            .fn_specs
            .iter()
            .filter(|((_, args), _)| !args.iter().any(|a| self.has_unresolved_type_param(a)))
            .map(|((orig_name, args), f)| (*orig_name, args.clone(), f.clone()))
            .collect();

        let mut fn_mangle_map: HashMap<(Symbol, Vec<HirType>), Symbol> = HashMap::new();
        for (orig_name, args, _) in &fn_specs {
            let mangled = self.mangle_name(*orig_name, args);
            fn_mangle_map.insert((*orig_name, args.clone()), mangled);
        }

        // Build struct_mangle_map from specialized structs
        let struct_specs_for_map: Vec<_> = self
            .struct_specs
            .iter()
            .filter(|((_, args), _)| !args.iter().any(|a| self.has_unresolved_type_param(a)))
            .map(|((orig_name, args), _)| (*orig_name, args.clone()))
            .collect();

        let mut struct_mangle_map: HashMap<Symbol, Symbol> = HashMap::new();
        for (orig_name, args) in struct_specs_for_map {
            let mangled = self.mangle_name(orig_name, &args);
            struct_mangle_map.insert(orig_name, mangled);
        }

        // Emit original non-generic items
        let empty_sub: HashMap<Symbol, HirType> = HashMap::new();
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) if f.type_params.is_empty() => {
                    let rewritten = self.rewrite_fn(
                        f,
                        &fn_mangle_map,
                        &struct_mangle_map,
                        &enum_spec_map,
                        &empty_sub,
                    );
                    items.push(HirItem::Fn(rewritten));
                }
                HirItem::Struct(s) if s.type_params.is_empty() => {
                    items.push(HirItem::Struct(s.clone()));
                }
                HirItem::Enum(e) => items.push(HirItem::Enum(e.clone())),
                HirItem::Extern(e) => items.push(HirItem::Extern(e.clone())),
                HirItem::Impl(imp) if imp.type_params.is_empty() => {
                    for m in &imp.methods {
                        if m.type_params.is_empty() {
                            let rewritten = self.rewrite_fn(
                                m,
                                &fn_mangle_map,
                                &struct_mangle_map,
                                &enum_spec_map,
                                &empty_sub,
                            );
                            items.push(HirItem::Fn(rewritten));
                        }
                    }
                }
                _ => {}
            }
        }

        // Emit specialized functions
        for (orig_name, args, f) in fn_specs {
            let mangled = self.mangle_name(orig_name, &args);
            let mut mono_f = f;
            mono_f.name = mangled;

            let original_type_params = self.get_fn_type_params(orig_name);
            let type_sub: HashMap<Symbol, HirType> = original_type_params
                .iter()
                .zip(args.iter())
                .map(|(tp, ct)| (*tp, ct.clone()))
                .collect();

            let mono_f = self.rewrite_fn(
                &mono_f,
                &fn_mangle_map,
                &struct_mangle_map,
                &enum_spec_map,
                &type_sub,
            );
            items.push(HirItem::Fn(mono_f));
        }

        // Concretize all remaining Generic types to Named(mangled)

        // Concretize all remaining Generic types to Named(mangled) in the output items
        for item in &mut items {
            match item {
                crate::item::HirItem::Fn(f) => {
                    for (_, ty) in &mut f.params {
                        *ty = self.concretize_type(ty);
                    }
                    if let Some(ret) = &mut f.ret {
                        *ret = self.concretize_type(ret);
                    }
                }
                crate::item::HirItem::Struct(s) => {
                    for field in &mut s.fields {
                        field.ty = self.concretize_type(&field.ty);
                    }
                }
                crate::item::HirItem::Enum(e) => {
                    for variant in &mut e.variants {
                        for field in &mut variant.fields {
                            field.ty = self.concretize_type(&field.ty);
                        }
                    }
                }
                crate::item::HirItem::Impl(imp) => {
                    for m in &mut imp.methods {
                        for (_, ty) in &mut m.params {
                            *ty = self.concretize_type(ty);
                        }
                        if let Some(ret) = &mut m.ret {
                            *ret = self.concretize_type(ret);
                        }
                    }
                }
                _ => {}
            }
        }

        // Assert no unresolved type parameters before returning
        for item in &items {
            match item {
                crate::item::HirItem::Fn(f) => {
                    crate::passes::no_type_params::assert_no_type_params(&f.body, self.interner);
                }
                crate::item::HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        crate::passes::no_type_params::assert_no_type_params(
                            &m.body,
                            self.interner,
                        );
                    }
                }
                crate::item::HirItem::Struct(_) => {}
                crate::item::HirItem::Enum(_) => {}
                crate::item::HirItem::Extern(_) => {}
            }
        }
        MonoResult {
            hir: crate::Hir { items },
            type_overrides: self.type_overrides,
        }
    }
}
