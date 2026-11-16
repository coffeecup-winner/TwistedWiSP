use std::{
    collections::{hash_map, HashMap},
    error::Error,
    path::Path,
};

use crate::{
    compiler::DataArrayHandle,
    core::{
        BuiltinFunction, CodeFunction, CodeFunctionParseResult, CodeFunctionParser, DataType,
        DefaultInputValue, FlowFunction, FlowNodeIndex, FunctionInput, FunctionOutput,
        WispFunction,
    },
    ir::{Instruction, Operand, SignalOutputIndex, TargetLocation},
};

use log::info;
use string_error::into_err;

use super::{Function, FunctionHandle};

#[derive(Debug)]
pub struct WispContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, FunctionHandle>,
    data_arrays: HashMap<String, HashMap<String, DataArrayHandle>>,
}

impl WispContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispContext {
            num_outputs,
            sample_rate,
            functions: HashMap::new(),
            data_arrays: HashMap::new(),
        }
    }

    pub fn add_builtin_functions(&mut self) {
        self.add_function(Self::build_function_out(self));
        self.add_function(Self::build_function_noise());
    }

    // TODO: Make a BuiltinFunction instead?
    fn build_function_out(ctx: &WispContext) -> Function {
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
        Function::Code(CodeFunction::new(
            "out".into(),
            out_inputs,
            vec![],
            vec![],
            instructions,
            None,
        ))
    }

    fn build_function_noise() -> Function {
        Function::Builtin(BuiltinFunction::new(
            "noise".into(),
            vec![],
            vec![FunctionOutput::new("out".into(), DataType::Float)],
        ))
    }

    pub fn load_core_functions(&mut self, wisp_core_path: &Path) -> Result<(), Box<dyn Error>> {
        let mut files = std::fs::read_dir(wisp_core_path)?
            .map(|file| file.map(|file| file.path()))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort();
        for file in files {
            if file.extension().and_then(|ext| ext.to_str()) != Some("twf") {
                continue;
            }
            let text = std::fs::read_to_string(&file)?;

            if text.starts_with("[flow]") {
                // TODO: Stop reading this file twice
                self.load_function(&file)?;
                continue;
            }

            let mut parser = CodeFunctionParser::new(&text);
            info!("Adding core functions:");
            while let Some(result) = parser
                .parse_next()
                .map_err(|error| into_err(format!("{}: {error}", file.display())))?
            {
                match result {
                    CodeFunctionParseResult::Function(func) => {
                        info!("  - {}", func.name());
                        self.add_function(Function::Code(func));
                    }
                    CodeFunctionParseResult::Alias(alias, target) => {
                        let func = self.get_function(&target).ok_or_else(|| {
                            into_err(format!("Unknown function alias target: {target}"))
                        })?;
                        info!("  - {} (alias of {})", alias, func.borrow().name());
                        let alias_func = FunctionHandle::new(func.borrow().clone());
                        *alias_func.borrow_mut().name_mut() = alias.clone();
                        self.functions.insert(alias, alias_func);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn load_function(&mut self, file_path: &Path) -> Result<String, Box<dyn Error>> {
        let text = std::fs::read_to_string(file_path)?;
        self.load_flow_text(&text)
    }

    pub fn load_flow_text(&mut self, text: &str) -> Result<String, Box<dyn Error>> {
        let func = FlowFunction::load(text, self)
            .ok_or_else(|| into_err("Invalid flow definition or edge endpoints".into()))?;
        let flow_name = func.name().to_owned();
        info!("Loading function: {}", flow_name);
        self.add_function(func);
        Ok(flow_name)
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn add_function(&mut self, func: Function) -> Option<FunctionHandle> {
        self.functions
            .insert(func.name().into(), FunctionHandle::new(func))
    }

    pub fn remove_function(&mut self, name: &str) -> Option<FunctionHandle> {
        self.functions.remove(name)
    }

    pub fn functions_iter(&self) -> hash_map::Values<'_, String, FunctionHandle> {
        self.functions.values()
    }

    pub fn get_function(&self, name: &str) -> Option<FunctionHandle> {
        if let Some((flow_name, _)) = name.split_once(':') {
            self.get_function(flow_name)?.as_flow()?.get_function(name)
        } else {
            self.functions.get(name).cloned()
        }
    }

    pub fn flow_add_node(&mut self, flow_name: &str, node_text: &str) -> (FlowNodeIndex, String) {
        let func = self.get_function(flow_name).unwrap();
        let mut flow = func.as_flow_mut().unwrap();
        let idx = flow.add_node(node_text);
        (idx, flow.get_node(idx).unwrap().name.clone())
    }

    pub fn flow_remove_node(&mut self, flow_name: &str, node_idx: FlowNodeIndex) -> Option<String> {
        let func = self.get_function(flow_name).unwrap();
        let mut flow = func.as_flow_mut().unwrap();
        if let Some(node) = flow.remove_node(node_idx) {
            if node.name.starts_with("$math") {
                self.remove_function(&node.name);
            }
            Some(node.name)
        } else {
            None
        }
    }

    pub fn add_data_array(&mut self, name: &str, array_name: String, data: Vec<f32>) {
        self.data_arrays
            .entry(name.into())
            .or_default()
            .insert(array_name, DataArrayHandle::from(data.as_slice()));
    }

    pub fn get_data_array(&mut self, name: &str, array_name: &str) -> Option<DataArrayHandle> {
        self.data_arrays
            .get(name)
            .and_then(|m| m.get(array_name).cloned())
    }

    pub fn load_wave_file(
        &mut self,
        name: &str,
        buffer_name: &str,
        filepath: &str,
    ) -> Result<(), Box<dyn Error>> {
        let audio = if filepath.is_empty() {
            crate::buffer::DecodedAudio::builtin(buffer_name)?
        } else {
            crate::buffer::DecodedAudio::load(std::path::Path::new(filepath))?
        };
        self.add_data_array(name, buffer_name.into(), audio.samples.to_vec());
        Ok(())
    }

    pub fn unload_wave_file(&mut self, name: &str, buffer_name: &str) {
        self.data_arrays
            .get_mut(name)
            .and_then(|m| m.remove(buffer_name));
    }
}
