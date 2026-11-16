use inkwell::{
    basic_block::BasicBlock,
    intrinsics::Intrinsic,
    passes::PassBuilderOptions,
    targets::{CodeModel, RelocMode, Target, TargetMachine},
    types::{BasicMetadataTypeEnum, BasicType},
    values::{AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum},
    OptimizationLevel,
};
use log::debug;
use rand::Rng;

use crate::{
    compiler::dependency_calculator,
    core::WispContext,
    ir::{IRFunction, IRFunctionDataType, Instruction, Operand},
    runner::{context::WispRuntimeContext, runtime::WispExecutionContext},
    CallIndex,
};

use super::{
    data_layout::DataLayout,
    error::SignalProcessCreationError,
    function_context::FunctionContext,
    module_context::ModuleContext,
    processor::{SignalProcessor, SignalProcessorContext},
};

pub struct SignalProcessorBuilder {
    id_gen: u64,
}

impl SignalProcessorBuilder {
    pub fn new() -> Self {
        SignalProcessorBuilder { id_gen: 0 }
    }

    pub fn build_signal_processor(
        &mut self,
        ctx: &WispContext,
        ectx: &WispExecutionContext,
        rctx: &mut WispRuntimeContext,
        top_level: &str,
    ) -> Result<SignalProcessor, SignalProcessCreationError> {
        self.id_gen += 1;

        // Signal processor compilation is done in phases. Every next phase is dependent on the previous one.
        // The intermediate results of each phase are stored in the runtime context and reused when possible.

        // Phase 1: Calculate the dependencies of each function

        for (_, func) in rctx.functions_iter() {
            func.dependencies().update(|dep_handle| {
                let ir_func = func.ir_function().get(dep_handle);
                dependency_calculator::calculate_dependencies(&ir_func)
            });
        }

        // Phase 2: Calculate the active set of functions to compile

        rctx.active_set().update(|dep_handle| {
            dependency_calculator::calculate_active_set(rctx, top_level, dep_handle)
        });

        // Phase 3: Calculate the data layout

        // Calculate the data layout for each function in reverse order, so that children data sizes and offsets are calculated first
        for name in rctx.active_set().get_untracked().iter().rev() {
            let func = rctx.get_function(name).unwrap();
            func.data_layout().update(|dep_handle| {
                let ir_func = func.ir_function().get(dep_handle.clone());
                DataLayout::calculate_function_data_layout(&ir_func, rctx, dep_handle)
            });
        }

        // Calculate the combined data layout
        rctx.data_layout()
            .update(|dep_handle| DataLayout::new(rctx, dep_handle));

        let module = ectx.llvm().create_module(&format!("wisp_{}", self.id_gen));
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .map_err(|_| SignalProcessCreationError::InitEE)?;

        let mut mctx = ModuleContext::new(ectx.llvm(), ctx, rctx, &module);

        let g_output = module.add_global(ectx.pf32_t(), None, "wisp_global_output");
        let g_empty_array = module.add_global(ectx.pf32_t(), None, "wisp_global_empty_array");
        let g_wisp_debug = module.add_function(
            "wisp_debug",
            ectx.void_t().fn_type(&[ectx.f32_t().into(); 1], false),
            None,
        );
        let g_noise = module.add_function("noise", ectx.f32_t().fn_type(&[], false), None);
        let mut spctx = Box::new(SignalProcessorContext {
            p_output: std::ptr::null_mut(),
        });
        execution_engine.add_global_mapping(&g_output, &mut spctx.p_output as *mut _ as usize);
        execution_engine.add_global_mapping(&g_empty_array, [0u32].as_ptr() as usize);
        extern "C" fn wisp_debug(v: f32) {
            debug!("Debug: {}", v);
        }
        execution_engine.add_global_mapping(&g_wisp_debug, wisp_debug as usize);
        extern "C" fn noise() -> f32 {
            rand::thread_rng().gen_range(-1.0..=1.0)
        }
        execution_engine.add_global_mapping(&g_noise, noise as usize);

        for name in rctx.active_set().get_untracked().iter() {
            let ir_func = rctx
                .get_function(name)
                .unwrap()
                .ir_function()
                .get_untracked();

            // >1 returns is currently not supported
            assert!(ir_func.outputs().len() < 2);
            let mut arg_types: Vec<BasicMetadataTypeEnum> = vec![];
            if rctx
                .get_function(name)
                .unwrap()
                .data_layout()
                .get_untracked()
                .is_some()
            {
                arg_types.push(ectx.pf32_t().into());
            }
            arg_types.extend(
                ir_func
                    .inputs()
                    .iter()
                    .map(|i| match i.type_ {
                        IRFunctionDataType::Float => ectx.f32_t().into(),
                        IRFunctionDataType::Array => ectx.pf32_t().into(),
                    })
                    .collect::<Vec<BasicMetadataTypeEnum>>(),
            );
            let fn_type = if ir_func.outputs().len() == 1 {
                match ir_func.outputs()[0].type_ {
                    IRFunctionDataType::Float => ectx.f32_t().fn_type(&arg_types, false),
                    IRFunctionDataType::Array => ectx.pf32_t().fn_type(&arg_types, false),
                }
            } else {
                ectx.void_t().fn_type(&arg_types, false)
            };
            module.add_function(name, fn_type, None);
        }

        for name in rctx.active_set().get_untracked().iter() {
            let func = rctx.get_function(name).unwrap();
            let ir_func = func.ir_function().get_untracked();

            // TODO: Instead, check explicitly for builtin functions using a function attribute
            if !ir_func.ir.is_empty() {
                self.build_function(rctx, ectx, &mut mctx, &ir_func)?;
            }
        }

        let entry = mctx.module.add_function(
            "wisp_entry",
            ectx.void_t().fn_type(&[ectx.pf32_t().into()], false),
            None,
        );
        let bb = ectx.llvm().append_basic_block(entry, "entry");
        mctx.builder.position_at_end(bb);
        let p_data = entry.get_first_param().ok_or_else(|| {
            SignalProcessCreationError::InvalidNumberOfInputs("wisp_entry".into(), 1, 0)
        })?;
        mctx.build("entry", |b, n| {
            b.build_call(
                mctx.module.get_function(top_level).unwrap(),
                &[BasicMetadataValueEnum::PointerValue(
                    p_data.into_pointer_value(),
                )],
                n,
            )
        })?;
        mctx.build("exit", |b, _| b.build_return(None))?;

        if cfg!(debug_assertions) {
            debug!("===== BEFORE =====");
            module.print_to_stderr();
        }

        // Optimization passes
        {
            // TODO: Cache target machine?
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

            // TODO: This crashes LLVM sometimes, investigate
            // const PASSES: &str = "inline,mem2reg,instcombine,gvn";
            const PASSES: &str = "mem2reg";
            module
                .run_passes(PASSES, &target_machine, options)
                .expect("Failed to run optimization passes");

            if cfg!(debug_assertions) {
                debug!("===== AFTER =====");
                module.print_to_stderr();
            }
        }

        let function = unsafe { execution_engine.get_function("wisp_entry") }
            .map_err(|_| SignalProcessCreationError::LoadFunction)?;
        Ok(SignalProcessor::new(
            execution_engine,
            spctx,
            top_level,
            unsafe { function.into_raw() },
            rctx.data_layout().get_untracked().clone(),
            ctx.num_outputs(),
        ))
    }

    fn build_function<'ectx>(
        &self,
        rctx: &WispRuntimeContext,
        ectx: &'ectx WispExecutionContext,
        mctx: &mut ModuleContext<'ectx, '_>,
        func: &IRFunction,
    ) -> Result<(), SignalProcessCreationError> {
        let function = mctx
            .module
            .get_function(func.name())
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(func.name().to_owned()))?;
        let data_arg = if rctx
            .get_function(func.name())
            .unwrap()
            .data_layout()
            .get_untracked()
            .is_some()
        {
            Some(
                function
                    .get_first_param()
                    .ok_or_else(|| {
                        SignalProcessCreationError::InvalidNumberOfInputs(
                            func.name().into(),
                            func.inputs().len() as u32 + 1,
                            0,
                        )
                    })?
                    .into_pointer_value(),
            )
        } else {
            None
        };
        let mut fctx = FunctionContext::new(func, function, data_arg, func.outputs().len());
        Self::translate_instructions(rctx, ectx, mctx, &mut fctx, func.instructions())?;
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

    fn translate_instructions<'ectx>(
        rctx: &WispRuntimeContext,
        ectx: &'ectx WispExecutionContext,
        mctx: &mut ModuleContext<'ectx, '_>,
        fctx: &mut FunctionContext<'ectx, '_>,
        instructions: &[Instruction],
    ) -> Result<(BasicBlock<'ectx>, BasicBlock<'ectx>), SignalProcessCreationError> {
        let mut current_block = ectx.llvm().append_basic_block(fctx.function, "start");
        mctx.builder.position_at_end(current_block);
        let start_block = current_block;

        for insn in instructions {
            use crate::ir::Instruction::*;
            match insn {
                AllocLocal(lref) => {
                    let local = mctx.build("alloc_local", |b, _| {
                        b.build_alloca(ectx.f32_t(), &format!("local_{}", lref.0))
                    })?;
                    fctx.locals.insert(*lref, local);
                }
                Load(vref, loc) => {
                    use crate::ir::SourceLocation::*;
                    let value = match loc {
                        Local(lref) => {
                            let local = fctx.get_local(lref)?;
                            mctx.build("load_local", |b, n| b.build_load(ectx.f32_t(), local, n))?
                        }
                        Data(dref) => {
                            let p_data = fctx.get_data_argument()?;
                            let p_data_item = unsafe {
                                p_data.const_gep(
                                    ectx.data_t(),
                                    &[ectx.i32_t().const_int(dref.0 as u64, false)],
                                )
                            };
                            mctx.build("load_data_item", |b, n| {
                                b.build_load(
                                    match fctx.func.data[dref.0 as usize].type_ {
                                        IRFunctionDataType::Float => {
                                            ectx.f32_t().as_basic_type_enum()
                                        }
                                        IRFunctionDataType::Array => {
                                            ectx.pf32_t().as_basic_type_enum()
                                        }
                                    },
                                    p_data_item,
                                    n,
                                )
                            })?
                        }
                        LastValue(id, name, dref) => {
                            let data_layout = rctx
                                .get_function(name)
                                .unwrap()
                                .data_layout()
                                .get_untracked();
                            let (_, child_offset) = *data_layout
                                .as_ref()
                                .and_then(|l| l.children_data_items.get(&CallIndex(id.0)))
                                .ok_or_else(|| {
                                    SignalProcessCreationError::InvalidDataLayout(
                                        fctx.func.name().into(),
                                    )
                                })?;

                            let child_data_item = *data_layout
                                .as_ref()
                                .and_then(|l| l.own_data_items.get(dref))
                                .ok_or_else(|| {
                                    SignalProcessCreationError::InvalidDataLayout(
                                        fctx.func.name().into(),
                                    )
                                })?;

                            let p_func_data = fctx.get_data_argument()?;
                            let p_data_item = unsafe {
                                p_func_data.const_gep(
                                    ectx.data_t(),
                                    &[ectx.i32_t().const_int(
                                        (child_offset + child_data_item.offset) as u64,
                                        false,
                                    )],
                                )
                            };

                            mctx.build("load_data_item", |b, n| {
                                b.build_load(ectx.f32_t(), p_data_item, n)
                            })?
                        }
                    };
                    fctx.vars.insert(*vref, value);
                }
                Store(loc, op) => {
                    let value = Self::resolve_operand(ectx, mctx, fctx, op)?;
                    use crate::ir::TargetLocation::*;
                    match loc {
                        Local(lref) => {
                            let local = fctx.get_local(lref)?;
                            mctx.build("store_local", |b, _| b.build_store(local, value))?;
                        }
                        Data(dref) => {
                            let p_data = fctx.get_data_argument()?;
                            let p_data_item = unsafe {
                                p_data.const_gep(
                                    ectx.data_t(),
                                    &[ectx.i32_t().const_int(dref.0 as u64, false)],
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
                                b.build_load(ectx.pf32_t(), pp_output.as_pointer_value(), n)
                            })?;
                            let output = unsafe {
                                p_output.into_pointer_value().const_gep(
                                    ectx.f32_t(),
                                    &[ectx.i32_t().const_int(idx.0 as u64, false)],
                                )
                            };
                            mctx.build("output", |b, _| b.build_store(output, value))?;
                        }
                    }
                }
                ILoad(vref, op_array, op_idx) => {
                    let array =
                        Self::resolve_operand(ectx, mctx, fctx, op_array)?.into_pointer_value();
                    let idx_f32 =
                        Self::resolve_operand(ectx, mctx, fctx, op_idx)?.into_float_value();
                    let idx = mctx.build("cast", |b, n| {
                        b.build_float_to_unsigned_int(idx_f32, ectx.i32_t(), n)
                    })?;
                    let value = mctx.build("iload", |b, n| {
                        b.build_load(
                            ectx.f32_t(),
                            unsafe {
                                b.build_gep(
                                    ectx.f32_t(),
                                    array,
                                    &[idx.const_add(ectx.i32_t().const_int(1, false))],
                                    n,
                                )?
                            },
                            n,
                        )
                    })?;
                    fctx.vars.insert(*vref, value.as_basic_value_enum());
                }
                IStore(op_array, op_idx, op_value) => {
                    let array =
                        Self::resolve_operand(ectx, mctx, fctx, op_array)?.into_pointer_value();
                    let idx_f32 =
                        Self::resolve_operand(ectx, mctx, fctx, op_idx)?.into_float_value();
                    let idx = mctx.build("cast", |b, n| {
                        b.build_float_to_unsigned_int(idx_f32, ectx.i32_t(), n)
                    })?;
                    let value =
                        Self::resolve_operand(ectx, mctx, fctx, op_value)?.into_float_value();
                    mctx.build("istore", |b, n| {
                        b.build_store(
                            unsafe {
                                b.build_gep(
                                    ectx.f32_t(),
                                    array,
                                    &[idx.const_add(ectx.i32_t().const_int(1, false))],
                                    n,
                                )?
                            },
                            value,
                        )
                    })?;
                }
                Len(vref, op_array) => {
                    let array =
                        Self::resolve_operand(ectx, mctx, fctx, op_array)?.into_pointer_value();
                    let len_i32 = mctx
                        .build("len", |b, n| b.build_load(ectx.i32_t(), array, n))?
                        .into_int_value();
                    let len = mctx.build("cast", |b, n| {
                        b.build_unsigned_int_to_float(len_i32, ectx.f32_t(), n)
                    })?;
                    fctx.vars.insert(*vref, len.as_basic_value_enum());
                }
                UnaryOp(vref, type_, op) => {
                    let operand = Self::resolve_operand(ectx, mctx, fctx, op)?.into_float_value();
                    use crate::ir::UnaryOpType::*;
                    let res = match type_ {
                        Truncate => mctx.build("unop_trunc", |b, n| {
                            let trunc = Intrinsic::find("llvm.trunc").unwrap();
                            b.build_call(
                                trunc
                                    .get_declaration(
                                        mctx.module,
                                        &[ectx.f32_t().as_basic_type_enum()],
                                    )
                                    .unwrap(),
                                &[BasicMetadataValueEnum::FloatValue(operand)],
                                n,
                            )
                        }),
                    }?;
                    fctx.vars.insert(
                        *vref,
                        res.as_any_value_enum()
                            .into_float_value()
                            .as_basic_value_enum(),
                    );
                }
                BinaryOp(vref, type_, op1, op2) => {
                    let left = Self::resolve_operand(ectx, mctx, fctx, op1)?.into_float_value();
                    let right = Self::resolve_operand(ectx, mctx, fctx, op2)?.into_float_value();
                    use crate::ir::BinaryOpType::*;
                    let res = match type_ {
                        Add => mctx.build("binop_add", |b, n| b.build_float_add(left, right, n)),
                        Subtract => {
                            mctx.build("binop_sub", |b, n| b.build_float_sub(left, right, n))
                        }
                        Multiply => {
                            mctx.build("binop_mul", |b, n| b.build_float_mul(left, right, n))
                        }
                        Divide => mctx.build("binop_div", |b, n| b.build_float_div(left, right, n)),
                        Remainder => {
                            mctx.build("binop_rem", |b, n| b.build_float_rem(left, right, n))
                        }
                    }?;
                    fctx.vars.insert(*vref, res.as_basic_value_enum());
                }
                ComparisonOp(vref, type_, op1, op2) => {
                    let left = Self::resolve_operand(ectx, mctx, fctx, op1)?.into_float_value();
                    let right = Self::resolve_operand(ectx, mctx, fctx, op2)?.into_float_value();
                    use crate::ir::ComparisonOpType::*;
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
                        Self::translate_instructions(rctx, ectx, mctx, fctx, then_branch)?;
                    let (else_block_first, else_block_last) =
                        Self::translate_instructions(rctx, ectx, mctx, fctx, else_branch)?;

                    // Tie blocks together
                    mctx.builder.position_at_end(current_block);
                    mctx.build("cond", |b, _| {
                        b.build_conditional_branch(cond, then_block_first, else_block_first)
                    })?;

                    let next_block = ectx.llvm().append_basic_block(fctx.function, "post_cond");

                    mctx.builder.position_at_end(then_block_last);
                    mctx.build("then_jump", |b, _| b.build_unconditional_branch(next_block))?;

                    mctx.builder.position_at_end(else_block_last);
                    mctx.build("else_jump", |b, _| b.build_unconditional_branch(next_block))?;

                    current_block = next_block;
                    mctx.builder.position_at_end(current_block);
                }
                Call(id, name, in_vrefs, out_vrefs) => {
                    let callee_func = mctx.get_function(name)?;
                    if in_vrefs.len() != callee_func.inputs().len() {
                        return Err(SignalProcessCreationError::InvalidNumberOfInputs(
                            name.into(),
                            callee_func.inputs().len() as u32,
                            in_vrefs.len() as u32,
                        ));
                    }
                    if out_vrefs.len() > callee_func.outputs().len() {
                        return Err(SignalProcessCreationError::InvalidNumberOfOutputs(
                            name.into(),
                            callee_func.outputs().len() as u32,
                            out_vrefs.len() as u32,
                        ));
                    }
                    let mut args: Vec<BasicMetadataValueEnum> = vec![];
                    for (idx, input) in in_vrefs.iter().enumerate() {
                        let value = Self::resolve_operand(ectx, mctx, fctx, input)?;
                        args.push(match callee_func.inputs()[idx].type_ {
                            IRFunctionDataType::Float => {
                                BasicMetadataValueEnum::FloatValue(value.into_float_value())
                            }
                            IRFunctionDataType::Array => {
                                BasicMetadataValueEnum::PointerValue(value.into_pointer_value())
                            }
                        });
                    }

                    if let Some((_, offset)) = rctx
                        .get_function(name)
                        .unwrap()
                        .data_layout()
                        .get_untracked()
                        .as_ref()
                        .and_then(|l| l.children_data_items.get(&CallIndex(id.0)))
                    {
                        let p_func_data = fctx.get_data_argument()?;
                        let p_callee_data = unsafe {
                            p_func_data.const_gep(
                                ectx.data_t(),
                                &[ectx.i32_t().const_int(*offset as u64, false)],
                            )
                        };
                        args.insert(0, BasicMetadataValueEnum::PointerValue(p_callee_data));
                    }

                    let func_value = mctx
                        .module
                        .get_function(name)
                        .ok_or_else(|| SignalProcessCreationError::UnknownFunction(name.clone()))?;

                    let call_site =
                        mctx.build("call", |b, n| b.build_call(func_value, &args, n))?;
                    let res = call_site.as_any_value_enum();
                    match callee_func.outputs().len() {
                        0 => { /* do nothing */ }
                        1 => {
                            // If we don't have to capture the result, don't capture it
                            if out_vrefs.len() == 1 {
                                fctx.vars.insert(
                                    out_vrefs[0],
                                    match callee_func.outputs()[0].type_ {
                                        IRFunctionDataType::Float => {
                                            res.into_float_value().as_basic_value_enum()
                                        }
                                        IRFunctionDataType::Array => {
                                            res.into_pointer_value().as_basic_value_enum()
                                        }
                                    },
                                );
                            }
                        }
                        _ => todo!(),
                    }
                }
                Debug(vref) => {
                    // NOTE: This duplicates Call()
                    let args = [vref]
                        .iter()
                        .map(|op| Self::resolve_operand(ectx, mctx, fctx, op))
                        .collect::<Result<Vec<_>, SignalProcessCreationError>>()?
                        .into_iter()
                        .map(|v| BasicMetadataValueEnum::FloatValue(v.into_float_value()))
                        .collect::<Vec<_>>();

                    let func_value = mctx.module.get_function("wisp_debug").ok_or_else(|| {
                        SignalProcessCreationError::UnknownFunction("wisp_debug".into())
                    })?;

                    mctx.build("call", |b, n| b.build_call(func_value, &args, n))?;
                }
                BoolToFloat(vref, op) => {
                    let value = Self::resolve_operand(ectx, mctx, fctx, op)?.into_int_value();
                    let res = mctx.build("cast", |b, n| {
                        b.build_unsigned_int_to_float(value, ectx.f32_t(), n)
                    })?;
                    fctx.vars.insert(*vref, res.as_basic_value_enum());
                }
            }
        }
        Ok((start_block, current_block))
    }

    fn resolve_operand<'ectx>(
        ectx: &'ectx WispExecutionContext,
        mctx: &ModuleContext<'ectx, '_>,
        fctx: &FunctionContext<'ectx, '_>,
        op: &Operand,
    ) -> Result<BasicValueEnum<'ectx>, SignalProcessCreationError> {
        use crate::ir::Operand::*;
        Ok(match op {
            Constant(c) => {
                use crate::ir::Constant::*;
                match c {
                    SampleRate => ectx
                        .f32_t()
                        .const_float(mctx.ctx.sample_rate() as f64)
                        .as_basic_value_enum(),
                    EmptyArray => mctx
                        .module
                        .get_global("wisp_global_empty_array")
                        .unwrap()
                        .as_pointer_value()
                        .as_basic_value_enum(),
                }
            }
            Literal(v) => ectx.f32_t().const_float(*v as f64).as_basic_value_enum(),
            Var(vref) => fctx.get_var(vref)?,
            Arg(idx) => fctx.get_argument(*idx)?,
        })
    }
}
