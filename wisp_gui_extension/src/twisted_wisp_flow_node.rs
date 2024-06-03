use godot::prelude::*;
use twisted_wisp::{FlowNodeIndex, WispFunction};
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{DataIndex, WatchIndex};

use crate::{TwistedWisp, TwistedWispFlow};

#[derive(GodotClass)]
#[class(no_init)]
pub struct TwistedWispFlowNode {
    base: Base<RefCounted>,
    wisp: Gd<TwistedWisp>,
    flow: Gd<TwistedWispFlow>,
    idx: FlowNodeIndex,
    watch_idx: Option<WatchIndex>,
}

#[godot_api]
impl TwistedWispFlowNode {
    pub fn create(
        wisp: Gd<TwistedWisp>,
        flow: Gd<TwistedWispFlow>,
        idx: FlowNodeIndex,
    ) -> Gd<Self> {
        Gd::from_init_fn(|base| Self {
            base,
            wisp,
            flow,
            idx,
            watch_idx: None,
        })
    }

    pub fn idx(&self) -> FlowNodeIndex {
        self.idx
    }

    #[func]
    fn id(&self) -> u32 {
        self.idx.index() as u32
    }

    #[func]
    fn flow(&self) -> Gd<TwistedWispFlow> {
        self.flow.clone()
    }

    #[func]
    fn function_name(&self) -> String {
        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(self.idx).expect("Failed to find node");
        node.name.clone()
    }

    #[func]
    fn display_name(&self) -> String {
        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(self.idx).unwrap();
        node.display_text.clone()
    }

    #[func]
    fn coordinates(&self) -> Dictionary {
        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let data = &flow.get_node(self.idx).unwrap().coords;
        dict! {
            "x": data.x,
            "y": data.y,
            "w": data.w,
            "h": data.h,
        }
    }

    #[func]
    fn set_coordinates(&mut self, x: i32, y: i32, w: u32, h: u32) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(self.flow.bind().name())
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let data = &mut flow.get_node_mut(self.idx).unwrap().coords;
        data.x = x;
        data.y = y;
        data.w = w;
        data.h = h;
    }

    #[func]
    fn add_watch(&mut self) {
        let mut wisp = self.wisp.bind_mut();
        // TODO: Maybe remove this and do flow borrow checking at runtime?
        let ctx = wisp.ctx();
        let flow = ctx
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let ir_functions = flow.get_ir_functions(ctx);
        let runner = wisp.runner_mut();
        // NOTE: We do not update the watch function as we expect it to never change
        // at runtime and it's a part of the core library
        runner.context_add_or_update_functions(ir_functions);
        runner.context_update();
        let watch_idx = runner
            .context_watch_data_value(
                self.flow.bind().name().to_owned(),
                CallId(self.idx.index() as u32),
                DataIndex(0),
            )
            .expect("Failed to watch a data value");
        self.watch_idx = Some(watch_idx);
    }

    #[func]
    fn get_watch_updates(&mut self) -> Array<f32> {
        let mut array = Array::new();
        if let Some(watch_idx) = self.watch_idx {
            if let Some(values) = self.flow.bind_mut().take_watch_updates(watch_idx) {
                for value in values {
                    array.push(value);
                }
            }
        }
        array
    }

    #[func]
    fn get_data_value(&self) -> f32 {
        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(self.idx).unwrap();
        node.value.unwrap_or(0.0)
    }

    #[func]
    fn set_data_value(&mut self, value: f32) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(self.flow.bind().name())
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.get_node_mut(self.idx).unwrap().value = Some(value);
        wisp.runner_mut().context_set_data_value(
            self.flow.bind().name().to_owned(),
            CallId(self.idx.index() as u32),
            DataIndex(0),
            value,
        );
    }

    #[func]
    fn set_data_buffer(&mut self, name: String) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(self.flow.bind().name())
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.get_node_mut(self.idx).unwrap().buffer = Some(name.clone());
        wisp.runner_mut().context_set_data_array(
            self.flow.bind().name().to_owned(),
            CallId(self.idx.index() as u32),
            DataIndex(0),
            name,
        );
    }
}
