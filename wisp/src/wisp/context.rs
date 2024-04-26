use std::collections::{hash_map, HashMap};

use crate::wisp::{
    function::{DefaultInputValue, FunctionInput},
    ir::{Instruction, Operand, SignalOutputIndex, TargetLocation},
};

use super::{
    function::{Function, FunctionDataItem, FunctionOutput},
    ir::{DataRef, FunctionOutputIndex, SourceLocation, VarRef},
};

#[derive(Debug, Default)]
pub struct WispContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, Function>,
}

impl WispContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispContext {
            num_outputs,
            sample_rate,
            ..Default::default()
        }
    }

    pub fn register_builtin_functions(&mut self) {
        self.add_function(Self::build_function_out(self));
        self.add_function(Self::build_function_lag());
    }

    fn build_function_out(ctx: &WispContext) -> Function {
        assert!(ctx.num_outputs > 0, "Invalid number of output channels");
        let mut out_inputs = vec![FunctionInput::new(Some(DefaultInputValue::Value(0.0)))];
        out_inputs.extend(vec![
            FunctionInput::new(Some(DefaultInputValue::Normal));
            ctx.num_outputs as usize - 1
        ]);
        let mut instructions = vec![];
        for i in 0..ctx.num_outputs {
            instructions.push(Instruction::Store(
                TargetLocation::SignalOutput(SignalOutputIndex(i)),
                Operand::Arg(i),
            ));
        }
        Function::new("out".into(), out_inputs, vec![], vec![], instructions, None)
    }

    fn build_function_lag() -> Function {
        Function::new(
            "lag".into(),
            vec![FunctionInput::default()],
            vec![FunctionOutput],
            vec![FunctionDataItem::new("prev".into(), 0.0)],
            vec![
                Instruction::Load(VarRef(0), SourceLocation::Data(DataRef(0))),
                Instruction::Store(
                    TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                    Operand::Var(VarRef(0)),
                ),
                Instruction::Store(TargetLocation::Data(DataRef(0)), Operand::Arg(0)),
            ],
            Some(DataRef(0)),
        )
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn add_function(&mut self, func: Function) {
        self.functions.insert(func.name().into(), func);
    }

    pub fn get_function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    pub fn functions_iter(&self) -> hash_map::Iter<'_, String, Function> {
        self.functions.iter()
    }

    pub fn update_all_function_instructions(&self) {
        for (_, f) in self.functions.iter() {
            f.update_instructions(self);
        }
    }
}
