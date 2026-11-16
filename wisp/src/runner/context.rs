use std::collections::{hash_map, BTreeSet, HashMap};

use crate::{ir::IRFunction, utils::dep_prop::Property};

#[derive(Debug)]
pub struct RuntimeFunction {
    ir_function: Property<IRFunction>,
    dependencies: Property<BTreeSet<String>>,
}

impl RuntimeFunction {
    pub fn new(ir_function: IRFunction) -> Self {
        RuntimeFunction {
            ir_function: Property::new(ir_function),
            dependencies: Property::new(BTreeSet::new()),
        }
    }

    pub fn ir_function(&self) -> &Property<IRFunction> {
        &self.ir_function
    }

    pub fn dependencies(&self) -> &Property<BTreeSet<String>> {
        &self.dependencies
    }
}

#[derive(Debug)]
pub struct WispRuntimeContext {
    functions: HashMap<String, RuntimeFunction>,
    active_set: Property<Vec<String>>,
}

impl WispRuntimeContext {
    pub fn new() -> Self {
        WispRuntimeContext {
            functions: HashMap::new(),
            active_set: Property::new(Vec::new()),
        }
    }

    pub fn reset(&mut self) {
        self.functions.clear();
    }

    pub fn add_function(&mut self, func: IRFunction) {
        if let Some(f) = self.functions.get_mut(func.name()) {
            // TODO: Check if the function didn't change?
            f.ir_function().set(func);
        } else {
            self.functions
                .insert(func.name().into(), RuntimeFunction::new(func));
        }
    }

    pub fn remove_function(&mut self, name: &str) -> Option<RuntimeFunction> {
        self.functions.remove(name)
    }

    pub fn get_function(&self, name: &str) -> Option<&RuntimeFunction> {
        self.functions.get(name)
    }

    pub fn functions_iter(&self) -> hash_map::Iter<'_, String, RuntimeFunction> {
        self.functions.iter()
    }

    pub fn active_set(&self) -> &Property<Vec<String>> {
        &self.active_set
    }
}
