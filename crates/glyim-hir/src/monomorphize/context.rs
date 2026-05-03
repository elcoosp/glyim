use super::*;
use crate::item::HirItem;
use crate::node::HirFn;
use glyim_interner::Symbol;

impl<'a> MonoContext<'a> {
    pub(crate) fn new(
        hir: &'a crate::Hir,
        interner: &'a mut Interner,
        expr_types: &'a [HirType],
        call_type_args: &'a HashMap<ExprId, Vec<HirType>>,
    ) -> Self {
        Self {
            hir,
            interner,
            expr_types,
            call_type_args,
            fn_specs: HashMap::new(),
            struct_specs: HashMap::new(),
            type_overrides: HashMap::new(),
            fn_work_queue: Vec::new(),
            fn_queued: HashSet::new(),
            inferred_call_args: HashMap::new(),
            current_type_params: vec![],
        }
    }

    pub(crate) fn find_fn(&mut self, name: Symbol) -> Option<HirFn> {
        let name_str = self.interner.resolve(name).to_string();
        for item in &self.hir.items {
            if let HirItem::Fn(f) = item
                && f.name == name { return Some(f.clone()); }
        }
        for item in &self.hir.items {
            if let HirItem::Impl(imp) = item {
                for m in &imp.methods {
                    if m.name == name { return Some(m.clone()); }
                }
            }
        }
        if let Some(pos) = name_str.rfind('_') {
            let base_method_name = name_str[pos + 1..].to_string();
            let prefix = name_str[..pos].to_string();
            let prefix_sym = self.interner.intern(&prefix);
            if self.find_struct(prefix_sym).is_some() {
                for item in &self.hir.items {
                    if let HirItem::Impl(imp) = item
                        && imp.target_name == prefix_sym {
                            for m in &imp.methods {
                                let m_name = self.interner.resolve(m.name).to_string();
                                if m_name == base_method_name
                                    || m_name.ends_with(&format!("_{}", base_method_name))
                                { return Some(m.clone()); }
                            }
                        }
                }
            }
        }
        None
    }

    pub(crate) fn find_struct(&self, name: Symbol) -> Option<StructDef> {
        for item in &self.hir.items {
            if let HirItem::Struct(s) = item
                && s.name == name { return Some(s.clone()); }
        }
        None
    }

    pub(crate) fn mangle_name(&mut self, base: Symbol, type_args: &[HirType]) -> Symbol {
        mangle_type_name(self.interner, base, type_args)
    }
}
