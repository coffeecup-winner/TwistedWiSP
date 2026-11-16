use std::{
    collections::{hash_map, HashMap},
    error::Error,
    io::BufReader,
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

use super::Function;

#[derive(Debug, Clone)]
struct WispDataArray {
    #[allow(dead_code)] // Only read by the JIT-compiled code
    data: Vec<f32>,
    array: DataArrayHandle,
}

#[derive(Debug)]
pub struct WispContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, Function>,
    main_function: Option<String>,
    data_arrays: HashMap<String, HashMap<String, WispDataArray>>,
}

impl WispContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispContext {
            num_outputs,
            sample_rate,
            functions: HashMap::new(),
            main_function: None,
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
                        self.add_function(Function::Code(func));
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

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn add_function(&mut self, func: Function) -> Option<Function> {
        self.functions.insert(func.name().into(), func)
    }

    pub fn remove_function(&mut self, name: &str) -> Option<Function> {
        self.functions.remove(name)
    }

    pub fn functions_iter(&self) -> hash_map::Values<'_, String, Function> {
        self.functions.values()
    }

    pub fn get_function(&self, name: &str) -> Option<&Function> {
        if let Some((flow_name, _)) = name.split_once(':') {
            self.get_function(flow_name)
                .and_then(|f| f.as_flow())
                .and_then(|f| f.get_function(name))
        } else {
            self.functions.get(name)
        }
    }

    pub fn get_function_mut(&mut self, name: &str) -> Option<&mut Function> {
        if let Some((flow_name, _)) = name.split_once(':') {
            self.get_function_mut(flow_name)
                .and_then(|f| f.as_flow_mut())
                .and_then(|f| f.get_function_mut(name))
        } else {
            self.functions.get_mut(name)
        }
    }

    pub fn set_main_function(&mut self, name: &str) {
        self.main_function = Some(name.into());
    }

    pub fn main_function(&self) -> Option<&str> {
        self.main_function.as_deref()
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

    pub fn add_data_array(&mut self, name: &str, array_name: String, mut data: Vec<f32>) {
        data.insert(0, f32::from_bits(data.len() as u32));
        let array = DataArrayHandle::from(&data[..]);
        self.data_arrays
            .entry(name.into())
            .or_default()
            .insert(array_name, WispDataArray { data, array });
    }

    pub fn get_data_array(&mut self, name: &str, array_name: &str) -> Option<DataArrayHandle> {
        self.data_arrays
            .get(name)
            .and_then(|m| m.get(array_name).map(|a| a.array))
    }

    pub fn load_wave_file(
        &mut self,
        name: &str,
        buffer_name: &str,
        filepath: &str,
    ) -> Result<(), Box<dyn Error>> {
        let data = if filepath.is_empty() {
            Self::get_builtin_data_array(buffer_name)
                .ok_or(into_err(format!("Unknown buffer: {}", buffer_name)))?
        } else {
            let wav = hound::WavReader::open(filepath)?;
            info!("Loaded wave file {}:\n{:?}", buffer_name, wav.spec());
            let data = Self::convert_wav_samples(wav);
            Self::mix_to_mono(&data)
        };
        self.add_data_array(name, buffer_name.into(), data);
        Ok(())
    }

    fn get_builtin_data_array(name: &str) -> Option<Vec<f32>> {
        match name {
            "sine" => {
                const LENGTH: usize = 1024;
                let mut data = vec![0.0; LENGTH];
                const STEP: f32 = 2.0 * std::f32::consts::PI / (LENGTH as f32);
                for (i, value) in data.iter_mut().enumerate() {
                    *value = (i as f32 * STEP).sin();
                }
                Some(data)
            }
            _ => None,
        }
    }

    fn convert_wav_samples(wav: hound::WavReader<BufReader<std::fs::File>>) -> Vec<f32> {
        let spec = wav.spec();
        if spec.sample_format == hound::SampleFormat::Float {
            return wav.into_samples::<f32>().map(|s| s.unwrap()).collect();
        }
        let mut data = Vec::new();
        if spec.bits_per_sample > 16 {
            let samples = wav.into_samples::<i32>();
            let max = (1i32 << (spec.bits_per_sample - 1)) as f32;
            for sample in samples {
                data.push(sample.unwrap() as f32 / max);
            }
        } else if spec.bits_per_sample > 8 {
            let samples = wav.into_samples::<i16>();
            let max = (1i16 << (spec.bits_per_sample - 1)) as f32;
            for sample in samples {
                data.push(sample.unwrap() as f32 / max);
            }
        } else {
            let samples = wav.into_samples::<i8>();
            let max = (1i8 << (spec.bits_per_sample - 1)) as f32;
            for sample in samples {
                data.push(sample.unwrap() as f32 / max);
            }
        }
        data
    }

    // TODO: Ideally provide several channels from the buffer node instead
    fn mix_to_mono(data: &[f32]) -> Vec<f32> {
        let mut i = 0;
        let mut result = Vec::with_capacity(data.len() / 2);
        while i < data.len() {
            result.push((data[i] + data[i + 1]) * 0.5);
            i += 2;
        }
        result
    }

    pub fn unload_wave_file(&mut self, name: &str, buffer_name: &str) {
        self.data_arrays
            .get_mut(name)
            .and_then(|m| m.remove(buffer_name));
    }
}
