use std::collections::HashMap;

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

pub struct SignalProcessor {
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    name: String,
    data_layout: DataLayout,
    num_outputs: usize,
    // Fields below are mutable and need to be copy over to the new instance
    data: Vec<DataValue>,
    watch_id_gen: u32,
    watches: HashMap<WatchIndex, Watch>,
    elapsed_ticks: u32,
}

impl SignalProcessor {
    pub fn new(
        ctx: Box<SignalProcessorContext>,
        name: &str,
        function: ProcessFn,
        data_layout: DataLayout,
        num_outputs: u32,
    ) -> Self {
        let data = data_layout.create_data(name);
        SignalProcessor {
            ctx,
            function,
            name: name.to_string(),
            data_layout,
            num_outputs: num_outputs as usize,
            data,
            watch_id_gen: 0,
            watches: HashMap::new(),
            elapsed_ticks: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn process(&mut self, output: &mut [f32]) {
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

            self.elapsed_ticks += 1;
        }
    }

    pub fn process_one(&mut self, output: &mut [f32]) {
        self.ctx.p_output = output.as_mut_ptr();
        unsafe {
            (self.function)(self.data.as_mut_ptr());
        }
    }

    pub fn copy_from(&mut self, other: SignalProcessor) {
        let other_layout = other.get_data_layout();
        let own_layout = self.get_data_layout();

        for (key, offset) in other_layout.iter() {
            if own_layout.contains_key(key) {
                self.data[own_layout[key] as usize] = other.data[*offset as usize];
            }
        }

        let mut other_offset_to_layout_key = HashMap::new();
        for (k, v) in other_layout {
            other_offset_to_layout_key.insert(v, k);
        }

        for (idx, watch) in other.watches {
            let key = &other_offset_to_layout_key[&watch.data_offset];
            if let Some(offset) = own_layout.get(key) {
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

    fn get_data_layout(&self) -> HashMap<String, u32> {
        let mut layout = HashMap::new();
        self.build_data_layout(&self.name, "", 0, &mut layout);
        layout
    }

    fn build_data_layout(
        &self,
        name: &str,
        prefix: &str,
        mut current_offset: u32,
        layout: &mut HashMap<String, u32>,
    ) {
        let data_layout = self.data_layout.get(name).unwrap();
        for (id, offset) in &data_layout.own_data_items {
            current_offset += offset.offset;
            layout.insert(format!("{}{}@{}", prefix, name, id.0), current_offset);
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
        _data_idx: DataIndex,
        value: f32,
    ) -> Option<()> {
        let data_layout = self.data_layout.get(name)?;
        let (_, child_offset) = data_layout.children_data_items.get(&call_idx)?;
        // TODO: Add data index within child using idx
        let data = self.data.get_mut(*child_offset as usize)?;
        *data = DataValue::new_float(value);
        Some(())
    }

    pub fn set_data_array(
        &mut self,
        name: &str,
        call_idx: CallIndex,
        _data_idx: DataIndex,
        array: DataArrayHandle,
    ) -> Option<()> {
        let data_layout = self.data_layout.get(name)?;
        let (_, child_offset) = data_layout.children_data_items.get(&call_idx)?;
        // TODO: Add data index within child using idx
        let data = self.data.get_mut(*child_offset as usize)?;
        *data = DataValue::new_array(array);
        Some(())
    }

    pub fn watch_data_value(
        &mut self,
        name: &str,
        call_idx: CallIndex,
        _data_idx: DataIndex,
        only_last_value: bool,
    ) -> Option<WatchIndex> {
        let data_layout = self.data_layout.get(name)?;
        let (_, child_offset) = data_layout.children_data_items.get(&call_idx)?;
        // TODO: Add data index within child using idx
        let idx = WatchIndex(self.watch_id_gen);
        self.watch_id_gen += 1;
        self.watches.insert(
            idx,
            Watch {
                data_offset: *child_offset,
                rate: 1,
                data: if only_last_value {
                    WatchedData::LastValue(self.data.get(*child_offset as usize)?.as_float())
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
