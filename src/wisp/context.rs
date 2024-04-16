use std::collections::HashMap;

use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::{AddressSpace, OptimizationLevel};
use thiserror::Error;

use super::function::Function;

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

#[derive(Debug, Error)]
pub enum SignalProcessCreationError {
    #[error("Failed to initialize the evaluation engine")]
    InitEE,

    #[error("Failed to load the function")]
    LoadFunction,

    #[error("Failed to build instruction: {0}")]
    BuildInstruction(String),

    #[error("Var ref {0} is uninitialized")]
    UninitializedVar(u32),

    #[error("Logical error: {0}")]
    CustomLogicalError(String),
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

    pub fn create_signal_processor(
        &mut self,
        func: &Function,
    ) -> Result<SignalProcessor, SignalProcessCreationError> {
        self.id_gen += 1;

        let module = self.context.create_module(&format!("wisp_{}", self.id_gen));
        let builder = self.context.create_builder();
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .map_err(|_| SignalProcessCreationError::InitEE)?;

        let type_f32 = self.context.f32_type();
        let type_pf32 = type_f32.ptr_type(AddressSpace::default());
        let fn_type = type_f32.fn_type(&[type_pf32.into(), type_pf32.into()], false);

        let function = module.add_function("process", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "start");

        builder.position_at_end(basic_block);

        let p_prev = function
            .get_nth_param(0)
            .ok_or_else(|| {
                SignalProcessCreationError::CustomLogicalError(
                    "Invalid number of function arguments".into(),
                )
            })?
            .into_pointer_value();
        let p_next = function
            .get_nth_param(1)
            .ok_or_else(|| {
                SignalProcessCreationError::CustomLogicalError(
                    "Invalid number of function arguments".into(),
                )
            })?
            .into_pointer_value();

        let mut var_refs = HashMap::new();
        for insn in func.instructions() {
            use super::ir::Instruction::*;

            match insn {
                LoadPrev(vref) => {
                    let prev = builder
                        .build_load(type_f32, p_prev, &format!("tmp_load_prev_{}", vref.0))
                        .map_err(|_| {
                            SignalProcessCreationError::BuildInstruction("load_prev".into())
                        })?;
                    var_refs.insert(vref, prev);
                }
                StoreNext(vref) => {
                    let var = var_refs
                        .get(vref)
                        .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?;
                    builder.build_store(p_next, *var).map_err(|_| {
                        SignalProcessCreationError::BuildInstruction("store_next".into())
                    })?;
                }
            }
        }

        builder
            .build_return(None)
            .map_err(|_| SignalProcessCreationError::BuildInstruction("return".into()))?;

        let function = unsafe { execution_engine.get_function("process") }
            .map_err(|_| SignalProcessCreationError::LoadFunction)?;
        Ok(SignalProcessor { function })
    }
}
