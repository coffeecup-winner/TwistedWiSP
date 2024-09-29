use std::{
    collections::{hash_map, HashMap},
    error::Error,
    io::BufReader,
};

use inkwell::context::Context;
use log::info;
use string_error::into_err;

use crate::{compiler::DataArrayHandle, ir::IRFunction};

pub struct WispExecutionContext {
    context: Context,
}

impl WispExecutionContext {
    pub fn init() -> Self {
        WispExecutionContext {
            context: Context::create(),
        }
    }

    pub fn llvm(&self) -> &Context {
        &self.context
    }
}

#[derive(Debug, Clone)]
struct WispDataArray {
    #[allow(dead_code)] // Only read by the JIT-compiled code
    data: Vec<f32>,
    array: DataArrayHandle,
}

#[derive(Debug, Default)]
pub struct WispEngineContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, IRFunction>,
    main_function: String,
    data_arrays: HashMap<String, HashMap<String, WispDataArray>>,
}

impl WispEngineContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispEngineContext {
            num_outputs,
            sample_rate,
            ..Default::default()
        }
    }

    pub fn reset(&mut self) {
        self.functions.clear();
        self.main_function = String::new();
    }

    pub fn num_outputs(&self) -> u32 {
        self.num_outputs
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn add_function(&mut self, func: IRFunction) {
        self.functions.insert(func.name().into(), func);
    }

    pub fn remove_function(&mut self, name: &str) -> Option<IRFunction> {
        self.functions.remove(name)
    }

    pub fn get_function(&self, name: &str) -> Option<&IRFunction> {
        self.functions.get(name)
    }

    pub fn functions_iter(&self) -> hash_map::Iter<'_, String, IRFunction> {
        self.functions.iter()
    }

    pub fn set_main_function(&mut self, name: &str) {
        self.main_function = name.into();
    }

    pub fn main_function(&self) -> &str {
        &self.main_function
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
