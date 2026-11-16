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

use super::data_layout::{DataLayout, DataValue};

pub struct SignalProcessorContext {
    pub p_output: *mut f32,
}
unsafe impl Send for SignalProcessorContext {}
unsafe impl Sync for SignalProcessorContext {}

type ProcessFn = unsafe extern "C" fn(*mut DataValue);

struct Watch {
    data_offset: u32,
    rate: u32,
    data: WatchedData,
}

enum WatchedData {
    LastValue(f32),
    Buffer(AllocRingBuffer<f32>),
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

struct OwnedArray {
    source: DataArrayHandle,
    samples: Box<[f32]>,
    #[cfg(test)]
    lifetime: std::sync::Arc<()>,
}
pub struct SignalProcessor {
    _ee: ExecutionEngineRef,
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    name: String,
    data_layout: DataLayout,
    num_outputs: usize,
    // Fields below are mutable and need to be copied over to the new instance
    data: Vec<DataValue>,
    // Keep replaced bindings too: another DSP state slot may have retained a
    // pointer to the previous allocation. All storage is exclusive to this processor.
    arrays: Vec<OwnedArray>,
    watch_id_gen: u32,
    watches: HashMap<WatchIndex, Watch>,
    elapsed_ticks: u32,
}

impl SignalProcessor {
    pub fn new(
        ee: ExecutionEngine,
        ctx: Box<SignalProcessorContext>,
        name: &str,
        function: ProcessFn,
        data_layout: DataLayout,
        num_outputs: u32,
    ) -> Self {
        let data = data_layout.create_data(name);
        SignalProcessor {
            _ee: ExecutionEngineRef::new(ee),
            ctx,
            function,
            name: name.to_string(),
            data_layout,
            num_outputs: num_outputs as usize,
            data,
            arrays: vec![],
            watch_id_gen: 0,
            watches: HashMap::new(),
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

    pub fn process_all(&mut self, output: &mut [f32]) {
        // TODO: Return error instead?
        assert_eq!(0, output.len() % self.num_outputs);
        for chunk in output.chunks_mut(self.num_outputs) {
            // Capture watch values before processing to have 0-based sample index
            for watch in &mut self.watches.values_mut() {
                if self.elapsed_ticks % watch.rate == 0 {
                    let value = self.data[watch.data_offset as usize];
                    match watch.data {
                        WatchedData::LastValue(ref mut v) => *v = value.as_float(),
                        WatchedData::Buffer(ref mut buffer) => buffer.push(value.as_float()),
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
            (self.function)(self.data.as_mut_ptr());
        }
    }

    pub fn copy_from(&mut self, other: SignalProcessor) {
        let other_layout = other.get_data_layout();
        let own_layout = self.get_data_layout();

        let own_floats = self.float_offsets();
        let other_floats = other.float_offsets();
        for (key, offset) in other_layout.iter() {
            if let Some(own_offset) = own_layout.get(key) {
                if own_floats.contains(own_offset) && other_floats.contains(offset) {
                    self.data[*own_offset as usize] = other.data[*offset as usize];
                }
            }
        }

        let mut other_offset_to_layout_key = HashMap::new();
        for (k, v) in other_layout {
            other_offset_to_layout_key.insert(v, k);
        }

        for (idx, watch) in other.watches {
            let key = &other_offset_to_layout_key[&watch.data_offset];
            if let Some(offset) = own_layout
                .get(key)
                .filter(|offset| own_floats.contains(offset))
            {
                self.watches.insert(
                    idx,
                    Watch {
                        data_offset: *offset,
                        ..watch
                    },
                );
            }
        }

        self.watch_id_gen = other.watch_id_gen;
        self.elapsed_ticks = other.elapsed_ticks;
    }

    fn float_offsets(&self) -> std::collections::HashSet<u32> {
        fn collect(
            layouts: &DataLayout,
            name: &str,
            base: u32,
            result: &mut std::collections::HashSet<u32>,
        ) {
            if let Some(layout) = layouts.get(name) {
                for item in layout.own_data_items.values() {
                    if matches!(item.type_, crate::ir::IRFunctionDataType::Float) {
                        result.insert(base + item.offset);
                    }
                }
                for (child, offset) in layout.children_data_items.values() {
                    collect(layouts, child, base + offset, result);
                }
            }
        }
        let mut result = Default::default();
        collect(&self.data_layout, &self.name, 0, &mut result);
        result
    }
    fn get_data_layout(&self) -> HashMap<String, u32> {
        let mut layout = HashMap::new();
        self.build_data_layout(&self.name, "", 0, &mut layout);
        layout
    }

    fn build_data_layout(
        &self,
        name: &str,
        prefix: &str,
        current_offset: u32,
        layout: &mut HashMap<String, u32>,
    ) {
        let data_layout = self.data_layout.get(name).unwrap();
        for (id, offset) in &data_layout.own_data_items {
            layout.insert(
                format!("{}{}@{}", prefix, name, id.0),
                current_offset + offset.offset,
            );
        }
        for (id, (child_name, offset)) in &data_layout.children_data_items {
            self.build_data_layout(
                child_name,
                &format!("{}{}#{}.", prefix, name, id.0),
                current_offset + *offset,
                layout,
            );
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
        let data = self.data.get_mut((*base + item.offset) as usize)?;
        *data = DataValue::new_float(value);
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
        let data = self.data.get_mut((*base + item.offset) as usize)?;
        if let Some(existing) = self
            .arrays
            .iter_mut()
            .find(|existing| existing.source.same_source(&array))
        {
            *data = DataValue::new_array(existing.samples.as_mut_ptr());
            return Some(());
        }
        let length = u32::try_from(array.samples().len()).ok()?;
        let mut storage = Vec::with_capacity(array.samples().len() + 1);
        storage.push(f32::from_bits(length));
        storage.extend_from_slice(array.samples());
        let mut storage = storage.into_boxed_slice();
        *data = DataValue::new_array(storage.as_mut_ptr());
        self.arrays.push(OwnedArray {
            source: array,
            samples: storage,
            #[cfg(test)]
            lifetime: std::sync::Arc::new(()),
        });
        Some(())
    }

    pub fn watch_data_value(
        &mut self,
        name: &str,
        call_idx: CallIndex,
        data_idx: DataIndex,
        only_last_value: bool,
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
        self.watches.insert(
            idx,
            Watch {
                data_offset: child_offset,
                rate: 1,
                data: if only_last_value {
                    WatchedData::LastValue(self.data.get(child_offset as usize)?.as_float())
                } else {
                    WatchedData::Buffer(AllocRingBuffer::new(4096))
                },
            },
        );
        Some(idx)
    }

    pub fn unwatch_data_value(&mut self, idx: WatchIndex) {
        self.watches.remove(&idx);
    }

    pub fn query_watched_data_value(&mut self) -> WatchedDataValues {
        let mut values = HashMap::new();
        for (idx, watch) in &mut self.watches {
            let mut history = match watch.data {
                WatchedData::LastValue(v) => {
                    vec![v]
                }
                WatchedData::Buffer(ref mut buffer) => buffer.drain().collect(),
            };
            // NaNs are serialized as nulls and can't be deserialized as f32, easier to zero them out
            for v in history.iter_mut() {
                if v.is_nan() {
                    *v = 0.0;
                }
            }
            values.insert(*idx, history);
        }
        WatchedDataValues { values }
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
        let old = std::sync::Arc::downgrade(&p.arrays[0].lifetime);
        p.set_data_array(
            &root,
            CallIndex(0),
            DataIndex(0),
            DataArrayHandle::from(&[3., 4.][..]),
        )
        .unwrap();
        assert!(old.upgrade().is_some()); // Rebinding cannot free retained DSP aliases.
        let current = std::sync::Arc::downgrade(&p.arrays[1].lifetime);
        drop(session);
        assert!(current.upgrade().is_some());
        drop(p);
        assert!(old.upgrade().is_none());
        assert!(current.upgrade().is_none());
    }
}
