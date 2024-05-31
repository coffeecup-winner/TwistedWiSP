use std::path::Path;

use godot::prelude::*;

use twisted_wisp::WispFunction;
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::DataIndex;

use crate::TwistedWisp;

#[derive(GodotClass)]
#[class(no_init)]
struct FlowNodeAddResult {
    #[var]
    idx: u32,
    #[var]
    name: GString,
}

#[derive(GodotClass)]
#[class(no_init)]
pub struct TwistedWispFlow {
    base: Base<RefCounted>,
    name: String,
}

#[godot_api]
impl TwistedWispFlow {
    pub fn create(name: String) -> Gd<Self> {
        Gd::from_init_fn(|base| Self { base, name })
    }

    #[func]
    fn save_to_file(&self, mut wisp: Gd<TwistedWisp>, path: String) {
        let mut wisp = wisp.bind_mut();
        let func = wisp.ctx_mut().get_function(&self.name).unwrap();
        let s = func.save();
        std::fs::write(Path::new(&path), s.as_bytes())
            .expect("Failed to save flow function to file");
    }

    #[func]
    fn add_node(
        &self,
        mut wisp: Gd<TwistedWisp>,
        func_text: String,
    ) -> Option<Gd<FlowNodeAddResult>> {
        let mut wisp = wisp.bind_mut();
        let ctx = wisp.ctx_mut();
        let (idx, func_name) = ctx.flow_add_node(&self.name, &func_text);
        if func_name.starts_with("$math") {
            let func = ctx.get_function(&func_name).unwrap();
            let ir_functions = func.get_ir_functions(ctx);
            wisp.runner_mut()
                .context_add_or_update_functions(ir_functions);
        }
        Some(Gd::from_object(FlowNodeAddResult {
            idx: idx.index() as u32,
            name: func_name.into(),
        }))
    }

    #[func]
    fn remove_node(&self, mut wisp: Gd<TwistedWisp>, node_idx: u32) {
        let mut wisp = wisp.bind_mut();
        let ctx = wisp.ctx_mut();
        let node_name = ctx
            .flow_remove_node(&self.name, node_idx.into())
            .expect("Failed to remove node");
        // Not removing watches here since they will automaticaly be removed
        // during the data layout update and will stop being sent
        let flow = ctx.get_function(&self.name).unwrap();
        let ir_functions = flow.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        if node_name.starts_with("$math") {
            runner.context_remove_function(node_name);
        }
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
    }

    #[func]
    fn list_nodes(&self, wisp: Gd<TwistedWisp>) -> Array<u32> {
        let wisp = wisp.bind();
        let mut array = Array::new();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        for idx in flow.node_indices() {
            array.push(idx.index() as u32);
        }
        array
    }

    #[func]
    fn get_node_name(&self, wisp: Gd<TwistedWisp>, node_idx: u32) -> String {
        let wisp = wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        flow.get_node(node_idx.into()).unwrap().name.clone()
    }

    #[func]
    fn get_node_display_name(&self, wisp: Gd<TwistedWisp>, node_idx: u32) -> String {
        let wisp = wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(node_idx.into()).unwrap();
        node.display_text.clone()
    }

    #[func]
    fn get_node_coordinates(&self, wisp: Gd<TwistedWisp>, node_idx: u32) -> Dictionary {
        let wisp = wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
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
    fn set_node_coordinates(
        &self,
        mut wisp: Gd<TwistedWisp>,
        node_idx: u32,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
    ) {
        let mut wisp = wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let data = &mut flow.get_node_mut(node_idx.into()).unwrap().data;
        data.x = x;
        data.y = y;
        data.w = w;
        data.h = h;
    }

    #[func]
    fn connect_nodes(
        &self,
        mut wisp: Gd<TwistedWisp>,
        node_out: u32,
        node_outlet: u32,
        node_in: u32,
        node_inlet: u32,
    ) {
        let mut wisp = wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .expect("Failed to get flow function");
        flow.connect(node_out.into(), node_outlet, node_in.into(), node_inlet);
        let ctx = wisp.ctx();
        let func = ctx.get_function(&self.name).unwrap();
        let ir_functions = func.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
    }

    #[func]
    fn disconnect_nodes(
        &self,
        mut wisp: Gd<TwistedWisp>,
        node_out: u32,
        node_outlet: u32,
        node_in: u32,
        node_inlet: u32,
    ) {
        let mut wisp = wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.disconnect(node_out.into(), node_outlet, node_in.into(), node_inlet);
        let ctx = wisp.ctx();
        let func = ctx.get_function(&self.name).unwrap();
        let ir_functions = func.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
    }

    #[func]
    fn list_connections(&self, wisp: Gd<TwistedWisp>) -> Array<u32> {
        let wisp = wisp.bind();
        let mut array = Array::new();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        for idx in flow.edge_indices() {
            array.push(idx.index() as u32);
        }
        array
    }

    #[func]
    fn get_connection(&self, wisp: Gd<TwistedWisp>, conn_idx: u32) -> Dictionary {
        let wisp = wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
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
    fn add_watch(&self, mut wisp: Gd<TwistedWisp>, idx: u32) {
        let mut wisp = wisp.bind_mut();
        // TODO: Maybe remove this and do flow borrow checking at runtime?
        let ctx = wisp.ctx();
        let flow = ctx
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        let ir_functions = flow.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        // NOTE: We do not update the watch function as we expect it to never change
        // at runtime and it's a part of the core library
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
        let watch_idx = runner
            .context_watch_data_value(self.name.clone(), CallId(idx), DataIndex(0))
            .expect("Failed to watch a data value");
        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.add_watch_idx(idx.into(), watch_idx.0);
    }

    #[func]
    fn get_watch_updates(&self, mut wisp: Gd<TwistedWisp>) -> Dictionary {
        let mut wisp = wisp.bind_mut();
        let watches = wisp.runner_mut().context_query_watched_data_values();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
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

    #[func]
    fn set_node_buffer(&self, mut wisp: Gd<TwistedWisp>, node_idx: u32, name: String) {
        let mut wisp = wisp.bind_mut();
        wisp.runner_mut().context_set_data_array(
            self.name.clone(),
            CallId(node_idx),
            DataIndex(0),
            name,
        );
    }

    #[func]
    fn set_node_value(&self, mut wisp: Gd<TwistedWisp>, node_idx: u32, value: f32) {
        let mut wisp = wisp.bind_mut();
        wisp.runner_mut().context_set_data_value(
            self.name.clone(),
            CallId(node_idx),
            DataIndex(0),
            value,
        );
    }

    #[func]
    fn set_as_main(&self, mut wisp: Gd<TwistedWisp>) {
        let mut wisp = wisp.bind_mut();
        wisp.runner_mut()
            .context_set_main_function(self.name.clone());
    }
}
