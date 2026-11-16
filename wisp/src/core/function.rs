use std::fmt::Debug;

use crate::{
    core::{context::WispContext, FlowFunction},
    ir::{DataRef, IRFunction, IRFunctionDataType},
};

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

impl From<DataType> for IRFunctionDataType {
    fn from(data_type: DataType) -> Self {
        match data_type {
            DataType::Float => IRFunctionDataType::Float,
            DataType::Array => IRFunctionDataType::Array,
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
    // Empty data array
    EmptyArray,
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
    fn name_mut(&mut self) -> &mut String;
    fn inputs(&self) -> &[FunctionInput];
    fn outputs(&self) -> &[FunctionOutput];
    fn get_ir_functions(&self, ctx: &WispContext) -> Vec<IRFunction>;

    fn lag_value(&self) -> Option<DataRef> {
        None
    }
    fn as_flow(&self) -> Option<&FlowFunction> {
        None
    }
    fn as_flow_mut(&mut self) -> Option<&mut FlowFunction> {
        None
    }

    fn save(&self) -> String;
}

#[derive(Debug, Clone)]
pub enum Function {
    Builtin(super::BuiltinFunction),
    Code(super::CodeFunction),
    Flow(super::FlowFunction),
    Math(super::MathFunction),
}

impl WispFunction for Function {
    fn name(&self) -> &str {
        match self {
            Function::Builtin(f) => f.name(),
            Function::Code(f) => f.name(),
            Function::Flow(f) => f.name(),
            Function::Math(f) => f.name(),
        }
    }

    fn name_mut(&mut self) -> &mut String {
        match self {
            Function::Builtin(f) => f.name_mut(),
            Function::Code(f) => f.name_mut(),
            Function::Flow(f) => f.name_mut(),
            Function::Math(f) => f.name_mut(),
        }
    }

    fn inputs(&self) -> &[FunctionInput] {
        match self {
            Function::Builtin(f) => f.inputs(),
            Function::Code(f) => f.inputs(),
            Function::Flow(f) => f.inputs(),
            Function::Math(f) => f.inputs(),
        }
    }

    fn outputs(&self) -> &[FunctionOutput] {
        match self {
            Function::Builtin(f) => f.outputs(),
            Function::Code(f) => f.outputs(),
            Function::Flow(f) => f.outputs(),
            Function::Math(f) => f.outputs(),
        }
    }

    fn get_ir_functions(&self, ctx: &WispContext) -> Vec<IRFunction> {
        match self {
            Function::Builtin(f) => f.get_ir_functions(ctx),
            Function::Code(f) => f.get_ir_functions(ctx),
            Function::Flow(f) => f.get_ir_functions(ctx),
            Function::Math(f) => f.get_ir_functions(ctx),
        }
    }

    fn lag_value(&self) -> Option<DataRef> {
        match self {
            Function::Builtin(f) => f.lag_value(),
            Function::Code(f) => f.lag_value(),
            Function::Flow(f) => f.lag_value(),
            Function::Math(f) => f.lag_value(),
        }
    }

    fn as_flow(&self) -> Option<&FlowFunction> {
        match self {
            Function::Flow(f) => Some(f),
            _ => None,
        }
    }

    fn as_flow_mut(&mut self) -> Option<&mut FlowFunction> {
        match self {
            Function::Flow(f) => Some(f),
            _ => None,
        }
    }

    fn save(&self) -> String {
        match self {
            Function::Builtin(f) => f.save(),
            Function::Code(f) => f.save(),
            Function::Flow(f) => f.save(),
            Function::Math(f) => f.save(),
        }
    }
}
