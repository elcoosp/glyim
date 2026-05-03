use super::*;
use crate::item::HirItem;
use crate::types::HirType;
use glyim_interner::Symbol;
use std::collections::HashMap;

impl<'a> MonoContext<'a> {
    /// Looks up the type parameters for a function from the original HIR.
    fn get_fn_type_params(&mut self, name: Symbol) -> Vec<Symbol> {
        for item in &self.hir.items {
            match item {
                HirItem::Fn(f) if f.name == name => {
                    return f.type_params.clone();
                }
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        // Check if this is a method that would be mangled to `name`
                        if !imp.type_params.is_empty() {
                            let mangled = format!(
                                "_{}_{}",
                                self.interner.resolve(imp.target_name),
                                self.interner.resolve(m.name)
                            );
                            if self.interner.intern(&mangled) == name {
                                return m.type_params.clone();
                            }
                        }
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
        let struct_specs: Vec<_> = self
            .struct_specs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for ((_orig_name, args), s) in &struct_specs {
            let mut mono_s = s.clone();
            mono_s.name = self.mangle_name(s.name, args);
            items.push(HirItem::Struct(mono_s));
        }
        let fn_keys: Vec<(Symbol, Vec<HirType>)> = self.fn_specs.keys().cloned().collect();
        let fn_mangled_names: Vec<((Symbol, Vec<HirType>), Symbol)> = {
            let mut names = Vec::new();
            for (name, args) in &fn_keys {
                let mangled = self.mangle_name(*name, args);
                names.push(((*name, args.clone()), mangled));
                if !args.is_empty() {
                    names.push(((*name, Vec::new()), mangled));
                }
            }
            names
        };
        let fn_mangle_map: HashMap<(Symbol, Vec<HirType>), Symbol> =
            fn_mangled_names.into_iter().collect();
        let struct_keys: Vec<(Symbol, Vec<HirType>)> = self.struct_specs.keys().cloned().collect();
        let struct_mangled_names: Vec<(Symbol, Symbol)> = {
            let mut names = Vec::new();
            for (name, args) in &struct_keys {
                let mangled = self.mangle_name(*name, args);
                names.push((*name, mangled));
            }
            names
        };
        let struct_mangle_map: HashMap<Symbol, Symbol> = struct_mangled_names.into_iter().collect();
        let original_items: Vec<crate::item::HirItem> = self
            .hir
            .items
            .iter()
            .filter_map(|item| match item {
                HirItem::Fn(_) | HirItem::Struct(_) | HirItem::Enum(_) | HirItem::Extern(_) => {
                    Some(item.clone())
                }
                HirItem::Impl(imp) if !imp.methods.is_empty() && imp.type_params.is_empty() => {
                    Some(item.clone())
                }
                _ => None,
            })
            .collect();

        // Empty substitution for non-generic functions
        let empty_sub: HashMap<Symbol, HirType> = HashMap::new();

        for item in &original_items {
            match item {
                HirItem::Fn(f) => {
                    if f.type_params.is_empty() {
                        let rewritten =
                            self.rewrite_fn(f, &fn_mangle_map, &struct_mangle_map, &empty_sub);
                        items.push(HirItem::Fn(rewritten));
                    }
                }
                HirItem::Struct(s) => items.push(HirItem::Struct(s.clone())),
                HirItem::Enum(e) => items.push(HirItem::Enum(e.clone())),
                HirItem::Extern(e) => items.push(HirItem::Extern(e.clone())),
                HirItem::Impl(imp) => {
                    for m in &imp.methods {
                        if m.type_params.is_empty() {
                            let rewritten =
                                self.rewrite_fn(m, &fn_mangle_map, &struct_mangle_map, &empty_sub);
                            items.push(HirItem::Fn(rewritten));
                        }
                    }
                }
            }
        }
        let fn_specs_clone: Vec<_> = self
            .fn_specs
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for ((orig_name, args), f) in &fn_specs_clone {
            let mut mono_f = f.clone();
            mono_f.name = self.mangle_name(*orig_name, args);

            // Build the type substitution map for this monomorphized function
            let original_type_params = self.get_fn_type_params(*orig_name);
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
