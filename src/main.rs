mod audio;
mod wisp;

use std::{
    borrow::BorrowMut,
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::wisp::{
    flow::Flow,
    function::{Function, FunctionInput, FunctionOutput},
    ir::{
        BinaryOpType, ComparisonOpType, FunctionOutputIndex, Instruction, LocalRef, Operand, VarRef,
    },
    runtime::Runtime,
    WispContext,
};

use clap::Parser;

use crate::audio::device::ConfiguredAudioDevice;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    list_audio_devices: bool,
    #[arg(short, long)]
    audio_host: Option<String>,
    #[arg(short = 'd', long)]
    audio_device: Option<String>,
    #[arg(short, long)]
    server: bool,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    if args.list_audio_devices {
        ConfiguredAudioDevice::list_all_devices()?;
        return Ok(());
    }

    let device = ConfiguredAudioDevice::open(args.audio_host, args.audio_device)?;

    let mut context = WispContext::new();

    let func = Function::new(
        "test".into(),
        vec![FunctionInput::default()],
        vec![FunctionOutput],
        vec![],
        vec![
            Instruction::AllocLocal(LocalRef(0)),
            Instruction::BinaryOp(
                VarRef(0),
                BinaryOpType::Add,
                Operand::Arg(0),
                Operand::Literal(0.01),
            ),
            Instruction::StoreLocal(LocalRef(0), Operand::Var(VarRef(0))),
            Instruction::ComparisonOp(
                VarRef(1),
                ComparisonOpType::Greater,
                Operand::Var(VarRef(0)),
                Operand::Literal(1.0),
            ),
            Instruction::Conditional(
                VarRef(1),
                vec![
                    Instruction::BinaryOp(
                        VarRef(0),
                        BinaryOpType::Subtract,
                        Operand::Var(VarRef(0)),
                        Operand::Literal(1.0),
                    ),
                    Instruction::StoreLocal(LocalRef(0), Operand::Var(VarRef(0))),
                ],
                vec![],
            ),
            Instruction::LoadLocal(VarRef(0), LocalRef(0)),
            Instruction::StoreFunctionOutput(FunctionOutputIndex(0), Operand::Var(VarRef(0))),
        ],
        None,
    );

    let mut flow = Flow::new();
    let idx_test = flow.add_function("test".into());
    let idx_out = flow.add_function("out".into());
    let idx_lag = flow.add_function("lag".into());
    flow.connect(idx_test, 0, idx_out, 0);
    flow.connect(idx_test, 0, idx_lag, 0);
    flow.connect(idx_lag, 0, idx_test, 0);

    let mut runtime = Runtime::init(device.num_output_channels(), device.sample_rate());
    runtime.register_function(func);

    let (mut processor, _ee) = context.create_signal_processor(&flow, &runtime)?;
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

    if args.server {
        let mut processor_mutex = Arc::new(Mutex::new(processor));
        let _stream = device
            .build_output_audio_stream(move |_num_outputs: u32, buffer: &mut [f32]| {
                processor_mutex.borrow_mut().lock().unwrap().process(buffer);
                // Clip the output to the safe levels
                for b in buffer.iter_mut() {
                    *b = b.clamp(-1.0, 1.0);
                }
            })
            .expect("msg");

        loop {
            std::thread::sleep(Duration::from_millis(50));
            // Wait until Ctrl+C
        }
    }

    Ok(())
}
