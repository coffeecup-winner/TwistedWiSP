// JIT-only packed ABI: length bits followed by samples, always processor-owned
// or the immutable empty sentinel. These entrypoints are never exposed to C/UI.
// All float validation happens before integer conversion or address arithmetic.
pub(crate) unsafe extern "C" fn array_read(array: *mut f32, index: f32) -> f32 {
    let length = unsafe { array.cast::<u32>().read() } as usize;
    if !index.is_finite() || index < 0. || index as f64 >= length as f64 {
        return 0.;
    }
    unsafe { array.add(index as usize + 1).read() }
}
pub(crate) unsafe extern "C" fn array_write(array: *mut f32, index: f32, value: f32) {
    let length = unsafe { array.cast::<u32>().read() } as usize;
    if !index.is_finite() || index < 0. || index as f64 >= length as f64 {
        return;
    }
    unsafe {
        array.add(index as usize + 1).write(value);
    }
}
