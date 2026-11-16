use std::{borrow::Borrow, ffi::c_char};

use crate::{compiler::SignalProcessor, runner::engine::*};

/// Global API

#[no_mangle]
pub extern "C" fn wisp_enable_logging() -> bool {
    TwistedWispEngine::enable_logging()
}

/// Engine config API

#[no_mangle]
pub extern "C" fn wisp_engine_config_create() -> *mut TwistedWispEngineConfig {
    Box::into_raw(Box::new(TwistedWispEngineConfig::default()))
}

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_config_destroy(config: *mut TwistedWispEngineConfig) {
    if !config.is_null() {
        drop(unsafe { Box::from_raw(config) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_config_set_core_path(
    config: *mut TwistedWispEngineConfig,
    core_path: *const c_char,
) {
    if !config.is_null() {
        unsafe {
            (*config).core_path = Some(
                std::ffi::CStr::from_ptr(core_path)
                    .to_string_lossy()
                    .into_owned()
                    .into(),
            );
        }
    }
}

/// Engine API

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_create(
    config: *mut TwistedWispEngineConfig,
) -> *mut TwistedWispEngine {
    let config = if config.is_null() {
        &TwistedWispEngineConfig::default()
    } else {
        unsafe { &*config }
    };
    match TwistedWispEngine::create(config) {
        Ok(engine) => Box::into_raw(Box::new(engine)),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_destroy(engine: *mut TwistedWispEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine) })
    }
}

/// Engine Context API

#[no_mangle]
pub unsafe extern "C" fn wisp_context_load_flow_from_file(
    engine: *mut TwistedWispEngine,
    file_name: *const c_char,
) -> *mut c_char {
    if let Some(engine) = unsafe { engine.as_mut() } {
        if let Ok(name) = engine.ctx_load_flow_from_file(
            unsafe { std::ffi::CStr::from_ptr(file_name) }
                .to_string_lossy()
                .borrow(),
        ) {
            return std::ffi::CString::new(name).unwrap().into_raw();
        }
    }
    std::ptr::null_mut()
}

/// Engine Runtime API

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_compile_signal_processor(
    engine: *mut TwistedWispEngine,
    function: *const c_char,
) -> *mut SignalProcessor {
    if let Some(engine) = unsafe { engine.as_mut() } {
        if let Ok(sp) = engine.runtime_compile_signal_processor(
            unsafe { std::ffi::CStr::from_ptr(function) }
                .to_string_lossy()
                .into_owned(),
        ) {
            return Box::into_raw(Box::new(sp));
        }
    }
    std::ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_switch_to_signal_processor(
    engine: *mut TwistedWispEngine,
    processor: *mut SignalProcessor,
) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.runtime_switch_to_signal_processor(unsafe { processor.read() });
    }
}

/// Processor API

#[no_mangle]
pub unsafe extern "C" fn wisp_processor_destroy(processor: *mut SignalProcessor) {
    if !processor.is_null() {
        drop(unsafe { Box::from_raw(processor) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_processor_process_one(
    processor: *mut SignalProcessor,
    output: *mut f32,
    size: usize,
) {
    if let Some(processor) = unsafe { processor.as_mut() } {
        unsafe { processor.process_one(std::slice::from_raw_parts_mut(output, size)) }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_processor_process_all(
    processor: *mut SignalProcessor,
    output: *mut f32,
    size: usize,
) {
    if let Some(processor) = unsafe { processor.as_mut() } {
        unsafe { processor.process_all(std::slice::from_raw_parts_mut(output, size)) }
    }
}
