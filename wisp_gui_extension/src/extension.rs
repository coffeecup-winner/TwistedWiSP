use std::path::Path;

use godot::{engine::Engine, prelude::*};

use log::info;
use twisted_wisp::{FlowFunction, WispContext, WispFunction};
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{DataIndex, WispRunnerClient};

use crate::logger::GodotLogger;

struct TwistedWispExtension;

#[gdextension]
unsafe impl ExtensionLibrary for TwistedWispExtension {
    fn on_level_init(level: InitLevel) {
        if level == InitLevel::Scene {
            Engine::singleton().register_singleton(
                "TwistedWisp".into(),
                TwistedWispSingleton::new_alloc().upcast(),
            )
        }
    }

    fn on_level_deinit(level: InitLevel) {
        if level == InitLevel::Scene {
            let mut engine = Engine::singleton();
            let name = StringName::from("TwistedWisp");

            let singleton = engine
                .get_singleton(name.clone())
                .expect("Failed to find the TwistedWisp singleton");
            engine.unregister_singleton(name);
            singleton.free();
        }
    }
}

#[derive(GodotClass)]
#[class(no_init)]
struct FlowNodeAddResult {
    #[var]
    idx: u32,
    #[var]
    name: GString,
}

#[derive(GodotClass)]
#[class(init, base=Object)]
struct TwistedWispSingleton {
    base: Base<Object>,
    runner: Option<WispRunnerClient>,
    ctx: Option<WispContext>,
}

#[godot_api]
impl TwistedWispSingleton {
    #[func]
    fn init(&mut self, wisp_exe_path: String, wisp_core_path: String) {
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
            runner.context_add_or_update_function(f.get_ir_function(&ctx));
        }

        self.runner = Some(runner);
        self.ctx = Some(ctx);

        info!("TwistedWiSP extension initialized");
    }

    fn runner_mut(&mut self) -> &mut WispRunnerClient {
        self.runner.as_mut().unwrap()
    }

    fn ctx(&self) -> &WispContext {
        self.ctx.as_ref().unwrap()
    }

    fn ctx_mut(&mut self) -> &mut WispContext {
        self.ctx.as_mut().unwrap()
    }

    #[func]
    fn dsp_start(&mut self) {
        godot::log::godot_print!("enable_dsp");
        self.runner_mut().dsp_start();
    }

    #[func]
    fn dsp_stop(&mut self) {
        godot::log::godot_print!("disable_dsp");
        self.runner_mut().dsp_stop();
    }

    #[func]
    fn function_create(&mut self) -> String {
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
        name
    }

    #[func]
    fn function_remove(&mut self, name: String) {
        // TODO: Handle this on the runner side
        self.ctx_mut().remove_function(&name);
    }

    #[func]
    fn function_list(&mut self) -> Array<GString> {
        let mut array = Array::new();
        for f in self.ctx_mut().functions_iter() {
            array.push(f.name().into());
        }
        array
    }

    #[func]
    fn function_get_metadata(&mut self, name: String) -> Dictionary {
        let func = self.ctx_mut().get_function(&name).unwrap();
        let mut inputs = Array::new();
        for i in 0..func.inputs_count() {
            inputs.push(func.input(i).unwrap().type_.to_str().into_godot());
        }
        let mut outputs = Array::new();
        for i in 0..func.outputs_count() {
            outputs.push(func.output(i).unwrap().type_.to_str().into_godot());
        }
        dict! {
            "inlets": inputs,
            "outlets": outputs,
        }
    }

    #[func]
    fn function_set_main(&mut self, name: String) {
        self.runner_mut().context_set_main_function(name);
    }

    #[func]
    fn function_open(&mut self, path: String) -> String {
        let result = self
            .ctx_mut()
            .load_function(&path)
            .expect("Failed to load the flow function");
        let ctx = self.ctx();
        let mut ir_functions = vec![];
        for name in result.math_function_names {
            let math_func = ctx.get_function(&name).unwrap();
            ir_functions.push(math_func.get_ir_function(ctx));
        }
        ir_functions.push(ctx.get_function(&result.name).unwrap().get_ir_function(ctx));
        let runner = self.runner_mut();
        for f in ir_functions {
            runner.context_add_or_update_function(f);
        }
        if result.replaced_existing {
            runner.context_update();
        }
        result.name
    }

    #[func]
    fn function_save(&mut self, name: String, path: String) {
        let func = self.ctx_mut().get_function(&name).unwrap();
        let s = func.save();
        std::fs::write(Path::new(&path), s.as_bytes())
            .expect("Failed to save flow function to file");
    }

    #[func]
    fn flow_list_nodes(&mut self, flow_name: String) -> Array<u32> {
        let mut array = Array::new();
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        for idx in flow.node_indices() {
            array.push(idx.index() as u32);
        }
        array
    }

    #[func]
    pub fn flow_add_node(
        &mut self,
        flow_name: String,
        func_text: String,
    ) -> Option<Gd<FlowNodeAddResult>> {
        let ctx = self.ctx_mut();
        let (idx, func_name) = ctx.flow_add_node(&flow_name, &func_text);
        if func_name.starts_with("$math") {
            let func = ctx.get_function(&func_name).unwrap();
            let ir_function = func.get_ir_function(ctx);
            self.runner_mut()
                .context_add_or_update_function(ir_function);
        }
        Some(Gd::from_object(FlowNodeAddResult {
            idx: idx.index() as u32,
            name: func_name.into(),
        }))
    }

    #[func]
    pub fn flow_add_watch(&mut self, flow_name: String, idx: u32) {
        // TODO: Maybe remove this and do flow borrow checking at runtime?
        let ctx = self.ctx();
        let flow = ctx
            .get_function(&flow_name)
            .and_then(|f| f.as_flow())
            .unwrap();
        let ir_function = flow.get_ir_function(ctx);
        let runner = self.runner_mut();
        // NOTE: We do not update the watch function as we expect it to never change
        // at runtime and it's a part of the core library
        runner.context_add_or_update_function(ir_function);
        runner.context_update();
        let watch_idx = runner
            .context_watch_data_value(flow_name.clone(), CallId(idx), DataIndex(0))
            .expect("Failed to watch a data value");
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.add_watch_idx(idx.into(), watch_idx.0);
    }

    #[func]
    fn flow_remove_node(&mut self, flow_name: String, node_idx: u32) {
        let ctx = self.ctx_mut();
        let node_name = ctx
            .flow_remove_node(&flow_name, node_idx.into())
            .expect("Failed to remove node");
        // Not removing watches here since they will automaticaly be removed
        // during the data layout update and will stop being sent
        let flow = ctx.get_function(&flow_name).unwrap();
        let ir_function = flow.get_ir_function(ctx);
        let runner = self.runner_mut();
        if node_name.starts_with("$math") {
            runner.context_remove_function(node_name);
        }
        runner.context_add_or_update_function(ir_function);
        runner.context_update();
    }

    #[func]
    fn flow_get_node_name(&mut self, flow_name: String, node_idx: u32) -> String {
        let flow = self
            .ctx()
            .get_function(&flow_name)
            .and_then(|f| f.as_flow())
            .unwrap();
        flow.get_node(node_idx.into()).unwrap().name.clone()
    }

    #[func]
    fn flow_get_node_display_name(&mut self, flow_name: String, node_idx: u32) -> String {
        let flow = self
            .ctx()
            .get_function(&flow_name)
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(node_idx.into()).unwrap();
        node.display_text.clone()
    }

    #[func]
    fn flow_get_node_coordinates(&mut self, flow_name: String, node_idx: u32) -> Dictionary {
        let flow = self
            .ctx()
            .get_function(&flow_name)
            .and_then(|f| f.as_flow())
            .unwrap();
        let data = &flow.get_node(node_idx.into()).unwrap().data;
        dict! {
            "x": data.x,
            "y": data.y,
            "w": data.w,
            "h": data.h,
        }
    }

    #[func]
    fn flow_set_node_coordinates(
        &mut self,
        flow_name: String,
        node_idx: u32,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    ) {
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let data = &mut flow.get_node_mut(node_idx.into()).unwrap().data;
        data.x = x;
        data.y = y;
        data.w = w;
        data.h = h;
    }

    #[func]
    fn flow_list_connections(&mut self, flow_name: String) -> Array<u32> {
        let mut array = Array::new();
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        for idx in flow.edge_indices() {
            array.push(idx.index() as u32);
        }
        array
    }

    #[func]
    fn flow_get_connection(&mut self, flow_name: String, conn_idx: u32) -> Dictionary {
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let (from, to, conn) = flow.get_connection(conn_idx.into()).unwrap();
        dict! {
            "from": from.index() as u32,
            "output_index": conn.output_index,
            "to": to.index() as u32,
            "input_index": conn.input_index,
        }
    }

    #[func]
    fn flow_connect(
        &mut self,
        flow_name: String,
        node_out: u32,
        node_outlet: u32,
        node_in: u32,
        node_inlet: u32,
    ) {
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.connect(node_out.into(), node_outlet, node_in.into(), node_inlet);
        let ctx = self.ctx();
        let func = ctx.get_function(&flow_name).unwrap();
        let ir_function = func.get_ir_function(ctx);
        let runner = self.runner_mut();
        runner.context_add_or_update_function(ir_function);
        runner.context_update();
    }

    #[func]
    fn flow_disconnect(
        &mut self,
        flow_name: String,
        node_out: u32,
        node_outlet: u32,
        node_in: u32,
        node_inlet: u32,
    ) {
        let flow = self
            .ctx_mut()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.disconnect(node_out.into(), node_outlet, node_in.into(), node_inlet);
        let ctx = self.ctx();
        let func = ctx.get_function(&flow_name).unwrap();
        let ir_function = func.get_ir_function(ctx);
        let runner = self.runner_mut();
        runner.context_add_or_update_function(ir_function);
        runner.context_update();
    }

    #[func]
    fn flow_node_set_buffer(&mut self, flow_name: String, node_idx: u32, name: String) {
        self.runner_mut()
            .context_set_data_array(flow_name, CallId(node_idx), DataIndex(0), name);
    }

    #[func]
    fn flow_node_on_value_changed(&mut self, flow_name: String, node_idx: u32, value: f32) {
        self.runner_mut()
            .context_set_data_value(flow_name, CallId(node_idx), DataIndex(0), value);
    }

    #[func]
    fn flow_get_watch_updates(&mut self, flow_name: String) -> Dictionary {
        let watches = self.runner_mut().context_query_watched_data_values();
        let flow = self
            .ctx()
            .get_function(&flow_name)
            .and_then(|f| f.as_flow())
            .unwrap();
        let mut updates = Dictionary::new();
        for (idx, values) in watches.values {
            let mut array = Array::new();
            for value in values {
                array.push(value);
            }
            if !array.is_empty() {
                updates.insert(flow.watch_idx_to_node_idx(idx.0).index() as u32, array);
            }
        }
        updates
    }
}
