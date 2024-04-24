use super::ir::{DataRef, Instruction};

#[derive(Debug, Default, Clone, Copy)]
pub struct FunctionInput {
    pub fallback: Option<DefaultInputValue>,
}

#[derive(Debug, Clone, Copy)]
pub enum DefaultInputValue {
    // Default constant value
    Value(f32),
    // Normalled to the previous argument
    Normal,
}

impl FunctionInput {
    pub fn new(fallback: Option<DefaultInputValue>) -> Self {
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
    instructions: Vec<Instruction>,
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
            instructions,
            lag_value,
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

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn lag_value(&self) -> Option<DataRef> {
        self.lag_value
    }
}
