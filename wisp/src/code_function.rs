use crate::{
    FunctionDataItem, FunctionInput, FunctionOutput, WispContext, WispFunction,
};

use twisted_wisp_ir::{
    DataRef, IRFunction, IRFunctionDataItem, IRFunctionInput, IRFunctionOutput, Instruction,
};

#[derive(Debug)]
pub struct CodeFunction {
    name: String,
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
    data: Vec<FunctionDataItem>,
    ir: Vec<Instruction>,
    lag_value: Option<DataRef>,
}

impl WispFunction for CodeFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn inputs_count(&self) -> u32 {
        self.inputs.len() as u32
    }

    fn input(&self, idx: u32) -> Option<&FunctionInput> {
        self.inputs.get(idx as usize)
    }

    fn outputs_count(&self) -> u32 {
        self.outputs.len() as u32
    }

    fn output(&self, idx: u32) -> Option<&FunctionOutput> {
        self.outputs.get(idx as usize)
    }

    fn get_ir_function(&self, _ctx: &WispContext) -> IRFunction {
        IRFunction {
            name: self.name.clone(),
            inputs: self.inputs.iter().map(|_| IRFunctionInput).collect(),
            outputs: self.outputs.iter().map(|_| IRFunctionOutput).collect(),
            data: self.data.iter().map(|_| IRFunctionDataItem).collect(),
            ir: self.ir.clone(),
        }
    }

    fn lag_value(&self) -> Option<DataRef> {
        self.lag_value
    }
}

impl CodeFunction {
    pub fn new(
        name: String,
        inputs: Vec<FunctionInput>,
        outputs: Vec<FunctionOutput>,
        data: Vec<FunctionDataItem>,
        instructions: Vec<Instruction>,
        lag_value: Option<DataRef>,
    ) -> Self {
        CodeFunction {
            name,
            inputs,
            outputs,
            data,
            ir: instructions,
            lag_value,
        }
    }
}
