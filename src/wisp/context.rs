use std::collections::HashMap;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::execution_engine::JitFunction;
use inkwell::module::Module;
use inkwell::values::{
    AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FloatValue, FunctionValue,
    PointerValue,
};
use inkwell::{AddressSpace, OptimizationLevel};
use thiserror::Error;

use super::flow::Flow;
use super::function::{Function, FunctionInput};
use super::ir::{FunctionInputIndex, GlobalRef, Instruction, LocalRef, Operand, VarRef};
use super::runtime::Runtime;

struct Globals {
    p_output: *mut f32,
    p_prev: *const f32,
    p_next: *mut f32,
}
type ProcessFn = unsafe extern "C" fn(buf_prev: *const f32, buf_next: *mut f32, output: *mut f32);

pub struct SignalProcessor<'ctx> {
    _globals: Box<Globals>,
    function: JitFunction<'ctx, ProcessFn>,
    num_outputs: usize,
    values0: Vec<f32>,
    values1: Vec<f32>,
    values_choice_flag: bool,
}

impl<'ctx> SignalProcessor<'ctx> {
    fn new(
        globals: Box<Globals>,
        function: JitFunction<'ctx, ProcessFn>,
        num_outputs: usize,
        num_signals: usize,
    ) -> Self {
        SignalProcessor {
            _globals: globals,
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

    #[error("Local ref {0} is uninitialized")]
    UninitializedLocal(u32),

    #[error("Function {0} is not found")]
    UnknownFunction(String),

    #[error("Invalid number of arguments for function {0}: expected {1}, found {2}")]
    InvalidNumberOfInputs(String, u32, u32),

    #[error("Invalid number of outputs for function {0}: expected at most {1}, found {2}")]
    InvalidNumberOfOutputs(String, u32, u32),

    #[error("Output {1} for function {0} was not initialized")]
    UninitialzedOutput(String, u32),

    #[error("Logical error: {0}")]
    CustomLogicalError(String),
}

#[derive(Debug, Default)]
struct FunctionRefs<'ctx> {
    vars: HashMap<VarRef, BasicValueEnum<'ctx>>,
    locals: HashMap<LocalRef, PointerValue<'ctx>>,
    outputs: Vec<Option<BasicValueEnum<'ctx>>>,
}

impl<'ctx> FunctionRefs<'ctx> {
    fn new(num_outputs: usize) -> Self {
        FunctionRefs {
            outputs: vec![None; num_outputs],
            ..Default::default()
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

    pub fn create_signal_processor(
        &mut self,
        flow: &Flow,
        runtime: &Runtime,
    ) -> Result<SignalProcessor, SignalProcessCreationError> {
        self.id_gen += 1;

        let module = self.context.create_module(&format!("wisp_{}", self.id_gen));
        let builder = self.context.create_builder();
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .map_err(|_| SignalProcessCreationError::InitEE)?;

        let f32_type = self.context.f32_type();
        let pf32_type = f32_type.ptr_type(AddressSpace::default());
        let g_output = module.add_global(pf32_type, None, "wisp_global_output");
        let g_prev = module.add_global(pf32_type, None, "wisp_global_prev");
        let g_next = module.add_global(pf32_type, None, "wisp_global_next");
        let g_wisp_debug = module.add_function(
            "wisp_debug",
            self.context
                .void_type()
                .fn_type(&[f32_type.into(); 1], false),
            None,
        );
        let mut globals = Box::new(Globals {
            p_output: std::ptr::null_mut(),
            p_prev: std::ptr::null(),
            p_next: std::ptr::null_mut(),
        });
        execution_engine.add_global_mapping(&g_output, &mut globals.p_output as *mut _ as usize);
        execution_engine.add_global_mapping(&g_prev, &mut globals.p_prev as *mut _ as usize);
        execution_engine.add_global_mapping(&g_next, &mut globals.p_next as *mut _ as usize);
        extern "C" fn wisp_debug(v: f32) {
            eprintln!("Debug: {}", v);
        }
        execution_engine.add_global_mapping(&g_wisp_debug, wisp_debug as usize);

        for (name, func) in runtime.functions_iter() {
            // >1 returns is currently not supported
            assert!(func.outputs().len() < 2);
            let arg_types = vec![f32_type.into(); func.inputs().len()];
            let fn_type = if func.outputs().len() == 1 {
                f32_type.fn_type(&arg_types, false)
            } else {
                self.context.void_type().fn_type(&arg_types, false)
            };
            module.add_function(name, fn_type, None);
        }

        for (_, func) in runtime.functions_iter() {
            self.build_function(&module, func, runtime, &builder)?;
        }

        let mut process_func_instructions = vec![
            Instruction::LoadFunctionInput(VarRef(0), FunctionInputIndex(0)),
            Instruction::StoreGlobal(GlobalRef::Prev, VarRef(0)),
            Instruction::LoadFunctionInput(VarRef(1), FunctionInputIndex(1)),
            Instruction::StoreGlobal(GlobalRef::Next, VarRef(1)),
            Instruction::LoadFunctionInput(VarRef(2), FunctionInputIndex(2)),
            Instruction::StoreGlobal(GlobalRef::Output, VarRef(2)),
        ];
        process_func_instructions.extend(flow.get_compiled_flow(runtime).iter().cloned());
        let func = Function::new(
            "wisp_process".into(),
            vec![FunctionInput, FunctionInput, FunctionInput],
            vec![],
            process_func_instructions,
        );

        module.add_function(
            func.name(),
            self.context
                .void_type()
                .fn_type(&[pf32_type.into(); 3], false),
            None,
        );

        self.build_function(&module, &func, runtime, &builder)?;

        if cfg!(debug_assertions) {
            module.print_to_stderr();
        }

        let function = unsafe { execution_engine.get_function("wisp_process") }
            .map_err(|_| SignalProcessCreationError::LoadFunction)?;
        Ok(SignalProcessor::new(
            globals,
            function,
            runtime.num_outputs() as usize,
            1,
        ))
    }

    fn build_function<'ctx, 'temp>(
        &'ctx self,
        module: &'temp Module<'ctx>,
        func: &Function,
        runtime: &Runtime,
        builder: &'temp Builder<'ctx>,
    ) -> Result<(), SignalProcessCreationError> {
        let function = module
            .get_function(func.name())
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(func.name().to_owned()))?;
        let mut refs = FunctionRefs::new(func.outputs().len());
        self.translate_instructions(
            func.instructions(),
            module,
            builder,
            func,
            function,
            runtime,
            &mut refs,
        )?;
        if !refs.outputs.iter().all(|o| o.is_some()) {
            return Err(SignalProcessCreationError::UninitialzedOutput(
                func.name().to_owned(),
                refs.outputs
                    .iter()
                    .enumerate()
                    .find(|(_, o)| o.is_none())
                    .unwrap()
                    .0 as u32,
            ));
        }
        match func.outputs().len() {
            0 => {
                builder
                    .build_return(None)
                    .map_err(|_| SignalProcessCreationError::BuildInstruction("return".into()))?;
            }
            1 => {
                builder
                    .build_return(Some(&refs.outputs[0].expect("Invalid function output")))
                    .map_err(|_| SignalProcessCreationError::BuildInstruction("return".into()))?;
            }
            _ => todo!(),
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn translate_instructions<'ctx, 'temp>(
        &'ctx self,
        instructions: &'temp [Instruction],
        module: &'temp Module<'ctx>,
        builder: &'temp Builder<'ctx>,
        func: &Function,
        function: FunctionValue<'ctx>,
        runtime: &Runtime,
        refs: &mut FunctionRefs<'ctx>,
    ) -> Result<(BasicBlock<'ctx>, BasicBlock<'ctx>), SignalProcessCreationError> {
        let mut current_block = self.context.append_basic_block(function, "start");
        builder.position_at_end(current_block);
        let start_block = current_block;

        for insn in instructions {
            use super::ir::Instruction::*;
            match insn {
                LoadPrev(vref) => {
                    let pp_prev = module
                        .get_global("wisp_global_prev")
                        .expect("Invalid global value");
                    let p_prev = Self::build(builder, "load_prev", vref, |b, n| {
                        b.build_load(
                            self.context.f32_type().ptr_type(AddressSpace::default()),
                            pp_prev.as_pointer_value(),
                            n,
                        )
                    })?;
                    let prev = Self::build(builder, "load_prev", vref, |b, n| {
                        b.build_load(self.context.f32_type(), p_prev.into_pointer_value(), n)
                    })?;
                    refs.vars.insert(*vref, prev);
                }
                StoreNext(vref) => {
                    let pp_next = module
                        .get_global("wisp_global_next")
                        .expect("Invalid global value");
                    let p_next = Self::build(builder, "load_next", vref, |b, n| {
                        b.build_load(
                            self.context.f32_type().ptr_type(AddressSpace::default()),
                            pp_next.as_pointer_value(),
                            n,
                        )
                    })?;
                    let var = Self::get_var(refs, vref)?;
                    Self::build(builder, "store_next", vref, |b, _| {
                        b.build_store(p_next.into_pointer_value(), var)
                    })?;
                }
                AllocLocal(lref) => {
                    let local = Self::build(builder, "alloc_local", &VarRef(0), |b, _| {
                        b.build_alloca(self.context.f32_type(), &format!("local_{}", lref.0))
                    })?;
                    refs.locals.insert(*lref, local);
                }
                LoadLocal(vref, lref) => {
                    let local = Self::get_local(refs, lref)?;
                    let value = Self::build(builder, "load_local", vref, |b, n| {
                        b.build_load(self.context.f32_type(), local, n)
                    })?;
                    refs.vars.insert(*vref, value);
                }
                StoreLocal(lref, vref) => {
                    let local = Self::get_local(refs, lref)?;
                    let value = Self::get_var(refs, vref)?;
                    Self::build(builder, "store_local", vref, |b, _| {
                        b.build_store(local, value)
                    })?;
                }
                LoadGlobal(vref, gref) => {
                    let global = match gref {
                        GlobalRef::Output => module.get_global("wisp_global_output"),
                        GlobalRef::Prev => module.get_global("wisp_global_prev"),
                        GlobalRef::Next => module.get_global("wisp_global_next"),
                    }
                    .expect("Invalid global name");
                    let value = Self::build(builder, "load_global", vref, |b, n| {
                        b.build_load(
                            self.context.f32_type().ptr_type(AddressSpace::default()),
                            global.as_pointer_value(),
                            n,
                        )
                    })?;
                    refs.vars.insert(*vref, value);
                }
                StoreGlobal(gref, vref) => {
                    let global = match gref {
                        GlobalRef::Output => module.get_global("wisp_global_output"),
                        GlobalRef::Prev => module.get_global("wisp_global_prev"),
                        GlobalRef::Next => module.get_global("wisp_global_next"),
                    }
                    .expect("Invalid global name");
                    let value = Self::get_var(refs, vref)?;
                    Self::build(builder, "store_global", vref, |b, _| {
                        b.build_store(global.as_pointer_value(), value)
                    })?;
                }
                LoadFunctionInput(vref, idx) => {
                    let arg = Self::get_argument(function, idx.0)?;
                    refs.vars.insert(*vref, arg);
                }
                StoreFunctionOutput(idx, vref) => {
                    let value = Self::get_var(refs, vref)?;
                    let out = refs.outputs.get_mut(idx.0 as usize).ok_or_else(|| {
                        SignalProcessCreationError::InvalidNumberOfOutputs(
                            func.name().to_owned(),
                            func.outputs().len() as u32,
                            idx.0,
                        )
                    })?;
                    *out = Some(value);
                }
                BinaryOp(vref, type_, op1, op2) => {
                    let left = self.resolve_operand(refs, op1)?;
                    let right = self.resolve_operand(refs, op2)?;
                    use crate::wisp::ir::BinaryOpType::*;
                    let res = match type_ {
                        Add => Self::build(builder, "binop_add", vref, |b, n| {
                            b.build_float_add(left, right, n)
                        }),
                        Subtract => Self::build(builder, "binop_sub", vref, |b, n| {
                            b.build_float_sub(left, right, n)
                        }),
                        Multiply => Self::build(builder, "binop_mul", vref, |b, n| {
                            b.build_float_mul(left, right, n)
                        }),
                        Divide => Self::build(builder, "binop_div", vref, |b, n| {
                            b.build_float_div(left, right, n)
                        }),
                    }?;
                    refs.vars.insert(*vref, res.as_basic_value_enum());
                }
                ComparisonOp(vref, type_, op1, op2) => {
                    let left = self.resolve_operand(refs, op1)?;
                    let right = self.resolve_operand(refs, op2)?;
                    use crate::wisp::ir::ComparisonOpType::*;
                    let res = Self::build(builder, "compop_eq", vref, |b, n| {
                        b.build_float_compare(
                            match type_ {
                                Equal => inkwell::FloatPredicate::OEQ,
                                NotEqual => inkwell::FloatPredicate::ONE,
                                Less => inkwell::FloatPredicate::OLT,
                                LessOrEqual => inkwell::FloatPredicate::OLE,
                                Greater => inkwell::FloatPredicate::OGT,
                                GreaterOrEqual => inkwell::FloatPredicate::OGE,
                            },
                            left,
                            right,
                            n,
                        )
                    })?;
                    refs.vars.insert(*vref, res.as_basic_value_enum());
                }
                Conditional(vref, then_branch, else_branch) => {
                    // Generate code
                    let cond = Self::get_var(refs, vref)?.into_int_value();
                    let (then_block_first, then_block_last) = self.translate_instructions(
                        then_branch,
                        module,
                        builder,
                        func,
                        function,
                        runtime,
                        refs,
                    )?;
                    let (else_block_first, else_block_last) = self.translate_instructions(
                        else_branch,
                        module,
                        builder,
                        func,
                        function,
                        runtime,
                        refs,
                    )?;

                    // Tie blocks together
                    builder.position_at_end(current_block);
                    Self::build(builder, "cond", vref, |b, _| {
                        b.build_conditional_branch(cond, then_block_first, else_block_first)
                    })?;

                    let next_block = self.context.append_basic_block(function, "post_cond");

                    builder.position_at_end(then_block_last);
                    Self::build(builder, "then_jump", vref, |b, _| {
                        b.build_unconditional_branch(next_block)
                    })?;

                    builder.position_at_end(else_block_last);
                    Self::build(builder, "else_jump", vref, |b, _| {
                        b.build_unconditional_branch(next_block)
                    })?;

                    current_block = next_block;
                    builder.position_at_end(current_block);
                }
                Call(name, in_vrefs, out_vrefs) => {
                    let func = Self::get_function(runtime, name)?;
                    if in_vrefs.len() != func.inputs().len() {
                        return Err(SignalProcessCreationError::InvalidNumberOfInputs(
                            name.into(),
                            func.inputs().len() as u32,
                            in_vrefs.len() as u32,
                        ));
                    }
                    if out_vrefs.len() > func.outputs().len() {
                        return Err(SignalProcessCreationError::InvalidNumberOfOutputs(
                            name.into(),
                            func.outputs().len() as u32,
                            out_vrefs.len() as u32,
                        ));
                    }
                    let args = in_vrefs
                        .iter()
                        .map(|vref| Self::get_var(refs, vref))
                        .collect::<Result<Vec<_>, SignalProcessCreationError>>()?
                        .into_iter()
                        .map(|v| BasicMetadataValueEnum::FloatValue(v.into_float_value()))
                        .collect::<Vec<_>>();

                    let func_value = module
                        .get_function(name)
                        .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.clone()))?;

                    let call_site = Self::build(builder, "call", &VarRef(0), |b, n| {
                        b.build_call(func_value, &args, n)
                    })?;
                    let res = call_site.as_any_value_enum();
                    match func.outputs().len() {
                        0 => { /* do nothing */ }
                        1 => {
                            refs.vars
                                .insert(out_vrefs[0], res.into_float_value().as_basic_value_enum());
                        }
                        _ => todo!(),
                    }
                }
                Output(idx, vref) => {
                    let pp_output = module
                        .get_global("wisp_global_output")
                        .expect("Invalid global name");
                    let p_output = Self::build(builder, "load_output", vref, |b, n| {
                        b.build_load(
                            self.context.f32_type().ptr_type(AddressSpace::default()),
                            pp_output.as_pointer_value(),
                            n,
                        )
                    })?;
                    let output = unsafe {
                        p_output.into_pointer_value().const_gep(
                            self.context.f32_type(),
                            &[self.context.i32_type().const_int(idx.0 as u64, false)],
                        )
                    };
                    let value = Self::get_var(refs, vref)?.into_float_value();
                    Self::build(builder, "output", vref, |b, _| b.build_store(output, value))?;
                }
                Debug(vref) => {
                    // NOTE: This duplicates Call()
                    let args = [vref]
                        .iter()
                        .map(|vref| Self::get_var(refs, vref))
                        .collect::<Result<Vec<_>, SignalProcessCreationError>>()?
                        .into_iter()
                        .map(|v| BasicMetadataValueEnum::FloatValue(v.into_float_value()))
                        .collect::<Vec<_>>();

                    let func_value = module.get_function("wisp_debug").ok_or_else(|| {
                        SignalProcessCreationError::UnknownFunction("wisp_debug".into())
                    })?;

                    Self::build(builder, "call", &VarRef(0), |b, n| {
                        b.build_call(func_value, &args, n)
                    })?;
                }
            }
        }
        Ok((start_block, current_block))
    }

    fn get_function<'runtime>(
        runtime: &'runtime Runtime,
        name: &str,
    ) -> Result<&'runtime Function, SignalProcessCreationError> {
        runtime
            .get_function(name)
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.into()))
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
        refs: &FunctionRefs<'ctx>,
        vref: &VarRef,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        Ok(*refs
            .vars
            .get(vref)
            .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?)
    }

    fn get_local<'ctx>(
        refs: &FunctionRefs<'ctx>,
        lref: &LocalRef,
    ) -> Result<PointerValue<'ctx>, SignalProcessCreationError> {
        Ok(*refs
            .locals
            .get(lref)
            .ok_or(SignalProcessCreationError::UninitializedLocal(lref.0))?)
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
        refs: &FunctionRefs<'ctx>,
        op1: &Operand,
    ) -> Result<FloatValue<'ctx>, SignalProcessCreationError> {
        use crate::wisp::ir::Operand::*;
        Ok(match op1 {
            Constant(v) => self.context.f32_type().const_float(*v as f64),
            Var(vref) => Self::get_var(refs, vref)?.into_float_value(),
        })
    }
}
