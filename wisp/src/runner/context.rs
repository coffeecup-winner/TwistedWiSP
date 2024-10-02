use std::collections::{hash_map, HashMap};

use inkwell::context::Context;

use crate::ir::IRFunction;

pub struct WispExecutionContext {
    context: Context,
}

impl WispExecutionContext {
    pub fn init() -> Self {
        WispExecutionContext {
            context: Context::create(),
        }
    }

    pub fn llvm(&self) -> &Context {
        &self.context
    }
}

#[derive(Debug, Default)]
pub struct WispEngineContext {
    functions: HashMap<String, IRFunction>,
}

impl WispEngineContext {
    pub fn new() -> Self {
        WispEngineContext {
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        self.functions.clear();
    }

    pub fn add_function(&mut self, func: IRFunction) {
        self.functions.insert(func.name().into(), func);
    }

    pub fn remove_function(&mut self, name: &str) -> Option<IRFunction> {
        self.functions.remove(name)
    }

    pub fn get_function(&self, name: &str) -> Option<&IRFunction> {
        self.functions.get(name)
    }

    pub fn functions_iter(&self) -> hash_map::Iter<'_, String, IRFunction> {
        self.functions.iter()
    }
}
