use super::ir::Instruction;

pub struct Function {
    ir: Vec<Instruction>,
}

impl Function {
    pub fn new(ir: Vec<Instruction>) -> Self {
        Function { ir }
    }

    pub fn instructions(&self) -> &[Instruction] {
        &self.ir
    }
}
