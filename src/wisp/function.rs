use super::ir::Instruction;

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
pub struct Function {
    name: String,
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
    instructions: Vec<Instruction>,
}

impl Function {
    pub fn new(
        name: String,
        inputs: Vec<FunctionInput>,
        outputs: Vec<FunctionOutput>,
        instructions: Vec<Instruction>,
    ) -> Self {
        Function {
            name,
            inputs,
            outputs,
            instructions,
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

    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }
}
