use std::path::{Path, PathBuf};

use godot::prelude::*;

use log::info;
use twisted_wisp::{FlowFunction, WispContext};
use twisted_wisp_protocol::WispRunnerClient;

use crate::{logger::GodotLogger, TwistedWispFlow};

#[derive(GodotClass)]
#[class(no_init, base=RefCounted)]
pub struct TwistedWisp {
    base: Base<RefCounted>,
    core_path: PathBuf,
    runner: Option<WispRunnerClient>,
    ctx: Option<WispContext>,
}

#[godot_api]
impl TwistedWisp {
    #[func]
    fn create(wisp_exe_path: String, wisp_core_path: String) -> Gd<Self> {
        godot::log::godot_print!("TwistedWiSP extension initializing");

        GodotLogger::init().expect("Failed to init the logger");

        info!("TwistedWiSP logger initialized");

        info!("Initializing server: {}", wisp_exe_path);
        let mut runner = WispRunnerClient::init(Path::new(&wisp_exe_path), Some(512), Some(48000));
        let sys_info = runner.get_system_info();

        let mut ctx = WispContext::new(sys_info.num_channels);
        ctx.add_builtin_functions();
        ctx.load_core_functions(&wisp_core_path)
            .expect("Failed to load core functions");

        for f in ctx.functions_iter() {
            runner.context_add_or_update_functions(f.get_ir_functions(&ctx));
        }

        info!("TwistedWiSP extension initialized");

        Gd::from_init_fn(|base| Self {
            base,
            core_path: PathBuf::from(wisp_core_path),
            runner: Some(runner),
            ctx: Some(ctx),
        })
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
    fn load_wave_file(&mut self, name: String, filepath: String) {
        let mut path = PathBuf::from(filepath);
        if !path.is_absolute() {
            path = self.core_path.join(path);
        }
        self.runner_mut()
            .context_load_wave_file(name, path.to_str().unwrap().to_owned());
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
        TwistedWispFlow::create(self.to_gd(), name)
    }

    #[func]
    fn load_flow_from_file(&mut self, path: String) -> Gd<TwistedWispFlow> {
        let result = self
            .ctx_mut()
            .load_function(&path)
            .expect("Failed to load the flow function");
        let ctx = self.ctx();
        let ir_functions = ctx
            .get_function(&result.name)
            .unwrap()
            .get_ir_functions(ctx);
        let runner = self.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        runner.context_set_main_function(result.name.clone());
        if result.replaced_existing {
            runner.context_update();
        }
        TwistedWispFlow::create(self.to_gd(), result.name)
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
