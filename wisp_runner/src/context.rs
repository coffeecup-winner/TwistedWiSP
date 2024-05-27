use std::{
    collections::{hash_map, HashMap},
    error::Error,
    io::BufReader,
};

use inkwell::context::Context;
use log::info;
use twisted_wisp_ir::IRFunction;

use crate::compiler::DataArray;

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
pub struct WispDataArray {
    pub data: Vec<f32>,
    pub array: *mut DataArray,
}

#[derive(Debug, Default)]
pub struct WispContext {
    num_outputs: u32,
    sample_rate: u32,
    functions: HashMap<String, IRFunction>,
    main_function: String,
    data_arrays: HashMap<String, WispDataArray>,
}

impl WispContext {
    pub fn new(num_outputs: u32, sample_rate: u32) -> Self {
        WispContext {
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

    pub fn add_builtin_data_arrays(&mut self) {
        const LENGTH: usize = 1024;
        let mut data = vec![0.0; LENGTH];
        const STEP: f32 = 2.0 * std::f32::consts::PI / (LENGTH as f32);
        for (i, value) in data.iter_mut().enumerate() {
            *value = (i as f32 * STEP).sin();
        }
        self.add_data_array("sine".into(), data);
    }

    pub fn add_data_array(&mut self, array_name: String, mut data: Vec<f32>) {
        data.insert(0, f32::from_bits(data.len() as u32));
        let array = data.as_mut_ptr() as *mut DataArray;
        self.data_arrays
            .insert(array_name, WispDataArray { data, array });
    }

    pub fn get_data_array(&mut self, array_name: &str) -> Option<*mut DataArray> {
        self.data_arrays.get_mut(array_name).map(|a| a.array)
    }

    pub fn load_wave_file(&mut self, name: &str, filepath: &str) -> Result<(), Box<dyn Error>> {
        let wav = hound::WavReader::open(filepath)?;
        info!("Loaded wave file {}:\n{:?}", name, wav.spec());
        let data = Self::convert_wav_samples(wav);
        let data = Self::mix_to_mono(&data);
        self.add_data_array(name.into(), data);
        Ok(())
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

    pub fn unload_wave_file(&mut self, name: &str) {
        self.data_arrays.remove(name);
    }
}
