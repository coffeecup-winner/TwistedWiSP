use std::collections::HashMap;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::execution_engine::ExecutionEngine;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{CodeModel, RelocMode, Target, TargetMachine};
use inkwell::values::{
    AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum, FunctionValue, PointerValue,
};
use inkwell::{AddressSpace, OptimizationLevel};
use thiserror::Error;

use crate::wisp::function::DefaultInputValue;

use super::flow::Flow;
use super::function::{Function, FunctionInput};
use super::ir::{GlobalRef, Instruction, LocalRef, Operand, VarRef};
use super::runtime::Runtime;

struct Globals {
    p_data: *mut f32,
    p_output: *mut f32,
}
unsafe impl Send for Globals {}
unsafe impl Sync for Globals {}

type ProcessFn = unsafe extern "C" fn(data: *mut f32, output: *mut f32);

pub struct SignalProcessor {
    _globals: Box<Globals>,
    function: ProcessFn,
    num_outputs: usize,
    data: Vec<f32>,
}

impl SignalProcessor {
    fn new(
        globals: Box<Globals>,
        function: ProcessFn,
        num_outputs: usize,
        data_length: usize,
    ) -> Self {
        SignalProcessor {
            _globals: globals,
            function,
            num_outputs,
            data: vec![0.0; data_length],
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
        unsafe {
            (self.function)(self.data.as_mut_ptr(), output.as_mut_ptr());
        }
    }

    #[allow(dead_code)]
    pub fn data(&self) -> &[f32] {
        &self.data
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

    #[error("Required input {1} for function {0} was not initialized")]
    UninitializedInput(String, u32),

    #[error("Invalid number of outputs for function {0}: expected at most {1}, found {2}")]
    InvalidNumberOfOutputs(String, u32, u32),

    #[error("Output {1} for function {0} was not initialized")]
    UninitializedOutput(String, u32),

    #[error("Logical error: {0}")]
    CustomLogicalError(String),
}

#[derive(Debug)]
struct ModuleContext<'ctx, 'temp> {
    runtime: &'temp Runtime,
    module: &'temp Module<'ctx>,
    builder: &'temp Builder<'ctx>,
    data_indices: HashMap<String, u32>,
}

impl<'ctx, 'temp> ModuleContext<'ctx, 'temp> {
    fn new(
        runtime: &'temp Runtime,
        module: &'temp Module<'ctx>,
        builder: &'temp Builder<'ctx>,
        data_indices: HashMap<String, u32>,
    ) -> Self {
        ModuleContext {
            runtime,
            module,
            builder,
            data_indices,
        }
    }

    fn get_function(&self, name: &str) -> Result<&'temp Function, SignalProcessCreationError> {
        self.runtime
            .get_function(name)
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.into()))
    }

    fn build<F, R>(&self, name: &str, func: F) -> Result<R, SignalProcessCreationError>
    where
        F: FnOnce(&Builder<'ctx>, &str) -> Result<R, BuilderError>,
    {
        func(self.builder, &format!("tmp_{}", name))
            .map_err(|_| SignalProcessCreationError::BuildInstruction(name.into()))
    }
}

#[derive(Debug)]
struct FunctionContext<'ctx, 'temp> {
    func: &'temp Function,
    function: FunctionValue<'ctx>,
    outputs: Vec<Option<BasicValueEnum<'ctx>>>,
    vars: HashMap<VarRef, BasicValueEnum<'ctx>>,
    locals: HashMap<LocalRef, PointerValue<'ctx>>,
}

impl<'ctx, 'temp> FunctionContext<'ctx, 'temp> {
    fn new(func: &'temp Function, function: FunctionValue<'ctx>, num_outputs: usize) -> Self {
        FunctionContext {
            func,
            function,
            outputs: vec![None; num_outputs],
            vars: HashMap::new(),
            locals: HashMap::new(),
        }
    }

    fn get_argument(&self, nth: u32) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        self.function.get_nth_param(nth).ok_or_else(|| {
            SignalProcessCreationError::CustomLogicalError(
                "Invalid number of function arguments".into(),
            )
        })
    }

    fn get_var(&self, vref: &VarRef) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        Ok(*self
            .vars
            .get(vref)
            .ok_or(SignalProcessCreationError::UninitializedVar(vref.0))?)
    }

    fn get_local(&self, lref: &LocalRef) -> Result<PointerValue<'ctx>, SignalProcessCreationError> {
        Ok(*self
            .locals
            .get(lref)
            .ok_or(SignalProcessCreationError::UninitializedLocal(lref.0))?)
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

    pub fn create_signal_processor<'ctx>(
        &'ctx mut self,
        flow: &Flow,
        runtime: &Runtime,
    ) -> Result<(SignalProcessor, ExecutionEngine<'ctx>), SignalProcessCreationError> {
        self.id_gen += 1;

        let module = self.context.create_module(&format!("wisp_{}", self.id_gen));
        let builder = self.context.create_builder();
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .map_err(|_| SignalProcessCreationError::InitEE)?;

        let f32_type = self.context.f32_type();
        let pf32_type = f32_type.ptr_type(AddressSpace::default());
        let g_data = module.add_global(pf32_type, None, "wisp_global_data");
        let g_output = module.add_global(pf32_type, None, "wisp_global_output");
        let g_wisp_debug = module.add_function(
            "wisp_debug",
            self.context
                .void_type()
                .fn_type(&[f32_type.into(); 1], false),
            None,
        );
        let mut globals = Box::new(Globals {
            p_data: std::ptr::null_mut(),
            p_output: std::ptr::null_mut(),
        });
        execution_engine.add_global_mapping(&g_data, &mut globals.p_data as *mut _ as usize);
        execution_engine.add_global_mapping(&g_output, &mut globals.p_output as *mut _ as usize);
        extern "C" fn wisp_debug(v: f32) {
            eprintln!("Debug: {}", v);
        }
        execution_engine.add_global_mapping(&g_wisp_debug, wisp_debug as usize);

        for (name, func) in runtime.functions_iter() {
            // >1 returns is currently not supported
            assert!(func.outputs().len() < 2);
            let mut arg_types = vec![f32_type.into(); func.inputs().len()];
            if !func.data().is_empty() {
                arg_types.push(f32_type.ptr_type(AddressSpace::default()).into());
            }
            let fn_type = if func.outputs().len() == 1 {
                f32_type.fn_type(&arg_types, false)
            } else {
                self.context.void_type().fn_type(&arg_types, false)
            };
            module.add_function(name, fn_type, None);
        }

        let mut mctx = ModuleContext::new(runtime, &module, &builder, HashMap::new());
        for (_, func) in runtime.functions_iter() {
            self.build_function(&mut mctx, func)?;
        }

        let mut process_func_instructions = vec![
            Instruction::StoreGlobal(GlobalRef::Data, Operand::Arg(0)),
            Instruction::StoreGlobal(GlobalRef::Output, Operand::Arg(1)),
        ];
        process_func_instructions.extend(flow.get_compiled_flow(runtime).iter().cloned());
        let func = Function::new(
            "wisp_process".into(),
            vec![FunctionInput::default(); 2],
            vec![],
            vec![],
            process_func_instructions,
            None,
        );

        module.add_function(
            func.name(),
            self.context
                .void_type()
                .fn_type(&[pf32_type.into(); 3], false),
            None,
        );

        self.build_function(&mut mctx, &func)?;

        if cfg!(debug_assertions) {
            eprintln!("===== BEFORE =====");
            module.print_to_stderr();
        }

        // TODO: Enable optimization passes
        if false {
            let target = Target::from_triple(&TargetMachine::get_default_triple())
                .expect("Failed to create LLVM target");
            let target_machine = target
                .create_target_machine(
                    &TargetMachine::get_default_triple(),
                    &TargetMachine::get_host_cpu_name().to_string(),
                    &TargetMachine::get_host_cpu_features().to_string(),
                    OptimizationLevel::None,
                    RelocMode::Default,
                    CodeModel::JITDefault,
                )
                .expect("Failed to create LLVM target machine");

            let options = PassBuilderOptions::create();
            options.set_merge_functions(true);
            module
                .run_passes("inline,mem2reg", &target_machine, options)
                .expect("Failed to run optimization passes");

            if cfg!(debug_assertions) {
                eprintln!("===== AFTER =====");
                module.print_to_stderr();
            }
        }

        let function = unsafe { execution_engine.get_function("wisp_process") }
            .map_err(|_| SignalProcessCreationError::LoadFunction)?;
        Ok((
            SignalProcessor::new(
                globals,
                unsafe { function.into_raw() },
                runtime.num_outputs() as usize,
                mctx.data_indices.len(),
            ),
            execution_engine,
        ))
    }

    fn build_function<'ctx>(
        &'ctx self,
        mctx: &mut ModuleContext<'ctx, '_>,
        func: &Function,
    ) -> Result<(), SignalProcessCreationError> {
        let function = mctx
            .module
            .get_function(func.name())
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(func.name().to_owned()))?;
        let mut fctx = FunctionContext::new(func, function, func.outputs().len());
        self.translate_instructions(mctx, &mut fctx, func.instructions())?;
        if !fctx.outputs.iter().all(|o| o.is_some()) {
            return Err(SignalProcessCreationError::UninitializedOutput(
                func.name().to_owned(),
                fctx.outputs
                    .iter()
                    .enumerate()
                    .find(|(_, o)| o.is_none())
                    .unwrap()
                    .0 as u32,
            ));
        }
        match func.outputs().len() {
            0 => {
                mctx.builder
                    .build_return(None)
                    .map_err(|_| SignalProcessCreationError::BuildInstruction("return".into()))?;
            }
            1 => {
                mctx.builder
                    .build_return(Some(&fctx.outputs[0].expect("Invalid function output")))
                    .map_err(|_| SignalProcessCreationError::BuildInstruction("return".into()))?;
            }
            _ => todo!(),
        }
        Ok(())
    }

    fn translate_instructions<'ctx>(
        &'ctx self,
        mctx: &mut ModuleContext<'ctx, '_>,
        fctx: &mut FunctionContext<'ctx, '_>,
        instructions: &[Instruction],
    ) -> Result<(BasicBlock<'ctx>, BasicBlock<'ctx>), SignalProcessCreationError> {
        let mut current_block = self.context.append_basic_block(fctx.function, "start");
        mctx.builder.position_at_end(current_block);
        let start_block = current_block;

        for insn in instructions {
            use super::ir::Instruction::*;
            match insn {
                AllocLocal(lref) => {
                    let local = mctx.build("alloc_local", |b, _| {
                        b.build_alloca(self.context.f32_type(), &format!("local_{}", lref.0))
                    })?;
                    fctx.locals.insert(*lref, local);
                }
                LoadLocal(vref, lref) => {
                    let local = fctx.get_local(lref)?;
                    let value = mctx.build("load_local", |b, n| {
                        b.build_load(self.context.f32_type(), local, n)
                    })?;
                    fctx.vars.insert(*vref, value);
                }
                StoreLocal(lref, op) => {
                    let local = fctx.get_local(lref)?;
                    let value = self.resolve_operand(mctx.runtime, fctx, op)?;
                    mctx.build("store_local", |b, _| b.build_store(local, value))?;
                }
                LoadGlobal(vref, gref) => {
                    let global = match gref {
                        GlobalRef::Data => mctx.module.get_global("wisp_global_data"),
                        GlobalRef::Output => mctx.module.get_global("wisp_global_output"),
                    }
                    .expect("Invalid global name");
                    let value = mctx.build("load_global", |b, n| {
                        b.build_load(
                            self.context.f32_type().ptr_type(AddressSpace::default()),
                            global.as_pointer_value(),
                            n,
                        )
                    })?;
                    fctx.vars.insert(*vref, value);
                }
                StoreGlobal(gref, op) => {
                    let global = match gref {
                        GlobalRef::Data => mctx.module.get_global("wisp_global_data"),
                        GlobalRef::Output => mctx.module.get_global("wisp_global_output"),
                    }
                    .expect("Invalid global name");
                    let value = self.resolve_operand(mctx.runtime, fctx, op)?;
                    mctx.build("store_global", |b, _| {
                        b.build_store(global.as_pointer_value(), value)
                    })?;
                }
                LoadData(vref, dref) => {
                    let p_data = fctx.get_argument(fctx.func.inputs().len() as u32)?;
                    let p_data_item = unsafe {
                        p_data.into_pointer_value().const_gep(
                            self.context.f32_type(),
                            &[self.context.i32_type().const_int(dref.0 as u64, false)],
                        )
                    };
                    let data_item = mctx.build("load_data_item", |b, n| {
                        b.build_load(self.context.f32_type(), p_data_item, n)
                    })?;
                    fctx.vars.insert(*vref, data_item);
                }
                StoreData(dref, op) => {
                    let p_data = fctx.get_argument(fctx.func.inputs().len() as u32)?;
                    let p_data_item = unsafe {
                        p_data.into_pointer_value().const_gep(
                            self.context.f32_type(),
                            &[self.context.i32_type().const_int(dref.0 as u64, false)],
                        )
                    };
                    let value = self.resolve_operand(mctx.runtime, fctx, op)?;
                    mctx.build("store_data_item", |b, _| b.build_store(p_data_item, value))?;
                }
                StoreFunctionOutput(idx, op) => {
                    let value = self.resolve_operand(mctx.runtime, fctx, op)?;
                    let out = fctx.outputs.get_mut(idx.0 as usize).ok_or_else(|| {
                        SignalProcessCreationError::InvalidNumberOfOutputs(
                            fctx.func.name().to_owned(),
                            fctx.func.outputs().len() as u32,
                            idx.0,
                        )
                    })?;
                    *out = Some(value.as_basic_value_enum());
                }
                BinaryOp(vref, type_, op1, op2) => {
                    let left = self
                        .resolve_operand(mctx.runtime, fctx, op1)?
                        .into_float_value();
                    let right = self
                        .resolve_operand(mctx.runtime, fctx, op2)?
                        .into_float_value();
                    use crate::wisp::ir::BinaryOpType::*;
                    let res = match type_ {
                        Add => mctx.build("binop_add", |b, n| b.build_float_add(left, right, n)),
                        Subtract => {
                            mctx.build("binop_sub", |b, n| b.build_float_sub(left, right, n))
                        }
                        Multiply => {
                            mctx.build("binop_mul", |b, n| b.build_float_mul(left, right, n))
                        }
                        Divide => mctx.build("binop_div", |b, n| b.build_float_div(left, right, n)),
                    }?;
                    fctx.vars.insert(*vref, res.as_basic_value_enum());
                }
                ComparisonOp(vref, type_, op1, op2) => {
                    let left = self
                        .resolve_operand(mctx.runtime, fctx, op1)?
                        .into_float_value();
                    let right = self
                        .resolve_operand(mctx.runtime, fctx, op2)?
                        .into_float_value();
                    use crate::wisp::ir::ComparisonOpType::*;
                    let res = mctx.build("compop_eq", |b, n| {
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
                    fctx.vars.insert(*vref, res.as_basic_value_enum());
                }
                Conditional(vref, then_branch, else_branch) => {
                    // Generate code
                    let cond = fctx.get_var(vref)?.into_int_value();
                    let (then_block_first, then_block_last) =
                        self.translate_instructions(mctx, fctx, then_branch)?;
                    let (else_block_first, else_block_last) =
                        self.translate_instructions(mctx, fctx, else_branch)?;

                    // Tie blocks together
                    mctx.builder.position_at_end(current_block);
                    mctx.build("cond", |b, _| {
                        b.build_conditional_branch(cond, then_block_first, else_block_first)
                    })?;

                    let next_block = self.context.append_basic_block(fctx.function, "post_cond");

                    mctx.builder.position_at_end(then_block_last);
                    mctx.build("then_jump", |b, _| b.build_unconditional_branch(next_block))?;

                    mctx.builder.position_at_end(else_block_last);
                    mctx.build("else_jump", |b, _| b.build_unconditional_branch(next_block))?;

                    current_block = next_block;
                    mctx.builder.position_at_end(current_block);
                }
                Call(_id, name, in_vrefs, out_vrefs) => {
                    let func = mctx.get_function(name)?;
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
                    let mut args: Vec<BasicMetadataValueEnum> = vec![];
                    for (idx, input) in in_vrefs.iter().enumerate() {
                        let value = match input {
                            Some(op) => self.resolve_operand(mctx.runtime, fctx, op)?,
                            None => match func.inputs()[idx].fallback {
                                Some(fallback) => match fallback {
                                    DefaultInputValue::Normal => {
                                        args[idx - 1].into_float_value().as_basic_value_enum()
                                    }
                                    DefaultInputValue::Value(v) => self
                                        .context
                                        .f32_type()
                                        .const_float(v as f64)
                                        .as_basic_value_enum(),
                                },
                                None => {
                                    return Err(SignalProcessCreationError::UninitializedInput(
                                        name.into(),
                                        idx as u32,
                                    ))
                                }
                            },
                        };
                        args.push(BasicMetadataValueEnum::FloatValue(value.into_float_value()));
                    }

                    if !func.data().is_empty() {
                        // TODO: Calculate based on call chain
                        let data_path = "data";
                        let idx = if let Some(idx) = mctx.data_indices.get(data_path) {
                            *idx
                        } else {
                            let idx = mctx.data_indices.len() as u32;
                            mctx.data_indices.insert(data_path.into(), idx);
                            idx
                        };
                        let pp_global_data = mctx
                            .module
                            .get_global("wisp_global_data")
                            .expect("Invalid global name");
                        let p_global_data = mctx.build("load_global_data", |b, n| {
                            b.build_load(
                                self.context.f32_type().ptr_type(AddressSpace::default()),
                                pp_global_data.as_pointer_value(),
                                n,
                            )
                        })?;
                        let p_func_data = unsafe {
                            p_global_data.into_pointer_value().const_gep(
                                self.context.f32_type(),
                                &[self.context.i32_type().const_int(idx as u64, false)],
                            )
                        };
                        args.push(BasicMetadataValueEnum::PointerValue(p_func_data));
                    }

                    let func_value = mctx
                        .module
                        .get_function(name)
                        .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.clone()))?;

                    let call_site =
                        mctx.build("call", |b, n| b.build_call(func_value, &args, n))?;
                    let res = call_site.as_any_value_enum();
                    match func.outputs().len() {
                        0 => { /* do nothing */ }
                        1 => {
                            fctx.vars
                                .insert(out_vrefs[0], res.into_float_value().as_basic_value_enum());
                        }
                        _ => todo!(),
                    }
                }
                LoadLastValue(_id, _name, dref, vref) => {
                    // TODO: Remove duplication with Call() and LoadData()
                    // TODO: Calculate based on call chain
                    let data_path = "data";
                    let idx = if let Some(idx) = mctx.data_indices.get(data_path) {
                        *idx
                    } else {
                        let idx = mctx.data_indices.len() as u32;
                        mctx.data_indices.insert(data_path.into(), idx);
                        idx
                    };
                    let pp_global_data = mctx
                        .module
                        .get_global("wisp_global_data")
                        .expect("Invalid global name");
                    let p_global_data = mctx.build("load_global_data", |b, n| {
                        b.build_load(
                            self.context.f32_type().ptr_type(AddressSpace::default()),
                            pp_global_data.as_pointer_value(),
                            n,
                        )
                    })?;
                    let p_func_data = unsafe {
                        p_global_data.into_pointer_value().const_gep(
                            self.context.f32_type(),
                            &[self.context.i32_type().const_int(idx as u64, false)],
                        )
                    };
                    // LoadData
                    let p_data_item = unsafe {
                        p_func_data.const_gep(
                            self.context.f32_type(),
                            &[self.context.i32_type().const_int(dref.0 as u64, false)],
                        )
                    };
                    let data_item = mctx.build("load_data_item", |b, n| {
                        b.build_load(self.context.f32_type(), p_data_item, n)
                    })?;
                    fctx.vars.insert(*vref, data_item);
                }
                Output(idx, op) => {
                    let pp_output = mctx
                        .module
                        .get_global("wisp_global_output")
                        .expect("Invalid global name");
                    let p_output = mctx.build("load_output", |b, n| {
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
                    let value = self.resolve_operand(mctx.runtime, fctx, op)?;
                    mctx.build("output", |b, _| b.build_store(output, value))?;
                }
                Debug(vref) => {
                    // NOTE: This duplicates Call()
                    let args = [vref]
                        .iter()
                        .map(|vref| fctx.get_var(vref))
                        .collect::<Result<Vec<_>, SignalProcessCreationError>>()?
                        .into_iter()
                        .map(|v| BasicMetadataValueEnum::FloatValue(v.into_float_value()))
                        .collect::<Vec<_>>();

                    let func_value = mctx.module.get_function("wisp_debug").ok_or_else(|| {
                        SignalProcessCreationError::UnknownFunction("wisp_debug".into())
                    })?;

                    mctx.build("call", |b, n| b.build_call(func_value, &args, n))?;
                }
            }
        }
        Ok((start_block, current_block))
    }

    fn resolve_operand<'ctx>(
        &'ctx self,
        runtime: &Runtime,
        fctx: &FunctionContext<'ctx, '_>,
        op: &Operand,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        use crate::wisp::ir::Operand::*;
        Ok(match op {
            Constant(c) => {
                use crate::wisp::ir::Constant::*;
                match c {
                    SampleRate => self
                        .context
                        .f32_type()
                        .const_float(runtime.sample_rate() as f64)
                        .as_basic_value_enum(),
                }
            }
            Literal(v) => self
                .context
                .f32_type()
                .const_float(*v as f64)
                .as_basic_value_enum(),
            Var(vref) => fctx.get_var(vref)?,
            Arg(idx) => fctx.get_argument(*idx)?,
        })
    }
}
