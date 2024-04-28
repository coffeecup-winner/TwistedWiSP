use std::fmt::Debug;

use crate::{context::WispContext, FlowFunction};

use twisted_wisp_ir::{DataRef, IRFunction};

#[derive(Debug, Clone, Copy)]
pub struct FunctionInput {
    pub fallback: DefaultInputValue,
}

#[derive(Debug, Clone, Copy)]
pub enum DefaultInputValue {
    // Default constant value
    Value(f32),
    // Normalled to the previous argument
    Normal,
    // Don't call this function (must have a lag value to use instead)
    Skip,
}

impl FunctionInput {
    pub fn new(fallback: DefaultInputValue) -> Self {
        FunctionInput { fallback }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FunctionOutput;

#[derive(Debug)]
pub struct FunctionDataItem {
    pub name: String,
    pub default_value: f32,
}

impl FunctionDataItem {
    pub fn new(name: String, default_value: f32) -> Self {
        FunctionDataItem {
            name,
            default_value,
        }
    }
}

pub trait WispFunction: Debug {
    fn name(&self) -> &str;
    fn inputs_count(&self) -> u32;
    fn input(&self, idx: u32) -> Option<&FunctionInput>;
    fn outputs_count(&self) -> u32;
    fn output(&self, idx: u32) -> Option<&FunctionOutput>;
    fn get_ir_function(&self, ctx: &WispContext) -> IRFunction;

    fn lag_value(&self) -> Option<DataRef> {
        None
    }
    fn as_flow(&self) -> Option<&FlowFunction> {
        None
    }
    fn as_flow_mut(&mut self) -> Option<&mut FlowFunction> {
        None
    }

    fn load(s: &str) -> Option<Box<dyn WispFunction>>
    where
        Self: Sized;
    fn save(&self) -> String;
}
