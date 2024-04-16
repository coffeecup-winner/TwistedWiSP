mod wisp;

use std::error::Error;

use crate::wisp::{
    function::Function,
    ir::{BinaryOpType, Instruction, Operand, OutputIndex, VarRef},
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
        Instruction::Output(OutputIndex(0), VarRef(1)),
    ]);

    let mut processor = context.create_signal_processor(&func)?;
    let mut v = vec![0.0; 2];
    processor.process_one(&mut v[0..]);
    processor.process_one(&mut v[1..]);
    println!("Result: {:?}", v);

    Ok(())
}
