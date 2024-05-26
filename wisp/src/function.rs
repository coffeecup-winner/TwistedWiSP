use std::fmt::Debug;

use crate::{context::WispContext, FlowFunction};

use twisted_wisp_ir::{DataRef, IRFunction};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DataType {
    Float,
    Array,
}

impl DataType {
    pub fn to_str(&self) -> &str {
        match self {
            DataType::Float => "float",
            DataType::Array => "array",
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionInput {
    pub name: String,
    pub type_: DataType,
    pub fallback: DefaultInputValue,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum DefaultInputValue {
    // Default constant value
    Value(f32),
    // Normalled to the previous argument
    Normal,
    // Don't call this function (must have a lag value to use instead)
    Skip,
}

impl FunctionInput {
    pub fn new(name: String, type_: DataType, fallback: DefaultInputValue) -> Self {
        FunctionInput {
            name,
            type_,
            fallback,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FunctionOutput {
    pub name: String,
    pub type_: DataType,
}

impl FunctionOutput {
    pub fn new(name: String, type_: DataType) -> Self {
        FunctionOutput { name, type_ }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct FunctionDataItem {
    pub name: String,
    pub type_: DataType,
}

impl FunctionDataItem {
    pub fn new(name: String, type_: DataType) -> Self {
        FunctionDataItem { name, type_ }
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
