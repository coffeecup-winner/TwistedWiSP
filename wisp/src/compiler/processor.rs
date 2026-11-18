use std::collections::HashMap;

use inkwell::{
    execution_engine::ExecutionEngine,
    llvm_sys::execution_engine::{LLVMDisposeExecutionEngine, LLVMExecutionEngineRef},
};
use ringbuffer::{AllocRingBuffer, RingBuffer};

use crate::{
    compiler::DataArrayHandle,
    runner::engine::{DataIndex, WatchIndex, WatchedDataValues},
    CallIndex,
};

use super::{
    data_layout::DataLayout,
    data_storage::{DataAddress, DataStorage, DataValue},
};

pub struct SignalProcessorContext {
    pub p_output: *mut f32,
    pub noise_state: u64,
}
unsafe impl Send for SignalProcessorContext {}
unsafe impl Sync for SignalProcessorContext {}

/// Advance processor-owned RNG state without allocation or thread-local state.
///
/// # Safety
/// `state` must point to a valid, aligned u64 exclusively owned by this invocation.
pub(crate) unsafe extern "C" fn noise(state: *mut u64) -> f32 {
    // The JIT passes its owning processor context's stable boxed state address.
    let state = unsafe { &mut *state };
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    let bits = (*state >> 40) as u32;
    bits as f32 * (2.0 / 16_777_215.0) - 1.0
}

type ProcessFn = unsafe extern "C" fn(*const DataAddress<DataValue>);

pub(crate) struct Watch {
    data_offset: u32,
    rate: u32,
    data: WatchedData,
}

enum WatchedData {
    LastValue(f32),
    Buffer(AllocRingBuffer<f32>),
}

impl Watch {
    pub(crate) fn prepare(last: bool) -> Self {
        Self {
            data_offset: 0,
            rate: 1,
            data: if last {
                WatchedData::LastValue(0.)
            } else {
                WatchedData::Buffer(AllocRingBuffer::new(4096))
            },
        }
    }
}

pub(crate) struct WatchBatch {
    slots: Vec<(WatchIndex, Vec<f32>)>,
    len: usize,
}
impl WatchBatch {
    pub(crate) fn new() -> Self {
        Self {
            slots: (0..128)
                .map(|_| (WatchIndex(0), Vec::with_capacity(4096)))
                .collect(),
            len: 0,
        }
    }
    pub(crate) fn values(&self) -> WatchedDataValues {
        WatchedDataValues {
            values: self.slots[..self.len].iter().cloned().collect(),
        }
    }
}

// We need this separate holder for the EE reference because inkwell's ExecutionEngine
// has a lifetime parameter to ensure that the context is not dropped before the EE,
// but we're doing it ourselves and having this lifetime parameter makes things
// unnecessarily complicated for the users.
struct ExecutionEngineRef {
    ee: LLVMExecutionEngineRef,
    // Dropped after LLVMDisposeExecutionEngine in Drop, never shared with another compiler.
    context_owner: Option<crate::runner::runtime::WispExecutionContext>,
}

impl ExecutionEngineRef {
    pub fn new(ee: ExecutionEngine) -> Self {
        let ee_ref = ee.as_mut_ptr();
        std::mem::forget(ee);
        ExecutionEngineRef {
            ee: ee_ref,
            context_owner: None,
        }
    }
}

// The execution engine and its exclusively owned, immutable context move together.
// They are not accessed concurrently: compilation finishes before transfer to audio.
unsafe impl Send for ExecutionEngineRef {}

impl Drop for ExecutionEngineRef {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeExecutionEngine(self.ee);
        }
    }
}

pub struct SignalProcessor {
    _ee: ExecutionEngineRef,
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    name: String,
    data_layout: DataLayout,
    num_outputs: usize,
    bindings: Vec<DataAddress<DataValue>>,
    storage: DataStorage,
    watch_id_gen: u32,
    watches: HashMap<WatchIndex, Watch>,
    elapsed_ticks: u32,
}

// Processing and state updates require exclusive access to this processor.
unsafe impl Send for SignalProcessor {}

impl SignalProcessor {
    pub fn new(
        ee: ExecutionEngine,
        ctx: Box<SignalProcessorContext>,
        name: &str,
        function: ProcessFn,
        data_layout: DataLayout,
        num_outputs: u32,
    ) -> Self {
        let mut storage = DataStorage::new();
        let bindings = data_layout.create_bindings(name, &mut storage);
        SignalProcessor {
            _ee: ExecutionEngineRef::new(ee),
            ctx,
            function,
            name: name.to_string(),
            data_layout,
            num_outputs: num_outputs as usize,
            bindings,
            storage,
            watch_id_gen: 0,
            watches: HashMap::with_capacity(128),
            elapsed_ticks: 0,
        }
    }

    pub(crate) fn retain_execution_context(
        &mut self,
        context: crate::runner::runtime::WispExecutionContext,
    ) {
        self._ee.context_owner = Some(context);
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn output_channels(&self) -> usize {
        self.num_outputs
    }

    pub fn process_one_checked(&mut self, output: &mut [f32]) -> Result<(), &'static str> {
        if output.len() < self.num_outputs {
            return Err("Output buffer is too small");
        }
        self.process_one(output);
        Ok(())
    }

    pub fn process_all_checked(&mut self, output: &mut [f32]) -> Result<(), &'static str> {
        if !output.len().is_multiple_of(self.num_outputs) {
            return Err("Output buffer must contain complete frames");
        }
        self.process_all(output);
        Ok(())
    }

    pub fn process_all(&mut self, output: &mut [f32]) {
        // TODO: Return error instead?
        assert_eq!(0, output.len() % self.num_outputs);
        for chunk in output.chunks_mut(self.num_outputs) {
            // Capture watch values before processing to have 0-based sample index
            for watch in &mut self.watches.values_mut() {
                if self.elapsed_ticks.is_multiple_of(watch.rate) {
                    // This processor keeps the bound cells alive and executes DSP serially.
                    let value = unsafe { self.bindings[watch.data_offset as usize].read() };
                    match watch.data {
                        WatchedData::LastValue(ref mut v) => *v = value.as_float(),
                        WatchedData::Buffer(ref mut buffer) => {
                            buffer.enqueue(value.as_float());
                        }
                    }
                }
            }

            self.process_one(chunk);

            self.elapsed_ticks = self.elapsed_ticks.wrapping_add(1);
        }
    }

    pub fn process_one(&mut self, output: &mut [f32]) {
        assert!(
            output.len() >= self.num_outputs,
            "Output buffer is too small"
        );
        output[..self.num_outputs].fill(0.0);
        self.ctx.p_output = output.as_mut_ptr();
        unsafe {
            (self.function)(self.bindings.as_ptr());
        }
    }

    pub fn set_data_value(
        &mut self,
        name: &str,
        call_idx: CallIndex,
        data_idx: DataIndex,
        value: f32,
    ) -> Option<()> {
        if name != self.name {
            return None;
        }
        let layout = self.data_layout.get(name)?;
        let (child, base) = layout.children_data_items.get(&call_idx)?;
        let item = self
            .data_layout
            .get(child)?
            .own_data_items
            .get(&crate::ir::DataRef(data_idx.0))?;
        if !matches!(item.type_, crate::ir::IRFunctionDataType::Float) {
            return None;
        }
        let address = self.bindings.get((*base + item.offset) as usize)?;
        // The arena owns this cell and &mut self excludes concurrent processing.
        unsafe { address.write(DataValue::new_float(value)) };
        Some(())
    }

    pub fn set_data_array(
        &mut self,
        name: &str,
        call_idx: CallIndex,
        data_idx: DataIndex,
        array: DataArrayHandle,
    ) -> Option<()> {
        if name != self.name {
            return None;
        }
        let layout = self.data_layout.get(name)?;
        let (child, base) = layout.children_data_items.get(&call_idx)?;
        let item = self
            .data_layout
            .get(child)?
            .own_data_items
            .get(&crate::ir::DataRef(data_idx.0))?;
        if !matches!(item.type_, crate::ir::IRFunctionDataType::Array) {
            return None;
        }
        let address = self.bindings.get((*base + item.offset) as usize)?;
        let samples = self.storage.alloc_array(array).as_ptr();
        // The arena owns both allocations and &mut self excludes concurrent processing.
        unsafe { address.write(DataValue::new_array(samples)) };
        Some(())
    }

    pub(crate) fn watch_prepared(
        &mut self,
        name: &str,
        call_idx: CallIndex,
        data_idx: DataIndex,
        prepared: &mut Option<Watch>,
    ) -> Option<WatchIndex> {
        if name != self.name {
            return None;
        }
        let layout = self.data_layout.get(name)?;
        let (child, base) = layout.children_data_items.get(&call_idx)?;
        let item = self
            .data_layout
            .get(child)?
            .own_data_items
            .get(&crate::ir::DataRef(data_idx.0))?;
        if !matches!(item.type_, crate::ir::IRFunctionDataType::Float) {
            return None;
        }
        let child_offset = base + item.offset;
        if self.watches.len() >= 128 {
            return None;
        }
        let idx = WatchIndex(self.watch_id_gen);
        self.watch_id_gen = self.watch_id_gen.checked_add(1)?;
        let mut watch = prepared.take()?;
        watch.data_offset = child_offset;
        if let WatchedData::LastValue(value) = &mut watch.data {
            let address = self.bindings.get(child_offset as usize)?;
            // This processor owns the cell and is the only writer.
            *value = unsafe { address.read() }.as_float();
        }
        self.watches.insert(idx, watch);
        Some(idx)
    }

    pub(crate) fn query_into(&mut self, batch: &mut WatchBatch) {
        batch.len = self.watches.len();
        for ((id, samples), (watch_id, watch)) in batch.slots.iter_mut().zip(&mut self.watches) {
            *id = *watch_id;
            samples.clear();
            match &mut watch.data {
                WatchedData::LastValue(value) => samples.push(*value),
                WatchedData::Buffer(buffer) => samples.extend(buffer.drain()),
            }
            for value in samples {
                if !value.is_finite() {
                    *value = 0.;
                }
            }
        }
    }
}

#[cfg(test)]
mod array_tests {
    use crate::{CallIndex, DataArrayHandle, DataIndex, OfflineSession};
    #[test]
    fn private_arrays_are_released_with_their_processor() {
        let mut session = OfflineSession::new(
            44100,
            1,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
        )
        .unwrap();
        let root = session
            .load_flow_text("[flow]\nname='lifetime'\n[[flow.nodes]]\ntext='buffer'\n")
            .unwrap();
        let mut p = session.compile(&root).unwrap();
        let source = DataArrayHandle::from(&[1., 2.][..]);
        p.set_data_array(&root, CallIndex(0), DataIndex(0), source.clone())
            .unwrap();
        let old = p.storage.array_lifetime(0);
        // Rebinding past the old cap must also keep every prior allocation alive.
        for value in 1..=128 {
            p.set_data_array(
                &root,
                CallIndex(0),
                DataIndex(0),
                DataArrayHandle::from(&[value as f32][..]),
            )
            .unwrap();
        }
        assert!(old.upgrade().is_some());
        let current = p.storage.array_lifetime(128);
        drop(session);
        assert!(current.upgrade().is_some());
        drop(p);
        assert!(old.upgrade().is_none());
        assert!(current.upgrade().is_none());
    }
}

#[cfg(test)]
mod noise_tests {
    use super::noise;
    use crate::OfflineSession;

    #[test]
    fn jit_noise_calls_rust_builtin_with_private_processor_state() {
        let mut session = OfflineSession::new(
            44100,
            1,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
        )
        .unwrap();
        let root = session.load_flow_text("[flow]\nname='noise_builtin_test'\n[[flow.nodes]]\ntext='noise'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=0\nto=1\noutput_index=0\ninput_index=0\n").unwrap();
        let mut first = session.compile(&root).unwrap();
        let mut second = session.compile(&root).unwrap();
        first.ctx.noise_state = 1;
        second.ctx.noise_state = 1;
        drop(session);
        let mut expected_state = 1u64;
        let mut output = [0.];
        for _ in 0..1024 {
            // SAFETY: expected_state is a live, exclusively borrowed u64.
            let expected = unsafe { noise(&mut expected_state) };
            first.process_one(&mut output);
            assert_eq!(output[0], expected);
            assert!((-1.0..=1.0).contains(&output[0]));
        }
        assert_eq!(first.ctx.noise_state, expected_state);
        assert_eq!(second.ctx.noise_state, 1);
        expected_state = 1;
        second.process_one(&mut output);
        assert_eq!(output[0], unsafe { noise(&mut expected_state) });
    }
}
