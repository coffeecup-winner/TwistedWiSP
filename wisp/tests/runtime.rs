//! Live Rust API smoke tests. Run explicitly with an audio device or ALSA null PCM.
use std::{
    path::Path,
    time::{Duration, Instant},
};
use twisted_wisp::{CallIndex, DataIndex, TwistedWispEngine, TwistedWispEngineConfig, WatchIndex};

fn wait_for_value(engine: &TwistedWispEngine, watch: WatchIndex, expected: f32) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let batch = engine.runtime_query_checked().unwrap();
        if batch.values.get(&watch).and_then(|v| v.last()) == Some(&expected) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Callback did not produce {expected}"
        );
    }
}

#[test]
#[ignore = "requires audio output or ALSA null PCM configured by ALSA_CONFIG_PATH"]
fn native_engine_lifecycle() {
    // Isolate LLVM/native teardown failures from the rest of the test process.
    if std::env::var_os("WISP_RUNTIME_TEST_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_engine_lifecycle",
                "--ignored",
                "--nocapture",
            ])
            .env("WISP_RUNTIME_TEST_CHILD", "1")
            .status()
            .unwrap();
        assert!(
            status.success(),
            "Native runtime subprocess failed: {status}"
        );
        return;
    }

    let config = TwistedWispEngineConfig {
        core_path: Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("core")),
        audio_output_channels: Some(2),
        audio_sample_rate: Some(44100),
        audio_buffer_size: Some(512),
        ..Default::default()
    };
    let mut engine = TwistedWispEngine::create(&config).unwrap();
    drop(config);
    let name = engine
        .ctx_load_flow_from_text(
            "[flow]\nname='runtime_test'\n\
         [[flow.nodes]]\ntext='control'\n\
         [[flow.nodes]]\ntext='watch'\n\
         [[flow.nodes]]\ntext='out'\n\
         [[flow.edges]]\nfrom=0\nto=1\noutput_index=0\ninput_index=0\n\
         [[flow.edges]]\nfrom=0\nto=2\noutput_index=0\ninput_index=0\n",
        )
        .unwrap();
    let mut independent = engine
        .runtime_compile_signal_processor(name.clone())
        .unwrap();
    independent
        .set_data_value(&name, CallIndex(0), DataIndex(0), 0.25)
        .unwrap();
    let installed = engine
        .runtime_compile_signal_processor(name.clone())
        .unwrap();
    // Consuming the processor makes reuse after installation a Rust compile error.
    engine.runtime_install_checked(installed).unwrap();
    let watch = engine
        .runtime_watch_checked(&name, CallIndex(1), true)
        .unwrap();
    engine
        .runtime_scalar_checked(&name, CallIndex(0), DataIndex(0), 0.375)
        .unwrap();
    engine.runtime_start_checked().unwrap();
    wait_for_value(&engine, watch, 0.375);
    engine.runtime_stop_checked().unwrap();
    engine
        .runtime_scalar_checked(&name, CallIndex(0), DataIndex(0), 0.75)
        .unwrap();
    engine.runtime_start_checked().unwrap();
    wait_for_value(&engine, watch, 0.75);
    engine.runtime_stop_checked().unwrap();
    assert!(engine.runtime_healthy());
    drop(engine);

    // An untransferred processor owns its LLVM context after the live host dies.
    let mut frame = vec![0.; independent.output_channels()];
    independent.process_one_checked(&mut frame).unwrap();
    assert!(frame.iter().all(|value| *value == 0.25));
}
