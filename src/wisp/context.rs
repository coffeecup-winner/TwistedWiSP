use std::collections::HashMap;

use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::values::{BasicValue, BasicValueEnum, FloatValue};
use inkwell::{AddressSpace, OptimizationLevel};
use thiserror::Error;

use super::function::Function;
use super::ir::{Operand, VarRef};

type ProcessFn = unsafe extern "C" fn(buf_prev: *const f32, buf_next: *mut f32, output: *mut f32);

pub struct SignalProcessor<'ctx> {
    function: JitFunction<'ctx, ProcessFn>,
    num_outputs: usize,
    values0: Vec<f32>,
    values1: Vec<f32>,
    values_choice_flag: bool,
}

impl<'ctx> SignalProcessor<'ctx> {
    pub fn new(
        function: JitFunction<'ctx, ProcessFn>,
        num_outputs: usize,
        num_signals: usize,
    ) -> Self {
        SignalProcessor {
            function,
            num_outputs,
            values0: vec![0.0; num_signals],
            values1: vec![0.0; num_signals],
            values_choice_flag: false,
        }
    }

    pub fn process(&mut self, output: &mut [f32]) {
        // TODO: Return error instead?
        assert_eq!(0, output.len() % self.num_outputs);
        for chunk in output.chunks_mut(self.num_outputs) {
            self.process_one(chunk);
        }
    }

    pub fn process_one(&mut self, output: &mut [f32]) {
        self.values_choice_flag = !self.values_choice_flag;
        let (prev, next) = if self.values_choice_flag {
            (&self.values0, &mut self.values1)
        } else {
            (&self.values1, &mut self.values0)
        };
        unsafe {
            self.function
                .call(prev.as_ptr(), next.as_mut_ptr(), output.as_mut_ptr());
        }
    }

    #[allow(dead_code)]
    pub fn values(&self) -> &[f32] {
        if self.values_choice_flag {
            &self.values1
        } else {
            &self.values0
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
        let fn_type = type_f32.fn_type(
            &[type_pf32.into(), type_pf32.into(), type_pf32.into()],
            false,
        );

        let function = module.add_function("process", fn_type, None);
        let basic_block = self.context.append_basic_block(function, "start");

        builder.position_at_end(basic_block);

        let p_prev = Self::get_argument(function, 0)?.into_pointer_value();
        let p_next = Self::get_argument(function, 1)?.into_pointer_value();
        let p_output = Self::get_argument(function, 2)?.into_pointer_value();

        let mut num_outputs = 0;
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
                    let var = Self::get_var(&var_refs, vref)?;
                    Self::build(&builder, "store_next", vref, |b, _| {
                        b.build_store(p_next, var)
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
                Output(idx, vref) => {
                    let output = unsafe {
                        p_output.const_gep(
                            type_f32,
                            &[self.context.i32_type().const_int(idx.0 as u64, false)],
                        )
                    };
                    let value = Self::get_var(&var_refs, vref)?.into_float_value();
                    Self::build(&builder, "output", vref, |b, _| {
                        b.build_store(output, value)
                    })?;
                    num_outputs = num_outputs.max(idx.0 + 1);
                }
            }
        }

        builder
            .build_return(None)
            .map_err(|_| SignalProcessCreationError::BuildInstruction("return".into()))?;

        let function = unsafe { execution_engine.get_function("process") }
            .map_err(|_| SignalProcessCreationError::LoadFunction)?;
        Ok(SignalProcessor::new(function, num_outputs as usize, 1))
    }

    fn get_argument(
        function: inkwell::values::FunctionValue,
        nth: u32,
    ) -> Result<BasicValueEnum, SignalProcessCreationError> {
        function.get_nth_param(nth).ok_or_else(|| {
            SignalProcessCreationError::CustomLogicalError(
                "Invalid number of function arguments".into(),
            )
        })
    }

    fn get_var<'ctx>(
        var_refs: &HashMap<&VarRef, BasicValueEnum<'ctx>>,
        vref: &VarRef,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        Ok(*var_refs
            .get(vref)
            .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?)
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
            Var(vref) => Self::get_var(var_refs, vref)?.into_float_value(),
        })
    }
}
