mod wisp;

use std::error::Error;

use crate::wisp::ir::{Instruction, VarRef};

fn main() -> Result<(), Box<dyn Error>> {
    let mut context = crate::wisp::context::SignalProcessorContext::new();

    let func = crate::wisp::function::Function::new(vec![
        Instruction::LoadPrev(VarRef(0)),
        Instruction::StoreNext(VarRef(0)),
    ]);

    let processor = context
        .create_signal_processor(&func)
        .ok_or("Unable to create signal processor")?;

    let x = [0.42];
    let mut y = [0.0];

    processor.process(&x, &mut y);
    println!("Result: {}", y[0]);

    Ok(())
}
