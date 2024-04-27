use std::cell::{Ref, RefCell};

use crate::context::WispContext;

use super::flow::Flow;

use twisted_wisp_ir::{
    DataRef, IRFunction, IRFunctionDataItem, IRFunctionInput, IRFunctionOutput, Instruction,
};

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

#[derive(Debug)]
pub struct Function {
    name: String,
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
    data: Vec<FunctionDataItem>,
    flow: Option<Flow>,
    instructions: RefCell<Vec<Instruction>>,
    lag_value: Option<DataRef>,
}

impl Function {
    pub fn new(
        name: String,
        inputs: Vec<FunctionInput>,
        outputs: Vec<FunctionOutput>,
        data: Vec<FunctionDataItem>,
        instructions: Vec<Instruction>,
        lag_value: Option<DataRef>,
    ) -> Self {
        Function {
            name,
            inputs,
            outputs,
            data,
            flow: None,
            instructions: RefCell::new(instructions),
            lag_value,
        }
    }

    pub fn new_flow(name: String, flow: Flow) -> Self {
        Function {
            name,
            inputs: vec![],
            outputs: vec![],
            data: vec![],
            flow: Some(flow),
            instructions: RefCell::new(vec![]),
            lag_value: None,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn inputs(&self) -> &[FunctionInput] {
        &self.inputs
    }

    pub fn outputs(&self) -> &[FunctionOutput] {
        &self.outputs
    }

    pub fn data(&self) -> &[FunctionDataItem] {
        &self.data
    }

    pub fn flow_mut(&mut self) -> Option<&mut Flow> {
        self.flow.as_mut()
    }

    pub fn instructions(&self) -> Ref<'_, Vec<Instruction>> {
        self.instructions.borrow()
    }

    pub fn lag_value(&self) -> Option<DataRef> {
        self.lag_value
    }

    pub fn update_instructions(&self, ctx: &WispContext) {
        // TODO: Only do this if the flow has changed
        if let Some(flow) = self.flow.as_ref() {
            *self.instructions.borrow_mut() = flow.compile_to_ir(ctx);
        }
    }

    pub fn get_ir_function(&self) -> IRFunction {
        IRFunction {
            name: self.name.clone(),
            inputs: self.inputs.iter().map(|_| IRFunctionInput).collect(),
            outputs: self.outputs.iter().map(|_| IRFunctionOutput).collect(),
            data: self.data.iter().map(|_| IRFunctionDataItem).collect(),
            ir: self.instructions.borrow().clone(),
        }
    }
}
