use std::collections::HashMap;

use inkwell::{
    basic_block::BasicBlock,
    context::Context,
    execution_engine::ExecutionEngine,
    passes::PassBuilderOptions,
    targets::{CodeModel, RelocMode, Target, TargetMachine},
    values::{AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum},
    OptimizationLevel,
};

use crate::wisp::{
    function::{DefaultInputValue, Function},
    ir::{Instruction, Operand},
    runtime::Runtime,
};

use super::{
    error::SignalProcessCreationError,
    function_context::FunctionContext,
    module_context::ModuleContext,
    processor::{SignalProcessor, SignalProcessorContext},
};

pub struct SignalProcessorBuilder {
    id_gen: u64,
    context: Context,
}

impl SignalProcessorBuilder {
    pub fn new() -> Self {
        SignalProcessorBuilder {
            id_gen: 0,
            context: Context::create(),
        }
    }

    pub fn create_signal_processor<'ctx>(
        &'ctx mut self,
        top_level: &str,
        runtime: &Runtime,
    ) -> Result<(SignalProcessor, ExecutionEngine<'ctx>), SignalProcessCreationError> {
        self.id_gen += 1;

        let module = self.context.create_module(&format!("wisp_{}", self.id_gen));
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .map_err(|_| SignalProcessCreationError::InitEE)?;

        let mut mctx = ModuleContext::new(&self.context, runtime, &module, HashMap::new());

        let g_data = module.add_global(mctx.types.pf32, None, "wisp_global_data");
        let g_output = module.add_global(mctx.types.pf32, None, "wisp_global_output");
        let g_wisp_debug = module.add_function(
            "wisp_debug",
            mctx.types.void.fn_type(&[mctx.types.f32.into(); 1], false),
            None,
        );
        let mut spctx = Box::new(SignalProcessorContext {
            p_data: std::ptr::null_mut(),
            p_output: std::ptr::null_mut(),
        });
        execution_engine.add_global_mapping(&g_data, &mut spctx.p_data as *mut _ as usize);
        execution_engine.add_global_mapping(&g_output, &mut spctx.p_output as *mut _ as usize);
        extern "C" fn wisp_debug(v: f32) {
            eprintln!("Debug: {}", v);
        }
        execution_engine.add_global_mapping(&g_wisp_debug, wisp_debug as usize);

        for (name, func) in runtime.functions_iter() {
            // >1 returns is currently not supported
            assert!(func.outputs().len() < 2);
            let mut arg_types = vec![mctx.types.f32.into(); func.inputs().len()];
            if !func.data().is_empty() {
                arg_types.push(mctx.types.pf32.into());
            }
            let fn_type = if func.outputs().len() == 1 {
                mctx.types.f32.fn_type(&arg_types, false)
            } else {
                mctx.types.void.fn_type(&arg_types, false)
            };
            module.add_function(name, fn_type, None);
        }

        for (_, func) in runtime.functions_iter() {
            self.build_function(&mut mctx, func)?;
        }

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

        let function = unsafe { execution_engine.get_function(top_level) }
            .map_err(|_| SignalProcessCreationError::LoadFunction)?;
        Ok((
            SignalProcessor::new(
                spctx,
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
            use crate::wisp::ir::Instruction::*;
            match insn {
                AllocLocal(lref) => {
                    let local = mctx.build("alloc_local", |b, _| {
                        b.build_alloca(mctx.types.f32, &format!("local_{}", lref.0))
                    })?;
                    fctx.locals.insert(*lref, local);
                }
                Load(vref, loc) => {
                    use crate::wisp::ir::SourceLocation::*;
                    let value = match loc {
                        Local(lref) => {
                            let local = fctx.get_local(lref)?;
                            mctx.build("load_local", |b, n| b.build_load(mctx.types.f32, local, n))?
                        }
                        Data(dref) => {
                            let p_data = fctx.get_argument(fctx.func.inputs().len() as u32)?;
                            let p_data_item = unsafe {
                                p_data.into_pointer_value().const_gep(
                                    mctx.types.f32,
                                    &[mctx.types.i32.const_int(dref.0 as u64, false)],
                                )
                            };
                            mctx.build("load_data_item", |b, n| {
                                b.build_load(mctx.types.f32, p_data_item, n)
                            })?
                        }
                        LastValue(id, dref) => {
                            // TODO: Remove duplication with Call() and LoadData()
                            let idx = if let Some(idx) = mctx.data_indices.get(id) {
                                *idx
                            } else {
                                let idx = mctx.data_indices.len() as u32;
                                mctx.data_indices.insert(*id, idx);
                                idx
                            };
                            let pp_global_data = mctx
                                .module
                                .get_global("wisp_global_data")
                                .expect("Invalid global name");
                            let p_global_data = mctx.build("load_global_data", |b, n| {
                                b.build_load(mctx.types.pf32, pp_global_data.as_pointer_value(), n)
                            })?;
                            let p_func_data = unsafe {
                                p_global_data.into_pointer_value().const_gep(
                                    mctx.types.f32,
                                    &[mctx.types.i32.const_int(idx as u64, false)],
                                )
                            };
                            // LoadData
                            let p_data_item = unsafe {
                                p_func_data.const_gep(
                                    mctx.types.f32,
                                    &[mctx.types.i32.const_int(dref.0 as u64, false)],
                                )
                            };
                            mctx.build("load_data_item", |b, n| {
                                b.build_load(mctx.types.f32, p_data_item, n)
                            })?
                        }
                    };
                    fctx.vars.insert(*vref, value);
                }
                Store(loc, op) => {
                    let value = Self::resolve_operand(mctx, fctx, op)?;
                    use crate::wisp::ir::TargetLocation::*;
                    match loc {
                        Local(lref) => {
                            let local = fctx.get_local(lref)?;
                            mctx.build("store_local", |b, _| b.build_store(local, value))?;
                        }
                        Data(dref) => {
                            let p_data = fctx.get_argument(fctx.func.inputs().len() as u32)?;
                            let p_data_item = unsafe {
                                p_data.into_pointer_value().const_gep(
                                    mctx.types.f32,
                                    &[mctx.types.i32.const_int(dref.0 as u64, false)],
                                )
                            };
                            mctx.build("store_data_item", |b, _| {
                                b.build_store(p_data_item, value)
                            })?;
                        }
                        FunctionOutput(idx) => {
                            let out = fctx.outputs.get_mut(idx.0 as usize).ok_or_else(|| {
                                SignalProcessCreationError::InvalidNumberOfOutputs(
                                    fctx.func.name().to_owned(),
                                    fctx.func.outputs().len() as u32,
                                    idx.0,
                                )
                            })?;
                            *out = Some(value.as_basic_value_enum());
                        }
                        SignalOutput(idx) => {
                            let pp_output = mctx
                                .module
                                .get_global("wisp_global_output")
                                .expect("Invalid global name");
                            let p_output = mctx.build("load_output", |b, n| {
                                b.build_load(mctx.types.pf32, pp_output.as_pointer_value(), n)
                            })?;
                            let output = unsafe {
                                p_output.into_pointer_value().const_gep(
                                    mctx.types.f32,
                                    &[mctx.types.i32.const_int(idx.0 as u64, false)],
                                )
                            };
                            mctx.build("output", |b, _| b.build_store(output, value))?;
                        }
                    }
                }
                BinaryOp(vref, type_, op1, op2) => {
                    let left = Self::resolve_operand(mctx, fctx, op1)?.into_float_value();
                    let right = Self::resolve_operand(mctx, fctx, op2)?.into_float_value();
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
                    let left = Self::resolve_operand(mctx, fctx, op1)?.into_float_value();
                    let right = Self::resolve_operand(mctx, fctx, op2)?.into_float_value();
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
                Call(id, name, in_vrefs, out_vrefs) => {
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
                            Some(op) => Self::resolve_operand(mctx, fctx, op)?,
                            None => match func.inputs()[idx].fallback {
                                Some(fallback) => match fallback {
                                    DefaultInputValue::Normal => {
                                        args[idx - 1].into_float_value().as_basic_value_enum()
                                    }
                                    DefaultInputValue::Value(v) => {
                                        mctx.types.f32.const_float(v as f64).as_basic_value_enum()
                                    }
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
                        let idx = if let Some(idx) = mctx.data_indices.get(id) {
                            *idx
                        } else {
                            let idx = mctx.data_indices.len() as u32;
                            mctx.data_indices.insert(*id, idx);
                            idx
                        };
                        let pp_global_data = mctx
                            .module
                            .get_global("wisp_global_data")
                            .expect("Invalid global name");
                        let p_global_data = mctx.build("load_global_data", |b, n| {
                            b.build_load(mctx.types.pf32, pp_global_data.as_pointer_value(), n)
                        })?;
                        let p_func_data = unsafe {
                            p_global_data.into_pointer_value().const_gep(
                                mctx.types.f32,
                                &[mctx.types.i32.const_int(idx as u64, false)],
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
        mctx: &ModuleContext<'ctx, '_>,
        fctx: &FunctionContext<'ctx, '_>,
        op: &Operand,
    ) -> Result<BasicValueEnum<'ctx>, SignalProcessCreationError> {
        use crate::wisp::ir::Operand::*;
        Ok(match op {
            Constant(c) => {
                use crate::wisp::ir::Constant::*;
                match c {
                    SampleRate => mctx
                        .types
                        .f32
                        .const_float(mctx.runtime.sample_rate() as f64)
                        .as_basic_value_enum(),
                }
            }
            Literal(v) => mctx.types.f32.const_float(*v as f64).as_basic_value_enum(),
            Var(vref) => fctx.get_var(vref)?,
            Arg(idx) => fctx.get_argument(*idx)?,
        })
    }
}
