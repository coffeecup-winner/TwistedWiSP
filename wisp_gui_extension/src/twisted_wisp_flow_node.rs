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

#[derive(GodotClass)]
#[class(init)]
pub struct TwistedWispFlowNodeProperty {
    base: Base<RefCounted>,
    #[var]
    name: GString,
    #[var]
    display_name: GString,
    #[var]
    value_type: GString,
    #[var]
    min_value: f32,
    #[var]
    max_value: f32,
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

    #[signal]
    fn coordinates_changed(&self, x: f32, y: f32, w: f32, h: f32);

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
    fn learn_midi_cc(&mut self) {
        let mut wisp = self.wisp.bind_mut();
        let watch_idx = wisp
            .runner_mut()
            .context_learn_midi_cc(
                self.flow.bind().name().to_owned(),
                CallId(self.idx.index() as u32),
                DataIndex(0),
            )
            .expect("Failed to learn a MIDI CC");
        self.watch_idx = Some(watch_idx);
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

    #[func]
    fn get_properties(&self) -> Array<Gd<TwistedWispFlowNodeProperty>> {
        let mut array = Array::new();
        array.extend([
            Gd::from_init_fn(|base| TwistedWispFlowNodeProperty {
                base,
                name: "x".into(),
                display_name: "x".into(),
                value_type: "number".into(),
                min_value: -10000.0,
                max_value: 10000.0,
            }),
            Gd::from_init_fn(|base| TwistedWispFlowNodeProperty {
                base,
                name: "y".into(),
                display_name: "y".into(),
                value_type: "number".into(),
                min_value: -10000.0,
                max_value: 10000.0,
            }),
            Gd::from_init_fn(|base| TwistedWispFlowNodeProperty {
                base,
                name: "w".into(),
                display_name: "w".into(),
                value_type: "number".into(),
                min_value: -10000.0,
                max_value: 10000.0,
            }),
            Gd::from_init_fn(|base| TwistedWispFlowNodeProperty {
                base,
                name: "h".into(),
                display_name: "h".into(),
                value_type: "number".into(),
                min_value: -10000.0,
                max_value: 10000.0,
            }),
        ]);
        array
    }

    #[func]
    fn get_property_number(&self, name: String) -> f32 {
        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(self.idx).unwrap();
        match name.as_str() {
            "x" => node.coords.x as f32,
            "y" => node.coords.y as f32,
            "w" => node.coords.w as f32,
            "h" => node.coords.h as f32,
            _ => 0.0,
        }
    }

    #[func]
    fn set_property_number(&mut self, name: String, value: f32) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(self.flow.bind().name())
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let node = flow.get_node_mut(self.idx).unwrap();
        let mut coords_changed = false;
        match name.as_str() {
            "x" => {
                node.coords.x = value as i32;
                coords_changed = true;
            }
            "y" => {
                node.coords.y = value as i32;
                coords_changed = true;
            }
            "w" => {
                node.coords.w = value as u32;
                coords_changed = true;
            }
            "h" => {
                node.coords.h = value as u32;
                coords_changed = true;
            }
            _ => {}
        }

        let coords = node.coords.clone();
        std::mem::drop(wisp);
        if coords_changed {
            self.to_gd().emit_signal(
                "coordinates_changed".into(),
                &[
                    Variant::from(coords.x),
                    Variant::from(coords.y),
                    Variant::from(coords.w),
                    Variant::from(coords.h),
                ],
            );
        }
    }
}
