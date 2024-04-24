pub struct SignalProcessorContext {
    pub p_output: *mut f32,
}
unsafe impl Send for SignalProcessorContext {}
unsafe impl Sync for SignalProcessorContext {}

type ProcessFn = unsafe extern "C" fn(*mut f32);

pub struct SignalProcessor {
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    num_outputs: usize,
    data: Vec<f32>,
}

impl SignalProcessor {
    pub fn new(
        ctx: Box<SignalProcessorContext>,
        function: ProcessFn,
        num_outputs: u32,
        data_length: u32,
    ) -> Self {
        SignalProcessor {
            ctx,
            function,
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

    #[allow(dead_code)]
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}
