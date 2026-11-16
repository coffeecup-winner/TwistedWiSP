use crate::runner::engine::*;

#[no_mangle]
pub extern "C" fn wisp_engine_create() -> *mut TwistedWispEngine {
    match TwistedWispEngine::create(TwistedWispEngineConfig::default()) {
        Ok(engine) => Box::into_raw(Box::new(engine)),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn wisp_engine_destroy(engine: *mut TwistedWispEngine) {
    if !engine.is_null() {
        drop(unsafe { Box::from_raw(engine) })
    }
}
