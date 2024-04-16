use std::collections::HashMap;

use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::values::{BasicValue, BasicValueEnum, FloatValue};
use inkwell::{AddressSpace, OptimizationLevel};
use thiserror::Error;

use super::function::Function;
use super::ir::{Operand, VarRef};

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
                    let prev = Self::build(&builder, "load_prev", vref, |b, n| {
                        b.build_load(type_f32, p_prev, n)
                    })?;
                    var_refs.insert(vref, prev);
                }
                StoreNext(vref) => {
                    let var = var_refs
                        .get(vref)
                        .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?;
                    Self::build(&builder, "store_next", vref, |b, _| {
                        b.build_store(p_next, *var)
                    })?;
                }
                BinaryOp(vref, type_, op1, op2) => {
                    let left = self.resolve_operand(&var_refs, op1)?;
                    let right = self.resolve_operand(&var_refs, op2)?;
                    use crate::wisp::ir::BinaryOpType::*;
                    let res = match type_ {
                        Add => Self::build(&builder, "binop_add", vref, |b, n| {
                            b.build_float_add(left, right, n)
                        }),
                        Subtract => Self::build(&builder, "binop_sub", vref, |b, n| {
                            b.build_float_sub(left, right, n)
                        }),
                        Multiply => Self::build(&builder, "binop_mul", vref, |b, n| {
                            b.build_float_mul(left, right, n)
                        }),
                        Divide => Self::build(&builder, "binop_div", vref, |b, n| {
                            b.build_float_div(left, right, n)
                        }),
                    }?;
                    var_refs.insert(vref, res.as_basic_value_enum());
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

    fn build<'ctx, F, R>(
        builder: &Builder<'ctx>,
        name: &str,
        vref: &VarRef,
        func: F,
    ) -> Result<R, SignalProcessCreationError>
    where
        F: FnOnce(&Builder<'ctx>, &str) -> Result<R, BuilderError>,
    {
        func(builder, &format!("tmp_{}_{}", name, vref.0))
            .map_err(|_| SignalProcessCreationError::BuildInstruction(name.into()))
    }

    fn resolve_operand<'ctx>(
        &'ctx self,
        var_refs: &HashMap<&VarRef, BasicValueEnum<'ctx>>,
        op1: &Operand,
    ) -> Result<FloatValue<'ctx>, SignalProcessCreationError> {
        use crate::wisp::ir::Operand::*;
        Ok(match op1 {
            Constant(v) => self.context.f32_type().const_float(*v as f64),
            Var(vref) => var_refs
                .get(vref)
                .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?
                .into_float_value(),
        })
    }
}
