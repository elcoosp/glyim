//! BFS monomorphization driver.

use crate::item::HirItem;
use crate::monomorphize::concretize;
use crate::monomorphize::discover;
use crate::monomorphize::index::MonoIndex;
use crate::monomorphize::mangle_table::MangleTable;
use crate::monomorphize::specialize;
use crate::monomorphize::subst::SubstContext;
use crate::monomorphize::work::{ItemKind, WorkItem, WorkQueue};
use crate::types::HirType;
use crate::Hir;
use glyim_interner::{Interner, Symbol};
use std::collections::HashMap;

#[derive(Debug)]
pub struct MonoResult {
    pub hir: crate::Hir,
    pub expr_types: Vec<HirType>,
}

pub struct MonoContext<'a> {
    interner: &'a mut Interner,
    input_expr_types: &'a [HirType],
    call_type_args: &'a HashMap<crate::types::ExprId, Vec<HirType>>,
    index: MonoIndex,
    mangle_table: MangleTable,
    work_queue: WorkQueue,
    output_items: Vec<HirItem>,
    output_expr_types: Vec<HirType>,
    global_next_expr_id: u32,
}

impl<'a> MonoContext<'a> {
    pub fn new(
        interner: &'a mut Interner,
        input_expr_types: &'a [HirType],
        call_type_args: &'a HashMap<crate::types::ExprId, Vec<HirType>>,
        hir: &Hir,
    ) -> Self {
        let index = MonoIndex::build(hir);
        Self {
            interner,
            input_expr_types,
            call_type_args,
            index,
            mangle_table: MangleTable::new(),
            work_queue: WorkQueue::new(),
            output_items: Vec::new(),
            output_expr_types: Vec::new(),
            global_next_expr_id: 0,
        }
    }

    fn process_fn_specialize(&mut self, name: Symbol, type_args: Vec<HirType>) {
        let generic_fn = match self.index.find_fn(name).cloned() {
            Some(f) => f,
            None => return,
        };

        // Skip if type args don't cover all type params
        if type_args.len() < generic_fn.type_params.len() {
            tracing::warn!(
                "Skipping specialization of {:?}: need {} type args, got {}",
                self.interner.resolve(name),
                generic_fn.type_params.len(),
                type_args.len()
            );
            return;
        }

        // Check for unresolved type params in the type args
        if type_args.iter().any(|t| concretize::has_unresolved_type_param(t, self.interner)) {
            tracing::warn!(
                "Skipping specialization of {:?}: unresolved type params in args",
                self.interner.resolve(name)
            );
            return;
        }

        let sub = specialize::build_fn_subst(&generic_fn, &type_args, &self.index, self.interner);
        let mangled_name = self.mangle_table.mangle_fn(name, &type_args, self.interner);

        let mut concrete_params: Vec<(Symbol, HirType)> = Vec::new();
        for (sym, ty) in &generic_fn.params {
            let ct = concretize::substitute_and_concretize(
                ty, &sub, &self.index, &mut self.mangle_table, self.interner,
            );
            concrete_params.push((*sym, ct));
        }
        let concrete_ret = generic_fn.ret.as_ref().map(|ty| {
            concretize::substitute_and_concretize(ty, &sub, &self.index, &mut self.mangle_table, self.interner)
        });

        // Discover struct/enum specializations from the SUBSTITUTED (pre-concretization)
        // parameter and return types. After concretization, Generic types become Named
        // and won't trigger discovery, so we must discover before concretization.
        for (_, ty) in &generic_fn.params {
            let substituted = crate::types::substitute_type(ty, &sub);
            let disc = discover::discover_type_specializations(&substituted, &self.index, self.interner);
            self.work_queue.extend(disc);
        }
        if let Some(ret) = &generic_fn.ret {
            let substituted = crate::types::substitute_type(ret, &sub);
            let disc = discover::discover_type_specializations(&substituted, &self.index, self.interner);
            self.work_queue.extend(disc);
        }

        let start_id = self.global_next_expr_id;
        let (new_body, local_expr_types, new_next_id, discoveries) = {
            let mut subst_ctx = SubstContext::new(
                start_id,
                self.interner,
                self.input_expr_types,
                self.call_type_args,
                &self.index,
                &mut self.mangle_table,
            );
            let body = subst_ctx.substitute_expr(&generic_fn.body, &sub);
            let next_id = subst_ctx.next_expr_id();
            let disc = std::mem::take(&mut subst_ctx.discovered);
            let types = std::mem::take(&mut subst_ctx.output_expr_types);
            (body, types, next_id, disc)
        };

        self.merge_expr_types(local_expr_types);
        self.global_next_expr_id = new_next_id;
        self.work_queue.extend(discoveries);

        let mut concrete_fn = generic_fn.clone();
        concrete_fn.type_params.clear();
        concrete_fn.name = mangled_name;
        concrete_fn.params = concrete_params;
        concrete_fn.ret = concrete_ret;
        concrete_fn.body = new_body;

        // Also discover from concretized param/return types (these are now Named,
        // so they won't add duplicates — the WorkQueue deduplicates)
        for (_, param_ty) in &concrete_fn.params {
            let disc = discover::discover_type_specializations(param_ty, &self.index, self.interner);
            self.work_queue.extend(disc);
        }
        if let Some(ret) = &concrete_fn.ret {
            let disc = discover::discover_type_specializations(ret, &self.index, self.interner);
            self.work_queue.extend(disc);
        }

        // After specialization, scan the concrete body for newly-exposed
        // generic calls that need further specialization.
        let extra_items = discover::discover_calls_in_body(
            &concrete_fn.body, &self.index, self.interner, &mut self.mangle_table,
            &self.output_expr_types, self.call_type_args, &sub,
        );
        self.work_queue.extend(extra_items);
        self.output_items.push(HirItem::Fn(concrete_fn));
    }

    fn process_fn_passthrough(&mut self, name: Symbol) {
        let fn_def = match self.index.find_fn(name).cloned() {
            Some(f) => f,
            None => return,
        };
        let start_id = self.global_next_expr_id;
        let (rewritten_body, local_types, new_id, discoveries) = {
            let mut subst_ctx = SubstContext::new(
                start_id,
                self.interner,
                self.input_expr_types,
                self.call_type_args,
                &self.index,
                &mut self.mangle_table,
            );
            let body = subst_ctx.substitute_expr(&fn_def.body, &HashMap::new());
            let next_id = subst_ctx.next_expr_id();
            let disc = std::mem::take(&mut subst_ctx.discovered);
            let types = std::mem::take(&mut subst_ctx.output_expr_types);
            (body, types, next_id, disc)
        };
        self.merge_expr_types(local_types);
        self.global_next_expr_id = new_id;
        self.work_queue.extend(discoveries);

        let mut rewritten = fn_def.clone();
        rewritten.body = rewritten_body;
        let extra_items = discover::discover_calls_in_body(
            &rewritten.body, &self.index, self.interner, &mut self.mangle_table,
            &self.output_expr_types, self.call_type_args, &HashMap::new(),
        );
        self.work_queue.extend(extra_items);
        self.output_items.push(HirItem::Fn(rewritten));
    }

    fn process_struct_specialize(&mut self, name: Symbol, type_args: Vec<HirType>) {
        let generic = match self.index.find_struct(name).cloned() {
            Some(s) => s,
            None => return,
        };

        // Skip if type args don't cover all type params or have unresolved params
        if type_args.len() < generic.type_params.len()
            || type_args.iter().any(|t| concretize::has_unresolved_type_param(t, self.interner))
        {
            return;
        }

        let concrete = specialize::specialize_struct(
            &generic, &type_args, &self.index, &mut self.mangle_table, self.interner,
        );

        // Discover from substituted (pre-concretization) field types
        let sub = concretize::build_subst(&generic.type_params, &type_args);
        for field in &generic.fields {
            let substituted = crate::types::substitute_type(&field.ty, &sub);
            let disc = discover::discover_type_specializations(&substituted, &self.index, self.interner);
            self.work_queue.extend(disc);
        }

        // Also from concrete field types (Named won't duplicate)
        for field in &concrete.fields {
            let disc = discover::discover_type_specializations(&field.ty, &self.index, self.interner);
            self.work_queue.extend(disc);
        }
        self.output_items.push(HirItem::Struct(concrete));
    }

    fn process_struct_passthrough(&mut self, name: Symbol) {
        if let Some(s) = self.index.find_struct(name).cloned() {
            self.output_items.push(HirItem::Struct(s));
        }
    }

    fn process_enum_specialize(&mut self, name: Symbol, type_args: Vec<HirType>) {
        let generic = match self.index.find_enum(name).cloned() {
            Some(e) => e,
            None => return,
        };

        // Skip if type args don't cover all type params or have unresolved params
        if type_args.len() < generic.type_params.len()
            || type_args.iter().any(|t| concretize::has_unresolved_type_param(t, self.interner))
        {
            return;
        }

        let concrete = specialize::specialize_enum(
            &generic, &type_args, &self.index, &mut self.mangle_table, self.interner,
        );

        // Discover from substituted (pre-concretization) variant field types
        let sub = concretize::build_subst(&generic.type_params, &type_args);
        for variant in &generic.variants {
            for field in &variant.fields {
                let substituted = crate::types::substitute_type(&field.ty, &sub);
                let disc = discover::discover_type_specializations(&substituted, &self.index, self.interner);
                self.work_queue.extend(disc);
            }
        }

        // Also from concrete field types
        for variant in &concrete.variants {
            for field in &variant.fields {
                let disc = discover::discover_type_specializations(&field.ty, &self.index, self.interner);
                self.work_queue.extend(disc);
            }
        }
        self.output_items.push(HirItem::Enum(concrete));
    }

    fn process_enum_passthrough(&mut self, name: Symbol) {
        if let Some(e) = self.index.find_enum(name).cloned() {
            self.output_items.push(HirItem::Enum(e));
        }
    }

    fn merge_expr_types(&mut self, local: Vec<HirType>) {
        if local.len() > self.output_expr_types.len() {
            self.output_expr_types.resize(local.len(), HirType::Error);
        }
        for (i, ty) in local.into_iter().enumerate() {
            if i < self.output_expr_types.len() && ty != HirType::Error {
                self.output_expr_types[i] = ty;
            } else if i >= self.output_expr_types.len() {
                self.output_expr_types.push(ty);
            }
        }
    }

    pub fn run(&mut self, hir: &Hir, entry_points: &[Symbol]) {
        for &ep in entry_points {
            self.work_queue.push(WorkItem::fn_passthrough(ep));
        }
        let non_gen_fns: Vec<_> = self.index.fn_names()
            .filter(|&n| !self.index.is_generic_fn(n)).collect();
        for name in non_gen_fns {
            self.work_queue.push(WorkItem::fn_passthrough(name));
        }
        let non_gen_methods: Vec<_> = self.index.impl_method_names()
            .filter(|&n| !self.index.is_generic_fn(n)).collect();
        for name in non_gen_methods {
            self.work_queue.push(WorkItem::fn_passthrough(name));
        }
        let non_gen_structs: Vec<_> = self.index.struct_names()
            .filter(|&n| !self.index.is_generic_struct(n)).collect();
        for name in non_gen_structs {
            self.work_queue.push(WorkItem::struct_passthrough(name));
        }
        let non_gen_enums: Vec<_> = self.index.enum_names()
            .filter(|&n| !self.index.is_generic_enum(n)).collect();
        for name in non_gen_enums {
            self.work_queue.push(WorkItem::enum_passthrough(name));
        }
        for item in &hir.items {
            if let HirItem::Extern(ext) = item {
                self.output_items.push(HirItem::Extern(ext.clone()));
            }
        }

        while let Some(item) = self.work_queue.pop() {
            match item.kind {
                ItemKind::FnSpecialize => self.process_fn_specialize(item.def_id, item.type_args),
                ItemKind::FnPassthrough => self.process_fn_passthrough(item.def_id),
                ItemKind::StructSpecialize => self.process_struct_specialize(item.def_id, item.type_args),
                ItemKind::StructPassthrough => self.process_struct_passthrough(item.def_id),
                ItemKind::EnumSpecialize => self.process_enum_specialize(item.def_id, item.type_args),
                ItemKind::EnumPassthrough => self.process_enum_passthrough(item.def_id),
            }
        }
    }

    pub fn into_result(self) -> MonoResult {
        MonoResult {
            hir: crate::Hir { items: self.output_items },
            expr_types: self.output_expr_types,
        }
    }
}
