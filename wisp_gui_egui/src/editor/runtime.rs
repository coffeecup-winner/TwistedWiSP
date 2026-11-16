//! Runtime implementations stay on the editor thread; tests inject a device-free fake.
use wisp::{CallIndex, DataIndex, TwistedWispEngine, TwistedWispEngineConfig};
pub trait EditorRuntime {
    fn configuration(&self) -> (u32, u32);
    fn install(&mut self, flow: &str) -> Result<(), String>;
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
            .map(|engine| Box::new(engine) as Box<dyn EditorRuntime>)
            .map_err(|e| e.to_string())
    })
}
impl EditorRuntime for TwistedWispEngine {
    fn configuration(&self) -> (u32, u32) {
        self.audio_configuration()
    }
    fn install(&mut self, text: &str) -> Result<(), String> {
        self.runtime_stop_checked()?;
        let name = self
            .ctx_load_flow_from_text(text)
            .map_err(|e| e.to_string())?;
        let processor = self
            .runtime_compile_signal_processor(name)
            .map_err(|e| e.to_string())?;
        self.runtime_install_checked(processor)
    }
    fn scalar(&mut self, root: &str, call: u32, value: f32) -> Result<(), String> {
        self.runtime_scalar_checked(root, CallIndex(call), DataIndex(0), value)
    }
    fn start(&mut self) -> Result<(), String> {
        self.runtime_start_checked()
    }
    fn stop(&mut self) -> Result<(), String> {
        self.runtime_stop_checked()
    }
    fn healthy(&self) -> bool {
        self.runtime_healthy()
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
        assert!(runtime.healthy());
        drop(runtime);
    }
}
