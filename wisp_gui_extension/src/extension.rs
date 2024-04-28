use std::{io::Write, path::Path};

use godot::{engine::Engine, prelude::*};
use twisted_wisp::{
    CodeFunction, DefaultInputValue, FlowFunction, FunctionInput, FunctionOutput, WispContext,
    WispFunction,
};
use twisted_wisp_ir::{
    BinaryOpType, ComparisonOpType, FunctionOutputIndex, Instruction, LocalRef, Operand,
    SourceLocation, TargetLocation, VarRef,
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

// TODO: Remove this
fn create_test_function() -> Box<dyn WispFunction> {
    Box::new(CodeFunction::new(
        "test".into(),
        vec![FunctionInput::new(DefaultInputValue::Value(0.0))],
        vec![FunctionOutput],
        vec![],
        vec![
            Instruction::AllocLocal(LocalRef(0)),
            Instruction::BinaryOp(
                VarRef(0),
                BinaryOpType::Add,
                Operand::Arg(0),
                Operand::Literal(0.01),
            ),
            Instruction::Store(TargetLocation::Local(LocalRef(0)), Operand::Var(VarRef(0))),
            Instruction::ComparisonOp(
                VarRef(1),
                ComparisonOpType::Greater,
                Operand::Var(VarRef(0)),
                Operand::Literal(1.0),
            ),
            Instruction::Conditional(
                VarRef(1),
                vec![
                    Instruction::BinaryOp(
                        VarRef(0),
                        BinaryOpType::Subtract,
                        Operand::Var(VarRef(0)),
                        Operand::Literal(1.0),
                    ),
                    Instruction::Store(TargetLocation::Local(LocalRef(0)), Operand::Var(VarRef(0))),
                ],
                vec![],
            ),
            Instruction::Load(VarRef(0), SourceLocation::Local(LocalRef(0))),
            Instruction::Store(
                TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                Operand::Var(VarRef(0)),
            ),
        ],
        None,
    ))
}

#[godot_api]
impl TwistedWispSingleton {
    #[func]
    fn init(&mut self, wisp_exe_path: String) {
        godot::log::godot_print!("init: {}", wisp_exe_path);

        let mut runner = WispRunnerClient::init(Path::new(&wisp_exe_path));
        let sys_info = runner.get_system_info();

        let mut ctx = WispContext::new(sys_info.num_channels);
        ctx.add_builtin_functions();

        // TODO: Remove this
        ctx.add_function(create_test_function());

        for f in ctx.functions_iter() {
            runner.context_add_or_update_function(f.get_ir_function(&ctx));
        }

        self.runner = Some(runner);
        self.ctx = Some(ctx);
    }

    #[func]
    fn dsp_start(&mut self) {
        godot::log::godot_print!("enable_dsp");
        self.runner.as_mut().unwrap().dsp_start();
    }

    #[func]
    fn dsp_stop(&mut self) {
        godot::log::godot_print!("disable_dsp");
        self.runner.as_mut().unwrap().dsp_stop();
    }

    #[func]
    fn function_create(&mut self) -> String {
        let ctx = self.ctx.as_mut().unwrap();
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
        self.ctx.as_mut().unwrap().remove_function(&name);
    }

    #[func]
    fn function_list(&mut self) -> Array<GString> {
        let mut array = Array::new();
        for f in self.ctx.as_mut().unwrap().functions_iter() {
            array.push(f.name().into());
        }
        array
    }

    #[func]
    fn function_get_metadata(&mut self, name: String) -> Dictionary {
        let func = self.ctx.as_mut().unwrap().get_function(&name).unwrap();
        dict! {
            "num_inlets": func.inputs_count(),
            "num_outlets": func.outputs_count(),
        }
    }

    #[func]
    fn function_set_main(&mut self, name: String) {
        self.runner
            .as_mut()
            .unwrap()
            .context_set_main_function(name);
    }

    #[func]
    fn function_open(&mut self, path: String) -> String {
        let _f = std::fs::File::open(Path::new(&path)).expect("Failed to open file to load");
        // TODO: Implement this
        "TODO".into()
    }

    #[func]
    fn function_save(&mut self, name: String, path: String) {
        let _func = self.ctx.as_mut().unwrap().get_function(&name).unwrap();
        let mut f = std::fs::File::create(Path::new(&path)).expect("Failed to open file to save");
        f.write_all("TODO".as_bytes())
            .expect("Failed to write flow to file");
    }

    #[func]
    fn flow_add_node(&mut self, flow_name: String, func_name: String) -> u32 {
        let flow = self
            .ctx
            .as_mut()
            .unwrap()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        let idx = flow.add_node(func_name);
        idx.index() as u32
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
            .ctx
            .as_mut()
            .unwrap()
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
    fn flow_connect(
        &mut self,
        flow_name: String,
        node_out: u32,
        node_outlet: u32,
        node_in: u32,
        node_inlet: u32,
    ) {
        let flow = self
            .ctx
            .as_mut()
            .unwrap()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.connect(node_out.into(), node_outlet, node_in.into(), node_inlet);
        let ctx = self.ctx.as_ref().unwrap();
        let func = ctx.get_function(&flow_name).unwrap();
        self.runner
            .as_mut()
            .unwrap()
            .context_add_or_update_function(func.get_ir_function(ctx));
        self.runner.as_mut().unwrap().context_update();
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
            .ctx
            .as_mut()
            .unwrap()
            .get_function_mut(&flow_name)
            .and_then(|f| f.as_flow_mut())
            .unwrap();
        flow.disconnect(node_out.into(), node_outlet, node_in.into(), node_inlet);
        let ctx = self.ctx.as_ref().unwrap();
        let func = ctx.get_function(&flow_name).unwrap();
        self.runner
            .as_mut()
            .unwrap()
            .context_add_or_update_function(func.get_ir_function(ctx));
        self.runner.as_mut().unwrap().context_update();
    }
}
