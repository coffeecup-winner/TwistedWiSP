use std::{
    collections::{hash_map, HashMap},
    error::Error,
    path::Path,
};

use crate::{
    core::{
        BuiltinFunction, CodeFunction, CodeFunctionParseResult, CodeFunctionParser, DataType,
        DefaultInputValue, FlowFunction, FlowNodeIndex, FunctionInput, FunctionOutput,
        WispFunction,
    },
    ir::{Instruction, Operand, SignalOutputIndex, TargetLocation},
};

use log::info;

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
        self.add_function(Self::build_function_noise());
    }

    // TODO: Make a BuiltinFunction instead?
    fn build_function_out(ctx: &WispContext) -> Box<dyn WispFunction> {
        assert!(ctx.num_outputs > 0, "Invalid number of output channels");
        let mut out_inputs = vec![FunctionInput::new(
            "ch".into(),
            DataType::Float,
            DefaultInputValue::Value(0.0),
        )];
        out_inputs.extend(vec![
            FunctionInput::new(
                "ch".into(),
                DataType::Float,
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

    fn build_function_noise() -> Box<dyn WispFunction> {
        Box::new(BuiltinFunction::new(
            "noise".into(),
            vec![],
            vec![FunctionOutput::new("out".into(), DataType::Float)],
        ))
    }

    pub fn load_core_functions(&mut self, wisp_core_path: &Path) -> Result<(), Box<dyn Error>> {
        for file in std::fs::read_dir(wisp_core_path)? {
            let file = file?;
            let text = std::fs::read_to_string(file.path())?;

            if text.starts_with("[flow]") {
                // TODO: Stop reading this file twice
                self.load_function(&file.path())?;
                continue;
            }

            let mut parser = CodeFunctionParser::new(&text);
            info!("Adding core functions:");
            while let Some(result) = parser.parse_function() {
                match result {
                    CodeFunctionParseResult::Function(func) => {
                        info!("  - {}", func.name());
                        self.add_function(Box::new(func));
                    }
                    CodeFunctionParseResult::Alias(alias, target) => {
                        let mut func = self
                            .get_function(&target)
                            .expect("Unknown function alias target")
                            .clone();
                        info!("  - {} (alias of {})", alias, func.name());
                        *func.name_mut() = alias;
                        self.add_function(func);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn load_function(&mut self, file_path: &Path) -> Result<String, Box<dyn Error>> {
        let text = std::fs::read_to_string(file_path)?;
        // TODO: Load flow or code function
        let func = FlowFunction::load(&text, self).expect("Failed to parse the flow function data");
        let flow_name = func.name().to_owned();
        info!("Loading function: {}", flow_name);
        self.add_function(func);
        Ok(flow_name)
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn add_function(&mut self, func: Box<dyn WispFunction>) -> Option<Box<dyn WispFunction>> {
        self.functions.insert(func.name().into(), func)
    }

    pub fn remove_function(&mut self, name: &str) -> Option<Box<dyn WispFunction>> {
        self.functions.remove(name)
    }

    pub fn functions_iter(&self) -> hash_map::Values<'_, String, Box<dyn WispFunction>> {
        self.functions.values()
    }

    pub fn get_function(&self, name: &str) -> Option<&dyn WispFunction> {
        if let Some((flow_name, _)) = name.split_once(':') {
            self.get_function(flow_name)
                .and_then(|f| f.as_flow())
                .and_then(|f| f.get_function(name))
        } else {
            self.functions.get(name).map(|f| &**f)
        }
    }

    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Box<dyn WispFunction>> {
        if let Some((flow_name, _)) = name.split_once(':') {
            self.get_function_mut(flow_name)
                .and_then(|f| f.as_flow_mut())
                .and_then(|f| f.get_function_mut(name))
        } else {
            self.functions.get_mut(name)
        }
    }

    pub fn flow_add_node(&mut self, flow_name: &str, node_text: &str) -> (FlowNodeIndex, String) {
        let flow = self
            .get_function_mut(flow_name)
            .unwrap()
            .as_flow_mut()
            .unwrap();
        let idx = flow.add_node(node_text);
        (idx, flow.get_node(idx).unwrap().name.clone())
    }

    pub fn flow_remove_node(&mut self, flow_name: &str, node_idx: FlowNodeIndex) -> Option<String> {
        let flow = self
            .get_function_mut(flow_name)
            .unwrap()
            .as_flow_mut()
            .unwrap();
        if let Some(node) = flow.remove_node(node_idx) {
            if node.name.starts_with("$math") {
                self.remove_function(&node.name);
            }
            Some(node.name)
        } else {
            None
        }
    }
}
