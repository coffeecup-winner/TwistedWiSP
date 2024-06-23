use std::path::{Path, PathBuf};

use godot::prelude::*;

use log::info;
use serde::{Deserialize, Serialize};
use twisted_wisp::{FlowFunction, WispContext, WispFunction};
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{DataIndex, WispRunnerClient};

use crate::{logger::GodotLogger, TwistedWispFlow};

#[derive(GodotClass)]
#[class(no_init, base=RefCounted)]
pub struct TwistedWisp {
    base: Base<RefCounted>,
    config: TwistedWispConfig,
    runner: Option<WispRunnerClient>,
    ctx: Option<WispContext>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TwistedWispConfigFormat {
    wisp: TwistedWispConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TwistedWispConfig {
    pub executable_path: PathBuf,
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
        godot::log::godot_print!("TwistedWiSP extension initializing");

        GodotLogger::init().expect("Failed to init the logger");

        info!("Initialized the logger");

        let config = toml::from_str::<TwistedWispConfigFormat>(&config)
            .expect("Failed to parse the config")
            .wisp;
        info!("Loaded the config");

        info!("Initializing server: {:?}", config.executable_path);
        let mut runner = WispRunnerClient::init(
            &config.executable_path,
            Some(512),
            Some(48000),
            config.midi_in_port.as_deref(),
        );
        let sys_info = runner.get_system_info();

        let mut ctx = WispContext::new(sys_info.num_channels);
        ctx.add_builtin_functions();
        ctx.load_core_functions(&config.core_path)
            .expect("Failed to load core functions");

        for f in ctx.functions_iter() {
            runner.context_add_or_update_functions(f.get_ir_functions(&ctx));
        }

        info!("TwistedWiSP extension initialized");

        Gd::from_init_fn(|base| Self {
            base,
            config,
            runner: Some(runner),
            ctx: Some(ctx),
        })
    }

    pub fn config(&self) -> &TwistedWispConfig {
        &self.config
    }

    pub fn runner_mut(&mut self) -> &mut WispRunnerClient {
        self.runner.as_mut().unwrap()
    }

    pub fn ctx(&self) -> &WispContext {
        self.ctx.as_ref().unwrap()
    }

    pub fn ctx_mut(&mut self) -> &mut WispContext {
        self.ctx.as_mut().unwrap()
    }

    #[func]
    fn start_dsp(&mut self) {
        self.runner_mut().dsp_start();
    }

    #[func]
    fn stop_dsp(&mut self) {
        self.runner_mut().dsp_stop();
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
        let func = Box::new(FlowFunction::new(name.clone()));
        ctx.add_function(func);
        let runner = self.runner_mut();
        runner.context_set_main_function(name.clone());
        TwistedWispFlow::create(self.to_gd(), name)
    }

    #[func]
    fn load_flow_from_file(&mut self, path: String) -> Gd<TwistedWispFlow> {
        let flow_name = self
            .ctx_mut()
            .load_function(&PathBuf::from(path))
            .expect("Failed to load the flow function");
        let ctx = self.ctx();
        let flow = ctx.get_function(&flow_name).unwrap().as_flow().unwrap();
        let ir_functions = flow.get_ir_functions(ctx);
        let mut buffers = vec![];
        for (name, path) in flow.buffers() {
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
        for idx in flow.node_indices() {
            let node = flow.get_node(idx).unwrap();
            if let Some(buffer_name) = node.extra_data.get("buffer") {
                buffer_nodes.push((idx, buffer_name.as_string().unwrap().to_owned()));
            }
        }
        let mut value_nodes = vec![];
        for idx in flow.node_indices() {
            let node = flow.get_node(idx).unwrap();
            if let Some(value) = node.extra_data.get("value") {
                value_nodes.push((idx, value.as_number().unwrap()));
            }
        }
        let runner = self.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        for (name, path) in buffers {
            runner.context_load_wave_file(flow_name.clone(), name, path);
        }
        runner.context_set_main_function(flow_name.clone());
        runner.context_update();
        for (idx, buffer_name) in buffer_nodes {
            runner.context_set_data_array(
                flow_name.clone(),
                CallId(idx.index() as u32),
                DataIndex(0),
                buffer_name,
            );
        }
        for (idx, value) in value_nodes {
            runner.context_set_data_value(
                flow_name.clone(),
                CallId(idx.index() as u32),
                DataIndex(0),
                value,
            );
        }
        TwistedWispFlow::create(self.to_gd(), flow_name)
    }

    #[func]
    fn remove_function(&mut self, name: String) {
        // TODO: Handle this on the runner side
        self.ctx_mut().remove_function(&name);
    }

    #[func]
    fn list_functions(&mut self) -> Array<GString> {
        let mut array = Array::new();
        for f in self.ctx_mut().functions_iter() {
            array.push(f.name().into());
        }
        array
    }

    #[func]
    fn get_function_metadata(&mut self, name: String) -> Dictionary {
        let func = self.ctx_mut().get_function(&name).unwrap();
        let mut inputs = Array::new();
        for input in func.inputs() {
            inputs.push(input.type_.to_str().into_godot());
        }
        let mut outputs = Array::new();
        for output in func.outputs() {
            outputs.push(output.type_.to_str().into_godot());
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
            executable_path = "path/to/executable"
            core_path = "path/to/core"
            data_paths = [
                "path/to/data1",
                "path/to/data2",
            ]
        "#;
        let config = toml::from_str::<TwistedWispConfigFormat>(config)
            .unwrap()
            .wisp;
        assert_eq!(PathBuf::from("path/to/executable"), config.executable_path);
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
