use std::path::{Path, PathBuf};

use godot::prelude::*;

use log::info;
use serde::{Deserialize, Serialize};

use twisted_wisp::{
    core::FlowFunction, CallIndex, DataIndex, TwistedWispEngine, TwistedWispEngineConfig,
};

use crate::{logger::GodotLogger, TwistedWispFlow};

#[derive(GodotClass)]
#[class(no_init, base=RefCounted)]
pub struct TwistedWisp {
    base: Base<RefCounted>,
    config: TwistedWispConfig,
    engine: Option<TwistedWispEngine>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TwistedWispConfigFormat {
    wisp: TwistedWispConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwistedWispConfig {
    pub core_path: PathBuf,
    pub data_paths: Vec<PathBuf>,
    pub midi_in_port: Option<String>,
}

impl TwistedWispConfig {
    pub fn resolve_data_path(&self, path: &Path) -> Option<PathBuf> {
        if path.is_absolute() {
            return Some(path.to_owned());
        }
        for data_path in &self.data_paths {
            let full_path = data_path.join(path);
            if full_path.exists() {
                return Some(full_path);
            }
        }
        None
    }
}

#[godot_api]
impl TwistedWisp {
    #[func]
    fn create(config: String) -> Gd<Self> {
        godot::global::godot_print!("TwistedWiSP extension initializing");

        GodotLogger::init().expect("Failed to init the logger");

        info!("Initialized the logger");

        let config = toml::from_str::<TwistedWispConfigFormat>(&config)
            .expect("Failed to parse the config")
            .wisp;
        info!("Loaded the config");

        info!("Initializing server");
        let engine_config = TwistedWispEngineConfig {
            audio_host: None,
            audio_device: None,
            audio_output_channels: None,
            audio_buffer_size: Some(512),
            audio_sample_rate: Some(48000),
            midi_in_port: config.midi_in_port.as_deref(),
            core_path: &config.core_path,
        };
        let engine = TwistedWispEngine::create(engine_config)
            .expect("Failed to create the Twisted WiSP engine");

        info!("TwistedWiSP extension initialized");

        Gd::from_init_fn(|base| Self {
            base,
            config,
            engine: Some(engine),
        })
    }

    pub fn config(&self) -> &TwistedWispConfig {
        &self.config
    }

    pub fn engine(&self) -> &TwistedWispEngine {
        self.engine.as_ref().unwrap()
    }

    pub fn engine_mut(&mut self) -> &mut TwistedWispEngine {
        self.engine.as_mut().unwrap()
    }

    #[func]
    fn start_dsp(&mut self) {
        self.engine_mut().dsp_start();
    }

    #[func]
    fn stop_dsp(&mut self) {
        self.engine_mut().dsp_stop();
    }

    #[func]
    fn create_flow(&mut self) -> Gd<TwistedWispFlow> {
        let ctx = self.ctx_mut();
        let mut name;
        let mut idx = 0;
        loop {
            name = format!("flow_{}", idx);
            if ctx.get_function(&name).is_none() {
                break;
            }
            idx += 1;
        }
        let func = Function::Flow(FlowFunction::new(name.clone()));
        ctx.add_function(func);
        let runner = self.engine_mut();
        runner.context_set_main_function(name.clone());
        TwistedWispFlow::create(self.to_gd(), name)
    }

    #[func]
    fn load_flow_from_file(&mut self, path: String) -> Gd<TwistedWispFlow> {
        let flow_name = self
            .engine_mut()
            .ctx_load_flow_from_file(&path)
            .expect("Failed to load the flow function");
        let flow = self.engine().ctx_get_flow(&flow_name).unwrap();
        let ir_functions = self.engine().flow_get_ir_functions(flow);
        let mut buffers = vec![];
        for (name, path) in self.engine().flow_get_buffers(flow) {
            let full_path = if let Some(path) = path {
                self.config
                    .resolve_data_path(path)
                    .expect("Failed to resolve a data path")
                    .to_str()
                    .unwrap()
                    .to_owned()
            } else {
                // For built-in buffers
                "".to_owned()
            };
            buffers.push((name.clone(), full_path));
        }
        let mut buffer_nodes = vec![];
        for idx in self.engine().flow_get_node_indices(flow) {
            let node = self.engine().flow_get_node(flow, idx).unwrap();
            if let Some(buffer_name) = node.extra_data.get("buffer") {
                buffer_nodes.push((idx, buffer_name.as_string().unwrap().to_owned()));
            }
        }
        let mut value_nodes = vec![];
        for idx in self.engine().flow_get_node_indices(flow) {
            let node = self.engine().flow_get_node(flow, idx).unwrap();
            if let Some(value) = node.extra_data.get("value") {
                value_nodes.push((idx, value.as_float().unwrap()));
            }
        }
        let engine = self.engine_mut();
        engine.context_add_or_update_functions(ir_functions);
        for (name, path) in buffers {
            engine
                .context_load_wave_file(flow_name.clone(), name, path)
                .expect("Failed to load a wave file");
        }
        engine.context_set_main_function(flow_name.clone());
        engine.context_update().expect("Failed to update context");
        for (idx, buffer_name) in buffer_nodes {
            engine.context_set_data_array(
                flow_name.clone(),
                CallIndex(idx.index() as u32),
                DataIndex(0),
                buffer_name,
            );
        }
        for (idx, value) in value_nodes {
            engine.context_set_data_value(
                flow_name.clone(),
                CallIndex(idx.index() as u32),
                DataIndex(0),
                value,
            );
        }
        TwistedWispFlow::create(self.to_gd(), flow_name)
    }

    #[func]
    fn remove_function(&mut self, name: String) {
        self.engine_mut().ctx_remove_function(&name);
    }

    #[func]
    fn list_functions(&mut self) -> Array<GString> {
        let mut array = Array::new();
        for name in self.engine().ctx_list_functions() {
            array.push(name.to_godot());
        }
        array
    }

    #[func]
    fn get_function_metadata(&mut self, name: String) -> Dictionary {
        let func = self.ctx_mut().get_function(&name).unwrap();
        let mut inputs = Array::new();
        for input in func.inputs() {
            inputs.push(input.type_.to_str().to_godot());
        }
        let mut outputs = Array::new();
        for output in func.outputs() {
            outputs.push(output.type_.to_str().to_godot());
        }
        dict! {
            "inlets": inputs,
            "outlets": outputs,
            "is_lag": func.lag_value().is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_config() {
        let config = r#"
            [wisp]
            core_path = "path/to/core"
            data_paths = [
                "path/to/data1",
                "path/to/data2",
            ]
        "#;
        let config = toml::from_str::<TwistedWispConfigFormat>(config)
            .unwrap()
            .wisp;
        assert_eq!(PathBuf::from("path/to/core"), config.core_path);
        assert_eq!(
            vec![
                PathBuf::from("path/to/data1"),
                PathBuf::from("path/to/data2")
            ],
            config.data_paths
        );
    }
}
