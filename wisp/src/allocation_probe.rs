//! Test-only per-thread allocator probe. Compiler/test harness allocations on
//! other threads do not contaminate the measured callback section.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
thread_local! {
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}
struct Probe;
#[global_allocator]
static ALLOCATOR: Probe = Probe;
fn count(allocate: bool) {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = COUNTS.try_with(|counts| {
                let (a, d) = counts.get();
                counts.set((a + usize::from(allocate), d + usize::from(!allocate)));
            });
        }
    });
}
unsafe impl GlobalAlloc for Probe {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(true);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count(false);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(true);
        count(false);
        unsafe { System.realloc(ptr, layout, size) }
    }
}
pub fn measure(f: impl FnOnce()) -> (usize, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|active| active.set(false));
        }
    }
    COUNTS.with(|counts| counts.set((0, 0)));
    ACTIVE.with(|active| active.set(true));
    let reset = Reset;
    f();
    drop(reset);
    COUNTS.with(Cell::get)
}
