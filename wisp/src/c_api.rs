use std::ffi::c_char;

use crate::runner::engine::*;

#[no_mangle]
pub extern "C" fn wisp_engine_create() -> *mut TwistedWispEngine {
    match TwistedWispEngine::create(TwistedWispEngineConfig::default()) {
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

#[no_mangle]
pub extern "C" fn wisp_context_set_main_function(
    engine: *mut TwistedWispEngine,
    function: *const c_char,
) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        engine.context_set_main_function(
            unsafe { std::ffi::CStr::from_ptr(function) }
                .to_string_lossy()
                .into_owned(),
        );
    }
}

#[no_mangle]
pub extern "C" fn wisp_context_update(engine: *mut TwistedWispEngine) {
    if let Some(engine) = unsafe { engine.as_mut() } {
        // TODO: Expose errors to the caller
        engine.context_update().expect("Failed to update context");
    }
}
