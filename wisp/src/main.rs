mod audio;
mod server;
mod wisp;

use std::error::Error;

use crate::wisp::{
    flow::Flow,
    function::{Function, FunctionInput, FunctionOutput},
    ir::{
        BinaryOpType, ComparisonOpType, FunctionOutputIndex, Instruction, LocalRef, Operand,
        SourceLocation, TargetLocation, VarRef,
    },
    WispContext,
};

use clap::Parser;
use stderrlog::LogLevelNum;

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

fn add_test_functions(wisp: &mut WispContext) {
    let test_func = Function::new(
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
            Instruction::Store(TargetLocation::Local(LocalRef(0)), Operand::Var(VarRef(0))),
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
                    Instruction::Store(TargetLocation::Local(LocalRef(0)), Operand::Var(VarRef(0))),
                ],
                vec![],
            ),
            Instruction::Load(VarRef(0), SourceLocation::Local(LocalRef(0))),
            Instruction::Store(
                TargetLocation::FunctionOutput(FunctionOutputIndex(0)),
                Operand::Var(VarRef(0)),
            ),
        ],
        None,
    );
    wisp.add_function(test_func);

    let mut flow = Flow::new();
    let idx_test = flow.add_function("test".into());
    let idx_out = flow.add_function("out".into());
    let idx_lag = flow.add_function("lag".into());
    flow.connect(idx_test, 0, idx_out, 0);
    flow.connect(idx_test, 0, idx_lag, 0);
    flow.connect(idx_lag, 0, idx_test, 0);
    let flow_func = Function::new_flow("example".into(), flow);
    wisp.add_function(flow_func);
}

fn main() -> Result<(), Box<dyn Error>> {
    stderrlog::new()
        .module(module_path!())
        .verbosity(LogLevelNum::Debug)
        .timestamp(stderrlog::Timestamp::Microsecond)
        .init()
        .expect("Failed to initialize the logger");

    let args = Args::parse();

    if args.list_audio_devices {
        ConfiguredAudioDevice::list_all_devices()?;
        return Ok(());
    }

    let device = ConfiguredAudioDevice::open(args.audio_host, args.audio_device)?;
    let mut wisp = WispContext::new(device.num_output_channels(), device.sample_rate());
    wisp.register_builtin_functions();

    // TODO: Remove this
    add_test_functions(&mut wisp);

    if args.server {
        return crate::server::main(wisp, device);
    }

    // let (mut processor, _ee) = wisp.create_signal_processor("example")?;
    // let mut v = vec![0.0; 64];
    // let start = std::time::Instant::now();
    // processor.process(&mut v);
    // let end = std::time::Instant::now();
    // let duration_ns = (end - start).as_nanos();
    // info!("Result: {:?}", v);
    // let time_limit_ns =
    //     1_000_000_000 / device.sample_rate() * v.len() as u32 / device.num_output_channels();
    // info!(
    //     "Took {}.{}µs (CPU usage: {:.2}%)",
    //     duration_ns / 1000,
    //     duration_ns % 1000,
    //     (duration_ns as f32 / time_limit_ns as f32 * 100.0)
    // );

    Ok(())
}
