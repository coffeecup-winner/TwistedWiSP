mod wisp;

use std::error::Error;

use crate::wisp::{
    flow::Flow,
    function::{Function, FunctionOutput},
    ir::{
        BinaryOpType, ComparisonOpType, FunctionOutputIndex, Instruction, LocalRef, Operand, VarRef,
    },
    runtime::Runtime,
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut context = crate::wisp::context::SignalProcessorContext::new();

    let func = Function::new(
        "test".into(),
        vec![],
        vec![FunctionOutput],
        vec![
            Instruction::LoadPrev(VarRef(0)),
            Instruction::AllocLocal(LocalRef(0)),
            Instruction::BinaryOp(
                VarRef(0),
                BinaryOpType::Add,
                Operand::Var(VarRef(0)),
                Operand::Constant(0.10),
            ),
            Instruction::StoreLocal(LocalRef(0), VarRef(0)),
            Instruction::ComparisonOp(
                VarRef(1),
                ComparisonOpType::Greater,
                Operand::Var(VarRef(0)),
                Operand::Constant(1.0),
            ),
            Instruction::Conditional(
                VarRef(1),
                vec![
                    Instruction::BinaryOp(
                        VarRef(0),
                        BinaryOpType::Subtract,
                        Operand::Var(VarRef(0)),
                        Operand::Constant(1.0),
                    ),
                    Instruction::StoreLocal(LocalRef(0), VarRef(0)),
                ],
                vec![],
            ),
            Instruction::LoadLocal(VarRef(0), LocalRef(0)),
            Instruction::StoreNext(VarRef(0)),
            Instruction::StoreFunctionOutput(FunctionOutputIndex(0), VarRef(0)),
        ],
    );

    let mut flow = Flow::new();
    let idx_test = flow.add_function("test".into());
    let idx_out = flow.add_function("out".into());
    flow.connect(idx_test, 0, idx_out, 0);

    let num_outputs = 1;
    let mut runtime = Runtime::init(num_outputs);
    runtime.register_function(func);

    let mut processor = context.create_signal_processor(&flow, &runtime)?;
    let mut v = vec![0.0; 64];
    let start = std::time::Instant::now();
    processor.process(&mut v);
    let end = std::time::Instant::now();
    let duration_ns = (end - start).as_nanos();
    println!("Result: {:?}", v);
    let time_limit_ns = 1_000_000_000 / 44100 * 64;
    println!(
        "Took {}.{}µs (CPU usage: {:.2}%)",
        duration_ns / 1000,
        duration_ns % 1000,
        (duration_ns as f32 / time_limit_ns as f32 * 100.0)
    );

    Ok(())
}
