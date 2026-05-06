use crate::dispatch::DispatchTable;
use inkwell::context::Context;
use inkwell::module::Module;
use std::collections::HashMap;
use std::sync::Arc;

pub struct MicroModuleManager<'ctx> {
    context: &'ctx Context,
    prefix: String,
    modules: HashMap<String, Module<'ctx>>,
    dispatch: Arc<DispatchTable>,
}

impl<'ctx> MicroModuleManager<'ctx> {
    pub fn new(context: &'ctx Context, prefix: &str, dispatch: Arc<DispatchTable>) -> Self {
        Self { context, prefix: prefix.to_string(), modules: HashMap::new(), dispatch }
    }

    pub fn create_module_for_item(&mut self, item_name: &str) -> Option<&Module<'ctx>> {
        let module_name = format!("{}_{}", self.prefix, item_name);
        let module = self.context.create_module(&module_name);
        self.modules.insert(item_name.to_string(), module);
        self.modules.get(item_name)
    }

    pub fn get_module(&self, item_name: &str) -> Option<&Module<'ctx>> {
        self.modules.get(item_name)
    }

    pub fn remove_module(&mut self, item_name: &str) {
        self.modules.remove(item_name);
    }

    pub fn contains(&self, item_name: &str) -> bool {
        self.modules.contains_key(item_name)
    }

    pub fn module_count(&self) -> usize { self.modules.len() }

    pub fn item_names(&self) -> Vec<&str> {
        self.modules.keys().map(|s| s.as_str()).collect()
    }

    pub fn dispatch(&self) -> &DispatchTable { &self.dispatch }
}
