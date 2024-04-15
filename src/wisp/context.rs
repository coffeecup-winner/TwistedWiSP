use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::{AddressSpace, OptimizationLevel};

type ProcessFn = unsafe extern "C" fn(*const f32, *mut f32);

pub struct SignalProcessor<'ctx> {
    function: JitFunction<'ctx, ProcessFn>,
}

impl<'ctx> SignalProcessor<'ctx> {
    pub fn process(&self, prev: &[f32], next: &mut [f32]) {
        unsafe {
            self.function.call(prev.as_ptr(), next.as_mut_ptr());
        }
    }
}

pub struct SignalProcessorContext {
    id_gen: u64,
    context: Context,
}

impl SignalProcessorContext {
    pub fn new() -> Self {
        SignalProcessorContext {
            id_gen: 0,
            context: Context::create(),
        }
    }

    pub fn create_signal_processor(&mut self) -> Option<SignalProcessor> {
        self.id_gen += 1;

        let module = self.context.create_module(&format!("wisp_{}", self.id_gen));
        let builder = self.context.create_builder();
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .unwrap();

        let type_f32 = self.context.f32_type();
        let type_pf32 = type_f32.ptr_type(AddressSpace::default());
        let fn_type = type_f32.fn_type(&[type_pf32.into(), type_pf32.into()], false);

        let function = module.add_function("process", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "start");

        builder.position_at_end(basic_block);

        let p_prev = function.get_nth_param(0)?.into_pointer_value();
        let p_next = function.get_nth_param(1)?.into_pointer_value();

        // TODO: Replace this with custom IR translation
        let prev = builder.build_load(type_f32, p_prev, "process").unwrap();
        builder.build_store(p_next, prev).unwrap();

        builder.build_return(None).unwrap();

        let function = unsafe { execution_engine.get_function("process").ok().unwrap() };
        Some(SignalProcessor { function })
    }
}
