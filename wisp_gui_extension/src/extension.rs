use std::path::Path;

use godot::{engine::Engine, prelude::*};
use twisted_wisp_protocol::{FlowNodeIndex, FlowNodeInletIndex, FlowNodeOutletIndex, WispClient};

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
    wisp: Option<WispClient>,
}

#[godot_api]
impl TwistedWispSingleton {
    #[func]
    fn init(&mut self, wisp_exe_path: String) {
        godot::log::godot_print!("init: {}", wisp_exe_path);
        self.wisp = Some(WispClient::init(Path::new(&wisp_exe_path)));
    }

    #[func]
    fn enable_dsp(&mut self) {
        godot::log::godot_print!("enable_dsp");
        self.wisp.as_mut().unwrap().enable_dsp();
    }

    #[func]
    fn disable_dsp(&mut self) {
        godot::log::godot_print!("disable_dsp");
        self.wisp.as_mut().unwrap().disable_dsp();
    }

    #[func]
    fn create_function(&mut self) -> String {
        self.wisp.as_mut().unwrap().create_function()
    }

    #[func]
    fn remove_function(&mut self, name: String) {
        self.wisp.as_mut().unwrap().remove_function(name)
    }

    #[func]
    fn flow_add_node(&mut self, flow_name: String, func_name: String) -> u32 {
        self.wisp
            .as_mut()
            .unwrap()
            .flow_add_node(flow_name, func_name)
            .0
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
        self.wisp.as_mut().unwrap().flow_connect(
            flow_name,
            FlowNodeIndex(node_out),
            FlowNodeOutletIndex(node_outlet),
            FlowNodeIndex(node_in),
            FlowNodeInletIndex(node_inlet),
        )
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
        self.wisp.as_mut().unwrap().flow_disconnect(
            flow_name,
            FlowNodeIndex(node_out),
            FlowNodeOutletIndex(node_outlet),
            FlowNodeIndex(node_in),
            FlowNodeInletIndex(node_inlet),
        )
    }
}
