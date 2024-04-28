use std::collections::{hash_map, HashMap};

use crate::{CodeFunction, DefaultInputValue, FunctionInput, WispFunction};

use twisted_wisp_ir::{Instruction, Operand, SignalOutputIndex, TargetLocation};

#[derive(Debug)]
pub struct WispContext {
    num_outputs: u32,
    functions: HashMap<String, Box<dyn WispFunction>>,
}

impl WispContext {
    pub fn new(num_outputs: u32) -> Self {
        WispContext {
            num_outputs,
            functions: HashMap::new(),
        }
    }

    pub fn add_builtin_functions(&mut self) {
        self.add_function(Self::build_function_out(self));
    }

    fn build_function_out(ctx: &WispContext) -> Box<dyn WispFunction> {
        assert!(ctx.num_outputs > 0, "Invalid number of output channels");
        let mut out_inputs = vec![FunctionInput::new(
            "ch".into(),
            DefaultInputValue::Value(0.0),
        )];
        out_inputs.extend(vec![
            FunctionInput::new(
                "ch".into(),
                DefaultInputValue::Normal
            );
            ctx.num_outputs as usize - 1
        ]);
        for (i, item) in out_inputs.iter_mut().enumerate() {
            item.name += &i.to_string();
        }
        let mut instructions = vec![];
        for i in 0..ctx.num_outputs {
            instructions.push(Instruction::Store(
                TargetLocation::SignalOutput(SignalOutputIndex(i)),
                Operand::Arg(i),
            ));
        }
        Box::new(CodeFunction::new(
            "out".into(),
            out_inputs,
            vec![],
            vec![],
            instructions,
            None,
        ))
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn add_function(&mut self, func: Box<dyn WispFunction>) {
        self.functions.insert(func.name().into(), func);
    }

    pub fn remove_function(&mut self, name: &str) -> Option<Box<dyn WispFunction>> {
        self.functions.remove(name)
    }

    pub fn functions_iter(&self) -> hash_map::Values<'_, String, Box<dyn WispFunction>> {
        self.functions.values()
    }

    pub fn get_function(&self, name: &str) -> Option<&dyn WispFunction> {
        self.functions.get(name).map(|f| &**f)
    }

    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Box<dyn WispFunction>> {
        self.functions.get_mut(name)
    }
}
