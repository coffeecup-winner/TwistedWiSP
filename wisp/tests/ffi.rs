use std::ptr::null_mut;
use twisted_wisp::{c_api::*, OfflineSession};

#[test]
fn null_transfer_and_invalid_frames_preserve_ownership() {
    let mut host = OfflineSession::new(
        44100,
        3,
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
    )
    .unwrap();
    let name = host
        .load_flow_text("[flow]\nname='empty'\nnodes=[]\nedges=[]")
        .unwrap();
    let mut processor = Box::into_raw(Box::new(host.compile(&name).unwrap()));
    let original = processor;
    drop(host);
    unsafe {
        assert!(!wisp_engine_take_signal_processor(
            null_mut(),
            &mut processor
        ));
        assert_eq!(processor, original);
        let mut small = [7.; 2];
        wisp_processor_process_one(processor, small.as_mut_ptr(), small.len());
        assert_eq!(small, [7.; 2]);
        wisp_processor_process_all(processor, small.as_mut_ptr(), small.len());
        assert_eq!(small, [7.; 2]);
        wisp_processor_process_all(processor, null_mut(), 0);
        let mut frame = [7.; 3];
        wisp_processor_process_one(processor, frame.as_mut_ptr(), frame.len());
        assert_eq!(frame, [0.; 3]);
        wisp_processor_destroy(processor);
        wisp_processor_destroy(null_mut());
        wisp_string_destroy(null_mut());
    }
}

#[test]
#[ignore = "requires audio output or ALSA null PCM"]
fn native_transfer_subprocess() {
    if std::env::var_os("WISP_FFI_CHILD").is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "native_transfer_subprocess", "--ignored"])
            .env("WISP_FFI_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    unsafe {
        let config = wisp_engine_config_create();
        let core = std::ffi::CString::new(format!("{}/core", env!("CARGO_MANIFEST_DIR"))).unwrap();
        wisp_engine_config_set_core_path(config, core.as_ptr());
        let engine = wisp_engine_create(config);
        wisp_engine_config_destroy(config);
        assert!(!engine.is_null());
        let file = std::ffi::CString::new(format!(
            "{}/../wisp_gui/examples/control.twf",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let name = wisp_context_load_flow_from_file(engine, file.as_ptr());
        assert!(!name.is_null());
        let mut processor = wisp_engine_compile_signal_processor(engine, name);
        wisp_string_destroy(name);
        assert!(!processor.is_null());
        assert!(wisp_engine_take_signal_processor(engine, &mut processor));
        assert!(processor.is_null());
        assert!(!wisp_engine_take_signal_processor(engine, &mut processor));
        wisp_engine_dsp_start(engine);
        wisp_engine_dsp_stop(engine);
        wisp_processor_destroy(processor);
        wisp_engine_destroy(engine);
    }
}
