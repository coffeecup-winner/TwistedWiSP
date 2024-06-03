use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use godot::prelude::*;
use twisted_wisp_protocol::WatchIndex;

use crate::{TwistedWisp, TwistedWispFlowNode};

#[derive(GodotClass)]
#[class(no_init)]
pub struct TwistedWispFlow {
    base: Base<RefCounted>,
    wisp: Gd<TwistedWisp>,
    name: String,
    watches: HashMap<WatchIndex, Vec<f32>>,
}

#[godot_api]
impl TwistedWispFlow {
    pub fn create(wisp: Gd<TwistedWisp>, name: String) -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            base,
            wisp,
            name,
            watches: HashMap::new(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn take_watch_updates(&mut self, watch_idx: WatchIndex) -> Option<Vec<f32>> {
        self.watches.remove(&watch_idx)
    }

    #[func]
    fn save_to_file(&mut self, path: String) {
        let mut wisp = self.wisp.bind_mut();
        let func = wisp.ctx_mut().get_function(&self.name).unwrap();
        let s = func.save();
        std::fs::write(Path::new(&path), s.as_bytes())
            .expect("Failed to save flow function to file");
    }

    #[func]
    fn add_node(&mut self, func_text: String) -> Gd<TwistedWispFlowNode> {
        let mut wisp = self.wisp.bind_mut();
        let ctx = wisp.ctx_mut();
        let (idx, func_name) = ctx.flow_add_node(&self.name, &func_text);
        if func_name.starts_with("$math") {
            let func = ctx.get_function(&func_name).unwrap();
            let ir_functions = func.get_ir_functions(ctx);
            wisp.runner_mut()
                .context_add_or_update_functions(ir_functions);
        }
        std::mem::drop(wisp);
        TwistedWispFlowNode::create(self.wisp.clone(), self.to_gd(), idx)
    }

    #[func]
    fn remove_node(&mut self, node: Gd<TwistedWispFlowNode>) {
        let mut wisp = self.wisp.bind_mut();
        let ctx = wisp.ctx_mut();
        let node_name = ctx
            .flow_remove_node(&self.name, node.bind().idx())
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
    fn list_nodes(&self) -> Array<Gd<TwistedWispFlowNode>> {
        let wisp = self.wisp.bind();
        let mut array = Array::new();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        for idx in flow.node_indices() {
            array.push(TwistedWispFlowNode::create(
                self.wisp.clone(),
                self.to_gd(),
                idx,
            ));
        }
        array
    }

    #[func]
    fn connect_nodes(
        &mut self,
        node_out: Gd<TwistedWispFlowNode>,
        node_outlet: u32,
        node_in: Gd<TwistedWispFlowNode>,
        node_inlet: u32,
    ) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .expect("Failed to get flow function");
        flow.connect(
            node_out.bind().idx(),
            node_outlet,
            node_in.bind().idx(),
            node_inlet,
        );
        let ctx = wisp.ctx();
        let func = ctx.get_function(&self.name).unwrap();
        let ir_functions = func.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
    }

    #[func]
    fn disconnect_nodes(
        &mut self,
        node_out: Gd<TwistedWispFlowNode>,
        node_outlet: u32,
        node_in: Gd<TwistedWispFlowNode>,
        node_inlet: u32,
    ) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.disconnect(
            node_out.bind().idx(),
            node_outlet,
            node_in.bind().idx(),
            node_inlet,
        );
        let ctx = wisp.ctx();
        let func = ctx.get_function(&self.name).unwrap();
        let ir_functions = func.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
    }

    #[func]
    fn list_connections(&self) -> Array<Dictionary> {
        let wisp = self.wisp.bind();
        let mut array = Array::new();
        let flow = wisp
            .ctx()
            .get_function(&self.name)
            .and_then(|f| f.as_flow())
            .unwrap();
        for idx in flow.edge_indices() {
            let (from, to, conn) = flow.get_connection(idx).unwrap();
            array.push(dict! {
                "from": TwistedWispFlowNode::create(self.wisp.clone(), self.to_gd(), from),
                "output_index": conn.output_index,
                "to": TwistedWispFlowNode::create(self.wisp.clone(), self.to_gd(), to),
                "input_index": conn.input_index,
            });
        }
        array
    }

    #[func]
    fn fetch_watch_updates(&mut self) {
        let mut wisp = self.wisp.bind_mut();
        // TODO: Take a flow name as an argument and return updates only for that flow
        self.watches = wisp.runner_mut().context_query_watched_data_values().values;
    }

    #[func]
    fn set_as_main(&mut self) {
        let mut wisp = self.wisp.bind_mut();
        wisp.runner_mut()
            .context_set_main_function(self.name.clone());
    }

    #[func]
    fn load_wave_file(&mut self, filepath: String) -> GString {
        let mut wisp = self.wisp.bind_mut();
        let path = wisp
            .config()
            .resolve_data_path(&PathBuf::from(filepath))
            .expect("Failed to resolve a data path");
        // TODO: Handle duplicate file names
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();

        let flow = wisp
            .ctx_mut()
            .get_function_mut(&self.name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();

        flow.add_buffer(&name, path.clone());

        wisp.runner_mut().context_load_wave_file(
            self.name.clone(),
            name.clone(),
            path.to_str().unwrap().to_owned(),
        );
        name.into()
    }
}
