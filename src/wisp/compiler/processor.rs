pub struct Globals {
    pub p_data: *mut f32,
    pub p_output: *mut f32,
}
unsafe impl Send for Globals {}
unsafe impl Sync for Globals {}

type ProcessFn = unsafe extern "C" fn(data: *mut f32, output: *mut f32);

pub struct SignalProcessor {
    _globals: Box<Globals>,
    function: ProcessFn,
    num_outputs: usize,
    data: Vec<f32>,
}

impl SignalProcessor {
    pub fn new(
        globals: Box<Globals>,
        function: ProcessFn,
        num_outputs: usize,
        data_length: usize,
    ) -> Self {
        SignalProcessor {
            _globals: globals,
            function,
            num_outputs,
            data: vec![0.0; data_length],
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
        unsafe {
            (self.function)(self.data.as_mut_ptr(), output.as_mut_ptr());
        }
    }

    #[allow(dead_code)]
    pub fn data(&self) -> &[f32] {
        &self.data
    }
}
