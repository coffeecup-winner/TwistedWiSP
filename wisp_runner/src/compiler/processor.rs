use std::collections::HashMap;

use ringbuffer::{AllocRingBuffer, RingBuffer};
use twisted_wisp_ir::CallId;
use twisted_wisp_protocol::{WatchIndex, WatchedDataValues};

use super::data_layout::FunctionDataLayout;

pub struct SignalProcessorContext {
    pub p_output: *mut f32,
}
unsafe impl Send for SignalProcessorContext {}
unsafe impl Sync for SignalProcessorContext {}

type ProcessFn = unsafe extern "C" fn(*mut f32);

struct Watch {
    data_offset: u32,
    rate: u32,
    history: AllocRingBuffer<f32>,
}

pub struct SignalProcessor {
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    data_layout: HashMap<String, FunctionDataLayout>,
    num_outputs: usize,
    data: Vec<f32>,
    watch_id_gen: u32,
    watches: HashMap<WatchIndex, Watch>,
    elapsed_ticks: u32,
}

impl SignalProcessor {
    pub fn new(
        ctx: Box<SignalProcessorContext>,
        name: &str,
        function: ProcessFn,
        data_layout: HashMap<String, FunctionDataLayout>,
        num_outputs: u32,
    ) -> Self {
        let data_length = data_layout.get(name).map_or(0, |l| l.total_size);
        SignalProcessor {
            ctx,
            function,
            data_layout,
            num_outputs: num_outputs as usize,
            data: vec![0.0; data_length as usize],
            watch_id_gen: 0,
            watches: HashMap::new(),
            elapsed_ticks: 0,
        }
    }

    pub fn process(&mut self, output: &mut [f32]) {
        // TODO: Return error instead?
        assert_eq!(0, output.len() % self.num_outputs);
        for chunk in output.chunks_mut(self.num_outputs) {
            // Capture watch values before processing to have 0-based sample index
            for watch in &mut self.watches.values_mut() {
                if self.elapsed_ticks % watch.rate == 0 {
                    watch.history.push(self.data[watch.data_offset as usize]);
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

    pub fn set_data_value(
        &mut self,
        name: String,
        id: CallId,
        _idx: u32,
        value: f32,
    ) -> Option<()> {
        let data_layout = self.data_layout.get(&name)?;
        let child_offset = data_layout.children_data_offsets.get(&id)?;
        // TODO: Add data index within child using idx
        let data = self.data.get_mut(*child_offset as usize)?;
        *data = value;
        Some(())
    }

    #[allow(dead_code)]
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    pub fn watch_data_value(&mut self, name: String, id: CallId, _idx: u32) -> Option<WatchIndex> {
        let data_layout = self.data_layout.get(&name)?;
        let child_offset = data_layout.children_data_offsets.get(&id)?;
        // TODO: Add data index within child using idx
        let idx = WatchIndex(self.watch_id_gen);
        self.watch_id_gen += 1;
        self.watches.insert(
            idx,
            Watch {
                data_offset: *child_offset,
                rate: 16,
                history: AllocRingBuffer::new(4096),
            },
        );
        Some(idx)
    }

    pub fn unwatch_data_value(&mut self, idx: WatchIndex) {
        self.watches.remove(&idx);
    }

    pub fn query_watched_data_value(&self) -> WatchedDataValues {
        let mut values = HashMap::new();
        for (idx, watch) in &self.watches {
            values.insert(*idx, watch.history.to_vec());
        }
        WatchedDataValues { values }
    }
}
