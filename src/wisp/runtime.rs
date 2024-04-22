use std::collections::{hash_map, HashMap};

use crate::wisp::ir::Operand;

use super::{
    function::{DefaultInputValue, Function, FunctionDataItem, FunctionInput, FunctionOutput},
    ir::{
        DataRef, FunctionOutputIndex, Instruction, SignalOutputIndex, SourceLocation,
        TargetLocation, VarRef,
    },
};

#[derive(Debug, Default)]
pub struct Runtime {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, Function>,
}

impl Runtime {
    pub fn init(num_outputs: u32, sample_rate: u32) -> Self {
        let mut runtime = Runtime {
            num_outputs,
            sample_rate,
            ..Default::default()
        };
        Self::register_builtin_functions(&mut runtime);
        runtime
    }

    fn register_builtin_functions(runtime: &mut Runtime) {
        runtime.add_function(Self::build_function_out(runtime));
        runtime.add_function(Self::build_function_lag());
    }

    fn build_function_out(runtime: &Runtime) -> Function {
        assert!(runtime.num_outputs > 0, "Invalid number of output channels");
        let mut out_inputs = vec![FunctionInput::new(Some(DefaultInputValue::Value(0.0)))];
        out_inputs.extend(vec![
            FunctionInput::new(Some(DefaultInputValue::Normal));
            runtime.num_outputs as usize - 1
        ]);
        let mut instructions = vec![];
        for i in 0..runtime.num_outputs {
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
                    Operand::Arg(0),
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
}
