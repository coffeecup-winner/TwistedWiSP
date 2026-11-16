use inkwell::{
    basic_block::BasicBlock,
    execution_engine::ExecutionEngine,
    intrinsics::Intrinsic,
    passes::PassBuilderOptions,
    targets::{CodeModel, RelocMode, Target, TargetMachine},
    types::{BasicMetadataTypeEnum, BasicType},
    values::{AnyValue, BasicMetadataValueEnum, BasicValue, BasicValueEnum},
    OptimizationLevel,
};
use log::debug;
use rand::Rng;

use crate::runner::{
    compiler::data_layout::DataLayout,
    context::{WispContext, WispExecutionContext},
};

use super::{
    error::SignalProcessCreationError,
    function_context::FunctionContext,
    module_context::ModuleContext,
    processor::{SignalProcessor, SignalProcessorContext},
};

use twisted_wisp_ir::{IRFunction, IRFunctionDataType, Instruction, Operand};

pub struct SignalProcessorBuilder {
    id_gen: u64,
}

impl SignalProcessorBuilder {
    pub fn new() -> Self {
        SignalProcessorBuilder { id_gen: 0 }
    }

    pub fn create_signal_processor<'ectx>(
        &mut self,
        ectx: &'ectx WispExecutionContext,
        wctx: &WispContext,
        top_level: &str,
    ) -> Result<(SignalProcessor, ExecutionEngine<'ectx>), SignalProcessCreationError> {
        self.id_gen += 1;

        let module = ectx.llvm().create_module(&format!("wisp_{}", self.id_gen));
        let execution_engine = module
            .create_jit_execution_engine(OptimizationLevel::None)
            .map_err(|_| SignalProcessCreationError::InitEE)?;

        let data_layout = DataLayout::calculate(wctx.get_function(top_level).unwrap(), wctx);
        let mut mctx = ModuleContext::new(ectx.llvm(), wctx, &module, &data_layout);

        let g_output = module.add_global(mctx.types.pf32, None, "wisp_global_output");
        let g_empty_array = module.add_global(mctx.types.pf32, None, "wisp_global_empty_array");
        let g_wisp_debug = module.add_function(
            "wisp_debug",
            mctx.types.void.fn_type(&[mctx.types.f32.into(); 1], false),
            None,
        );
        let g_noise = module.add_function("noise", mctx.types.f32.fn_type(&[], false), None);
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

        for (name, func) in wctx.functions_iter() {
            if !data_layout.was_called(name) {
                continue;
            }

            // >1 returns is currently not supported
            assert!(func.outputs().len() < 2);
            let mut arg_types: Vec<BasicMetadataTypeEnum> = vec![];
            if mctx.data_layout.get(name).is_some() {
                arg_types.push(mctx.types.pf32.into());
            }
            arg_types.extend(
                func.inputs()
                    .iter()
                    .map(|i| match i.type_ {
                        IRFunctionDataType::Float => mctx.types.f32.into(),
                        IRFunctionDataType::Array => mctx.types.pf32.into(),
                    })
                    .collect::<Vec<BasicMetadataTypeEnum>>(),
            );
            let fn_type = if func.outputs().len() == 1 {
                match func.outputs()[0].type_ {
                    IRFunctionDataType::Float => mctx.types.f32.fn_type(&arg_types, false),
                    IRFunctionDataType::Array => mctx.types.pf32.fn_type(&arg_types, false),
                }
            } else {
                mctx.types.void.fn_type(&arg_types, false)
            };
            module.add_function(name, fn_type, None);
        }

        for (_, func) in wctx.functions_iter() {
            if !data_layout.was_called(func.name()) {
                continue;
            }

            // TODO: Instead, check explicitly for builtin functions using a function attribute
            if !func.ir.is_empty() {
                self.build_function(ectx, &mut mctx, func)?;
            }
        }

        let entry = mctx.module.add_function(
            "wisp_entry",
            mctx.types.void.fn_type(&[mctx.types.pf32.into()], false),
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
        Ok((
            SignalProcessor::new(
                spctx,
                top_level,
                unsafe { function.into_raw() },
                data_layout,
                wctx.num_outputs(),
            ),
            execution_engine,
        ))
    }

    fn build_function<'ectx>(
        &self,
        ectx: &'ectx WispExecutionContext,
        mctx: &mut ModuleContext<'ectx, '_>,
        func: &IRFunction,
    ) -> Result<(), SignalProcessCreationError> {
        let function = mctx
            .module
            .get_function(func.name())
            .ok_or_else(|| SignalProcessCreationError::UnknownFunction(func.name().to_owned()))?;
        let data_arg = if mctx.data_layout.get(func.name()).is_some() {
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
        Self::translate_instructions(ectx, mctx, &mut fctx, func.instructions())?;
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
        ectx: &'ectx WispExecutionContext,
        mctx: &mut ModuleContext<'ectx, '_>,
        fctx: &mut FunctionContext<'ectx, '_>,
        instructions: &[Instruction],
    ) -> Result<(BasicBlock<'ectx>, BasicBlock<'ectx>), SignalProcessCreationError> {
        let mut current_block = ectx.llvm().append_basic_block(fctx.function, "start");
        mctx.builder.position_at_end(current_block);
        let start_block = current_block;

        for insn in instructions {
            use twisted_wisp_ir::Instruction::*;
            match insn {
                AllocLocal(lref) => {
                    let local = mctx.build("alloc_local", |b, _| {
                        b.build_alloca(mctx.types.f32, &format!("local_{}", lref.0))
                    })?;
                    fctx.locals.insert(*lref, local);
                }
                Load(vref, loc) => {
                    use twisted_wisp_ir::SourceLocation::*;
                    let value = match loc {
                        Local(lref) => {
                            let local = fctx.get_local(lref)?;
                            mctx.build("load_local", |b, n| b.build_load(mctx.types.f32, local, n))?
                        }
                        Data(dref) => {
                            let p_data = fctx.get_data_argument()?;
                            let p_data_item = unsafe {
                                p_data.const_gep(
                                    mctx.types.data,
                                    &[mctx.types.i32.const_int(dref.0 as u64, false)],
                                )
                            };
                            mctx.build("load_data_item", |b, n| {
                                b.build_load(
                                    match fctx.func.data[dref.0 as usize].type_ {
                                        IRFunctionDataType::Float => {
                                            mctx.types.f32.as_basic_type_enum()
                                        }
                                        IRFunctionDataType::Array => {
                                            mctx.types.pf32.as_basic_type_enum()
                                        }
                                    },
                                    p_data_item,
                                    n,
                                )
                            })?
                        }
                        LastValue(id, name, dref) => {
                            let (_, child_offset) = *mctx
                                .data_layout
                                .get(fctx.func.name())
                                .and_then(|l| l.children_data_items.get(id))
                                .ok_or_else(|| {
                                    SignalProcessCreationError::InvalidDataLayout(
                                        fctx.func.name().into(),
                                    )
                                })?;

                            let child_data_item = *mctx
                                .data_layout
                                .get(name)
                                .and_then(|l| l.own_data_items.get(dref))
                                .ok_or_else(|| {
                                    SignalProcessCreationError::InvalidDataLayout(
                                        fctx.func.name().into(),
                                    )
                                })?;

                            let p_func_data = fctx.get_data_argument()?;
                            let p_data_item = unsafe {
                                p_func_data.const_gep(
                                    mctx.types.data,
                                    &[mctx.types.i32.const_int(
                                        (child_offset + child_data_item.offset) as u64,
                                        false,
                                    )],
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
                    use twisted_wisp_ir::TargetLocation::*;
                    match loc {
                        Local(lref) => {
                            let local = fctx.get_local(lref)?;
                            mctx.build("store_local", |b, _| b.build_store(local, value))?;
                        }
                        Data(dref) => {
                            let p_data = fctx.get_data_argument()?;
                            let p_data_item = unsafe {
                                p_data.const_gep(
                                    mctx.types.data,
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
                ILoad(vref, op_array, op_idx) => {
                    let array = Self::resolve_operand(mctx, fctx, op_array)?.into_pointer_value();
                    let idx_f32 = Self::resolve_operand(mctx, fctx, op_idx)?.into_float_value();
                    let idx = mctx.build("cast", |b, n| {
                        b.build_float_to_unsigned_int(idx_f32, mctx.types.i32, n)
                    })?;
                    let value = mctx.build("iload", |b, n| {
                        b.build_load(
                            mctx.types.f32,
                            unsafe {
                                b.build_gep(
                                    mctx.types.f32,
                                    array,
                                    &[idx.const_add(mctx.types.i32.const_int(1, false))],
                                    n,
                                )?
                            },
                            n,
                        )
                    })?;
                    fctx.vars.insert(*vref, value.as_basic_value_enum());
                }
                IStore(op_array, op_idx, op_value) => {
                    let array = Self::resolve_operand(mctx, fctx, op_array)?.into_pointer_value();
                    let idx_f32 = Self::resolve_operand(mctx, fctx, op_idx)?.into_float_value();
                    let idx = mctx.build("cast", |b, n| {
                        b.build_float_to_unsigned_int(idx_f32, mctx.types.i32, n)
                    })?;
                    let value = Self::resolve_operand(mctx, fctx, op_value)?.into_float_value();
                    mctx.build("istore", |b, n| {
                        b.build_store(
                            unsafe {
                                b.build_gep(
                                    mctx.types.f32,
                                    array,
                                    &[idx.const_add(mctx.types.i32.const_int(1, false))],
                                    n,
                                )?
                            },
                            value,
                        )
                    })?;
                }
                Len(vref, op_array) => {
                    let array = Self::resolve_operand(mctx, fctx, op_array)?.into_pointer_value();
                    let len_i32 = mctx
                        .build("len", |b, n| b.build_load(mctx.types.i32, array, n))?
                        .into_int_value();
                    let len = mctx.build("cast", |b, n| {
                        b.build_unsigned_int_to_float(len_i32, mctx.types.f32, n)
                    })?;
                    fctx.vars.insert(*vref, len.as_basic_value_enum());
                }
                UnaryOp(vref, type_, op) => {
                    let operand = Self::resolve_operand(mctx, fctx, op)?.into_float_value();
                    use twisted_wisp_ir::UnaryOpType::*;
                    let res = match type_ {
                        Truncate => mctx.build("unop_trunc", |b, n| {
                            let trunc = Intrinsic::find("llvm.trunc").unwrap();
                            b.build_call(
                                trunc
                                    .get_declaration(
                                        mctx.module,
                                        &[mctx.types.f32.as_basic_type_enum()],
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
                    let left = Self::resolve_operand(mctx, fctx, op1)?.into_float_value();
                    let right = Self::resolve_operand(mctx, fctx, op2)?.into_float_value();
                    use twisted_wisp_ir::BinaryOpType::*;
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
                    let left = Self::resolve_operand(mctx, fctx, op1)?.into_float_value();
                    let right = Self::resolve_operand(mctx, fctx, op2)?.into_float_value();
                    use twisted_wisp_ir::ComparisonOpType::*;
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
                        Self::translate_instructions(ectx, mctx, fctx, then_branch)?;
                    let (else_block_first, else_block_last) =
                        Self::translate_instructions(ectx, mctx, fctx, else_branch)?;

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
                        let value = Self::resolve_operand(mctx, fctx, input)?;
                        args.push(match callee_func.inputs()[idx].type_ {
                            IRFunctionDataType::Float => {
                                BasicMetadataValueEnum::FloatValue(value.into_float_value())
                            }
                            IRFunctionDataType::Array => {
                                BasicMetadataValueEnum::PointerValue(value.into_pointer_value())
                            }
                        });
                    }

                    if let Some((_, offset)) = mctx
                        .data_layout
                        .get(fctx.func.name())
                        .and_then(|l| l.children_data_items.get(id))
                    {
                        let p_func_data = fctx.get_data_argument()?;
                        let p_callee_data = unsafe {
                            p_func_data.const_gep(
                                mctx.types.data,
                                &[mctx.types.i32.const_int(*offset as u64, false)],
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
                        .map(|op| Self::resolve_operand(mctx, fctx, op))
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
                    let value = Self::resolve_operand(mctx, fctx, op)?.into_int_value();
                    let res = mctx.build("cast", |b, n| {
                        b.build_unsigned_int_to_float(value, mctx.types.f32, n)
                    })?;
                    fctx.vars.insert(*vref, res.as_basic_value_enum());
                }
            }
        }
        Ok((start_block, current_block))
    }

    fn resolve_operand<'ectx>(
        mctx: &ModuleContext<'ectx, '_>,
        fctx: &FunctionContext<'ectx, '_>,
        op: &Operand,
    ) -> Result<BasicValueEnum<'ectx>, SignalProcessCreationError> {
        use twisted_wisp_ir::Operand::*;
        Ok(match op {
            Constant(c) => {
                use twisted_wisp_ir::Constant::*;
                match c {
                    SampleRate => mctx
                        .types
                        .f32
                        .const_float(mctx.wctx.sample_rate() as f64)
                        .as_basic_value_enum(),
                    EmptyArray => mctx
                        .module
                        .get_global("wisp_global_empty_array")
                        .unwrap()
                        .as_pointer_value()
                        .as_basic_value_enum(),
                }
            }
            Literal(v) => mctx.types.f32.const_float(*v as f64).as_basic_value_enum(),
            Var(vref) => fctx.get_var(vref)?,
            Arg(idx) => fctx.get_argument(*idx)?,
        })
    }
}
