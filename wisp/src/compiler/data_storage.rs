use std::cell::UnsafeCell;

use super::data_layout::DataArrayHandle;

#[derive(Clone, Copy)]
#[repr(C)]
pub union DataValue {
    float: f32,
    array: *mut f32,
}
// The data storage ownership guarantees that arrays are safe to use
unsafe impl Send for DataValue {}

impl DataValue {
    pub fn new_float(value: f32) -> DataValue {
        DataValue { float: value }
    }

    pub(crate) fn new_array(array: *mut f32) -> DataValue {
        DataValue { array }
    }

    pub fn as_float(&self) -> f32 {
        unsafe { self.float }
    }
}

/// Data storage for the processor chain, keeps tracks of all the data (function instance-local and arrays).
/// Data storage rules:
///   1. Function instance-local data is allocated in blocks
///   2. Arrays have their own fixed-size allocations
///   3. While the DSP is running, the data storage can only grow (no reuse), even when processors are swapped
///       - This enables easy undo functionality (not snapshotted, keeping old values)
///       - Where possible, data cells are reused in the updated processors
///       - In the future cleanup could be added (e.g. by limiting the max undo count and enabling large array offload to disk)
///   4. All the pointers into data blocks and arrays are stable
#[derive(Default)]
pub struct DataStorage {
    blocks: Vec<Box<[UnsafeCell<DataValue>]>>,
    next_slot: usize,
    arrays: Vec<DataArray>,
}

#[derive(Debug, Default)]
struct DataArray {
    source: Option<DataArrayHandle>,
    #[cfg(test)]
    lifetime: std::sync::Arc<()>,
    data: Box<[UnsafeCell<f32>]>,
}

/// An address into session-owned storage. Dereferencing it requires the storage
/// to remain alive and the caller to have exclusive access for writes.
// The JIT reads binding table entries as native pointers.
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct DataAddress<T>(*mut T);

impl<T> DataAddress<T> {
    pub fn as_ptr(&self) -> *mut T {
        self.0
    }
}

impl DataAddress<DataValue> {
    /// # Safety
    /// The cell must be live and initialized, with no concurrent writer.
    pub(super) unsafe fn read(&self) -> DataValue {
        unsafe { self.0.read() }
    }

    /// # Safety
    /// The cell must be live, and the caller must have exclusive access.
    pub(super) unsafe fn write(&self, value: DataValue) {
        unsafe { self.0.write(value) };
    }
}

impl DataStorage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Safe to use without locks while a processor is running
    pub fn alloc_cell(&mut self, value: DataValue) -> DataAddress<DataValue> {
        const SLOTS_PER_BLOCK: usize = 4096 / std::mem::size_of::<DataValue>();
        if self
            .blocks
            .last()
            .is_none_or(|block| self.next_slot == block.len())
        {
            self.blocks.push(
                (0..SLOTS_PER_BLOCK)
                    .map(|_| UnsafeCell::new(DataValue::new_float(0.0)))
                    .collect(),
            );
            self.next_slot = 0;
        }

        let block = self.blocks.last().unwrap();
        // Access the slot through UnsafeCell without borrowing the slice mutably
        let addr = block[self.next_slot].get();
        // This slot has never been used before so it's safe to write to
        unsafe { addr.write(value) };
        self.next_slot += 1;
        DataAddress(addr)
    }

    pub fn empty_array(&mut self) -> DataAddress<f32> {
        self.push_array(None, &[])
    }

    pub fn alloc_array(&mut self, source: DataArrayHandle) -> DataAddress<f32> {
        if let Some(existing) = self.arrays.iter().find(|array| {
            array
                .source
                .as_ref()
                .is_some_and(|bound| bound.same_source(&source))
        }) {
            return existing.address();
        }

        self.push_array(Some(source.clone()), source.samples())
    }

    fn push_array(&mut self, source: Option<DataArrayHandle>, data: &[f32]) -> DataAddress<f32> {
        self.arrays.push(DataArray {
            source,
            // JIT array operations expect a length header followed by the samples
            data: std::iter::once(f32::from_bits(data.len() as u32))
                .chain(data.iter().copied())
                .map(UnsafeCell::new)
                .collect(),
            #[cfg(test)]
            lifetime: std::sync::Arc::new(()),
        });
        self.arrays.last().unwrap().address()
    }

    #[cfg(test)]
    pub(super) fn array_lifetime(&self, index: usize) -> std::sync::Weak<()> {
        let array = self
            .arrays
            .iter()
            .filter(|array| array.source.is_some())
            .nth(index)
            .unwrap();
        std::sync::Arc::downgrade(&array.lifetime)
    }
}

impl DataArray {
    fn address(&self) -> DataAddress<f32> {
        DataAddress(UnsafeCell::raw_get(self.data.as_ptr()))
    }
}
