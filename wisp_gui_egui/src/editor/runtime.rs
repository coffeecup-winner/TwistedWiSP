//! Runtime implementations stay on the editor thread; tests inject a device-free fake.
use wisp::{CallIndex, DataIndex, TwistedWispEngine, TwistedWispEngineConfig};
pub trait EditorRuntime {
    fn buffers(&mut self, bindings: &[super::resources::BufferBinding]) -> Result<(), String> {
        if bindings.is_empty() {
            Ok(())
        } else {
            Err("Runtime cannot bind arrays".into())
        }
    }
    fn watch(&mut self, _root: &str, _call: u32, _last: bool) -> Result<wisp::WatchIndex, String> {
        Err("Watches unavailable".into())
    }
    fn query(&mut self) -> Result<wisp::WatchedDataValues, String> {
        Ok(Default::default())
    }
    fn midi_available(&self) -> bool {
        false
    }
    fn midi_learn(&mut self, _target: u64) -> Result<(), String> {
        Err("MIDI unavailable".into())
    }
    fn midi_targets(&self) -> Vec<u64> {
        vec![]
    }
    fn midi_cancel(&mut self) {}
    fn midi_remove(&mut self, _target: u64) {}
    fn midi_poll(&mut self) -> Vec<wisp::MidiObservation> {
        vec![]
    }
    fn scalar_try(&mut self, root: &str, call: u32, value: f32) -> Result<(), String> {
        self.scalar(root, call, value)
    }
    fn configuration(&self) -> (u32, u32);
    fn prepare(&mut self, flow: &str) -> Result<(), String>;
    fn install_prepared(&mut self) -> Result<(), String>;
    fn install(&mut self, flow: &str) -> Result<(), String> {
        self.stop()?;
        self.prepare(flow)?;
        self.install_prepared()
    }
    fn scalar(&mut self, root: &str, call: u32, value: f32) -> Result<(), String>;
    fn start(&mut self) -> Result<(), String>;
    fn stop(&mut self) -> Result<(), String>;
    fn healthy(&self) -> bool;
}
pub type RuntimeFactory =
    Box<dyn FnMut(&TwistedWispEngineConfig) -> Result<Box<dyn EditorRuntime>, String> + Send>;
pub fn live_factory() -> RuntimeFactory {
    Box::new(|config| {
        TwistedWispEngine::create(config)
            .map(|engine| {
                Box::new(LiveRuntime {
                    engine,
                    prepared: None,
                }) as Box<dyn EditorRuntime>
            })
            .map_err(|e| e.to_string())
    })
}
struct LiveRuntime {
    engine: TwistedWispEngine,
    prepared: Option<wisp::SignalProcessor>,
}
impl EditorRuntime for LiveRuntime {
    fn buffers(&mut self, bindings: &[super::resources::BufferBinding]) -> Result<(), String> {
        let p = self.prepared.as_mut().ok_or("No prepared processor")?;
        let root = p.name().to_owned();
        for binding in bindings {
            p.set_data_array(
                &root,
                CallIndex(binding.call),
                DataIndex(0),
                wisp::DataArrayHandle::from_shared(binding.audio.samples.clone()),
            )
            .ok_or("Invalid buffer address")?;
        }
        Ok(())
    }
    fn watch(&mut self, root: &str, call: u32, last: bool) -> Result<wisp::WatchIndex, String> {
        self.engine
            .runtime_watch_checked(root, CallIndex(call), last)
    }
    fn query(&mut self) -> Result<wisp::WatchedDataValues, String> {
        self.engine.runtime_query_checked()
    }
    fn midi_available(&self) -> bool {
        self.engine.midi_available()
    }
    fn midi_learn(&mut self, target: u64) -> Result<(), String> {
        self.engine.midi_learn(target)
    }
    fn midi_targets(&self) -> Vec<u64> {
        self.engine.midi_targets()
    }
    fn midi_cancel(&mut self) {
        self.engine.midi_cancel();
    }
    fn midi_remove(&mut self, target: u64) {
        self.engine.midi_remove(target);
    }
    fn midi_poll(&mut self) -> Vec<wisp::MidiObservation> {
        self.engine.midi_poll()
    }
    fn scalar_try(&mut self, root: &str, call: u32, value: f32) -> Result<(), String> {
        self.engine.runtime_scalar_try(root, CallIndex(call), value)
    }
    fn configuration(&self) -> (u32, u32) {
        self.engine.audio_configuration()
    }
    fn prepare(&mut self, text: &str) -> Result<(), String> {
        self.prepared = None;
        let name = self
            .engine
            .ctx_load_flow_from_text(text)
            .map_err(|e| e.to_string())?;
        self.prepared = Some(
            self.engine
                .runtime_compile_signal_processor(name)
                .map_err(|e| e.to_string())?,
        );
        Ok(())
    }
    fn install_prepared(&mut self) -> Result<(), String> {
        self.engine
            .runtime_install_checked(self.prepared.take().ok_or("No prepared processor")?)
    }
    fn scalar(&mut self, root: &str, call: u32, value: f32) -> Result<(), String> {
        self.engine
            .runtime_scalar_checked(root, CallIndex(call), DataIndex(0), value)
    }
    fn start(&mut self) -> Result<(), String> {
        self.engine.runtime_start_checked()
    }
    fn stop(&mut self) -> Result<(), String> {
        self.engine.runtime_stop_checked()
    }
    fn healthy(&self) -> bool {
        self.engine.runtime_healthy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires an audio device or ALSA null PCM configured by ALSA_CONFIG_PATH"]
    fn native_callback_start_scalar_stop_restart() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let config = TwistedWispEngineConfig {
            core_path: Some(root.join("wisp/core")),
            audio_output_channels: Some(2),
            audio_sample_rate: Some(44100),
            audio_buffer_size: Some(256),
            ..Default::default()
        };
        let mut runtime = live_factory()(&config).unwrap();
        runtime
            .install(&std::fs::read_to_string(root.join("wisp_gui/control.twf")).unwrap())
            .unwrap();
        runtime.scalar("control_test", 1, 0.103).unwrap();
        runtime.scalar("control_test", 4, 0.1).unwrap();
        runtime.start().unwrap();
        runtime.scalar("control_test", 1, 0.2).unwrap();
        runtime.stop().unwrap();
        runtime.scalar("control_test", 4, 0.05).unwrap();
        runtime.start().unwrap();
        runtime.stop().unwrap();
        let original = std::fs::read_to_string(root.join("wisp_gui/control.twf")).unwrap();
        let malformed = original.replacen("= ($0 + $1 / SampleRate) % 1.0", "= (", 1);
        assert!(runtime.prepare(&malformed).is_err());
        assert!(runtime.healthy());
        runtime.prepare(&original).unwrap();
        runtime.install_prepared().unwrap();
        runtime.scalar("control_test", 1, 0.1).unwrap();
        runtime.start().unwrap();
        runtime.stop().unwrap();
        assert!(runtime.healthy());
        drop(runtime);
    }
    #[test]
    #[ignore = "requires an audio device or ALSA null PCM"]
    fn native_callback_arrays_and_watches() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let config = TwistedWispEngineConfig {
            core_path: Some(root.join("wisp/core")),
            audio_output_channels: Some(2),
            audio_sample_rate: Some(44100),
            audio_buffer_size: Some(256),
            ..Default::default()
        };
        let mut runtime = live_factory()(&config).unwrap();
        let text = "[flow]\nname='native_array'\n[[flow.nodes]]\ntext='buffer'\n[[flow.nodes]]\ntext='control'\n[[flow.nodes]]\ntext='read'\n[[flow.nodes]]\ntext='watch'\n[[flow.nodes]]\ntext='out'\n[[flow.edges]]\nfrom=0\nto=2\noutput_index=0\ninput_index=0\n[[flow.edges]]\nfrom=1\nto=2\noutput_index=0\ninput_index=1\n[[flow.edges]]\nfrom=2\nto=3\noutput_index=0\ninput_index=0\n[[flow.edges]]\nfrom=2\nto=4\noutput_index=0\ninput_index=0\n";
        runtime.prepare(text).unwrap();
        runtime
            .buffers(&[super::super::resources::BufferBinding {
                call: 0,
                audio: std::sync::Arc::new(wisp::buffer::DecodedAudio::builtin("sine").unwrap()),
            }])
            .unwrap();
        runtime.install_prepared().unwrap();
        runtime.scalar("native_array", 1, 256.).unwrap();
        let watch = runtime.watch("native_array", 3, true).unwrap();
        runtime.start().unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            let values = runtime.query().unwrap();
            if values
                .values
                .get(&watch)
                .and_then(|v| v.last())
                .is_some_and(|v| (*v - 1.).abs() < 1e-6)
            {
                break;
            }
            assert!(std::time::Instant::now() < end);
        }
        runtime.stop().unwrap();
        assert!(runtime.healthy());
    }
}
