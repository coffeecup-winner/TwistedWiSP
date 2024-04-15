mod wisp;

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let mut context = crate::wisp::SignalProcessorContext::new();
    let processor = context
        .create_signal_processor()
        .ok_or("Unable to create signal processor")?;

    let x = [0.42];
    let mut y = [0.0];

    processor.process(&x, &mut y);
    println!("Result: {}", y[0]);

    Ok(())
}
