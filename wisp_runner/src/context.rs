use std::collections::{hash_map, HashMap};

use inkwell::context::Context;
use twisted_wisp_ir::IRFunction;

use crate::compiler::DataArray;

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

#[derive(Debug, Clone)]
pub struct WispDataArray {
    pub data: Vec<f32>,
    pub array: *mut DataArray,
}

#[derive(Debug, Default)]
pub struct WispContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, IRFunction>,
    main_function: String,
    data_arrays: HashMap<String, WispDataArray>,
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

    pub fn add_builtin_data_arrays(&mut self) {
        const LENGTH: usize = 1024;
        let mut data = vec![0.0; LENGTH];
        const STEP: f32 = 2.0 * std::f32::consts::PI / (LENGTH as f32);
        for (i, value) in data.iter_mut().enumerate() {
            *value = (i as f32 * STEP).sin();
        }
        self.add_data_array("sine".into(), data);
    }

    pub fn add_data_array(&mut self, array_name: String, mut data: Vec<f32>) {
        data.insert(0, f32::from_bits(data.len() as u32));
        let array = data.as_mut_ptr() as *mut DataArray;
        self.data_arrays
            .insert(array_name, WispDataArray { data, array });
    }

    pub fn get_data_array(&mut self, array_name: &str) -> Option<*mut DataArray> {
        self.data_arrays.get_mut(array_name).map(|a| a.array)
    }
}
