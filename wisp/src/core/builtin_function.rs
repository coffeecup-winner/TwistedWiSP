use crate::{
    core::{FunctionInput, FunctionOutput, WispContext, WispFunction},
    ir::{IRFunction, IRFunctionInput, IRFunctionOutput},
};

#[derive(Debug, PartialEq, Clone)]
pub struct BuiltinFunction {
    name: String,
    inputs: Vec<FunctionInput>,
    outputs: Vec<FunctionOutput>,
}

impl BuiltinFunction {
    pub fn new(name: String, inputs: Vec<FunctionInput>, outputs: Vec<FunctionOutput>) -> Self {
        BuiltinFunction {
            name,
            inputs,
            outputs,
        }
    }
}

impl WispFunction for BuiltinFunction {
    fn name(&self) -> &str {
        &self.name
    }

    fn name_mut(&mut self) -> &mut String {
        &mut self.name
    }

    fn inputs(&self) -> &[FunctionInput] {
        &self.inputs
    }

    fn outputs(&self) -> &[FunctionOutput] {
        &self.outputs
    }

    fn get_ir_functions(&self, _ctx: &WispContext) -> Vec<IRFunction> {
        let vec = vec![IRFunction {
            name: self.name.clone(),
            inputs: self
                .inputs
                .iter()
                .map(|i| IRFunctionInput {
                    type_: i.type_.into(),
                })
                .collect(),
            outputs: self
                .outputs
                .iter()
                .map(|o| IRFunctionOutput {
                    type_: o.type_.into(),
                })
                .collect(),
            data: vec![],
            ir: vec![],
        }];
        vec
    }

    fn load(_s: &str, _ctx: &WispContext) -> Option<Box<dyn WispFunction>> {
        None
    }

    fn save(&self) -> String {
        self.name.clone()
    }

    fn clone(&self) -> Box<dyn WispFunction> {
        Box::new(std::clone::Clone::clone(self))
    }
}
