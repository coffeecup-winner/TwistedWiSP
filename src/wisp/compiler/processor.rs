pub struct SignalProcessorContext {
    pub p_data: *mut f32,
    pub p_output: *mut f32,
}
unsafe impl Send for SignalProcessorContext {}
unsafe impl Sync for SignalProcessorContext {}

type ProcessFn = unsafe extern "C" fn();

pub struct SignalProcessor {
    ctx: Box<SignalProcessorContext>,
    function: ProcessFn,
    num_outputs: usize,
    data: Vec<f32>,
}

impl SignalProcessor {
    pub fn new(
        mut ctx: Box<SignalProcessorContext>,
        function: ProcessFn,
        num_outputs: usize,
        data_length: usize,
    ) -> Self {
        let mut data = vec![0.0; data_length];
        ctx.p_data = data.as_mut_ptr();
        SignalProcessor {
            ctx,
            function,
            num_outputs,
            data,
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
            (self.function)();
        }
    }

    #[allow(dead_code)]
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}
