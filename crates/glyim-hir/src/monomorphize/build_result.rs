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
                    let rewritten =
                        self.rewrite_fn(f, &fn_mangle_map, &struct_mangle_map, &empty_sub);
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
                            let rewritten =
                                self.rewrite_fn(m, &fn_mangle_map, &struct_mangle_map, &empty_sub);
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

            let mono_f = self.rewrite_fn(&mono_f, &fn_mangle_map, &struct_mangle_map, &type_sub);
            items.push(HirItem::Fn(mono_f));
        }

        MonoResult {
            hir: crate::Hir { items },
            type_overrides: self.type_overrides,
        }
    }
}
