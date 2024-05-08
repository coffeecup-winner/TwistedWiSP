use std::{
    collections::{hash_map, HashMap},
    error::Error,
    path::Path,
};

use crate::{
    CodeFunction, CodeFunctionParser, DefaultInputValue, FlowFunction, FlowNodeIndex,
    FunctionInput, MathFunctionParser, WispFunction,
};

use log::info;
use twisted_wisp_ir::{Instruction, Operand, SignalOutputIndex, TargetLocation};

#[derive(Debug)]
pub struct LoadFunctionResult {
    pub name: String,
    pub math_function_names: Vec<String>,
    pub replaced_existing: bool,
}

#[derive(Debug)]
pub struct WispContext {
    num_outputs: u32,
    functions: HashMap<String, Box<dyn WispFunction>>,
    math_function_id_gen: HashMap<String, u32>,
}

impl WispContext {
    pub fn new(num_outputs: u32) -> Self {
        WispContext {
            num_outputs,
            functions: HashMap::new(),
            math_function_id_gen: HashMap::new(),
        }
    }

    pub fn add_builtin_functions(&mut self) {
        self.add_function(Self::build_function_out(self));
        self.add_function(Self::build_function_stub());
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

    fn build_function_stub() -> Box<dyn WispFunction> {
        Box::new(CodeFunction::new(
            "$stub".into(),
            vec![],
            vec![],
            vec![],
            vec![],
            None,
        ))
    }

    pub fn load_core_functions(&mut self, wisp_core_path: &str) -> Result<(), Box<dyn Error>> {
        for file in std::fs::read_dir(Path::new(wisp_core_path))? {
            let text = std::fs::read_to_string(file?.path())?;
            let mut parser = CodeFunctionParser::new(&text);
            info!("Adding core functions:");
            while let Some(func) = parser.parse_function() {
                info!("  - {}", func.name());
                self.add_function(Box::new(func));
            }
        }
        Ok(())
    }

    pub fn load_function(&mut self, file_path: &str) -> Result<LoadFunctionResult, Box<dyn Error>> {
        let text = std::fs::read_to_string(Path::new(file_path))?;
        // TODO: Load flow or code function
        let mut func = FlowFunction::load(&text).expect("Failed to parse the flow function data");
        let flow_name = func.name().to_owned();
        info!("Loading function: {}", flow_name);
        let flow = func.as_flow_mut().unwrap();
        let mut math_function_names = vec![];
        for n in flow.node_indices() {
            let node = flow.get_node(n).unwrap();
            if node.name.starts_with("$math") {
                let parts = node.name.split('$');
                let id = parts.last().unwrap().parse::<u32>().unwrap();
                let id_gen = self
                    .math_function_id_gen
                    .entry(flow_name.to_string())
                    .or_insert(0);
                *id_gen = (*id_gen).max(id + 1);
                let math_func = Box::new(
                    MathFunctionParser::parse_function(&flow_name, id, &node.display_text).unwrap(),
                );
                info!("  - {}: {}", math_func.name(), text);
                math_function_names.push(math_func.name().into());
                self.add_function(math_func);
            }
        }
        let old_function = self.add_function(func);
        Ok(LoadFunctionResult {
            name: flow_name,
            math_function_names,
            replaced_existing: old_function.is_some(),
        })
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
        self.functions.get(name).map(|f| &**f)
    }

    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Box<dyn WispFunction>> {
        self.functions.get_mut(name)
    }

    pub fn flow_add_node(&mut self, flow_name: &str, node_text: &str) -> (FlowNodeIndex, String) {
        let func_name = if node_text.starts_with('=') {
            let id_gen = self
                .math_function_id_gen
                .entry(flow_name.to_string())
                .or_insert(0);
            let id = *id_gen;
            *id_gen += 1;
            if let Some(func) = MathFunctionParser::parse_function(flow_name, id, node_text) {
                let func_name = func.name().to_owned();
                self.add_function(Box::new(func));
                func_name
            } else {
                "$stub".into()
            }
        } else if let Some(func) = self.get_function(node_text) {
            func.name().to_owned()
        } else {
            "$stub".into()
        };

        let flow = self
            .get_function_mut(flow_name)
            .unwrap()
            .as_flow_mut()
            .unwrap();
        let idx = flow.add_node(&func_name, node_text);
        (idx, func_name)
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
