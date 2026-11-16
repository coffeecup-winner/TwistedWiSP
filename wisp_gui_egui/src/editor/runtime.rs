//! Runtime implementations stay on the editor thread; tests inject a device-free fake.
use wisp::{CallIndex, DataIndex, TwistedWispEngine, TwistedWispEngineConfig};
pub trait EditorRuntime {
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
}
