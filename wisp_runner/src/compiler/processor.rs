use std::collections::HashMap;

use twisted_wisp_ir::CallId;

use super::data_layout::FunctionDataLayout;

pub struct SignalProcessorContext {
    pub p_output: *mut f32,
}
unsafe impl Send for SignalProcessorContext {}
unsafe impl Sync for SignalProcessorContext {}

type ProcessFn = unsafe extern "C" fn(*mut f32);

pub struct SignalProcessor {
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    data_layout: HashMap<String, FunctionDataLayout>,
    num_outputs: usize,
    data: Vec<f32>,
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
        }
    }

    pub fn process(&mut self, output: &mut [f32]) {
        // TODO: Return error instead?
        assert_eq!(0, output.len() % self.num_outputs);
        for chunk in output.chunks_mut(self.num_outputs) {
            self.process_one(chunk);
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
}
