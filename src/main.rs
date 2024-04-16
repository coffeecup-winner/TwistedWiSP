mod wisp;

use std::error::Error;

use crate::wisp::{
    function::Function,
    ir::{BinaryOpType, Instruction, Operand, VarRef},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut context = crate::wisp::context::SignalProcessorContext::new();

    let func = Function::new(vec![
        Instruction::LoadPrev(VarRef(0)),
        Instruction::BinaryOp(
            VarRef(1),
            BinaryOpType::Add,
            Operand::Var(VarRef(0)),
            Operand::Constant(0.01),
        ),
        Instruction::StoreNext(VarRef(1)),
    ]);

    let mut processor = context.create_signal_processor(&func)?;
    processor.process();
    println!("Result: {}", processor.values()[0]);
    processor.process();
    println!("Result: {}", processor.values()[0]);

    Ok(())
}
