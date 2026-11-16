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
    if !config.is_null() && !core_path.is_null() {
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
    if file_name.is_null() {
        return std::ptr::null_mut();
    }
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
pub unsafe extern "C" fn wisp_engine_dsp_start(engine: *mut TwistedWispEngine) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.runtime_dsp_start();
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_dsp_stop(engine: *mut TwistedWispEngine) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.runtime_dsp_stop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_engine_compile_signal_processor(
    engine: *mut TwistedWispEngine,
    function: *const c_char,
) -> *mut SignalProcessor {
    if function.is_null() {
        return std::ptr::null_mut();
    }
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
    if !processor.is_null() {
        if let Some(engine) = unsafe { engine.as_mut() } {
            // Legacy transfer consumes the allocation, including on install failure.
            engine.runtime_switch_to_signal_processor(*unsafe { Box::from_raw(processor) });
        }
    }
}

/// Consumes a nonnull processor for a valid engine and clears the caller's handle.
/// False with an unchanged handle means invalid/null arguments; false with a null
/// handle means installation failed after consumption. Stale pointers are invalid.
#[no_mangle]
pub unsafe extern "C" fn wisp_engine_take_signal_processor(
    engine: *mut TwistedWispEngine,
    processor: *mut *mut SignalProcessor,
) -> bool {
    let (Some(engine), Some(handle)) = (unsafe { engine.as_mut() }, unsafe { processor.as_mut() })
    else {
        return false;
    };
    if handle.is_null() {
        return false;
    }
    let owned = *unsafe { Box::from_raw(*handle) };
    *handle = std::ptr::null_mut();
    engine.runtime_install_checked(owned).is_ok()
}

/// Frees a string returned by wisp_context_load_flow_from_file, exactly once.
#[no_mangle]
pub unsafe extern "C" fn wisp_string_destroy(value: *mut c_char) {
    if !value.is_null() {
        drop(unsafe { std::ffi::CString::from_raw(value) });
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
    if !output.is_null() && size <= isize::MAX as usize / std::mem::size_of::<f32>() {
        if let Some(processor) = unsafe { processor.as_mut() } {
            let _ = processor
                .process_one_checked(unsafe { std::slice::from_raw_parts_mut(output, size) });
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wisp_processor_process_all(
    processor: *mut SignalProcessor,
    output: *mut f32,
    size: usize,
) {
    if !output.is_null() && size <= isize::MAX as usize / std::mem::size_of::<f32>() {
        if let Some(processor) = unsafe { processor.as_mut() } {
            let _ = processor
                .process_all_checked(unsafe { std::slice::from_raw_parts_mut(output, size) });
        }
    }
}
