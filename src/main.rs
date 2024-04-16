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

    let processor = context.create_signal_processor(&func)?;

    let v0 = [0.0];
    let mut v1 = [0.0];
    let mut v2 = [0.0];

    processor.process(&v0, &mut v1);
    println!("Result: {}", v1[0]);

    processor.process(&v1, &mut v2);
    println!("Result: {}", v2[0]);

    Ok(())
}
