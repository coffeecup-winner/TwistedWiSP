mod wisp;

use std::error::Error;

use crate::wisp::{
    function::Function,
    ir::{BinaryOpType, ComparisonOpType, Instruction, Operand, OutputIndex, VarRef},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut context = crate::wisp::context::SignalProcessorContext::new();

    let func = Function::new(vec![
        Instruction::LoadPrev(VarRef(0)),
        Instruction::BinaryOp(
            VarRef(0),
            BinaryOpType::Add,
            Operand::Var(VarRef(0)),
            Operand::Constant(0.10),
        ),
        Instruction::ComparisonOp(
            VarRef(1),
            ComparisonOpType::Greater,
            Operand::Var(VarRef(0)),
            Operand::Constant(1.0),
        ),
        Instruction::Conditional(VarRef(1), vec![], vec![]),
        Instruction::StoreNext(VarRef(0)),
        Instruction::Output(OutputIndex(0), VarRef(0)),
    ]);

    let mut processor = context.create_signal_processor(&func)?;
    let mut v = vec![0.0; 20];
    processor.process(&mut v);
    println!("Result: {:?}", v);

    Ok(())
}
