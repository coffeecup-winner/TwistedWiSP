//! Hardware-independent source loading and compilation for tests and offline hosts.
use crate::{
    SignalProcessor,
    core::WispContext,
    runner::{context::WispRuntimeContext, runtime::compile_context},
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
        assert!(
            processor
                .set_data_value(&name, CallIndex(0), DataIndex(0), 0.25)
                .is_some()
        );
        let mut output = [0.0; 2];
        processor.process_one(&mut output);
        assert_eq!(output, [0.25, 0.25]);
        assert!(
            processor
                .set_data_value(&name, CallIndex(0), DataIndex(8), 1.0)
                .is_none()
        );
        assert!(
            processor
                .set_data_value("wrong-root", CallIndex(0), DataIndex(0), 1.0)
                .is_none()
        );
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
                    .join("../wisp_gui/examples")
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
    #[test]
    fn conditional_outputs_follow_both_paths_and_nested_branches() {
        use crate::core::{CodeFunctionParseResult, CodeFunctionParser, Function};
        for body in [
            "if %positive\n store #out, 10\nelse\n store #out, 20\nend",
            "if %positive\n add %merged, $value, 10\nelse\n sub %merged, 20, $value\nend\n mul %scaled, %merged, 2\n store #out, %scaled",
            "if %positive\n add %left, $value, 10\n store #out, %left\nelse\n sub %right, 20, $value\n store #out, %right\nend",
            "if %positive\n cmp.gt %large, $value, 1\n if %large\n store #out, 10\n else\n store #out, 11\n end\nelse\n store #out, 20\nend",
        ] {
            let mut session = session();
            let source = format!(
                "func branch(value: float) -> (out: float)\nbegin\ncmp.gt %positive, $value, 0\n{body}\nend"
            );
            let CodeFunctionParseResult::Function(function) =
                CodeFunctionParser::new(&source).parse_function().unwrap()
            else {
                panic!()
            };
            session.context.add_function(Function::Code(function));
            let root = session.load_flow_text("[flow]\nname='branches'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='branch'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=0\nto=1\noutput_index=0\ninput_index=0\n[[flow.edges]]\nfrom=1\nto=2\noutput_index=0\ninput_index=0\n").unwrap();
            let mut processor = session.compile(&root).unwrap();
            for value in [-2., 0.5, 2.] {
                processor
                    .set_data_value(&root, CallIndex(0), DataIndex(0), value)
                    .unwrap();
                let mut output = [0.; 2];
                processor.process_one(&mut output);
                let expected = if body.contains("%merged") {
                    2. * if value > 0. { value + 10. } else { 20. - value }
                } else if body.contains("%left") {
                    if value > 0. { value + 10. } else { 20. - value }
                } else if value <= 0. {
                    20.
                } else if body.contains("%large") && value <= 1. {
                    11.
                } else {
                    10.
                };
                assert_eq!(output, [expected; 2], "{body}, input={value}");
            }
        }
    }

    fn array_flow(operation: &str) -> String {
        format!(
            "[flow]\nname='arrays'\n[[flow.nodes]]\ntext='buffer'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='{operation}'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=0\nto=2\noutput_index=0\ninput_index=0\n[[flow.edges]]\nfrom=1\nto=2\noutput_index=0\ninput_index=1\n[[flow.edges]]\nfrom=2\nto=3\noutput_index=0\ninput_index=0\n"
        )
    }
    #[test]
    fn checked_array_jit_indices_interpolation_and_owned_bindings() {
        for operation in ["read", "sample"] {
            let mut session = session();
            let name = session.load_flow_text(&array_flow(operation)).unwrap();
            let source = crate::DataArrayHandle::from(&[10., 20.][..]);
            let mut first = session.compile(&name).unwrap();
            let mut second = session.compile(&name).unwrap();
            first
                .set_data_array(&name, CallIndex(0), DataIndex(0), source.clone())
                .unwrap();
            second
                .set_data_array(&name, CallIndex(0), DataIndex(0), source)
                .unwrap();
            drop(session);
            let cases: &[(f32, f32)] = if operation == "sample" {
                &[
                    (0., 10.),
                    (0.125, 12.5),
                    (0.5, 20.),
                    (0.875, 12.5),
                    (1., 10.),
                    (-0.1, 0.),
                    (1.1, 0.),
                ]
            } else {
                &[
                    (0., 10.),
                    (0.9, 10.),
                    (1., 20.),
                    (2., 0.),
                    (-0.1, 0.),
                    (f32::MAX, 0.),
                ]
            };
            for &(index, expected) in cases
                .iter()
                .chain([(f32::NAN, 0.), (f32::INFINITY, 0.), (f32::NEG_INFINITY, 0.)].iter())
            {
                first
                    .set_data_value(&name, CallIndex(1), DataIndex(0), index)
                    .unwrap();
                let mut output = [0.; 2];
                first.process_one(&mut output);
                assert_eq!(output, [expected; 2], "{operation}({index})");
            }
            first
                .set_data_array(
                    &name,
                    CallIndex(0),
                    DataIndex(0),
                    crate::DataArrayHandle::from(&[][..]),
                )
                .unwrap();
            let mut output = [1.; 2];
            first.process_one(&mut output);
            assert_eq!(output, [0.; 2]);
            second.process_one(&mut output);
            assert_eq!(output, [10.; 2]);
        }
        let mut session = session();
        let text = array_flow("read")
            .replace("text='buffer'", "text='control'")
            .replace("text='read'", "text='floor'")
            .replace("input_index=1", "input_index=0");
        // Use a simple scalar graph to test negative floor through the JIT.
        let text = text.replace(
            "[[flow.edges]]\nfrom=0\nto=2\noutput_index=0\ninput_index=0\n",
            "",
        );
        let name = session.load_flow_text(&text).unwrap();
        let mut p = session.compile(&name).unwrap();
        for (value, expected) in [
            (-1.5, -2.),
            (-1., -1.),
            (-0.25, -1.),
            (0., 0.),
            (0.25, 0.),
            (1.5, 1.),
        ] {
            p.set_data_value(&name, CallIndex(1), DataIndex(0), value)
                .unwrap();
            let mut out = [0.; 2];
            p.process_one(&mut out);
            assert_eq!(out, [expected; 2]);
        }
    }
    #[test]
    fn checked_writes_and_mutable_arrays_are_private_to_processors() {
        let mut session = session();
        let source = crate::DataArrayHandle::from(&[10., 20.][..]);
        let text = array_flow("read").replace(
            "[[flow.edges]]\nfrom=1\nto=2\noutput_index=0\ninput_index=1\n",
            "",
        ) + "\n[[flow.nodes]]\ntext='write'\n[[flow.nodes]]\ntext='control'\n[[flow.edges]]\nfrom=0\nto=4\noutput_index=0\ninput_index=0\n[[flow.edges]]\nfrom=1\nto=4\noutput_index=0\ninput_index=1\n[[flow.edges]]\nfrom=5\nto=4\noutput_index=0\ninput_index=2\n";
        let root = session.load_flow_text(&text).unwrap();
        let mut first = session.compile(&root).unwrap();
        let mut second = session.compile(&root).unwrap();
        for p in [&mut first, &mut second] {
            p.set_data_array(&root, CallIndex(0), DataIndex(0), source.clone())
                .unwrap();
            p.set_data_value(&root, CallIndex(5), DataIndex(0), 100.)
                .unwrap();
            p.set_data_value(&root, CallIndex(1), DataIndex(0), -1.)
                .unwrap();
        }
        drop(session);
        let mut out = [0.; 4];
        for invalid in [
            -1.,
            -0.25,
            2.,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
        ] {
            first
                .set_data_value(&root, CallIndex(1), DataIndex(0), invalid)
                .unwrap();
            first.process_all(&mut out);
            assert!(out.iter().all(|v| *v == 10.), "{invalid}: {out:?}");
        }
        first
            .set_data_value(&root, CallIndex(1), DataIndex(0), 0.)
            .unwrap();
        first.process_all(&mut out);
        assert_eq!(out[3], 100.);
        second.process_all(&mut out);
        assert!(out.iter().all(|v| *v == 10.));
        assert_eq!(source.samples(), &[10., 20.]);
        first
            .set_data_array(
                &root,
                CallIndex(0),
                DataIndex(0),
                crate::DataArrayHandle::from(&[][..]),
            )
            .unwrap();
        first.process_all(&mut out);
        assert_eq!(out, [0.; 4]);
    }
}
