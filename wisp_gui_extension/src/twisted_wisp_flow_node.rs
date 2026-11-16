use std::str::FromStr;

use godot::prelude::*;

use twisted_wisp::{
    core::{FlowNodeExtraData, FlowNodeIndex, WispFunction},
    ir::CallId,
    protocol::{DataIndex, WatchIndex},
};

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

pub enum TwistedWispFlowNodePropertyType {
    Integer,
    Float,
    String,
}

impl FromStr for TwistedWispFlowNodePropertyType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "integer" => Ok(Self::Integer),
            "float" => Ok(Self::Float),
            "string" => Ok(Self::String),
            _ => Err(()),
        }
    }
}

impl TwistedWispFlowNodePropertyType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Integer => "integer",
            Self::Float => "float",
            Self::String => "string",
        }
    }
}

pub enum TwistedWispFlowNodeProperty {
    PositionX,
    PositionY,
    Width,
    Height,
    Value,
    Buffer,
}

impl FromStr for TwistedWispFlowNodeProperty {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "x" => Ok(Self::PositionX),
            "y" => Ok(Self::PositionY),
            "w" => Ok(Self::Width),
            "h" => Ok(Self::Height),
            "value" => Ok(Self::Value),
            "buffer" => Ok(Self::Buffer),
            _ => Err(()),
        }
    }
}

impl TwistedWispFlowNodeProperty {
    pub fn as_str(&self) -> &str {
        match self {
            Self::PositionX => "x",
            Self::PositionY => "y",
            Self::Width => "w",
            Self::Height => "h",
            Self::Value => "value",
            Self::Buffer => "buffer",
        }
    }

    pub fn value_type(&self) -> TwistedWispFlowNodePropertyType {
        match self {
            Self::PositionX => TwistedWispFlowNodePropertyType::Integer,
            Self::PositionY => TwistedWispFlowNodePropertyType::Integer,
            Self::Width => TwistedWispFlowNodePropertyType::Integer,
            Self::Height => TwistedWispFlowNodePropertyType::Integer,
            Self::Value => TwistedWispFlowNodePropertyType::Float,
            Self::Buffer => TwistedWispFlowNodePropertyType::String,
        }
    }

    pub fn get_descriptor(&self) -> Gd<TwistedWispFlowNodePropertyData> {
        let (display_name, min_value, max_value, step) = match self {
            Self::PositionX => ("x".into(), -10000.0, 10000.0, 1.0),
            Self::PositionY => ("y".into(), -10000.0, 10000.0, 1.0),
            Self::Width => ("w".into(), -10000.0, 10000.0, 1.0),
            Self::Height => ("h".into(), -10000.0, 10000.0, 1.0),
            Self::Value => ("value".into(), 0.0, 1.0, 0.001),
            Self::Buffer => ("buffer".into(), 0.0, 0.0, 0.0),
        };
        Gd::from_init_fn(|base| TwistedWispFlowNodePropertyData {
            base,
            name: self.as_str().into(),
            display_name,
            value_type: self.value_type().as_str().into(),
            min_value,
            max_value,
            step,
        })
    }
}

#[derive(GodotClass)]
#[class(init)]
pub struct TwistedWispFlowNodePropertyData {
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
    #[var]
    step: f32,
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
    fn property_value_changed(&self, name: GString, value: Variant);

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
    fn get_properties(&self) -> Array<Gd<TwistedWispFlowNodePropertyData>> {
        let mut array = Array::new();
        array.extend(
            [
                TwistedWispFlowNodeProperty::PositionX,
                TwistedWispFlowNodeProperty::PositionY,
                TwistedWispFlowNodeProperty::Width,
                TwistedWispFlowNodeProperty::Height,
            ]
            .into_iter()
            .map(|p| p.get_descriptor()),
        );

        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(self.idx).unwrap();
        match &node.name[..] {
            "control" => {
                array.push(TwistedWispFlowNodeProperty::Value.get_descriptor());
            }
            "buffer" => {
                array.push(TwistedWispFlowNodeProperty::Buffer.get_descriptor());
            }
            _ => {}
        }

        array
    }

    #[func]
    fn get_property_value(&self, name: String) -> Variant {
        let wisp = self.wisp.bind();
        let flow = wisp
            .ctx()
            .get_function(self.flow.bind().name())
            .and_then(|f| f.as_flow())
            .unwrap();
        let node = flow.get_node(self.idx).unwrap();
        let prop = name
            .parse::<TwistedWispFlowNodeProperty>()
            .expect("Invalid property name");
        match prop.value_type() {
            TwistedWispFlowNodePropertyType::Integer => {
                Variant::from(node.extra_data[prop.as_str()].as_integer().unwrap())
            }
            TwistedWispFlowNodePropertyType::Float => {
                Variant::from(node.extra_data[prop.as_str()].as_float().unwrap())
            }
            TwistedWispFlowNodePropertyType::String => {
                Variant::from(node.extra_data[prop.as_str()].as_string().unwrap())
            }
        }
    }

    #[func]
    fn set_property_value(&mut self, name: String, value: Variant) {
        let mut wisp = self.wisp.bind_mut();
        let flow = wisp
            .ctx_mut()
            .get_function_mut(self.flow.bind().name())
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let node = flow.get_node_mut(self.idx).unwrap();
        let prop = name
            .parse::<TwistedWispFlowNodeProperty>()
            .expect("Invalid property name");
        let new_value = match prop.value_type() {
            TwistedWispFlowNodePropertyType::Integer => {
                assert_eq!(value.get_type(), VariantType::INT);
                FlowNodeExtraData::Integer(value.to::<i32>())
            }
            TwistedWispFlowNodePropertyType::Float => {
                assert_eq!(value.get_type(), VariantType::FLOAT);
                FlowNodeExtraData::Float(value.to::<f32>())
            }
            TwistedWispFlowNodePropertyType::String => {
                assert_eq!(value.get_type(), VariantType::STRING);
                FlowNodeExtraData::String(value.to::<String>())
            }
        };
        // TODO: Should there always be an old value?
        if let Some(old_value) = node
            .extra_data
            .insert(prop.as_str().to_owned(), new_value.clone())
        {
            if old_value == new_value {
                // No need to run the on change handlers as there was no change
                return;
            }
        }

        // Handle the property change

        match prop {
            TwistedWispFlowNodeProperty::Value => wisp.runner_mut().context_set_data_value(
                self.flow.bind().name().to_owned(),
                CallId(self.idx.index() as u32),
                DataIndex(0),
                value.to::<f32>(),
            ),
            TwistedWispFlowNodeProperty::Buffer => {
                wisp.runner_mut().context_set_data_array(
                    self.flow.bind().name().to_owned(),
                    CallId(self.idx.index() as u32),
                    DataIndex(0),
                    value.to::<String>(),
                );
            }
            _ => {}
        }

        // Make sure we drop the mutable reference since we can reenter before the end of the function
        std::mem::drop(wisp);
        self.base_mut().emit_signal(
            "property_value_changed".into(),
            &[Variant::from(name), value],
        );
    }
}
