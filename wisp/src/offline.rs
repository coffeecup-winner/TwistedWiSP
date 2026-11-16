//! Hardware-independent source loading and compilation for tests and offline hosts.
use crate::{
    core::WispContext,
    runner::{context::WispRuntimeContext, runtime::compile_context},
    SignalProcessor,
};
use std::path::Path;
pub struct OfflineSession {
    context: WispContext,
}
impl OfflineSession {
    pub fn new(
        sample_rate: u32,
        outputs: u32,
        core_path: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if sample_rate == 0 || outputs == 0 {
            return Err("Sample rate and output count must be positive".into());
        }
        let mut context = WispContext::new(outputs, sample_rate);
        context.add_builtin_functions();
        context.load_core_functions(core_path)?;
        Ok(Self { context })
    }
    pub fn load_flow_text(&mut self, text: &str) -> Result<String, Box<dyn std::error::Error>> {
        self.context.load_flow_text(text)
    }
    pub fn compile(&self, name: &str) -> Result<SignalProcessor, Box<dyn std::error::Error>> {
        Ok(compile_context(
            &self.context,
            &mut WispRuntimeContext::new(),
            name,
        )?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CallIndex, DataIndex};
    fn session() -> OfflineSession {
        OfflineSession::new(
            44100,
            2,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("core"),
        )
        .unwrap()
    }
    #[test]
    fn control_changes_actual_jit_output_and_processor_outlives_session() {
        let mut session = session();
        let name = session.load_flow_text("[flow]\nname='test'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=0\nto=1\noutput_index=0\ninput_index=0\n").unwrap();
        let mut processor = session.compile(&name).unwrap();
        drop(session);
        assert!(processor
            .set_data_value(&name, CallIndex(0), DataIndex(0), 0.25)
            .is_some());
        let mut output = [0.0; 2];
        processor.process_one(&mut output);
        assert_eq!(output, [0.25, 0.25]);
        assert!(processor
            .set_data_value(&name, CallIndex(0), DataIndex(8), 1.0)
            .is_none());
        assert!(processor
            .set_data_value("wrong-root", CallIndex(0), DataIndex(0), 1.0)
            .is_none());
        processor
            .set_data_value(&name, CallIndex(0), DataIndex(0), 0.75)
            .unwrap();
        processor.process_one(&mut output);
        assert_eq!(output, [0.75, 0.75]);
    }
    #[test]
    fn existing_control_and_nested_phasor_flows_compile_and_run() {
        for fixture in ["control.twf", "phasor_test.twf"] {
            let mut session = session();
            let text = std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../wisp_gui")
                    .join(fixture),
            )
            .unwrap();
            let name = session.load_flow_text(&text).unwrap();
            let mut processor = session.compile(&name).unwrap();
            let mut output = [0.0; 32];
            processor.process_all(&mut output);
            assert!(output.iter().all(|v| v.is_finite()));
        }
    }
    #[test]
    fn stateless_root_and_invalid_graphs_do_not_crash_jit() {
        let mut session = session();
        let name = session
            .load_flow_text("[flow]\nname='empty'\nnodes=[]\nedges=[]")
            .unwrap();
        let mut processor = session.compile(&name).unwrap();
        let mut output = [1.0; 2];
        processor.process_one(&mut output);
        assert_eq!(output, [0.0; 2]);
        let name = session
            .load_flow_text("[flow]\nname='bad'\n[[flow.nodes]]\ntext='missing'\nedges=[]")
            .err();
        assert!(name.is_some());
    }
}
