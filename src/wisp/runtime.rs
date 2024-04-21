use std::collections::{hash_map, HashMap};

use super::{
    function::{Function, FunctionInput},
    ir::{FunctionInputIndex, Instruction, OutputIndex, VarRef},
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
        let out = Function::new(
            "out".into(),
            vec![FunctionInput; runtime.num_outputs as usize],
            vec![],
            vec![
                Instruction::LoadFunctionInput(VarRef(0), FunctionInputIndex(0)),
                Instruction::Output(OutputIndex(0), VarRef(0)),
            ],
        );
        runtime.register_function(out);
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn register_function(&mut self, func: Function) {
        self.functions.insert(func.name().into(), func);
    }

    pub fn get_function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    pub fn functions_iter(&self) -> hash_map::Iter<'_, String, Function> {
        self.functions.iter()
    }
}
