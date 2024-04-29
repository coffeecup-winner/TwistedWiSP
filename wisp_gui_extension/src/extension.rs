use std::path::Path;

use godot::{engine::Engine, prelude::*};

use twisted_wisp::{
    CodeFunctionParser, FlowFunction, MathFunctionParser, WispContext, WispFunction,
};
use twisted_wisp_protocol::WispRunnerClient;

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
        godot::log::godot_print!("init: {}", wisp_exe_path);

        let mut runner = WispRunnerClient::init(Path::new(&wisp_exe_path));
        let sys_info = runner.get_system_info();

        let mut ctx = WispContext::new(sys_info.num_channels);
        ctx.add_builtin_functions();

        for file in std::fs::read_dir(Path::new(&wisp_core_path)).expect("Failed to open core path")
        {
            let path = file.unwrap().path();
            let text = std::fs::read_to_string(path).expect("Failed to read file");
            let mut parser = CodeFunctionParser::new(&text);
            godot_print!("Adding core functions:");
            while let Some(func) = parser.parse_function() {
                godot_print!("  - {}", func.name());
                ctx.add_function(Box::new(func));
            }
        }

        for f in ctx.functions_iter() {
            runner.context_add_or_update_function(f.get_ir_function(&ctx));
        }

        self.runner = Some(runner);
        self.ctx = Some(ctx);
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
        dict! {
            "num_inlets": func.inputs_count(),
            "num_outlets": func.outputs_count(),
        }
    }

    #[func]
    fn function_set_main(&mut self, name: String) {
        self.runner_mut().context_set_main_function(name);
    }

    #[func]
    fn function_open(&mut self, path: String) -> String {
        let s = std::fs::read_to_string(Path::new(&path)).expect("Failed to open file to load");
        let mut func = FlowFunction::load(&s).expect("Failed to parse the flow function data");
        let ctx = self.ctx_mut();
        let flow_name = func.name().to_owned();
        let flow = func.as_flow_mut().unwrap();
        let mut ir_functions = vec![];
        for n in flow.node_indices() {
            let node = flow.get_node(n).unwrap();
            if let Some(text) = &node.expr {
                let mut parts = node.name.split('$');
                let id = parts.nth(2).unwrap().parse::<u32>().unwrap();
                let math_func = Box::new(
                    MathFunctionParser::parse_function(&flow_name, id, text.clone()).unwrap(),
                );
                ir_functions.push(math_func.get_ir_function(ctx));
                ctx.add_function(math_func);
            }
        }
        ir_functions.push(func.get_ir_function(ctx));
        let runner = self.runner_mut();
        for f in ir_functions {
            runner.context_add_or_update_function(f);
        }
        let ctx = self.ctx_mut();
        if let Some(f) = ctx.get_function_mut(func.name()) {
            *f = func;
            self.runner_mut().context_update();
        } else {
            ctx.add_function(func);
        }
        flow_name
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
    fn flow_add_node(&mut self, flow_name: String, func_text: String) -> Dictionary {
        let ctx = self.ctx_mut();
        let flow = ctx
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let (idx, func_name) = if func_text.starts_with('=') {
            let id = flow.next_math_function_id();
            let func = Box::new(
                MathFunctionParser::parse_function(&flow_name, id, func_text.clone()).unwrap(),
            );
            let idx = flow.add_node(func.name().into(), Some(func_text.clone()));
            let func_name = func.name().to_owned();
            let ir_function = func.get_ir_function(ctx);
            ctx.add_function(func);
            self.runner_mut()
                .context_add_or_update_function(ir_function);
            (idx, func_name)
        } else {
            (flow.add_node(func_text.clone(), None), func_text.clone())
        };
        dict! {
            "idx": idx.index() as u32,
            "name": func_name,
            "display_name": func_text,
        }
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
        if let Some(expr) = &node.expr {
            expr.clone()
        } else {
            node.name.clone()
        }
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
}
