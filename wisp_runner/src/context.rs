use std::collections::{hash_map, HashMap};

use inkwell::context::Context;
use twisted_wisp_ir::IRFunction;

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
pub struct WispContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, IRFunction>,
    main_function: String,
}

impl WispContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispContext {
            num_outputs,
            sample_rate,
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        self.functions.clear();
        self.main_function = String::new();
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
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

    pub fn set_main_function(&mut self, name: &str) {
        self.main_function = name.into();
    }

    pub fn main_function(&self) -> &str {
        &self.main_function
    }
}
