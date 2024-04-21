use super::ir::Instruction;

#[derive(Debug, Clone, Copy)]
pub struct FunctionInput;

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
